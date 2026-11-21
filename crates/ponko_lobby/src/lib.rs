//! A reference lobby server for Ponko games: rooms with short codes,
//! matchmaking queues, host migration, and a relay for players who can't
//! connect to each other directly.
//!
//! The `ponko-lobby` program runs it as a dedicated server; this library is
//! the same server for a program of your own, with your own
//! [`Matchmaker`] or [`HostMigration`] rule. Clients are the engine's
//! [`LobbyClient`](ponko::net::lobby::LobbyClient) (feature `lobby`); the
//! wire protocol is [`ponko::net::lobby::protocol`].
//!
//! It is a reference: it keeps everything in memory (nothing survives a
//! restart), knows no accounts (a member is a connection), and checks no
//! game rules. The relay forwards the game's bytes without reading them.
//! What it does limit, per address: requests and relayed bytes per second,
//! connections; and in size: requests, relay payloads, rooms, room text,
//! idle connections and idle rooms ([`LobbyLimits`]).
//!
//! It is also a signaling server for the engine's WebRTC endpoints (feature
//! `webrtc`): their offers and answers are relayed like any other bytes.
//! [`StunResponder`] answers STUN binding requests, so peers learn their
//! public address from the lobby itself.
//!
//! ```
//! use std::time::Duration;
//! use ponko::net::NetMessages;
//! use ponko::net::lobby::{LobbyClient, LobbyEvent};
//! use ponko_lobby::{LobbyLimits, LobbyServer};
//!
//! // A server and two clients in memory, on a clock of our own.
//! let mut endpoints = NetMessages::loopback(3);
//! let mut b = LobbyClient::new(endpoints.pop().unwrap());
//! let mut a = LobbyClient::new(endpoints.pop().unwrap());
//! let mut server = LobbyServer::new(LobbyLimits::default());
//! server.listen(endpoints.pop().unwrap());
//!
//! a.create_room(2).unwrap();
//! let mut code = None;
//! for _ in 0..50 {
//!     server.poll_elapsed(Duration::from_millis(10));
//!     for client in [&mut a, &mut b] {
//!         client.poll_elapsed(Duration::from_millis(10)).unwrap();
//!     }
//!     while let Some(event) = a.next_event() {
//!         if let LobbyEvent::Joined(room) = event {
//!             b.join_room(&room.code).unwrap();
//!             code = Some(room.code);
//!         }
//!     }
//! }
//! assert_eq!(server.room(code.as_deref().unwrap()).unwrap().members.len(), 2);
//! assert!(a.is_host() && !b.is_host());
//! ```

#![deny(missing_docs)]

pub mod load;
mod proxy;
mod stun;

pub use proxy::TrustedProxy;
pub use stun::{StunResponder, StunStats};

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::time::{Duration, Instant};

use ponko::net::lobby::protocol::{self, Frame, Reply, Request, ServerLimits};
use ponko::net::lobby::{LeaveReason, LobbyError, MemberId, RoomInfo};
use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};

/// What counts as one address for the limits per address.
/// ```
/// assert_eq!(ponko_lobby::AddressKey::default(), ponko_lobby::AddressKey::Ip);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddressKey {
    /// The IP address, whatever the port: every client behind one IP
    /// shares the budgets. A browser (WebSocket) connection is its own
    /// address, unless [`LobbyLimits::trusted_proxies`] names the reverse
    /// proxies in front of the server: then it is the client's IP address,
    /// from the proxy's `X-Forwarded-For` header.
    #[default]
    Ip,
    /// Every connection on its own (behind a proxy that hides the clients'
    /// addresses, or for load tests from one machine).
    Connection,
}

/// The server's limits. Every one has a default for a public server; the
/// guide's "Lobbies, matchmaking and relays" chapter explains them.
///
/// ```
/// use std::time::Duration;
/// let mut limits = ponko_lobby::LobbyLimits::default();
/// limits.max_room_size = 8;
/// limits.idle_timeout = Duration::from_secs(30);
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LobbyLimits {
    /// What counts as one address (default [`AddressKey::Ip`]).
    pub address_key: AddressKey,
    /// Reverse proxies in front of the WebSocket port whose
    /// `X-Forwarded-For` header the server believes (default none). A
    /// browser connection from one of them counts as the client address
    /// the proxies wrote ([`WebPeer::client_ip`](ponko::net::WebPeer::client_ip)),
    /// and the limits per address use that. With any proxy named, a
    /// browser connection from anywhere else counts as its own IP address
    /// (its header ignored); a trusted proxy's connection without a
    /// readable header counts on its own. Empty: every browser connection
    /// is an address of its own, and the header is never read.
    pub trusted_proxies: Vec<TrustedProxy>,
    /// Connections one address may hold (default 16); more are refused
    /// with [`LobbyError::TooManyConnections`] and closed.
    pub connections_per_address: u32,
    /// Requests (create, join, queue, ...) per second per address, on
    /// average (default 10).
    pub requests_per_second: f64,
    /// Requests an address may make at once before the rate applies
    /// (default 20). Over it, requests are refused with
    /// [`LobbyError::RateLimited`].
    pub request_burst: f64,
    /// Relayed payload bytes per second per address, on average (default
    /// 256 KiB). Over it, relay messages are dropped.
    pub relay_bytes_per_second: f64,
    /// Relayed bytes an address may send at once (default 512 KiB).
    pub relay_burst_bytes: f64,
    /// The largest request, in bytes (default 1 KiB); larger ones are
    /// refused with [`LobbyError::TooLarge`].
    pub max_request_bytes: usize,
    /// The largest relay payload (default 4 KiB); larger ones are dropped.
    pub max_relay_bytes: u32,
    /// The largest room (default 16).
    pub max_room_size: u16,
    /// The longest room text in bytes (default 256), and of a queue name.
    pub max_meta_bytes: u32,
    /// Rooms the server holds at most (default 10 000); more are refused
    /// with [`LobbyError::ServerFull`].
    pub max_rooms: usize,
    /// How long a connection may stay outside any room and queue before
    /// the server closes it (default 60 s).
    pub idle_timeout: Duration,
    /// How long a room may go without a request or relay message from any
    /// member before the server closes it (default 10 min); its members
    /// get [`LeaveReason::Idle`].
    pub room_idle_timeout: Duration,
    /// How often the matchmaker runs over every waiting queue, besides
    /// whenever someone joins it (default 1 s), for rules that change
    /// with waiting time.
    pub matchmaking_every: Duration,
}

impl Default for LobbyLimits {
    fn default() -> Self {
        Self {
            address_key: AddressKey::Ip,
            trusted_proxies: Vec::new(),
            connections_per_address: 16,
            requests_per_second: 10.0,
            request_burst: 20.0,
            relay_bytes_per_second: 256.0 * 1024.0,
            relay_burst_bytes: 512.0 * 1024.0,
            max_request_bytes: 1024,
            max_relay_bytes: 4096,
            max_room_size: 16,
            max_meta_bytes: 256,
            max_rooms: 10_000,
            idle_timeout: Duration::from_secs(60),
            room_idle_timeout: Duration::from_secs(600),
            matchmaking_every: Duration::from_secs(1),
        }
    }
}

/// A player waiting in a matchmaking queue, as a [`Matchmaker`] sees it.
/// ```
/// use ponko::net::lobby::MemberId;
/// let w = ponko_lobby::Waiting { member: MemberId::from_bits(1), rating: 1500, since: std::time::Duration::ZERO };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// Who.
    pub member: MemberId,
    /// The number the client queued with (a skill rating).
    pub rating: i32,
    /// When they started waiting, on the server's clock.
    pub since: Duration,
}

/// A game's matchmaking rule: which waiting players go into a room
/// together. The default, [`FillBySize`], takes them in arrival order.
///
/// The server calls it whenever someone joins a queue and every
/// [`matchmaking_every`](LobbyLimits::matchmaking_every) for queues with
/// players in them.
///
/// ```
/// use std::time::Duration;
/// use ponko_lobby::{Matchmaker, Waiting};
///
/// /// Pairs players whose ratings are within 100, or 400 after 10 s.
/// struct CloseRatings;
/// impl Matchmaker for CloseRatings {
///     fn form(&mut self, _mode: &str, size: u16, waiting: &[Waiting], now: Duration, groups: &mut Vec<Vec<usize>>) {
///         let mut used = vec![false; waiting.len()];
///         for i in 0..waiting.len() {
///             if used[i] { continue; }
///             let window = if now - waiting[i].since > Duration::from_secs(10) { 400 } else { 100 };
///             let mut group = vec![i];
///             for j in i + 1..waiting.len() {
///                 if !used[j] && group.len() < usize::from(size)
///                     && (waiting[j].rating - waiting[i].rating).abs() <= window {
///                     group.push(j);
///                 }
///             }
///             if group.len() == usize::from(size) {
///                 group.iter().for_each(|&k| used[k] = true);
///                 groups.push(group);
///             }
///         }
///     }
/// }
/// let server = ponko_lobby::LobbyServer::new(Default::default()).matchmaker(CloseRatings);
/// ```
pub trait Matchmaker: Send + 'static {
    /// Pushes groups of players for new rooms to `groups`, as indices into
    /// `waiting` (the queue `mode` for rooms of `size`, oldest first). A
    /// group becomes a room of `size` whose first player hosts it. Groups
    /// with fewer than 2 or more than `size` players, an index out of
    /// range or used twice are skipped.
    fn form(&mut self, mode: &str, size: u16, waiting: &[Waiting], now: Duration, groups: &mut Vec<Vec<usize>>);
}

/// The default matchmaking rule: first come, first served, a room as soon
/// as `size` players wait in the same queue.
/// ```
/// use ponko_lobby::{FillBySize, Matchmaker, Waiting};
/// use ponko::net::lobby::MemberId;
/// let waiting: Vec<Waiting> = (1..=5)
///     .map(|i| Waiting { member: MemberId::from_bits(i), rating: 0, since: Default::default() })
///     .collect();
/// let mut groups = Vec::new();
/// FillBySize.form("duel", 2, &waiting, Default::default(), &mut groups);
/// assert_eq!(groups, [vec![0, 1], vec![2, 3]]);
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct FillBySize;

impl Matchmaker for FillBySize {
    fn form(&mut self, _mode: &str, size: u16, waiting: &[Waiting], _now: Duration, groups: &mut Vec<Vec<usize>>) {
        let size = usize::from(size);
        for start in (0..waiting.len() / size).map(|g| g * size) {
            groups.push((start..start + size).collect());
        }
    }
}

/// The host migration hook: who hosts a room after its host left. The
/// default rule picks the member who has been in the room longest.
///
/// Any `FnMut(&RoomInfo) -> Option<MemberId>` is one: it gets the room
/// without the old host (whose id is still in [`RoomInfo::host`]) and
/// returns a remaining member, or `None` to close the room (every member
/// gets [`LeaveReason::RoomClosed`]; so does a member who isn't in it).
///
/// ```
/// use ponko::net::lobby::RoomInfo;
/// // Games whose host can't hand over its world: close the room instead.
/// let server = ponko_lobby::LobbyServer::new(Default::default()).on_host_left(|_: &RoomInfo| None);
/// ```
pub trait HostMigration: Send + 'static {
    /// The next host of `room`, or `None` to close it.
    fn next_host(&mut self, room: &RoomInfo) -> Option<MemberId>;
}

impl<F: FnMut(&RoomInfo) -> Option<MemberId> + Send + 'static> HostMigration for F {
    fn next_host(&mut self, room: &RoomInfo) -> Option<MemberId> {
        self(room)
    }
}

/// The longest-present member hosts next.
fn oldest_member(room: &RoomInfo) -> Option<MemberId> {
    room.members.first().copied()
}

/// Counters since the server started, and its size now.
/// ```
/// let stats = ponko_lobby::LobbyServer::new(Default::default()).stats();
/// assert_eq!(stats.rooms, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LobbyStats {
    /// Connections now.
    pub connections: usize,
    /// Rooms now.
    pub rooms: usize,
    /// Players waiting in matchmaking queues now.
    pub queued: usize,
    /// Rooms made, by request or by matchmaking.
    pub rooms_created: u64,
    /// Rooms matchmaking made.
    pub matches_made: u64,
    /// Rooms whose host left and that got a new one.
    pub host_migrations: u64,
    /// Rooms closed by the host migration rule or for being idle.
    pub rooms_closed: u64,
    /// Requests read.
    pub requests: u64,
    /// Requests refused (rate limits included).
    pub refused: u64,
    /// Requests refused or relay messages dropped for a rate limit.
    pub rate_limited: u64,
    /// Relay messages received.
    pub relay_in: u64,
    /// Relay messages forwarded (one per receiving member).
    pub relay_out: u64,
    /// Relay payload bytes received.
    pub relay_bytes_in: u64,
    /// Relay payload bytes forwarded.
    pub relay_bytes_out: u64,
    /// Relay messages dropped: over the size cap or the rate limit, from a
    /// member in no room, for a member in another room, or a full lane.
    pub relay_dropped: u64,
    /// Connections closed for being idle, or for too many from one address.
    pub closed_connections: u64,
    /// Errors from the endpoints' sockets (the latest is
    /// [`LobbyServer::last_error`]).
    pub socket_errors: u64,
}

/// A token bucket: `tokens` refill at `rate` per second up to `burst`.
#[derive(Clone, Copy, Debug)]
struct Bucket {
    tokens: f64,
    at: Duration,
}

impl Bucket {
    fn full(burst: f64, now: Duration) -> Self {
        Self { tokens: burst, at: now }
    }
    fn refill(&mut self, rate: f64, burst: f64, now: Duration) {
        let dt = now.saturating_sub(self.at).as_secs_f64();
        self.tokens = (self.tokens + dt * rate).min(burst);
        self.at = now;
    }
    fn take(&mut self, cost: f64, rate: f64, burst: f64, now: Duration) -> bool {
        self.refill(rate, burst, now);
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum AddrKey {
    Ip(IpAddr),
    Member(MemberId),
}

#[derive(Debug)]
struct Address {
    connections: u32,
    requests: Bucket,
    relay: Bucket,
    /// When this address was last told it is rate limited: at most once a
    /// second, so the refusals don't become traffic of their own.
    noticed: Option<Duration>,
}

#[derive(Debug)]
struct Conn {
    endpoint: usize,
    peer: PeerId,
    addr: AddrKey,
    room: Option<String>,
    queue: Option<(String, u16)>,
    /// Since when the connection is in no room and no queue.
    idle_since: Option<Duration>,
    /// When the server closes it (after its last reply went out).
    closing_at: Option<Duration>,
}

#[derive(Debug)]
struct Room {
    info: RoomInfo,
    last_active: Duration,
}

/// Room codes: letters and digits without the look-alikes 0, O, 1 and I.
const CODE_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 6;
/// How long a refused connection gets for its last reply to go out.
const CLOSE_DELAY: Duration = Duration::from_millis(200);
/// How often timeouts are checked.
const SWEEP_EVERY: Duration = Duration::from_millis(100);
/// Polls of one endpoint per server poll while its socket has packets
/// waiting (each takes up to 256).
const MAX_DRAIN_ROUNDS: usize = 16;

/// The lobby server: polls its endpoints, keeps rooms, queues and
/// connections, and forwards relay messages. Owns no thread: call
/// [`poll`](Self::poll) in a loop (the `ponko-lobby` program does it at
/// 100 Hz in [`Game::run_server`](ponko::game::Game::run_server)).
///
/// ```
/// use ponko::net::NetMessages;
/// use ponko_lobby::{LobbyLimits, LobbyServer};
///
/// let mut server = LobbyServer::new(LobbyLimits::default());
/// server.listen(NetMessages::host("127.0.0.1:0", 1024).unwrap());
/// server.poll();
/// assert_eq!(server.stats().connections, 0);
/// ```
pub struct LobbyServer {
    limits: LobbyLimits,
    endpoints: Vec<NetMessages>,
    conns: HashMap<MemberId, Conn>,
    by_peer: HashMap<(usize, PeerId), MemberId>,
    rooms: HashMap<String, Room>,
    queues: BTreeMap<(String, u16), Vec<Waiting>>,
    addresses: HashMap<AddrKey, Address>,
    next_member: u32,
    now: Duration,
    last_poll: Option<Instant>,
    next_sweep: Duration,
    next_matchmaking: Duration,
    rng: u64,
    matchmaker: Box<dyn Matchmaker>,
    host_migration: Box<dyn HostMigration>,
    stats: LobbyStats,
    last_error: Option<String>,
    scratch: Vec<u8>,
    groups: Vec<Vec<usize>>,
}

impl std::fmt::Debug for LobbyServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LobbyServer")
            .field("endpoints", &self.endpoints.len())
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl LobbyServer {
    /// A server with these limits and no endpoints yet
    /// ([`listen`](Self::listen)). Room codes are random; [`seed`](Self::seed)
    /// makes them repeatable.
    /// ```
    /// let server = ponko_lobby::LobbyServer::new(ponko_lobby::LobbyLimits::default());
    /// ```
    pub fn new(limits: LobbyLimits) -> Self {
        let seed = {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u8(1);
            h.finish()
        };
        Self {
            limits,
            endpoints: Vec::new(),
            conns: HashMap::new(),
            by_peer: HashMap::new(),
            rooms: HashMap::new(),
            queues: BTreeMap::new(),
            addresses: HashMap::new(),
            next_member: 1,
            now: Duration::ZERO,
            last_poll: None,
            next_sweep: Duration::ZERO,
            next_matchmaking: Duration::ZERO,
            rng: seed,
            matchmaker: Box::new(FillBySize),
            host_migration: Box::new(oldest_member),
            stats: LobbyStats::default(),
            last_error: None,
            scratch: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// Room codes from this seed, the same in every run (for tests).
    /// Codes are not secrets either way: anyone may try codes, at the
    /// request rate limit.
    /// ```
    /// let server = ponko_lobby::LobbyServer::new(Default::default()).seed(7);
    /// ```
    pub fn seed(mut self, seed: u64) -> Self {
        self.rng = seed;
        self
    }

    /// Uses `rule` for matchmaking instead of [`FillBySize`].
    /// ```
    /// let server = ponko_lobby::LobbyServer::new(Default::default()).matchmaker(ponko_lobby::FillBySize);
    /// ```
    pub fn matchmaker(mut self, rule: impl Matchmaker) -> Self {
        self.matchmaker = Box::new(rule);
        self
    }

    /// Uses `rule` to pick a room's next host when its host leaves, instead
    /// of the longest-present member.
    /// ```
    /// use ponko::net::lobby::RoomInfo;
    /// // The newest member hosts next.
    /// let server = ponko_lobby::LobbyServer::new(Default::default())
    ///     .on_host_left(|room: &RoomInfo| room.members.last().copied());
    /// ```
    pub fn on_host_left(mut self, rule: impl HostMigration) -> Self {
        self.host_migration = Box::new(rule);
        self
    }

    /// Takes clients from a host endpoint ([`NetMessages::host`], with
    /// [`accept_web`](NetMessages::accept_web) for browsers, or one end of
    /// [`NetMessages::loopback`]). A server may listen on several, for
    /// IPv4 and IPv6, or for more clients than one endpoint takes (1024).
    /// Returns its index for [`endpoint`](Self::endpoint).
    /// ```
    /// let mut server = ponko_lobby::LobbyServer::new(Default::default());
    /// let index = server.listen(ponko::net::NetMessages::host("127.0.0.1:0", 64).unwrap());
    /// assert!(server.endpoint(index).unwrap().local_addr().is_some());
    /// ```
    pub fn listen(&mut self, endpoint: NetMessages) -> usize {
        self.endpoints.push(endpoint);
        self.endpoints.len() - 1
    }

    /// An endpoint given to [`listen`](Self::listen), for its address and
    /// counters.
    /// ```
    /// assert!(ponko_lobby::LobbyServer::new(Default::default()).endpoint(0).is_none());
    /// ```
    pub fn endpoint(&self, index: usize) -> Option<&NetMessages> {
        self.endpoints.get(index)
    }

    /// Polls with the wall time since the last poll: call it in a loop,
    /// every few milliseconds (packets wait in the socket until then).
    /// ```
    /// let mut server = ponko_lobby::LobbyServer::new(Default::default());
    /// server.poll();
    /// ```
    pub fn poll(&mut self) {
        let now = Instant::now();
        let elapsed = self.last_poll.map_or(Duration::ZERO, |last| now - last);
        self.last_poll = Some(now);
        self.poll_elapsed(elapsed);
    }

    /// [`poll`](Self::poll) with the time that passed given: the server's
    /// clock (timeouts, rate limits) and its endpoints' move by `elapsed`.
    /// For tests and load tests on a clock of their own.
    /// ```
    /// let mut server = ponko_lobby::LobbyServer::new(Default::default());
    /// server.poll_elapsed(std::time::Duration::from_millis(10));
    /// ```
    pub fn poll_elapsed(&mut self, elapsed: Duration) {
        self.now += elapsed;
        for index in 0..self.endpoints.len() {
            for round in 0..MAX_DRAIN_ROUNDS {
                let before = self.endpoints[index].stats().packets_received;
                let step = if round == 0 { elapsed } else { Duration::ZERO };
                if let Err(e) = self.endpoints[index].poll_elapsed(step) {
                    self.socket_error(e);
                }
                self.drain(index);
                if self.endpoints[index].stats().packets_received - before < 256 {
                    break;
                }
            }
        }
        if self.now >= self.next_sweep {
            self.next_sweep = self.now + SWEEP_EVERY;
            self.sweep();
        }
        if self.now >= self.next_matchmaking {
            self.next_matchmaking = self.now + self.limits.matchmaking_every;
            let keys: Vec<_> = self.queues.keys().cloned().collect();
            for key in keys {
                self.matchmake(&key);
            }
        }
        // Send what the events above produced now, not a poll later.
        for index in 0..self.endpoints.len() {
            if let Err(e) = self.endpoints[index].poll_elapsed(Duration::ZERO) {
                self.socket_error(e);
            }
        }
    }

    fn socket_error(&mut self, e: std::io::Error) {
        self.stats.socket_errors += 1;
        self.last_error = Some(e.to_string());
    }

    /// The latest socket error of an endpoint, for logs. The server keeps
    /// running through them.
    /// ```
    /// assert!(ponko_lobby::LobbyServer::new(Default::default()).last_error().is_none());
    /// ```
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Counters and sizes.
    /// ```
    /// assert_eq!(ponko_lobby::LobbyServer::new(Default::default()).stats().connections, 0);
    /// ```
    pub fn stats(&self) -> LobbyStats {
        LobbyStats {
            connections: self.conns.len(),
            rooms: self.rooms.len(),
            queued: self.queues.values().map(Vec::len).sum(),
            ..self.stats
        }
    }

    /// The room with `code` (any letter case).
    /// ```
    /// assert!(ponko_lobby::LobbyServer::new(Default::default()).room("ABCDEF").is_none());
    /// ```
    pub fn room(&self, code: &str) -> Option<&RoomInfo> {
        self.rooms.get(&code.to_ascii_uppercase()).map(|r| &r.info)
    }

    /// Every room.
    /// ```
    /// assert_eq!(ponko_lobby::LobbyServer::new(Default::default()).rooms().count(), 0);
    /// ```
    pub fn rooms(&self) -> impl Iterator<Item = &RoomInfo> {
        self.rooms.values().map(|r| &r.info)
    }

    /// Tells every member in a room that the server stops
    /// ([`LeaveReason::ServerStopping`]) and closes every connection
    /// shortly after (keep polling for a fraction of a second so the
    /// messages go out; [`stats`](Self::stats) shows 0 connections once
    /// they did).
    /// ```
    /// let mut server = ponko_lobby::LobbyServer::new(Default::default());
    /// server.shutdown();
    /// server.poll();
    /// assert_eq!(server.stats().connections, 0);
    /// ```
    pub fn shutdown(&mut self) {
        let codes: Vec<String> = self.rooms.keys().cloned().collect();
        for code in codes {
            self.close_room(&code, LeaveReason::ServerStopping);
        }
        let close = self.now + CLOSE_DELAY;
        for conn in self.conns.values_mut() {
            conn.queue = None;
            conn.closing_at.get_or_insert(close);
        }
        self.queues.clear();
        self.next_sweep = self.now;
    }

    fn drain(&mut self, endpoint: usize) {
        while let Some(event) = self.endpoints[endpoint].next_event() {
            match event {
                MessageEvent::Connected(peer) => self.connected(endpoint, peer),
                MessageEvent::Disconnected(peer, _) => {
                    if let Some(member) = self.by_peer.remove(&(endpoint, peer)) {
                        self.gone(member);
                    }
                }
                MessageEvent::Message { peer, lane, bytes } => {
                    if let Some(&member) = self.by_peer.get(&(endpoint, peer)) {
                        self.message(member, lane, &bytes);
                    }
                }
            }
        }
    }

    fn connected(&mut self, endpoint: usize, peer: PeerId) {
        let member = MemberId::from_bits(self.next_member);
        self.next_member = self.next_member.wrapping_add(1).max(1);
        let addr = self.address_of(endpoint, peer).map_or(AddrKey::Member(member), AddrKey::Ip);
        let (requests, relay) = (self.limits.request_burst, self.limits.relay_burst_bytes);
        let now = self.now;
        let address = self.addresses.entry(addr).or_insert_with(|| Address {
            connections: 0,
            requests: Bucket::full(requests, now),
            relay: Bucket::full(relay, now),
            noticed: None,
        });
        address.connections += 1;
        let over = address.connections > self.limits.connections_per_address;
        self.conns.insert(
            member,
            Conn {
                endpoint,
                peer,
                addr,
                room: None,
                queue: None,
                idle_since: Some(now),
                closing_at: None,
            },
        );
        self.by_peer.insert((endpoint, peer), member);
        if over {
            self.refuse(member, LobbyError::TooManyConnections);
            self.close(member);
            return;
        }
        let limits = ServerLimits::new(
            self.limits.max_relay_bytes,
            self.limits.max_room_size,
            self.limits.max_meta_bytes,
            u64::try_from(self.limits.idle_timeout.as_millis()).unwrap_or(u64::MAX),
        );
        self.reply(
            member,
            &Reply::Welcome {
                version: protocol::VERSION,
                member,
                limits,
            },
        );
    }

    /// The IP address a connection's limits count against, if it has one:
    /// a browser behind a trusted proxy is the client the proxy names.
    fn address_of(&self, endpoint: usize, peer: PeerId) -> Option<IpAddr> {
        if self.limits.address_key != AddressKey::Ip {
            return None;
        }
        let net = &self.endpoints[endpoint];
        match net.web_peer(peer) {
            Some(web) if !self.limits.trusted_proxies.is_empty() => {
                let proxies = &self.limits.trusted_proxies;
                web.client_ip(|ip| proxies.iter().any(|p| p.contains(ip)))
            }
            _ => net.peer_addr(peer).map(|a| a.ip()),
        }
    }

    /// Closes `member` after its last messages went out.
    fn close(&mut self, member: MemberId) {
        if let Some(conn) = self.conns.get_mut(&member) {
            conn.closing_at.get_or_insert(self.now + CLOSE_DELAY);
            self.next_sweep = self.next_sweep.min(self.now + CLOSE_DELAY);
            self.stats.closed_connections += 1;
        }
    }

    /// A connection is gone: out of its room and queue.
    fn gone(&mut self, member: MemberId) {
        self.leave_room(member, None);
        self.unqueue(member);
        if let Some(conn) = self.conns.remove(&member)
            && let Some(address) = self.addresses.get_mut(&conn.addr)
        {
            address.connections = address.connections.saturating_sub(1);
        }
    }

    fn message(&mut self, member: MemberId, lane: Lane, bytes: &[u8]) {
        let Some(conn) = self.conns.get(&member) else { return };
        if conn.closing_at.is_some() {
            return;
        }
        let addr = conn.addr;
        match bytes.first() {
            Some(&protocol::RELAY) => self.relay(member, addr, lane, bytes),
            _ => {
                self.stats.requests += 1;
                let limits = &self.limits;
                let now = self.now;
                let Some(address) = self.addresses.get_mut(&addr) else {
                    return;
                };
                if !address
                    .requests
                    .take(1.0, limits.requests_per_second, limits.request_burst, now)
                {
                    self.stats.rate_limited += 1;
                    self.stats.refused += 1;
                    if address
                        .noticed
                        .is_none_or(|t| now.saturating_sub(t) >= Duration::from_secs(1))
                    {
                        address.noticed = Some(now);
                        self.reply(
                            member,
                            &Reply::Refused {
                                error: LobbyError::RateLimited,
                            },
                        );
                    }
                    return;
                }
                if bytes.len() > self.limits.max_request_bytes {
                    return self.refuse(member, LobbyError::TooLarge);
                }
                match protocol::decode::<Request>(bytes) {
                    Some(Frame::Control(request)) => self.request(member, request),
                    _ => self.refuse(member, LobbyError::BadRequest),
                }
            }
        }
    }

    fn relay(&mut self, member: MemberId, addr: AddrKey, lane: Lane, bytes: &[u8]) {
        self.stats.relay_in += 1;
        let Some(Frame::Relay { member: to, payload }) = protocol::decode::<Request>(bytes) else {
            self.stats.relay_dropped += 1;
            return;
        };
        self.stats.relay_bytes_in += payload.len() as u64;
        let room = self.conns.get(&member).and_then(|c| c.room.as_ref());
        let Some(room) = room.and_then(|code| self.rooms.get_mut(code)) else {
            self.stats.relay_dropped += 1;
            return;
        };
        if payload.len() > self.limits.max_relay_bytes as usize {
            self.stats.relay_dropped += 1;
            return;
        }
        let limits = &self.limits;
        let Some(address) = self.addresses.get_mut(&addr) else {
            return;
        };
        if !address.relay.take(
            payload.len() as f64,
            limits.relay_bytes_per_second,
            limits.relay_burst_bytes,
            self.now,
        ) {
            self.stats.rate_limited += 1;
            self.stats.relay_dropped += 1;
            return;
        }
        room.last_active = self.now;
        protocol::encode_relay(member.to_bits(), payload, &mut self.scratch);
        let everyone = to == protocol::EVERYONE;
        if !everyone && (to == member.to_bits() || !room.info.members.contains(&MemberId::from_bits(to))) {
            self.stats.relay_dropped += 1;
            return;
        }
        for &target in &room.info.members {
            if target == member || !(everyone || target.to_bits() == to) {
                continue;
            }
            let Some(conn) = self.conns.get(&target) else { continue };
            match self.endpoints[conn.endpoint].send(conn.peer, lane, &self.scratch) {
                Ok(()) => {
                    self.stats.relay_out += 1;
                    self.stats.relay_bytes_out += payload.len() as u64;
                }
                Err(_) => self.stats.relay_dropped += 1,
            }
        }
    }

    fn request(&mut self, member: MemberId, request: Request) {
        let Some(conn) = self.conns.get(&member) else { return };
        let in_room = conn.room.is_some();
        let queued = conn.queue.is_some();
        match request {
            Request::Create { size, meta } => {
                if let Some(error) = self.can_enter(in_room, queued) {
                    return self.refuse(member, error);
                }
                if !(2..=self.limits.max_room_size).contains(&size) {
                    return self.refuse(member, LobbyError::BadSize);
                }
                if meta.len() > self.limits.max_meta_bytes as usize {
                    return self.refuse(member, LobbyError::TooLarge);
                }
                if self.rooms.len() >= self.limits.max_rooms {
                    return self.refuse(member, LobbyError::ServerFull);
                }
                self.open_room(&[member], size, meta);
            }
            Request::Join { code } => {
                if let Some(error) = self.can_enter(in_room, queued) {
                    return self.refuse(member, error);
                }
                let code = code.to_ascii_uppercase();
                let Some(room) = self.rooms.get_mut(&code) else {
                    return self.refuse(member, LobbyError::RoomNotFound);
                };
                if room.info.is_full() {
                    return self.refuse(member, LobbyError::RoomFull);
                }
                room.info.members.push(member);
                room.last_active = self.now;
                let info = room.info.clone();
                self.enter(member, &code);
                self.reply(member, &Reply::Joined { room: info });
                self.tell_room(&code, Some(member), &Reply::MemberJoined { member });
            }
            Request::Leave => {
                if !in_room {
                    return self.refuse(member, LobbyError::NotInRoom);
                }
                self.leave_room(member, Some(LeaveReason::Requested));
            }
            Request::Queue { mode, size, rating } => {
                if let Some(error) = self.can_enter(in_room, queued) {
                    return self.refuse(member, error);
                }
                if !(2..=self.limits.max_room_size).contains(&size) {
                    return self.refuse(member, LobbyError::BadSize);
                }
                if mode.len() > self.limits.max_meta_bytes as usize {
                    return self.refuse(member, LobbyError::TooLarge);
                }
                let key = (mode, size);
                self.queues.entry(key.clone()).or_default().push(Waiting {
                    member,
                    rating,
                    since: self.now,
                });
                if let Some(conn) = self.conns.get_mut(&member) {
                    conn.queue = Some(key.clone());
                    conn.idle_since = None;
                }
                self.reply(
                    member,
                    &Reply::Queued {
                        mode: key.0.clone(),
                        size,
                    },
                );
                self.matchmake(&key);
            }
            Request::CancelQueue => {
                if !queued {
                    return self.refuse(member, LobbyError::NotInRoom);
                }
                self.unqueue(member);
                self.reply(member, &Reply::Unqueued);
            }
            Request::SetMeta { meta } => {
                let Some(code) = self.conns.get(&member).and_then(|c| c.room.clone()) else {
                    return self.refuse(member, LobbyError::NotInRoom);
                };
                if meta.len() > self.limits.max_meta_bytes as usize {
                    return self.refuse(member, LobbyError::TooLarge);
                }
                let Some(room) = self.rooms.get_mut(&code) else { return };
                if room.info.host != member {
                    return self.refuse(member, LobbyError::NotHost);
                }
                room.info.meta.clone_from(&meta);
                room.last_active = self.now;
                self.tell_room(&code, None, &Reply::MetaChanged { meta });
            }
            // A request from a newer protocol: not something this server does.
            _ => self.refuse(member, LobbyError::BadRequest),
        }
    }

    fn can_enter(&self, in_room: bool, queued: bool) -> Option<LobbyError> {
        if in_room {
            Some(LobbyError::AlreadyInRoom)
        } else if queued {
            Some(LobbyError::AlreadyQueued)
        } else {
            None
        }
    }

    fn new_code(&mut self) -> String {
        loop {
            let mut code = String::with_capacity(CODE_LEN);
            for _ in 0..CODE_LEN {
                // splitmix64
                self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
                let mut z = self.rng;
                z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
                z ^= z >> 31;
                code.push(char::from(CODE_ALPHABET[(z % 32) as usize]));
            }
            if !self.rooms.contains_key(&code) {
                return code;
            }
        }
    }

    /// A new room with these members (the first hosts), each told.
    fn open_room(&mut self, members: &[MemberId], size: u16, meta: String) {
        let code = self.new_code();
        let info = RoomInfo {
            code: code.clone(),
            host: members[0],
            members: members.to_vec(),
            size,
            meta,
        };
        for &member in members {
            self.enter(member, &code);
            self.reply(member, &Reply::Joined { room: info.clone() });
        }
        self.rooms.insert(
            code,
            Room {
                info,
                last_active: self.now,
            },
        );
        self.stats.rooms_created += 1;
    }

    fn enter(&mut self, member: MemberId, code: &str) {
        if let Some(conn) = self.conns.get_mut(&member) {
            conn.room = Some(code.to_owned());
            conn.idle_since = None;
        }
    }

    /// Takes `member` out of its room; `tell` sends it `Left` with that
    /// reason. The others hear `MemberLeft`, and the host migration rule
    /// runs if it hosted.
    fn leave_room(&mut self, member: MemberId, tell: Option<LeaveReason>) {
        let Some(conn) = self.conns.get_mut(&member) else {
            return;
        };
        let Some(code) = conn.room.take() else { return };
        conn.idle_since = Some(self.now);
        if let Some(reason) = tell {
            self.reply(member, &Reply::Left { reason });
        }
        let Some(room) = self.rooms.get_mut(&code) else { return };
        room.info.members.retain(|&m| m != member);
        room.last_active = self.now;
        if room.info.members.is_empty() {
            self.rooms.remove(&code);
            return;
        }
        let was_host = room.info.host == member;
        self.tell_room(&code, None, &Reply::MemberLeft { member });
        if !was_host {
            return;
        }
        let Some(room) = self.rooms.get_mut(&code) else { return };
        match self.host_migration.next_host(&room.info) {
            Some(host) if room.info.members.contains(&host) => {
                room.info.host = host;
                self.stats.host_migrations += 1;
                self.tell_room(&code, None, &Reply::HostChanged { host });
            }
            _ => self.close_room(&code, LeaveReason::RoomClosed),
        }
    }

    fn close_room(&mut self, code: &str, reason: LeaveReason) {
        let Some(room) = self.rooms.remove(code) else { return };
        self.stats.rooms_closed += 1;
        for member in room.info.members {
            if let Some(conn) = self.conns.get_mut(&member) {
                conn.room = None;
                conn.idle_since = Some(self.now);
            }
            self.reply(member, &Reply::Left { reason });
        }
    }

    fn unqueue(&mut self, member: MemberId) {
        let Some(key) = self.conns.get_mut(&member).and_then(|c| c.queue.take()) else {
            return;
        };
        if let Some(conn) = self.conns.get_mut(&member) {
            conn.idle_since = Some(self.now);
        }
        if let Some(queue) = self.queues.get_mut(&key) {
            queue.retain(|w| w.member != member);
            if queue.is_empty() {
                self.queues.remove(&key);
            }
        }
    }

    fn matchmake(&mut self, key: &(String, u16)) {
        let Some(waiting) = self.queues.get(key) else { return };
        let mut groups = std::mem::take(&mut self.groups);
        groups.clear();
        self.matchmaker.form(&key.0, key.1, waiting, self.now, &mut groups);
        if groups.is_empty() {
            self.groups = groups;
            return;
        }
        let size = usize::from(key.1);
        let mut used = vec![false; waiting.len()];
        let mut rooms = Vec::new();
        for group in &groups {
            let valid = (2..=size).contains(&group.len())
                && group.iter().all(|&i| i < used.len() && !used[i])
                && group.iter().enumerate().all(|(n, i)| !group[..n].contains(i));
            if !valid || self.rooms.len() + rooms.len() >= self.limits.max_rooms {
                continue;
            }
            group.iter().for_each(|&i| used[i] = true);
            rooms.push(group.iter().map(|&i| waiting[i].member).collect::<Vec<_>>());
        }
        self.groups = groups;
        if rooms.is_empty() {
            return;
        }
        if let Some(queue) = self.queues.get_mut(key) {
            let mut i = 0;
            queue.retain(|_| {
                i += 1;
                !used[i - 1]
            });
            if queue.is_empty() {
                self.queues.remove(key);
            }
        }
        for members in rooms {
            for member in &members {
                if let Some(conn) = self.conns.get_mut(member) {
                    conn.queue = None;
                }
            }
            self.open_room(&members, key.1, String::new());
            self.stats.matches_made += 1;
        }
    }

    /// Timeouts: connections to close, idle connections and rooms, and
    /// addresses nobody uses whose budgets are full again.
    fn sweep(&mut self) {
        let now = self.now;
        let mut close = Vec::new();
        let mut idle = Vec::new();
        for (&member, conn) in &self.conns {
            if conn.closing_at.is_some_and(|t| now >= t) {
                close.push(member);
            } else if conn.closing_at.is_none()
                && conn
                    .idle_since
                    .is_some_and(|t| now.saturating_sub(t) >= self.limits.idle_timeout)
            {
                idle.push(member);
            }
        }
        for member in idle {
            // Told why, then closed once that went out.
            self.reply(
                member,
                &Reply::Left {
                    reason: LeaveReason::Idle,
                },
            );
            self.close(member);
        }
        for member in close {
            let Some(conn) = self.conns.get(&member) else { continue };
            let (endpoint, peer) = (conn.endpoint, conn.peer);
            if self.endpoints[endpoint].disconnect(peer).is_err() {
                // Already gone from the endpoint: forget it here too.
                self.by_peer.remove(&(endpoint, peer));
                self.gone(member);
            }
            self.drain(endpoint);
        }
        let stale: Vec<String> = self
            .rooms
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.last_active) >= self.limits.room_idle_timeout)
            .map(|(code, _)| code.clone())
            .collect();
        for code in stale {
            self.close_room(&code, LeaveReason::Idle);
        }
        let limits = &self.limits;
        self.addresses.retain(|_, a| {
            a.requests.refill(limits.requests_per_second, limits.request_burst, now);
            a.relay
                .refill(limits.relay_bytes_per_second, limits.relay_burst_bytes, now);
            a.connections > 0 || a.requests.tokens < limits.request_burst || a.relay.tokens < limits.relay_burst_bytes
        });
    }

    fn refuse(&mut self, member: MemberId, error: LobbyError) {
        self.stats.refused += 1;
        self.reply(member, &Reply::Refused { error });
    }

    fn reply(&mut self, member: MemberId, reply: &Reply) {
        let Some(conn) = self.conns.get(&member) else { return };
        let bytes = protocol::encode_control(reply);
        if let Err(e) = self.endpoints[conn.endpoint].send(conn.peer, Lane::Reliable, &bytes) {
            self.socket_error(e);
        }
    }

    /// Sends `reply` to every member of room `code` but `except`.
    fn tell_room(&mut self, code: &str, except: Option<MemberId>, reply: &Reply) {
        let Some(room) = self.rooms.get(code) else { return };
        let bytes = protocol::encode_control(reply);
        for &member in &room.info.members {
            if Some(member) == except {
                continue;
            }
            let Some(conn) = self.conns.get(&member) else { continue };
            if let Err(e) = self.endpoints[conn.endpoint].send(conn.peer, Lane::Reliable, &bytes) {
                self.stats.socket_errors += 1;
                self.last_error = Some(e.to_string());
            }
        }
    }

    /// How long the server has run, on its clock (the sum of the polls'
    /// elapsed times).
    /// ```
    /// assert_eq!(ponko_lobby::LobbyServer::new(Default::default()).uptime(), std::time::Duration::ZERO);
    /// ```
    pub fn uptime(&self) -> Duration {
        self.now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bucket_refills_at_its_rate_up_to_its_burst() {
        let mut b = Bucket::full(2.0, Duration::ZERO);
        assert!(b.take(1.0, 1.0, 2.0, Duration::ZERO));
        assert!(b.take(1.0, 1.0, 2.0, Duration::ZERO));
        assert!(!b.take(1.0, 1.0, 2.0, Duration::ZERO));
        assert!(!b.take(1.0, 1.0, 2.0, Duration::from_millis(500)));
        assert!(b.take(1.0, 1.0, 2.0, Duration::from_millis(1000)));
        b.refill(1.0, 2.0, Duration::from_secs(100));
        assert_eq!(b.tokens, 2.0);
    }

    #[test]
    fn room_codes_avoid_look_alike_characters() {
        let mut server = LobbyServer::new(LobbyLimits::default()).seed(1);
        for _ in 0..1000 {
            let code = server.new_code();
            assert_eq!(code.len(), CODE_LEN);
            assert!(!code.contains(['0', 'O', '1', 'I']), "{code}");
        }
    }
}
