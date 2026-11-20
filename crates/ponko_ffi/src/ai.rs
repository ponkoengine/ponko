//! The AI kit from C: behaviour trees compiled from JSON, and brains with
//! their blackboards that a native game keeps in its own storage (Rust
//! `ponko::ai::BehaviorTree`, `Brain`, `Blackboard`, utility helpers).
//! Keys are passed by id (`ponko_ai_key`).
use std::ffi::c_char;
use std::sync::Arc;

use ponko::ai::{BehaviorTree, Blackboard, Brain, Curve, Key, Status, Value};
use ponko::prelude::{Entity, Vec2, Vec3};

use crate::PonkoVec3;
use crate::ctx::{out, text};
use crate::types::*;
use crate::{Failure, PonkoStatus, boundary, error, invalid, invalid_owned, write_text};

/// A compiled behaviour tree. Read only by brains, so one tree may be used
/// from many threads at once.
pub struct PonkoBehaviorTree(Arc<BehaviorTree>);

/// One agent's brain and blackboard. Use from one thread at a time.
pub struct PonkoBrain {
    brain: Brain,
    bb: Blackboard,
}

/// What an action or node did (Rust `ponko::ai::Status`).
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAiStatus {
    /// Not finished.
    PONKO_AI_RUNNING = 0,
    /// Done, and it worked.
    PONKO_AI_SUCCESS = 1,
    /// Done, and it didn't.
    PONKO_AI_FAILURE = 2,
}

/// A response curve (Rust `ponko::ai::Curve`); `param` of
/// `ponko_ai_curve` is the exponent, steepness or threshold of the last three.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAiCurve {
    /// y = x.
    PONKO_AI_CURVE_LINEAR = 0,
    /// y = x^2.
    PONKO_AI_CURVE_QUADRATIC = 1,
    /// y = x^3.
    PONKO_AI_CURVE_CUBIC = 2,
    /// y = sqrt(x).
    PONKO_AI_CURVE_SQRT = 3,
    /// Smoothstep.
    PONKO_AI_CURVE_SMOOTH = 4,
    /// y = x^param.
    PONKO_AI_CURVE_POWER = 5,
    /// An S curve around 0.5 of steepness param.
    PONKO_AI_CURVE_LOGISTIC = 6,
    /// 0 below param, 1 from it on.
    PONKO_AI_CURVE_STEP = 7,
}

unsafe fn tree_ref<'a>(tree: *const PonkoBehaviorTree) -> Result<&'a Arc<BehaviorTree>, Failure> {
    // SAFETY: caller supplies null or a live tree.
    unsafe { tree.as_ref() }
        .map(|t| &t.0)
        .ok_or_else(|| invalid("behaviour tree is null"))
}

unsafe fn brain_ref<'a>(brain: *const PonkoBrain) -> Result<&'a PonkoBrain, Failure> {
    // SAFETY: caller supplies null or a live brain.
    unsafe { brain.as_ref() }.ok_or_else(|| invalid("brain is null"))
}

unsafe fn brain_mut<'a>(brain: *mut PonkoBrain) -> Result<&'a mut PonkoBrain, Failure> {
    // SAFETY: caller supplies null or a live brain, used by one thread.
    unsafe { brain.as_mut() }.ok_or_else(|| invalid("brain is null"))
}

fn key_of(id: u64) -> Result<Key, Failure> {
    Key::from_id(id).ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no such key: make it with ponko_ai_key"))
}

/// The id of the key `name` (Rust `Key::named(name).id()`): blackboard
/// entries, actions and parameters are passed by id. Call it once per
/// name, when loading, and keep the id. Ids are the same in every run.
/// ```
/// let mut id = 0;
/// let name = c"health";
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_key(name.as_ptr(), &mut id) } as u32, 0);
/// assert_eq!(id, ponko::ai::Key::new("health").id());
/// ```
/// # Safety
/// `name` null or a NUL-terminated string; `out_key` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_key(name: *const c_char, out_key: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let name = text(name)?;
        if name.is_empty() {
            return Err(invalid("a key name can't be empty"));
        }
        out(out_key, Key::named(name).id())
    })
}

/// Writes the name of key `key` (`(buffer, capacity, out_len)`, see
/// `ponko_last_error`). `PONKO_NOT_FOUND` for an id no name has.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_key_name(1, std::ptr::null_mut(), 0, &mut len) } as u32, 2);
/// ```
/// # Safety
/// `buffer` writable for `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_key_name(
    key: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_text(buffer, capacity, out_len, key_of(key)?.name()) })
}

/// Compiles a tree from JSON (Rust `BehaviorTree::from_json`; the format is
/// in the guide's "Game AI"). Returns null on a bad tree; `ponko_last_error`
/// names the node and the problem.
/// ```
/// let tree = unsafe { ponko_ffi::ponko_behavior_tree_new(c"{\"root\": {\"do\": \"idle\"}}".as_ptr()) };
/// assert!(!tree.is_null());
/// unsafe { ponko_ffi::ponko_behavior_tree_free(tree) };
/// assert!(unsafe { ponko_ffi::ponko_behavior_tree_new(c"{}".as_ptr()) }.is_null());
/// ```
/// # Safety
/// `json` null or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_behavior_tree_new(json: *const c_char) -> *mut PonkoBehaviorTree {
    let mut tree = std::ptr::null_mut();
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let compiled = BehaviorTree::from_json(text(json)?).map_err(invalid_owned)?;
        tree = Box::into_raw(Box::new(PonkoBehaviorTree(Arc::new(compiled))));
        Ok(())
    });
    tree
}

/// Frees a tree. Null is ignored. Brains that ran it keep what they need.
/// ```
/// unsafe { ponko_ffi::ponko_behavior_tree_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `tree` null or from `ponko_behavior_tree_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_behavior_tree_free(tree: *mut PonkoBehaviorTree) {
    if !tree.is_null() {
        // SAFETY: caller passes a tree from ponko_behavior_tree_new, once.
        drop(unsafe { Box::from_raw(tree) });
    }
}

/// How many nodes the tree has.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_behavior_tree_len(std::ptr::null(), &mut n) } as u32, 1);
/// ```
/// # Safety
/// `tree` null or live; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_behavior_tree_len(tree: *const PonkoBehaviorTree, out_len: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_len, tree_ref(tree)?.len()) })
}

/// A brain with an empty blackboard that thinks every `period` ticks (0
/// counts as 1; see `ponko_brain_due`). It has no tree until its first
/// `ponko_brain_think`.
/// ```
/// let brain = ponko_ffi::ponko_brain_new(1);
/// unsafe { ponko_ffi::ponko_brain_free(brain) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_brain_new(period: u32) -> *mut PonkoBrain {
    Box::into_raw(Box::new(PonkoBrain {
        brain: Brain::new("native").with_period(period),
        bb: Blackboard::new(),
    }))
}

/// Frees a brain. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_brain_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `brain` null or from `ponko_brain_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_free(brain: *mut PonkoBrain) {
    if !brain.is_null() {
        // SAFETY: caller passes a brain from ponko_brain_new, once.
        drop(unsafe { Box::from_raw(brain) });
    }
}

/// Thinks once with `tree` at sim tick `tick` of `dt` seconds (Rust
/// `Brain::think`): runs the tree from the root over the brain's
/// blackboard. Another tree than last time starts over. Brains are
/// independent: think different brains on different threads at once.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_think(std::ptr::null_mut(), std::ptr::null(), 0, 0.1) } as u32, 1);
/// ```
/// # Safety
/// `brain` and `tree` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_think(
    brain: *mut PonkoBrain,
    tree: *const PonkoBehaviorTree,
    tick: u64,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let tree = tree_ref(tree)?;
        let b = brain_mut(brain)?;
        b.brain.think(tree, &mut b.bb, tick, dt);
        Ok(())
    })
}

/// Whether the brain should think at `tick` for an agent numbered `index`
/// (Rust `Brain::due`): when woken, before its first think, or when
/// `tick + index` is a multiple of its period.
/// ```
/// let mut due = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_due(std::ptr::null(), 0, 0, &mut due) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_due` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_due(
    brain: *const PonkoBrain,
    index: u32,
    tick: u64,
    out_due: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_due, brain_ref(brain)?.brain.due(index, tick)) })
}

/// Sets how many ticks apart it thinks (0 counts as 1).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_period(std::ptr::null_mut(), 4) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_period(brain: *mut PonkoBrain, period: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        brain_mut(brain)?.brain.set_period(period);
        Ok(())
    })
}

/// The key id of the action it is doing, or 0 when it does none.
/// ```
/// let mut key = 1;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_action(std::ptr::null(), &mut key) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_key` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_action(brain: *const PonkoBrain, out_key: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_key, brain_ref(brain)?.brain.action().map_or(0, Key::id)) })
}

/// Whether its action started at the last think.
/// ```
/// let mut started = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_started(std::ptr::null(), &mut started) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_started` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_started(brain: *const PonkoBrain, out_started: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_started, brain_ref(brain)?.brain.started()) })
}

/// The number parameter `key` of the current action (its `do` node's
/// other fields). `PONKO_NOT_FOUND` if it has none, or it isn't a number.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_param_num(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_param_num(brain: *const PonkoBrain, key: u64, out_value: *mut f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_ref(brain)?;
        let value = Key::from_id(key)
            .and_then(|k| b.brain.param_num(k))
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "the action has no such number parameter"))?;
        out(out_value, value)
    })
}

/// Reports the current action done (`PONKO_AI_SUCCESS` or
/// `PONKO_AI_FAILURE`; Rust `Brain::succeed`, `Brain::fail`): the tree goes
/// on at the next think, which is due at once. Running is invalid.
/// ```
/// use ponko_ffi::PonkoAiStatus::*;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_finish(std::ptr::null_mut(), PONKO_AI_SUCCESS) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_finish(brain: *mut PonkoBrain, status: PonkoAiStatus) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_mut(brain)?;
        match status {
            PonkoAiStatus::PONKO_AI_SUCCESS => b.brain.succeed(),
            PonkoAiStatus::PONKO_AI_FAILURE => b.brain.fail(),
            PonkoAiStatus::PONKO_AI_RUNNING => return Err(invalid("finish takes success or failure")),
        }
        Ok(())
    })
}

/// What the game reported for the current action (Running until finished).
/// ```
/// let mut s = ponko_ffi::PonkoAiStatus::PONKO_AI_RUNNING;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_status(std::ptr::null(), &mut s) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_status` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_status(brain: *const PonkoBrain, out_status: *mut PonkoAiStatus) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let status = match brain_ref(brain)?.brain.status() {
            Status::Running => PonkoAiStatus::PONKO_AI_RUNNING,
            Status::Success => PonkoAiStatus::PONKO_AI_SUCCESS,
            Status::Failure => PonkoAiStatus::PONKO_AI_FAILURE,
        };
        out(out_status, status)
    })
}

/// Makes it think at the next chance, whatever its period.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_wake(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_wake(brain: *mut PonkoBrain) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        brain_mut(brain)?.brain.wake();
        Ok(())
    })
}

unsafe fn set(brain: *mut PonkoBrain, key: u64, value: Value) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_mut(brain)?;
        b.bb.set(key_of(key)?, value);
        Ok(())
    })
}

/// Sets blackboard entry `key` to a number. `PONKO_NOT_FOUND` for a key id
/// that `ponko_ai_key` didn't make.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_num(std::ptr::null_mut(), 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_num(brain: *mut PonkoBrain, key: u64, value: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(brain, key, Value::Num(value)) }
}

/// Sets blackboard entry `key` to a flag.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_flag(std::ptr::null_mut(), 0, true) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_flag(brain: *mut PonkoBrain, key: u64, value: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(brain, key, Value::Bool(value)) }
}

/// Sets blackboard entry `key` to a 2D point.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_vec2(std::ptr::null_mut(), 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_vec2(brain: *mut PonkoBrain, key: u64, value: PonkoVec2) -> PonkoStatus {
    let v: Vec2 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { set(brain, key, Value::Vec2(v)) }
}

/// Sets blackboard entry `key` to a 3D point.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_vec3(std::ptr::null_mut(), 0, Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_vec3(brain: *mut PonkoBrain, key: u64, value: PonkoVec3) -> PonkoStatus {
    let v: Vec3 = to_c(value);
    // SAFETY: forwarded caller contract.
    unsafe { set(brain, key, Value::Vec3(v)) }
}

/// Sets blackboard entry `key` to an entity (its bits).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_set_entity(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_set_entity(brain: *mut PonkoBrain, key: u64, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set(brain, key, Value::Entity(Entity::from_bits(entity))) }
}

/// The number at blackboard entry `key`. `PONKO_NOT_FOUND` if it has none
/// or it isn't a number.
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_num(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_num(brain: *const PonkoBrain, key: u64, out_value: *mut f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_ref(brain)?;
        let value = Key::from_id(key)
            .and_then(|k| b.bb.get(k))
            .and_then(Value::as_num)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no such number on the blackboard"))?;
        out(out_value, value)
    })
}

/// The flag at blackboard entry `key`. `PONKO_NOT_FOUND` if it has none or
/// it isn't a flag.
/// ```
/// let mut v = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_flag(std::ptr::null(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `out_value` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_flag(brain: *const PonkoBrain, key: u64, out_value: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = brain_ref(brain)?;
        let value = Key::from_id(key)
            .and_then(|k| b.bb.get(k))
            .and_then(Value::as_bool)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no such flag on the blackboard"))?;
        out(out_value, value)
    })
}

/// Writes the brain's blackboard as JSON (`{"health": 0.5, ...}`), for
/// logs and tools.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_blackboard_json(std::ptr::null(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `buffer` writable for `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_blackboard_json(
    brain: *const PonkoBrain,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let json = serde_json::to_string(&brain_ref(brain)?.bb).map_err(|e| invalid_owned(e.to_string()))?;
        write_text(buffer, capacity, out_len, &json)
    })
}

/// Writes the tree as the debug view shows it, one node a line: indented
/// by depth, marked with what it did at the last think (`>` running, `+`
/// success, `x` failure, `.` not reached), and a utility child's score.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_brain_trace(std::ptr::null(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `brain` null or live; `buffer` writable for `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_brain_trace(
    brain: *const PonkoBrain,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut text = String::new();
        for line in brain_ref(brain)?.brain.trace() {
            let mark = match line.status {
                Some(Status::Running) => '>',
                Some(Status::Success) => '+',
                Some(Status::Failure) => 'x',
                None => '.',
            };
            text.push_str(&"  ".repeat(line.depth));
            text.push(mark);
            text.push(' ');
            text.push_str(line.label);
            if let Some(score) = line.score {
                text.push_str(&format!(" [{score:.2}]"));
            }
            text.push('\n');
        }
        write_text(buffer, capacity, out_len, &text)
    })
}

/// The score of `x` (clamped to 0..1) on `curve` (Rust `Curve::eval`).
/// ```
/// use ponko_ffi::PonkoAiCurve::*;
/// assert_eq!(ponko_ffi::ponko_ai_curve(PONKO_AI_CURVE_QUADRATIC, 0.0, 0.5), 0.25);
/// assert_eq!(ponko_ffi::ponko_ai_curve(PONKO_AI_CURVE_STEP, 0.3, 0.2), 0.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ai_curve(curve: PonkoAiCurve, param: f32, x: f32) -> f32 {
    use PonkoAiCurve::*;
    let curve = match curve {
        PONKO_AI_CURVE_LINEAR => Curve::Linear,
        PONKO_AI_CURVE_QUADRATIC => Curve::Quadratic,
        PONKO_AI_CURVE_CUBIC => Curve::Cubic,
        PONKO_AI_CURVE_SQRT => Curve::Sqrt,
        PONKO_AI_CURVE_SMOOTH => Curve::Smooth,
        PONKO_AI_CURVE_POWER => Curve::Power(param),
        PONKO_AI_CURVE_LOGISTIC => Curve::Logistic(param),
        PONKO_AI_CURVE_STEP => Curve::Step(param),
    };
    curve.eval(x)
}

/// Combines `count` scores into one (Rust `ponko::ai::combine`: the
/// product, made up for the number of factors). Null with count 0 is 1.
/// ```
/// let scores = [0.5f32, 0.5];
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_combine(scores.as_ptr(), 2) }, 0.390625);
/// assert_eq!(unsafe { ponko_ffi::ponko_ai_combine(std::ptr::null(), 0) }, 1.0);
/// ```
/// # Safety
/// `scores` readable for `count` floats (or null with count 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ai_combine(scores: *const f32, count: usize) -> f32 {
    if scores.is_null() || count == 0 {
        return 1.0;
    }
    // SAFETY: caller supplies `count` readable floats.
    ponko::ai::combine(unsafe { std::slice::from_raw_parts(scores, count) })
}
