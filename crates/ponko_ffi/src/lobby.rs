//! The lobby client from C (Rust `ponko::net::lobby::LobbyClient`): rooms
//! with codes, matchmaking, host migration and a relay, through a lobby
//! server (`ponko-lobby`). The game owns the client, polls it every frame
//! and reads its events one at a time.

use std::ffi::{CString, c_char};

use ponko::net::Lane;
use ponko::net::lobby::{LeaveReason, LobbyClient, LobbyError, LobbyEvent, MemberId};

use crate::ctx::{out, text};
use crate::messages::PonkoLane;
use crate::{Failure, PonkoStatus, boundary, invalid, invalid_owned};

fn io(e: std::io::Error) -> Failure {
    invalid_owned(format!("lobby: {e}"))
}

/// A lobby client from `ponko_lobby_join`. Free
/// it with `ponko_lobby_free`.
pub struct PonkoLobby {
    pub(crate) client: LobbyClient,
    /// The event last handed out: its bytes and text stay valid until the
    /// next `ponko_lobby_next_event`.
    current: Option<LobbyEvent>,
    text: CString,
}

/// What a `PonkoLobbyEvent` is (Rust `LobbyEvent`).
/// ```
/// let _ = ponko_ffi::PonkoLobbyEventKind::PONKO_LOBBY_JOINED;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoLobbyEventKind {
    /// The server answered; `member` is this client's id.
    PONKO_LOBBY_CONNECTED = 0,
    /// This client is in a room; `text` is its code, `member` its host
    /// (`ponko_lobby_state` and `ponko_lobby_members` have the rest).
    PONKO_LOBBY_JOINED = 1,
    /// `member` joined the room.
    PONKO_LOBBY_MEMBER_JOINED = 2,
    /// `member` left the room.
    PONKO_LOBBY_MEMBER_LEFT = 3,
    /// `member` hosts the room now.
    PONKO_LOBBY_HOST_CHANGED = 4,
    /// The room's text is `text`.
    PONKO_LOBBY_META_CHANGED = 5,
    /// This client is out of its room; `reason` says why.
    PONKO_LOBBY_LEFT = 6,
    /// This client waits in a matchmaking queue.
    PONKO_LOBBY_QUEUED = 7,
    /// This client left the matchmaking queue.
    PONKO_LOBBY_UNQUEUED = 8,
    /// The server refused a request; `error` says why.
    PONKO_LOBBY_REFUSED = 9,
    /// `member` sent `len` bytes at `bytes` on `lane` through the relay.
    PONKO_LOBBY_RELAYED = 10,
    /// The connection is gone; `text` says why.
    PONKO_LOBBY_DISCONNECTED = 11,
}

/// Why the server refused a request (Rust `LobbyError`).
/// ```
/// let _ = ponko_ffi::PonkoLobbyError::PONKO_LOBBY_ROOM_FULL;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoLobbyError {
    /// Not a refusal (other events).
    PONKO_LOBBY_NO_ERROR = 0,
    /// No room has that code.
    PONKO_LOBBY_ROOM_NOT_FOUND = 1,
    /// The room is full.
    PONKO_LOBBY_ROOM_FULL = 2,
    /// Already in a room.
    PONKO_LOBBY_ALREADY_IN_ROOM = 3,
    /// Already waiting in a matchmaking queue.
    PONKO_LOBBY_ALREADY_QUEUED = 4,
    /// Not in a room (or a queue).
    PONKO_LOBBY_NOT_IN_ROOM = 5,
    /// Only the host may do that.
    PONKO_LOBBY_NOT_HOST = 6,
    /// Too many requests from this address.
    PONKO_LOBBY_RATE_LIMITED = 7,
    /// The message or room text is too long.
    PONKO_LOBBY_TOO_LARGE = 8,
    /// The room size is out of range.
    PONKO_LOBBY_BAD_SIZE = 9,
    /// The server holds as many rooms as it may.
    PONKO_LOBBY_SERVER_FULL = 10,
    /// Too many connections from this address.
    PONKO_LOBBY_TOO_MANY_CONNECTIONS = 11,
    /// The server could not read the request.
    PONKO_LOBBY_BAD_REQUEST = 12,
    /// An error this library doesn't know.
    PONKO_LOBBY_OTHER_ERROR = 13,
}

/// Why a client is out of its room (Rust `LeaveReason`).
/// ```
/// let _ = ponko_ffi::PonkoLobbyLeaveReason::PONKO_LOBBY_LEFT_REQUESTED;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoLobbyLeaveReason {
    /// Not a leave (other events).
    PONKO_LOBBY_LEFT_NONE = 0,
    /// The client asked to leave.
    PONKO_LOBBY_LEFT_REQUESTED = 1,
    /// The host left and the room was closed.
    PONKO_LOBBY_LEFT_ROOM_CLOSED = 2,
    /// The room (or the connection) was idle too long.
    PONKO_LOBBY_LEFT_IDLE = 3,
    /// The server is stopping.
    PONKO_LOBBY_LEFT_SERVER_STOPPING = 4,
    /// A reason this library doesn't know.
    PONKO_LOBBY_LEFT_OTHER = 5,
}

/// One event from `ponko_lobby_next_event`. `bytes` and `text` stay valid
/// until the next call on the same client (or its free).
/// ```
/// let e = ponko_ffi::PonkoLobbyEvent::default();
/// assert_eq!(e.len, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoLobbyEvent {
    /// What happened.
    pub kind: PonkoLobbyEventKind,
    /// The member it is about (0 when none).
    pub member: u32,
    /// The relayed message's lane.
    pub lane: PonkoLane,
    /// The relayed message (null for other events).
    pub bytes: *const u8,
    /// Its length in bytes.
    pub len: usize,
    /// A room code, room text or disconnect reason (NUL-terminated; empty
    /// for other events).
    pub text: *const c_char,
    /// Why a request was refused.
    pub error: PonkoLobbyError,
    /// Why the client left its room.
    pub reason: PonkoLobbyLeaveReason,
}
impl Default for PonkoLobbyEvent {
    fn default() -> Self {
        Self {
            kind: PonkoLobbyEventKind::PONKO_LOBBY_CONNECTED,
            member: 0,
            lane: PonkoLane::PONKO_LANE_RELIABLE,
            bytes: std::ptr::null(),
            len: 0,
            text: c"".as_ptr(),
            error: PonkoLobbyError::PONKO_LOBBY_NO_ERROR,
            reason: PonkoLobbyLeaveReason::PONKO_LOBBY_LEFT_NONE,
        }
    }
}

/// A lobby client's state now (Rust `LobbyClient::me`, `room`, `is_host`,
/// `is_queued`).
/// ```
/// let s = ponko_ffi::PonkoLobbyState::default();
/// assert!(!s.in_room);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoLobbyState {
    /// The server answered and the connection is up.
    pub connected: bool,
    /// This client's member id (0 before the server answered).
    pub me: u32,
    /// In a room.
    pub in_room: bool,
    /// Hosts its room.
    pub is_host: bool,
    /// Waits in a matchmaking queue.
    pub queued: bool,
    /// The room's host (0 outside a room).
    pub host: u32,
    /// The room's size (0 outside a room).
    pub size: u16,
    /// Members in the room now.
    pub members: u32,
    /// The room's code, NUL-terminated (empty outside a room).
    pub code: [c_char; 8],
    /// The largest relay payload the server forwards (0 before it answered).
    pub max_relay_bytes: u32,
}

fn wrap(client: LobbyClient) -> *mut PonkoLobby {
    Box::into_raw(Box::new(PonkoLobby {
        client,
        current: None,
        text: CString::default(),
    }))
}
pub(crate) fn live<'a>(lobby: *mut PonkoLobby) -> Result<&'a mut PonkoLobby, Failure> {
    // SAFETY: callers pass null or a live client.
    unsafe { lobby.as_mut() }.ok_or_else(|| invalid("lobby is null"))
}
fn rust_lane(l: PonkoLane) -> Lane {
    match l {
        PonkoLane::PONKO_LANE_RELIABLE => Lane::Reliable,
        PonkoLane::PONKO_LANE_UNRELIABLE => Lane::Unreliable,
        PonkoLane::PONKO_LANE_RELIABLE_UNORDERED => Lane::ReliableUnordered,
    }
}
fn c_lane(l: Lane) -> PonkoLane {
    match l {
        Lane::Reliable => PonkoLane::PONKO_LANE_RELIABLE,
        Lane::Unreliable => PonkoLane::PONKO_LANE_UNRELIABLE,
        Lane::ReliableUnordered => PonkoLane::PONKO_LANE_RELIABLE_UNORDERED,
    }
}
fn c_error(e: LobbyError) -> PonkoLobbyError {
    use PonkoLobbyError as C;
    match e {
        LobbyError::RoomNotFound => C::PONKO_LOBBY_ROOM_NOT_FOUND,
        LobbyError::RoomFull => C::PONKO_LOBBY_ROOM_FULL,
        LobbyError::AlreadyInRoom => C::PONKO_LOBBY_ALREADY_IN_ROOM,
        LobbyError::AlreadyQueued => C::PONKO_LOBBY_ALREADY_QUEUED,
        LobbyError::NotInRoom => C::PONKO_LOBBY_NOT_IN_ROOM,
        LobbyError::NotHost => C::PONKO_LOBBY_NOT_HOST,
        LobbyError::RateLimited => C::PONKO_LOBBY_RATE_LIMITED,
        LobbyError::TooLarge => C::PONKO_LOBBY_TOO_LARGE,
        LobbyError::BadSize => C::PONKO_LOBBY_BAD_SIZE,
        LobbyError::ServerFull => C::PONKO_LOBBY_SERVER_FULL,
        LobbyError::TooManyConnections => C::PONKO_LOBBY_TOO_MANY_CONNECTIONS,
        LobbyError::BadRequest => C::PONKO_LOBBY_BAD_REQUEST,
        _ => C::PONKO_LOBBY_OTHER_ERROR,
    }
}
fn c_reason(r: LeaveReason) -> PonkoLobbyLeaveReason {
    use PonkoLobbyLeaveReason as C;
    match r {
        LeaveReason::Requested => C::PONKO_LOBBY_LEFT_REQUESTED,
        LeaveReason::RoomClosed => C::PONKO_LOBBY_LEFT_ROOM_CLOSED,
        LeaveReason::Idle => C::PONKO_LOBBY_LEFT_IDLE,
        LeaveReason::ServerStopping => C::PONKO_LOBBY_LEFT_SERVER_STOPPING,
        _ => C::PONKO_LOBBY_LEFT_OTHER,
    }
}
/// Whether the C API has a kind for `event`.
fn named(event: &LobbyEvent) -> bool {
    matches!(
        event,
        LobbyEvent::Connected { .. }
            | LobbyEvent::Joined(_)
            | LobbyEvent::MemberJoined(_)
            | LobbyEvent::MemberLeft(_)
            | LobbyEvent::HostChanged(_)
            | LobbyEvent::MetaChanged(_)
            | LobbyEvent::Left(_)
            | LobbyEvent::Queued
            | LobbyEvent::Unqueued
            | LobbyEvent::Refused(_)
            | LobbyEvent::Relayed { .. }
            | LobbyEvent::Disconnected(_)
    )
}
/// # Safety
/// `bytes` must hold `len` bytes when `len` > 0.
unsafe fn payload<'a>(bytes: *const u8, len: usize) -> Result<&'a [u8], Failure> {
    if len == 0 {
        Ok(&[])
    } else if bytes.is_null() {
        Err(invalid("bytes is null"))
    } else {
        // SAFETY: forwarded caller contract.
        Ok(unsafe { std::slice::from_raw_parts(bytes, len) })
    }
}

/// Starts connecting to a lobby server at `addr` (e.g. "lobby.example.com:7700")
/// over UDP. Poll it until a `PONKO_LOBBY_CONNECTED` (or
/// `PONKO_LOBBY_DISCONNECTED`) event.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_join(c"127.0.0.1:7700".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `addr` a NUL-terminated string; `out_lobby` writable (null on failure).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_join(addr: *const c_char, out_lobby: *mut *mut PonkoLobby) -> PonkoStatus {
    boundary(|| {
        if out_lobby.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: checked non-null above.
        unsafe { out_lobby.write(std::ptr::null_mut()) };
        // SAFETY: forwarded caller contract.
        let addr = unsafe { text(addr) }?;
        let client = LobbyClient::join(addr).map_err(io)?;
        // SAFETY: checked non-null above.
        unsafe { out(out_lobby, wrap(client)) }
    })
}

/// Frees a lobby client (null does nothing). The server notices it is gone
/// after its timeout; call `ponko_lobby_disconnect` first to tell it at once.
/// ```
/// unsafe { ponko_ffi::ponko_lobby_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `lobby` null or live, and not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_free(lobby: *mut PonkoLobby) {
    if !lobby.is_null() {
        // SAFETY: the caller hands back a client from this library.
        drop(unsafe { Box::from_raw(lobby) });
    }
}

/// Sends what's queued and turns what arrived into events; call it every
/// frame, then `ponko_lobby_next_event` until there are none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_poll(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_poll(lobby: *mut PonkoLobby) -> PonkoStatus {
    boundary(|| live(lobby)?.client.poll().map_err(io))
}

/// Takes the oldest event into `*out_event`; `*out_found` is false when
/// there are none left. Its bytes and text stay valid until the next call.
/// ```
/// let mut e = Default::default();
/// let mut found = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_next_event(std::ptr::null_mut(), &mut e, &mut found) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_next_event(
    lobby: *mut PonkoLobby,
    out_event: *mut PonkoLobbyEvent,
    out_found: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let l = live(lobby)?;
        if out_event.is_null() || out_found.is_null() {
            return Err(invalid("output is null"));
        }
        use PonkoLobbyEventKind as K;
        let mut e = PonkoLobbyEvent::default();
        let mut text: Option<&str> = None;
        // Events this C API doesn't name (from a newer engine) are skipped.
        let event = loop {
            l.current = l.client.next_event();
            match &l.current {
                None => {
                    // SAFETY: checked non-null above.
                    return unsafe { out(out_found, false) };
                }
                Some(event) if named(event) => break l.current.as_ref().ok_or_else(|| invalid("no event"))?,
                Some(_) => {}
            }
        };
        match event {
            LobbyEvent::Connected { me } => e.member = me.to_bits(),
            LobbyEvent::Joined(room) => {
                e.kind = K::PONKO_LOBBY_JOINED;
                e.member = room.host.to_bits();
                text = Some(&room.code);
            }
            LobbyEvent::MemberJoined(m) => (e.kind, e.member) = (K::PONKO_LOBBY_MEMBER_JOINED, m.to_bits()),
            LobbyEvent::MemberLeft(m) => (e.kind, e.member) = (K::PONKO_LOBBY_MEMBER_LEFT, m.to_bits()),
            LobbyEvent::HostChanged(m) => (e.kind, e.member) = (K::PONKO_LOBBY_HOST_CHANGED, m.to_bits()),
            LobbyEvent::MetaChanged(meta) => {
                e.kind = K::PONKO_LOBBY_META_CHANGED;
                text = Some(meta);
            }
            LobbyEvent::Left(reason) => (e.kind, e.reason) = (K::PONKO_LOBBY_LEFT, c_reason(*reason)),
            LobbyEvent::Queued => e.kind = K::PONKO_LOBBY_QUEUED,
            LobbyEvent::Unqueued => e.kind = K::PONKO_LOBBY_UNQUEUED,
            LobbyEvent::Refused(error) => (e.kind, e.error) = (K::PONKO_LOBBY_REFUSED, c_error(*error)),
            LobbyEvent::Relayed { from, lane, bytes } => {
                e.kind = K::PONKO_LOBBY_RELAYED;
                e.member = from.to_bits();
                e.lane = c_lane(*lane);
                e.bytes = bytes.as_ptr();
                e.len = bytes.len();
            }
            LobbyEvent::Disconnected(why) => {
                e.kind = K::PONKO_LOBBY_DISCONNECTED;
                text = Some(why);
            }
            _ => {}
        }
        if let Some(t) = text {
            l.text = CString::new(t.replace('\0', " ")).unwrap_or_default();
            e.text = l.text.as_ptr();
        }
        // SAFETY: checked non-null above.
        unsafe {
            out_event.write(e);
            out(out_found, true)
        }
    })
}

/// Makes a room for up to `size` members (2 up to the server's cap) and
/// joins it as host; a `PONKO_LOBBY_JOINED` event brings its code. Sent once
/// the server answered.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_create_room(std::ptr::null_mut(), 4) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_create_room(lobby: *mut PonkoLobby, size: u16) -> PonkoStatus {
    boundary(|| live(lobby)?.client.create_room(size).map_err(io))
}

/// Joins the room with `code` (any letter case).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_join_room(std::ptr::null_mut(), c"KQ7X2M".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `code` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_join_room(lobby: *mut PonkoLobby, code: *const c_char) -> PonkoStatus {
    boundary(|| {
        let l = live(lobby)?;
        // SAFETY: forwarded caller contract.
        let code = unsafe { text(code) }?;
        l.client.join_room(code).map_err(io)
    })
}

/// Leaves the room; if this client hosted it, the server picks the next host.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_leave_room(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_leave_room(lobby: *mut PonkoLobby) -> PonkoStatus {
    boundary(|| live(lobby)?.client.leave_room().map_err(io))
}

/// Waits in the matchmaking queue `mode` for a room of `size`; `rating` is
/// for the server's matchmaking rule (the default one ignores it).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_queue(std::ptr::null_mut(), c"duel".as_ptr(), 2, 0) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `mode` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_queue(
    lobby: *mut PonkoLobby,
    mode: *const c_char,
    size: u16,
    rating: i32,
) -> PonkoStatus {
    boundary(|| {
        let l = live(lobby)?;
        // SAFETY: forwarded caller contract.
        let mode = unsafe { text(mode) }?;
        l.client.queue(mode, size, rating).map_err(io)
    })
}

/// Leaves the matchmaking queue.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_cancel_queue(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_cancel_queue(lobby: *mut PonkoLobby) -> PonkoStatus {
    boundary(|| live(lobby)?.client.cancel_queue().map_err(io))
}

/// Sets the room's text (host only): a mode, the host's own address, ...
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_set_meta(std::ptr::null_mut(), c"duel".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `meta` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_set_meta(lobby: *mut PonkoLobby, meta: *const c_char) -> PonkoStatus {
    boundary(|| {
        let l = live(lobby)?;
        // SAFETY: forwarded caller contract.
        let meta = unsafe { text(meta) }?;
        l.client.set_meta(meta).map_err(io)
    })
}

/// Sends `len` bytes through the server's relay to member `to` of this
/// client's room (0: every other member) on `lane`. The server forwards them
/// unread, within its relay rate limit.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_relay(std::ptr::null_mut(), 0, ponko_ffi::PonkoLane::PONKO_LANE_RELIABLE, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `bytes` holds `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_relay(
    lobby: *mut PonkoLobby,
    to: u32,
    lane: PonkoLane,
    bytes: *const u8,
    len: usize,
) -> PonkoStatus {
    boundary(|| {
        let l = live(lobby)?;
        // SAFETY: forwarded caller contract.
        let bytes = unsafe { payload(bytes, len) }?;
        let to = (to != 0).then_some(MemberId::from_bits(to));
        l.client.relay(to, rust_lane(lane), bytes).map_err(io)
    })
}

/// Closes the connection, telling the server (it then takes this client out
/// of its room at once).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_disconnect(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_disconnect(lobby: *mut PonkoLobby) -> PonkoStatus {
    boundary(|| live(lobby)?.client.disconnect().map_err(io))
}

/// The client's state now: connected, its id, its room.
/// ```
/// let mut s = ponko_ffi::PonkoLobbyState::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_state(std::ptr::null_mut(), &mut s) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `out_state` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_state(lobby: *mut PonkoLobby, out_state: *mut PonkoLobbyState) -> PonkoStatus {
    boundary(|| {
        let c = &live(lobby)?.client;
        let mut s = PonkoLobbyState {
            connected: c.is_connected(),
            me: c.me().map_or(0, MemberId::to_bits),
            in_room: c.room().is_some(),
            is_host: c.is_host(),
            queued: c.is_queued(),
            max_relay_bytes: c.server_limits().max_relay_bytes,
            ..PonkoLobbyState::default()
        };
        if let Some(room) = c.room() {
            s.host = room.host.to_bits();
            s.size = room.size;
            s.members = u32::try_from(room.members.len()).unwrap_or(u32::MAX);
            for (slot, b) in s.code.iter_mut().zip(room.code.bytes().take(7)) {
                *slot = b as c_char;
            }
        }
        // SAFETY: forwarded caller contract.
        unsafe { out(out_state, s) }
    })
}

/// The room's members in the order they came in. `*out_count` is how many
/// there are; with a null `members` and zero `capacity` it only counts.
/// Too small a buffer is an error, without partial output.
/// ```
/// let mut n = 0usize;
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_members(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `members` holds `capacity` values; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_members(
    lobby: *mut PonkoLobby,
    members: *mut u32,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let c = &live(lobby)?.client;
        let list = c.room().map_or(&[][..], |r| &r.members[..]);
        // SAFETY: forwarded caller contract.
        unsafe { out(out_count, list.len()) }?;
        if capacity == 0 && list.is_empty() || capacity == 0 && members.is_null() {
            return Ok(());
        }
        if members.is_null() || capacity < list.len() {
            return Err(invalid("members buffer too small or null"));
        }
        for (i, m) in list.iter().enumerate() {
            // SAFETY: checked capacity; the caller's buffer holds `capacity` values.
            unsafe { members.add(i).write(m.to_bits()) };
        }
        Ok(())
    })
}

/// The room's text, NUL-terminated into `buffer` when it fits;
/// `*out_len` is its length without the NUL (empty outside a room).
/// ```
/// let mut n = 0usize;
/// assert_eq!(unsafe { ponko_ffi::ponko_lobby_room_meta(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `lobby` null or live; `buffer` holds `capacity` bytes; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lobby_room_meta(
    lobby: *mut PonkoLobby,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let c = &live(lobby)?.client;
        let meta = c.room().map_or("", |r| r.meta.as_str());
        // SAFETY: forwarded caller contract.
        unsafe { crate::ui_retained::copy_string(meta, buffer, capacity, out_len) }
    })
}
