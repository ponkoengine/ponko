//! Game-owned byte messages over UDP, independently of deterministic lockstep.
//! Renet handles channels, acknowledgements, retransmission and fragmentation.
//! UDP sessions are unauthenticated and unencrypted: use a trusted network or
//! an application-managed secure tunnel. Feature `steam` adds Steam relays;
//! browser clients connect over WebSocket (`accept_web`, `join_web`).

use std::collections::{BTreeMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use renet::{ChannelConfig, ConnectionConfig, RenetClient, SendType};
use serde::{Serialize, de::DeserializeOwned};

/// Maximum application payload, in either lane (1 MiB).
/// ```
/// assert_eq!(ponko::net::MAX_MESSAGE_BYTES, 1024 * 1024);
/// ```
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const CHANNEL_MEMORY: usize = 4 * MAX_MESSAGE_BYTES;
const HEADER: usize = 21;
const MAGIC: &[u8; 4] = b"CRM1";
const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const DATA: u8 = 3;
const BYE: u8 = 4;
const FULL: u8 = 5;
const HEARTBEAT: u8 = 6;
const INTERVAL: Duration = Duration::from_millis(100);
const EVENT_LIMIT: usize = 4096;

/// Delivery policy; ordering is independent in each direction and peer.
/// ```
/// use ponko::net::Lane;
/// assert_ne!(ponko::net::Lane::Reliable, ponko::net::Lane::Unreliable);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    /// Retransmitted until acknowledged, delivered exactly once in send order
    /// while the connection survives.
    /// ```
    /// let lane = ponko::net::Lane::Reliable;
    /// ```
    Reliable,
    /// Reliable delivery without ordering between messages.
    /// ```
    /// let lane = ponko::net::Lane::ReliableUnordered;
    /// ```
    ReliableUnordered,
    /// Never retransmitted; messages older than the newest delivered message
    /// are discarded. A fragmented message is lost if any fragment is lost.
    /// ```
    /// let lane = ponko::net::Lane::Unreliable;
    /// ```
    Unreliable,
}
impl Lane {
    fn channel(self) -> u8 {
        match self {
            Self::Reliable => 0,
            Self::Unreliable => 1,
            Self::ReliableUnordered => 2,
        }
    }
}

/// Opaque, session-local peer identifier. Clients address the host as `HOST`;
/// hosts obtain client identifiers from connection events. IDs are not reused.
/// ```
/// let host = ponko::net::PeerId::HOST;
/// assert_eq!(host, ponko::net::PeerId::HOST);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(u64);
impl PeerId {
    /// Destination for a joining client's messages.
    /// ```
    /// let peer = ponko::net::PeerId::HOST;
    /// ```
    pub const HOST: Self = Self(0);

    /// The id as a number, for language bindings and logs.
    /// ```
    /// assert_eq!(ponko::net::PeerId::HOST.to_bits(), 0);
    /// ```
    pub const fn to_bits(self) -> u64 {
        self.0
    }

    /// An id from [`to_bits`](Self::to_bits). Sending to one that never
    /// connected fails; it isn't unsafe.
    /// ```
    /// use ponko::net::PeerId;
    /// assert_eq!(ponko::net::PeerId::from_bits(0), ponko::net::PeerId::HOST);
    /// ```
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }
}

/// Connection changes and owned application payloads. Drain events every poll.
/// ```
/// use ponko::net::MessageEvent;
/// use ponko::net::PeerId;
/// assert!(matches!(ponko::net::MessageEvent::Connected(ponko::net::PeerId::HOST), ponko::net::MessageEvent::Connected(_)));
/// ```
#[derive(Debug, PartialEq, Eq)]
pub enum MessageEvent {
    /// A peer can now receive messages.
    /// ```
    /// let event = ponko::net::MessageEvent::Connected(ponko::net::PeerId::HOST);
    /// ```
    Connected(PeerId),
    /// A peer left, timed out, was refused, or violated protocol/channel limits.
    /// The reason is intended for diagnostics, not machine parsing.
    /// ```
    /// let event = ponko::net::MessageEvent::Disconnected(ponko::net::PeerId::HOST, "timeout".into());
    /// ```
    Disconnected(PeerId, String),
    /// One complete application message.
    /// ```
    /// use ponko::prelude::*;
    /// let event = ponko::net::MessageEvent::Message { peer: ponko::net::PeerId::HOST, lane: ponko::net::Lane::Reliable, bytes: vec![1] };
    /// ```
    Message {
        /// Sender, as in `Connected`.
        /// ```
        /// # use ponko::prelude::*;
        /// let peer = ponko::net::PeerId::HOST;
        /// ```
        peer: PeerId,
        /// Delivery policy used by the sender.
        /// ```
        /// let lane = ponko::net::Lane::Reliable;
        /// ```
        lane: Lane,
        /// Complete payload, without transport or sequence headers.
        /// ```
        /// let bytes = vec![1u8, 2, 3];
        /// ```
        bytes: Vec<u8>,
    },
}
impl MessageEvent {
    /// Decode a JSON message sent with `send_serde`; `None` for other events
    /// or a payload that does not match `T`.
    /// ```
    /// use ponko::prelude::*;
    /// let event = ponko::net::MessageEvent::Message { peer: ponko::net::PeerId::HOST, lane: ponko::net::Lane::Reliable, bytes: b"42".to_vec() };
    /// assert_eq!(event.decode::<u32>(), Some(42));
    /// ```
    pub fn decode<T: DeserializeOwned>(&self) -> Option<T> {
        match self {
            Self::Message { bytes, .. } => serde_json::from_slice(bytes).ok(),
            _ => None,
        }
    }
}

/// Cumulative transport counters, including handshake, heartbeat and resends.
/// ```
/// assert_eq!(ponko::net::MessageStats::default().bytes_sent, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageStats {
    /// Successfully submitted datagrams (including simulated loss).
    /// ```
    /// assert_eq!(ponko::net::MessageStats::default().packets_sent, 0);
    /// ```
    pub packets_sent: u64,
    /// Submitted datagram bytes, including framing.
    /// ```
    /// assert_eq!(ponko::net::MessageStats::default().bytes_sent, 0);
    /// ```
    pub bytes_sent: u64,
    /// Received datagrams, including ignored packets.
    /// ```
    /// assert_eq!(ponko::net::MessageStats::default().packets_received, 0);
    /// ```
    pub packets_received: u64,
    /// Received datagram bytes.
    /// ```
    /// assert_eq!(ponko::net::MessageStats::default().bytes_received, 0);
    /// ```
    pub bytes_received: u64,
}

struct Link {
    addr: SocketAddr,
    client_nonce: u64,
    server_nonce: u64,
    connected: bool,
    last_seen: Duration,
    last_sent: Duration,
    renet: RenetClient,
    next_unreliable: u64,
    received_unreliable: Option<u64>,
}
impl Link {
    fn new(addr: SocketAddr, client_nonce: u64, server_nonce: u64, now: Duration) -> Self {
        let channels = vec![
            ChannelConfig {
                channel_id: 0,
                max_memory_usage_bytes: CHANNEL_MEMORY,
                send_type: SendType::ReliableOrdered { resend_time: INTERVAL },
            },
            ChannelConfig {
                channel_id: 2,
                max_memory_usage_bytes: CHANNEL_MEMORY,
                send_type: SendType::ReliableUnordered { resend_time: INTERVAL },
            },
            ChannelConfig {
                channel_id: 1,
                max_memory_usage_bytes: CHANNEL_MEMORY,
                send_type: SendType::Unreliable,
            },
        ];
        let mut renet = RenetClient::new(ConnectionConfig {
            // Unreliable messages must fit in one Renet update budget or Renet
            // discards them before fragmentation. Allow the full API limit.
            available_bytes_per_tick: (2 * MAX_MESSAGE_BYTES) as u64,
            server_channels_config: channels.clone(),
            client_channels_config: channels,
        });
        renet.set_connected();
        Self {
            addr,
            client_nonce,
            server_nonce,
            connected: server_nonce != 0,
            last_seen: now,
            last_sent: now,
            renet,
            next_unreliable: 0,
            received_unreliable: None,
        }
    }
}
type Inbox = VecDeque<(SocketAddr, Vec<u8>)>;
/// Packet boundary below the renet protocol and session handshake.
///
/// Implementations: the Steam relay (feature `steam`) and the scripted
/// transport used by the conformance tests in `net/message_conformance.rs`,
/// which also lists the contract each transport has to keep.
#[cfg(any(test, feature = "steam"))]
pub(super) trait PacketTransport: Send {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool>;
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>>;
    #[cfg(feature = "steam")]
    fn lobby(&self) -> Option<&super::SteamLobby> {
        None
    }
}

enum Wire {
    #[cfg(any(test, feature = "steam"))]
    Custom(Box<dyn PacketTransport>),
    Udp(UdpSocket),
    /// A UDP host that also takes browser clients over WebSocket.
    #[cfg(not(target_arch = "wasm32"))]
    UdpWeb(UdpSocket, Box<super::websocket::WebListener>, bool),
    /// A client's WebSocket to its host.
    Web(Box<super::websocket::WebClient>),
    Local {
        me: usize,
        inboxes: Arc<Mutex<Vec<Inbox>>>,
        drop_one_in: u32,
        sent: u64,
    },
}
impl Wire {
    fn addr(&self) -> io::Result<SocketAddr> {
        match self {
            #[cfg(any(test, feature = "steam"))]
            Self::Custom(_) => Err(io::Error::other("no UDP address")),
            Self::Udp(s) => s.local_addr(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::UdpWeb(s, ..) => s.local_addr(),
            Self::Web(_) => Err(io::Error::other("no UDP address")),
            Self::Local { me, .. } => Ok(local_addr(*me)),
        }
    }
    fn send(&mut self, addr: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        match self {
            #[cfg(any(test, feature = "steam"))]
            Self::Custom(t) => t.send(addr, bytes),
            Self::Udp(s) => udp_send(s, addr, bytes),
            #[cfg(not(target_arch = "wasm32"))]
            Self::UdpWeb(s, web, _) => {
                if super::websocket::WebListener::owns(addr) {
                    Ok(web.send(addr, bytes))
                } else {
                    udp_send(s, addr, bytes)
                }
            }
            Self::Web(client) => Ok(client.send(bytes)),
            Self::Local {
                me,
                inboxes,
                drop_one_in,
                sent,
            } => {
                *sent += 1;
                if *drop_one_in != 0 && sent.is_multiple_of(u64::from(*drop_one_in)) {
                    return Ok(true);
                }
                let mut queues = inboxes.lock().expect("loopback inbox");
                if let Some(queue) = queues.get_mut(usize::from(addr.port() - 1))
                    && queue.len() < 4096
                {
                    queue.push_back((local_addr(*me), bytes.to_vec()));
                }
                Ok(true)
            }
        }
    }
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        match self {
            #[cfg(any(test, feature = "steam"))]
            Self::Custom(t) => t.recv(buf),
            Self::Udp(s) => udp_recv(s, buf),
            #[cfg(not(target_arch = "wasm32"))]
            Self::UdpWeb(s, web, web_first) => {
                // Alternate, so neither side starves the other.
                *web_first = !*web_first;
                if *web_first && let Some(packet) = web.recv(buf) {
                    return Ok(Some(packet));
                }
                match udp_recv(s, buf)? {
                    Some(packet) => Ok(Some(packet)),
                    None => Ok(web.recv(buf)),
                }
            }
            Self::Web(client) => {
                if let Some(e) = client.take_error() {
                    return Err(io::Error::new(io::ErrorKind::ConnectionAborted, e));
                }
                Ok(client.recv(buf).map(|n| (super::websocket::HOST, n)))
            }
            Self::Local { me, inboxes, .. } => {
                // Reverse arrival intentionally exercises packet reordering.
                Ok(inboxes.lock().expect("loopback inbox")[*me]
                    .pop_back()
                    .map(|(addr, bytes)| {
                        buf[..bytes.len()].copy_from_slice(&bytes);
                        (addr, bytes.len())
                    }))
            }
        }
    }
}
fn udp_send(s: &UdpSocket, addr: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
    match s.send_to(bytes, addr) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(false),
        Err(e) => Err(e),
    }
}
fn udp_recv(s: &UdpSocket, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
    match s.recv_from(buf) {
        Ok((n, addr)) => Ok(Some((addr, n))),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e),
    }
}
fn local_addr(index: usize) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], (index + 1) as u16))
}
fn nonce() -> u64 {
    // RandomState seeds each hasher from OS randomness. This is a session
    // discriminator, not authentication or a cryptographic key.
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u8(1);
    h.finish().max(1)
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// A game-owned, explicitly polled message endpoint. Hosts talk to clients;
/// clients talk to their host. For P2P, each peer can host a separate endpoint.
/// No simulation barriers, background threads or async runtime. Poll and drain
/// every frame, even when simulation is paused. Application messages allocate;
/// Renet also allocates outgoing packet batches. Queues are bounded.
///
/// ```
/// use ponko::prelude::*;
/// let mut host = ponko::net::NetMessages::host("127.0.0.1:0", 8).unwrap();
/// host.poll().unwrap();
/// for event in host.events() { println!("{event:?}"); }
/// ```
pub struct NetMessages {
    wire: Wire,
    hosting: bool,
    max_clients: usize,
    links: BTreeMap<PeerId, Link>,
    next_peer: u64,
    now: Duration,
    last_poll: crate::perf::Instant,
    timeout: Duration,
    events: VecDeque<MessageEvent>,
    event_bytes: usize,
    stats: MessageStats,
    scratch: Vec<u8>,
}
impl NetMessages {
    #[cfg(any(test, feature = "steam"))]
    pub(super) fn transported(
        transport: Box<dyn PacketTransport>,
        host: Option<SocketAddr>,
        max_clients: usize,
    ) -> Self {
        let mut net = Self::new(Wire::Custom(transport), host.is_none(), max_clients);
        if let Some(addr) = host {
            net.links.insert(PeerId::HOST, Link::new(addr, nonce(), 0, net.now));
        }
        net
    }
    #[cfg(feature = "steam")]
    pub(super) fn transport_peer_addr(&self, peer: PeerId) -> Option<SocketAddr> {
        self.links.get(&peer).filter(|l| l.connected).map(|l| l.addr)
    }
    #[cfg(feature = "steam")]
    pub(super) fn transport_lobby(&self) -> Option<&super::SteamLobby> {
        match &self.wire {
            Wire::Custom(t) => t.lobby(),
            _ => None,
        }
    }
    fn new(wire: Wire, hosting: bool, max_clients: usize) -> Self {
        Self {
            wire,
            hosting,
            max_clients,
            links: BTreeMap::new(),
            next_peer: 1,
            now: Duration::ZERO,
            last_poll: crate::perf::Instant::now(),
            timeout: Duration::from_secs(10),
            events: VecDeque::new(),
            event_bytes: 0,
            stats: MessageStats::default(),
            scratch: Vec::with_capacity(1500),
        }
    }
    /// Bind a nonblocking UDP host. `max_clients` is 1..=1024, excluding host.
    /// Port zero chooses an available port.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `max_clients` is outside 1..=1024, or the error of
    /// binding or configuring the socket.
    ///
    /// ```
    /// let host = ponko::net::NetMessages::host("127.0.0.1:0", 4).unwrap();
    /// ```
    pub fn host(addr: impl ToSocketAddrs, max_clients: usize) -> io::Result<Self> {
        if !(1..=1024).contains(&max_clients) {
            return Err(invalid("max_clients must be 1..=1024"));
        }
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(Self::new(Wire::Udp(socket), true, max_clients))
    }
    /// Start joining a UDP host. Poll both sides until `Connected` or
    /// `Disconnected`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` if `addr` resolves to no address, or the error of
    /// resolving it or binding the local socket. They are returned at once;
    /// a host that never answers shows up later as `Disconnected`.
    ///
    /// ```
    /// let client = ponko::net::NetMessages::join("127.0.0.1:7777").unwrap();
    /// ```
    pub fn join(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| invalid("no host address"))?;
        let bind = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        let mut net = Self::new(Wire::Udp(socket), false, 1);
        net.links.insert(PeerId::HOST, Link::new(addr, nonce(), 0, net.now));
        Ok(net)
    }
    /// In-memory star topology: first endpoint hosts. Packets arrive newest
    /// first to exercise reordering.
    ///
    /// # Panics
    ///
    /// Unless `players` is 2..=1025.
    ///
    /// ```
    /// assert_eq!(ponko::net::NetMessages::loopback(3).len(), 3);
    /// ```
    pub fn loopback(players: usize) -> Vec<Self> {
        Self::loopback_lossy(players, 0)
    }
    /// Like `loopback`, dropping every `drop_one_in`th outgoing datagram per
    /// endpoint (zero disables loss). A value of one drops all packets.
    ///
    /// # Panics
    ///
    /// Unless `players` is 2..=1025.
    ///
    /// ```
    /// let nets = ponko::net::NetMessages::loopback_lossy(2, 5);
    /// assert_eq!(nets.len(), 2);
    /// ```
    pub fn loopback_lossy(players: usize, drop_one_in: u32) -> Vec<Self> {
        assert!((2..=1025).contains(&players), "players must be 2..=1025");
        let inboxes = Arc::new(Mutex::new((0..players).map(|_| VecDeque::new()).collect()));
        (0..players)
            .map(|me| {
                let mut net = Self::new(
                    Wire::Local {
                        me,
                        inboxes: inboxes.clone(),
                        drop_one_in,
                        sent: me as u64,
                    },
                    me == 0,
                    players - 1,
                );
                if me != 0 {
                    net.links
                        .insert(PeerId::HOST, Link::new(local_addr(0), nonce(), 0, net.now));
                }
                net
            })
            .collect()
    }
    /// Set silence/connection timeout (default ten seconds).
    ///
    /// # Panics
    ///
    /// If `timeout` is zero.
    ///
    /// ```
    /// let net = ponko::net::NetMessages::loopback(2).remove(0)
    ///     .timeout(std::time::Duration::from_secs(3));
    /// ```
    pub fn timeout(mut self, timeout: Duration) -> Self {
        assert!(!timeout.is_zero(), "timeout must be positive");
        self.timeout = timeout;
        self
    }
    /// Local UDP address (`None` for in-memory sessions).
    /// ```
    /// assert!(ponko::net::NetMessages::loopback(2)[0].local_addr().is_none());
    /// ```
    pub fn local_addr(&self) -> Option<SocketAddr> {
        match self.wire {
            Wire::Udp(_) => self.wire.addr().ok(),
            #[cfg(not(target_arch = "wasm32"))]
            Wire::UdpWeb(..) => self.wire.addr().ok(),
            _ => None,
        }
    }
    /// The address browser clients connect to, once
    /// [`accept_web`](Self::accept_web) opened it.
    /// ```
    /// let host = ponko::net::NetMessages::host("127.0.0.1:0", 4).unwrap().accept_web("127.0.0.1:0").unwrap();
    /// assert!(host.web_addr().unwrap().port() != 0);
    /// ```
    pub fn web_addr(&self) -> Option<SocketAddr> {
        match &self.wire {
            #[cfg(not(target_arch = "wasm32"))]
            Wire::UdpWeb(_, web, _) => web.local_addr().ok(),
            _ => None,
        }
    }
    /// Also lets browser clients in: they join over WebSocket on `addr` (TCP,
    /// such as `"0.0.0.0:7778"`; port 0 picks a free one, see
    /// [`web_addr`](Self::web_addr)) with [`join_web`](Self::join_web), next
    /// to UDP clients, within the same `max_clients`. Plain `ws://`; put a TLS
    /// proxy in front for pages served over `https://`. Unauthenticated, like
    /// UDP. A WebSocket delivers everything in order, so the unreliable lane
    /// arrives too, only no sooner than the reliable one.
    /// ```
    /// let host = ponko::net::NetMessages::host("127.0.0.1:0", 4).unwrap().accept_web("127.0.0.1:0").unwrap();
    /// ```
    /// # Errors
    /// `InvalidInput` unless this is a UDP host (or it already accepts
    /// browser clients); `Unsupported` in a browser; the socket's error if
    /// `addr` can't be bound.
    pub fn accept_web(mut self, addr: impl ToSocketAddrs) -> io::Result<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !self.hosting || !matches!(self.wire, Wire::Udp(_)) {
                return Err(invalid("only a UDP host accepts browser clients"));
            }
            let web = super::websocket::WebListener::bind(addr)?;
            let Wire::Udp(socket) = self.wire else {
                unreachable!("checked above");
            };
            self.wire = Wire::UdpWeb(socket, Box::new(web), false);
            Ok(self)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (&mut self, addr);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "a browser can't accept connections",
            ))
        }
    }
    /// Start joining a host over WebSocket at `url`, such as
    /// `"ws://192.168.1.20:7778"` (its [`accept_web`](Self::accept_web)
    /// address): how a web build joins, as browsers can't open UDP sockets.
    /// Poll until `Connected` or `Disconnected`, as with [`join`](Self::join).
    /// A page served over `https://` needs `wss://`.
    /// ```
    /// let client = ponko::net::NetMessages::join_web("ws://127.0.0.1:7778").unwrap();
    /// ```
    /// # Errors
    /// `InvalidInput` for a URL that isn't `ws://` (or `wss://` in a browser)
    /// with a host; `Unsupported` for `wss://` on the desktop.
    pub fn join_web(url: &str) -> io::Result<Self> {
        let client = super::websocket::WebClient::connect(url)?;
        let mut net = Self::new(Wire::Web(Box::new(client)), false, 1);
        net.links
            .insert(PeerId::HOST, Link::new(super::websocket::HOST, nonce(), 0, net.now));
        Ok(net)
    }
    /// Queue bytes for a connected peer. Returns `NotConnected` for unknown or
    /// joining peers, `InvalidInput` above 1 MiB and `WouldBlock` when the lane's
    /// 4 MiB send budget is exhausted. Reliable messages remain queued until acked.
    ///
    /// # Errors
    ///
    /// `NotConnected` for an unknown or still joining peer, `InvalidInput`
    /// for more than 1 MiB (or an exhausted unreliable sequence), and
    /// `WouldBlock` when the lane's 4 MiB send budget is full.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// assert!(nets[1].send(ponko::net::PeerId::HOST, ponko::net::Lane::Reliable, b"hi").is_err());
    /// ```
    pub fn send(&mut self, peer: PeerId, lane: Lane, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(invalid("message exceeds 1 MiB"));
        }
        let link = self
            .links
            .get_mut(&peer)
            .filter(|l| l.connected)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "peer is not connected"))?;
        let extra = if lane == Lane::Unreliable { 8 } else { 0 };
        if !link.renet.can_send_message(lane.channel(), bytes.len() + extra) {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "message lane is full"));
        }
        let mut payload = Vec::with_capacity(bytes.len() + extra);
        if lane == Lane::Unreliable {
            payload.extend_from_slice(&link.next_unreliable.to_le_bytes());
            link.next_unreliable = link
                .next_unreliable
                .checked_add(1)
                .ok_or_else(|| invalid("sequence exhausted"))?;
        }
        payload.extend_from_slice(bytes);
        link.renet.send_message(lane.channel(), payload);
        Ok(())
    }
    /// Serialize JSON and queue it as bytes; the application may use
    /// `MessageEvent::decode` to receive it.
    ///
    /// # Errors
    ///
    /// The serialization error, or any error of [`send`](Self::send).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// let _ = nets[1].send_serde(ponko::net::PeerId::HOST, ponko::net::Lane::Reliable, &42u32);
    /// ```
    pub fn send_serde<T: Serialize>(&mut self, peer: PeerId, lane: Lane, value: &T) -> io::Result<()> {
        let bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
        self.send(peer, lane, &bytes)
    }
    /// Queue a message for all connected peers. Stops on the first error;
    /// earlier peers may already have it. Hosts do not implicitly relay messages.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for more than 1 MiB, or the first error of
    /// [`send`](Self::send) for a connected peer.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// ponko::net::NetMessages::loopback(2)[0].send_to_all(ponko::net::Lane::Reliable, b"hello").unwrap();
    /// ```
    pub fn send_to_all(&mut self, lane: Lane, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(invalid("message exceeds 1 MiB"));
        }
        // Temporarily moving links would complicate send; the small ID snapshot
        // allocates only when broadcasting, never during an idle poll.
        let peers: Vec<_> = self
            .links
            .iter()
            .filter(|(_, l)| l.connected)
            .map(|(&p, _)| p)
            .collect();
        for peer in peers {
            self.send(peer, lane, bytes)?;
        }
        Ok(())
    }
    /// Poll with elapsed wall time, once per game frame. Processes at most 256
    /// incoming datagrams per call.
    ///
    /// # Errors
    ///
    /// Errors from the UDP socket, and `WouldBlock` if the event queue is
    /// full: drain `events()` and continue polling.
    ///
    /// ```
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// nets[0].poll().unwrap();
    /// ```
    pub fn poll(&mut self) -> io::Result<()> {
        let delta = self.last_poll.elapsed();
        self.poll_elapsed(delta)
    }
    /// Poll with an explicit elapsed duration for deterministic tests or a
    /// caller-owned clock. Do not mix with `poll` in one clock interval.
    ///
    /// # Errors
    ///
    /// The errors of [`poll`](Self::poll).
    ///
    /// ```
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// nets[0].poll_elapsed(std::time::Duration::from_millis(16)).unwrap();
    /// ```
    pub fn poll_elapsed(&mut self, elapsed: Duration) -> io::Result<()> {
        self.last_poll = crate::perf::Instant::now();
        self.now = self.now.saturating_add(elapsed);
        if self.events.len() >= EVENT_LIMIT || self.event_bytes >= CHANNEL_MEMORY {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "drain message events before polling",
            ));
        }
        for link in self.links.values_mut() {
            link.renet.update(elapsed);
        }
        let mut buf = [0u8; 1500];
        for _ in 0..256 {
            let Some((addr, n)) = self.wire.recv(&mut buf)? else {
                break;
            };
            self.stats.packets_received += 1;
            self.stats.bytes_received += n as u64;
            self.receive(addr, &buf[..n])?;
        }
        // Reuse the peer map allocation; remove expired links with retain after
        // collecting all work, without a per-frame peer snapshot.
        let mut links = std::mem::take(&mut self.links);
        let result = (|| {
            for (&peer, link) in &mut links {
                if self.now.saturating_sub(link.last_seen) >= self.timeout {
                    self.events
                        .push_back(MessageEvent::Disconnected(peer, "peer timed out".into()));
                    link.renet.disconnect();
                    continue;
                }
                if let Some(reason) = link.renet.disconnect_reason() {
                    self.events
                        .push_back(MessageEvent::Disconnected(peer, reason.to_string()));
                    continue;
                }
                if link.connected {
                    for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
                        while self.events.len() < EVENT_LIMIT && self.event_bytes < CHANNEL_MEMORY {
                            let Some(message) = link.renet.receive_message(lane.channel()) else {
                                break;
                            };
                            let bytes = if lane == Lane::Unreliable {
                                let Some(seq) = message.get(..8) else { continue };
                                let seq = u64::from_le_bytes(seq.try_into().expect("sequence"));
                                if link.received_unreliable.is_some_and(|last| seq <= last) {
                                    continue;
                                }
                                link.received_unreliable = Some(seq);
                                &message[8..]
                            } else {
                                &message[..]
                            };
                            if bytes.len() > MAX_MESSAGE_BYTES {
                                link.renet.disconnect();
                                self.events
                                    .push_back(MessageEvent::Disconnected(peer, "message exceeds 1 MiB".into()));
                                break;
                            }
                            self.event_bytes += bytes.len();
                            self.events.push_back(MessageEvent::Message {
                                peer,
                                lane,
                                bytes: bytes.to_vec(),
                            });
                        }
                    }
                    for packet in link.renet.get_packets_to_send() {
                        self.transmit(link, DATA, &packet)?;
                    }
                }
                if self.now.saturating_sub(link.last_sent) >= INTERVAL {
                    self.transmit(link, if link.connected { HEARTBEAT } else { HELLO }, &[])?;
                    link.last_sent = self.now;
                }
            }
            Ok(())
        })();
        links.retain(|_, link| !link.renet.is_disconnected());
        self.links = links;
        result
    }
    fn transmit(&mut self, link: &Link, kind: u8, data: &[u8]) -> io::Result<()> {
        self.scratch.clear();
        self.scratch.extend_from_slice(MAGIC);
        self.scratch.push(kind);
        self.scratch.extend_from_slice(&link.client_nonce.to_le_bytes());
        self.scratch.extend_from_slice(&link.server_nonce.to_le_bytes());
        self.scratch.extend_from_slice(data);
        if self.wire.send(link.addr, &self.scratch)? {
            self.stats.packets_sent += 1;
            self.stats.bytes_sent += self.scratch.len() as u64;
        }
        Ok(())
    }
    fn receive(&mut self, addr: SocketAddr, data: &[u8]) -> io::Result<()> {
        if data.len() < HEADER || &data[..4] != MAGIC {
            return Ok(());
        }
        let kind = data[4];
        let cn = u64::from_le_bytes(data[5..13].try_into().expect("nonce"));
        let sn = u64::from_le_bytes(data[13..21].try_into().expect("nonce"));
        if self.hosting && kind == HELLO && cn != 0 && sn == 0 {
            if let Some(link) = self.links.values().find(|l| l.addr == addr) {
                if link.client_nonce == cn {
                    let peer = *self.links.iter().find(|(_, l)| l.addr == addr).expect("link").0;
                    let link = self.links.remove(&peer).expect("link");
                    let result = self.transmit(&link, WELCOME, &[]);
                    self.links.insert(peer, link);
                    result?;
                }
                return Ok(());
            }
            let link = Link::new(addr, cn, nonce(), self.now);
            if self.links.len() >= self.max_clients {
                // FULL uses the nonce the client already knows.
                let rejected = Link::new(addr, cn, 0, self.now);
                return self.transmit(&rejected, FULL, &[]);
            }
            let peer = PeerId(self.next_peer);
            self.next_peer = self
                .next_peer
                .checked_add(1)
                .ok_or_else(|| io::Error::other("peer IDs exhausted"))?;
            self.transmit(&link, WELCOME, &[])?;
            self.links.insert(peer, link);
            self.events.push_back(MessageEvent::Connected(peer));
            return Ok(());
        }
        let Some((&peer, _)) = self.links.iter().find(|(_, l)| l.addr == addr && l.client_nonce == cn) else {
            return Ok(());
        };
        let link = self.links.get_mut(&peer).expect("link");
        if !self.hosting && !link.connected && kind == FULL && sn == 0 {
            self.links.remove(&peer);
            self.events
                .push_back(MessageEvent::Disconnected(peer, "host is full".into()));
            return Ok(());
        }
        if !self.hosting && !link.connected && kind == WELCOME && sn != 0 {
            link.server_nonce = sn;
            link.connected = true;
            self.events.push_back(MessageEvent::Connected(peer));
        }
        if !link.connected || sn != link.server_nonce {
            return Ok(());
        }
        if !matches!(kind, WELCOME | DATA | BYE | HEARTBEAT) {
            return Ok(());
        }
        link.last_seen = self.now;
        match kind {
            DATA => link.renet.process_packet(&data[HEADER..]),
            BYE => {
                self.links.remove(&peer);
                self.events
                    .push_back(MessageEvent::Disconnected(peer, "peer disconnected".into()));
            }
            _ => {}
        }
        Ok(())
    }
    /// Takes the oldest event, if any: [`events`](Self::events) one at a time
    /// (dropping that iterator early discards the rest; this doesn't).
    /// ```
    /// use ponko::net::NetMessages;
    /// let mut peers = ponko::net::NetMessages::loopback(2);
    /// while let Some(event) = peers[0].next_event() { println!("{event:?}"); }
    /// ```
    pub fn next_event(&mut self) -> Option<MessageEvent> {
        let event = self.events.pop_front()?;
        if let MessageEvent::Message { bytes, .. } = &event {
            self.event_bytes = self.event_bytes.saturating_sub(bytes.len());
        }
        Some(event)
    }
    /// Drain pending events. Dropping the iterator discards its remaining
    /// events. Backpressure limits undrained events to about 5 MiB/4096 entries.
    /// ```
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// assert_eq!(nets[0].events().count(), 0);
    /// ```
    pub fn events(&mut self) -> impl Iterator<Item = MessageEvent> + '_ {
        self.event_bytes = 0;
        self.events.drain(..)
    }
    /// Disconnect immediately and discard queued messages. Sends three best-effort
    /// notifications; if all are lost the remote reports a timeout. A local
    /// `Disconnected` event is queued even if the socket returns an error.
    ///
    /// # Errors
    ///
    /// `NotConnected` for an unknown peer, or the socket's error while
    /// sending the notifications.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// nets[1].disconnect(ponko::net::PeerId::HOST).unwrap();
    /// ```
    pub fn disconnect(&mut self, peer: PeerId) -> io::Result<()> {
        let link = self
            .links
            .remove(&peer)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "unknown peer"))?;
        self.events
            .push_back(MessageEvent::Disconnected(peer, "disconnected locally".into()));
        for _ in 0..3 {
            self.transmit(&link, BYE, &[])?;
        }
        Ok(())
    }
    /// Cumulative datagram counters for this endpoint.
    /// ```
    /// let nets = ponko::net::NetMessages::loopback(2);
    /// assert_eq!(nets[0].stats().packets_sent, 0);
    /// ```
    pub fn stats(&self) -> MessageStats {
        self.stats
    }
    /// How many peers are connected now: clients for a host, the host (0 or
    /// 1) for a client.
    /// ```
    /// let mut nets = ponko::net::NetMessages::loopback(2);
    /// assert_eq!(nets[0].connected_peers(), 0);
    /// for _ in 0..40 {
    ///     nets.iter_mut().for_each(|n| n.poll_elapsed(std::time::Duration::from_millis(20)).unwrap());
    /// }
    /// assert_eq!((nets[0].connected_peers(), nets[1].connected_peers()), (1, 1));
    /// ```
    pub fn connected_peers(&self) -> usize {
        self.links.values().filter(|link| link.connected).count()
    }
}

/// Hooks for the fuzzing entry points in `net/fuzz.rs`.
#[cfg(any(test, fuzzing))]
impl NetMessages {
    /// The address of loopback endpoint `index`.
    pub(super) fn loopback_addr(index: usize) -> SocketAddr {
        local_addr(index)
    }

    /// Takes `packet` as a datagram from `from`. With `header`, `packet`
    /// starts with a kind byte, and the magic and the nonces of the link
    /// to `from` (made-up ones when there is none) go around it.
    pub(super) fn fuzz_receive(&mut self, from: SocketAddr, packet: &[u8], header: bool) -> io::Result<()> {
        if !header {
            return self.receive(from, packet);
        }
        let (kind, rest) = packet.split_first().map_or((DATA, &[][..]), |(k, r)| (*k, r));
        let (cn, sn) = self
            .links
            .values()
            .find(|l| l.addr == from)
            .map_or((1, 0), |l| (l.client_nonce, l.server_nonce));
        let mut datagram = Vec::with_capacity(HEADER + rest.len());
        datagram.extend_from_slice(MAGIC);
        datagram.push(kind);
        datagram.extend_from_slice(&cn.to_le_bytes());
        datagram.extend_from_slice(&sn.to_le_bytes());
        datagram.extend_from_slice(rest);
        self.receive(from, &datagram)
    }
}
