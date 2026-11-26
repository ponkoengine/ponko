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
/// Every version's packets start with `CRM` and the version's digit.
const MAGIC_FAMILY: &[u8; 3] = b"CRM";
/// Version 2: hellos are padded and UDP hosts check address cookies.
const MAGIC: &[u8; 4] = b"CRM2";
const HELLO: u8 = 1;
const WELCOME: u8 = 2;
const DATA: u8 = 3;
const BYE: u8 = 4;
const FULL: u8 = 5;
const HEARTBEAT: u8 = 6;
/// A UDP host's answer to a hello without a valid cookie: the cookie to
/// echo, and nothing else kept.
const CHALLENGE: u8 = 7;
/// A host's answer to a hello of another protocol version. Its layout
/// (family magic and the host's digit, kind, the client's nonce, zero) must
/// stay the same in every version, so that any client can read it.
const WRONG_VERSION: u8 = 8;
/// A cookie: the key period it was made in (`u32`), then the 128-bit tag.
const COOKIE: usize = 20;
/// Every hello is padded to this size, so that the challenge it can cause
/// (`HEADER + COOKIE` bytes) is never larger: a host can't be used to
/// amplify a flood towards a forged address.
const HELLO_BYTES: usize = 64;
const _: () = assert!(
    HEADER + COOKIE <= HELLO_BYTES,
    "a challenge is never larger than its hello"
);
/// Cookie keys change this often; the current and the previous key are
/// kept, so a cookie is good for 5 to 10 s.
const COOKIE_PERIOD: u64 = 5;
/// At most this many challenges and version refusals a second, all
/// strangers together, so that a host can't be made to flood one victim.
const STRANGER_ANSWERS_PER_SECOND: u32 = 256;
/// At most this many of them a second to one network (an IPv4 /24 or an
/// IPv6 /56), so that a flood from one network doesn't use up the answers
/// meant for joins from others.
const NETWORK_ANSWERS_PER_SECOND: u32 = 16;
/// Networks with a budget of their own at once. A network's entry is free
/// again one second after its last answer (its budget is full by then);
/// while every entry is in use, a new network has only the global budget.
const NETWORK_BUDGETS: usize = 64;
const INTERVAL: Duration = Duration::from_millis(100);
const EVENT_LIMIT: usize = 4096;
/// Renet channels of the engine's internal lanes (replication).
const INTERNAL_RELIABLE: u8 = 3;
const INTERNAL_UNRELIABLE: u8 = 4;
/// Internal messages kept for the replication layer at most; more are
/// left in renet (reliable) or dropped (unreliable) until it takes them.
const INTERNAL_LIMIT: usize = 4096;

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
    last_sent: Option<Duration>,
    renet: RenetClient,
    next_unreliable: u64,
    received_unreliable: Option<u64>,
    /// A joining client's cookie from the host's challenge (zeros before).
    cookie: [u8; COOKIE],
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
            // The engine's own lanes for replication (`net::replicate`):
            // never seen by the game's `events()`. Empty lanes send nothing.
            ChannelConfig {
                channel_id: INTERNAL_RELIABLE,
                max_memory_usage_bytes: CHANNEL_MEMORY,
                send_type: SendType::ReliableOrdered { resend_time: INTERVAL },
            },
            ChannelConfig {
                channel_id: INTERNAL_UNRELIABLE,
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
            last_sent: (server_nonce != 0).then_some(now),
            renet,
            next_unreliable: 0,
            received_unreliable: None,
            cookie: [0; COOKIE],
        }
    }
    /// A hello's payload: the cookie, then zeros up to `HELLO_BYTES`.
    fn hello(&self) -> [u8; HELLO_BYTES - HEADER] {
        let mut payload = [0; HELLO_BYTES - HEADER];
        payload[..COOKIE].copy_from_slice(&self.cookie);
        payload
    }
}

/// The stateless address check of a UDP host (book/src/security.md,
/// "Forged hellos"). A hello without a valid cookie gets a challenge with
/// one and nothing else; the cookie is a keyed MAC over the client's
/// address and port, the protocol version, the client's nonce and the key
/// period, so the host keeps nothing per stranger and a cookie seen from
/// another address, or an old one, is worthless.
struct Cookies {
    current: [u8; 16],
    previous: [u8; 16],
    /// The key period of `current`: protocol seconds / `COOKIE_PERIOD`.
    period: u64,
}
impl Cookies {
    fn new() -> Self {
        Self {
            current: secret(),
            previous: secret(),
            period: 0,
        }
    }
    /// Moves to the key period of `now`: the current key becomes the
    /// previous one (or both are new after a longer gap).
    fn rotate(&mut self, now: Duration) {
        let period = now.as_secs() / COOKIE_PERIOD;
        if period == self.period {
            return;
        }
        self.previous = if period == self.period + 1 {
            self.current
        } else {
            secret()
        };
        self.current = secret();
        self.period = period;
    }
    fn tag(key: &[u8; 16], addr: SocketAddr, client_nonce: u64, period: u32) -> [u8; 16] {
        use siphasher::sip128::Hasher128;
        let mut mac = siphasher::sip128::SipHasher24::new_with_key(key);
        let ip = match addr.ip() {
            std::net::IpAddr::V4(ip) => ip.to_ipv6_mapped(),
            std::net::IpAddr::V6(ip) => ip,
        };
        mac.write(&[MAGIC[3]]);
        mac.write(&ip.octets());
        mac.write(&addr.port().to_le_bytes());
        mac.write(&client_nonce.to_le_bytes());
        mac.write(&period.to_le_bytes());
        mac.finish128().as_bytes()
    }
    fn issue(&mut self, now: Duration, addr: SocketAddr, client_nonce: u64) -> [u8; COOKIE] {
        self.rotate(now);
        let period = self.period as u32;
        let mut cookie = [0; COOKIE];
        cookie[..4].copy_from_slice(&period.to_le_bytes());
        cookie[4..].copy_from_slice(&Self::tag(&self.current, addr, client_nonce, period));
        cookie
    }
    fn check(&mut self, now: Duration, addr: SocketAddr, client_nonce: u64, cookie: &[u8; COOKIE]) -> bool {
        use subtle::ConstantTimeEq;
        self.rotate(now);
        let period = u32::from_le_bytes([cookie[0], cookie[1], cookie[2], cookie[3]]);
        let key = if u64::from(period) == self.period {
            &self.current
        } else if u64::from(period) + 1 == self.period {
            &self.previous
        } else {
            return false;
        };
        Self::tag(key, addr, client_nonce, period).ct_eq(&cookie[4..]).into()
    }
}
/// Token buckets for answers to strangers (book/src/security.md, "The
/// challenge budget"): one for all of them, and one per network in a
/// fixed table, so that a flood from one network can't use up the answers
/// meant for others. Nothing in it allocates or outlives a second. An
/// endpoint makes its budget (2.6 KB, boxed) when it first answers a
/// stranger: a client never does, and thousands of them in one process
/// (the lobby's load test) would otherwise carry one each.
struct StrangerBudget {
    global: Bucket,
    networks: [(u64, Bucket); NETWORK_BUDGETS],
}
/// Time's worth of answers, refilled with protocol time up to one second.
#[derive(Clone, Copy)]
struct Bucket {
    allowance: Duration,
    at: Duration,
}
impl Bucket {
    const FULL: Self = Self {
        allowance: Duration::from_secs(1),
        at: Duration::ZERO,
    };
    fn refill(&mut self, now: Duration) {
        let since = now.saturating_sub(self.at);
        self.at = now;
        self.allowance = (self.allowance + since).min(Duration::from_secs(1));
    }
}
impl StrangerBudget {
    fn new() -> Self {
        Self {
            global: Bucket::FULL,
            networks: [(0, Bucket::FULL); NETWORK_BUDGETS],
        }
    }
    /// The network of `addr`: its IPv4 /24 or IPv6 /56, tagged with the
    /// family; never zero (zero marks a free entry).
    fn network(addr: SocketAddr) -> u64 {
        match addr.ip().to_canonical() {
            std::net::IpAddr::V4(ip) => (u64::from(u32::from(ip) & 0xFFFF_FF00) << 8) | 4,
            std::net::IpAddr::V6(ip) => (((u128::from(ip) >> 64) as u64) & !0xFF) | 6,
        }
    }
    /// Whether an entry still holds anything: one that last answered a
    /// second or more ago has a full budget again, so it is free.
    fn live(entry: &(u64, Bucket), now: Duration) -> bool {
        entry.0 != 0 && now.saturating_sub(entry.1.at) < Duration::from_secs(1)
    }
    /// Takes one answer to `addr` from its network's budget and the global
    /// one, if both have it. Without a free entry for a new network, only
    /// the global budget counts.
    fn take(&mut self, now: Duration, addr: SocketAddr) -> bool {
        let network = Self::network(addr);
        let entry = match self.networks.iter().position(|e| e.0 == network) {
            Some(i) => Some(i),
            None => self.networks.iter().position(|e| !Self::live(e, now)),
        };
        let network_cost = Duration::from_secs(1) / NETWORK_ANSWERS_PER_SECOND;
        if let Some(i) = entry {
            let (key, bucket) = &mut self.networks[i];
            if *key != network {
                *key = network;
                *bucket = Bucket::FULL;
            }
            bucket.refill(now);
            if bucket.allowance < network_cost {
                return false;
            }
        }
        let cost = Duration::from_secs(1) / STRANGER_ANSWERS_PER_SECOND;
        self.global.refill(now);
        if self.global.allowance < cost {
            return false;
        }
        self.global.allowance -= cost;
        if let Some(i) = entry {
            self.networks[i].1.allowance -= network_cost;
        }
        true
    }
    /// Networks with an entry in use at `now`.
    #[cfg(test)]
    fn live_networks(&self, now: Duration) -> usize {
        self.networks.iter().filter(|e| Self::live(e, now)).count()
    }
}

/// A cookie key: from the OS's secure random source where there is one.
fn secret() -> [u8; 16] {
    let mut key = [0; 16];
    #[cfg(any(unix, windows))]
    if getrandom::fill(&mut key).is_ok() {
        return key;
    }
    key[..8].copy_from_slice(&nonce().to_le_bytes());
    key[8..].copy_from_slice(&nonce().to_le_bytes());
    key
}
/// Datagrams waiting for an in-memory endpoint: sender, bytes, and the
/// time they may arrive (zero: at once).
type Inbox = VecDeque<(SocketAddr, Vec<u8>, Duration)>;

/// What an in-memory link does to datagrams
/// ([`NetMessages::loopback_link`]): loses a share of them, and delays
/// each by a fixed time plus a random part, so that some overtake others.
/// The dice are a hash of the seed, the sender and the datagram's number:
/// the same seed loses and delays the same datagrams on every run.
///
/// ```
/// use std::time::Duration;
/// let link = ponko::net::LoopbackLink::new()
///     .loss(0.05)
///     .delay(Duration::from_millis(30))
///     .jitter(Duration::from_millis(50))
///     .seed(7);
/// assert_ne!(link, ponko::net::LoopbackLink::new());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoopbackLink {
    loss: f64,
    delay: Duration,
    jitter: Duration,
    seed: u64,
}

impl LoopbackLink {
    /// A perfect link: nothing lost, nothing delayed.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loses this share of the datagrams, `0.0..=1.0`.
    ///
    /// # Panics
    ///
    /// If `share` is outside `0.0..=1.0`.
    #[must_use]
    pub fn loss(mut self, share: f64) -> Self {
        assert!((0.0..=1.0).contains(&share), "loss must be a share from 0.0 to 1.0");
        self.loss = share;
        self
    }

    /// Every datagram arrives this long after it was sent, at the
    /// earliest.
    #[must_use]
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// And up to this much later, picked per datagram: a datagram delayed
    /// more than the gap to the next one arrives after it.
    #[must_use]
    pub fn jitter(mut self, jitter: Duration) -> Self {
        self.jitter = jitter;
        self
    }

    /// Another seed picks other datagrams to lose and delay.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// When datagram number `n` from endpoint `me`, sent at `now`, arrives;
    /// `None` if it is lost.
    fn arrival(&self, me: usize, n: u64, now: Duration) -> Option<Duration> {
        let roll = |salt: u64| {
            let z = mix(self.seed ^ mix((me as u64) << 32 ^ n) ^ salt);
            (z >> 11) as f64 / (1u64 << 53) as f64
        };
        if self.loss > 0.0 && roll(1) < self.loss {
            return None;
        }
        if self.delay.is_zero() && self.jitter.is_zero() {
            return Some(Duration::ZERO);
        }
        Some(now + self.delay + self.jitter.mul_f64(roll(2)))
    }
}

/// A fixed scramble of `n` (splitmix64): the loopback link's dice.
fn mix(n: u64) -> u64 {
    let mut z = n.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
/// Packet boundary below the renet protocol and session handshake.
///
/// Implementations: the Steam relay (feature `steam`), WebRTC data channels
/// (feature `webrtc`) and the scripted transport used by the conformance
/// tests in `net/message_conformance.rs`, which also lists the contract each
/// transport has to keep.
#[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
pub(super) trait PacketTransport: Send {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool>;
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>>;
    /// Protocol time, before each poll's receives: for transports with
    /// timers of their own (WebRTC), so they follow `poll_elapsed`'s clock.
    fn advance(&mut self, _now: Duration) {}
    /// After each poll's sends: for transports that queue packets for a
    /// carrier of their own (the lobby relay), so they leave in this poll.
    fn flush(&mut self) {}
    /// The signaling peer behind a session address (WebRTC).
    #[cfg(feature = "webrtc")]
    fn signal_peer(&self, _addr: SocketAddr) -> Option<u64> {
        None
    }
    #[cfg(feature = "steam")]
    fn lobby(&self) -> Option<&super::SteamLobby> {
        None
    }
    /// The lobby connection under a relay endpoint.
    #[cfg(feature = "lobby")]
    fn relay_lobby(&mut self) -> Option<&mut super::lobby::LobbyClient> {
        None
    }
}

enum Wire {
    #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
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
        link: LoopbackLink,
        sent: u64,
        /// The endpoint's protocol time at its last poll.
        now: Duration,
    },
}
impl Wire {
    fn addr(&self) -> io::Result<SocketAddr> {
        match self {
            #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
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
            #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
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
                link,
                sent,
                now,
            } => {
                *sent += 1;
                if *drop_one_in != 0 && sent.is_multiple_of(u64::from(*drop_one_in)) {
                    return Ok(true);
                }
                let Some(due) = link.arrival(*me, *sent, *now) else {
                    return Ok(true);
                };
                let mut queues = inboxes.lock().expect("loopback inbox");
                if let Some(queue) = queues.get_mut(usize::from(addr.port() - 1))
                    && queue.len() < 4096
                {
                    queue.push_back((local_addr(*me), bytes.to_vec(), due));
                }
                Ok(true)
            }
        }
    }
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        match self {
            #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
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
            Self::Local { me, inboxes, now, .. } => {
                // Reverse arrival intentionally exercises packet reordering:
                // the newest datagram that has arrived goes first.
                let mut queues = inboxes.lock().expect("loopback inbox");
                let queue = &mut queues[*me];
                Ok(queue
                    .iter()
                    .rposition(|&(_, _, due)| due <= *now)
                    .and_then(|i| queue.remove(i))
                    .map(|(addr, bytes, _)| {
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
/// A random number from the OS-seeded hasher, for session discriminators.
#[cfg(feature = "webrtc")]
pub(super) fn random_u64() -> u64 {
    nonce()
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
    has_polled: bool,
    /// Polled by a game on its own clock ([`game_clock`](Self::game_clock)):
    /// the game time of the last poll.
    game_clock: bool,
    game_polled: Option<f64>,
    timeout: Duration,
    events: VecDeque<MessageEvent>,
    event_bytes: usize,
    /// Messages on the internal lanes, for the replication layer:
    /// (sender, reliable, payload).
    internal: VecDeque<(PeerId, bool, renet::Bytes)>,
    stats: MessageStats,
    scratch: Vec<u8>,
    /// A UDP host's address check (`None` on other transports).
    cookies: Option<Cookies>,
    /// How many answers to strangers (challenges, version refusals) may
    /// still be sent, in all and per network. Only a host answers them, so
    /// only a host has one (2.6 KB, made with it): a client endpoint is a
    /// fraction of that.
    stranger_budget: Option<Box<StrangerBudget>>,
}
impl NetMessages {
    #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
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
    #[cfg(feature = "webrtc")]
    pub(super) fn transport_signal_peer(&self, peer: PeerId) -> Option<u64> {
        let link = self.links.get(&peer).filter(|l| l.connected)?;
        match &self.wire {
            Wire::Custom(t) => t.signal_peer(link.addr),
            _ => None,
        }
    }
    #[cfg(feature = "lobby")]
    pub(super) fn transport_relay_lobby(&mut self) -> Option<&mut super::lobby::LobbyClient> {
        match &mut self.wire {
            Wire::Custom(t) => t.relay_lobby(),
            _ => None,
        }
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
            has_polled: false,
            game_clock: false,
            game_polled: None,
            timeout: Duration::from_secs(10),
            events: VecDeque::new(),
            event_bytes: 0,
            internal: VecDeque::new(),
            stats: MessageStats::default(),
            scratch: Vec::with_capacity(1500),
            cookies: None,
            stranger_budget: hosting.then(|| Box::new(StrangerBudget::new())),
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
        let mut net = Self::new(Wire::Udp(socket), true, max_clients);
        net.cookies = Some(Cookies::new());
        Ok(net)
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
        Self::loopback_on(players, drop_one_in, LoopbackLink::new())
    }
    /// Like `loopback`, over a link that loses and delays datagrams as
    /// `link` says, the same ones on every run. The delays count on the
    /// endpoints' protocol clocks, from the sender's last poll to the
    /// receiver's: poll them together, every frame, as games do, and on
    /// game time ([`game_clock`](Self::game_clock)) for runs that repeat
    /// exactly.
    ///
    /// # Panics
    ///
    /// Unless `players` is 2..=1025.
    ///
    /// ```
    /// use ponko::net::{LoopbackLink, NetMessages};
    /// let link = LoopbackLink::new().loss(0.05).jitter(std::time::Duration::from_millis(50));
    /// let nets = NetMessages::loopback_link(3, link);
    /// assert_eq!(nets.len(), 3);
    /// ```
    pub fn loopback_link(players: usize, link: LoopbackLink) -> Vec<Self> {
        Self::loopback_on(players, 0, link)
    }
    fn loopback_on(players: usize, drop_one_in: u32, link: LoopbackLink) -> Vec<Self> {
        assert!((2..=1025).contains(&players), "players must be 2..=1025");
        let inboxes = Arc::new(Mutex::new((0..players).map(|_| VecDeque::new()).collect()));
        (0..players)
            .map(|me| {
                let mut net = Self::new(
                    Wire::Local {
                        me,
                        inboxes: inboxes.clone(),
                        drop_one_in,
                        link,
                        sent: me as u64,
                        now: Duration::ZERO,
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
    /// When a game polls this endpoint ([`Game::messages`](crate::game::Game::messages)),
    /// count the game's time instead of the wall clock: resends, timeouts
    /// and pacing follow the frames the game runs. A headless run goes as
    /// fast as it can, so on the wall clock it would see no time pass
    /// between frames; on the game clock it plays the same every time. For
    /// tests, bots and replays; real play keeps the wall clock (the
    /// default), so a slow frame doesn't slow the connection down.
    ///
    /// Polling it yourself ([`poll`](Self::poll)) still uses the wall clock.
    ///
    /// ```
    /// let net = ponko::net::NetMessages::loopback(2).remove(0).game_clock();
    /// assert!(net.is_on_game_clock());
    /// ```
    #[must_use]
    pub fn game_clock(mut self) -> Self {
        self.game_clock = true;
        self
    }
    /// Whether a game polls this endpoint on its own clock
    /// ([`game_clock`](Self::game_clock)).
    /// ```
    /// assert!(!ponko::net::NetMessages::loopback(2)[0].is_on_game_clock());
    /// ```
    #[must_use]
    pub fn is_on_game_clock(&self) -> bool {
        self.game_clock
    }
    /// A game's poll, once a frame: on the wall clock, or on the game's
    /// (`game_time`, seconds) with [`game_clock`](Self::game_clock).
    pub(crate) fn poll_frame(&mut self, game_time: f64) -> io::Result<()> {
        if !self.game_clock {
            return self.poll();
        }
        let step = self.game_polled.map_or(0.0, |t| (game_time - t).max(0.0));
        self.game_polled = Some(game_time);
        self.poll_elapsed(Duration::from_secs_f64(step))
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
    /// incoming datagrams per call. The clock starts at the first poll. Later
    /// gaps are bounded as in [`poll_elapsed`](Self::poll_elapsed).
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
    /// The first poll starts the clock and ignores the elapsed duration. A
    /// joining endpoint attempts its first hello immediately. Each later poll
    /// advances protocol time by at most half the configured timeout (with a
    /// minimum step limit of one nanosecond). This gives a peer time to reply
    /// after a stalled frame. Silence still accumulates across polls, and
    /// incoming packets are processed before checking for timeouts.
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
        let elapsed = if self.has_polled {
            elapsed.min((self.timeout / 2).max(Duration::from_nanos(1)))
        } else {
            self.has_polled = true;
            Duration::ZERO
        };
        let now = self.now.saturating_add(elapsed);
        let elapsed = now - self.now;
        self.now = now;
        if self.events.len() >= EVENT_LIMIT || self.event_bytes >= CHANNEL_MEMORY {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "drain message events before polling",
            ));
        }
        for link in self.links.values_mut() {
            link.renet.update(elapsed);
        }
        #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
        if let Wire::Custom(t) = &mut self.wire {
            t.advance(self.now);
        }
        if let Wire::Local { now, .. } = &mut self.wire {
            *now = self.now;
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
                    for (channel, reliable) in [(INTERNAL_RELIABLE, true), (INTERNAL_UNRELIABLE, false)] {
                        while self.internal.len() < INTERNAL_LIMIT {
                            let Some(message) = link.renet.receive_message(channel) else {
                                break;
                            };
                            self.internal.push_back((peer, reliable, message));
                        }
                    }
                    for packet in link.renet.get_packets_to_send() {
                        self.transmit(link, DATA, &packet)?;
                    }
                }
                if link
                    .last_sent
                    .is_none_or(|sent| self.now.saturating_sub(sent) >= INTERVAL)
                {
                    if link.connected {
                        self.transmit(link, HEARTBEAT, &[])?;
                    } else {
                        self.transmit(link, HELLO, &link.hello())?;
                    }
                    link.last_sent = Some(self.now);
                }
            }
            Ok(())
        })();
        links.retain(|_, link| !link.renet.is_disconnected());
        self.links = links;
        #[cfg(any(test, feature = "steam", feature = "webrtc", feature = "lobby"))]
        if let Wire::Custom(t) = &mut self.wire {
            t.flush();
        }
        result
    }
    fn transmit(&mut self, link: &Link, kind: u8, data: &[u8]) -> io::Result<()> {
        self.send_raw(link.addr, kind, link.client_nonce, link.server_nonce, data)
    }
    /// Frames and sends one datagram without a link (answers to strangers
    /// use only the reused scratch buffer, no allocation).
    fn send_raw(&mut self, addr: SocketAddr, kind: u8, cn: u64, sn: u64, data: &[u8]) -> io::Result<()> {
        self.scratch.clear();
        self.scratch.extend_from_slice(MAGIC);
        self.scratch.push(kind);
        self.scratch.extend_from_slice(&cn.to_le_bytes());
        self.scratch.extend_from_slice(&sn.to_le_bytes());
        self.scratch.extend_from_slice(data);
        if self.wire.send(addr, &self.scratch)? {
            self.stats.packets_sent += 1;
            self.stats.bytes_sent += self.scratch.len() as u64;
        }
        Ok(())
    }
    /// Whether a hello from `addr` has to carry a cookie: on a UDP host,
    /// for UDP addresses (browser clients come over a TCP connection, which
    /// proved the address already).
    fn needs_cookie(&self, addr: SocketAddr) -> bool {
        let _ = addr;
        self.cookies.is_some()
            && match &self.wire {
                #[cfg(not(target_arch = "wasm32"))]
                Wire::UdpWeb(..) => !super::websocket::WebListener::owns(addr),
                _ => true,
            }
    }
    /// Takes one answer to `addr`, a stranger, from the budgets, if they
    /// allow one (a host's; an endpoint that isn't one answers none).
    fn may_answer_stranger(&mut self, addr: SocketAddr) -> bool {
        let now = self.now;
        self.stranger_budget.as_mut().is_some_and(|b| b.take(now, addr))
    }
    fn receive(&mut self, addr: SocketAddr, data: &[u8]) -> io::Result<()> {
        if data.len() < HEADER || &data[..3] != MAGIC_FAMILY {
            return Ok(());
        }
        let kind = data[4];
        let cn = u64::from_le_bytes(data[5..13].try_into().expect("nonce"));
        let sn = u64::from_le_bytes(data[13..21].try_into().expect("nonce"));
        if &data[..4] != MAGIC {
            return self.receive_other_version(addr, data[3], kind, cn);
        }
        if self.hosting && kind == HELLO && cn != 0 && sn == 0 {
            if let Some((&peer, link)) = self.links.iter().find(|(_, l)| l.addr == addr) {
                if link.client_nonce == cn {
                    let mut link = self.links.remove(&peer).expect("link");
                    link.last_seen = self.now;
                    let result = self.transmit(&link, WELCOME, &[]);
                    self.links.insert(peer, link);
                    result?;
                }
                return Ok(());
            }
            if self.needs_cookie(addr) {
                // An unpadded hello could make the challenge an amplifier.
                let cookie = data.get(HEADER..HEADER + COOKIE).filter(|_| data.len() >= HELLO_BYTES);
                let Some(cookie) = cookie.and_then(|c| <&[u8; COOKIE]>::try_from(c).ok()) else {
                    return Ok(());
                };
                let Some(cookies) = self.cookies.as_mut() else {
                    return Ok(());
                };
                if !cookies.check(self.now, addr, cn, cookie) {
                    if self.may_answer_stranger(addr)
                        && let Some(cookies) = self.cookies.as_mut()
                    {
                        let cookie = cookies.issue(self.now, addr, cn);
                        return self.send_raw(addr, CHALLENGE, cn, 0, &cookie);
                    }
                    return Ok(());
                }
            }
            if self.links.len() >= self.max_clients {
                // FULL uses the nonce the client already knows.
                return self.send_raw(addr, FULL, cn, 0, &[]);
            }
            let link = Link::new(addr, cn, nonce(), self.now);
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
        if !self.hosting && !link.connected && kind == CHALLENGE && sn == 0 {
            // Echo the cookie at once: one extra round trip, no more.
            let Some(cookie) = data.get(HEADER..HEADER + COOKIE) else {
                return Ok(());
            };
            link.cookie.copy_from_slice(cookie);
            link.last_sent = Some(self.now);
            let payload = link.hello();
            let (cn, sn, to) = (link.client_nonce, link.server_nonce, link.addr);
            return self.send_raw(to, HELLO, cn, sn, &payload);
        }
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
    /// A datagram of another protocol version (`digit` after the family
    /// magic). A host answers a hello with one refusal no larger than it;
    /// a joining client takes a host's refusal as the end of the join.
    fn receive_other_version(&mut self, addr: SocketAddr, digit: u8, kind: u8, cn: u64) -> io::Result<()> {
        if self.hosting {
            if kind == HELLO && cn != 0 && self.may_answer_stranger(addr) {
                return self.send_raw(addr, WRONG_VERSION, cn, 0, &[]);
            }
            return Ok(());
        }
        if kind != WRONG_VERSION {
            return Ok(());
        }
        let Some((&peer, _)) = self
            .links
            .iter()
            .find(|(_, l)| l.addr == addr && l.client_nonce == cn && !l.connected)
        else {
            return Ok(());
        };
        self.links.remove(&peer);
        let theirs = char::from(digit);
        let ours = char::from(MAGIC[3]);
        self.events.push_back(MessageEvent::Disconnected(
            peer,
            format!("version mismatch: the host speaks message protocol {theirs}, this endpoint {ours}"),
        ));
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
    /// The address a connected peer's packets come from, for a server's
    /// own limits per address (`None` for an unknown or still joining
    /// peer). A browser peer of [`accept_web`](Self::accept_web) has an
    /// address of its own per connection, in a private IPv6 range, not the
    /// browser's; an in-memory peer has `127.0.0.1` and its index plus one
    /// as the port.
    /// ```
    /// use ponko::net::{NetMessages, PeerId};
    /// let mut nets = NetMessages::loopback(2);
    /// for _ in 0..40 {
    ///     nets.iter_mut().for_each(|n| n.poll_elapsed(std::time::Duration::from_millis(20)).unwrap());
    /// }
    /// let addr = nets[1].peer_addr(PeerId::HOST).unwrap();
    /// assert_eq!((addr.ip().to_string(), addr.port()), ("127.0.0.1".to_string(), 1));
    /// assert!(nets[1].peer_addr(PeerId::from_bits(9)).is_none());
    /// ```
    pub fn peer_addr(&self, peer: PeerId) -> Option<SocketAddr> {
        self.links.get(&peer).filter(|l| l.connected).map(|l| l.addr)
    }

    /// Where a connected browser peer of [`accept_web`](Self::accept_web)
    /// comes from: its TCP address and the `X-Forwarded-For` header of its
    /// upgrade request, for a server behind a reverse proxy that limits per
    /// client ([`WebPeer::client_ip`](super::WebPeer::client_ip)). `None`
    /// for any other peer (UDP, in memory, unknown) and in a browser.
    /// ```
    /// let nets = ponko::net::NetMessages::loopback(2);
    /// assert!(nets[1].web_peer(ponko::net::PeerId::HOST).is_none());
    /// ```
    pub fn web_peer(&self, peer: PeerId) -> Option<super::WebPeer> {
        let addr = self.links.get(&peer).filter(|l| l.connected)?.addr;
        #[cfg(not(target_arch = "wasm32"))]
        if let Wire::UdpWeb(_, web, _) = &self.wire {
            return web.peer(addr);
        }
        let _ = addr;
        None
    }

    /// Whether this endpoint hosts (from [`host`](Self::host) or the first
    /// of [`loopback`](Self::loopback)) rather than joins.
    /// ```
    /// let nets = ponko::net::NetMessages::loopback(2);
    /// assert!(nets[0].is_host() && !nets[1].is_host());
    /// ```
    pub fn is_host(&self) -> bool {
        self.hosting
    }

    /// The peers connected now, in id order.
    pub(crate) fn peer_ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.links.iter().filter(|(_, l)| l.connected).map(|(&p, _)| p)
    }

    /// Queues `bytes` on an internal lane (the replication layer's).
    pub(crate) fn send_internal(&mut self, peer: PeerId, reliable: bool, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_MESSAGE_BYTES {
            return Err(invalid("message exceeds 1 MiB"));
        }
        let link = self
            .links
            .get_mut(&peer)
            .filter(|l| l.connected)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "peer is not connected"))?;
        let channel = if reliable {
            INTERNAL_RELIABLE
        } else {
            INTERNAL_UNRELIABLE
        };
        if !link.renet.can_send_message(channel, bytes.len()) {
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "message lane is full"));
        }
        link.renet.send_message(channel, bytes.to_vec());
        Ok(())
    }

    /// The oldest message received on an internal lane: (sender, reliable, bytes).
    pub(crate) fn next_internal(&mut self) -> Option<(PeerId, bool, renet::Bytes)> {
        self.internal.pop_front()
    }

    /// Queues `bytes` as an internal message from `peer`, as if it had
    /// arrived over the link (the replication fuzz target, `net/fuzz.rs`).
    #[cfg(any(test, fuzzing))]
    pub(crate) fn fuzz_internal(&mut self, peer: PeerId, reliable: bool, bytes: &[u8]) {
        if self.internal.len() < INTERNAL_LIMIT {
            self.internal
                .push_back((peer, reliable, renet::Bytes::copy_from_slice(bytes)));
        }
    }
}

/// Hooks for the fuzzing entry points in `net/fuzz.rs`.
#[cfg(any(test, fuzzing))]
impl NetMessages {
    /// The address of loopback endpoint `index`.
    pub(super) fn loopback_addr(index: usize) -> SocketAddr {
        local_addr(index)
    }

    /// Makes an in-memory host check address cookies, as a UDP host does.
    pub(super) fn require_cookies(mut self) -> Self {
        if self.hosting {
            self.cookies = Some(Cookies::new());
        }
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Datagrams 0..1000 from endpoint 1 to endpoint 0 of a loopback over
    /// `link`, one per 60 Hz frame; the numbers in the order they arrived.
    fn through(link: LoopbackLink) -> Vec<u32> {
        let mut nets = NetMessages::loopback_link(2, link);
        let set_now = |net: &mut NetMessages, t: Duration| {
            if let Wire::Local { now, .. } = &mut net.wire {
                *now = t;
            }
        };
        let frame = Duration::from_micros(16_667);
        let mut got = Vec::new();
        let mut buf = [0u8; 1500];
        for i in 0..1100u32 {
            let t = frame * i;
            set_now(&mut nets[0], t);
            set_now(&mut nets[1], t);
            if i < 1000 {
                nets[1].wire.send(local_addr(0), &i.to_le_bytes()).unwrap();
            }
            while let Some((_, n)) = nets[0].wire.recv(&mut buf).unwrap() {
                got.push(u32::from_le_bytes(buf[..n].try_into().unwrap()));
            }
        }
        got
    }

    #[test]
    fn a_loopback_link_loses_and_reorders_the_same_datagrams_on_every_run() {
        let link = LoopbackLink::new()
            .loss(0.1)
            .delay(Duration::from_millis(20))
            .jitter(Duration::from_millis(50))
            .seed(3);
        let got = through(link);
        assert_eq!(got, through(link), "the same seed, the same run");
        assert_ne!(got, through(link.seed(4)), "another seed, another run");
        // About a tenth lost, none twice.
        assert!((850..=950).contains(&got.len()), "{} of 1000 arrived", got.len());
        let mut sorted = got.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), got.len(), "each arrives once");
        // Up to three frames of jitter: many overtake an earlier one.
        let overtaken = got.windows(2).filter(|w| w[1] < w[0]).count();
        assert!(overtaken > 100, "{overtaken} arrived after a later one");
        // Negative control: a perfect link delivers all, in order (one a
        // frame, so the newest-first order within a poll never shows).
        assert_eq!(through(LoopbackLink::new()), (0..1000).collect::<Vec<_>>());
    }

    #[test]
    #[should_panic(expected = "loss must be a share from 0.0 to 1.0")]
    fn a_loss_above_one_is_refused() {
        let _ = LoopbackLink::new().loss(1.5);
    }

    #[test]
    fn first_wall_clock_poll_sends_hello_after_long_startup() {
        let mut client = NetMessages::loopback(2).remove(1);
        client.last_poll -= Duration::from_secs(30);

        client.poll().unwrap();

        assert_eq!(client.next_event(), None);
        assert_eq!(client.stats().packets_sent, 1, "first poll sends a hello");
    }

    #[test]
    fn first_explicit_poll_sends_hello_after_long_startup() {
        let mut nets = NetMessages::loopback(2);
        for net in nets.iter_mut().rev() {
            net.poll_elapsed(Duration::from_secs(30)).unwrap();
        }
        nets[1].poll_elapsed(Duration::ZERO).unwrap();

        assert!(matches!(nets[0].next_event(), Some(MessageEvent::Connected(_))));
        assert_eq!(nets[1].next_event(), Some(MessageEvent::Connected(PeerId::HOST)));
    }

    /// A version-2 hello from a client with nonce `cn`, carrying `cookie`.
    fn hello(cn: u64, cookie: &[u8; COOKIE]) -> Vec<u8> {
        let mut packet = MAGIC.to_vec();
        packet.push(HELLO);
        packet.extend_from_slice(&cn.to_le_bytes());
        packet.extend_from_slice(&0u64.to_le_bytes());
        packet.extend_from_slice(cookie);
        packet.resize(HELLO_BYTES, 0);
        packet
    }

    /// A loopback host that checks cookies, polled once (its clock runs).
    fn cookie_host() -> NetMessages {
        let mut host = NetMessages::loopback(2).remove(0).require_cookies();
        host.poll_elapsed(Duration::ZERO).unwrap();
        host
    }

    /// An address no loopback endpoint has: what it is sent goes nowhere.
    fn forged(i: u32) -> SocketAddr {
        SocketAddr::from((
            [10, (i >> 16) as u8, (i >> 8) as u8, i as u8],
            40_000 + (i % 1000) as u16,
        ))
    }

    #[test]
    fn forged_hellos_get_a_small_challenge_and_leave_no_state() {
        let mut host = cookie_host();
        let capacity = host.scratch.capacity();
        for i in 0..10_000u32 {
            let packet = hello(u64::from(i) + 1, &[0; COOKIE]);
            host.receive(forged(i), &packet).unwrap();
            // Garbage in the cookie field is no better than none.
            host.receive(forged(i), &hello(u64::from(i) + 1, &[0xAB; COOKIE]))
                .unwrap();
        }
        assert!(host.links.is_empty(), "no connection slot for an unproven address");
        assert_eq!(host.next_event(), None);
        assert_eq!(host.scratch.capacity(), capacity, "answers reuse the scratch buffer");
        let stats = host.stats();
        assert_eq!(
            stats.packets_sent,
            u64::from(STRANGER_ANSWERS_PER_SECOND),
            "challenges are capped per second"
        );
        assert_eq!(stats.bytes_sent, stats.packets_sent * (HEADER + COOKIE) as u64);
        // The allowance refills with time.
        host.poll_elapsed(Duration::from_secs(1)).unwrap();
        host.receive(forged(1), &hello(2, &[0; COOKIE])).unwrap();
        assert_eq!(host.stats().packets_sent, stats.packets_sent + 1);
    }

    #[test]
    fn short_hellos_get_no_challenge_from_a_cookie_host() {
        let mut host = cookie_host();
        let mut short = hello(7, &[0; COOKIE]);
        short.truncate(HEADER + COOKIE);
        host.receive(forged(1), &short).unwrap();
        short.truncate(HEADER);
        host.receive(forged(1), &short).unwrap();
        assert_eq!(
            host.stats().packets_sent,
            0,
            "an answer could outgrow an unpadded hello"
        );
        assert!(host.links.is_empty());
    }

    #[test]
    fn a_valid_cookie_opens_the_connection() {
        let mut host = cookie_host();
        let addr = forged(5);
        let cookie = host.cookies.as_mut().unwrap().issue(host.now, addr, 9);
        host.receive(addr, &hello(9, &cookie)).unwrap();
        assert!(matches!(host.next_event(), Some(MessageEvent::Connected(_))));
        assert_eq!(host.links.len(), 1);
    }

    #[test]
    fn a_cookie_replayed_from_another_address_is_refused() {
        let mut host = cookie_host();
        let cookie = host.cookies.as_mut().unwrap().issue(host.now, forged(5), 9);
        let mut other_port = forged(5);
        other_port.set_port(other_port.port() + 1);
        for addr in [forged(6), other_port] {
            host.receive(addr, &hello(9, &cookie)).unwrap();
        }
        // Another client nonce from the right address: a different connection attempt.
        host.receive(forged(5), &hello(10, &cookie)).unwrap();
        // A changed tag.
        let mut tampered = cookie;
        tampered[COOKIE - 1] ^= 1;
        host.receive(forged(5), &hello(9, &tampered)).unwrap();
        assert!(host.links.is_empty());
        assert_eq!(host.next_event(), None);
        assert_eq!(host.stats().packets_sent, 4, "each got a fresh challenge instead");
    }

    #[test]
    fn a_cookie_is_good_for_one_key_period_after_its_own_and_no_longer() {
        let addr = forged(5);
        // Still good 9.9 s later: made at the start of a period, used at
        // the end of the next.
        let mut host = cookie_host();
        host.poll_elapsed(Duration::from_secs(5)).unwrap();
        let cookie = host.cookies.as_mut().unwrap().issue(host.now, addr, 9);
        for _ in 0..2 {
            host.poll_elapsed(Duration::from_millis(4_950)).unwrap();
        }
        host.receive(addr, &hello(9, &cookie)).unwrap();
        assert_eq!(host.links.len(), 1, "a cookie from the previous period is good");

        // Expired after two key changes (at most 10 s).
        let mut host = cookie_host();
        let cookie = host.cookies.as_mut().unwrap().issue(host.now, addr, 9);
        for _ in 0..2 {
            host.poll_elapsed(Duration::from_secs(5)).unwrap();
        }
        host.receive(addr, &hello(9, &cookie)).unwrap();
        assert!(host.links.is_empty(), "an expired cookie is refused");
        assert_eq!(host.stats().packets_sent, 1, "and answered with a new challenge");
    }

    #[test]
    fn a_cookie_of_a_restarted_host_is_refused() {
        let mut first = cookie_host();
        let cookie = first.cookies.as_mut().unwrap().issue(first.now, forged(5), 9);
        let mut second = cookie_host();
        second.receive(forged(5), &hello(9, &cookie)).unwrap();
        assert!(second.links.is_empty(), "keys are made at start");
    }

    #[test]
    fn a_cookie_client_connects_in_one_extra_round_trip() {
        let mut nets = NetMessages::loopback(2);
        let host = nets.remove(0).require_cookies();
        nets.insert(0, host);
        let mut hops = 0;
        // Client hello, host challenge, client hello with the cookie, host
        // welcome, client connected: two hops more than without cookies.
        for (i, side) in [1, 0, 1, 0, 1].into_iter().enumerate() {
            nets[side].poll_elapsed(Duration::ZERO).unwrap();
            hops = i + 1;
            if nets[1].connected_peers() == 1 {
                break;
            }
        }
        assert_eq!(hops, 5);
        assert_eq!(nets[1].next_event(), Some(MessageEvent::Connected(PeerId::HOST)));
        assert!(matches!(nets[0].next_event(), Some(MessageEvent::Connected(_))));
        assert_eq!((nets[0].stats().packets_sent, nets[1].stats().packets_sent), (2, 2));

        // Negative control: without cookies it takes three hops.
        let mut nets = NetMessages::loopback(2);
        for side in [1, 0, 1] {
            nets[side].poll_elapsed(Duration::ZERO).unwrap();
        }
        assert_eq!(nets[1].connected_peers(), 1);
    }

    /// Polls a cookie host (`nets[0]`) and a joining client (`nets[1]`)
    /// in turn, protocol time standing still, for at most `limit` hops;
    /// returns the hops it took the client to connect.
    fn hops_to_connect(nets: &mut [NetMessages], limit: usize) -> Option<usize> {
        for (i, side) in [1, 0].into_iter().cycle().take(limit).enumerate() {
            nets[side].poll_elapsed(Duration::ZERO).unwrap();
            if nets[1].connected_peers() == 1 {
                return Some(i + 1);
            }
        }
        None
    }

    #[test]
    fn a_flood_from_one_network_leaves_challenges_for_joins_from_others() {
        let flood = |host: &mut NetMessages, addr: fn(u32) -> SocketAddr| {
            for i in 0..10_000u32 {
                host.receive(addr(i), &hello(u64::from(i) + 1, &[0; COOKIE])).unwrap();
            }
        };
        // Every hello from one /24 (10.0.0.0/24), on many ports.
        let one_network = |i: u32| SocketAddr::from(([10, 0, 0, i as u8], 40_000 + (i % 1000) as u16));
        let mut nets = NetMessages::loopback(2);
        let mut host = nets.remove(0).require_cookies();
        host.poll_elapsed(Duration::ZERO).unwrap();
        flood(&mut host, one_network);
        assert_eq!(
            host.stats().packets_sent,
            u64::from(NETWORK_ANSWERS_PER_SECOND),
            "the flooding network gets its own budget and no more"
        );
        nets.insert(0, host);
        // The client (127.0.0.1) connects in the usual five hops: hello,
        // challenge, hello with the cookie, welcome, connected.
        assert_eq!(hops_to_connect(&mut nets, 9), Some(5));

        // Negative control: a flood spread over many networks does use up
        // the global budget, and the same join then gets no challenge.
        let mut nets = NetMessages::loopback(2);
        let mut host = nets.remove(0).require_cookies();
        host.poll_elapsed(Duration::ZERO).unwrap();
        flood(&mut host, forged);
        assert_eq!(host.stats().packets_sent, u64::from(STRANGER_ANSWERS_PER_SECOND));
        nets.insert(0, host);
        assert_eq!(hops_to_connect(&mut nets, 9), None);
    }

    #[test]
    fn the_network_budget_table_cannot_grow_and_forgets_within_a_second() {
        let mut host = cookie_host();
        // 10 000 hellos, each from a /24 of its own.
        for i in 0..10_000u32 {
            let addr = SocketAddr::from(([10, (i >> 8) as u8, i as u8, 1], 40_000));
            host.receive(addr, &hello(u64::from(i) + 1, &[0; COOKIE])).unwrap();
            assert!(host.stranger_budget.as_ref().map_or(0, |b| b.live_networks(host.now)) <= NETWORK_BUDGETS);
        }
        assert_eq!(
            host.stranger_budget.as_ref().map_or(0, |b| b.live_networks(host.now)),
            NETWORK_BUDGETS
        );
        assert_eq!(
            host.stats().packets_sent,
            u64::from(STRANGER_ANSWERS_PER_SECOND),
            "networks without an entry still count against the global budget"
        );
        assert_eq!(
            std::mem::size_of::<StrangerBudget>(),
            std::mem::size_of::<Bucket>() + NETWORK_BUDGETS * std::mem::size_of::<(u64, Bucket)>(),
            "the table is inline, with nothing behind a pointer"
        );
        host.poll_elapsed(Duration::from_millis(999)).unwrap();
        assert_eq!(
            host.stranger_budget.as_ref().map_or(0, |b| b.live_networks(host.now)),
            NETWORK_BUDGETS
        );
        host.poll_elapsed(Duration::from_millis(1)).unwrap();
        assert_eq!(
            host.stranger_budget.as_ref().map_or(0, |b| b.live_networks(host.now)),
            0,
            "nothing outlives a second"
        );
    }

    /// Only a host, the one endpoint that answers strangers, holds the
    /// budget: its clients never have one, and an endpoint without it is a
    /// fraction of its size.
    #[test]
    fn only_a_host_holds_a_stranger_budget() {
        let mut nets = NetMessages::loopback(2);
        assert!(hops_to_connect(&mut nets, 9).is_some(), "the client connects");
        assert!(nets[0].stranger_budget.is_some(), "the host has its budget");
        assert!(
            nets[1..].iter().all(|n| n.stranger_budget.is_none()),
            "a client answers no stranger"
        );
        assert!(std::mem::size_of::<NetMessages>() < std::mem::size_of::<StrangerBudget>());
        let mut host = cookie_host();
        host.receive(SocketAddr::from(([10, 0, 0, 1], 40_000)), &hello(1, &[0; COOKIE]))
            .unwrap();
        assert_eq!(host.stats().packets_sent, 1, "a UDP host answers a stranger's hello");
    }

    #[test]
    fn networks_are_ipv4_slash_24_and_ipv6_slash_56() {
        let net = |s: &str| StrangerBudget::network(SocketAddr::new(s.parse().unwrap(), 1));
        assert_eq!(net("192.0.2.1"), net("192.0.2.254"));
        assert_ne!(net("192.0.2.1"), net("192.0.3.1"));
        assert_eq!(net("2001:db8:0:ff::1"), net("2001:db8:0:1::2"));
        assert_ne!(net("2001:db8:0:100::1"), net("2001:db8:0:ff::1"));
        assert_eq!(
            net("::ffff:192.0.2.9"),
            net("192.0.2.1"),
            "IPv4-mapped addresses are IPv4"
        );
        assert_ne!(net("::"), net("0.0.0.0"), "the families never share a network");
        assert_ne!(net("0.0.0.0"), 0, "zero marks a free entry");
    }

    #[test]
    fn a_hello_of_another_version_gets_one_refusal_no_larger_than_it() {
        let mut host = cookie_host();
        let mut old = b"CRM1".to_vec();
        old.push(HELLO);
        old.extend_from_slice(&3u64.to_le_bytes());
        old.extend_from_slice(&0u64.to_le_bytes());
        host.receive(forged(1), &old).unwrap();
        assert!(host.links.is_empty());
        assert_eq!(host.stats().packets_sent, 1);
        assert_eq!(host.stats().bytes_sent, old.len() as u64);
        // Other kinds of another version get nothing; neither does a foreign magic.
        old[4] = DATA;
        host.receive(forged(1), &old).unwrap();
        old[..4].copy_from_slice(b"XXXX");
        old[4] = HELLO;
        host.receive(forged(1), &old).unwrap();
        assert_eq!(host.stats().packets_sent, 1);
    }

    #[test]
    fn a_joining_client_reports_a_hosts_version_refusal() {
        let mut client = NetMessages::loopback(2).remove(1);
        client.poll_elapsed(Duration::ZERO).unwrap();
        let cn = client.links[&PeerId::HOST].client_nonce;
        let mut refusal = b"CRM3".to_vec();
        refusal.push(WRONG_VERSION);
        refusal.extend_from_slice(&(cn + 1).to_le_bytes());
        refusal.extend_from_slice(&0u64.to_le_bytes());
        client.receive(local_addr(0), &refusal).unwrap();
        assert_eq!(
            client.next_event(),
            None,
            "another connection attempt's refusal is ignored"
        );
        refusal[5..13].copy_from_slice(&cn.to_le_bytes());
        client.receive(local_addr(0), &refusal).unwrap();
        match client.next_event() {
            Some(MessageEvent::Disconnected(PeerId::HOST, reason)) => {
                assert!(reason.starts_with("version mismatch"), "{reason}");
                assert!(reason.contains("protocol 3"), "{reason}");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(client.links.is_empty());
    }

    #[test]
    fn one_long_poll_preserves_a_healthy_link_on_both_sides() {
        for side in [0, 1] {
            let mut nets = NetMessages::loopback(2);
            nets[1].poll_elapsed(INTERVAL).unwrap();
            nets[0].poll_elapsed(INTERVAL).unwrap();
            nets[1].poll_elapsed(Duration::ZERO).unwrap();
            assert!(matches!(nets[0].next_event(), Some(MessageEvent::Connected(_))));
            assert_eq!(nets[1].next_event(), Some(MessageEvent::Connected(PeerId::HOST)));

            // Leave the other endpoint unpolled, with no reply in flight.
            let net = &mut nets[side];
            net.poll_elapsed(Duration::MAX).unwrap();
            assert_eq!(net.next_event(), None);
            assert_eq!(net.connected_peers(), 1);
        }
    }
}
