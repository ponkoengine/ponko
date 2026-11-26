//! The lobby server's relay as a session transport: a lockstep session
//! ([`Net::host_relay`], [`Net::join_relay`]) or a message endpoint, and so
//! replication on top ([`NetMessages::host_relay`], [`NetMessages::join_relay`]),
//! between the members of one room, for players who can't reach each other
//! directly.
//!
//! The session takes the [`LobbyClient`] and polls it itself, from inside
//! the session's own polls: a lockstep game that waits for an input still
//! moves the relay's packets. Each session datagram is one relay frame to
//! one member, on the lobby's unreliable lane, behind a 4-byte prefix
//! ([`MAGIC`]) that keeps it apart from the game's own relayed bytes. The
//! session protocols above make it reliable exactly as over UDP: they
//! resend what the relay loses.
//!
//! Members are session addresses in a private IPv6 range
//! (`fd77:7765:7265:6c79::/64`, "wrely"), next to the WebRTC and WebSocket
//! ranges, so the sessions treat them like any other peer.

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
use std::time::Duration;

use super::super::messages::PacketTransport;
use super::super::{Lane, MAX_PLAYERS, Net, NetMessages, Role, Session, Transport};
use super::{LobbyClient, MemberId};
use crate::perf::Instant;

/// Every session datagram starts with these bytes ("ponko relay session",
/// version 1), so a game's own relayed bytes never reach a session.
pub(super) const MAGIC: &[u8; 4] = b"PRS\x01";
/// The largest relay payload a session sends: its largest datagram (the
/// message protocol's 1500 bytes, more than lockstep's 1400) behind
/// [`MAGIC`]. The server's `max_relay_bytes` must allow it (4096 by default).
pub(super) const LARGEST: usize = MAGIC.len() + 1500;
/// Session datagrams waiting for the session at most; more are dropped (the
/// session protocols resend what matters).
pub(super) const INBOX: usize = 4096;
/// Lobby events kept while a session owns the client and the game doesn't
/// read them; older ones are dropped.
pub(super) const EVENTS: usize = 256;
/// The member range's first four segments.
const PREFIX: [u16; 4] = [0xfd77, 0x7765, 0x7265, 0x6c79];

/// The session datagram in a relay payload, if it is one.
pub(super) fn session_packet(payload: &[u8]) -> Option<&[u8]> {
    payload.strip_prefix(MAGIC.as_slice())
}

/// The session address of `member`.
fn member_addr(member: MemberId) -> SocketAddr {
    let bits = member.to_bits();
    let ip = Ipv6Addr::new(
        PREFIX[0],
        PREFIX[1],
        PREFIX[2],
        PREFIX[3],
        0,
        0,
        (bits >> 16) as u16,
        bits as u16,
    );
    SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, 0))
}

/// The member behind a session address, if it is one.
fn member_of(addr: SocketAddr) -> Option<MemberId> {
    let SocketAddr::V6(v6) = addr else { return None };
    let s = v6.ip().segments();
    if s[..4] != PREFIX || s[4] != 0 || s[5] != 0 {
        return None;
    }
    Some(MemberId::from_bits(u32::from(s[6]) << 16 | u32::from(s[7])))
}

/// A reading of the session's clock.
#[derive(Clone, Copy)]
enum SessionClock {
    /// Lockstep's `now`.
    Wall(Instant),
    /// A message endpoint's protocol time.
    Protocol(Duration),
}

impl SessionClock {
    /// The time from `then` to this reading; zero across kinds or backwards.
    fn since(self, then: Self) -> Duration {
        match (self, then) {
            (Self::Wall(now), Self::Wall(then)) => now.saturating_duration_since(then),
            (Self::Protocol(now), Self::Protocol(then)) => now.saturating_sub(then),
            _ => Duration::ZERO,
        }
    }
}

/// A session's packets through the lobby relay; owns the lobby client.
pub(in crate::net) struct RelayTransport {
    lobby: LobbyClient,
    me: MemberId,
    /// A client's host; `None` on a host.
    host: Option<MemberId>,
    /// Lockstep: the lobby was polled for the current drain of `recv`.
    polled: bool,
    /// The session's clock at the last pump that advanced the lobby:
    /// lockstep's `now` (from `flush`) or the endpoint's protocol time
    /// (from `advance`). The lobby follows it, so a session on a clock of
    /// its own (a scripted test) moves the lobby's timers with it.
    clock: Option<SessionClock>,
    /// A lobby failure (`poll`'s error, a lost connection, leaving the
    /// room), reported to the session once.
    error: Option<String>,
    told: bool,
    /// The framed datagram being sent, reused.
    scratch: Vec<u8>,
}

impl RelayTransport {
    /// A transport over `lobby` (in a room), hosting or joining the room's host.
    fn new(mut lobby: LobbyClient, hosting: bool) -> io::Result<Self> {
        let (Some(me), Some(room)) = (lobby.me(), lobby.room()) else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "a relay session needs a lobby client in a room: wait for LobbyEvent::Joined",
            ));
        };
        let limit = lobby.server_limits().max_relay_bytes as usize;
        if limit < LARGEST {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "the lobby server relays at most {limit} bytes at once; a relay session needs {LARGEST}: \
                     raise the server's max_relay_bytes"
                ),
            ));
        }
        let host = if hosting {
            None
        } else if room.host == me {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "this client hosts its lobby room: host the relay session instead of joining it",
            ));
        } else {
            Some(room.host)
        };
        lobby.session.get_or_insert_with(VecDeque::new);
        Ok(Self {
            lobby,
            me,
            host,
            polled: false,
            clock: None,
            error: None,
            told: false,
            scratch: Vec::new(),
        })
    }

    fn host_addr(&self) -> Option<SocketAddr> {
        self.host.map(member_addr)
    }

    /// Moves the lobby's packets: the relay frames the session queued go
    /// out, relayed ones come in.
    fn pump(&mut self, elapsed: Duration) {
        if let Err(e) = self.lobby.poll_elapsed(elapsed)
            && e.kind() != io::ErrorKind::WouldBlock
        {
            self.error
                .get_or_insert_with(|| format!("the lobby connection failed: {e}"));
        }
        if self.lobby.closed {
            self.error.get_or_insert_with(|| "the lobby connection closed".into());
        } else if self.lobby.room.is_none() {
            self.error
                .get_or_insert_with(|| "this client left its lobby room".into());
        }
    }

    /// The next session datagram (from the host only, on a client).
    fn next(&mut self, buf: &mut [u8]) -> Option<(SocketAddr, usize)> {
        let inbox = self.lobby.session.as_mut()?;
        while let Some((from, bytes)) = inbox.pop_front() {
            if self.host.is_some_and(|host| host != from) || bytes.len() > buf.len() {
                continue;
            }
            buf[..bytes.len()].copy_from_slice(&bytes);
            return Some((member_addr(from), bytes.len()));
        }
        None
    }

    /// Sends one session datagram to member `to` through the relay.
    fn relay(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        let Some(member) = member_of(to) else {
            return Ok(false);
        };
        self.scratch.clear();
        self.scratch.extend_from_slice(MAGIC);
        self.scratch.extend_from_slice(bytes);
        match self.lobby.relay(Some(member), Lane::Unreliable, &self.scratch) {
            Ok(()) => Ok(true),
            // Out of the room (reported by `pump`), or the lane is full: lost.
            Err(e) if matches!(e.kind(), io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Pumps with the time that passed since the last clock reading.
    fn pump_at(&mut self, now: SessionClock) {
        let elapsed = self.clock.map_or(Duration::ZERO, |then| now.since(then));
        self.clock = Some(now);
        self.pump(elapsed);
    }

    fn take_error(&mut self) -> Option<String> {
        if self.told {
            return None;
        }
        let e = self.error.clone()?;
        self.told = true;
        Some(e)
    }
}

impl Drop for RelayTransport {
    fn drop(&mut self) {
        // The session's last packets (a lockstep goodbye) are queued in the
        // lobby's endpoint: send them before the game drops the client.
        if self.error.is_none() {
            self.pump(Duration::ZERO);
        }
    }
}

/// Lockstep: fire-and-forget sends, errors through `error()`.
impl Transport for RelayTransport {
    fn send(&mut self, to: SocketAddr, data: &[u8]) {
        // A failed send is a lost packet; lockstep resends it.
        let _ = self.relay(to, data);
    }

    fn recv(&mut self, buf: &mut [u8]) -> Option<(SocketAddr, usize)> {
        if !self.polled {
            self.polled = true;
            self.pump(Duration::ZERO);
        }
        let packet = self.next(buf);
        if packet.is_none() {
            self.polled = false;
        }
        packet
    }

    fn local_addr(&self) -> SocketAddr {
        member_addr(self.me)
    }

    fn error(&mut self) -> Option<String> {
        self.take_error()
    }

    fn flush(&mut self, now: Instant) {
        self.pump_at(SessionClock::Wall(now));
    }

    fn lobby_mut(&mut self) -> Option<&mut LobbyClient> {
        Some(&mut self.lobby)
    }
}

/// Messages: the endpoint's clock, errors from `poll`.
impl PacketTransport for RelayTransport {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        self.relay(to, bytes)
    }

    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        if let Some(e) = self.take_error() {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, e));
        }
        Ok(self.next(buf))
    }

    fn advance(&mut self, now: Duration) {
        self.pump_at(SessionClock::Protocol(now));
    }

    fn flush(&mut self) {
        self.pump(Duration::ZERO);
    }

    fn relay_lobby(&mut self) -> Option<&mut LobbyClient> {
        Some(&mut self.lobby)
    }
}

impl Net {
    /// Hosts a lockstep session for `players` players (2–8, you included as
    /// player 0) through the lobby server's relay (feature `lobby`), for
    /// room members who can't connect to each other directly. `lobby` must
    /// be in a room ([`LobbyEvent::Joined`](super::LobbyEvent::Joined));
    /// the others join with [`join_relay`](Self::join_relay). Then it is
    /// the session [`host`](Self::host) makes: the same settings, inputs,
    /// checks and timeouts.
    ///
    /// The session owns the lobby client from now on and polls it itself
    /// (also while a tick waits for inputs); read its events through
    /// [`relay_lobby`](Self::relay_lobby). The relay forwards within the
    /// server's relay budget per address (256 KiB a second by default).
    ///
    /// ```
    /// use ponko::net::{Net, NetMessages};
    /// use ponko::net::lobby::LobbyClient;
    /// // A client that is not in a room yet can't carry a session:
    /// let lobby = LobbyClient::new(NetMessages::loopback(2).pop().unwrap());
    /// assert!(Net::host_relay(lobby, 2).is_err());
    /// ```
    ///
    /// # Errors
    ///
    /// `InvalidInput` unless `players` is 2 to 8; `NotConnected` if the
    /// lobby client isn't in a room; `InvalidInput` if the server relays
    /// less than 1504 bytes at once (its `max_relay_bytes`).
    pub fn host_relay(lobby: LobbyClient, players: usize) -> io::Result<Net> {
        if !(2..=MAX_PLAYERS).contains(&players) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Net: expected 2 to 8 players; pass a player count in that range",
            ));
        }
        let transport = RelayTransport::new(lobby, true)?;
        Ok(Self::with(Session::new(Box::new(transport), Role::Host), 0, players))
    }

    /// Joins the lockstep session the host of `lobby`'s room hosts with
    /// [`host_relay`](Self::host_relay), through the lobby server's relay
    /// (feature `lobby`): [`join`](Self::join) for a host you can't reach
    /// directly. Like `join`, it keeps trying until the host answers or
    /// [`timeout`](Self::timeout) passes.
    ///
    /// ```
    /// use ponko::net::{Net, NetMessages};
    /// use ponko::net::lobby::LobbyClient;
    /// let lobby = LobbyClient::new(NetMessages::loopback(2).pop().unwrap());
    /// assert!(Net::join_relay(lobby).is_err(), "not in a room yet");
    /// ```
    ///
    /// # Errors
    ///
    /// `NotConnected` if the lobby client isn't in a room; `InvalidInput`
    /// if it hosts the room (host the session instead) or the server
    /// relays less than 1504 bytes at once.
    pub fn join_relay(lobby: LobbyClient) -> io::Result<Net> {
        let transport = RelayTransport::new(lobby, false)?;
        // A joining transport always has the room's host.
        let host = transport
            .host_addr()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the lobby room has no host"))?;
        Ok(Self::with(Session::new(Box::new(transport), Role::Client(host)), 0, 0))
    }

    /// The lobby client under a relay session ([`host_relay`](Self::host_relay),
    /// [`join_relay`](Self::join_relay)), for its events and state; `None`
    /// for any other session. The session polls it: read its events
    /// with [`LobbyClient::next_event`] (it keeps the newest 256).
    ///
    /// ```
    /// assert!(ponko::net::Net::default().relay_lobby().is_none());
    /// ```
    pub fn relay_lobby(&mut self) -> Option<&mut LobbyClient> {
        self.session.as_deref_mut()?.transport.lobby_mut()
    }
}

impl NetMessages {
    /// A message host for the members of `lobby`'s room, through the lobby
    /// server's relay (feature `lobby`): [`host`](Self::host) for clients
    /// that can't reach this machine directly. Up to `max_clients`
    /// (1..=1024) members join it with [`join_relay`](Self::join_relay);
    /// then it is like any host (lanes, events, timeouts, and replication
    /// on top). `lobby` must be in a room.
    ///
    /// The endpoint owns the lobby client and polls it in its own
    /// [`poll`](Self::poll), on the endpoint's clock (also
    /// [`poll_elapsed`](Self::poll_elapsed)'s); read the lobby's events
    /// through [`relay_lobby`](Self::relay_lobby).
    ///
    /// ```
    /// use ponko::net::NetMessages;
    /// use ponko::net::lobby::LobbyClient;
    /// let lobby = LobbyClient::new(NetMessages::loopback(2).pop().unwrap());
    /// assert!(NetMessages::host_relay(lobby, 4).is_err(), "not in a room yet");
    /// ```
    ///
    /// # Errors
    ///
    /// `InvalidInput` for `max_clients` outside 1..=1024; `NotConnected`
    /// if the lobby client isn't in a room; `InvalidInput` if the server
    /// relays less than 1504 bytes at once (its `max_relay_bytes`).
    pub fn host_relay(lobby: LobbyClient, max_clients: usize) -> io::Result<Self> {
        if !(1..=1024).contains(&max_clients) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "max_clients must be 1..=1024",
            ));
        }
        let transport = RelayTransport::new(lobby, true)?;
        Ok(Self::transported(Box::new(transport), None, max_clients))
    }

    /// Joins the message host that the host of `lobby`'s room runs with
    /// [`host_relay`](Self::host_relay), through the lobby server's relay
    /// (feature `lobby`). Poll until `Connected` or `Disconnected`, as with
    /// [`join`](Self::join).
    ///
    /// ```
    /// use ponko::net::NetMessages;
    /// use ponko::net::lobby::LobbyClient;
    /// let lobby = LobbyClient::new(NetMessages::loopback(2).pop().unwrap());
    /// assert!(NetMessages::join_relay(lobby).is_err(), "not in a room yet");
    /// ```
    ///
    /// # Errors
    ///
    /// `NotConnected` if the lobby client isn't in a room; `InvalidInput`
    /// if it hosts the room (host the endpoint instead) or the server
    /// relays less than 1504 bytes at once.
    pub fn join_relay(lobby: LobbyClient) -> io::Result<Self> {
        let transport = RelayTransport::new(lobby, false)?;
        let host = transport.host_addr();
        Ok(Self::transported(Box::new(transport), host, 1))
    }

    /// The lobby client under a relay endpoint ([`host_relay`](Self::host_relay),
    /// [`join_relay`](Self::join_relay)), for its events and state; `None`
    /// for any other endpoint. The endpoint polls it: read its events with
    /// [`LobbyClient::next_event`] (it keeps the newest 256).
    ///
    /// ```
    /// assert!(ponko::net::NetMessages::loopback(2)[0].relay_lobby().is_none());
    /// ```
    pub fn relay_lobby(&mut self) -> Option<&mut LobbyClient> {
        self.transport_relay_lobby()
    }
}

/// A relay server in memory for the tests here and the transport
/// conformance suites (`net/lockstep_conformance.rs`,
/// `net/message_conformance.rs`), which run their scripted latency and loss
/// tests through it.
#[cfg(test)]
pub(in crate::net) mod testing {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::super::protocol::{self, Frame, Reply, Request, ServerLimits};
    use super::super::{LobbyClient, MemberId, RoomInfo};
    use crate::net::scripted::{Shared, addr, port};
    use crate::net::{Lane, MessageEvent, NetMessages, PeerId};

    /// A lobby server in memory that speaks the real protocol, as much of
    /// it as a relay session needs: it welcomes every client, puts them all
    /// in one room once `size` are connected (the first one hosts), and
    /// forwards relay frames on the lane they came on. The `ponko_lobby`
    /// crate's tests run the sessions through the real server.
    pub(in crate::net) struct RelayServer {
        pub(in crate::net) net: NetMessages,
        /// The relay limit the server announces (4096, as `ponko-lobby`'s).
        pub(in crate::net) max_relay_bytes: u32,
        size: usize,
        members: BTreeMap<PeerId, MemberId>,
        order: Vec<MemberId>,
        scratch: Vec<u8>,
    }

    impl RelayServer {
        /// A server on `net` that puts the first `size` clients in one room.
        pub(in crate::net) fn new(net: NetMessages, size: usize) -> Self {
            Self {
                net,
                max_relay_bytes: 4096,
                size,
                members: BTreeMap::new(),
                order: Vec::new(),
                scratch: Vec::new(),
            }
        }

        pub(in crate::net) fn poll(&mut self, elapsed: Duration) {
            let _ = self.net.poll_elapsed(elapsed);
            while let Some(event) = self.net.next_event() {
                match event {
                    MessageEvent::Connected(peer) => {
                        let member = MemberId::from_bits(u32::try_from(peer.to_bits()).unwrap());
                        self.members.insert(peer, member);
                        self.order.push(member);
                        let limits = ServerLimits::new(self.max_relay_bytes, 16, 256, 60_000);
                        let welcome = Reply::Welcome {
                            version: protocol::VERSION,
                            member,
                            limits,
                        };
                        let _ = self.net.send(peer, Lane::Reliable, &protocol::encode_control(&welcome));
                        if self.order.len() == self.size {
                            let room = RoomInfo {
                                code: "RELAY1".into(),
                                host: self.order[0],
                                members: self.order.clone(),
                                size: u16::try_from(self.size).unwrap(),
                                meta: String::new(),
                            };
                            let joined = protocol::encode_control(&Reply::Joined { room });
                            for &p in self.members.keys() {
                                let _ = self.net.send(p, Lane::Reliable, &joined);
                            }
                        }
                    }
                    MessageEvent::Message { peer, lane, bytes } => {
                        let Some(&from) = self.members.get(&peer) else { continue };
                        let Some(Frame::Relay { member: to, payload }) = protocol::decode::<Request>(&bytes) else {
                            continue;
                        };
                        protocol::encode_relay(from.to_bits(), payload, &mut self.scratch);
                        let targets: Vec<PeerId> = self
                            .members
                            .iter()
                            .filter(|&(&p, m)| p != peer && (to == protocol::EVERYONE || m.to_bits() == to))
                            .map(|(&p, _)| p)
                            .collect();
                        for p in targets {
                            let _ = self.net.send(p, lane, &self.scratch);
                        }
                    }
                    MessageEvent::Disconnected(peer, _) => {
                        self.members.remove(&peer);
                    }
                }
            }
        }
    }

    /// A relay server at the scripted `addr(0)` and `clients` lobby
    /// clients at `addr(1)`, ... in its one room (the first one hosts),
    /// stepping `net` by one tick and `step` of time until they are in.
    pub(in crate::net) fn scripted_room(
        net: &Shared,
        clients: usize,
        step: Duration,
    ) -> (RelayServer, Vec<LobbyClient>) {
        let mut server = RelayServer::new(NetMessages::transported(port(net, 0), None, clients), clients);
        let mut lobbies: Vec<LobbyClient> = (1..=clients)
            .map(|i| LobbyClient::new(NetMessages::transported(port(net, i), Some(addr(0)), 1)))
            .collect();
        for _ in 0..2000 {
            net.lock().unwrap().tick += 1;
            server.poll(step);
            for lobby in &mut lobbies {
                lobby.poll_elapsed(step).unwrap();
            }
            if lobbies.iter().all(|l| l.room().is_some()) {
                lobbies.sort_by_key(|l| !l.is_host());
                for lobby in &mut lobbies {
                    while lobby.next_event().is_some() {}
                }
                return (server, lobbies);
            }
        }
        panic!("the clients never got into the room");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::LobbyEvent;
    use super::testing::RelayServer;
    use super::*;
    use crate::net::replication_conformance::{STEP, Session, declarations, exercise};
    use crate::net::{LoopbackLink, MessageEvent, PeerId};

    #[test]
    fn member_addresses_round_trip_and_stay_apart_from_other_transports() {
        for bits in [1, 2, 0xffff, 0x1_0000, u32::MAX] {
            let member = MemberId::from_bits(bits);
            assert_eq!(member_of(member_addr(member)), Some(member));
        }
        assert_eq!(member_of(super::super::super::websocket::HOST), None);
        assert_eq!(member_of("127.0.0.1:9".parse().unwrap()), None);
    }

    #[test]
    fn only_prefixed_payloads_are_session_packets() {
        assert_eq!(session_packet(b"PRS\x01abc"), Some(&b"abc"[..]));
        assert_eq!(session_packet(b"PRS\x02abc"), None);
        assert_eq!(session_packet(b"position 3 4"), None);
    }

    /// The relay server and `clients` lobby clients in its one room, over
    /// in-memory links shaped by `link`, the room's host first.
    fn room(clients: usize, link: LoopbackLink) -> (Arc<Mutex<RelayServer>>, Vec<LobbyClient>) {
        let mut endpoints = NetMessages::loopback_link(clients + 1, link);
        let server = Arc::new(Mutex::new(RelayServer::new(endpoints.remove(0), clients)));
        let mut lobbies: Vec<LobbyClient> = endpoints.into_iter().map(LobbyClient::new).collect();
        for _ in 0..600 {
            server.lock().unwrap().poll(STEP);
            for lobby in &mut lobbies {
                lobby.poll_elapsed(STEP).unwrap();
            }
            if lobbies.iter().all(|l| l.room().is_some()) {
                lobbies.sort_by_key(|l| !l.is_host());
                for lobby in &mut lobbies {
                    while lobby.next_event().is_some() {}
                }
                return (server, lobbies);
            }
        }
        panic!("the clients never got into the room");
    }

    /// Links that lose 5% of the datagrams and delay each by 17 to 47 ms,
    /// so they also arrive out of order; every datagram crosses two of
    /// them (to the server and on).
    fn lossy() -> LoopbackLink {
        LoopbackLink::new()
            .loss(0.05)
            .delay(Duration::from_millis(17))
            .jitter(Duration::from_millis(30))
            .seed(3)
    }

    /// Message endpoints for `lobbies` (host first), and the server polled
    /// before each of their ticks.
    fn endpoints(lobbies: Vec<LobbyClient>) -> Vec<NetMessages> {
        let clients = lobbies.len() - 1;
        let mut lobbies = lobbies.into_iter();
        let mut nets = vec![NetMessages::host_relay(lobbies.next().unwrap(), clients).unwrap()];
        nets.extend(lobbies.map(|l| NetMessages::join_relay(l).unwrap()));
        nets
    }

    #[test]
    fn replicates_over_the_lobby_relay_through_loss_and_latency() {
        let (server, lobbies) = room(3, lossy());
        let mut s = Session::over(endpoints(lobbies), declarations, false);
        s.before = Some(Box::new(move || server.lock().unwrap().poll(STEP)));
        exercise(s, 600, 300);
    }

    #[test]
    fn reliable_messages_arrive_once_and_in_order_through_a_lossy_relay() {
        let (server, lobbies) = room(2, lossy());
        let mut nets = endpoints(lobbies);
        let mut got = Vec::new();
        let mut sent = 0u32;
        for tick in 0..3000 {
            server.lock().unwrap().poll(STEP);
            for net in &mut nets {
                net.poll_elapsed(STEP).unwrap();
            }
            if nets[1].connected_peers() == 1 && sent < 200 && tick % 2 == 0 {
                nets[1].send(PeerId::HOST, Lane::Reliable, &sent.to_le_bytes()).unwrap();
                sent += 1;
            }
            for event in nets[0].events() {
                if let MessageEvent::Message { bytes, lane, .. } = event {
                    assert_eq!(lane, Lane::Reliable);
                    got.push(u32::from_le_bytes(bytes[..4].try_into().unwrap()));
                }
            }
            nets[1].events().for_each(drop);
            if got.len() == 200 {
                break;
            }
        }
        assert_eq!(got, (0..200).collect::<Vec<u32>>(), "every message once, in order");
    }

    #[test]
    fn the_games_own_relay_bytes_still_reach_it_next_to_a_session() {
        let (server, lobbies) = room(2, LoopbackLink::new());
        let mut nets = endpoints(lobbies);
        let mut heard = Vec::new();
        for _ in 0..200 {
            server.lock().unwrap().poll(STEP);
            for net in &mut nets {
                net.poll_elapsed(STEP).unwrap();
                net.events().for_each(drop);
            }
            if nets[1].connected_peers() == 1 && heard.is_empty() {
                let lobby = nets[1].relay_lobby().unwrap();
                let _ = lobby.relay(None, Lane::Reliable, b"chat: hello");
            }
            let lobby = nets[0].relay_lobby().unwrap();
            while let Some(event) = lobby.next_event() {
                if let LobbyEvent::Relayed { bytes, .. } = event {
                    heard.push(bytes);
                }
            }
            if !heard.is_empty() {
                break;
            }
        }
        assert_eq!(heard, [b"chat: hello".to_vec()]);
        assert_eq!(nets[0].connected_peers(), 1, "the session runs beside it");
    }

    #[test]
    fn a_relay_endpoint_reports_a_lost_lobby_connection_from_poll() {
        let (server, lobbies) = room(2, LoopbackLink::new());
        let mut nets = endpoints(lobbies);
        for _ in 0..100 {
            server.lock().unwrap().poll(STEP);
            for net in &mut nets {
                net.poll_elapsed(STEP).unwrap();
            }
        }
        assert_eq!(nets[1].connected_peers(), 1);
        drop(server); // The server is gone: the lobby connection times out.
        let mut error = None;
        for _ in 0..2000 {
            if let Err(e) = nets[1].poll_elapsed(STEP) {
                error = Some(e);
                break;
            }
        }
        let error = error.expect("poll reports the lost lobby");
        assert_eq!(error.kind(), io::ErrorKind::ConnectionAborted);
        assert!(error.to_string().contains("lobby"), "{error}");
    }

    #[test]
    fn a_server_that_relays_too_little_for_a_session_is_refused() {
        let mut endpoints = NetMessages::loopback(3);
        let mut server = RelayServer::new(endpoints.remove(0), 2);
        server.max_relay_bytes = 1000;
        let mut lobbies: Vec<LobbyClient> = endpoints.into_iter().map(LobbyClient::new).collect();
        for _ in 0..100 {
            server.poll(STEP);
            for lobby in &mut lobbies {
                lobby.poll_elapsed(STEP).unwrap();
            }
        }
        lobbies.sort_by_key(|l| !l.is_host());
        let guest = lobbies.pop().unwrap();
        let error = NetMessages::host_relay(lobbies.pop().unwrap(), 1)
            .err()
            .expect("refused");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("max_relay_bytes"), "{error}");
        assert!(Net::join_relay(guest).is_err());
    }

    #[test]
    fn joining_as_the_rooms_host_is_refused() {
        let (_server, mut lobbies) = room(2, LoopbackLink::new());
        let host = lobbies.remove(0);
        assert_eq!(
            NetMessages::join_relay(host).err().map(|e| e.kind()),
            Some(io::ErrorKind::InvalidInput)
        );
    }
}
