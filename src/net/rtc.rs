//! Peer-to-peer message endpoints over WebRTC data channels (feature
//! `webrtc`): browser to browser, browser to desktop and desktop to desktop,
//! through home routers (NATs) with STUN.
//!
//! A [`NetMessages`] endpoint made with [`NetMessages::host_rtc`] or
//! [`NetMessages::join_rtc`] behaves like any other: the same lanes, events,
//! limits and handshake. Only the packets travel differently: each client
//! has a WebRTC connection to its host, and every packet of the message
//! protocol is one message on an unordered data channel that never
//! retransmits, so it behaves like UDP. The protocol on top makes the
//! [`Lane::Reliable`](super::Lane::Reliable) and
//! [`Lane::ReliableUnordered`](super::Lane::ReliableUnordered) lanes
//! reliable and leaves [`Lane::Unreliable`](super::Lane::Unreliable) as it
//! is, exactly as over UDP: a lost unreliable message never delays the next.
//!
//! Before two peers can connect they must exchange a few short messages (an
//! offer, an answer and their addresses): **signaling**. A [`Signaling`]
//! handle holds those messages; any channel the game has can carry them.
//! With feature `lobby`, [`LobbyClient::signaling`](super::lobby::LobbyClient::signaling)
//! gives a handle that the lobby server's relay carries for the room's
//! members, so the lobby is the signaling server:
//!
//! ```
//! use ponko::net::NetMessages;
//! use ponko::net::rtc::{RtcConfig, Signaling};
//!
//! # fn run() -> std::io::Result<()> {
//! // Any signaling channel: here two handles a test wires together.
//! let (host_side, client_side) = (Signaling::new(), Signaling::new());
//! let config = RtcConfig::new().stun("stun:stun.example.com:3478");
//! let mut host = NetMessages::host_rtc(host_side.clone(), config.clone(), 4)?;
//! // The client knows the host as signaling peer 1; the host will know it as 2.
//! let mut client = NetMessages::join_rtc(client_side.clone(), 1, config)?;
//! // Every frame: poll both, and carry the signaling messages over.
//! host.poll()?;
//! client.poll()?;
//! while let Some((_to, bytes)) = client_side.next_outgoing() {
//!     host_side.deliver(2, &bytes);
//! }
//! while let Some((_to, bytes)) = host_side.next_outgoing() {
//!     client_side.deliver(1, &bytes);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`RtcConfig`] names the STUN servers peers ask for their public address,
//! and the TURN servers that relay for peers that can't reach each other
//! at all. Without any, peers connect on the same network or when one of
//! them has a public address.
//!
//! On the desktop the connections are str0m's, polled from the game thread
//! on one UDP socket (no async runtime, no threads); in a browser they are
//! the browser's own `RTCPeerConnection`. The guide's "Peer to peer over
//! WebRTC" section says what was checked where, and how to deploy.

#![deny(missing_docs)]

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::NetMessages;
use super::messages::PacketTransport;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use super::rtc_native::Agent;

#[cfg(target_arch = "wasm32")]
pub(super) use super::rtc_browser::Agent;

/// Every signaling message starts with these bytes, so a channel that
/// carries other traffic too can tell them apart ([`Signaling::is_signal`]).
const MAGIC: &[u8; 5] = b"PRTC\x01";
/// Signaling messages kept per direction; more are dropped.
const QUEUE: usize = 1024;
/// The largest signaling message read: far above an offer (about 1 KiB).
pub(super) const MAX_SIGNAL: usize = 16 * 1024;
/// A connection that isn't open this long after it started is given up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// An open connection with no packet for this long is closed (the message
/// protocol sends a heartbeat every 100 ms, and times out sooner itself).
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Connections being set up beyond `max_clients` a host allows at once.
const SPARE_CONNECTIONS: usize = 8;
/// The data channel's label.
pub(super) const CHANNEL_LABEL: &str = "ponko";

/// A STUN or TURN server for [`RtcConfig`].
#[derive(Clone, Debug, PartialEq, Eq)]
struct IceServer {
    url: String,
    username: Option<String>,
    credential: Option<String>,
}

/// How WebRTC peers find each other: STUN and TURN servers, and on the
/// desktop the UDP socket the connections use.
///
/// ```
/// use ponko::net::rtc::RtcConfig;
/// let config = RtcConfig::new()
///     .stun("stun:stun.example.com:3478")
///     .bind("0.0.0.0:7800".parse().unwrap());
/// assert_eq!(config.stun_urls().count(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtcConfig {
    servers: Vec<IceServer>,
    pub(super) bind: SocketAddr,
    pub(super) host_addrs: Vec<IpAddr>,
    pub(super) relay_only: bool,
}

impl Default for RtcConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl RtcConfig {
    /// No STUN or TURN servers: peers connect on the same network, or when
    /// one of them has a public address. On the desktop, a UDP socket on
    /// any port of every interface (`0.0.0.0:0`).
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new();
    /// assert_eq!(config.stun_urls().count(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            servers: Vec::new(),
            bind: SocketAddr::from(([0, 0, 0, 0], 0)),
            host_addrs: Vec::new(),
            relay_only: false,
        }
    }

    /// Adds a STUN server, as a `stun:host` or `stun:host:port` URL (port
    /// 3478 when left out): peers ask it which public address their packets
    /// come from, so peers behind most home routers can reach each other.
    /// The lobby server can be one (`LOBBY_STUN`). On the desktop the host
    /// name is looked up when the endpoint is made.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().stun("stun:192.0.2.10");
    /// ```
    pub fn stun(mut self, url: &str) -> Self {
        self.servers.push(IceServer {
            url: url.to_string(),
            username: None,
            credential: None,
        });
        self
    }

    /// Adds a TURN server (`turn:host:port`, port 3478 when left out) with
    /// its credentials: it relays the traffic of peers that can't reach
    /// each other at all (both behind strict NATs). Peers still connect
    /// directly when they can; the relayed path is the last one tried.
    ///
    /// The desktop speaks TURN over UDP with long-term credentials (what
    /// coturn and most services use); its host name is looked up when the
    /// endpoint is made. `turns:` (TLS) and `?transport=tcp` servers are
    /// used by browsers only; the desktop skips them.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().turn("turn:192.0.2.10:3478", "user", "secret");
    /// assert_eq!(config.stun_urls().count(), 0);
    /// ```
    pub fn turn(mut self, url: &str, username: &str, credential: &str) -> Self {
        self.servers.push(IceServer {
            url: url.to_string(),
            username: Some(username.to_string()),
            credential: Some(credential.to_string()),
        });
        self
    }

    /// On the desktop, connects through the TURN servers only, never
    /// directly: no host or STUN addresses are offered, so the other side
    /// never learns this machine's address (what a browser's
    /// `iceTransportPolicy: "relay"` does). Without a TURN server nothing
    /// connects. Ignored in a browser for now.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new()
    ///     .turn("turn:192.0.2.10:3478", "user", "secret")
    ///     .relay_only();
    /// ```
    #[must_use]
    pub fn relay_only(mut self) -> Self {
        self.relay_only = true;
        self
    }

    /// The desktop UDP socket's address (default `0.0.0.0:0`: every
    /// interface, any port). A fixed port is easier to forward on a router.
    /// Ignored in a browser.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().bind("0.0.0.0:7800".parse().unwrap());
    /// ```
    pub fn bind(mut self, addr: SocketAddr) -> Self {
        self.bind = addr;
        self
    }

    /// On the desktop, offers `ip` (with the socket's port) to peers as
    /// this machine's own address. By default the socket's address, or for
    /// a socket on every interface the address of the interface that
    /// reaches the internet (found without sending anything). Give the
    /// machine's public address here for a host with one (a server with
    /// port forwarding). Ignored in a browser.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().host_address("192.168.1.20".parse().unwrap());
    /// ```
    pub fn host_address(mut self, ip: IpAddr) -> Self {
        self.host_addrs.push(ip);
        self
    }

    /// The STUN server URLs, in the order given.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().stun("stun:a").stun("stun:b");
    /// assert_eq!(config.stun_urls().collect::<Vec<_>>(), ["stun:a", "stun:b"]);
    /// ```
    pub fn stun_urls(&self) -> impl Iterator<Item = &str> {
        self.servers
            .iter()
            .filter(|s| s.username.is_none())
            .map(|s| s.url.as_str())
    }

    /// Every TURN server as `(url, username, credential)`, in the order given.
    /// ```
    /// let config = ponko::net::rtc::RtcConfig::new().stun("stun:a").turn("turn:b", "u", "p");
    /// assert_eq!(config.turn_servers().collect::<Vec<_>>(), [("turn:b", "u", "p")]);
    /// ```
    pub fn turn_servers(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.servers.iter().filter_map(|s| match (&s.username, &s.credential) {
            (Some(user), Some(credential)) => Some((s.url.as_str(), user.as_str(), credential.as_str())),
            _ => None,
        })
    }

    /// Every server as `(url, username, credential)`, for the browser.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub(super) fn servers(&self) -> impl Iterator<Item = (&str, Option<&str>, Option<&str>)> {
        self.servers
            .iter()
            .map(|s| (s.url.as_str(), s.username.as_deref(), s.credential.as_deref()))
    }
}

/// One signaling message's content, between the two ends of a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(super) enum Signal {
    /// The joining side's session description.
    Offer { sdp: String },
    /// The host's reply to an offer.
    Answer { sdp: String },
    /// One more address to try (trickle ICE), as an SDP `candidate:` line.
    Candidate { candidate: String },
    /// The sender closed this connection.
    Bye,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    /// The joining side's session: a random number per connection attempt.
    s: u64,
    #[serde(flatten)]
    signal: Signal,
}

fn encode(session: u64, signal: Signal) -> Vec<u8> {
    let mut bytes = MAGIC.to_vec();
    // Serializing plain strings and numbers can't fail.
    serde_json::to_writer(&mut bytes, &Envelope { s: session, signal }).unwrap_or_default();
    bytes
}

fn decode(bytes: &[u8]) -> Option<(u64, Signal)> {
    let json = bytes.strip_prefix(MAGIC.as_slice())?;
    if bytes.len() > MAX_SIGNAL {
        return None;
    }
    let envelope: Envelope = serde_json::from_slice(json).ok()?;
    Some((envelope.s, envelope.signal))
}

#[derive(Default)]
struct Queues {
    incoming: VecDeque<(u64, Vec<u8>)>,
    outgoing: VecDeque<(u64, Vec<u8>)>,
}

/// The signaling messages of WebRTC endpoints, waiting to be carried: the
/// endpoints put what they want sent into it, the game carries those to the
/// other side over any channel it has and hands what arrives back with
/// [`deliver`](Self::deliver).
///
/// Peers are numbered by the signaling channel: a lobby's member ids, a
/// server's connection ids. A clone is the same handle (one endpoint's
/// handle may be shared by several endpoints of the same game). With the
/// lobby, [`LobbyClient::signaling`](super::lobby::LobbyClient::signaling)
/// carries the messages itself.
///
/// The messages are short JSON (an offer is about 1 KiB) behind a fixed
/// prefix, and must arrive in order, each once: carry them on a reliable,
/// ordered channel. They hold network addresses, not secrets.
///
/// ```
/// use ponko::net::rtc::Signaling;
/// let signaling = Signaling::new();
/// assert!(signaling.next_outgoing().is_none());
/// assert!(!signaling.deliver(2, b"a game message")); // not a signal: not taken
/// ```
#[derive(Clone, Default)]
pub struct Signaling(Arc<Mutex<Queues>>);

impl std::fmt::Debug for Signaling {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let q = self.lock();
        f.debug_struct("Signaling")
            .field("incoming", &q.incoming.len())
            .field("outgoing", &q.outgoing.len())
            .finish()
    }
}

impl Signaling {
    /// An empty handle.
    /// ```
    /// let signaling = ponko::net::rtc::Signaling::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queues> {
        // The queues stay consistent through a panic elsewhere: plain pushes and pops.
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether `bytes` is a signaling message (by its prefix), for a channel
    /// that carries other messages too.
    /// ```
    /// assert!(!ponko::net::rtc::Signaling::is_signal(b"hello"));
    /// ```
    pub fn is_signal(bytes: &[u8]) -> bool {
        bytes.starts_with(MAGIC)
    }

    /// Hands a message that arrived from signaling peer `from` to the
    /// endpoints. Returns whether it was a signaling message (and taken);
    /// anything else is left to the caller. Past 1024 waiting messages
    /// (endpoints that are never polled), more are dropped.
    /// ```
    /// let signaling = ponko::net::rtc::Signaling::new();
    /// assert!(!signaling.deliver(1, b"not a signal"));
    /// ```
    pub fn deliver(&self, from: u64, bytes: &[u8]) -> bool {
        if !Self::is_signal(bytes) {
            return false;
        }
        let mut q = self.lock();
        if q.incoming.len() < QUEUE && bytes.len() <= MAX_SIGNAL {
            q.incoming.push_back((from, bytes.to_vec()));
        }
        true
    }

    /// The oldest message an endpoint wants sent, and the signaling peer it
    /// goes to. Send them in this order, reliably.
    /// ```
    /// assert!(ponko::net::rtc::Signaling::new().next_outgoing().is_none());
    /// ```
    pub fn next_outgoing(&self) -> Option<(u64, Vec<u8>)> {
        self.lock().outgoing.pop_front()
    }

    /// Puts a message that couldn't be sent yet back in front.
    #[cfg(feature = "lobby")]
    pub(super) fn unsend(&self, to: u64, bytes: Vec<u8>) {
        self.lock().outgoing.push_front((to, bytes));
    }

    fn send(&self, to: u64, bytes: Vec<u8>) {
        let mut q = self.lock();
        if q.outgoing.len() < QUEUE {
            q.outgoing.push_back((to, bytes));
        }
    }

    fn take_incoming(&self) -> Option<(u64, Vec<u8>)> {
        self.lock().incoming.pop_front()
    }
}

/// The session address of connection `conn`, in a private IPv6 range
/// (`fd77:7765:6272:7463::/64`, "wwebrtc"), as the WebSocket transport does.
fn conn_addr(conn: u64) -> SocketAddr {
    let ip = Ipv6Addr::new(
        0xfd77,
        0x7765,
        0x6272,
        0x7463,
        (conn >> 48) as u16,
        (conn >> 32) as u16,
        (conn >> 16) as u16,
        conn as u16,
    );
    SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0))
}

fn conn_of(addr: SocketAddr) -> Option<u64> {
    let SocketAddr::V6(v6) = addr else { return None };
    let s = v6.ip().segments();
    if s[..4] != [0xfd77, 0x7765, 0x6272, 0x7463] {
        return None;
    }
    Some(u64::from(s[4]) << 48 | u64::from(s[5]) << 32 | u64::from(s[6]) << 16 | u64::from(s[7]))
}

/// One connection, as the transport sees it.
struct Conn {
    /// The signaling peer at the other end.
    peer: u64,
    /// The joining side's session number.
    session: u64,
    started: Duration,
    last_heard: Duration,
}

/// The packet transport under a WebRTC message endpoint: signaling in and
/// out, connections by their session addresses, stale ones closed.
pub(super) struct RtcTransport {
    signaling: Signaling,
    agent: Agent,
    /// A client's host (its signaling peer); `None` on a host.
    host: Option<u64>,
    conns: BTreeMap<u64, Conn>,
    max_conns: usize,
    next_conn: u64,
    now: Duration,
}

impl RtcTransport {
    pub(super) fn host(signaling: Signaling, agent: Agent, max_clients: usize) -> Self {
        Self {
            signaling,
            agent,
            host: None,
            conns: BTreeMap::new(),
            max_conns: max_clients + SPARE_CONNECTIONS,
            next_conn: 1,
            now: Duration::ZERO,
        }
    }

    /// A client's transport, with its one connection (`conn_addr(1)`)
    /// started.
    pub(super) fn join(signaling: Signaling, mut agent: Agent, host: u64) -> Self {
        let session = super::messages::random_u64();
        agent.connect(1);
        let mut conns = BTreeMap::new();
        conns.insert(
            1,
            Conn {
                peer: host,
                session,
                started: Duration::ZERO,
                last_heard: Duration::ZERO,
            },
        );
        let mut transport = Self {
            signaling,
            agent,
            host: Some(host),
            conns,
            max_conns: 1,
            next_conn: 2,
            now: Duration::ZERO,
        };
        transport.flush_signals();
        transport
    }

    fn handle_signal(&mut self, from: u64, session: u64, signal: Signal) {
        let known = self
            .conns
            .iter()
            .find(|(_, c)| c.peer == from && c.session == session)
            .map(|(&id, _)| id);
        match (known, signal) {
            (Some(conn), Signal::Bye) => self.drop_conn(conn, false),
            (Some(conn), signal @ (Signal::Answer { .. } | Signal::Candidate { .. })) => {
                self.agent.signal(conn, signal);
            }
            (None, Signal::Offer { sdp }) if self.host.is_none() => {
                // A newer attempt from the same peer replaces its old one.
                let old: Vec<u64> = self
                    .conns
                    .iter()
                    .filter(|(_, c)| c.peer == from)
                    .map(|(&id, _)| id)
                    .collect();
                for conn in old {
                    self.drop_conn(conn, true);
                }
                if self.conns.len() >= self.max_conns {
                    return; // The client gives up after its timeout.
                }
                let conn = self.next_conn;
                self.next_conn += 1;
                self.conns.insert(
                    conn,
                    Conn {
                        peer: from,
                        session,
                        started: self.now,
                        last_heard: self.now,
                    },
                );
                self.agent.signal(conn, Signal::Offer { sdp });
            }
            // Signals for connections that ended, or from strangers.
            _ => {}
        }
    }

    fn drop_conn(&mut self, conn: u64, tell: bool) {
        if let Some(c) = self.conns.remove(&conn) {
            self.agent.close(conn);
            if tell {
                self.signaling.send(c.peer, encode(c.session, Signal::Bye));
            }
        }
    }

    fn flush_signals(&mut self) {
        while let Some((conn, signal)) = self.agent.take_signal() {
            if let Some(c) = self.conns.get(&conn) {
                self.signaling.send(c.peer, encode(c.session, signal));
            }
        }
    }
}

impl PacketTransport for RtcTransport {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        let Some(conn) = conn_of(to).filter(|c| self.conns.contains_key(c)) else {
            return Ok(false);
        };
        let sent = self.agent.send(conn, bytes);
        self.flush_signals();
        Ok(sent)
    }

    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        while let Some((conn, n)) = self.agent.recv(buf) {
            if let Some(c) = self.conns.get_mut(&conn) {
                c.last_heard = self.now;
                return Ok(Some((conn_addr(conn), n)));
            }
        }
        Ok(None)
    }

    fn advance(&mut self, now: Duration) {
        self.now = now;
        while let Some((from, bytes)) = self.signaling.take_incoming() {
            let Some((session, signal)) = decode(&bytes) else {
                continue;
            };
            if self.host.is_some_and(|host| host != from) {
                continue;
            }
            self.handle_signal(from, session, signal);
        }
        self.agent.advance(now);
        self.flush_signals();
        let stale: Vec<u64> = self
            .conns
            .iter()
            .filter(|&(&id, c)| {
                let open = self.agent.is_open(id);
                self.agent.is_closed(id)
                    || (!open && now.saturating_sub(c.started) >= CONNECT_TIMEOUT)
                    || (open && now.saturating_sub(c.last_heard) >= IDLE_TIMEOUT)
            })
            .map(|(&id, _)| id)
            .collect();
        for conn in stale {
            self.drop_conn(conn, true);
        }
    }

    fn signal_peer(&self, addr: SocketAddr) -> Option<u64> {
        conn_of(addr).and_then(|conn| self.conns.get(&conn)).map(|c| c.peer)
    }
}

impl Drop for RtcTransport {
    fn drop(&mut self) {
        // Tell the other ends at once, rather than after their timeout (the
        // game's signaling channel carries it on its next poll).
        for c in self.conns.values() {
            self.signaling.send(c.peer, encode(c.session, Signal::Bye));
        }
    }
}

impl NetMessages {
    /// A host that takes clients over WebRTC (feature `webrtc`): browser and
    /// desktop clients that join with [`join_rtc`](Self::join_rtc), up to
    /// `max_clients` (1..=1024), through the signaling handle's channel.
    /// Then it is like any host: poll it every frame, read its events, send
    /// on any lane. Poll the signaling channel every frame too (a
    /// [`LobbyClient`](super::lobby::LobbyClient) does it in its own poll).
    ///
    /// A browser can host too: peer to peer between browsers, no server
    /// but the signaling one.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `max_clients` outside 1..=1024 or a STUN URL that
    /// can't be read; on the desktop, the error of binding the UDP socket.
    ///
    /// ```
    /// use ponko::net::{NetMessages, rtc::{RtcConfig, Signaling}};
    /// let host = NetMessages::host_rtc(Signaling::new(), RtcConfig::new().bind("127.0.0.1:0".parse().unwrap()), 4).unwrap();
    /// ```
    pub fn host_rtc(signaling: Signaling, config: RtcConfig, max_clients: usize) -> io::Result<Self> {
        if !(1..=1024).contains(&max_clients) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "max_clients must be 1..=1024",
            ));
        }
        let agent = Agent::new(&config)?;
        Ok(Self::rtc_host_with(signaling, agent, max_clients))
    }

    /// Starts joining the host that signaling peer `host` runs (its lobby
    /// member id, with the lobby) over WebRTC (feature `webrtc`). Poll until
    /// `Connected` or `Disconnected`, as with [`join`](Self::join): setting
    /// up takes a few round trips through the signaling channel, then the
    /// connection is direct between the two peers.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a STUN URL that can't be read; on the desktop, the
    /// error of binding the UDP socket.
    ///
    /// ```
    /// use ponko::net::{NetMessages, rtc::{RtcConfig, Signaling}};
    /// let signaling = Signaling::new();
    /// let client = NetMessages::join_rtc(signaling.clone(), 1, RtcConfig::new().bind("127.0.0.1:0".parse().unwrap())).unwrap();
    /// assert!(signaling.next_outgoing().is_some(), "the offer, for peer 1");
    /// ```
    pub fn join_rtc(signaling: Signaling, host: u64, config: RtcConfig) -> io::Result<Self> {
        let agent = Agent::new(&config)?;
        Ok(Self::rtc_join_with(signaling, agent, host))
    }

    pub(super) fn rtc_host_with(signaling: Signaling, agent: Agent, max_clients: usize) -> Self {
        Self::transported(
            Box::new(RtcTransport::host(signaling, agent, max_clients)),
            None,
            max_clients,
        )
    }

    pub(super) fn rtc_join_with(signaling: Signaling, agent: Agent, host: u64) -> Self {
        Self::transported(
            Box::new(RtcTransport::join(signaling, agent, host)),
            Some(conn_addr(1)),
            1,
        )
    }

    /// The signaling peer behind a connected peer of a WebRTC endpoint: for
    /// a host, the client's lobby member id (with the lobby); for a client,
    /// its host's. `None` for any other endpoint or an unknown peer.
    /// ```
    /// assert!(ponko::net::NetMessages::loopback(2)[0].rtc_signal_peer(ponko::net::PeerId::HOST).is_none());
    /// ```
    pub fn rtc_signal_peer(&self, peer: super::PeerId) -> Option<u64> {
        self.transport_signal_peer(peer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_round_trip_and_foreign_bytes_are_left_alone() {
        let bytes = encode(
            7,
            Signal::Candidate {
                candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".into(),
            },
        );
        assert!(Signaling::is_signal(&bytes));
        assert_eq!(
            decode(&bytes),
            Some((
                7,
                Signal::Candidate {
                    candidate: "candidate:1 1 udp 1 127.0.0.1 9 typ host".into()
                }
            ))
        );
        assert_eq!(decode(b"PRTC\x01{not json"), None);
        assert_eq!(decode(b"chat: hello"), None);
        let signaling = Signaling::new();
        assert!(!signaling.deliver(3, b"chat: hello"));
        assert!(signaling.deliver(3, &bytes));
        assert_eq!(signaling.take_incoming(), Some((3, bytes)));
    }

    #[test]
    fn session_addresses_round_trip_and_stay_apart_from_other_transports() {
        for conn in [1, 2, 0xffff_ffff_ffff] {
            assert_eq!(conn_of(conn_addr(conn)), Some(conn));
        }
        assert_eq!(conn_of(super::super::websocket::HOST), None);
        assert_eq!(conn_of("127.0.0.1:9".parse().unwrap()), None);
    }

    #[test]
    fn signaling_queues_are_bounded() {
        let signaling = Signaling::new();
        let bytes = encode(1, Signal::Bye);
        for _ in 0..QUEUE + 10 {
            signaling.deliver(1, &bytes);
            signaling.send(1, bytes.clone());
        }
        let q = signaling.lock();
        assert_eq!((q.incoming.len(), q.outgoing.len()), (QUEUE, QUEUE));
    }
}
