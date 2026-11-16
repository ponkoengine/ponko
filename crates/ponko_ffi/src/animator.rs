//! Animation graphs and animators from C (Rust `AnimationGraph`, `Motion`,
//! `AnimationState`, `AnimationTransition`, `Animator`). C builds a graph
//! with a builder: parameters, then states (each from a motion: a clip or a
//! 1D/2D blend tree), then transitions with conditions. The finished graph
//! is immutable and shared by every animator made from it.

use std::ffi::c_char;
use std::sync::Arc;

use ponko::anim::AnimationEvent;
use ponko::anim::AnimationGraph;
use ponko::anim::AnimationGraphError;
use ponko::anim::AnimationLayer;
use ponko::anim::AnimationParameter;
use ponko::anim::AnimationState;
use ponko::anim::AnimationTransition;
use ponko::anim::Animator;
use ponko::anim::LayerBlend;
use ponko::anim::Motion;
use ponko::anim::MotionChild;
use ponko::anim::Skeleton;
use ponko::anim::{AnimationCondition, LayerId, ParamId, StateId};
use ponko::{
    anim::JointMask,
    prelude::{Ctx, Entity, Vec2},
};

use crate::ctx::{ctx_access, out, text};
use crate::skin::PonkoClip;
use crate::three::{PonkoQuat, PonkoVec3};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid, invalid_owned};

/// A motion being built: a clip, or a blend tree of motions. Free it with
/// `ponko_motion_free` unless it was handed to a state or a tree, which
/// takes it over.
pub struct PonkoMotion(Motion);

/// A graph being built, from `ponko_animation_graph_builder_new`. Finish
/// it with `ponko_animation_graph_build` (which frees the builder), or free
/// it with `ponko_animation_graph_builder_free`.
#[derive(Default)]
pub struct PonkoAnimationGraphBuilder {
    parameters: Vec<AnimationParameter>,
    states: Vec<AnimationState>,
    transitions: Vec<AnimationTransition>,
}

/// A finished, shared animation graph. Free it with
/// `ponko_animation_graph_free`; animators made from it keep their own share.
pub struct PonkoAnimationGraph(Arc<AnimationGraph>);

/// What a transition condition tests (Rust `AnimationCondition`).
/// ```
/// let _ = ponko_ffi::PonkoAnimationCondition::PONKO_CONDITION_TRIGGER;
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAnimationCondition {
    /// A float parameter is above `value`.
    PONKO_CONDITION_GREATER = 0,
    /// A float parameter is below `value`.
    PONKO_CONDITION_LESS = 1,
    /// A bool parameter is true (`value` != 0) or false (`value` == 0).
    PONKO_CONDITION_BOOL = 2,
    /// A trigger parameter is set; `value` is unused.
    PONKO_CONDITION_TRIGGER = 3,
}

/// Frees what a builder call took over and turns a bad pointer into an error.
unsafe fn take_motion(motion: *mut PonkoMotion) -> Result<Motion, Failure> {
    if motion.is_null() {
        return Err(invalid("motion is null"));
    }
    // SAFETY: caller passes a motion from ponko_motion_* that nothing else owns.
    Ok(unsafe { Box::from_raw(motion) }.0)
}
fn motion_out(motion: Motion, out_motion: *mut *mut PonkoMotion) -> Result<(), Failure> {
    if out_motion.is_null() {
        return Err(invalid("output is null"));
    }
    // SAFETY: checked non-null; the caller passes writable storage.
    unsafe { out_motion.write(Box::into_raw(Box::new(PonkoMotion(motion)))) };
    Ok(())
}
fn builder<'a>(b: *mut PonkoAnimationGraphBuilder) -> Result<&'a mut PonkoAnimationGraphBuilder, Failure> {
    // SAFETY: caller passes null or a live builder.
    unsafe { b.as_mut() }.ok_or_else(|| invalid("builder is null"))
}
fn index_out(out_index: *mut usize, index: usize) -> Result<(), Failure> {
    if !out_index.is_null() {
        // SAFETY: checked non-null; the caller passes writable storage.
        unsafe { out_index.write(index) };
    }
    Ok(())
}
fn entity(c: &Ctx, bits: u64) -> Result<Entity, Failure> {
    let e = Entity::from_bits(bits);
    if c.world.is_alive(e) {
        Ok(e)
    } else {
        Err(invalid("no such entity"))
    }
}
fn animator(c: &mut Ctx, bits: u64) -> Result<&mut Animator, Failure> {
    let e = entity(c, bits)?;
    c.world
        .get_mut::<Animator>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no animator"))
}

// ---- motions ----------------------------------------------------------------------

/// A motion that plays `clip` (copied; the clip stays yours).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_motion_clip(std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `clip` null or live; `out_motion` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_clip(clip: *const PonkoClip, out_motion: *mut *mut PonkoMotion) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller passes null or a live clip.
        let clip = unsafe { clip.as_ref() }.ok_or_else(|| invalid("clip is null"))?;
        motion_out(Motion::Clip(clip.0.clone()), out_motion)
    })
}

/// An empty 1D blend tree on float parameter `parameter`; add children with
/// `ponko_motion_blend1d_add`, in increasing threshold order.
/// ```
/// let mut m = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_motion_blend1d(0, &mut m) } as u32, 0);
/// unsafe { ponko_ffi::ponko_motion_free(m) };
/// ```
/// # Safety
/// `out_motion` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_blend1d(parameter: usize, out_motion: *mut *mut PonkoMotion) -> PonkoStatus {
    boundary(|| motion_out(Motion::blend_1d(ParamId(parameter), Vec::new()), out_motion))
}

/// An empty 2D blend tree on float parameters `x` and `y`; add children
/// with `ponko_motion_blend2d_add`.
/// ```
/// let mut m = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_motion_blend2d(0, 1, &mut m) } as u32, 0);
/// unsafe { ponko_ffi::ponko_motion_free(m) };
/// ```
/// # Safety
/// `out_motion` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_blend2d(x: usize, y: usize, out_motion: *mut *mut PonkoMotion) -> PonkoStatus {
    boundary(|| motion_out(Motion::blend_2d(ParamId(x), ParamId(y), Vec::new()), out_motion))
}

/// Adds `child` to a 1D tree at `threshold`, its clock scaled by
/// `time_scale` (1 for as recorded). Takes `child` over, even on failure.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_motion_blend1d_add(std::ptr::null_mut(), 0.0, std::ptr::null_mut(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `tree` null or a live motion; `child` null or a motion nothing else owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_blend1d_add(
    tree: *mut PonkoMotion,
    threshold: f32,
    child: *mut PonkoMotion,
    time_scale: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let child = take_motion(child)?;
        match tree.as_mut().map(|t| &mut t.0) {
            Some(Motion::Blend1D { children, .. }) => {
                children.push((threshold, MotionChild::new(child).with_time_scale(time_scale)));
                Ok(())
            }
            Some(_) => Err(invalid("that motion is not a 1D blend tree")),
            None => Err(invalid("tree is null")),
        }
    })
}

/// Adds `child` to a 2D tree at point (`x`, `y`), its clock scaled by
/// `time_scale`. Takes `child` over, even on failure.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_motion_blend2d_add(std::ptr::null_mut(), 0.0, 0.0, std::ptr::null_mut(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `tree` null or a live motion; `child` null or a motion nothing else owns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_blend2d_add(
    tree: *mut PonkoMotion,
    x: f32,
    y: f32,
    child: *mut PonkoMotion,
    time_scale: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let child = take_motion(child)?;
        match tree.as_mut().map(|t| &mut t.0) {
            Some(Motion::Blend2D { children, .. }) => {
                children.push((Vec2::new(x, y), MotionChild::new(child).with_time_scale(time_scale)));
                Ok(())
            }
            Some(_) => Err(invalid("that motion is not a 2D blend tree")),
            None => Err(invalid("tree is null")),
        }
    })
}

/// Frees a motion that wasn't handed over. Null does nothing.
/// ```
/// unsafe { ponko_ffi::ponko_motion_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `motion` null or a motion nothing else owns; not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_motion_free(motion: *mut PonkoMotion) {
    if !motion.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(motion) });
    }
}

// ---- the graph builder ------------------------------------------------------------

/// Starts a graph with no parameters, states or transitions.
/// ```
/// let b = ponko_ffi::ponko_animation_graph_builder_new();
/// unsafe { ponko_ffi::ponko_animation_graph_builder_free(b) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_animation_graph_builder_new() -> *mut PonkoAnimationGraphBuilder {
    Box::into_raw(Box::default())
}

/// Frees a builder that wasn't built. Null does nothing.
/// ```
/// unsafe { ponko_ffi::ponko_animation_graph_builder_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `builder` null or live; not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_builder_free(builder: *mut PonkoAnimationGraphBuilder) {
    if !builder.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(builder) });
    }
}

/// Adds a float parameter starting at `value`; its index goes to `out_index`
/// (parameters are numbered from 0 in the order added, whatever their kind).
/// ```
/// let b = ponko_ffi::ponko_animation_graph_builder_new();
/// let mut i = 9;
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_float(b, 0.0, &mut i) } as u32, 0);
/// assert_eq!(i, 0);
/// unsafe { ponko_ffi::ponko_animation_graph_builder_free(b) };
/// ```
/// # Safety
/// `builder` null or live; `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_float(
    builder: *mut PonkoAnimationGraphBuilder,
    value: f32,
    out_index: *mut usize,
) -> PonkoStatus {
    add_parameter(builder, AnimationParameter::Float(value), out_index)
}

/// Adds a bool parameter starting at `value`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_bool(std::ptr::null_mut(), false, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live; `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_bool(
    builder: *mut PonkoAnimationGraphBuilder,
    value: bool,
    out_index: *mut usize,
) -> PonkoStatus {
    add_parameter(builder, AnimationParameter::Bool(value), out_index)
}

/// Adds a trigger parameter: a one-shot flag a transition consumes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_trigger(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live; `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_trigger(
    builder: *mut PonkoAnimationGraphBuilder,
    out_index: *mut usize,
) -> PonkoStatus {
    add_parameter(builder, AnimationParameter::Trigger(false), out_index)
}

fn add_parameter(b: *mut PonkoAnimationGraphBuilder, p: AnimationParameter, out_index: *mut usize) -> PonkoStatus {
    boundary(|| {
        let b = builder(b)?;
        b.parameters.push(p);
        index_out(out_index, b.parameters.len() - 1)
    })
}

/// Adds a state playing `motion` (taken over, even on failure) at `speed`
/// (1 for as recorded), its speed also multiplied by float parameter
/// `speed_parameter` unless that is -1. Looping states wrap; others hold
/// their last frame. Its index goes to `out_index` (states count from 0).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_state(std::ptr::null_mut(), std::ptr::null_mut(), 1.0, -1, true, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live; `motion` null or a motion nothing else owns;
/// `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_state(
    builder_ptr: *mut PonkoAnimationGraphBuilder,
    motion: *mut PonkoMotion,
    speed: f32,
    speed_parameter: i64,
    looping: bool,
    out_index: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let motion = take_motion(motion)?;
        let b = builder(builder_ptr)?;
        let mut state = AnimationState::new(motion);
        state.speed = speed;
        state.speed_parameter = usize::try_from(speed_parameter).ok().map(ParamId);
        state.looping = looping;
        b.states.push(state);
        index_out(out_index, b.states.len() - 1)
    })
}

/// Adds an event named `name` to `state` at normalized time `time` (0 to
/// 1); animators report it when their clock passes it.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_event(std::ptr::null_mut(), 0, 0.5, c"step".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live; `name` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_event(
    builder_ptr: *mut PonkoAnimationGraphBuilder,
    state: usize,
    time: f32,
    name: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let b = builder(builder_ptr)?;
        let name = text(name)?;
        let s = b
            .states
            .get_mut(state)
            .ok_or_else(|| invalid_owned(format!("no state {state}")))?;
        s.events.push(AnimationEvent::new(time, name));
        Ok(())
    })
}

/// Adds a transition from state `from` (-1: from any state) to `to`,
/// crossfading over `duration` seconds and starting `to` at normalized time
/// `offset`. A negative `exit_time` means none; otherwise the transition
/// waits until the state's normalized time reaches it. With no conditions
/// it always fires. The first matching transition, in the order added, wins.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_transition(std::ptr::null_mut(), 0, 1, -1.0, 0.2, 0.0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live; `out_index` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_transition(
    builder_ptr: *mut PonkoAnimationGraphBuilder,
    from: i64,
    to: usize,
    exit_time: f32,
    duration: f32,
    offset: f32,
    out_index: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let b = builder(builder_ptr)?;
        let transition = match usize::try_from(from) {
            Ok(from) => AnimationTransition::new(StateId(from), StateId(to)),
            Err(_) => AnimationTransition::from_any(StateId(to)),
        };
        b.transitions.push(
            transition
                .with_exit_time((exit_time >= 0.0).then_some(exit_time))
                .with_duration(duration)
                .with_offset(offset),
        );
        index_out(out_index, b.transitions.len() - 1)
    })
}

/// Adds a condition to `transition`; all of them must hold for it to fire.
/// ```
/// use ponko_ffi::PonkoAnimationCondition::PONKO_CONDITION_GREATER;
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_add_condition(std::ptr::null_mut(), 0, PONKO_CONDITION_GREATER, 0, 0.1) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_add_condition(
    builder_ptr: *mut PonkoAnimationGraphBuilder,
    transition: usize,
    kind: PonkoAnimationCondition,
    parameter: usize,
    value: f32,
) -> PonkoStatus {
    boundary(|| {
        let b = builder(builder_ptr)?;
        let t = b
            .transitions
            .get_mut(transition)
            .ok_or_else(|| invalid_owned(format!("no transition {transition}")))?;
        t.conditions.push(match kind {
            PonkoAnimationCondition::PONKO_CONDITION_GREATER => AnimationCondition::Greater(ParamId(parameter), value),
            PonkoAnimationCondition::PONKO_CONDITION_LESS => AnimationCondition::Less(ParamId(parameter), value),
            PonkoAnimationCondition::PONKO_CONDITION_BOOL => AnimationCondition::Bool(ParamId(parameter), value != 0.0),
            PonkoAnimationCondition::PONKO_CONDITION_TRIGGER => AnimationCondition::Trigger(ParamId(parameter)),
        });
        Ok(())
    })
}

/// Checks and finishes the graph, starting in state `initial`, and frees
/// the builder (even on failure). Fails with the reason when a state,
/// parameter or clock is out of range.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_animation_graph_build(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `builder` null or live, not used afterwards; `out_graph` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_build(
    builder_ptr: *mut PonkoAnimationGraphBuilder,
    initial: usize,
    out_graph: *mut *mut PonkoAnimationGraph,
) -> PonkoStatus {
    boundary(|| {
        if builder_ptr.is_null() {
            return Err(invalid("builder is null"));
        }
        // SAFETY: forwarded caller contract; the builder is ours from here.
        let b = unsafe { Box::from_raw(builder_ptr) };
        if out_graph.is_null() {
            return Err(invalid("output is null"));
        }
        let graph = AnimationGraph::new(b.parameters, b.states, b.transitions, StateId(initial))
            .map_err(|e| invalid_owned(e.to_string()))?;
        // SAFETY: checked non-null; the caller passes writable storage.
        unsafe { out_graph.write(Box::into_raw(Box::new(PonkoAnimationGraph(graph)))) };
        Ok(())
    })
}

/// Frees your share of a graph. Animators made from it keep theirs. Null
/// does nothing.
/// ```
/// unsafe { ponko_ffi::ponko_animation_graph_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `graph` null or live; not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_animation_graph_free(graph: *mut PonkoAnimationGraph) {
    if !graph.is_null() {
        // SAFETY: forwarded caller contract.
        drop(unsafe { Box::from_raw(graph) });
    }
}

// ---- animators on entities --------------------------------------------------------

/// Gives `entity` (which has a skeleton) an animator playing `graph` from
/// its initial state (Rust `Animator`). The engine advances it every frame
/// after the frame systems. Setting it again starts over.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_animator(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `graph` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_animator(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    graph: *const PonkoAnimationGraph,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let graph = graph.as_ref().ok_or_else(|| invalid("graph is null"))?;
            let e = entity(c, entity_bits)?;
            if c.world.get::<Skeleton>(e).is_none() {
                return Err(error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no skeleton"));
            }
            c.world.insert(e, Animator::new(graph.0.clone()));
            Ok(())
        })
    })
}

/// Sets float parameter `parameter` of `entity`'s animator.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_float(std::ptr::null_mut(), 0, 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_float(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = animator(c, entity_bits)?;
            a.try_set_float(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Sets bool parameter `parameter` of `entity`'s animator.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_bool(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_bool(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = animator(c, entity_bits)?;
            a.try_set_bool(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Sets (or clears) trigger parameter `parameter` of `entity`'s animator;
/// the transition it fires clears it.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_trigger(std::ptr::null_mut(), 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_trigger(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = animator(c, entity_bits)?;
            a.try_set_trigger(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// The state `entity`'s animator is in (or fading to), and how far through
/// it is (normalized: 1 is one full play). Either output may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_state(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_state(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_state: *mut usize,
    out_normalized_time: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = animator(c, entity_bits)?;
            index_out(out_state, a.state().0)?;
            if !out_normalized_time.is_null() {
                out_normalized_time.write(a.normalized_time());
            }
            Ok(())
        })
    })
}

/// Crossfades `entity`'s animator to `state` over `duration` seconds (0
/// jumps), starting it at normalized time `offset`, on its next update.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_crossfade(std::ptr::null_mut(), 0, 0, 0.2, 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_crossfade(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    state: usize,
    duration: f32,
    offset: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = animator(c, entity_bits)?;
            a.try_crossfade(StateId(state), duration, offset).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Turns the engine's automatic update of `entity`'s animator off (`true`)
/// or back on; while off, pose it with `ponko_ctx_animator_update`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_manual(std::ptr::null_mut(), 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_manual(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    manual: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            animator(c, entity_bits)?.manual = manual;
            Ok(())
        })
    })
}

/// Advances `entity`'s animator by `dt` seconds and poses its skeleton now.
/// `dt` must be finite and not negative (an invalid argument otherwise; the
/// animator is left as it was).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_update(std::ptr::null_mut(), 0, 0.016) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_update(ctx: *mut PonkoCtx, entity_bits: u64, dt: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let mut query = c.world.query::<(&mut Animator, &mut Skeleton)>();
            let (a, s) = query.get(e).ok_or_else(|| {
                error(
                    PonkoStatus::PONKO_NOT_FOUND,
                    "that entity needs an animator and a skeleton",
                )
            })?;
            // `Animator::update` panics on purpose for such a `dt`.
            if !(dt.is_finite() && dt >= 0.) {
                return Err(invalid("dt must be finite and not negative"));
            }
            a.update(dt, s);
            drop(query);
            c.gfx.write_animated_morphs(&mut c.world);
            Ok(())
        })
    })
}

/// Takes the oldest event `entity`'s animator reported, copying its name
/// (NUL-terminated, cut to fit) into `buffer` of `capacity` bytes.
/// `*out_found` is false when there are none left; call it until then.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_next_event(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_found` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_next_event(
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
            let a = animator(c, entity_bits)?;
            let Some(name) = a.next_event() else {
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

// ---- layers and root motion -------------------------------------------------------

/// How an animation layer combines with the layers under it.
/// ```
/// assert_eq!(ponko_ffi::PonkoLayerBlend::PONKO_LAYER_ADDITIVE as u32, 1);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoLayerBlend {
    /// Blends toward the layer's pose by its weight.
    PONKO_LAYER_OVERRIDE,
    /// Adds the layer's difference from its first frame (a lean, a breath).
    PONKO_LAYER_ADDITIVE,
}

/// What an animator does with its root joint's horizontal movement and turning.
/// ```
/// assert_eq!(ponko_ffi::PonkoRootMotion::PONKO_ROOT_MOTION_APPLY as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoRootMotion {
    /// Leave it in the pose (the default).
    PONKO_ROOT_MOTION_OFF,
    /// Take it out of the pose; read it with `ponko_ctx_animator_root_motion`.
    PONKO_ROOT_MOTION_EXTRACT,
    /// Take it out of the pose and move the entity's transform by it.
    PONKO_ROOT_MOTION_APPLY,
}

/// `Animator` panics on purpose for a wrong parameter index or kind; these
/// check first, so a C caller gets `PONKO_INVALID_ARGUMENT` instead.
/// An animator's refusal of a command, as a C-side "invalid argument".
fn graph_error(e: AnimationGraphError) -> Failure {
    invalid_owned(e.to_string())
}

fn layer(c: &mut Ctx, bits: u64, index: usize) -> Result<&mut Animator, Failure> {
    animator(c, bits)?
        .get_layer_mut(LayerId(index))
        .map(|l| &mut *l.animator)
        .ok_or_else(|| invalid("no such animation layer"))
}

/// Adds a layer playing `graph` over `entity`'s animator, with its own
/// parameters and clock, at full weight. `mode` is a `PonkoLayerBlend`.
/// `mask_joint` limits it to that joint and its children (null: every joint).
/// Writes its index (0 for the first layer) to `out_layer`, which may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_add_layer(std::ptr::null_mut(), 0, std::ptr::null(), 0, std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `graph` null or live; `mask_joint` null or a
/// NUL-terminated string; `out_layer` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_add_layer(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    graph: *const PonkoAnimationGraph,
    mode: u32,
    mask_joint: *const c_char,
    out_layer: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let graph = graph.as_ref().ok_or_else(|| invalid("graph is null"))?;
            let mode = match mode {
                0 => LayerBlend::Override,
                1 => LayerBlend::Additive,
                _ => return Err(invalid("unknown layer blend")),
            };
            let mask = if mask_joint.is_null() {
                None
            } else {
                let name = text(mask_joint)?;
                let e = entity(c, entity_bits)?;
                let skeleton = c
                    .world
                    .get::<Skeleton>(e)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no skeleton"))?;
                Some(
                    JointMask::descendants(skeleton, name)
                        .ok_or_else(|| invalid_owned(format!("no joint named {name:?}")))?,
                )
            };
            let mut l = AnimationLayer::new(graph.0.clone());
            l.mode = mode;
            l.mask = mask;
            let index = animator(c, entity_bits)?.add_layer(l);
            if !out_layer.is_null() {
                out_layer.write(index.0);
            }
            Ok(())
        })
    })
}

/// Sets how strongly layer `layer` of `entity`'s animator shows, 0 to 1.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_layer_weight(std::ptr::null_mut(), 0, 0, 0.5) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_layer_weight(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    layer: usize,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if !weight.is_finite() {
                return Err(invalid("layer weight must be finite"));
            }
            animator(c, entity_bits)?
                .get_layer_mut(LayerId(layer))
                .ok_or_else(|| invalid("no such animation layer"))?
                .weight = weight.clamp(0., 1.);
            Ok(())
        })
    })
}

/// Sets float parameter `parameter` of layer `layer`'s own graph.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_layer_set_float(std::ptr::null_mut(), 0, 0, 0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_layer_set_float(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    layer_index: usize,
    parameter: usize,
    value: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = layer(c, entity_bits, layer_index)?;
            a.try_set_float(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Sets bool parameter `parameter` of layer `layer`'s own graph.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_layer_set_bool(std::ptr::null_mut(), 0, 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_layer_set_bool(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    layer_index: usize,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = layer(c, entity_bits, layer_index)?;
            a.try_set_bool(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Sets (or clears) trigger parameter `parameter` of layer `layer`'s own graph.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_layer_set_trigger(std::ptr::null_mut(), 0, 0, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_layer_set_trigger(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    layer_index: usize,
    parameter: usize,
    value: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = layer(c, entity_bits, layer_index)?;
            a.try_set_trigger(ParamId(parameter), value).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Crossfades layer `layer` to `state` of its own graph (like
/// `ponko_ctx_animator_crossfade`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_layer_crossfade(std::ptr::null_mut(), 0, 0, 0, 0.2, 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_layer_crossfade(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    layer_index: usize,
    state: usize,
    duration: f32,
    offset: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let a = layer(c, entity_bits, layer_index)?;
            a.try_crossfade(StateId(state), duration, offset).map_err(graph_error)?;
            Ok(())
        })
    })
}

/// Sets what `entity`'s animator does with joint `root_joint`'s horizontal
/// movement and turning: `mode` is a `PonkoRootMotion`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_set_root_motion(std::ptr::null_mut(), 0, 2, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_set_root_motion(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    mode: u32,
    root_joint: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (extract, apply) = match mode {
                0 => (false, false),
                1 => (true, false),
                2 => (true, true),
                _ => return Err(invalid("unknown root motion mode")),
            };
            let a = animator(c, entity_bits)?;
            (a.extract_root_motion, a.apply_root_motion, a.root_joint) = (extract, apply, root_joint);
            Ok(())
        })
    })
}

/// The root movement (in the entity's own axes, before its scale) and turn
/// that `entity`'s animator took out of the pose in its latest update.
/// Either output may be null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_animator_root_motion(std::ptr::null_mut(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_animator_root_motion(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_position: *mut PonkoVec3,
    out_rotation: *mut PonkoQuat,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let m = animator(c, entity_bits)?.root_motion();
            if !out_position.is_null() {
                out_position.write(PonkoVec3 {
                    x: m.position.x,
                    y: m.position.y,
                    z: m.position.z,
                });
            }
            if !out_rotation.is_null() {
                out_rotation.write(PonkoQuat {
                    x: m.rotation.x,
                    y: m.rotation.y,
                    z: m.rotation.z,
                    w: m.rotation.w,
                });
            }
            Ok(())
        })
    })
}
