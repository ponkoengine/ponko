//! World brains inside C loops: the `Brain` and `Blackboard` components
//! are query columns (`ponko_ai_brain_component`,
//! `ponko_ai_blackboard_component`), so a `ponko_query` visits each
//! entity's brain and blackboard with its other components, and the
//! `ponko_ai_brain_*` and `ponko_ai_blackboard_*` calls read and write
//! them row by row without the ctx: they work inside `ponko_query` and
//! `ponko_query_par` callbacks, where ctx calls are refused. Rust: a
//! `Query<(&mut Brain, &mut Blackboard, ...)>`.
//!
//! The blackboard getters for entities and points (`*_entity`, `*_vec2`,
//! `*_vec3`) are here too, for every kind of brain.
use ponko::ai::{Blackboard, Brain, Key, Value};
use ponko::prelude::{Entity, Vec2, Vec3};

use crate::ai::{PonkoAiStatus, action_param_num, finish_action, key_of, named_action};
use crate::ctx::out;
use crate::types::*;
use crate::{Failure, PonkoChunk, PonkoStatus, PonkoVec3, PonkoWorld, boundary, builtin, error, invalid};

/// A world brain (Rust `ai::Brain`) in a query column, opaque: reach a row
/// with `ponko_chunk_ai_brain`, and read it with the `ponko_ai_brain_*`
/// calls. Valid until the query callback returns.
pub struct PonkoAiBrain {
    _opaque: [u8; 0],
}

/// A blackboard (Rust `ai::Blackboard`) in a query column, opaque: reach a
/// row with `ponko_chunk_ai_blackboard`, and use it with the
/// `ponko_ai_blackboard_*` calls. Valid until the query callback returns.
pub struct PonkoAiBlackboard {
    _opaque: [u8; 0],
}

// Rows of one column are used from many threads at once (`ponko_query_par`),
// each row by one thread.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<Brain>();
    send_sync::<Blackboard>();
};

/// Whether world component `id` is a brain or blackboard column: Rust
/// types without a C layout that queries still take, because C reads
/// their rows only through the calls here.
pub(crate) fn is_ai_column(world: &ponko_ecs::World, id: ponko_ecs::ComponentId) -> bool {
    let t = world.component_info(id).type_id();
    t == Some(std::any::TypeId::of::<Brain>()) || t == Some(std::any::TypeId::of::<Blackboard>())
}

/// The component id of world brains (Rust `ai::Brain`), for the ids of a
/// `ponko_query`: the query then visits only entities with a brain, and
/// `ponko_chunk_ai_brain` reaches each row's. `ponko_get`, `ponko_set`
/// and `ponko_spawn` refuse it (`ponko_ctx_ai_add_brain` gives a brain).
///
/// Ownership: `out_id` is the caller's, written during the call only.
/// ```
/// let w = ponko_ffi::ponko_world_new();
/// let mut id = u32::MAX;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_brain_component(w, &mut id) } as u32, 0);
/// assert_ne!(id, u32::MAX);
/// unsafe { ponko_ffi::ponko_world_free(w) };
/// ```
/// # Safety
/// `world` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_brain_component(world: *mut PonkoWorld, out_id: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { builtin::<Brain>(world, out_id) }
}

/// The component id of blackboards (Rust `ai::Blackboard`), for the ids
/// of a `ponko_query`; `ponko_chunk_ai_blackboard` reaches each row's.
///
/// Ownership: `out_id` is the caller's, written during the call only.
/// ```
/// let w = ponko_ffi::ponko_world_new();
/// let mut id = u32::MAX;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_component(w, &mut id) } as u32, 0);
/// assert_ne!(id, u32::MAX);
/// unsafe { ponko_ffi::ponko_world_free(w) };
/// ```
/// # Safety
/// `world` null or live; `out_id` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_component(world: *mut PonkoWorld, out_id: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { builtin::<Blackboard>(world, out_id) }
}

/// Row `row` of column `column` of `chunk`, a column of `T`; null when out
/// of range.
unsafe fn row_of<T>(chunk: *const PonkoChunk, column: usize, row: usize) -> *mut u8 {
    // SAFETY: caller contract: null or the chunk of a running query callback.
    let Some(chunk) = (unsafe { chunk.as_ref() }) else {
        return std::ptr::null_mut();
    };
    if column >= chunk.column_count || row >= chunk.count || chunk.columns.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: caller contract: column `column` holds `count` packed values
    // of `T` (its stride is the component's size); both are in range.
    unsafe { (*chunk.columns.add(column)).cast::<T>().add(row).cast() }
}

/// The world brain in row `row` of a query's brain column `column` (the
/// place of `ponko_ai_brain_component`'s id in the query's ids). Null when
/// `column` or `row` is out of range. Valid until the callback returns;
/// in `ponko_query_par`, use only the rows of the callback's own chunk.
///
/// Ownership: the returned brain is borrowed from the world until the
/// callback returns; never free it.
/// ```
/// assert!(unsafe { ponko_ffi::ponko_chunk_ai_brain(std::ptr::null(), 0, 0) }.is_null());
/// ```
/// # Safety
/// `chunk` null or the chunk passed to a running query callback, whose
/// column `column` is the brain column.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_chunk_ai_brain(
    chunk: *const PonkoChunk,
    column: usize,
    row: usize,
) -> *mut PonkoAiBrain {
    // SAFETY: forwarded caller contract.
    unsafe { row_of::<Brain>(chunk, column, row).cast() }
}

/// The blackboard in row `row` of a query's blackboard column `column`.
/// Null when out of range; valid until the callback returns.
///
/// Ownership: the returned blackboard is borrowed from the world until the
/// callback returns; never free it.
/// ```
/// assert!(unsafe { ponko_ffi::ponko_chunk_ai_blackboard(std::ptr::null(), 0, 0) }.is_null());
/// ```
/// # Safety
/// `chunk` null or the chunk passed to a running query callback, whose
/// column `column` is the blackboard column.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_chunk_ai_blackboard(
    chunk: *const PonkoChunk,
    column: usize,
    row: usize,
) -> *mut PonkoAiBlackboard {
    // SAFETY: forwarded caller contract.
    unsafe { row_of::<Blackboard>(chunk, column, row).cast() }
}

unsafe fn brain_ref<'a>(brain: *const PonkoAiBrain) -> Result<&'a Brain, Failure> {
    // SAFETY: caller contract: null or a row from `ponko_chunk_ai_brain`.
    unsafe { brain.cast::<Brain>().as_ref() }.ok_or_else(|| invalid("brain is null"))
}

unsafe fn brain_mut<'a>(brain: *mut PonkoAiBrain) -> Result<&'a mut Brain, Failure> {
    // SAFETY: caller contract: null or a row from `ponko_chunk_ai_brain`,
    // used by one thread.
    unsafe { brain.cast::<Brain>().as_mut() }.ok_or_else(|| invalid("brain is null"))
}

unsafe fn board_ref<'a>(board: *const PonkoAiBlackboard) -> Result<&'a Blackboard, Failure> {
    // SAFETY: caller contract: null or a row from `ponko_chunk_ai_blackboard`.
    unsafe { board.cast::<Blackboard>().as_ref() }.ok_or_else(|| invalid("blackboard is null"))
}

unsafe fn board_mut<'a>(board: *mut PonkoAiBlackboard) -> Result<&'a mut Blackboard, Failure> {
    // SAFETY: caller contract: null or a row from `ponko_chunk_ai_blackboard`,
    // used by one thread.
    unsafe { board.cast::<Blackboard>().as_mut() }.ok_or_else(|| invalid("blackboard is null"))
}

/// The key id of the brain's `index`th action in tree order, or 0 past the
/// last (as `ponko_ctx_ai_action`). Safe inside loops and from
/// `ponko_query_par`.
///
/// Ownership: `brain` is read and `out_key` written during the call only.
/// ```
/// let mut key = 1;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_brain_action(std::ptr::null(), 0, &mut key) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or a row from `ponko_chunk_ai_brain`; `out_key` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_brain_action(
    brain: *const PonkoAiBrain,
    index: usize,
    out_key: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_key, brain_ref(brain)?.actions().nth(index).map_or(0, Key::id)) })
}

/// Whether running action `action` (a key id; 0 for the first) started at
/// this tick's think (as `ponko_ctx_ai_started`). `PONKO_NOT_FOUND` if it
/// isn't doing it.
///
/// Ownership: `brain` is read and `out_started` written during the call only.
/// ```
/// let mut started = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_brain_started(std::ptr::null(), 0, &mut started) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or a row from `ponko_chunk_ai_brain`; `out_started` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_brain_started(
    brain: *const PonkoAiBrain,
    action: u64,
    out_started: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_ref(brain)?;
        out(out_started, b.started_action(named_action(b, action)?))
    })
}

/// Reports running action `action` (a key id; 0 for the first) done, as
/// `PONKO_AI_SUCCESS` or `PONKO_AI_FAILURE` (as `ponko_ctx_ai_finish`):
/// the brain goes on at the next think. `PONKO_NOT_FOUND` if it isn't
/// doing it.
///
/// Ownership: `brain` is changed during the call only.
/// ```
/// use ponko_ffi::PonkoAiStatus::*;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_brain_finish(std::ptr::null_mut(), 0, PONKO_AI_SUCCESS) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or a row from `ponko_chunk_ai_brain`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_brain_finish(
    brain: *mut PonkoAiBrain,
    action: u64,
    status: PonkoAiStatus,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { finish_action(brain_mut(brain)?, action, status) })
}

/// The number parameter `key` of running action `action` (a key id; 0 for
/// the first; as `ponko_ctx_ai_param_num`). `PONKO_NOT_FOUND` if it isn't
/// doing it or the action has no such number.
///
/// Ownership: `brain` is read and `out_value` written during the call only.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_brain_param_num(std::ptr::null(), 0, 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or a row from `ponko_chunk_ai_brain`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_brain_param_num(
    brain: *const PonkoAiBrain,
    action: u64,
    key: u64,
    out_value: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_value, action_param_num(brain_ref(brain)?, action, key)?) })
}

// ---- blackboard rows ---------------------------------------------------------------

unsafe fn board_set(board: *mut PonkoAiBlackboard, key: u64, value: Value) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let board = board_mut(board)?;
        board.set(key_of(key)?, value);
        Ok(())
    })
}

/// Sets entry `key` of a blackboard row to a number (as
/// `ponko_ctx_ai_set_num`). Safe inside loops and from `ponko_query_par`.
///
/// Ownership: `board` is changed during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_set_num(std::ptr::null_mut(), 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_set_num(
    board: *mut PonkoAiBlackboard,
    key: u64,
    value: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { board_set(board, key, Value::Num(value)) }
}

/// Sets entry `key` of a blackboard row to a flag.
///
/// Ownership: `board` is changed during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_set_flag(std::ptr::null_mut(), 0, true) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_set_flag(
    board: *mut PonkoAiBlackboard,
    key: u64,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { board_set(board, key, Value::Bool(value)) }
}

/// Sets entry `key` of a blackboard row to a 2D point.
///
/// Ownership: `board` is changed during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_set_vec2(std::ptr::null_mut(), 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_set_vec2(
    board: *mut PonkoAiBlackboard,
    key: u64,
    value: PonkoVec2,
) -> PonkoStatus {
    let v: Vec2 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { board_set(board, key, Value::Vec2(v)) }
}

/// Sets entry `key` of a blackboard row to a 3D point.
///
/// Ownership: `board` is changed during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_set_vec3(std::ptr::null_mut(), 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_set_vec3(
    board: *mut PonkoAiBlackboard,
    key: u64,
    value: PonkoVec3,
) -> PonkoStatus {
    let v: Vec3 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { board_set(board, key, Value::Vec3(v)) }
}

/// Sets entry `key` of a blackboard row to an entity (its bits).
///
/// Ownership: `board` is changed during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_set_entity(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_set_entity(
    board: *mut PonkoAiBlackboard,
    key: u64,
    value: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { board_set(board, key, Value::Entity(Entity::from_bits(value))) }
}

/// Entry `key` of `board` read with `pick`, or `PONKO_NOT_FOUND` naming
/// `what` when there is no such entry or it holds another kind of value.
/// Shared by the world, row and native blackboard getters.
pub(crate) fn board_value<T>(
    board: &Blackboard,
    key: u64,
    pick: fn(Value) -> Option<T>,
    what: &'static str,
) -> Result<T, Failure> {
    Key::from_id(key)
        .and_then(|k| board.get(k))
        .and_then(pick)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, what))
}

pub(crate) const NO_NUM: &str = "no number at that key";
pub(crate) const NO_FLAG: &str = "no flag at that key";
pub(crate) const NO_ENTITY: &str = "no entity at that key";
pub(crate) const NO_VEC2: &str = "no 2D point at that key";
pub(crate) const NO_VEC3: &str = "no 3D point at that key";

/// The number at entry `key` of a blackboard row. `PONKO_NOT_FOUND` if
/// there is none. Safe inside loops and from `ponko_query_par`.
///
/// Ownership: `board` is read and `out_value` written during the call only.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_num(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_num(
    board: *const PonkoAiBlackboard,
    key: u64,
    out_value: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_value, board_value(board_ref(board)?, key, Value::as_num, NO_NUM)?) })
}

/// The flag at entry `key` of a blackboard row. `PONKO_NOT_FOUND` if there
/// is none.
///
/// Ownership: `board` is read and `out_value` written during the call only.
/// ```
/// let mut v = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_flag(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_flag(
    board: *const PonkoAiBlackboard,
    key: u64,
    out_value: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_value, board_value(board_ref(board)?, key, Value::as_bool, NO_FLAG)?) })
}

/// The entity (its bits) at entry `key` of a blackboard row.
/// `PONKO_NOT_FOUND` if there is none.
///
/// Ownership: `board` is read and `out_value` written during the call only.
/// ```
/// let mut v = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_entity(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_entity(
    board: *const PonkoAiBlackboard,
    key: u64,
    out_value: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let e = board_value(board_ref(board)?, key, Value::as_entity, NO_ENTITY)?;
        out(out_value, e.to_bits())
    })
}

/// The 2D point at entry `key` of a blackboard row. `PONKO_NOT_FOUND` if
/// there is none.
///
/// Ownership: `board` is read and `out_value` written during the call only.
/// ```
/// let mut v = ponko_ffi::PonkoVec2::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_vec2(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_vec2(
    board: *const PonkoAiBlackboard,
    key: u64,
    out_value: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        out(
            out_value,
            to_c(board_value(board_ref(board)?, key, Value::as_vec2, NO_VEC2)?),
        )
    })
}

/// The 3D point at entry `key` of a blackboard row. `PONKO_NOT_FOUND` if
/// there is none.
///
/// Ownership: `board` is read and `out_value` written during the call only.
/// ```
/// let mut v = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_blackboard_vec3(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `board` null or a row from `ponko_chunk_ai_blackboard`; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_blackboard_vec3(
    board: *const PonkoAiBlackboard,
    key: u64,
    out_value: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        out(
            out_value,
            to_c(board_value(board_ref(board)?, key, Value::as_vec3, NO_VEC3)?),
        )
    })
}
