//! A client for a lobby server (feature `lobby`): rooms with short codes,
//! matchmaking queues, host migration, and a relay for players who can't
//! connect to each other directly.
//!
//! The server is the `ponko_lobby` crate's `ponko-lobby` program (or a
//! game's own build of its library); [`protocol`] is what the two say, over
//! a [message endpoint](crate::net::NetMessages): UDP, or WebSocket from a
//! browser.
//!
//! A [`LobbyClient`] is polled like a message endpoint, once per frame, and
//! reports [`LobbyEvent`]s. Requests made before the server answered are
//! sent once it does.
//!
//! ```
//! use ponko::net::lobby::{LobbyClient, LobbyEvent};
//!
//! # fn run() -> std::io::Result<()> {
//! let mut lobby = LobbyClient::join("127.0.0.1:7700")?;
//! lobby.create_room(4)?;
//! // Every frame:
//! lobby.poll()?;
//! while let Some(event) = lobby.next_event() {
//!     match event {
//!         LobbyEvent::Joined(room) => println!("room code: {}", room.code),
//!         LobbyEvent::Relayed { from, bytes, .. } => println!("{from:?} sent {} bytes", bytes.len()),
//!         _ => {}
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! The server is a reference server: no accounts, no persistence, no
//! anti-cheat; it forwards relay bytes without reading them. The guide's
//! "Lobbies, matchmaking and relays" chapter says what it limits and how to
//! run it.

#![deny(missing_docs)]

pub mod protocol;
mod relay;

use std::collections::VecDeque;
use std::io;
use std::net::ToSocketAddrs;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{Lane, MessageEvent, NetMessages, PeerId};
use protocol::{Frame, Reply, Request, ServerLimits};

/// A member of the lobby: one client connection, numbered by the server
/// from 1 and never reused while the server runs.
/// ```
/// use ponko::net::lobby::MemberId;
/// assert_eq!(MemberId::from_bits(5).to_bits(), 5);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemberId(u32);

impl MemberId {
    /// The id as a number, for bindings and logs.
    /// ```
    /// assert_eq!(ponko::net::lobby::MemberId::from_bits(3).to_bits(), 3);
    /// ```
    pub const fn to_bits(self) -> u32 {
        self.0
    }

    /// An id from [`to_bits`](Self::to_bits).
    /// ```
    /// let id = ponko::net::lobby::MemberId::from_bits(3);
    /// ```
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
}

/// A room as its members see it.
/// ```
/// use ponko::net::lobby::{MemberId, RoomInfo};
/// let room = RoomInfo { code: "KQ7X2M".into(), host: MemberId::from_bits(1), members: vec![MemberId::from_bits(1)], size: 4, meta: String::new() };
/// assert!(room.is_full() == false);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomInfo {
    /// The code others join with: six letters and digits, without the
    /// look-alikes 0, O, 1 and I.
    /// ```
    /// let code = ponko::net::lobby::RoomInfo::default().code;
    /// ```
    pub code: String,
    /// The member who hosts the game.
    /// ```
    /// let host = ponko::net::lobby::RoomInfo::default().host;
    /// ```
    pub host: MemberId,
    /// Every member, in the order they came in.
    /// ```
    /// let n = ponko::net::lobby::RoomInfo::default().members.len();
    /// ```
    pub members: Vec<MemberId>,
    /// Most members.
    /// ```
    /// let size = ponko::net::lobby::RoomInfo::default().size;
    /// ```
    pub size: u16,
    /// The room's text, set by its host.
    /// ```
    /// let meta = ponko::net::lobby::RoomInfo::default().meta;
    /// ```
    #[serde(default)]
    pub meta: String,
}

impl RoomInfo {
    /// Whether the room has `size` members.
    /// ```
    /// assert!(ponko::net::lobby::RoomInfo::default().is_full());
    /// ```
    pub fn is_full(&self) -> bool {
        self.members.len() >= usize::from(self.size)
    }
}

/// Why the server refused a request.
/// ```
/// let e = ponko::net::lobby::LobbyError::RoomNotFound;
/// assert_eq!(e.to_string(), "no room has that code");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LobbyError {
    /// No room has that code.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::RoomNotFound;
    /// ```
    RoomNotFound,
    /// The room has as many members as it can take.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::RoomFull;
    /// ```
    RoomFull,
    /// The client is in a room already: leave it first.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::AlreadyInRoom;
    /// ```
    AlreadyInRoom,
    /// The client waits in a matchmaking queue: cancel it first.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::AlreadyQueued;
    /// ```
    AlreadyQueued,
    /// The client isn't in a room (or a queue, for a cancel).
    /// ```
    /// let e = ponko::net::lobby::LobbyError::NotInRoom;
    /// ```
    NotInRoom,
    /// Only the room's host may do that.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::NotHost;
    /// ```
    NotHost,
    /// Too many requests from this address: wait a moment.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::RateLimited;
    /// ```
    RateLimited,
    /// The message or the room's text is longer than the server takes.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::TooLarge;
    /// ```
    TooLarge,
    /// The room size is under 2 or over the server's cap.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::BadSize;
    /// ```
    BadSize,
    /// The server has as many rooms as it may hold.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::ServerFull;
    /// ```
    ServerFull,
    /// Too many connections from this address; the server closes this one.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::TooManyConnections;
    /// ```
    TooManyConnections,
    /// The server couldn't read the request.
    /// ```
    /// let e = ponko::net::lobby::LobbyError::BadRequest;
    /// ```
    BadRequest,
    /// An error this version doesn't know (from a newer server).
    /// ```
    /// let e = ponko::net::lobby::LobbyError::Other;
    /// ```
    #[serde(other)]
    Other,
}

impl std::fmt::Display for LobbyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::RoomNotFound => "no room has that code",
            Self::RoomFull => "the room is full",
            Self::AlreadyInRoom => "already in a room",
            Self::AlreadyQueued => "already waiting in a matchmaking queue",
            Self::NotInRoom => "not in a room or queue",
            Self::NotHost => "only the room's host may do that",
            Self::RateLimited => "too many requests from this address",
            Self::TooLarge => "the message is too large",
            Self::BadSize => "the room size is out of range",
            Self::ServerFull => "the server has no room for more rooms",
            Self::TooManyConnections => "too many connections from this address",
            Self::BadRequest => "the server could not read the request",
            Self::Other => "refused by the server",
        })
    }
}

impl std::error::Error for LobbyError {}

/// Why a client is out of its room.
/// ```
/// let r = ponko::net::lobby::LeaveReason::RoomClosed;
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LeaveReason {
    /// The client asked to leave.
    /// ```
    /// let r = ponko::net::lobby::LeaveReason::Requested;
    /// ```
    Requested,
    /// The host left and the server's host migration rule closed the room.
    /// ```
    /// let r = ponko::net::lobby::LeaveReason::RoomClosed;
    /// ```
    RoomClosed,
    /// Nobody in the room sent anything for the server's room idle timeout.
    /// ```
    /// let r = ponko::net::lobby::LeaveReason::Idle;
    /// ```
    Idle,
    /// The server is shutting down.
    /// ```
    /// let r = ponko::net::lobby::LeaveReason::ServerStopping;
    /// ```
    ServerStopping,
    /// A reason this version doesn't know (from a newer server).
    /// ```
    /// let r = ponko::net::lobby::LeaveReason::Other;
    /// ```
    #[serde(other)]
    Other,
}

/// What happened since the last poll, from [`LobbyClient::next_event`].
/// ```
/// use ponko::net::lobby::{LobbyEvent, MemberId};
/// let e = LobbyEvent::MemberJoined(MemberId::from_bits(2));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LobbyEvent {
    /// The server answered: this client is member `me`.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Connected { me: ponko::net::lobby::MemberId::from_bits(1) };
    /// ```
    Connected {
        /// This client's member id.
        /// ```
        /// let me = ponko::net::lobby::MemberId::from_bits(1);
        /// ```
        me: MemberId,
    },
    /// The client is in a room now: it made one, joined one, or matchmaking
    /// put it in one. [`RoomInfo::host`] says who hosts the game.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Joined(ponko::net::lobby::RoomInfo::default());
    /// ```
    Joined(RoomInfo),
    /// Someone joined the room.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::MemberJoined(ponko::net::lobby::MemberId::from_bits(2));
    /// ```
    MemberJoined(MemberId),
    /// Someone left the room (or lost their connection).
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::MemberLeft(ponko::net::lobby::MemberId::from_bits(2));
    /// ```
    MemberLeft(MemberId),
    /// The host left and the room has a new one; if it is this client
    /// ([`LobbyClient::is_host`]), it hosts the game from now on.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::HostChanged(ponko::net::lobby::MemberId::from_bits(2));
    /// ```
    HostChanged(MemberId),
    /// The host changed the room's text.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::MetaChanged("started".into());
    /// ```
    MetaChanged(String),
    /// The client is out of its room.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Left(ponko::net::lobby::LeaveReason::Requested);
    /// ```
    Left(LeaveReason),
    /// The client waits in a matchmaking queue; [`Joined`](Self::Joined)
    /// follows when a room is made.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Queued;
    /// ```
    Queued,
    /// The client left the matchmaking queue.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Unqueued;
    /// ```
    Unqueued,
    /// The server didn't do a request.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Refused(ponko::net::lobby::LobbyError::RoomFull);
    /// ```
    Refused(LobbyError),
    /// Bytes another member of the room sent through the server's relay.
    /// ```
    /// use ponko::net::{Lane, lobby::{LobbyEvent, MemberId}};
    /// let e = LobbyEvent::Relayed { from: MemberId::from_bits(2), lane: Lane::Unreliable, bytes: vec![1] };
    /// ```
    Relayed {
        /// The sender.
        /// ```
        /// let m = ponko::net::lobby::MemberId::from_bits(2);
        /// ```
        from: MemberId,
        /// The lane the sender used.
        /// ```
        /// let lane = ponko::net::Lane::Reliable;
        /// ```
        lane: Lane,
        /// The sender's bytes, as sent.
        /// ```
        /// let bytes: Vec<u8> = vec![];
        /// ```
        bytes: Vec<u8>,
    },
    /// The connection to the server is gone (it closed it, timed out, or
    /// was never made); the reason is for logs. Make a new client to retry.
    /// ```
    /// let e = ponko::net::lobby::LobbyEvent::Disconnected("timed out".into());
    /// ```
    Disconnected(String),
}

/// Requests kept until the server answers; more are refused.
const MAX_PENDING: usize = 64;

/// A connection to a lobby server. Poll it every frame (like
/// [`NetMessages`], which it wraps) and read its [`LobbyEvent`]s.
///
/// It keeps track of its own state: [`me`](Self::me), [`room`](Self::room)
/// and [`is_host`](Self::is_host) are up to date after every poll.
///
/// ```
/// use ponko::net::NetMessages;
/// use ponko::net::lobby::LobbyClient;
/// // Over a client endpoint of any transport (here, in memory):
/// let mut endpoints = NetMessages::loopback(2);
/// let mut client = LobbyClient::new(endpoints.pop().unwrap());
/// client.create_room(2).unwrap(); // sent once the server answers
/// assert!(client.me().is_none() && client.room().is_none());
/// ```
pub struct LobbyClient {
    net: NetMessages,
    me: Option<MemberId>,
    room: Option<RoomInfo>,
    queued: bool,
    limits: ServerLimits,
    connected: bool,
    closed: bool,
    pending: Vec<Vec<u8>>,
    events: VecDeque<LobbyEvent>,
    scratch: Vec<u8>,
    #[cfg(feature = "webrtc")]
    signaling: Option<super::rtc::Signaling>,
    /// Session packets relayed to a [relay session](relay) that owns this
    /// client, by sender; `None` while no session does.
    session: Option<VecDeque<(MemberId, Vec<u8>)>>,
}

impl std::fmt::Debug for LobbyClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LobbyClient")
            .field("me", &self.me)
            .field("room", &self.room)
            .field("queued", &self.queued)
            .field("connected", &self.connected)
            .finish_non_exhaustive()
    }
}

impl LobbyClient {
    /// Starts connecting to a lobby server over UDP. Poll until
    /// [`LobbyEvent::Connected`] (or `Disconnected`).
    ///
    /// # Errors
    ///
    /// The errors of [`NetMessages::join`]: an address that doesn't
    /// resolve, or a local socket that can't be opened.
    ///
    /// ```
    /// let lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// ```
    pub fn join(addr: impl ToSocketAddrs) -> io::Result<Self> {
        NetMessages::join(addr).map(Self::new)
    }

    /// Starts connecting to a lobby server's WebSocket port (`ws://`), as a
    /// browser build must.
    ///
    /// # Errors
    ///
    /// The errors of [`NetMessages::join_web`]: a URL that isn't `ws://`.
    ///
    /// ```
    /// let lobby = ponko::net::lobby::LobbyClient::join_web("ws://127.0.0.1:7701");
    /// assert!(lobby.is_ok());
    /// ```
    pub fn join_web(url: &str) -> io::Result<Self> {
        NetMessages::join_web(url).map(Self::new)
    }

    /// A client over a client endpoint that is already joining (any
    /// transport: [`NetMessages::join`], `join_web`, Steam, in memory).
    /// ```
    /// let net = ponko::net::NetMessages::join("127.0.0.1:7700").unwrap();
    /// let lobby = ponko::net::lobby::LobbyClient::new(net);
    /// ```
    pub fn new(endpoint: NetMessages) -> Self {
        Self {
            net: endpoint,
            me: None,
            room: None,
            queued: false,
            limits: ServerLimits::default(),
            connected: false,
            closed: false,
            pending: Vec::new(),
            events: VecDeque::new(),
            scratch: Vec::new(),
            #[cfg(feature = "webrtc")]
            signaling: None,
            session: None,
        }
    }

    /// Moves packets and turns the server's messages into events. Call it
    /// every frame.
    ///
    /// # Errors
    ///
    /// The socket's errors, as [`NetMessages::poll`].
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.poll().unwrap();
    /// ```
    pub fn poll(&mut self) -> io::Result<()> {
        self.net.poll()?;
        self.read();
        self.send_signals();
        Ok(())
    }

    /// [`poll`](Self::poll) with the time that passed given, for tests and
    /// a clock of the caller's own (see [`NetMessages::poll_elapsed`]).
    ///
    /// # Errors
    ///
    /// The errors of [`poll`](Self::poll).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.poll_elapsed(std::time::Duration::from_millis(16)).unwrap();
    /// ```
    pub fn poll_elapsed(&mut self, elapsed: Duration) -> io::Result<()> {
        self.net.poll_elapsed(elapsed)?;
        self.read();
        self.send_signals();
        Ok(())
    }

    /// A [`Signaling`](super::rtc::Signaling) handle that this client's
    /// relay carries (feature `webrtc`): give it to
    /// [`NetMessages::host_rtc`] or [`NetMessages::join_rtc`], with the
    /// room host's member id ([`RoomInfo::host`]) to join, and the room's
    /// members connect peer to peer. Signaling peers are member ids.
    ///
    /// From then on, relayed signaling messages go to the handle instead of
    /// arriving as [`LobbyEvent::Relayed`], and every [`poll`](Self::poll)
    /// relays what the endpoints want sent (reliably, to one member each).
    /// Messages wait while the client isn't in a room yet. Every call
    /// returns the same handle.
    ///
    /// ```
    /// use ponko::net::NetMessages;
    /// use ponko::net::lobby::LobbyClient;
    /// use ponko::net::rtc::RtcConfig;
    /// let mut lobby = LobbyClient::join("127.0.0.1:7700").unwrap();
    /// // Once in a room, as its host:
    /// let host = NetMessages::host_rtc(lobby.signaling(), RtcConfig::new(), 4).unwrap();
    /// ```
    #[cfg(feature = "webrtc")]
    pub fn signaling(&mut self) -> super::rtc::Signaling {
        self.signaling.get_or_insert_with(super::rtc::Signaling::new).clone()
    }

    #[cfg(feature = "webrtc")]
    fn send_signals(&mut self) {
        let Some(signaling) = self.signaling.clone() else {
            return;
        };
        while let Some((to, bytes)) = signaling.next_outgoing() {
            let Ok(member) = u32::try_from(to) else { continue };
            match self.relay(Some(MemberId(member)), Lane::Reliable, &bytes) {
                Ok(()) => {}
                // Not in a room yet, or the lane is full: try again next poll.
                Err(e) if matches!(e.kind(), io::ErrorKind::NotConnected | io::ErrorKind::WouldBlock) => {
                    if !self.closed {
                        signaling.unsend(to, bytes);
                    }
                    break;
                }
                // Too large for this server: the connection can't be made.
                Err(_) => {}
            }
        }
    }

    #[cfg(not(feature = "webrtc"))]
    fn send_signals(&mut self) {}

    fn read(&mut self) {
        while let Some(event) = self.net.next_event() {
            match event {
                MessageEvent::Connected(_) => {
                    self.connected = true;
                    for request in std::mem::take(&mut self.pending) {
                        // A full lane fails like any send would; the server
                        // just never sees that request.
                        let _ = self.net.send(PeerId::HOST, Lane::Reliable, &request);
                    }
                }
                MessageEvent::Disconnected(_, reason) => {
                    self.connected = false;
                    self.closed = true;
                    self.room = None;
                    self.queued = false;
                    self.pending.clear();
                    self.events.push_back(LobbyEvent::Disconnected(reason));
                }
                MessageEvent::Message { lane, bytes, .. } => match protocol::decode::<Reply>(&bytes) {
                    Some(Frame::Control(reply)) => self.reply(reply),
                    Some(Frame::Relay { member, payload }) => {
                        if let Some(inbox) = &mut self.session
                            && let Some(packet) = relay::session_packet(payload)
                        {
                            if inbox.len() < relay::INBOX {
                                inbox.push_back((MemberId(member), packet.to_vec()));
                            }
                            continue;
                        }
                        #[cfg(feature = "webrtc")]
                        if let Some(signaling) = &self.signaling
                            && signaling.deliver(u64::from(member), payload)
                        {
                            continue;
                        }
                        self.events.push_back(LobbyEvent::Relayed {
                            from: MemberId(member),
                            lane,
                            bytes: payload.to_vec(),
                        });
                    }
                    None => {} // From a newer server: skipped.
                },
            }
        }
        // A session owns this client and the game may never read its
        // events: keep the newest.
        if self.session.is_some() {
            while self.events.len() > relay::EVENTS {
                self.events.pop_front();
            }
        }
    }

    fn reply(&mut self, reply: Reply) {
        let event = match reply {
            Reply::Welcome { member, limits, .. } => {
                self.me = Some(member);
                self.limits = limits;
                LobbyEvent::Connected { me: member }
            }
            Reply::Joined { room } => {
                self.queued = false;
                self.room = Some(room.clone());
                LobbyEvent::Joined(room)
            }
            Reply::MemberJoined { member } => {
                if let Some(room) = &mut self.room
                    && !room.members.contains(&member)
                {
                    room.members.push(member);
                }
                LobbyEvent::MemberJoined(member)
            }
            Reply::MemberLeft { member } => {
                if let Some(room) = &mut self.room {
                    room.members.retain(|&m| m != member);
                }
                LobbyEvent::MemberLeft(member)
            }
            Reply::HostChanged { host } => {
                if let Some(room) = &mut self.room {
                    room.host = host;
                }
                LobbyEvent::HostChanged(host)
            }
            Reply::MetaChanged { meta } => {
                if let Some(room) = &mut self.room {
                    room.meta.clone_from(&meta);
                }
                LobbyEvent::MetaChanged(meta)
            }
            Reply::Left { reason } => {
                self.room = None;
                LobbyEvent::Left(reason)
            }
            Reply::Queued { .. } => {
                self.queued = true;
                LobbyEvent::Queued
            }
            Reply::Unqueued => {
                self.queued = false;
                LobbyEvent::Unqueued
            }
            Reply::Refused { error } => LobbyEvent::Refused(error),
        };
        self.events.push_back(event);
    }

    /// Takes the oldest event, if any.
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// while let Some(event) = lobby.next_event() { println!("{event:?}"); }
    /// ```
    pub fn next_event(&mut self) -> Option<LobbyEvent> {
        self.events.pop_front()
    }

    /// Makes a room for up to `size` members (2 up to the server's cap) and
    /// joins it as its host: [`LobbyEvent::Joined`] brings its code.
    ///
    /// # Errors
    ///
    /// As [`join_room`](Self::join_room).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.create_room(4).unwrap();
    /// ```
    pub fn create_room(&mut self, size: u16) -> io::Result<()> {
        self.request(&Request::Create {
            size,
            meta: String::new(),
        })
    }

    /// Joins the room with `code` (any letter case).
    ///
    /// # Errors
    ///
    /// `NotConnected` once the connection is gone, `WouldBlock` with too
    /// many requests waiting for the server's first answer, and the
    /// errors of [`NetMessages::send`]. What the server refuses arrives
    /// as [`LobbyEvent::Refused`].
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.join_room("KQ7X2M").unwrap();
    /// ```
    pub fn join_room(&mut self, code: &str) -> io::Result<()> {
        self.request(&Request::Join { code: code.into() })
    }

    /// Leaves the current room. If this client hosts it, the server's host
    /// migration rule picks the next host (by default the member who has
    /// been in the room longest).
    ///
    /// # Errors
    ///
    /// As [`join_room`](Self::join_room).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.leave_room().unwrap();
    /// ```
    pub fn leave_room(&mut self) -> io::Result<()> {
        self.request(&Request::Leave)
    }

    /// Waits in the matchmaking queue `mode` for a room of `size`;
    /// `rating` is for the server's matchmaking rule (the default rule
    /// fills rooms first come, first served, and ignores it).
    ///
    /// # Errors
    ///
    /// As [`join_room`](Self::join_room).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.queue("duel", 2, 0).unwrap();
    /// ```
    pub fn queue(&mut self, mode: &str, size: u16, rating: i32) -> io::Result<()> {
        self.request(&Request::Queue {
            mode: mode.into(),
            size,
            rating,
        })
    }

    /// Leaves the matchmaking queue.
    ///
    /// # Errors
    ///
    /// As [`join_room`](Self::join_room).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.cancel_queue().unwrap();
    /// ```
    pub fn cancel_queue(&mut self) -> io::Result<()> {
        self.request(&Request::CancelQueue)
    }

    /// Sets the room's text (host only): a game mode, the host's own
    /// address for players who can connect directly, a "started" flag.
    ///
    /// # Errors
    ///
    /// As [`join_room`](Self::join_room).
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// lobby.set_meta("udp=203.0.113.5:7777").unwrap();
    /// ```
    pub fn set_meta(&mut self, meta: &str) -> io::Result<()> {
        self.request(&Request::SetMeta { meta: meta.into() })
    }

    fn request(&mut self, request: &Request) -> io::Result<()> {
        if self.closed {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the lobby connection is gone",
            ));
        }
        let bytes = protocol::encode_control(request);
        if self.connected {
            return self.net.send(PeerId::HOST, Lane::Reliable, &bytes);
        }
        if self.pending.len() >= MAX_PENDING {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "too many requests before the server answered",
            ));
        }
        self.pending.push(bytes);
        Ok(())
    }

    /// Sends `bytes` through the server's relay to member `to` of this
    /// client's room, or to every other member with `None`, on `lane`. The
    /// server forwards them without reading them, within its relay rate
    /// limit (over it, it drops them). They arrive as
    /// [`LobbyEvent::Relayed`].
    ///
    /// # Errors
    ///
    /// `NotConnected` before the client is in a room, `InvalidInput` for
    /// more than the server's [`max_relay_bytes`](ServerLimits::max_relay_bytes),
    /// and the errors of [`NetMessages::send`] (`WouldBlock` when the
    /// lane's send budget is full).
    ///
    /// ```
    /// use ponko::net::Lane;
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// // Not in a room yet:
    /// assert!(lobby.relay(None, Lane::Unreliable, b"position").is_err());
    /// ```
    pub fn relay(&mut self, to: Option<MemberId>, lane: Lane, bytes: &[u8]) -> io::Result<()> {
        if self.room.is_none() || !self.connected {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "not in a lobby room"));
        }
        if bytes.len() > self.limits.max_relay_bytes as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "relay payload of {} bytes is over the server's {}",
                    bytes.len(),
                    self.limits.max_relay_bytes
                ),
            ));
        }
        let to = to.map_or(protocol::EVERYONE, MemberId::to_bits);
        protocol::encode_relay(to, bytes, &mut self.scratch);
        self.net.send(PeerId::HOST, lane, &self.scratch)
    }

    /// This client's member id, once the server answered.
    /// ```
    /// assert!(ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap().me().is_none());
    /// ```
    pub fn me(&self) -> Option<MemberId> {
        self.me
    }

    /// The room this client is in, as it is now.
    /// ```
    /// assert!(ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap().room().is_none());
    /// ```
    pub fn room(&self) -> Option<&RoomInfo> {
        self.room.as_ref()
    }

    /// Whether this client hosts its room.
    /// ```
    /// assert!(!ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap().is_host());
    /// ```
    pub fn is_host(&self) -> bool {
        self.room.as_ref().is_some_and(|r| Some(r.host) == self.me)
    }

    /// Whether this client waits in a matchmaking queue.
    /// ```
    /// assert!(!ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap().is_queued());
    /// ```
    pub fn is_queued(&self) -> bool {
        self.queued
    }

    /// Whether the server answered and the connection is up.
    /// ```
    /// assert!(!ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap().is_connected());
    /// ```
    pub fn is_connected(&self) -> bool {
        self.connected && self.me.is_some()
    }

    /// The limits the server announced (all zero before it answered).
    /// ```
    /// let lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// assert_eq!(lobby.server_limits().max_relay_bytes, 0);
    /// ```
    pub fn server_limits(&self) -> ServerLimits {
        self.limits
    }

    /// Closes the connection, telling the server (it then removes this
    /// client from its room and queue at once, instead of after a
    /// timeout).
    ///
    /// # Errors
    ///
    /// `NotConnected` if it was never made or is gone.
    ///
    /// ```
    /// let mut lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// assert!(lobby.disconnect().is_err()); // never connected
    /// ```
    pub fn disconnect(&mut self) -> io::Result<()> {
        if !self.connected {
            return Err(io::Error::new(io::ErrorKind::NotConnected, "not connected"));
        }
        self.net.disconnect(PeerId::HOST)?;
        self.read();
        Ok(())
    }

    /// The message endpoint underneath, for its counters
    /// ([`NetMessages::stats`]).
    /// ```
    /// let lobby = ponko::net::lobby::LobbyClient::join("127.0.0.1:7700").unwrap();
    /// assert_eq!(lobby.endpoint().stats().bytes_received, 0);
    /// ```
    pub fn endpoint(&self) -> &NetMessages {
        &self.net
    }
}
