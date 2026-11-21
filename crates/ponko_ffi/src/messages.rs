//! Message networking from C (Rust `NetMessages`): the game owns an
//! endpoint, polls it every frame, sends bytes on a reliable or an
//! unreliable lane, and reads connection and message events one at a time.

use std::ffi::{CString, c_char};
use std::time::Duration;

use ponko::net::Lane;
use ponko::net::MessageEvent;
use ponko::net::NetMessages;
use ponko::net::PeerId;

use crate::ctx::{out, text};
use crate::{Failure, PonkoStatus, boundary, invalid, invalid_owned};

fn io(e: std::io::Error) -> Failure {
    invalid_owned(format!("messages: {e}"))
}

/// A message endpoint from `ponko_net_messages_host`, `_join` or
/// `_loopback`. Free it with `ponko_net_messages_free`.
pub struct PonkoNetMessages {
    pub(crate) net: NetMessages,
    /// The event last handed out: its bytes and reason stay valid until
    /// the next `ponko_net_messages_next_event`.
    current: Option<MessageEvent>,
    reason: CString,
}

/// Which lane a message travels on (Rust `Lane`).
/// ```
/// let _ = ponko_ffi::PonkoLane::PONKO_LANE_RELIABLE;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoLane {
    /// Arrives, once, in the order sent (chat, purchases, state changes).
    PONKO_LANE_RELIABLE = 0,
    /// May be lost; late ones are dropped (positions, aim).
    PONKO_LANE_UNRELIABLE = 1,
    /// Reliable delivery without ordering.
    PONKO_LANE_RELIABLE_UNORDERED = 2,
}

/// What a `PonkoMessageEvent` is.
/// ```
/// let _ = ponko_ffi::PonkoMessageEventKind::PONKO_MESSAGE;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoMessageEventKind {
    /// `peer` connected.
    PONKO_MESSAGE_CONNECTED = 0,
    /// `peer` left or timed out; `reason` says why.
    PONKO_MESSAGE_DISCONNECTED = 1,
    /// `peer` sent `len` bytes at `bytes` on `lane`.
    PONKO_MESSAGE = 2,
}

/// One event from `ponko_net_messages_next_event`. `bytes` and `reason`
/// stay valid until the next call on the same endpoint (or its free).
/// ```
/// let e = ponko_ffi::PonkoMessageEvent::default();
/// assert_eq!(e.len, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoMessageEvent {
    /// What happened.
    pub kind: PonkoMessageEventKind,
    /// Who it's about. The host is always peer 0.
    pub peer: u64,
    /// The message's lane.
    pub lane: PonkoLane,
    /// The message (null for other events).
    pub bytes: *const u8,
    /// Its length in bytes.
    pub len: usize,
    /// Why a peer disconnected (NUL-terminated; empty for other events).
    pub reason: *const c_char,
}
impl Default for PonkoMessageEvent {
    fn default() -> Self {
        Self {
            kind: PonkoMessageEventKind::PONKO_MESSAGE_CONNECTED,
            peer: 0,
            lane: PonkoLane::PONKO_LANE_RELIABLE,
            bytes: std::ptr::null(),
            len: 0,
            reason: c"".as_ptr(),
        }
    }
}

/// Traffic counters (Rust `MessageStats`).
/// ```
/// assert_eq!(ponko_ffi::PonkoMessageStats::default().bytes_sent, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoMessageStats {
    /// Packets sent.
    pub packets_sent: u64,
    /// Bytes sent, headers included.
    pub bytes_sent: u64,
    /// Packets received.
    pub packets_received: u64,
    /// Bytes received, headers included.
    pub bytes_received: u64,
}

pub(crate) fn wrap(net: NetMessages) -> *mut PonkoNetMessages {
    Box::into_raw(Box::new(PonkoNetMessages {
        net,
        current: None,
        reason: CString::default(),
    }))
}
pub(crate) fn live<'a>(m: *mut PonkoNetMessages) -> Result<&'a mut PonkoNetMessages, Failure> {
    // SAFETY: callers pass null or a live endpoint.
    unsafe { m.as_mut() }.ok_or_else(|| invalid("endpoint is null"))
}
fn lane(l: PonkoLane) -> Lane {
    match l {
        PonkoLane::PONKO_LANE_RELIABLE => Lane::Reliable,
        PonkoLane::PONKO_LANE_UNRELIABLE => Lane::Unreliable,
        PonkoLane::PONKO_LANE_RELIABLE_UNORDERED => Lane::ReliableUnordered,
    }
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

/// Hosts at `addr` (e.g. "0.0.0.0:7777"; port 0 picks a free one) for up to
/// `max_clients` clients. Plain UDP: not authenticated or encrypted.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_host(c"127.0.0.1:0".as_ptr(), 4, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `addr` NUL-terminated; `out_endpoint` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_host(
    addr: *const c_char,
    max_clients: usize,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let addr = text(addr)?;
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        out(out_endpoint, wrap(NetMessages::host(addr, max_clients).map_err(io)?))
    })
}

/// Joins a host at `addr` (e.g. "192.168.1.5:7777"). `PONKO_MESSAGE_CONNECTED`
/// for peer 0 (the host) arrives once it answers.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_join(c"127.0.0.1:7777".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `addr` NUL-terminated; `out_endpoint` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_join(
    addr: *const c_char,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let addr = text(addr)?;
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        out(out_endpoint, wrap(NetMessages::join(addr).map_err(io)?))
    })
}

/// `players` endpoints joined in memory (the first hosts), for tests: they
/// talk without sockets. Writes `players` pointers to `out_endpoints`; free
/// each one.
///
/// Ownership: on success `out_endpoints` receives two owned endpoints, the
/// caller's; free each with ponko_net_messages_free (or give it to
/// ponko_game_messages, which takes it over).
///
/// ```
/// let mut e = [std::ptr::null_mut(); 2];
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_loopback(2, e.as_mut_ptr()) } as u32, 0);
/// for p in e { unsafe { ponko_ffi::ponko_net_messages_free(p) } }
/// ```
/// # Safety
/// `out_endpoints` writable for `players` pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_loopback(
    players: usize,
    out_endpoints: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    boundary(|| {
        if out_endpoints.is_null() {
            return Err(invalid("output is null"));
        }
        if players < 2 {
            return Err(invalid("a loopback needs at least 2 players"));
        }
        for (i, net) in NetMessages::loopback(players).into_iter().enumerate() {
            // SAFETY: the caller made room for `players` pointers.
            unsafe { out_endpoints.add(i).write(wrap(net)) };
        }
        Ok(())
    })
}

/// Closes an endpoint (without telling peers; disconnect them first for
/// that). Null does nothing.
/// ```
/// unsafe { ponko_ffi::ponko_net_messages_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `endpoint` null or live; not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_free(endpoint: *mut PonkoNetMessages) {
    if !endpoint.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(endpoint) });
    }
}

/// How long a silent peer may stay connected, in milliseconds.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_set_timeout(std::ptr::null_mut(), 5000) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_set_timeout(endpoint: *mut PonkoNetMessages, ms: u64) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        // `timeout` takes the endpoint by value; swap a placeholder in meanwhile.
        let net = std::mem::replace(&mut m.net, placeholder());
        m.net = net.timeout(Duration::from_millis(ms));
        Ok(())
    })
}
fn placeholder() -> NetMessages {
    NetMessages::loopback(2).swap_remove(0)
}

/// The UDP port this endpoint listens on (0 for loopback ones).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_local_port(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live; `out_port` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_local_port(
    endpoint: *mut PonkoNetMessages,
    out_port: *mut u16,
) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        let port = m.net.local_addr().map_or(0, |a| a.port());
        // SAFETY: forwarded caller contract.
        unsafe { out(out_port, port) }
    })
}

/// Sends `len` bytes (at most 1 MiB) to `peer` on `lane`. Clients send to
/// the host, peer 0.
/// ```
/// use ponko_ffi::PonkoLane::PONKO_LANE_RELIABLE;
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_send(std::ptr::null_mut(), 0, PONKO_LANE_RELIABLE, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live; `bytes` readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_send(
    endpoint: *mut PonkoNetMessages,
    peer: u64,
    lane_: PonkoLane,
    bytes: *const u8,
    len: usize,
) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        // SAFETY: forwarded caller contract.
        let bytes = unsafe { payload(bytes, len)? };
        m.net.send(PeerId::from_bits(peer), lane(lane_), bytes).map_err(io)
    })
}

/// Sends `len` bytes to every connected peer on `lane`.
/// ```
/// use ponko_ffi::PonkoLane::PONKO_LANE_UNRELIABLE;
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_send_to_all(std::ptr::null_mut(), PONKO_LANE_UNRELIABLE, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live; `bytes` readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_send_to_all(
    endpoint: *mut PonkoNetMessages,
    lane_: PonkoLane,
    bytes: *const u8,
    len: usize,
) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        // SAFETY: forwarded caller contract.
        let bytes = unsafe { payload(bytes, len)? };
        m.net.send_to_all(lane(lane_), bytes).map_err(io)
    })
}

/// Sends what's queued and receives what arrived. Call it once a frame,
/// then read events until there are none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_poll(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_poll(endpoint: *mut PonkoNetMessages) -> PonkoStatus {
    boundary(|| live(endpoint)?.net.poll().map_err(io))
}

/// Takes the oldest event into `*out_event`; `*out_found` is false when
/// there are none left. Its bytes and reason stay valid until the next call.
/// ```
/// let mut e = Default::default();
/// let mut found = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_next_event(std::ptr::null_mut(), &mut e, &mut found) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_next_event(
    endpoint: *mut PonkoNetMessages,
    out_event: *mut PonkoMessageEvent,
    out_found: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        if out_event.is_null() || out_found.is_null() {
            return Err(invalid("output is null"));
        }
        m.current = m.net.next_event();
        let mut e = PonkoMessageEvent::default();
        match &m.current {
            None => {
                // SAFETY: checked non-null above.
                return unsafe { out(out_found, false) };
            }
            Some(MessageEvent::Connected(peer)) => e.peer = peer.to_bits(),
            Some(MessageEvent::Disconnected(peer, reason)) => {
                e.kind = PonkoMessageEventKind::PONKO_MESSAGE_DISCONNECTED;
                e.peer = peer.to_bits();
                m.reason = CString::new(reason.replace('\0', " ")).unwrap_or_default();
                e.reason = m.reason.as_ptr();
            }
            Some(MessageEvent::Message { peer, lane, bytes }) => {
                e.kind = PonkoMessageEventKind::PONKO_MESSAGE;
                e.peer = peer.to_bits();
                e.lane = match lane {
                    Lane::Reliable => PonkoLane::PONKO_LANE_RELIABLE,
                    Lane::Unreliable => PonkoLane::PONKO_LANE_UNRELIABLE,
                    Lane::ReliableUnordered => PonkoLane::PONKO_LANE_RELIABLE_UNORDERED,
                };
                e.bytes = bytes.as_ptr();
                e.len = bytes.len();
            }
        }
        // SAFETY: checked non-null above.
        unsafe {
            out_event.write(e);
            out(out_found, true)
        }
    })
}

/// Disconnects `peer`, telling it so.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_disconnect(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_disconnect(endpoint: *mut PonkoNetMessages, peer: u64) -> PonkoStatus {
    boundary(|| live(endpoint)?.net.disconnect(PeerId::from_bits(peer)).map_err(io))
}

/// Traffic so far.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_stats(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `endpoint` null or live; `out_stats` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_stats(
    endpoint: *mut PonkoNetMessages,
    out_stats: *mut PonkoMessageStats,
) -> PonkoStatus {
    boundary(|| {
        let s = live(endpoint)?.net.stats();
        // SAFETY: forwarded caller contract.
        unsafe {
            out(
                out_stats,
                PonkoMessageStats {
                    packets_sent: s.packets_sent,
                    bytes_sent: s.bytes_sent,
                    packets_received: s.packets_received,
                    bytes_received: s.bytes_received,
                },
            )
        }
    })
}

/// Host a message endpoint over Steam; requires the `steam` feature.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// # Safety
/// `out_endpoint` writable. On failure it is set to null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_host(480, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_host(
    app_id: u32,
    max_clients: usize,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    boundary(|| {
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: checked writable output under caller contract.
        unsafe {
            out_endpoint.write(std::ptr::null_mut());
        }
        if !(1..=249).contains(&max_clients) {
            return Err(invalid("max_clients must be 1..=249"));
        }
        #[cfg(feature = "steam")]
        {
            let client = crate::steam::client::get(app_id)?;
            let net = NetMessages::steam_host(&client, max_clients).map_err(io)?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_endpoint, wrap(net)) }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, max_clients);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Join a message endpoint over Steam; requires the `steam` feature.
///
/// Ownership: on success `out_endpoint` receives an owned endpoint, the
/// caller's; free it with ponko_net_messages_free. (or give it to
/// ponko_game_messages, which takes it over).
///
/// # Safety
/// `out_endpoint` writable. On failure it is set to null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_join(480, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_join(
    app_id: u32,
    lobby: u64,
    out_endpoint: *mut *mut PonkoNetMessages,
) -> PonkoStatus {
    boundary(|| {
        if out_endpoint.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: checked writable output under caller contract.
        unsafe {
            out_endpoint.write(std::ptr::null_mut());
        }
        if lobby == 0 {
            return Err(invalid("lobby id is zero"));
        }
        #[cfg(feature = "steam")]
        {
            let client = crate::steam::client::get(app_id)?;
            let net = NetMessages::steam_join(&client, ponko::net::steam::LobbyId::from_raw(lobby)).map_err(io)?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_endpoint, wrap(net)) }
        }
        #[cfg(not(feature = "steam"))]
        {
            let _ = (app_id, lobby);
            Err(invalid("ponko_ffi was built without the `steam` feature"))
        }
    })
}

/// Steam identity of a connected peer, or zero for unknown/non-Steam peers.
/// # Safety
/// Live/null endpoint; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_net_messages_steam_peer(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_net_messages_steam_peer(
    endpoint: *mut PonkoNetMessages,
    peer: u64,
    output: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        let m = live(endpoint)?;
        #[cfg(feature = "steam")]
        let value = m.net.steam_peer(PeerId::from_bits(peer)).map_or(0, |id| id.raw());
        #[cfg(not(feature = "steam"))]
        let value = {
            let _ = (m, peer);
            0
        };
        // SAFETY: forwarded caller contract.
        unsafe { out(output, value) }
    })
}
