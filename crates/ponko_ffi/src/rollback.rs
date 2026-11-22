//! Rollback for a whole game from C (Rust `net::GameRollback`), with each
//! player's input as up to [`PONKO_ROLLBACK_INPUT_MAX`] bytes, and world
//! snapshots (Rust `WorldSnapshot`).
//!
//! A session runs inside the game loop as in Rust: started in setup or a
//! frame system, it runs every simulation tick through the session, and when
//! a late input differs from the prediction the engine loads the world, the
//! physics, the events and the tick saved before it and runs the simulation
//! systems again. Frame systems set this player's input and carry the
//! packets; simulation systems read every player's input for the tick.

use std::ffi::c_void;

use ponko::ecs_core::{SnapshotError, WorldSnapshot};
use ponko::net::GameRollback;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ctx::{PonkoCtx, ctx_access, out};
use crate::{Failure, PonkoStatus, PonkoWorld, access, boundary, error, invalid, invalid_owned};

/// The most bytes one player's input for one tick has in a C rollback
/// session (`ponko_ctx_rollback_set_input`).
pub const PONKO_ROLLBACK_INPUT_MAX: usize = 32;

/// One player's input for one tick: the bytes C gave, zero padded. On the
/// wire it is the bytes up to the last non-zero one, in hex.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub(crate) struct RollbackInput([u8; PONKO_ROLLBACK_INPUT_MAX]);

const HEX: &[u8; 16] = b"0123456789abcdef";

impl Serialize for RollbackInput {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let used = self.0.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
        let mut text = [0u8; 2 * PONKO_ROLLBACK_INPUT_MAX];
        for (i, b) in self.0[..used].iter().enumerate() {
            text[2 * i] = HEX[usize::from(b >> 4)];
            text[2 * i + 1] = HEX[usize::from(b & 15)];
        }
        // Hex digits are ASCII.
        s.serialize_str(std::str::from_utf8(&text[..2 * used]).unwrap_or_default())
    }
}

impl<'de> Deserialize<'de> for RollbackInput {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Hex;
        impl Visitor<'_> for Hex {
            type Value = RollbackInput;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "at most {PONKO_ROLLBACK_INPUT_MAX} bytes in hex")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<RollbackInput, E> {
                let digits = v.as_bytes();
                if !digits.len().is_multiple_of(2) || digits.len() > 2 * PONKO_ROLLBACK_INPUT_MAX {
                    return Err(E::invalid_length(digits.len(), &self));
                }
                let digit = |c: u8| {
                    (c as char)
                        .to_digit(16)
                        .map(|d| d as u8)
                        .ok_or_else(|| E::invalid_value(de::Unexpected::Str(v), &self))
                };
                let mut input = RollbackInput::default();
                for (i, pair) in digits.as_chunks::<2>().0.iter().enumerate() {
                    input.0[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
                }
                Ok(input)
            }
        }
        d.deserialize_str(Hex)
    }
}

type Session = GameRollback<RollbackInput>;

/// How a C rollback session runs (`ponko_ctx_rollback_start`). Start from
/// `ponko_rollback_config_default`.
/// ```
/// let c = ponko_ffi::ponko_rollback_config_default(2, 1);
/// assert_eq!((c.players, c.player, c.input_delay, c.max_prediction, c.checksum_every), (2, 1, 2, 8, 60));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoRollbackConfig {
    /// Players in the session, 2 to 8.
    pub players: u32,
    /// Which of them this peer is.
    pub player: u32,
    /// Input given at tick `t` is used at tick `t + input_delay` everywhere
    /// (at most 16).
    pub input_delay: u32,
    /// The deepest rollback, and how far the game runs ahead of the last
    /// tick every input is in (1 to 32).
    pub max_prediction: u32,
    /// Compares world hashes with the other peers every this many ticks
    /// (0 never).
    pub checksum_every: u32,
}

/// The defaults for player `player` of `players`: 2 ticks of input delay,
/// rollbacks of up to 8 ticks, world hashes compared every 60 ticks.
#[unsafe(no_mangle)]
pub extern "C" fn ponko_rollback_config_default(players: u32, player: u32) -> PonkoRollbackConfig {
    PonkoRollbackConfig {
        players,
        player,
        input_delay: 2,
        max_prediction: 8,
        checksum_every: 60,
    }
}

/// What a rollback session did so far (Rust `RollbackStats`).
/// ```
/// let s = ponko_ffi::PonkoRollbackStats::default();
/// assert_eq!(s.rollbacks, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoRollbackStats {
    /// Ticks that ran (not counting the ones simulated again).
    pub ticks: u64,
    /// Ticks that ran with at least one predicted input.
    pub predicted: u64,
    /// Rollbacks: real inputs that differed from the prediction.
    pub rollbacks: u64,
    /// Ticks simulated again by rollbacks, in total.
    pub resimulated: u64,
    /// The most ticks one rollback simulated again.
    pub deepest: u32,
    /// Ticks that waited for a peer too far behind.
    pub stalls: u64,
    /// Packets built.
    pub packets_sent: u64,
    /// Bytes in those packets.
    pub bytes_sent: u64,
    /// Packets ignored: not a rollback packet, other settings, malformed.
    pub rejected: u64,
}

/// Runs `f` on the game's C rollback session; `PONKO_NOT_FOUND` if none runs.
///
/// # Safety
/// `ctx` null or live.
unsafe fn session<R>(ctx: *mut PonkoCtx, f: impl FnOnce(&mut Session) -> Result<R, Failure>) -> Result<R, Failure> {
    // SAFETY: forwarded caller contract.
    unsafe {
        ctx_access(ctx, |c| match c.net.rollback::<RollbackInput>() {
            Some(s) => f(s),
            None => Err(error(PonkoStatus::PONKO_NOT_FOUND, "no rollback session runs")),
        })
    }
}

/// Starts a rollback session for the game (Rust `ctx.net.start_rollback`),
/// in setup or a frame system: from the next simulation tick on, every
/// tick runs through it. Replaces a session that ran. Every component of
/// the world must be copyable: C components without a destructor are.
/// # Safety
/// Live ctx; `config` readable.
/// ```
/// let c = ponko_ffi::ponko_rollback_config_default(2, 0);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_start(std::ptr::null_mut(), &c) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_start(
    ctx: *mut PonkoCtx,
    config: *const PonkoRollbackConfig,
) -> PonkoStatus {
    boundary(|| {
        if config.is_null() {
            return Err(invalid("config is null"));
        }
        // SAFETY: caller supplies a readable config.
        let c = unsafe { *config };
        if !(2..=8).contains(&c.players) || c.player >= c.players {
            return Err(invalid_owned(format!(
                "rollback: {} players with this peer player {}; 2 to 8 players, the peer below that",
                c.players, c.player
            )));
        }
        if c.input_delay > 16 || !(1..=32).contains(&c.max_prediction) {
            return Err(invalid("rollback: input_delay is at most 16, max_prediction 1 to 32"));
        }
        let session = Session::new(c.players as usize, c.player as usize)
            .input_delay(c.input_delay)
            .max_prediction(c.max_prediction)
            .checksum_every(c.checksum_every);
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |cx| {
                cx.net.start_rollback(session);
                Ok(())
            })
        }
    })
}

/// Stops the game's rollback session (Rust `ctx.net.stop_rollback`): ticks
/// run as before. `PONKO_NOT_FOUND` if none ran.
/// # Safety
/// Live ctx.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_stop(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_stop(ctx: *mut PonkoCtx) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| match c.net.stop_rollback::<RollbackInput>() {
                Some(_) => Ok(()),
                None => Err(error(PonkoStatus::PONKO_NOT_FOUND, "no rollback session runs")),
            })
        }
    })
}

/// This player's input for the next tick that runs: `size` bytes (at most
/// `PONKO_ROLLBACK_INPUT_MAX`) copied from `data`, zero padded. It stays
/// until set again.
/// # Safety
/// Live ctx; `data` readable for `size` bytes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_set_input(std::ptr::null_mut(), std::ptr::null(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_set_input(
    ctx: *mut PonkoCtx,
    data: *const c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        if size > PONKO_ROLLBACK_INPUT_MAX {
            return Err(invalid("rollback input is over PONKO_ROLLBACK_INPUT_MAX bytes"));
        }
        if data.is_null() && size != 0 {
            return Err(invalid("input data is null"));
        }
        let mut input = RollbackInput::default();
        if size > 0 {
            // SAFETY: caller supplies `size` readable bytes.
            input.0[..size].copy_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) });
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            session(ctx, |s| {
                s.set_input(input);
                Ok(())
            })
        }
    })
}

/// The packet to send to every other peer, every frame: `*out_size` bytes
/// at `*out_data`.
///
/// Ownership: `out_data` receives a view of bytes the session owns, valid
/// until the next `ponko_ctx_rollback_*` call or the end of this system;
/// never free it.
///
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_packet(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_packet(
    ctx: *mut PonkoCtx,
    out_data: *mut *const u8,
    out_size: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_data.is_null() || out_size.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract; the packet lives in the session
        // in ctx.net until it builds the next one.
        unsafe {
            let (ptr, len) = session(ctx, |s| {
                let p = s.packet();
                Ok((p.as_ptr(), p.len()))
            })?;
            out(out_data, ptr)?;
            out(out_size, len)
        }
    })
}

/// Takes a packet from another peer (any transport, any order, lost ones
/// don't matter). `*out_accepted` is false for a packet that isn't one of
/// this session's (another session's settings, malformed). A rollback it
/// calls for runs at the next tick.
/// # Safety
/// Live ctx; `data` readable for `size` bytes; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_receive(std::ptr::null_mut(), std::ptr::null(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_receive(
    ctx: *mut PonkoCtx,
    data: *const c_void,
    size: usize,
    out_accepted: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        if out_accepted.is_null() {
            return Err(invalid("output is null"));
        }
        if data.is_null() && size != 0 {
            return Err(invalid("packet data is null"));
        }
        let bytes = if size == 0 {
            &[][..]
        } else {
            // SAFETY: caller supplies `size` readable bytes.
            unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }
        };
        // SAFETY: forwarded caller contract.
        unsafe {
            let accepted = session(ctx, |s| Ok(s.receive(bytes)))?;
            out(out_accepted, accepted)
        }
    })
}

/// In a simulation system: `player`'s input for this tick, its first `size`
/// bytes (at most `PONKO_ROLLBACK_INPUT_MAX`) copied to `out`; zeros before
/// a player's first input. `PONKO_NOT_FOUND` outside a tick a rollback
/// session runs.
/// # Safety
/// Live ctx; `out` writable for `size` bytes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_input(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_input(
    ctx: *mut PonkoCtx,
    player: u32,
    out_input: *mut c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        if size > PONKO_ROLLBACK_INPUT_MAX {
            return Err(invalid("rollback input is over PONKO_ROLLBACK_INPUT_MAX bytes"));
        }
        if out_input.is_null() && size != 0 {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        let input = unsafe {
            ctx_access(ctx, |c| {
                let inputs = c
                    .net
                    .rollback_inputs::<RollbackInput>()
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no rollback tick runs"))?;
                inputs
                    .get(player as usize)
                    .copied()
                    .ok_or_else(|| invalid("no such player in the rollback session"))
            })?
        };
        if size > 0 {
            // SAFETY: caller supplies `size` writable bytes.
            unsafe { std::ptr::copy_nonoverlapping(input.0.as_ptr(), out_input.cast::<u8>(), size) };
        }
        Ok(())
    })
}

/// Whether this tick runs again after a rollback (Rust
/// `ctx.net.is_resimulating`): sounds played now are dropped, and one-shot
/// effects outside the world should be skipped too.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_resimulating(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_resimulating(ctx: *mut PonkoCtx, out_again: *mut bool) -> PonkoStatus {
    boundary(|| {
        if out_again.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let again = ctx_access(ctx, |c| Ok(c.net.is_resimulating()))?;
            out(out_again, again)
        }
    })
}

/// The session's frame (the next tick it runs, counted from its start) and
/// its confirmed frame: the last one every player's input is in for
/// (`*out_has_confirmed` false before the first). `PONKO_NOT_FOUND` if no
/// session runs.
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_frames(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_frames(
    ctx: *mut PonkoCtx,
    out_frame: *mut u64,
    out_has_confirmed: *mut bool,
    out_confirmed: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if out_frame.is_null() || out_has_confirmed.is_null() || out_confirmed.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let (frame, confirmed) = session(ctx, |s| Ok((s.frame(), s.confirmed_frame())))?;
            out(out_frame, frame)?;
            out(out_has_confirmed, confirmed.is_some())?;
            out(out_confirmed, confirmed.unwrap_or(0))
        }
    })
}

/// What the session did so far. `PONKO_NOT_FOUND` if none runs.
/// # Safety
/// Live ctx; writable output.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_stats(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_stats(
    ctx: *mut PonkoCtx,
    out_stats: *mut PonkoRollbackStats,
) -> PonkoStatus {
    boundary(|| {
        if out_stats.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let s = session(ctx, |s| Ok(s.stats()))?;
            out(
                out_stats,
                PonkoRollbackStats {
                    ticks: s.ticks,
                    predicted: s.predicted,
                    rollbacks: s.rollbacks,
                    resimulated: s.resimulated,
                    deepest: s.deepest,
                    stalls: s.stalls,
                    packets_sent: s.packets_sent,
                    bytes_sent: s.bytes_sent,
                    rejected: s.rejected,
                },
            )
        }
    })
}

/// Whether two peers' worlds stopped matching; if so, the first checked
/// frame that differed and the player whose world differs.
/// `PONKO_NOT_FOUND` if no session runs.
/// # Safety
/// Live ctx; writable outputs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_rollback_desync(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_rollback_desync(
    ctx: *mut PonkoCtx,
    out_found: *mut bool,
    out_frame: *mut u64,
    out_player: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        if out_found.is_null() || out_frame.is_null() || out_player.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let d = session(ctx, |s| Ok(s.desync()))?;
            out(out_found, d.is_some())?;
            out(out_frame, d.map_or(0, |d| d.frame))?;
            out(out_player, d.map_or(0, |d| d.player as u32))
        }
    })
}

// ---- world snapshots ---------------------------------------------------------

/// A copy of a world in memory (Rust `WorldSnapshot`): every entity and
/// component, the entity allocator and the change history. Made with
/// `ponko_world_snapshot_new`, freed with `ponko_world_snapshot_free`.
pub struct PonkoWorldSnapshot {
    snapshot: WorldSnapshot,
}

/// An empty snapshot, to save worlds into again and again (it reuses its
/// memory).
///
/// Ownership: the caller owns the returned snapshot and frees it with
/// `ponko_world_snapshot_free`.
///
/// ```
/// let s = ponko_ffi::ponko_world_snapshot_new();
/// unsafe { ponko_ffi::ponko_world_snapshot_free(s) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_world_snapshot_new() -> *mut PonkoWorldSnapshot {
    Box::into_raw(Box::new(PonkoWorldSnapshot {
        snapshot: WorldSnapshot::new(),
    }))
}

/// Frees a snapshot. Null is ignored.
/// # Safety
/// `snapshot` null or from `ponko_world_snapshot_new`, not used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_snapshot_free(snapshot: *mut PonkoWorldSnapshot) {
    if !snapshot.is_null() {
        // SAFETY: caller contract: from `Box::into_raw`, freed once.
        drop(unsafe { Box::from_raw(snapshot) });
    }
}

fn snapshot_failure(e: SnapshotError) -> Failure {
    let status = match e {
        SnapshotError::OtherWorld => PonkoStatus::PONKO_INVALID_ARGUMENT,
        _ => PonkoStatus::PONKO_UNSUPPORTED,
    };
    Failure(status, e.to_string().into(), crate::kind_of_status(status), None)
}

/// Copies `world` into `snapshot` (Rust `World::save_snapshot`), replacing
/// what it held. `PONKO_UNSUPPORTED` (and nothing saved) if a component
/// can't be copied: a Rust type that isn't `Clone`, or a C component with
/// a destructor.
/// # Safety
/// Live world and snapshot.
/// ```
/// use ponko_ffi::*;
/// let (w, s) = (ponko_world_new(), ponko_world_snapshot_new());
/// unsafe {
///     assert_eq!(ponko_world_save_snapshot(w, s), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_world_restore_snapshot(w, s), PonkoStatus::PONKO_OK);
///     ponko_world_snapshot_free(s);
///     ponko_world_free(w);
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_save_snapshot(
    world: *mut PonkoWorld,
    snapshot: *mut PonkoWorldSnapshot,
) -> PonkoStatus {
    boundary(|| {
        if snapshot.is_null() {
            return Err(invalid("snapshot is null"));
        }
        // SAFETY: caller supplies a live snapshot, not aliased during the call.
        let snapshot = unsafe { &mut (*snapshot).snapshot };
        // SAFETY: forwarded caller contract.
        unsafe { access(world, |w, _| w.save_snapshot(snapshot).map_err(snapshot_failure)) }
    })
}

/// Makes `world` what it was when `snapshot` was saved from it (Rust
/// `World::restore_snapshot`): the same entities with the same ids, the
/// same ids for the next spawns, change stamps as old as they were then.
/// `PONKO_INVALID_ARGUMENT` for a snapshot of another world.
/// # Safety
/// Live world and snapshot.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_restore_snapshot(
    world: *mut PonkoWorld,
    snapshot: *const PonkoWorldSnapshot,
) -> PonkoStatus {
    boundary(|| {
        if snapshot.is_null() {
            return Err(invalid("snapshot is null"));
        }
        // SAFETY: caller supplies a live snapshot.
        let snapshot = unsafe { &(*snapshot).snapshot };
        // SAFETY: forwarded caller contract.
        unsafe { access(world, |w, _| w.restore_snapshot(snapshot).map_err(snapshot_failure)) }
    })
}

/// Entities in the snapshot.
/// # Safety
/// Live snapshot; writable output.
/// ```
/// use ponko_ffi::*;
/// let s = ponko_world_snapshot_new();
/// let mut n = 9;
/// unsafe {
///     assert_eq!(ponko_world_snapshot_len(s, &mut n), PonkoStatus::PONKO_OK);
///     ponko_world_snapshot_free(s);
/// }
/// assert_eq!(n, 0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_snapshot_len(
    snapshot: *const PonkoWorldSnapshot,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if snapshot.is_null() {
            return Err(invalid("snapshot is null"));
        }
        // SAFETY: caller supplies a live snapshot and writable output.
        unsafe { out(out_len, (*snapshot).snapshot.len()) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_travel_as_short_hex() {
        let mut input = RollbackInput::default();
        input.0[0] = 0xab;
        input.0[2] = 0x01;
        let json = serde_json::to_string(&input).unwrap();
        assert_eq!(json, "\"ab0001\"");
        assert_eq!(serde_json::from_str::<RollbackInput>(&json).unwrap(), input);
        assert_eq!(serde_json::to_string(&RollbackInput::default()).unwrap(), "\"\"");
        assert!(serde_json::from_str::<RollbackInput>("\"abc\"").is_err());
        assert!(serde_json::from_str::<RollbackInput>("\"zz\"").is_err());
        let all = RollbackInput([0xff; PONKO_ROLLBACK_INPUT_MAX]);
        let json = serde_json::to_string(&all).unwrap();
        assert_eq!(serde_json::from_str::<RollbackInput>(&json).unwrap(), all);
        assert!(serde_json::from_str::<RollbackInput>(&format!("\"{}00\"", &json[1..json.len() - 1])).is_err());
    }
}
