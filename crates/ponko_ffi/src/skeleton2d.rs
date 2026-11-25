//! 2D skeletons from C (Rust `ponko::skeleton2d`): load a skeleton file
//! (Ponko's format or a Spine export with its atlas), put a skeleton on an
//! entity, and drive it: clips and crossfades, the state machine's states
//! and parameters, IK targets, events, skins and attachments. The skeleton
//! lives on its entity; every call names the entity. Names (bones, slots,
//! IK constraints, states, parameters) become indices once with the
//! `*_index` lookups, so per-frame calls take plain numbers.

use std::ffi::c_char;

use ponko::anim::{AnimationGraphError, ParamId, StateId};
use ponko::prelude::{Ctx, Entity, Vec2};
use ponko::skeleton2d::{ClipId, Skeleton};

use crate::ctx::{ctx_access, out, text};
use crate::types::{PonkoColor, PonkoVec2};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, error, invalid, invalid_owned, write_text};

fn skeleton(c: &mut Ctx, bits: u64) -> Result<&mut Skeleton, Failure> {
    let e = Entity::from_bits(bits);
    if !c.world.is_alive(e) {
        return Err(invalid("no such entity"));
    }
    c.world
        .get_mut::<Skeleton>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no 2D skeleton"))
}

fn graph_error(e: AnimationGraphError) -> Failure {
    invalid_owned(e.to_string())
}

fn not_found(what: &'static str) -> Failure {
    error(PonkoStatus::PONKO_NOT_FOUND, what)
}

fn finite(v: f32, what: &'static str) -> Result<f32, Failure> {
    if v.is_finite() { Ok(v) } else { Err(invalid(what)) }
}

/// Writes `index` of what `find` finds by `name` on `entity`'s skeleton.
unsafe fn lookup(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_index: *mut usize,
    missing: &'static str,
    find: impl FnOnce(&Skeleton, &str) -> Option<usize>,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_index.is_null() {
                return Err(invalid("output is null"));
            }
            let s = skeleton(c, entity_bits)?;
            let index = find(s, name).ok_or_else(|| not_found(missing))?;
            out(out_index, index)
        })
    })
}

/// [`lookup`] for names a typo of which should say what there is: the
/// error of `find` (`PONKO_NOT_FOUND`, its message listing the names).
unsafe fn resolve(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_index: *mut usize,
    find: impl FnOnce(&Skeleton, &str) -> Result<usize, ponko::prelude::Error>,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_index.is_null() {
                return Err(invalid("output is null"));
            }
            let s = skeleton(c, entity_bits)?;
            let index = find(s, name).map_err(engine_error)?;
            out(out_index, index)
        })
    })
}

// ---- loading and attaching ----------------------------------------------------------

/// Loads skeleton file `path` from the assets (Ponko's format, or a Spine
/// JSON export with the `.atlas` beside it) and its images, once: later
/// loads and `ponko_ctx_set_skeleton2d` reuse it. Writes how many Spine
/// features the file uses that aren't read to `out_warnings` (may be null);
/// read them with `ponko_ctx_skeleton2d_warning`. They are also printed.
/// `PONKO_NOT_FOUND` for a missing file or image.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_skeleton2d(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string; `out_warnings` null
/// or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_skeleton2d(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    out_warnings: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            let data = c.try_load_skeleton2d(path).map_err(engine_error)?;
            if !out_warnings.is_null() {
                out_warnings.write(data.warnings().len());
            }
            Ok(())
        })
    })
}

/// Copies warning `index` of loaded skeleton file `path` (see
/// `ponko_ctx_load_skeleton2d`) into `buffer` as NUL-terminated text, and its
/// length (without the NUL) to `out_len`. A buffer too small gets an empty
/// string: call with the length + 1. `PONKO_NOT_FOUND` past the last one.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_warning(std::ptr::null_mut(), std::ptr::null(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string; `buffer` writable for
/// `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_warning(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    index: usize,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            let data = c.try_load_skeleton2d(path).map_err(engine_error)?;
            let warning = data.warnings().get(index).ok_or_else(|| not_found("no such warning"))?;
            write_text(buffer, capacity, out_len, warning)
        })
    })
}

/// Puts a skeleton playing skeleton file `path` (loaded once, as
/// `ponko_ctx_load_skeleton2d`) on `entity`, replacing one it had. It starts
/// at rest, in its state machine's first state if the file has one. Give the
/// entity a `PonkoTransform2D` to place it; the engine advances and draws it.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_skeleton2d(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_skeleton2d(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    path: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            let e = Entity::from_bits(entity_bits);
            if !c.world.is_alive(e) {
                return Err(invalid("no such entity"));
            }
            let data = c.try_load_skeleton2d(path).map_err(engine_error)?;
            c.world.insert(e, Skeleton::new(data));
            Ok(())
        })
    })
}

/// Takes `entity`'s skeleton off (nothing happens if it has none).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_skeleton2d(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_skeleton2d(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = Entity::from_bits(entity_bits);
            if !c.world.is_alive(e) {
                return Err(invalid("no such entity"));
            }
            c.world.remove::<Skeleton>(e);
            Ok(())
        })
    })
}

// ---- playing --------------------------------------------------------------------------

/// Plays clip `clip` on `entity`'s skeleton from its start, cutting off what
/// played: looping, or once (then holding its last pose). Takes effect at
/// its next update. `PONKO_NOT_FOUND` if there is no such clip
/// (`ponko_last_error` lists the clips). A system that plays clips every
/// frame resolves them once with `ponko_ctx_skeleton2d_clip_index` and uses
/// `ponko_ctx_skeleton2d_play_index`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_play(std::ptr::null_mut(), 0, std::ptr::null(), true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `clip` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_play(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    clip: *const c_char,
    looping: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let clip = text(clip)?;
            let s = skeleton(c, entity_bits)?;
            let clip = s.clip_id(clip).map_err(engine_error)?;
            if looping {
                s.play(clip)
            } else {
                s.play_once(clip)
            };
            Ok(())
        })
    })
}

/// Fades `entity`'s skeleton from what plays now to clip `clip` (looping)
/// over `duration` seconds. `PONKO_NOT_FOUND` if there is no such clip
/// (`ponko_last_error` lists the clips).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_crossfade(std::ptr::null_mut(), 0, std::ptr::null(), 0.2) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `clip` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_crossfade(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    clip: *const c_char,
    duration: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let clip = text(clip)?;
            if !(duration.is_finite() && duration >= 0.0) {
                return Err(invalid("crossfade duration must be finite and not negative"));
            }
            let s = skeleton(c, entity_bits)?;
            let clip = s.clip_id(clip).map_err(engine_error)?;
            s.crossfade(clip, duration);
            Ok(())
        })
    })
}

/// The index of the clip named `name` of `entity`'s skeleton (Rust
/// `Skeleton::clip_id`), for `ponko_ctx_skeleton2d_play_index` and
/// `ponko_ctx_skeleton2d_crossfade_index`, which then skip the name lookup:
/// resolve it once and keep it. `PONKO_NOT_FOUND` if there is none;
/// `ponko_last_error` then lists the clips.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_clip_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_clip` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_clip_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_clip: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { resolve(ctx, entity_bits, name, out_clip, |s, name| s.clip_id(name).map(|c| c.0)) }
}

/// `ponko_ctx_skeleton2d_play` by a clip index from
/// `ponko_ctx_skeleton2d_clip_index`. `PONKO_NOT_FOUND` if there is no such
/// clip.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_play_index(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_play_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    clip: usize,
    looping: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            let clip = ClipId(clip);
            let found = if looping { s.play(clip) } else { s.play_once(clip) };
            if found {
                Ok(())
            } else {
                Err(not_found("no clip with that index"))
            }
        })
    })
}

/// `ponko_ctx_skeleton2d_crossfade` by a clip index from
/// `ponko_ctx_skeleton2d_clip_index`. `PONKO_NOT_FOUND` if there is no such
/// clip.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_crossfade_index(std::ptr::null_mut(), 0, 0, 0.2) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_crossfade_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    clip: usize,
    duration: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if !(duration.is_finite() && duration >= 0.0) {
                return Err(invalid("crossfade duration must be finite and not negative"));
            }
            let s = skeleton(c, entity_bits)?;
            if s.crossfade(ClipId(clip), duration) {
                Ok(())
            } else {
                Err(not_found("no clip with that index"))
            }
        })
    })
}

/// Switches the state machine of `entity`'s skeleton to state `state`
/// (from `ponko_ctx_skeleton2d_state_index`), starting at normalized time
/// `offset`, fading over `duration` seconds (0 jumps). Fails, changing
/// nothing, for a state not in the graph (or without one) or a negative or
/// non-finite duration or offset.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_crossfade_state(std::ptr::null_mut(), 0, 0, 0.2, 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_crossfade_state(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    state: usize,
    duration: f32,
    offset: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton(c, entity_bits)?
                .try_crossfade_state(StateId(state), duration, offset)
                .map_err(graph_error)
        })
    })
}

/// What `entity`'s skeleton plays: its state machine's state (`SIZE_MAX`
/// while a clip played with `ponko_ctx_skeleton2d_play` is on, or nothing
/// plays), how far it is (normalized: 1 is one full play) and whether
/// something played once reached its end. Any output may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_state(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_state(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_state: *mut usize,
    out_normalized_time: *mut f32,
    out_finished: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            if !out_state.is_null() {
                out_state.write(s.state().map_or(usize::MAX, |s| s.0));
            }
            if !out_normalized_time.is_null() {
                out_normalized_time.write(s.normalized_time());
            }
            if !out_finished.is_null() {
                out_finished.write(s.finished());
            }
            Ok(())
        })
    })
}

/// Unwrapped normalized time of the 2D skeleton, in double precision (1 is one play).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_normalized_time_f64(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_time` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_normalized_time_f64(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_time: *mut f64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_time.is_null() {
                return Err(invalid("output is null"));
            }
            out_time.write(skeleton(c, entity_bits)?.normalized_time_f64());
            Ok(())
        })
    })
}

/// The index of the state machine's state named `name`, for
/// `ponko_ctx_skeleton2d_crossfade_state`. `PONKO_NOT_FOUND` if there is
/// none (or no state machine); `ponko_last_error` then lists the states.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_state_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_state` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_state_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_state: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        resolve(ctx, entity_bits, name, out_state, |s, name| match s.data().graph() {
            Some(graph) => graph.try_state(name).map(|s| s.0),
            None => Err(ponko::prelude::Error::new(
                ponko::prelude::ErrorKind::NotFound,
                format!("unknown animation state {name:?} (the skeleton has no state machine)"),
            )),
        })
    }
}

// ---- parameters -----------------------------------------------------------------------

/// The index of the state machine's parameter named `name`, for the
/// `ponko_ctx_skeleton2d_set_*` calls. `PONKO_NOT_FOUND` if there is none;
/// `ponko_last_error` then lists the parameters.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_parameter_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_parameter` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_parameter_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_parameter: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        resolve(ctx, entity_bits, name, out_parameter, |s, name| {
            s.try_parameter_id(name).map(|p| p.0)
        })
    }
}

/// Sets float parameter `parameter` of `entity`'s skeleton. Fails, changing
/// nothing, for one that isn't a float parameter or a NaN or infinite value.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_float(std::ptr::null_mut(), 0, 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_float(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton(c, entity_bits)?
                .try_set_float(ParamId(parameter), value)
                .map_err(graph_error)
        })
    })
}

/// Sets bool parameter `parameter` of `entity`'s skeleton. Fails, changing
/// nothing, for one that isn't a bool parameter.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_bool(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_bool(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton(c, entity_bits)?
                .try_set_bool(ParamId(parameter), value)
                .map_err(graph_error)
        })
    })
}

/// Sets (or clears) trigger `parameter` of `entity`'s skeleton; the
/// transition it fires clears it. Fails, changing nothing, for one that
/// isn't a trigger.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_trigger(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_trigger(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton(c, entity_bits)?
                .try_set_trigger(ParamId(parameter), value)
                .map_err(graph_error)
        })
    })
}

// ---- events ---------------------------------------------------------------------------

/// Takes the oldest event `entity`'s skeleton fired, copying its name
/// (NUL-terminated, cut to fit) into `buffer` of `capacity` bytes.
/// `*out_found` is false when there are none left; call it until then.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_next_event(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_next_event(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_found: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_found.is_null() {
                return Err(invalid("output is null"));
            }
            if buffer.is_null() && capacity > 0 {
                return Err(invalid("buffer is null"));
            }
            let Some(name) = skeleton(c, entity_bits)?.next_event() else {
                return out(out_found, false);
            };
            if capacity > 0 {
                let n = name.len().min(capacity - 1);
                std::ptr::copy_nonoverlapping(name.as_ptr(), buffer.cast::<u8>(), n);
                buffer.add(n).write(0);
            }
            out(out_found, true)
        })
    })
}

// ---- IK ---------------------------------------------------------------------------------

/// The index of the IK constraint named `name`. `PONKO_NOT_FOUND` if there
/// is none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_ik_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_ik` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_ik_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_ik: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        lookup(
            ctx,
            entity_bits,
            name,
            out_ik,
            "no IK constraint by that name",
            |s, name| s.data().ik_constraint(name),
        )
    }
}

/// Points IK constraint `ik` of `entity`'s skeleton at `target`, in the
/// skeleton's own space (before the entity's transform), instead of its
/// target bone, until `ponko_ctx_skeleton2d_clear_ik_target`.
/// ```
/// let to = ponko_ffi::PonkoVec2 { x: 1.0, y: 2.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_ik_target(std::ptr::null_mut(), 0, 0, to) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_ik_target(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    ik: usize,
    target: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let target = Vec2::new(
                finite(target.x, "target must be finite")?,
                finite(target.y, "target must be finite")?,
            );
            let s = skeleton(c, entity_bits)?;
            if ik >= s.data().ik_constraints().len() {
                return Err(invalid("no such IK constraint"));
            }
            s.set_ik_target(ik, Some(target));
            Ok(())
        })
    })
}

/// Hands IK constraint `ik` of `entity`'s skeleton back to its target bone.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_clear_ik_target(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_clear_ik_target(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    ik: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            if ik >= s.data().ik_constraints().len() {
                return Err(invalid("no such IK constraint"));
            }
            s.set_ik_target(ik, None);
            Ok(())
        })
    })
}

/// How much IK constraint `ik` of `entity`'s skeleton bends its bones: 0
/// not at all, 1 all the way to the target.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_ik_mix(std::ptr::null_mut(), 0, 0, 0.5) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_ik_mix(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    ik: usize,
    mix: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let mix = finite(mix, "mix must be finite")?;
            let s = skeleton(c, entity_bits)?;
            if ik >= s.data().ik_constraints().len() {
                return Err(invalid("no such IK constraint"));
            }
            s.set_ik_mix(ik, mix);
            Ok(())
        })
    })
}

/// Turns bone `bone` of `entity`'s skeleton so its +x axis points at
/// `target` (the skeleton's own space), `mix` of the way (0 to 1), and
/// moves the bones below it (Rust `Skeleton::look_at`). A one-off: call it
/// in a late system, after the update; the next update poses the bone from
/// its clips again.
/// ```
/// let to = ponko_ffi::PonkoVec2 { x: 1.0, y: 2.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_look_at(std::ptr::null_mut(), 0, 0, to, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_look_at(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    bone: usize,
    target: PonkoVec2,
    mix: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let target = Vec2::new(
                finite(target.x, "target must be finite")?,
                finite(target.y, "target must be finite")?,
            );
            let mix = finite(mix, "mix must be finite")?;
            let s = skeleton(c, entity_bits)?;
            if bone >= s.data().bones().len() {
                return Err(invalid("no such bone"));
            }
            s.look_at(bone, target, mix);
            Ok(())
        })
    })
}

/// Bends bone `upper` of `entity`'s skeleton and its child `lower` so the
/// tip of `lower` reaches `target` (the skeleton's own space), or as close
/// as their lengths allow, `mix` of the way (Rust `Skeleton::two_bone_ik`).
/// `bend_positive`: the joint turns clockwise on screen; false the other
/// way. A one-off, as `ponko_ctx_skeleton2d_look_at`. `PONKO_INVALID_ARGUMENT`
/// if `lower`'s parent isn't `upper`.
/// ```
/// let to = ponko_ffi::PonkoVec2 { x: 1.0, y: 2.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_two_bone_ik(std::ptr::null_mut(), 0, 0, 1, to, true, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_two_bone_ik(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    upper: usize,
    lower: usize,
    target: PonkoVec2,
    bend_positive: bool,
    mix: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let target = Vec2::new(
                finite(target.x, "target must be finite")?,
                finite(target.y, "target must be finite")?,
            );
            let mix = finite(mix, "mix must be finite")?;
            let s = skeleton(c, entity_bits)?;
            let bones = s.data().bones();
            if upper >= bones.len() || lower >= bones.len() {
                return Err(invalid("no such bone"));
            }
            // `Skeleton::two_bone_ik` panics on purpose for such a chain.
            if bones[lower].parent != Some(upper) {
                return Err(invalid("the lower bone must be a child of the upper one"));
            }
            s.two_bone_ik(upper, lower, target, bend_positive, mix);
            Ok(())
        })
    })
}

// ---- bones, slots and skins -------------------------------------------------------------

/// The index of the bone named `name`. `PONKO_NOT_FOUND` if there is none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_bone_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_bone` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_bone_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_bone: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        lookup(ctx, entity_bits, name, out_bone, "no bone by that name", |s, name| {
            s.bone(name)
        })
    }
}

/// Where bone `bone` of `entity`'s skeleton is and which way it points
/// (radians, clockwise on screen), in the skeleton's own space as last
/// posed. Either output may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_bone(std::ptr::null_mut(), 0, 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_bone(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    bone: usize,
    out_position: *mut PonkoVec2,
    out_rotation: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            if bone >= s.data().bones().len() {
                return Err(invalid("no such bone"));
            }
            if !out_position.is_null() {
                let p = s.bone_position(bone);
                out_position.write(PonkoVec2 { x: p.x, y: p.y });
            }
            if !out_rotation.is_null() {
                out_rotation.write(s.bone_rotation(bone));
            }
            Ok(())
        })
    })
}

/// The index of the slot named `name`. `PONKO_NOT_FOUND` if there is none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_slot_index(std::ptr::null_mut(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `out_slot` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_slot_index(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_slot: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        lookup(ctx, entity_bits, name, out_slot, "no slot by that name", |s, name| {
            s.data().slot(name)
        })
    }
}

/// Makes slot `slot` of `entity`'s skeleton show attachment `name` (null:
/// nothing) whatever its clips key, until
/// `ponko_ctx_skeleton2d_reset_attachment`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_attachment(std::ptr::null_mut(), 0, 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_attachment(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    slot: usize,
    name: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = if name.is_null() { None } else { Some(text(name)?) };
            let s = skeleton(c, entity_bits)?;
            if slot >= s.data().slots().len() {
                return Err(invalid("no such slot"));
            }
            s.set_attachment(slot, Some(name));
            Ok(())
        })
    })
}

/// Copies the name of the attachment slot `slot` of `entity`'s skeleton
/// shows into `buffer` as NUL-terminated text (empty when it shows
/// nothing), and its length to `out_len`. A buffer too small gets an empty
/// string: call with the length + 1.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_attachment(std::ptr::null_mut(), 0, 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_attachment(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    slot: usize,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            if slot >= s.data().slots().len() {
                return Err(invalid("no such slot"));
            }
            write_text(buffer, capacity, out_len, s.attachment(slot).unwrap_or(""))
        })
    })
}

/// Lets slot `slot` of `entity`'s skeleton show what its clips and rest
/// pose say again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_reset_attachment(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_reset_attachment(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    slot: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            if slot >= s.data().slots().len() {
                return Err(invalid("no such slot"));
            }
            s.set_attachment(slot, None);
            Ok(())
        })
    })
}

/// Shows skin `name` on `entity`'s skeleton (its attachments replace the
/// default skin's of the same names). `PONKO_NOT_FOUND` if there is none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_skin(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_skin(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if skeleton(c, entity_bits)?.set_skin(name) {
                Ok(())
            } else {
                Err(not_found("no skin by that name"))
            }
        })
    })
}

// ---- look and clock -----------------------------------------------------------------------

/// How `entity`'s skeleton draws and runs: `z` (draw order among sprites,
/// higher on top), `color` (tints every part) and `speed` (multiplies every
/// clip's speed; 1 as recorded, 0 pauses).
/// ```
/// let white = ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_look(std::ptr::null_mut(), 0, 0.0, white, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_look(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    z: f32,
    color: PonkoColor,
    speed: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let z = finite(z, "z must be finite")?;
            if !(speed.is_finite() && speed >= 0.0) {
                return Err(invalid("speed must be finite and not negative"));
            }
            let s = skeleton(c, entity_bits)?;
            s.z = z;
            s.color = ponko::prelude::Color::rgba(color.r, color.g, color.b, color.a);
            s.speed = speed;
            Ok(())
        })
    })
}

/// Turns the engine's automatic update of `entity`'s skeleton off (`true`)
/// or back on; while off, advance it with `ponko_ctx_skeleton2d_update`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_set_manual(std::ptr::null_mut(), 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_set_manual(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    manual: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton(c, entity_bits)?.manual = manual;
            Ok(())
        })
    })
}

/// Advances `entity`'s skeleton by `dt` seconds and poses it now (what the
/// engine does every frame unless it is manual).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_skeleton2d_update(std::ptr::null_mut(), 0, 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_skeleton2d_update(ctx: *mut PonkoCtx, entity_bits: u64, dt: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            // `Skeleton::update` panics on purpose for such a `dt`.
            if !(dt.is_finite() && dt >= 0.0) {
                return Err(invalid("dt must be finite and not negative"));
            }
            skeleton(c, entity_bits)?.update(dt);
            Ok(())
        })
    })
}
