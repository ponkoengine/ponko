//! Lockstep networking from C: host or join a session, send this player's
//! input as bytes, read everyone's.
use std::ffi::{c_char, c_void};

use ponko::net::Net;
use ponko::net::NetState;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::game::build;
use crate::{Failure, PonkoGame, PonkoStatus, boundary, invalid};

/// Where a lockstep session is (`ponko_ctx_net_state`).
/// ```
/// assert_eq!(ponko_ffi::PonkoNetState::PONKO_NET_RUNNING as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoNetState {
    /// No session: one local player.
    PONKO_NET_OFFLINE,
    /// Waiting for players (host) or for the host (join).
    PONKO_NET_CONNECTING,
    /// Everyone is in.
    PONKO_NET_RUNNING,
    /// A peer went quiet, or the host refused us; ticks stopped.
    PONKO_NET_LOST,
}

/// Gives the game its session.
/// # Safety
/// `game` null or live.
unsafe fn session(game: *mut PonkoGame, net: Option<Net>, status: PonkoStatus) -> PonkoStatus {
    match net {
        // SAFETY: forwarded caller contract.
        Some(net) => unsafe { build(game, |g| (g.lockstep(net), Ok(()))) },
        None => status,
    }
}

/// Hosts a lockstep session for `players` players (2-8, the host is player
/// 0) on `addr` ("0.0.0.0:7777"; port 0 picks one, written to `*out_port`
/// if not null). `input_delay` ticks (at most 64; 3 is a good start) hide
/// latency; worlds are compared every `checksum_every` ticks (0 never).
/// The game starts when everyone joined; ticks then run only once every
/// player's input is in.
/// # Safety
/// Live game; `addr` NUL-terminated; `out_port` null or writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_lockstep_host(std::ptr::null_mut(), c"127.0.0.1:0".as_ptr(), 2, 3, 60, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_lockstep_host(
    game: *mut PonkoGame,
    addr: *const c_char,
    players: u32,
    input_delay: u32,
    checksum_every: u32,
    out_port: *mut u16,
) -> PonkoStatus {
    let mut net = None;
    let status = boundary(|| {
        if game.is_null() {
            return Err(invalid("game is null"));
        }
        if input_delay > 64 {
            return Err(invalid("input delay is at most 64 ticks"));
        }
        // SAFETY: forwarded caller contract.
        let addr = unsafe { text(addr)? };
        let n = Net::host(addr, players as usize)?
            .input_delay(input_delay)
            .checksum_every(checksum_every);
        if !out_port.is_null() {
            // SAFETY: checked non-null; caller supplies writable storage.
            unsafe { out_port.write(n.local_addr().map_or(0, |a| a.port())) };
        }
        net = Some(n);
        Ok(())
    });
    // SAFETY: forwarded caller contract.
    unsafe { session(game, net, status) }
}

/// Joins the session hosted at `addr` ("192.168.1.20:7777"). The host
/// decides players, delay and checks; both must use the same seed.
/// # Safety
/// Live game; `addr` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_lockstep_join(std::ptr::null_mut(), c"127.0.0.1:7777".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_lockstep_join(game: *mut PonkoGame, addr: *const c_char) -> PonkoStatus {
    let mut net = None;
    let status = boundary(|| {
        if game.is_null() {
            return Err(invalid("game is null"));
        }
        // SAFETY: forwarded caller contract.
        let addr = unsafe { text(addr)? };
        net = Some(Net::join(addr)?);
        Ok(())
    });
    // SAFETY: forwarded caller contract.
    unsafe { session(game, net, status) }
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        crate::invalid_owned(ponko::prelude::Error::new(
            ponko::prelude::ErrorKind::Io,
            format!("lockstep: {e}"),
        ))
    }
}

/// This player's number (0 = host, also offline) and the player count.
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_player(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_player(
    ctx: *mut PonkoCtx,
    out_player: *mut u32,
    out_players: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        if out_player.is_null() || out_players.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let (p, n) = ctx_access(ctx, |c| Ok((c.net.player(), c.net.players())))?;
            out(out_player, p as u32)?;
            out(out_players, n as u32)
        }
    })
}

/// Where the session is: a `PonkoNetState`.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_state(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_state(ctx: *mut PonkoCtx, out_state: *mut u32) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            let state = ctx_access(ctx, |c| {
                Ok(match c.net.state() {
                    NetState::Offline => PonkoNetState::PONKO_NET_OFFLINE,
                    NetState::Connecting => PonkoNetState::PONKO_NET_CONNECTING,
                    NetState::Running => PonkoNetState::PONKO_NET_RUNNING,
                    NetState::Lost => PonkoNetState::PONKO_NET_LOST,
                    _ => unreachable!("a new NetState needs a PonkoNetState"),
                })
            })?;
            out(out_state, state as u32)
        }
    })
}

/// Sets this player's input for the next tick it can go into: `size`
/// bytes (at most 1024) copied from `data`. The last call before that tick
/// wins; the input is cleared after it's sent.
/// # Safety
/// Live ctx; `data` readable for `size` bytes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_input(std::ptr::null_mut(), std::ptr::null(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_input(ctx: *mut PonkoCtx, data: *const c_void, size: usize) -> PonkoStatus {
    boundary(|| {
        if size > ponko::net::MAX_INPUT_BYTES {
            return Err(invalid("input is over 1024 bytes"));
        }
        if data.is_null() && size != 0 {
            return Err(invalid("input data is null"));
        }
        let bytes = if size == 0 {
            &[][..]
        } else {
            // SAFETY: caller supplies `size` readable bytes.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }
        };
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                c.net.set_input_bytes(bytes);
                Ok(())
            })
        }
    })
}

/// `player`'s input for the current tick: `*out_size` bytes at `*out_data`
/// (size 0 if they sent none), valid until this system returns.
///
/// Ownership: `out_data` receives a view of bytes the engine owns, valid until
/// the system returns; never free it.
///
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_input(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_input(
    ctx: *mut PonkoCtx,
    player: u32,
    out_data: *mut *const c_void,
    out_size: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_data.is_null() || out_size.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract; the bytes live in ctx.net until
        // the next tick, which can't start while this system runs.
        unsafe {
            let (ptr, len) = ctx_access(ctx, |c| {
                let b = c.net.input_bytes(player as usize);
                Ok((b.as_ptr().cast::<c_void>(), b.len()))
            })?;
            out(out_data, ptr)?;
            out(out_size, len)
        }
    })
}

/// Whether the peers' worlds stopped matching; if so, the first checked
/// tick that differed and the player whose world differs (the components
/// that differ are printed to stderr).
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_net_desync(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_net_desync(
    ctx: *mut PonkoCtx,
    out_found: *mut bool,
    out_tick: *mut u64,
    out_player: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        if out_found.is_null() || out_tick.is_null() || out_player.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let d = ctx_access(ctx, |c| Ok(c.net.desync().map(|d| (d.tick, d.player as u32))))?;
            out(out_found, d.is_some())?;
            out(out_tick, d.map_or(0, |d| d.0))?;
            out(out_player, d.map_or(0, |d| d.1))
        }
    })
}
