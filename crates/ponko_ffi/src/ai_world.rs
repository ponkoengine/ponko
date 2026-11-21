//! The AI kit's world brains from C: trees in the world's `BehaviorTrees`,
//! `Brain` and `Blackboard` components on entities, the `ai::think` system,
//! and `ai::Signal` events (Rust `ponko::ai::think`, `Signal`,
//! `Assets::load_behavior_tree`). Keys go by id (`ponko_ai_key`).
use std::ffi::c_char;

use ponko::ai::{self, BehaviorTrees, Blackboard, Brain, Key, Signal, Value};
use ponko::prelude::{Ctx, Entity, Vec2, Vec3};
use ponko::reflect::WorldReflect;

use crate::ai::{PonkoAiStatus, PonkoBehaviorTree, action_param_num, finish_action, key_of, named_action};
use crate::ai_columns::{NO_ENTITY, NO_VEC2, NO_VEC3, board_value};
use crate::ctx::{ctx_access, events, out, text};
use crate::types::*;
use crate::{Failure, PonkoCtx, PonkoStatus, PonkoVec3, boundary, engine_error, error, invalid};

/// The `target` of a signal for every brain whose tree waits for it
/// (Rust `Signal::broadcast`).
pub const PONKO_AI_EVERY_BRAIN: u64 = u64::MAX;

fn alive(c: &Ctx, bits: u64) -> Result<Entity, Failure> {
    let e = Entity::from_bits(bits);
    if c.world.is_alive(e) {
        Ok(e)
    } else {
        Err(invalid("no such entity"))
    }
}

fn brain(c: &mut Ctx, bits: u64) -> Result<&mut Brain, Failure> {
    let e = alive(c, bits)?;
    c.world
        .get_mut::<Brain>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "the entity has no brain"))
}

fn blackboard(c: &mut Ctx, bits: u64) -> Result<&mut Blackboard, Failure> {
    let e = alive(c, bits)?;
    c.world
        .get_mut::<Blackboard>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "the entity has no blackboard"))
}

/// Loads a tree from a JSON file in `assets/` into the world, under its
/// path, with the files its `subtree` nodes name (Rust
/// `Assets::try_load_behavior_tree`); with hot reload on, saving a file
/// swaps the tree in the running game. A missing file is
/// `PONKO_NOT_FOUND`, a bad tree `PONKO_INVALID_ARGUMENT` naming the node.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_load_tree(std::ptr::null_mut(), c"ai/guard.json".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_load_tree(ctx: *mut PonkoCtx, path: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        ctx_access(ctx, |c| {
            c.assets
                .try_load_behavior_tree(&mut c.world, path)
                .map_err(engine_error)
        })
    })
}

/// Adds a compiled tree to the world under `name` (Rust
/// `BehaviorTrees::try_insert`), or replaces the one there: brains running
/// it start the new one at their next think. A tree that runs itself
/// through `subtree` nodes is `PONKO_INVALID_ARGUMENT`, naming the cycle.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_add_tree(std::ptr::null_mut(), c"idle".as_ptr(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a NUL-terminated string; `tree` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_add_tree(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    tree: *const PonkoBehaviorTree,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let name = text(name)?;
        let tree = tree.as_ref().ok_or_else(|| invalid("tree is null"))?.0.clone();
        ctx_access(ctx, |c| {
            c.world.reflect::<Brain>();
            c.world.reflect::<Blackboard>();
            if c.world.get_resource::<BehaviorTrees>().is_none() {
                c.world.insert_resource(BehaviorTrees::default());
            }
            c.world
                .resource_mut::<BehaviorTrees>()
                .try_insert(name, tree)
                .map_err(engine_error)
        })
    })
}

/// Gives `entity` a brain that runs the world's tree `tree` (a name from
/// `ponko_ctx_ai_load_tree` or `ponko_ctx_ai_add_tree`), thinking every
/// `period` ticks (0 counts as 1), and an empty blackboard if it has none
/// (Rust `Brain::new(tree).with_period(period)`, `Blackboard::new()`). A
/// brain it had is replaced.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_add_brain(std::ptr::null_mut(), 0, c"ai/guard.json".as_ptr(), 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `tree` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_add_brain(
    ctx: *mut PonkoCtx,
    entity: u64,
    tree: *const c_char,
    period: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let tree = text(tree)?;
        if tree.is_empty() {
            return Err(invalid("a tree name can't be empty"));
        }
        ctx_access(ctx, |c| {
            let e = alive(c, entity)?;
            c.world.reflect::<Brain>();
            c.world.reflect::<Blackboard>();
            c.world.insert(e, Brain::new(tree).with_period(period));
            if c.world.get::<Blackboard>(e).is_none() {
                c.world.insert(e, Blackboard::new());
            }
            Ok(())
        })
    })
}

/// Runs every due world brain on its blackboard, on all cores (the Rust
/// sim system `ai::think`): call it from a sim system, after the systems
/// that write blackboards and send signals, before the ones that act.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_think(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_think(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            ai::think(c);
            Ok(())
        })
    })
}

// ---- signals ----------------------------------------------------------------------

unsafe fn send(ctx: *mut PonkoCtx, target: u64, key: u64, value: Option<Value>) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        events(ctx, |ev| {
            let name = key_of(key)?;
            let mut signal = if target == PONKO_AI_EVERY_BRAIN {
                Signal::broadcast(name)
            } else {
                Signal::to(Entity::from_bits(target), name)
            };
            signal.value = value;
            ev.send(signal);
            Ok(())
        })
    })
}

/// Sends signal `key` to the world brain of entity `target`, or to every
/// brain with `PONKO_AI_EVERY_BRAIN` (Rust
/// `ctx.events.send(Signal::to(target, key))`): a `wait_for` node waiting
/// for it succeeds at the brain's next think, which comes at once whatever
/// its period. Send it before `ponko_ctx_ai_think` in the tick. Brains
/// whose tree doesn't wait for it, and entities without a brain, ignore it.
/// Works inside `ponko_query` callbacks.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_signal(std::ptr::null_mut(), ponko_ffi::PONKO_AI_EVERY_BRAIN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_signal(ctx: *mut PonkoCtx, target: u64, key: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { send(ctx, target, key, None) }
}

/// `ponko_ctx_ai_signal` carrying a number, which a `wait_for` node with
/// `"store": "key"` writes to the blackboard (Rust `Signal::with`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_signal_num(std::ptr::null_mut(), 0, 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_signal_num(ctx: *mut PonkoCtx, target: u64, key: u64, value: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { send(ctx, target, key, Some(Value::Num(value))) }
}

/// `ponko_ctx_ai_signal` carrying an entity (who shouted, who shot).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_signal_entity(std::ptr::null_mut(), 0, 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_signal_entity(
    ctx: *mut PonkoCtx,
    target: u64,
    key: u64,
    value: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { send(ctx, target, key, Some(Value::Entity(Entity::from_bits(value)))) }
}

/// `ponko_ctx_ai_signal` carrying a 2D point (where the noise was).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_signal_vec2(std::ptr::null_mut(), 0, 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_signal_vec2(
    ctx: *mut PonkoCtx,
    target: u64,
    key: u64,
    value: PonkoVec2,
) -> PonkoStatus {
    let v: Vec2 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { send(ctx, target, key, Some(Value::Vec2(v))) }
}

/// `ponko_ctx_ai_signal` carrying a 3D point.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_signal_vec3(std::ptr::null_mut(), 0, 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_signal_vec3(
    ctx: *mut PonkoCtx,
    target: u64,
    key: u64,
    value: PonkoVec3,
) -> PonkoStatus {
    let v: Vec3 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { send(ctx, target, key, Some(Value::Vec3(v))) }
}

// ---- acting -----------------------------------------------------------------------

/// The key id of the `index`th action the world brain of `entity` is
/// doing, in tree order, or 0 past the last (Rust `Brain::actions`; index
/// 0 is `Brain::action`). `PONKO_NOT_FOUND` if it has no brain.
/// ```
/// let mut key = 1;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_action(std::ptr::null_mut(), 0, 0, &mut key) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_key` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_action(
    ctx: *mut PonkoCtx,
    entity: u64,
    index: usize,
    out_key: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let key = ctx_access(ctx, |c| Ok(brain(c, entity)?.actions().nth(index).map_or(0, Key::id)))?;
        out(out_key, key)
    })
}

/// Whether running action `action` (a key id; 0 for the first) of the
/// world brain of `entity` started at this tick's think (Rust
/// `Brain::started_action`). `PONKO_NOT_FOUND` if it isn't doing it.
/// ```
/// let mut started = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_started(std::ptr::null_mut(), 0, 0, &mut started) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_started` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_started(
    ctx: *mut PonkoCtx,
    entity: u64,
    action: u64,
    out_started: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let started = ctx_access(ctx, |c| {
            let b = brain(c, entity)?;
            Ok(b.started_action(named_action(b, action)?))
        })?;
        out(out_started, started)
    })
}

/// Reports running action `action` (a key id; 0 for the first) of the
/// world brain of `entity` done, as `PONKO_AI_SUCCESS` or
/// `PONKO_AI_FAILURE` (Rust `Brain::succeed_action`, `fail_action`): the
/// brain goes on at the next tick's think; its other actions carry on.
/// `PONKO_NOT_FOUND` if it isn't doing it.
/// ```
/// use ponko_ffi::PonkoAiStatus::*;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_finish(std::ptr::null_mut(), 0, 0, PONKO_AI_SUCCESS) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_finish(
    ctx: *mut PonkoCtx,
    entity: u64,
    action: u64,
    status: PonkoAiStatus,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| finish_action(brain(c, entity)?, action, status)) })
}

/// The number parameter `key` of running action `action` (a key id; 0 for
/// the first) of the world brain of `entity` (Rust `Brain::action_param`).
/// `PONKO_NOT_FOUND` if it isn't doing it or the action has no such number.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_param_num(std::ptr::null_mut(), 0, 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_param_num(
    ctx: *mut PonkoCtx,
    entity: u64,
    action: u64,
    key: u64,
    out_value: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let value = ctx_access(ctx, |c| action_param_num(brain(c, entity)?, action, key))?;
        out(out_value, value)
    })
}

// ---- blackboards ------------------------------------------------------------------

unsafe fn set(ctx: *mut PonkoCtx, entity: u64, key: u64, value: Value) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let key = key_of(key)?;
            blackboard(c, entity)?.set(key, value);
            Ok(())
        })
    })
}

/// Sets entry `key` of the blackboard of `entity` to a number (Rust
/// `Blackboard::set`): the senses of a world brain. `PONKO_NOT_FOUND` if
/// it has no blackboard (`ponko_ctx_ai_add_brain` gives it one).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_set_num(std::ptr::null_mut(), 0, 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_set_num(ctx: *mut PonkoCtx, entity: u64, key: u64, value: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(ctx, entity, key, Value::Num(value)) }
}

/// Sets entry `key` of the blackboard of `entity` to a flag.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_set_flag(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_set_flag(ctx: *mut PonkoCtx, entity: u64, key: u64, value: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(ctx, entity, key, Value::Bool(value)) }
}

/// Sets entry `key` of the blackboard of `entity` to a 2D point.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_set_vec2(std::ptr::null_mut(), 0, 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_set_vec2(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    value: PonkoVec2,
) -> PonkoStatus {
    let v: Vec2 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { set(ctx, entity, key, Value::Vec2(v)) }
}

/// Sets entry `key` of the blackboard of `entity` to a 3D point.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_set_vec3(std::ptr::null_mut(), 0, 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_set_vec3(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    value: PonkoVec3,
) -> PonkoStatus {
    let v: Vec3 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { set(ctx, entity, key, Value::Vec3(v)) }
}

/// Sets entry `key` of the blackboard of `entity` to another entity (its
/// bits).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_set_entity(std::ptr::null_mut(), 0, 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_set_entity(ctx: *mut PonkoCtx, entity: u64, key: u64, value: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(ctx, entity, key, Value::Entity(Entity::from_bits(value))) }
}

unsafe fn get(ctx: *mut PonkoCtx, entity: u64, key: u64) -> Result<Option<Value>, Failure> {
    // SAFETY: forwarded caller contract.
    unsafe {
        ctx_access(ctx, |c| {
            let bb = blackboard(c, entity)?;
            Ok(Key::from_id(key).and_then(|k| bb.get(k)))
        })
    }
}

/// The number at entry `key` of the blackboard of `entity`.
/// `PONKO_NOT_FOUND` if it has no blackboard, or no number there.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_num(std::ptr::null_mut(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_num(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    out_value: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let value = get(ctx, entity, key)?
            .and_then(Value::as_num)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no number at that key"))?;
        out(out_value, value)
    })
}

/// The flag at entry `key` of the blackboard of `entity`.
/// `PONKO_NOT_FOUND` if it has no blackboard, or no flag there.
/// ```
/// let mut v = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_flag(std::ptr::null_mut(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_flag(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    out_value: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let value = match get(ctx, entity, key)? {
            Some(Value::Bool(b)) => b,
            _ => return Err(error(PonkoStatus::PONKO_NOT_FOUND, "no flag at that key")),
        };
        out(out_value, value)
    })
}

/// The entity (its bits) at entry `key` of the blackboard of `entity`.
/// `PONKO_NOT_FOUND` if it has no blackboard, or no entity there.
///
/// Ownership: `out_value` is the caller's, written during the call only.
/// ```
/// let mut v = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_entity(std::ptr::null_mut(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_entity(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    out_value: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let e = ctx_access(ctx, |c| {
            board_value(blackboard(c, entity)?, key, Value::as_entity, NO_ENTITY)
        })?;
        out(out_value, e.to_bits())
    })
}

/// The 2D point at entry `key` of the blackboard of `entity`.
/// `PONKO_NOT_FOUND` if it has no blackboard, or no 2D point there.
///
/// Ownership: `out_value` is the caller's, written during the call only.
/// ```
/// let mut v = ponko_ffi::PonkoVec2::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_vec2(std::ptr::null_mut(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_vec2(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    out_value: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| {
            board_value(blackboard(c, entity)?, key, Value::as_vec2, NO_VEC2)
        })?;
        out(out_value, to_c(v))
    })
}

/// The 3D point at entry `key` of the blackboard of `entity`.
/// `PONKO_NOT_FOUND` if it has no blackboard, or no 3D point there.
///
/// Ownership: `out_value` is the caller's, written during the call only.
/// ```
/// let mut v = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ai_vec3(std::ptr::null_mut(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ai_vec3(
    ctx: *mut PonkoCtx,
    entity: u64,
    key: u64,
    out_value: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let v = ctx_access(ctx, |c| {
            board_value(blackboard(c, entity)?, key, Value::as_vec3, NO_VEC3)
        })?;
        out(out_value, to_c(v))
    })
}
