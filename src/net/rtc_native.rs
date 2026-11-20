//! The desktop side of the WebRTC transport (`rtc.rs`): str0m connections,
//! all on one nonblocking UDP socket, polled from the game thread.
//!
//! str0m is sans-IO: it never reads a clock or a socket itself. This agent
//! feeds it the packets the socket received and the protocol time of
//! `NetMessages::poll_elapsed` (as instants after the agent's start), and
//! sends what it asks to. So the same code runs over a real socket and, in
//! the tests, over an in-memory network on a simulated clock.
//!
//! NAT traversal: the socket's own address is the host candidate; a STUN
//! binding request to each configured server finds the address a NAT maps
//! the socket to, offered as a server-reflexive candidate. ICE then tries
//! every pair, which punches holes through both NATs when they map a socket
//! to the same address for every destination (most home routers). There is
//! no TURN client: two peers behind strict (symmetric) NATs can't connect.

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Arc;
use std::time::{Duration, Instant};

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelConfig, ChannelId, Reliability};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

use super::rtc::{CHANNEL_LABEL, RtcConfig, Signal};
use super::stun;

/// Datagrams one advance reads at most.
const READS: usize = 1024;
/// Channel messages waiting for the session at most; more are dropped
/// (the message protocol resends what matters).
const INBOX: usize = 4096;
/// The largest data channel message passed up: the message protocol's
/// datagrams are at most 1500 bytes.
const MAX_MESSAGE: usize = 1500;
/// STUN requests per server: the first at once, then after 250 ms, 500 ms,
/// 1 s and 2 s.
const STUN_TRIES: u32 = 5;

/// What the agent sends and receives on: the UDP socket, or the tests'
/// in-memory network.
pub(super) trait Socket: Send {
    fn send_to(&mut self, bytes: &[u8], to: SocketAddr) -> io::Result<()>;
    /// `Ok(None)` when nothing is waiting.
    fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>>;
}

impl Socket for UdpSocket {
    fn send_to(&mut self, bytes: &[u8], to: SocketAddr) -> io::Result<()> {
        UdpSocket::send_to(self, bytes, to).map(|_| ())
    }
    fn recv_from(&mut self, buf: &mut [u8]) -> io::Result<Option<(usize, SocketAddr)>> {
        match UdpSocket::recv_from(self, buf) {
            Ok(got) => Ok(Some(got)),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}

struct StunQuery {
    server: SocketAddr,
    transaction: [u8; 12],
    sent: u32,
    next: Duration,
    done: bool,
}

struct Peer {
    rtc: Rtc,
    channel: Option<ChannelId>,
    pending: Option<SdpPendingOffer>,
    open: bool,
    closed: bool,
}

/// Every WebRTC connection of one endpoint.
pub(in crate::net) struct Agent {
    socket: Box<dyn Socket>,
    /// The host candidate: what the socket's packets come from.
    base: SocketAddr,
    /// Candidates every connection offers: the host one, then reflexive ones.
    local: Vec<Candidate>,
    stun: Vec<StunQuery>,
    peers: BTreeMap<u64, Peer>,
    provider: Arc<str0m::crypto::CryptoProvider>,
    cert: str0m::config::DtlsCert,
    epoch: Instant,
    now: Duration,
    signals: VecDeque<(u64, Signal)>,
    inbox: VecDeque<(u64, Vec<u8>)>,
}

impl Agent {
    /// Binds the UDP socket of `config` and resolves its STUN servers.
    pub(in crate::net) fn new(config: &RtcConfig) -> io::Result<Self> {
        let socket = UdpSocket::bind(config.bind)?;
        socket.set_nonblocking(true)?;
        let local = socket.local_addr()?;
        let ip = match config.host_addrs.first() {
            Some(&ip) => ip,
            None if local.ip().is_unspecified() => primary_ip(local.is_ipv4()),
            None => local.ip(),
        };
        let mut servers = Vec::new();
        for url in config.stun_urls() {
            servers.extend(resolve_stun(url, ip.is_ipv4())?);
        }
        Self::with_socket(Box::new(socket), SocketAddr::new(ip, local.port()), &servers)
    }

    /// An agent on `socket`, whose packets come from `base`, asking the STUN
    /// servers at `stun_servers`.
    pub(super) fn with_socket(
        socket: Box<dyn Socket>,
        base: SocketAddr,
        stun_servers: &[SocketAddr],
    ) -> io::Result<Self> {
        let provider = str0m::crypto::from_feature_flags();
        let cert = provider
            .dtls_provider
            .generate_certificate()
            .ok_or_else(|| io::Error::other("could not make a DTLS certificate"))?;
        let host = Candidate::host(base, "udp")
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("{base}: {e}")))?;
        let stun = stun_servers
            .iter()
            .map(|&server| StunQuery {
                server,
                transaction: transaction(),
                sent: 0,
                next: Duration::ZERO,
                done: false,
            })
            .collect();
        Ok(Self {
            socket,
            base,
            local: vec![host],
            stun,
            peers: BTreeMap::new(),
            provider: Arc::new(provider),
            cert,
            epoch: Instant::now(),
            now: Duration::ZERO,
            signals: VecDeque::new(),
            inbox: VecDeque::new(),
        })
    }

    fn instant(&self) -> Instant {
        self.epoch + self.now
    }

    /// A connection with every local candidate added.
    fn new_peer(&mut self) -> Peer {
        let rtc = Rtc::builder()
            .clear_codecs()
            .set_crypto_provider(self.provider.clone())
            .set_dtls_cert(self.cert.clone())
            .build(self.instant());
        let mut peer = Peer {
            rtc,
            channel: None,
            pending: None,
            open: false,
            closed: false,
        };
        for candidate in self.local.clone() {
            peer.rtc.add_local_candidate(candidate);
            self.drain(0, &mut peer);
        }
        peer
    }

    /// Starts connection `conn` from this side: the offer, with the data
    /// channel, goes out as a signal.
    pub(in crate::net) fn connect(&mut self, conn: u64) {
        let mut peer = self.new_peer();
        let mut api = peer.rtc.sdp_api();
        api.add_channel_with_config(ChannelConfig {
            label: CHANNEL_LABEL.into(),
            ordered: false,
            reliability: Reliability::MaxRetransmits { retransmits: 0 },
            negotiated: None,
            protocol: String::new(),
        });
        match api.apply() {
            Some((offer, pending)) => {
                peer.pending = Some(pending);
                self.signals.push_back((
                    conn,
                    Signal::Offer {
                        sdp: offer.to_sdp_string(),
                    },
                ));
            }
            None => peer.closed = true,
        }
        self.drain(conn, &mut peer);
        self.peers.insert(conn, peer);
    }

    /// A signal from the other end of `conn`; an offer starts it.
    pub(in crate::net) fn signal(&mut self, conn: u64, signal: Signal) {
        if let Signal::Offer { sdp } = &signal {
            let mut peer = self.new_peer();
            let answer = SdpOffer::from_sdp_string(sdp)
                .ok()
                .and_then(|offer| peer.rtc.sdp_api().accept_offer(offer).ok());
            match answer {
                Some(answer) => self.signals.push_back((
                    conn,
                    Signal::Answer {
                        sdp: answer.to_sdp_string(),
                    },
                )),
                None => peer.closed = true,
            }
            self.drain(conn, &mut peer);
            self.peers.insert(conn, peer);
            return;
        }
        let Some(mut peer) = self.peers.remove(&conn) else {
            return;
        };
        match signal {
            Signal::Answer { sdp } => {
                let accepted = match (peer.pending.take(), SdpAnswer::from_sdp_string(&sdp)) {
                    (Some(pending), Ok(answer)) => peer.rtc.sdp_api().accept_answer(pending, answer).is_ok(),
                    _ => false,
                };
                if !accepted {
                    peer.closed = true;
                }
            }
            Signal::Candidate { candidate } => {
                // A browser's `.local` (mDNS) host candidates can't be
                // resolved here and fail to parse: the browser's checks
                // still reach us, and ICE learns its address from them.
                let line = candidate.strip_prefix("a=").unwrap_or(&candidate);
                if let Ok(c) = Candidate::from_sdp_string(line) {
                    peer.rtc.add_remote_candidate(c);
                }
            }
            Signal::Offer { .. } | Signal::Bye => {}
        }
        self.drain(conn, &mut peer);
        self.peers.insert(conn, peer);
    }

    /// Reads the socket, runs every connection's timers up to `now`, and
    /// asks the STUN servers.
    pub(in crate::net) fn advance(&mut self, now: Duration) {
        self.now = self.now.max(now);
        self.ask_stun();
        let mut buf = [0u8; 2048];
        for _ in 0..READS {
            let Ok(Some((n, from))) = self.socket.recv_from(&mut buf) else {
                break;
            };
            self.receive(from, &buf[..n]);
        }
        let at = self.instant();
        let ids: Vec<u64> = self.peers.keys().copied().collect();
        for conn in ids {
            let Some(mut peer) = self.peers.remove(&conn) else {
                continue;
            };
            // Every advance, not only at str0m's deadline: a new connection
            // reports no deadline until its first timeout, and its ICE checks
            // only start then.
            if !peer.closed {
                if peer.rtc.handle_input(Input::Timeout(at)).is_err() {
                    peer.closed = true;
                }
                self.drain(conn, &mut peer);
            }
            self.peers.insert(conn, peer);
        }
    }

    fn ask_stun(&mut self) {
        for i in 0..self.stun.len() {
            let q = &mut self.stun[i];
            if q.done || q.next > self.now {
                continue;
            }
            if q.sent >= STUN_TRIES {
                q.done = true;
                continue;
            }
            q.next = self.now + Duration::from_millis(250 << q.sent);
            q.sent += 1;
            let (server, request) = (q.server, stun::binding_request(q.transaction));
            let _ = self.socket.send_to(&request, server);
        }
    }

    fn receive(&mut self, from: SocketAddr, packet: &[u8]) {
        if stun::is_stun(packet)
            && let Some(q) = self.stun.iter_mut().find(|q| q.server == from && !q.done)
            && let Some(mapped) = stun::mapped_address(packet, q.transaction)
        {
            q.done = true;
            self.reflexive(mapped);
            return;
        }
        let Ok(receive) = Receive::new(Protocol::Udp, from, self.base, packet) else {
            return;
        };
        let input = Input::Receive(self.instant(), receive);
        let Some((&conn, _)) = self.peers.iter().find(|(_, p)| !p.closed && p.rtc.accepts(&input)) else {
            return;
        };
        let Some(mut peer) = self.peers.remove(&conn) else {
            return;
        };
        if peer.rtc.handle_input(input).is_err() {
            peer.closed = true;
        }
        self.drain(conn, &mut peer);
        self.peers.insert(conn, peer);
    }

    /// The NAT maps the socket to `mapped`: offer it on every connection.
    fn reflexive(&mut self, mapped: SocketAddr) {
        if mapped == self.base || self.local.iter().any(|c| c.addr() == mapped) {
            return;
        }
        let Ok(candidate) = Candidate::server_reflexive(mapped, self.base, "udp") else {
            return;
        };
        self.local.push(candidate.clone());
        let ids: Vec<u64> = self.peers.keys().copied().collect();
        for conn in ids {
            let Some(mut peer) = self.peers.remove(&conn) else {
                continue;
            };
            if let Some(added) = peer.rtc.add_local_candidate(candidate.clone()) {
                self.signals.push_back((
                    conn,
                    Signal::Candidate {
                        candidate: added.to_sdp_string(),
                    },
                ));
            }
            self.drain(conn, &mut peer);
            self.peers.insert(conn, peer);
        }
    }

    /// Takes everything str0m has to say after a change: packets to send,
    /// events, the next timeout.
    fn drain(&mut self, conn: u64, peer: &mut Peer) {
        loop {
            match peer.rtc.poll_output() {
                Ok(Output::Timeout(_)) => break,
                Ok(Output::Transmit(t)) => {
                    let _ = self.socket.send_to(&t.contents, t.destination);
                }
                Ok(Output::Event(event)) => match event {
                    Event::ChannelOpen(id, _) => {
                        peer.channel = Some(id);
                        peer.open = true;
                    }
                    Event::ChannelData(data) => {
                        if self.inbox.len() < INBOX && data.data.len() <= MAX_MESSAGE {
                            self.inbox.push_back((conn, data.data));
                        }
                    }
                    Event::ChannelClose(_) | Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
                        peer.closed = true;
                    }
                    _ => {}
                },
                Err(_) => {
                    peer.closed = true;
                    break;
                }
            }
        }
        if !peer.rtc.is_alive() {
            peer.closed = true;
        }
    }

    pub(in crate::net) fn send(&mut self, conn: u64, bytes: &[u8]) -> bool {
        let Some(mut peer) = self.peers.remove(&conn) else {
            return false;
        };
        let sent = match (peer.open && !peer.closed, peer.channel) {
            (true, Some(id)) => peer
                .rtc
                .channel(id)
                .is_some_and(|mut channel| channel.write(true, bytes).unwrap_or(false)),
            _ => false,
        };
        if sent {
            self.drain(conn, &mut peer);
        }
        self.peers.insert(conn, peer);
        sent
    }

    pub(in crate::net) fn recv(&mut self, buf: &mut [u8]) -> Option<(u64, usize)> {
        while let Some((conn, bytes)) = self.inbox.pop_front() {
            if bytes.len() <= buf.len() {
                buf[..bytes.len()].copy_from_slice(&bytes);
                return Some((conn, bytes.len()));
            }
        }
        None
    }

    pub(in crate::net) fn take_signal(&mut self) -> Option<(u64, Signal)> {
        self.signals.pop_front()
    }

    pub(in crate::net) fn is_open(&self, conn: u64) -> bool {
        self.peers.get(&conn).is_some_and(|p| p.open && !p.closed)
    }

    pub(in crate::net) fn is_closed(&self, conn: u64) -> bool {
        self.peers.get(&conn).is_none_or(|p| p.closed)
    }

    pub(in crate::net) fn close(&mut self, conn: u64) {
        if let Some(mut peer) = self.peers.remove(&conn) {
            if peer.rtc.close().is_ok() {
                self.drain(conn, &mut peer);
            }
            self.inbox.retain(|(c, _)| *c != conn);
            self.signals.retain(|(c, _)| *c != conn);
        }
    }
}

/// A fresh random STUN transaction id.
fn transaction() -> [u8; 12] {
    let a = super::messages::random_u64().to_le_bytes();
    let b = super::messages::random_u64().to_le_bytes();
    let mut id = [0u8; 12];
    id[..8].copy_from_slice(&a);
    id[8..].copy_from_slice(&b[..4]);
    id
}

/// The address of the interface that routes to the internet: connecting a
/// UDP socket only picks a route, nothing is sent. Loopback if there is
/// no route.
fn primary_ip(v4: bool) -> IpAddr {
    let (bind, probe): (SocketAddr, SocketAddr) = if v4 {
        (
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            // TEST-NET-1 (RFC 5737): routed like the internet, never used.
            SocketAddr::from((Ipv4Addr::new(192, 0, 2, 1), 9)),
        )
    } else {
        (
            SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
            // The documentation prefix (RFC 3849).
            SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 9)),
        )
    };
    UdpSocket::bind(bind)
        .and_then(|s| s.connect(probe).and_then(|()| s.local_addr()))
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_unspecified())
        .unwrap_or(if v4 {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        })
}

/// The addresses of a `stun:host[:port]` URL in the socket's family; other
/// schemes (TURN) are for browsers and give none.
fn resolve_stun(url: &str, v4: bool) -> io::Result<Vec<SocketAddr>> {
    let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidInput, format!("{url}: {why}"));
    if url.starts_with("turn:") || url.starts_with("turns:") {
        return Ok(Vec::new());
    }
    let rest = url.strip_prefix("stun:").ok_or_else(|| invalid("not a stun: URL"))?;
    let rest = rest.split('?').next().unwrap_or_default();
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (ip, after) = v6.split_once(']').ok_or_else(|| invalid("unclosed IPv6 address"))?;
        (ip, after.strip_prefix(':'))
    } else {
        match rest.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (rest, None),
        }
    };
    if host.is_empty() {
        return Err(invalid("no host"));
    }
    let port = match port {
        Some(p) => p.parse().map_err(|_| invalid("bad port"))?,
        None => stun::DEFAULT_PORT,
    };
    // A name that doesn't resolve (offline) leaves this server out.
    Ok((host, port)
        .to_socket_addrs()
        .map(|addrs| addrs.filter(|a| a.is_ipv4() == v4).collect())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stun_urls_resolve_ip_literals_and_skip_turn() {
        assert_eq!(
            resolve_stun("stun:192.0.2.10", true).unwrap(),
            ["192.0.2.10:3478".parse().unwrap()]
        );
        assert_eq!(
            resolve_stun("stun:[2001:db8::1]:5000", false).unwrap(),
            ["[2001:db8::1]:5000".parse().unwrap()]
        );
        assert!(
            resolve_stun("stun:192.0.2.10", false).unwrap().is_empty(),
            "other family"
        );
        assert!(resolve_stun("turn:192.0.2.10:3478", true).unwrap().is_empty());
        for bad in ["http://x", "stun:", "stun:host:port"] {
            assert_eq!(
                resolve_stun(bad, true).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{bad}"
            );
        }
    }
}
