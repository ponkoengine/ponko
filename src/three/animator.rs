//! Shared animation graphs and per-entity playback.
use super::{AnimationClip, Skeleton, Transform3D};
use crate::anim::graph::{
    AnimationEvent, AnimationGraphError, AnimationParameter, AnimationTransition, ParamId, StateId,
};
use crate::anim::graph::{
    check_command, check_parameters, check_transitions, consume_triggers, expect_kind, float_parameter,
    select_transition, set_bool, set_float, set_trigger,
};
use glam::{Quat, Vec2, Vec3};
use ponko_ecs::Component;
use std::sync::Arc;

/// An overlay of an [`Animator`], as [`Animator::add_layer`] returns it:
/// overlays are numbered from 0 in the order they were added (the base
/// layer has none).
/// ```
/// let first = ponko::anim::LayerId(0);
/// assert_eq!(first.0, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub usize);

/// A recursive motion. Trees use seconds, with each child scaling its clock.
/// Tree duration is the longest child duration divided by its time scale.
/// 2D uses normalized inverse squared distance; an exact point selects only
/// that child (the first if points coincide). Quaternion blends use ordered
/// incremental slerp, so child order matters for more than two rotations.
/// ```
/// use ponko::prelude::*;
/// let clip = ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![]);
/// let motion = ponko::anim::Motion::Clip(clip);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Motion {
    /// A single clip.
    Clip(AnimationClip),
    /// Sorted, distinct thresholds on a float parameter. Ends clamp.
    /// Build one with [`Motion::blend_1d`]; fields may be added in any
    /// release.
    #[non_exhaustive]
    Blend1D {
        /// Float parameter.
        parameter: ParamId,
        /// Threshold and child pairs, in increasing order.
        children: Vec<(f32, MotionChild)>,
    },
    /// Freeform points on two float parameters. Build one with
    /// [`Motion::blend_2d`]; fields may be added in any release.
    #[non_exhaustive]
    Blend2D {
        /// X float parameter.
        x: ParamId,
        /// Y float parameter.
        y: ParamId,
        /// Point and child pairs.
        children: Vec<(Vec2, MotionChild)>,
    },
}

impl Motion {
    /// A 1D blend tree on float parameter `parameter`: `children` are
    /// threshold and child pairs in strictly increasing threshold order.
    /// [`AnimationGraph::new`] checks the order and the parameter's kind.
    /// ```
    /// use ponko::anim::{AnimationClip, Motion, MotionChild, ParamId, Skeleton};
    /// let s = Skeleton::new(vec![]);
    /// let walk = MotionChild::new(Motion::Clip(AnimationClip::new("walk", &s, vec![])));
    /// let run = MotionChild::new(Motion::Clip(AnimationClip::new("run", &s, vec![])));
    /// let locomotion = Motion::blend_1d(ParamId(0), vec![(1.0, walk), (3.0, run)]);
    /// assert!(matches!(locomotion, Motion::Blend1D { .. }));
    /// ```
    #[must_use]
    pub fn blend_1d(parameter: ParamId, children: Vec<(f32, MotionChild)>) -> Self {
        Self::Blend1D { parameter, children }
    }

    /// A 2D freeform blend tree on float parameters `x` and `y`: `children`
    /// are point and child pairs.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::{AnimationClip, Motion, MotionChild, ParamId, Skeleton};
    /// let s = Skeleton::new(vec![]);
    /// let child = |name: &str| MotionChild::new(Motion::Clip(AnimationClip::new(name, &s, vec![])));
    /// let strafe = Motion::blend_2d(ParamId(0), ParamId(1), vec![(Vec2::ZERO, child("idle")), (Vec2::X, child("right"))]);
    /// assert!(matches!(strafe, Motion::Blend2D { .. }));
    /// ```
    #[must_use]
    pub fn blend_2d(x: ParamId, y: ParamId, children: Vec<(Vec2, MotionChild)>) -> Self {
        Self::Blend2D { x, y, children }
    }
}

/// A motion inside a tree; positive time scale multiplies seconds.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) and the `with_*` methods.
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimationClip, Motion, MotionChild, Skeleton};
/// let idle = AnimationClip::new("idle", &Skeleton::new(vec![]), vec![]);
/// let child = MotionChild::new(Motion::Clip(idle)).with_time_scale(1.5);
/// assert_eq!(child.time_scale, 1.5);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MotionChild {
    /// Nested clip or tree.
    pub motion: Motion,
    /// Positive, finite clock multiplier.
    pub time_scale: f32,
}

impl MotionChild {
    /// `motion` at its recorded speed (time scale 1).
    /// ```
    /// use ponko::anim::{AnimationClip, Motion, MotionChild, Skeleton};
    /// let idle = AnimationClip::new("idle", &Skeleton::new(vec![]), vec![]);
    /// assert_eq!(MotionChild::new(Motion::Clip(idle)).time_scale, 1.0);
    /// ```
    #[must_use]
    pub fn new(motion: Motion) -> Self {
        Self { motion, time_scale: 1. }
    }

    with_fields! {
        /// Sets [`time_scale`](Self::time_scale), the positive clock
        /// multiplier (2 plays the child twice as fast).
        time_scale: f32 => with_time_scale;
    }
}

/// State playback settings. Use [`AnimationState::new`] for looping defaults.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) and the `with_*` methods.
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{AnimationClip, AnimationEvent, AnimationState, Motion, Skeleton};
/// let hit = AnimationClip::new("hit", &Skeleton::new(vec![]), vec![]);
/// let s = AnimationState::new(Motion::Clip(hit))
///     .with_looping(false)
///     .with_speed(1.25)
///     .with_event(AnimationEvent::new(0.4, "impact"));
/// assert!(!s.looping);
/// assert_eq!(s.events.len(), 1);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AnimationState {
    /// Motion to sample.
    pub motion: Motion,
    /// Nonnegative playback multiplier.
    pub speed: f32,
    /// Optional float parameter multiplying speed; negative values stop time.
    pub speed_parameter: Option<ParamId>,
    /// Wrap time when true, otherwise clamp at the motion's end.
    pub looping: bool,
    /// Events, reported in time order (ties retain definition order).
    pub events: Vec<AnimationEvent>,
}
impl AnimationState {
    /// Loop a motion at normal speed, without events.
    /// ```
    /// use ponko::prelude::*;
    /// let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// assert_eq!(s.speed, 1.0);
    /// ```
    #[must_use]
    pub fn new(motion: Motion) -> Self {
        Self {
            motion,
            speed: 1.,
            speed_parameter: None,
            looping: true,
            events: Vec::new(),
        }
    }

    /// Adds an event (see [`events`](Self::events)); call once per event.
    /// ```
    /// use ponko::anim::{AnimationClip, AnimationEvent, AnimationState, Motion, Skeleton};
    /// let walk = AnimationClip::new("walk", &Skeleton::new(vec![]), vec![]);
    /// let s = AnimationState::new(Motion::Clip(walk))
    ///     .with_event(AnimationEvent::new(0.0, "left_foot"))
    ///     .with_event(AnimationEvent::new(0.5, "right_foot"));
    /// assert_eq!(s.events.len(), 2);
    /// ```
    #[must_use]
    pub fn with_event(mut self, event: AnimationEvent) -> Self {
        self.events.push(event);
        self
    }

    with_fields! {
        /// Sets [`speed`](Self::speed), the nonnegative playback multiplier.
        speed: f32 => with_speed;
        /// Sets [`speed_parameter`](Self::speed_parameter), the float
        /// parameter multiplying speed (`None`: none).
        speed_parameter: Option<ParamId> => with_speed_parameter;
        /// Sets [`looping`](Self::looping): wrap time when true, otherwise
        /// hold the motion's last pose.
        looping: bool => with_looping;
        /// Replaces [`events`](Self::events).
        events: Vec<AnimationEvent> => with_events;
    }
}

/// Immutable graph shared by animators with independent parameters and clocks.
/// ```
/// use ponko::prelude::*;
/// let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
/// let graph = ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph");
/// let animator = ponko::anim::Animator::new(graph.clone());
/// ```
#[derive(Debug)]
pub struct AnimationGraph {
    parameters: Vec<AnimationParameter>,
    states: Vec<AnimationState>,
    transitions: Vec<AnimationTransition>,
    initial: usize,
}
impl AnimationGraph {
    /// Checks ids, parameter kinds, finite clocks and nonempty trees, and
    /// returns the shared graph.
    ///
    /// # Errors
    ///
    /// An [`AnimationGraphError`] naming what is wrong with the graph: an
    /// initial state that isn't in it, a parameter, state or transition that
    /// refers to an id or a parameter of the wrong kind, a speed that isn't a
    /// finite, nonnegative number, an event time outside `0..1`, or an empty
    /// blend tree.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::anim::StateId;
    /// let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// let graph = ponko::anim::AnimationGraph::new(vec![], vec![s], vec![], StateId(0)).expect("a valid graph");
    /// ```
    pub fn new(
        parameters: Vec<AnimationParameter>,
        mut states: Vec<AnimationState>,
        transitions: Vec<AnimationTransition>,
        initial: StateId,
    ) -> Result<Arc<Self>, AnimationGraphError> {
        if initial.0 >= states.len() {
            return Err(AnimationGraphError::InvalidInitialState(initial));
        }
        check_parameters(&parameters)?;
        for (i, s) in states.iter_mut().enumerate() {
            let id = StateId(i);
            s.motion.validate(&parameters, id)?;
            if !(s.speed.is_finite() && s.speed >= 0.) {
                return Err(AnimationGraphError::InvalidSpeed(id));
            }
            if let Some(p) = s.speed_parameter {
                expect_kind(&parameters, p, "float")?;
            }
            if !s
                .events
                .iter()
                .all(|e| e.time.is_finite() && (0. ..1.).contains(&e.time))
            {
                return Err(AnimationGraphError::InvalidEventTime(id));
            }
            s.events.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        check_transitions(&parameters, &transitions, states.len())?;
        Ok(Arc::new(Self {
            parameters,
            states,
            transitions,
            initial: initial.0,
        }))
    }
}
impl Motion {
    fn validate(&self, p: &[AnimationParameter], state: StateId) -> Result<(), AnimationGraphError> {
        match self {
            Self::Clip(_) => (),
            Self::Blend1D { parameter, children } => {
                expect_kind(p, *parameter, "float")?;
                if children.is_empty()
                    || !children.iter().all(|(v, _)| v.is_finite())
                    || !children.windows(2).all(|c| c[0].0 < c[1].0)
                {
                    return Err(AnimationGraphError::InvalidThresholds(state));
                }
                for (_, c) in children {
                    c.validate(p, state)?;
                }
            }
            Self::Blend2D { x, y, children } => {
                expect_kind(p, *x, "float")?;
                expect_kind(p, *y, "float")?;
                if children.is_empty() || !children.iter().all(|(v, _)| v.is_finite()) {
                    return Err(AnimationGraphError::InvalidPoints(state));
                }
                for (_, c) in children {
                    c.validate(p, state)?;
                }
            }
        }
        Ok(())
    }
    fn duration(&self) -> f32 {
        match self {
            Self::Clip(c) => c.duration(),
            Self::Blend1D { children, .. } => children
                .iter()
                .map(|(_, c)| c.motion.duration() / c.time_scale)
                .fold(0., f32::max),
            Self::Blend2D { children, .. } => children
                .iter()
                .map(|(_, c)| c.motion.duration() / c.time_scale)
                .fold(0., f32::max),
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &self,
        time: f64,
        looping: bool,
        weight: f32,
        p: &[AnimationParameter],
        output: &mut Skeleton,
        scratch: &mut Skeleton,
        total: &mut f32,
    ) {
        if weight <= 0. {
            return;
        }
        match self {
            Self::Clip(c) => {
                scratch.reset_pose();
                // Within one loop before narrowing: the clock counts whole
                // loops in f64, so the pose stays exact however long it runs.
                let t = if looping && c.duration() > 0. {
                    time.rem_euclid(f64::from(c.duration()))
                } else {
                    time
                };
                c.sample(t as f32, scratch);
                *total += weight;
                mix(&mut output.pose, &scratch.pose, weight / *total);
                output.morphs.mix(&scratch.morphs, weight / *total);
            }
            Self::Blend1D { parameter, children } => {
                let v = float_parameter(p, *parameter);
                let hi = children.partition_point(|(t, _)| *t <= v);
                if hi == 0 {
                    children[0].1.sample(time, looping, weight, p, output, scratch, total);
                } else if hi == children.len() {
                    children[hi - 1]
                        .1
                        .sample(time, looping, weight, p, output, scratch, total);
                } else {
                    let f = (v - children[hi - 1].0) / (children[hi].0 - children[hi - 1].0);
                    children[hi - 1]
                        .1
                        .sample(time, looping, weight * (1. - f), p, output, scratch, total);
                    children[hi]
                        .1
                        .sample(time, looping, weight * f, p, output, scratch, total);
                }
            }
            Self::Blend2D { x, y, children } => {
                let v = Vec2::new(float_parameter(p, *x), float_parameter(p, *y));
                if let Some((_, c)) = children.iter().find(|(point, _)| *point == v) {
                    c.sample(time, looping, weight, p, output, scratch, total);
                } else {
                    // f64 avoids overflow near points and at large finite parameters.
                    let distance = |point: Vec2| {
                        let dx = f64::from(point.x) - f64::from(v.x);
                        let dy = f64::from(point.y) - f64::from(v.y);
                        dx * dx + dy * dy
                    };
                    let sum: f64 = children.iter().map(|(point, _)| 1. / distance(*point)).sum();
                    for (point, c) in children {
                        c.sample(
                            time,
                            looping,
                            weight * ((1. / distance(*point)) / sum) as f32,
                            p,
                            output,
                            scratch,
                            total,
                        );
                    }
                }
            }
        }
    }
}
impl Motion {
    /// Horizontal root translation and yaw between two unwrapped clock times,
    /// counting whole loops, blended with the same weights as the pose.
    pub(crate) fn root_delta(
        &self,
        from: f64,
        to: f64,
        looping: bool,
        p: &[AnimationParameter],
        skeleton: &Skeleton,
        root: usize,
    ) -> (Vec3, f32) {
        let mut sum = (Vec3::ZERO, 0.);
        self.add_root_delta(from, to, looping, 1., p, skeleton, root, &mut sum);
        sum
    }
    #[allow(clippy::too_many_arguments)]
    fn add_root_delta(
        &self,
        from: f64,
        to: f64,
        looping: bool,
        weight: f32,
        p: &[AnimationParameter],
        skeleton: &Skeleton,
        root: usize,
        sum: &mut (Vec3, f32),
    ) {
        if weight <= 0. {
            return;
        }
        let child = |c: &MotionChild, w: f32, sum: &mut (Vec3, f32)| {
            c.motion.add_root_delta(
                from * f64::from(c.time_scale),
                to * f64::from(c.time_scale),
                looping,
                w,
                p,
                skeleton,
                root,
                sum,
            );
        };
        match self {
            Self::Clip(c) => {
                let at = |t: f32| {
                    let pose = c.sample_joint(t, skeleton, root);
                    (pose.position * Vec3::new(1., 0., 1.), yaw(pose.rotation))
                };
                let d = c.duration();
                let between = |a: f32, b: f32| {
                    let (pa, ya) = at(a);
                    let (pb, yb) = at(b);
                    (pb - pa, angle_delta(ya, yb))
                };
                let (position, angle) = if looping && d > 0. {
                    let d64 = f64::from(d);
                    let loops = ((to / d64).floor() - (from / d64).floor()) as f32;
                    let (lp, ly) = between(0., d);
                    let (pp, py) = between(from.rem_euclid(d64) as f32, to.rem_euclid(d64) as f32);
                    (pp + lp * loops, py + ly * loops)
                } else {
                    between(from as f32, to as f32)
                };
                sum.0 += position * weight;
                sum.1 += angle * weight;
            }
            Self::Blend1D { parameter, children } => {
                let v = float_parameter(p, *parameter);
                let hi = children.partition_point(|(t, _)| *t <= v);
                if hi == 0 {
                    child(&children[0].1, weight, sum);
                } else if hi == children.len() {
                    child(&children[hi - 1].1, weight, sum);
                } else {
                    let f = (v - children[hi - 1].0) / (children[hi].0 - children[hi - 1].0);
                    child(&children[hi - 1].1, weight * (1. - f), sum);
                    child(&children[hi].1, weight * f, sum);
                }
            }
            Self::Blend2D { x, y, children } => {
                let v = Vec2::new(float_parameter(p, *x), float_parameter(p, *y));
                if let Some((_, c)) = children.iter().find(|(point, _)| *point == v) {
                    child(c, weight, sum);
                } else {
                    let distance = |point: Vec2| {
                        let dx = f64::from(point.x) - f64::from(v.x);
                        let dy = f64::from(point.y) - f64::from(v.y);
                        dx * dx + dy * dy
                    };
                    let total: f64 = children.iter().map(|(point, _)| 1. / distance(*point)).sum();
                    for (point, c) in children {
                        child(c, weight * ((1. / distance(*point)) / total) as f32, sum);
                    }
                }
            }
        }
    }
}
impl MotionChild {
    fn validate(&self, p: &[AnimationParameter], state: StateId) -> Result<(), AnimationGraphError> {
        if !(self.time_scale.is_finite() && self.time_scale > 0.) {
            return Err(AnimationGraphError::InvalidTimeScale(state));
        }
        self.motion.validate(p, state)
    }
    #[allow(clippy::too_many_arguments)]
    fn sample(
        &self,
        time: f64,
        looping: bool,
        weight: f32,
        p: &[AnimationParameter],
        output: &mut Skeleton,
        scratch: &mut Skeleton,
        total: &mut f32,
    ) {
        self.motion.sample(
            time * f64::from(self.time_scale),
            looping,
            weight,
            p,
            output,
            scratch,
            total,
        );
    }
}
fn mix(output: &mut [Transform3D], source: &[Transform3D], weight: f32) {
    for (a, b) in output.iter_mut().zip(source) {
        a.position = a.position.lerp(b.position, weight);
        a.scale = a.scale.lerp(b.scale, weight);
        a.rotation = a.rotation.slerp(b.rotation, weight);
    }
}
#[derive(Debug, Clone)]
struct Fade {
    source: Option<(usize, f64)>,
    elapsed: f32,
    duration: f32,
}

/// How a layer combines with the lower pose.
/// ```
/// use ponko::anim::LayerBlend;
/// let mode = ponko::anim::LayerBlend::Additive;
/// assert_ne!(mode, ponko::anim::LayerBlend::Override);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LayerBlend {
    /// Interpolate toward the layer pose.
    Override,
    /// Add translation/scale differences and postmultiply the reference-relative rotation.
    Additive,
}

/// Joint indices selected for one skeleton layout. Build once, reuse each frame.
/// ```
/// use ponko::prelude::*;
/// let mask = ponko::anim::JointMask::new(vec![0, 2]);
/// assert!(mask.contains(2));
/// ```
#[derive(Debug, Clone)]
pub struct JointMask(Vec<usize>);
impl JointMask {
    /// Select explicit joint indices (duplicates are removed).
    /// ```
    /// use ponko::anim::JointMask;
    /// assert!(ponko::anim::JointMask::new(vec![3, 3]).contains(3));
    /// ```
    pub fn new(mut joints: Vec<usize>) -> Self {
        joints.sort_unstable();
        joints.dedup();
        Self(joints)
    }
    /// Select the named joint and every descendant; None if the name is absent.
    /// ```
    /// use ponko::prelude::*;
    /// assert!(ponko::anim::JointMask::descendants(&ponko::anim::Skeleton::new(vec![]), "arm").is_none());
    /// ```
    pub fn descendants(skeleton: &Skeleton, name: &str) -> Option<Self> {
        let root = skeleton.find(name)?;
        let mut joints = vec![root];
        for (i, joint) in skeleton.joints().iter().enumerate().skip(root + 1) {
            if joint.parent.is_some_and(|p| joints.binary_search(&p).is_ok()) {
                joints.push(i);
            }
        }
        Some(Self(joints))
    }
    /// Whether the joint index is selected.
    /// ```
    /// use ponko::anim::JointMask;
    /// assert!(!ponko::anim::JointMask::new(vec![1]).contains(0));
    /// ```
    pub fn contains(&self, joint: usize) -> bool {
        self.0.binary_search(&joint).is_ok()
    }
}

/// An overlay with its own graph, parameters, events and clock. Layers run even
/// at weight zero. Configure parameters/commands through `animator`; base-layer
/// commands remain on the parent Animator. Nested layers are supported.
/// ```
/// use ponko::prelude::*;
/// let s = ponko::anim::Skeleton::new(vec![]);
/// let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
/// let mut layer = ponko::anim::AnimationLayer::new(ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
/// layer.weight = 0.5;
/// layer.mode = ponko::anim::LayerBlend::Additive;
/// ```
#[derive(Debug, Clone)]
pub struct AnimationLayer {
    /// Independent playback; its root-motion settings do not move the parent entity.
    pub animator: Box<Animator>,
    /// Runtime weight, clamped to [0, 1]; NaN means zero.
    pub weight: f32,
    /// Override or reference-relative additive blending.
    pub mode: LayerBlend,
    /// None selects every joint. Rebuild index masks when changing skeleton layouts.
    pub mask: Option<JointMask>,
    /// Chosen additive reference; None samples the destination motion at time zero
    /// with current parameters. Missing reference joints use the skeleton rest pose.
    pub reference_pose: Option<Vec<Transform3D>>,
    buffers: Option<(Skeleton, Skeleton, Skeleton)>,
}
impl AnimationLayer {
    /// Make a full-weight override layer, without a mask or explicit reference.
    /// ```
    /// use ponko::prelude::*;
    /// let s = ponko::anim::Skeleton::new(vec![]);
    /// let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// let layer = ponko::anim::AnimationLayer::new(ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(layer.weight, 1.0);
    /// ```
    pub fn new(graph: Arc<AnimationGraph>) -> Self {
        Self {
            animator: Box::new(Animator::new(graph)),
            weight: 1.,
            mode: LayerBlend::Override,
            mask: None,
            reference_pose: None,
            buffers: None,
        }
    }
    fn apply(&mut self, dt: f32, skeleton: &mut Skeleton) {
        if self
            .buffers
            .as_ref()
            .is_none_or(|(s, _, _)| !s.shares_joints(skeleton.shared_joints()))
        {
            self.buffers = Some((skeleton.clone(), skeleton.clone(), skeleton.clone()));
        }
        let (pose, reference, scratch) = self.buffers.as_mut().unwrap();
        self.animator.update(dt, pose);
        let w = if self.weight.is_nan() {
            0.
        } else {
            self.weight.clamp(0., 1.)
        };
        if w == 0. {
            return;
        }
        if self.mode == LayerBlend::Additive && self.reference_pose.is_none() {
            reference.reset_pose();
            self.animator.graph.states[self.animator.state].motion.sample(
                0.,
                false,
                1.,
                &self.animator.parameters,
                reference,
                scratch,
                &mut 0.,
            );
        }
        skeleton
            .morphs
            .layer(&pose.morphs, w, self.mode == LayerBlend::Additive);
        let joints = skeleton.shared_joints().clone();
        for (i, (a, b)) in skeleton.pose.iter_mut().zip(&pose.pose).enumerate() {
            if self.mask.as_ref().is_some_and(|m| !m.contains(i)) {
                continue;
            }
            match self.mode {
                LayerBlend::Override => {
                    a.position = a.position.lerp(b.position, w);
                    a.rotation = a.rotation.slerp(b.rotation, w);
                    a.scale = a.scale.lerp(b.scale, w);
                }
                LayerBlend::Additive => {
                    let r = self
                        .reference_pose
                        .as_ref()
                        .map_or_else(|| reference.pose.get(i), |p| p.get(i))
                        .unwrap_or(&joints[i].rest);
                    a.position += (b.position - r.position) * w;
                    a.scale += (b.scale - r.scale) * w;
                    a.rotation = (a.rotation * Quat::IDENTITY.slerp(r.rotation.inverse() * b.rotation, w)).normalize();
                }
            }
        }
    }
}

/// Horizontal translation in skeleton-local axes and yaw for the latest update.
/// Vertical motion and pitch/roll stay in the pose. Apply translation using the
/// entity's pre-update orientation, then postmultiply its rotation.
/// ```
/// use ponko::prelude::*;
/// let delta = ponko::anim::RootMotion::default();
/// assert_eq!(delta.position, Vec3::ZERO);
/// assert_eq!(delta.rotation, Quat::IDENTITY);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RootMotion {
    /// Local horizontal displacement, before entity scale.
    pub position: Vec3,
    /// Yaw quaternion delta.
    pub rotation: Quat,
}
impl Default for RootMotion {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        }
    }
}
fn yaw(q: Quat) -> f32 {
    let forward = q * Vec3::Z;
    forward.x.atan2(forward.z)
}
fn angle_delta(a: f32, b: f32) -> f32 {
    (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Animation component. The engine updates it every frame after the frame
/// systems and before the draw systems: frame systems see the previous
/// frame's pose, draw systems and the renderer this frame's. `run_headless`
/// has no frames, so there (or to pose it at another point) set `manual` and
/// call [`update`](Self::update) yourself, e.g. from a sim system.
/// Buffers allocate on first use or skeleton changes, never in steady state.
/// Interruptions freeze the last displayed mixed pose as the new fade source;
/// ordinary fades continue advancing the outgoing state. Only the destination
/// emits events and tests transitions. Events accumulate until drained, up to
/// 64 entries; excess events are dropped, retaining the earliest events.
/// ```
/// use ponko::prelude::*;
/// let mut s = ponko::anim::Skeleton::new(vec![]);
/// let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
/// let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
/// a.manual = true;
/// a.update(0.016, &mut s);
/// ```
#[derive(Debug, Clone, Component)]
pub struct Animator {
    graph: Arc<AnimationGraph>,
    parameters: Vec<AnimationParameter>,
    state: usize,
    /// Seconds into the state, unwrapped (whole loops included). f64: an
    /// f32 counting up would step in milliseconds after hours in one
    /// looping state, and the animation would run slow and judder.
    time: f64,
    fade: Option<Fade>,
    pending: Option<(usize, f32, f32)>,
    buffers: Option<(Skeleton, Skeleton, Skeleton)>,
    events: Vec<Arc<str>>,
    layers: Vec<AnimationLayer>,
    root_delta: RootMotion,
    /// Extract base-layer horizontal translation and yaw instead of posing them.
    pub extract_root_motion: bool,
    /// Root joint index, default zero; invalid indices produce an identity delta.
    /// Extraction uses this joint's local axes, so normally choose a hierarchy root.
    pub root_joint: usize,
    /// Apply extracted motion to the entity Transform3D during automatic updates.
    /// Implies extraction, even when extract_root_motion is false.
    pub apply_root_motion: bool,
    /// Skip automatic engine updates; manual calls still work.
    pub manual: bool,
}
impl Animator {
    /// Start at the graph's initial state with its default parameters.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![], vec![s], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.state(), ponko::anim::StateId(0));
    /// ```
    pub fn new(graph: Arc<AnimationGraph>) -> Self {
        Self {
            parameters: graph.parameters.clone(),
            state: graph.initial,
            graph,
            time: 0.,
            fade: None,
            pending: None,
            buffers: None,
            events: Vec::with_capacity(64),
            layers: Vec::new(),
            root_delta: RootMotion::default(),
            extract_root_motion: false,
            root_joint: 0,
            apply_root_motion: false,
            manual: false,
        }
    }
    /// Append an overlay, returning its zero-based overlay index (base excluded).
    /// ```
    /// use ponko::prelude::*;
    /// let s = ponko::anim::Skeleton::new(vec![]);
    /// let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// let g = ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph");
    /// let mut a = ponko::anim::Animator::new(g.clone());
    /// assert_eq!(a.add_layer(ponko::anim::AnimationLayer::new(g)), ponko::anim::LayerId(0));
    /// ```
    pub fn add_layer(&mut self, layer: AnimationLayer) -> LayerId {
        let index = self.layers.len();
        self.layers.push(layer);
        LayerId(index)
    }
    /// Edit an overlay by index. For an index from outside (a save file, a
    /// script, a C caller) use [`get_layer_mut`](Self::get_layer_mut).
    ///
    /// # Panics
    ///
    /// For an index this animator didn't return from
    /// [`add_layer`](Self::add_layer).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// # let s = ponko::anim::Skeleton::new(vec![]);
    /// # let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// # let g = ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph");
    /// # let mut a = ponko::anim::Animator::new(g.clone());
    /// let index = a.add_layer(ponko::anim::AnimationLayer::new(g));
    /// a.layer_mut(index).weight = 0.25;
    /// ```
    pub fn layer_mut(&mut self, layer: LayerId) -> &mut AnimationLayer {
        self.get_layer_mut(layer).unwrap_or_else(|| {
            panic!(
                "Animator::layer_mut: no layer {}; add a layer first or use get_layer_mut",
                layer.0
            )
        })
    }
    /// Edit an overlay by index, or `None` when there is no such overlay.
    /// ```
    /// use ponko::prelude::*;
    /// # let s = ponko::anim::Skeleton::new(vec![]);
    /// # let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// # let g = ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph");
    /// # let mut a = ponko::anim::Animator::new(g.clone());
    /// let index = a.add_layer(ponko::anim::AnimationLayer::new(g));
    /// a.get_layer_mut(index).expect("just added").weight = 0.25;
    /// assert!(a.get_layer_mut(ponko::anim::LayerId(7)).is_none());
    /// ```
    pub fn get_layer_mut(&mut self, layer: LayerId) -> Option<&mut AnimationLayer> {
        self.layers.get_mut(layer.0)
    }
    /// Number of overlays added with [`add_layer`](Self::add_layer).
    /// ```
    /// use ponko::prelude::*;
    /// # let s = ponko::anim::Skeleton::new(vec![]);
    /// # let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.layer_count(), 0);
    /// ```
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }
    /// Latest update's base-layer motion; identity when extraction is disabled.
    /// Reading does not consume it. play/offset changes never add displacement.
    /// Ordinary crossfades blend deltas using the end-of-update fade weight;
    /// interrupted fades blend from zero velocity (their pose source is frozen).
    /// ```
    /// use ponko::prelude::*;
    /// # let s = ponko::anim::Skeleton::new(vec![]);
    /// # let state = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &s, vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![], vec![state], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.extract_root_motion = true;
    /// assert_eq!(a.root_motion().position, Vec3::ZERO);
    /// ```
    pub fn root_motion(&self) -> RootMotion {
        self.root_delta
    }
    /// The current value of a parameter by index, or `None` for an invalid
    /// index. Lets callers check an index and kind before a setter that panics.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Bool(true)], vec![s], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert!(matches!(a.parameter(ponko::anim::ParamId(0)), Some(ponko::anim::AnimationParameter::Bool(true))));
    /// assert!(a.parameter(ponko::anim::ParamId(1)).is_none());
    /// ```
    pub fn parameter(&self, parameter: ParamId) -> Option<AnimationParameter> {
        self.parameters.get(parameter.0).copied()
    }
    /// Number of states in the graph, so a state index can be checked before
    /// [`crossfade`](Self::crossfade) or [`play`](Self::play) panic on it.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![], vec![s], vec![], ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.state_count(), 1);
    /// ```
    pub fn state_count(&self) -> usize {
        self.graph.states.len()
    }
    /// Set a float by index. Use it with ids from your own graph, and
    /// [`try_set_float`](Self::try_set_float) for ids or values from outside.
    ///
    /// # Panics
    ///
    /// On an index that isn't a float parameter of the graph, or a value that
    /// is not finite.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Float(0.)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.set_float(ponko::anim::ParamId(0), 1.5);
    /// ```
    pub fn set_float(&mut self, parameter: ParamId, value: f32) {
        self.try_set_float(parameter, value)
            .unwrap_or_else(|e| panic!("Animator::set_float: {e}"));
    }
    /// Set a float by index, or say why not. Nothing changes on an error.
    ///
    /// # Errors
    ///
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for an
    /// index that isn't a float parameter (or isn't in the graph),
    /// [`NonFiniteValue`](AnimationGraphError::NonFiniteValue) for NaN or
    /// infinity.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Float(0.)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// use ponko::anim::{AnimationGraphError, ParamId};
    /// assert_eq!(a.try_set_float(ParamId(0), 1.5), Ok(()));
    /// assert_eq!(a.try_set_float(ParamId(0), f32::NAN), Err(AnimationGraphError::NonFiniteValue(ParamId(0))));
    /// assert!(a.try_set_float(ParamId(9), 1.0).is_err());
    /// ```
    pub fn try_set_float(&mut self, parameter: ParamId, value: f32) -> Result<(), AnimationGraphError> {
        set_float(&mut self.parameters, parameter, value)
    }
    /// Set a persistent bool by index (use
    /// [`try_set_bool`](Self::try_set_bool) for ids from outside).
    ///
    /// # Panics
    ///
    /// On an index that isn't a bool parameter of the graph.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Bool(false)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.set_bool(ponko::anim::ParamId(0), true);
    /// ```
    pub fn set_bool(&mut self, parameter: ParamId, value: bool) {
        self.try_set_bool(parameter, value)
            .unwrap_or_else(|e| panic!("Animator::set_bool: {e}"));
    }
    /// Set a persistent bool by index, or say why not.
    ///
    /// # Errors
    ///
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for an
    /// index that isn't a bool parameter (or isn't in the graph).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Bool(false)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.try_set_bool(ponko::anim::ParamId(0), true), Ok(()));
    /// assert!(a.try_set_bool(ponko::anim::ParamId(1), true).is_err());
    /// ```
    pub fn try_set_bool(&mut self, parameter: ParamId, value: bool) -> Result<(), AnimationGraphError> {
        set_bool(&mut self.parameters, parameter, value)
    }
    /// Set or reset a trigger by index (use
    /// [`try_set_trigger`](Self::try_set_trigger) for ids from outside).
    ///
    /// # Panics
    ///
    /// On an index that isn't a trigger parameter of the graph.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Trigger(false)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.set_trigger(ponko::anim::ParamId(0), true);
    /// ```
    pub fn set_trigger(&mut self, parameter: ParamId, value: bool) {
        self.try_set_trigger(parameter, value)
            .unwrap_or_else(|e| panic!("Animator::set_trigger: {e}"));
    }
    /// Set or reset a trigger by index, or say why not.
    ///
    /// # Errors
    ///
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for an
    /// index that isn't a trigger parameter (or isn't in the graph).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![ponko::anim::AnimationParameter::Trigger(false)],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.try_set_trigger(ponko::anim::ParamId(0), true), Ok(()));
    /// assert!(a.try_set_trigger(ponko::anim::ParamId(1), true).is_err());
    /// ```
    pub fn try_set_trigger(&mut self, parameter: ParamId, value: bool) -> Result<(), AnimationGraphError> {
        set_trigger(&mut self.parameters, parameter, value)
    }
    /// Destination state index, including while crossfading.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.state(), ponko::anim::StateId(0));
    /// ```
    pub fn state(&self) -> StateId {
        StateId(self.state)
    }
    /// Unwrapped normalized destination clock (clamped for non-looping states).
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.normalized_time(), 0.0);
    /// ```
    ///
    /// An `f32`: after hours in one looping state its fraction steps
    /// coarsely (a 1 s loop reads in steps of 1/256 after ten hours).
    /// The animator itself keeps time in `f64`, so poses, events, exit
    /// times and root motion stay exact; [`normalized_time_f64`](Self::normalized_time_f64)
    /// reads the same clock in full.
    pub fn normalized_time(&self) -> f32 {
        self.normalized_time_f64() as f32
    }
    /// [`normalized_time`](Self::normalized_time) in full precision: whole
    /// loops plus the fraction of the current one, exact after any length
    /// of play.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// let loops = a.normalized_time_f64().floor();
    /// assert_eq!(loops, 0.0);
    /// ```
    pub fn normalized_time_f64(&self) -> f64 {
        let d = self.graph.states[self.state].motion.duration();
        if d > 0. { self.time / f64::from(d) } else { 0. }
    }
    /// Destination weight; one when no crossfade is active.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// assert_eq!(a.fade_weight(), 1.0);
    /// ```
    pub fn fade_weight(&self) -> f32 {
        self.fade.as_ref().map_or(1., |f| (f.elapsed / f.duration).min(1.))
    }
    /// Queue a fixed-time fade; applied on the next update, before advancing.
    /// Multiple commands before an update use the last command. Use
    /// [`try_crossfade`](Self::try_crossfade) for states from outside.
    ///
    /// # Panics
    ///
    /// For a state that isn't in the graph, or a negative or non-finite
    /// duration or normalized offset.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.crossfade(ponko::anim::StateId(0), 0.12, 0.0);
    /// ```
    pub fn crossfade(&mut self, state: StateId, duration: f32, normalized_offset: f32) {
        self.try_crossfade(state, duration, normalized_offset)
            .unwrap_or_else(|e| panic!("Animator: invalid playback command: {e}"));
    }
    /// Queue a fade like [`crossfade`](Self::crossfade), or say why not.
    /// Nothing is queued on an error.
    ///
    /// # Errors
    ///
    /// [`UnknownState`](AnimationGraphError::UnknownState) for a state not in
    /// the graph, [`InvalidPlayback`](AnimationGraphError::InvalidPlayback)
    /// for a negative or non-finite duration or offset.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// use ponko::anim::{AnimationGraphError, StateId};
    /// assert_eq!(a.try_crossfade(StateId(0), 0.2, 0.0), Ok(()));
    /// assert_eq!(a.try_crossfade(StateId(4), 0.2, 0.0), Err(AnimationGraphError::UnknownState(StateId(4))));
    /// ```
    pub fn try_crossfade(
        &mut self,
        state: StateId,
        duration: f32,
        normalized_offset: f32,
    ) -> Result<(), AnimationGraphError> {
        check_command(self.graph.states.len(), state, duration, normalized_offset)?;
        self.pending = Some((state.0, duration, normalized_offset));
        Ok(())
    }
    /// Queue an immediate state change at a normalized time; applied on update.
    /// Use [`try_play`](Self::try_play) for states from outside.
    /// The pose snaps: with TAA or temporal AO on, call
    /// [`Graphics::reset_motion`](crate::gfx::Graphics::reset_motion) for the
    /// posed model on that frame, so its jump is not read as motion.
    ///
    /// # Panics
    ///
    /// For a state that isn't in the graph, or a negative or non-finite time.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.play(ponko::anim::StateId(0), 0.5);
    /// ```
    pub fn play(&mut self, state: StateId, normalized_time: f32) {
        self.crossfade(state, 0., normalized_time);
    }
    /// Queue an immediate state change like [`play`](Self::play), or say why
    /// not. Nothing is queued on an error.
    ///
    /// # Errors
    ///
    /// [`UnknownState`](AnimationGraphError::UnknownState) for a state
    /// not in the graph,
    /// [`InvalidPlayback`](AnimationGraphError::InvalidPlayback) for a
    /// negative or non-finite time.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// use ponko::anim::{AnimationGraphError, StateId};
    /// assert_eq!(a.try_play(StateId(0), 0.5), Ok(()));
    /// assert_eq!(a.try_play(StateId(0), -1.0), Err(AnimationGraphError::InvalidPlayback(StateId(0))));
    /// ```
    pub fn try_play(&mut self, state: StateId, normalized_time: f32) -> Result<(), AnimationGraphError> {
        self.try_crossfade(state, 0., normalized_time)
    }
    /// Takes the oldest unread event, if any (for reading events one at a
    /// time, as the C API does; [`drain_events`](Self::drain_events) takes all).
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// while let Some(name) = a.next_event() { println!("{name}"); }
    /// ```
    pub fn next_event(&mut self) -> Option<Arc<str>> {
        (!self.events.is_empty()).then(|| self.events.remove(0))
    }
    /// Drain retained event names, preserving the buffer's allocation.
    /// ```
    /// # use ponko::prelude::*;
    /// # let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &ponko::anim::Skeleton::new(vec![]), vec![])));
    /// # let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// for name in a.drain_events() { println!("{name}"); }
    /// ```
    pub fn drain_events(&mut self) -> impl Iterator<Item = Arc<str>> + '_ {
        self.events.drain(..)
    }
    fn advance(&self, state: usize, time: f64, dt: f32) -> f64 {
        let s = &self.graph.states[state];
        let speed = s.speed
            * s.speed_parameter
                .map_or(1., |p| float_parameter(&self.parameters, p).max(0.));
        let next = (time + f64::from(dt * speed)).min(f64::from(f32::MAX));
        if s.looping {
            next
        } else {
            next.min(f64::from(s.motion.duration()))
        }
    }
    /// Advance by nonnegative finite seconds and sample into the skeleton.
    /// Calls with zero dt apply queued commands and
    /// transitions but emit no events. Warm up once with each skeleton layout
    /// before allocation-sensitive gameplay (clip name binding also warms up).
    ///
    /// # Panics
    ///
    /// If `dt` is negative or not finite.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut skeleton = ponko::anim::Skeleton::new(vec![]);
    /// let s = ponko::anim::AnimationState::new(ponko::anim::Motion::Clip(ponko::anim::AnimationClip::new("idle", &skeleton, vec![])));
    /// let mut a = ponko::anim::Animator::new(ponko::anim::AnimationGraph::new(vec![],vec![s],vec![],ponko::anim::StateId(0)).expect("a valid graph"));
    /// a.update(0.016, &mut skeleton);
    /// ```
    pub fn update(&mut self, dt: f32, skeleton: &mut Skeleton) {
        self.update_base(dt, skeleton);
        for layer in &mut self.layers {
            layer.apply(dt, skeleton);
        }
        if self.extract_root_motion || self.apply_root_motion {
            let rest = skeleton.joints().get(self.root_joint).map(|j| j.rest);
            if let (Some(rest), Some(pose)) = (rest, skeleton.pose.get_mut(self.root_joint)) {
                pose.position.x = rest.position.x;
                pose.position.z = rest.position.z;
                pose.rotation = Quat::from_rotation_y(yaw(rest.rotation) - yaw(pose.rotation)) * pose.rotation;
            }
        }
    }
    fn update_base(&mut self, dt: f32, skeleton: &mut Skeleton) {
        assert!(
            dt.is_finite() && dt >= 0.,
            "Animator: dt must be finite and nonnegative"
        );
        if self
            .buffers
            .as_ref()
            .is_none_or(|(s, _, _)| !s.shares_joints(skeleton.shared_joints()))
        {
            self.buffers = Some((skeleton.clone(), skeleton.clone(), skeleton.clone()));
        }
        let command = self.pending.take();
        let selected = if command.is_none() {
            select_transition(&self.graph.transitions, &self.parameters, self.state, |exit| {
                let d = self.graph.states[self.state].motion.duration();
                d > 0. && self.advance(self.state, self.time, dt) / f64::from(d) >= f64::from(exit)
            })
        } else {
            None
        };
        let command = command.or_else(|| {
            selected.map(|i| {
                let t = &self.graph.transitions[i];
                consume_triggers(&mut self.parameters, t);
                (t.to.0, t.duration, t.offset)
            })
        });
        if let Some((state, duration, offset)) = command {
            let source = if self.fade.is_some() {
                None
            } else {
                Some((self.state, self.time))
            };
            let (_, _, snapshot) = self.buffers.as_mut().unwrap();
            snapshot.pose.clone_from(&skeleton.pose);
            snapshot.morphs.clone_from(&skeleton.morphs);
            self.fade = if duration > 0. {
                Some(Fade {
                    source,
                    elapsed: 0.,
                    duration,
                })
            } else {
                None
            };
            self.state = state;
            let s = &self.graph.states[state];
            self.time = f64::from(offset * s.motion.duration());
            if !s.looping {
                self.time = self.time.min(f64::from(s.motion.duration()));
            }
        }
        let old = self.time;
        self.time = self.advance(self.state, self.time, dt);
        self.collect_events(old, self.time);
        self.root_delta = RootMotion::default();
        if (self.extract_root_motion || self.apply_root_motion) && self.root_joint < skeleton.joints().len() {
            let state = &self.graph.states[self.state];
            let mut delta = state.motion.root_delta(
                old,
                self.time,
                state.looping,
                &self.parameters,
                skeleton,
                self.root_joint,
            );
            if let Some(fade) = &self.fade {
                let w = ((fade.elapsed + dt) / fade.duration).min(1.);
                let source = fade.source.map_or((Vec3::ZERO, 0.), |(i, time)| {
                    let s = &self.graph.states[i];
                    s.motion.root_delta(
                        time,
                        self.advance(i, time, dt),
                        s.looping,
                        &self.parameters,
                        skeleton,
                        self.root_joint,
                    )
                });
                delta = (source.0.lerp(delta.0, w), source.1 + (delta.1 - source.1) * w);
            }
            self.root_delta = RootMotion {
                position: delta.0,
                rotation: Quat::from_rotation_y(delta.1),
            };
        }
        if let Some(mut fade) = self.fade.take() {
            fade.elapsed = (fade.elapsed + dt).min(fade.duration);
            if let Some((state, time)) = fade.source {
                fade.source = Some((state, self.advance(state, time, dt)));
            }
            if fade.elapsed < fade.duration {
                self.fade = Some(fade);
            }
        }
        let (output, scratch, snapshot) = self.buffers.as_mut().unwrap();
        let state = &self.graph.states[self.state];
        output.reset_pose();
        state
            .motion
            .sample(self.time, state.looping, 1., &self.parameters, output, scratch, &mut 0.);
        if let Some(f) = &self.fade {
            if let Some((i, time)) = f.source {
                let s = &self.graph.states[i];
                snapshot.reset_pose();
                s.motion
                    .sample(time, s.looping, 1., &self.parameters, snapshot, scratch, &mut 0.);
            }
            mix(&mut output.pose, &snapshot.pose, 1. - f.elapsed / f.duration);
            output.morphs.mix(&snapshot.morphs, 1. - f.elapsed / f.duration);
        }
        skeleton.pose.clone_from(&output.pose);
        skeleton.morphs.clone_from(&output.morphs);
    }
    fn collect_events(&mut self, old: f64, new: f64) {
        let state = &self.graph.states[self.state];
        let d = state.motion.duration();
        if d <= 0. || new <= old || state.events.is_empty() {
            return;
        }
        let start = old / f64::from(d);
        let end = new / f64::from(d);
        let mut cycle = if state.looping { start.floor() } else { 0. };
        while cycle <= end.floor() && self.events.len() < 64 {
            for e in &state.events {
                let at = cycle + f64::from(e.time);
                if at > start && at <= end && self.events.len() < 64 {
                    self.events.push(e.name.clone());
                }
            }
            if !state.looping {
                break;
            }
            let next = cycle + 1.;
            if next == cycle {
                break;
            }
            cycle = next;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::AnimationCondition;
    use crate::anim::AnimationTrack;
    use crate::anim::Interpolation;
    use crate::anim::Joint;
    use crate::anim::JointAnimation;
    use glam::Vec4;
    /// A graph that must be valid, with plain indices for brevity.
    fn graph(
        parameters: Vec<AnimationParameter>,
        states: Vec<AnimationState>,
        transitions: Vec<AnimationTransition>,
        initial: usize,
    ) -> Arc<AnimationGraph> {
        AnimationGraph::new(parameters, states, transitions, StateId(initial)).expect("a valid graph")
    }
    fn skeleton() -> Skeleton {
        Skeleton::new(vec![Joint::new("root", None)])
    }
    fn clip(s: &Skeleton, x: f32) -> AnimationClip {
        AnimationClip::new(
            "pose",
            s,
            vec![JointAnimation::new(0, "root").with_translation(AnimationTrack::new(
                vec![0., 1.],
                vec![Vec4::new(x, 0., 0., 0.); 2],
                Interpolation::Linear,
            ))],
        )
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }
    #[test]
    fn layers_weight_mask_and_additive_rotation() {
        let mut s = Skeleton::new(vec![Joint::new("root", None), Joint::new("arm", 0)]);
        let mut a = animator(&s, Motion::Clip(clip(&s, 2.)), vec![]);
        let overlay = animator(&s, Motion::Clip(clip(&s, 10.)), vec![]);
        let mut layer = AnimationLayer::new(overlay.graph.clone());
        layer.mask = Some(JointMask::descendants(&s, "arm").unwrap());
        let i = a.add_layer(layer);
        a.update(0., &mut s);
        near(s.pose[0].position.x, 2.);
        a.layer_mut(i).mask = None;
        a.layer_mut(i).weight = 0.;
        a.update(0., &mut s);
        near(s.pose[0].position.x, 2.);
        a.layer_mut(i).weight = 1.;
        a.update(0., &mut s);
        near(s.pose[0].position.x, 10.);
        let rotation = AnimationClip::new(
            "add",
            &s,
            vec![JointAnimation::new(0, "root").with_rotation(AnimationTrack::new(
                vec![0., 1.],
                vec![Vec4::W, Vec4::from_array(glam::Quat::from_rotation_z(0.2).to_array())],
                Interpolation::Linear,
            ))],
        );
        let mut layer = AnimationLayer::new(animator(&s, Motion::Clip(rotation), vec![]).graph);
        layer.mode = LayerBlend::Additive;
        layer.animator.graph = graph(
            vec![],
            vec![{
                let mut state = layer.animator.graph.states[0].clone();
                state.looping = false;
                state
            }],
            vec![],
            0,
        );
        a.add_layer(layer);
        a.update(1., &mut s);
        assert!(s.pose[0].rotation.abs_diff_eq(glam::Quat::from_rotation_z(0.2), 1e-5));
    }
    fn walk(s: &Skeleton, distance: f32) -> AnimationClip {
        AnimationClip::new(
            "walk",
            s,
            vec![JointAnimation::new(0, "root").with_translation(AnimationTrack::new(
                vec![0., 1.],
                vec![Vec4::ZERO, Vec4::new(distance, 0., 0., 0.)],
                Interpolation::Linear,
            ))],
        )
    }
    #[test]
    fn root_motion_wrap_speed_and_crossfade() {
        let mut s = skeleton();
        let mut fast = AnimationState::new(Motion::Clip(walk(&s, 2.)));
        fast.speed = 2.;
        let mut a = Animator::new(graph(
            vec![],
            vec![AnimationState::new(Motion::Clip(walk(&s, 1.))), fast],
            vec![],
            0,
        ));
        a.extract_root_motion = true;
        let mut total = 0.;
        for _ in 0..10 {
            a.update(0.3, &mut s);
            total += a.root_motion().position.x;
            near(s.pose[0].position.x, 0.);
        }
        near(total, 3.);
        a.crossfade(StateId(1), 1., 0.);
        a.update(0.5, &mut s);
        near(a.root_motion().position.x, 1.25);
        a.update(2., &mut s);
        near(a.root_motion().position.x, 8.);
    }
    #[test]
    fn idle_walk_run_use_ordered_transitions_and_fixed_time_weights() {
        let mut s = skeleton();
        let states = [0., 1., 2.]
            .map(|x| AnimationState::new(Motion::Clip(clip(&s, x))))
            .to_vec();
        let transition = |from, to, condition| AnimationTransition {
            from: Some(StateId(from)),
            to: StateId(to),
            conditions: vec![condition],
            duration: 0.12,
            exit_time: None,
            offset: 0.,
        };
        let g = graph(
            vec![AnimationParameter::Float(0.)],
            states,
            vec![
                transition(0, 1, AnimationCondition::Greater(ParamId(0), 0.05)),
                transition(1, 2, AnimationCondition::Greater(ParamId(0), 1.4)),
            ],
            0,
        );
        let mut a = Animator::new(g);
        a.update(0., &mut s);
        a.set_float(ParamId(0), 1.);
        a.update(0.03, &mut s);
        assert_eq!(a.state(), StateId(1));
        near(a.fade_weight(), 0.25);
        near(s.pose[0].position.x, 0.25);
        a.update(0.09, &mut s);
        near(a.fade_weight(), 1.);
        a.set_float(ParamId(0), 2.);
        a.update(0.06, &mut s);
        assert_eq!(a.state(), StateId(2));
        near(s.pose[0].position.x, 1.5);
    }
    fn animator(_s: &Skeleton, motion: Motion, parameters: Vec<AnimationParameter>) -> Animator {
        Animator::new(graph(parameters, vec![AnimationState::new(motion)], vec![], 0))
    }
    /// Ten hours of 60 Hz updates in one looping state: the pose still
    /// moves by one step's worth each update, its events fire once a loop,
    /// and the clock reads ten hours' worth of loops.
    #[test]
    fn a_looping_state_keeps_its_speed_and_events_after_ten_hours() {
        let mut s = skeleton();
        let state = AnimationState::new(Motion::Clip(walk(&s, 1.))).with_event(AnimationEvent::new(0.5, "step"));
        let mut a = Animator::new(graph(vec![], vec![state], vec![], 0));
        let dt = 1. / 60.;
        let steps = 10 * 3600 * 60;
        for _ in 0..steps {
            a.update(dt, &mut s);
        }
        a.drain_events().for_each(drop);
        let loops = a.normalized_time_f64();
        let expected = steps as f64 * f64::from(dt);
        assert!((loops - expected).abs() < 1e-6, "{loops} loops, expected {expected}");
        // The next two seconds: every step moves the root by 1/60 (or
        // wraps), and the event fires twice.
        let mut x = s.pose[0].position.x;
        for _ in 0..120 {
            a.update(dt, &mut s);
            let now = s.pose[0].position.x;
            let moved = (now - x).rem_euclid(1.);
            assert!((moved - dt).abs() < 1e-4, "moved {moved} in one step");
            x = now;
        }
        assert_eq!(a.drain_events().count(), 2);
        // The negative control: a clock in f32 at ten hours steps in 1/256
        // s, so a 1/60 s step would land on 1/64 s.
        let clock = 36_000.0_f32;
        assert_eq!((clock + dt) - clock, 1. / 64.);
    }
    fn child(s: &Skeleton, x: f32) -> MotionChild {
        MotionChild::new(Motion::Clip(clip(s, x)))
    }
    #[test]
    fn one_dimensional_blends_match_thresholds_and_interpolate_neighbours() {
        let mut s = skeleton();
        let tree = Motion::blend_1d(
            ParamId(0),
            vec![(0., child(&s, 0.)), (1., child(&s, 10.)), (2., child(&s, 30.))],
        );
        let mut a = animator(&s, tree, vec![AnimationParameter::Float(0.)]);
        for (v, want) in [
            (-1., 0.),
            (0., 0.),
            (0.5, 5.),
            (1., 10.),
            (1.5, 20.),
            (2., 30.),
            (3., 30.),
        ] {
            a.set_float(ParamId(0), v);
            a.update(0., &mut s);
            near(s.pose[0].position.x, want);
        }
    }
    #[test]
    fn two_dimensional_blends_select_cardinals_exactly_and_support_nested_trees() {
        let mut s = skeleton();
        let points = [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y];
        let children = points
            .into_iter()
            .enumerate()
            .map(|(i, p)| (p, child(&s, i as f32)))
            .collect();
        let tree = Motion::blend_2d(ParamId(0), ParamId(1), children);
        let nested = Motion::blend_1d(ParamId(0), vec![(0., MotionChild::new(tree))]);
        let mut a = animator(&s, nested, vec![AnimationParameter::Float(0.); 2]);
        for (i, p) in points.into_iter().enumerate() {
            a.set_float(ParamId(0), p.x);
            a.set_float(ParamId(1), p.y);
            a.update(0., &mut s);
            near(s.pose[0].position.x, i as f32);
        }
        a.set_float(ParamId(0), 0.);
        a.set_float(ParamId(1), 0.);
        a.update(0., &mut s);
        near(s.pose[0].position.x, 1.5);
    }
    fn transition(from: Option<usize>, to: usize, conditions: Vec<AnimationCondition>) -> AnimationTransition {
        AnimationTransition {
            from: from.map(StateId),
            to: StateId(to),
            conditions,
            exit_time: None,
            duration: 0.,
            offset: 0.,
        }
    }
    #[test]
    fn selected_transition_consumes_triggers_but_failed_conditions_do_not() {
        let mut s = skeleton();
        let states = vec![AnimationState::new(Motion::Clip(clip(&s, 0.))); 2];
        let transitions = vec![
            transition(
                Some(0),
                1,
                vec![
                    AnimationCondition::Trigger(ParamId(0)),
                    AnimationCondition::Bool(ParamId(1), true),
                ],
            ),
            transition(Some(1), 0, vec![AnimationCondition::Trigger(ParamId(0))]),
        ];
        let mut a = Animator::new(graph(
            vec![AnimationParameter::Trigger(false), AnimationParameter::Bool(false)],
            states,
            transitions,
            0,
        ));
        a.set_trigger(ParamId(0), true);
        a.update(0., &mut s);
        assert_eq!(a.state(), StateId(0));
        a.set_bool(ParamId(1), true);
        a.update(0., &mut s);
        assert_eq!(a.state(), StateId(1));
        a.update(0., &mut s);
        assert_eq!(a.state(), StateId(1));
        a.set_trigger(ParamId(0), true);
        a.update(0., &mut s);
        assert_eq!(a.state(), StateId(0));
    }
    #[test]
    fn exit_time_waits_until_normalized_threshold_and_any_state_ignores_it() {
        let mut s = skeleton();
        let states = vec![AnimationState::new(Motion::Clip(clip(&s, 0.))); 2];
        let mut t = transition(Some(0), 1, vec![]);
        t.exit_time = Some(0.75);
        let mut a = Animator::new(graph(vec![], states.clone(), vec![t.clone()], 0));
        a.update(0.5, &mut s);
        assert_eq!(a.state(), StateId(0));
        a.update(0.25, &mut s);
        assert_eq!(a.state(), StateId(1));
        t.from = None;
        t.exit_time = Some(100.);
        let mut a = Animator::new(graph(vec![], states, vec![t], 0));
        a.update(0., &mut s);
        assert_eq!(a.state(), StateId(1));
    }
    #[test]
    fn interrupted_crossfade_starts_at_the_last_mixed_pose() {
        let mut s = skeleton();
        let states = [0., 10., 20.]
            .map(|x| AnimationState::new(Motion::Clip(clip(&s, x))))
            .to_vec();
        let mut a = Animator::new(graph(vec![], states, vec![], 0));
        a.update(0., &mut s);
        a.crossfade(StateId(1), 1., 0.);
        a.update(0.5, &mut s);
        near(s.pose[0].position.x, 5.);
        a.crossfade(StateId(2), 1., 0.);
        a.update(0., &mut s);
        near(s.pose[0].position.x, 5.);
        a.update(0.5, &mut s);
        near(s.pose[0].position.x, 12.5);
        a.play(StateId(0), 0.);
        a.update(0., &mut s);
        near(s.pose[0].position.x, 0.);
    }
    #[test]
    fn events_cross_loop_wrap_in_time_order_and_reuse_buffer() {
        let mut s = skeleton();
        let state = AnimationState::new(Motion::Clip(clip(&s, 0.)))
            .with_event(AnimationEvent::new(0.9, "end"))
            .with_event(AnimationEvent::new(0.1, "start"));
        let mut a = Animator::new(graph(vec![], vec![state], vec![], 0));
        a.play(StateId(0), 0.8);
        a.update(0.4, &mut s);
        assert_eq!(
            a.drain_events().map(|s| s.to_string()).collect::<Vec<_>>(),
            ["end", "start"]
        );
        a.update(0., &mut s);
        assert_eq!(a.drain_events().count(), 0);
        a.update(100., &mut s);
        assert_eq!(a.drain_events().count(), 64);
    }
    #[test]
    fn child_time_scale_and_parameter_speed_multiply_and_clamping_holds_end() {
        let mut s = skeleton();
        let c = AnimationClip::new(
            "ramp",
            &s,
            vec![JointAnimation::new(0, "root").with_translation(AnimationTrack::new(
                vec![0., 1.],
                vec![Vec4::ZERO, Vec4::X],
                Interpolation::Linear,
            ))],
        );
        let mut state = AnimationState::new(Motion::blend_1d(
            ParamId(0),
            vec![(0., MotionChild::new(Motion::Clip(c)).with_time_scale(2.))],
        ));
        state.speed = 2.;
        state.speed_parameter = Some(ParamId(0));
        state.looping = false;
        let mut a = Animator::new(graph(vec![AnimationParameter::Float(0.5)], vec![state], vec![], 0));
        a.update(0.125, &mut s);
        near(s.pose[0].position.x, 0.25);
        near(a.normalized_time(), 0.25);
        a.update(2., &mut s);
        near(s.pose[0].position.x, 1.);
        near(a.normalized_time(), 1.);
        a.set_float(ParamId(0), -1.);
        a.play(StateId(0), 0.);
        a.update(1., &mut s);
        near(s.pose[0].position.x, 0.);
    }
    #[test]
    fn duplicate_thresholds_are_rejected() {
        let s = skeleton();
        let state = AnimationState::new(Motion::blend_1d(
            ParamId(0),
            vec![(0., child(&s, 0.)), (0., child(&s, 1.))],
        ));
        let err =
            AnimationGraph::new(vec![AnimationParameter::Float(0.)], vec![state], vec![], StateId(0)).unwrap_err();
        assert_eq!(err, AnimationGraphError::InvalidThresholds(StateId(0)));
        assert_eq!(err.to_string(), "state 0 has invalid 1D blend thresholds");
    }
    #[test]
    fn invalid_graphs_are_errors_naming_the_problem() {
        let s = skeleton();
        let idle = || AnimationState::new(Motion::Clip(clip(&s, 0.)));
        let new = |p, states, t, i| AnimationGraph::new(p, states, t, StateId(i)).unwrap_err();
        assert_eq!(
            new(vec![], vec![idle()], vec![], 1),
            AnimationGraphError::InvalidInitialState(StateId(1))
        );
        assert_eq!(
            new(vec![AnimationParameter::Float(f32::NAN)], vec![idle()], vec![], 0),
            AnimationGraphError::NonFiniteParameter(ParamId(0))
        );
        assert_eq!(
            new(vec![], vec![idle()], vec![transition(Some(0), 3, vec![])], 0),
            AnimationGraphError::InvalidTransitionState(0)
        );
        assert_eq!(
            new(
                vec![AnimationParameter::Bool(false)],
                vec![idle()],
                vec![transition(None, 0, vec![AnimationCondition::Trigger(ParamId(0))])],
                0
            ),
            AnimationGraphError::WrongParameterKind {
                parameter: ParamId(0),
                expected: "trigger"
            }
        );
        let mut fast = idle();
        fast.speed = -1.;
        assert_eq!(
            new(vec![], vec![idle(), fast], vec![], 0),
            AnimationGraphError::InvalidSpeed(StateId(1))
        );
    }
    #[test]
    fn automatic_updates_run_without_a_renderer_and_manual_animators_are_skipped() {
        let mut ctx = crate::ctx::Ctx::new(0.25, Vec2::ONE);
        let s = skeleton();
        let c = clip(&s, 3.);
        let graph = graph(vec![], vec![AnimationState::new(Motion::Clip(c))], vec![], 0);
        let automatic = ctx.world.spawn((Animator::new(graph.clone()), s.clone()));
        let mut manual = Animator::new(graph);
        manual.manual = true;
        let manual = ctx.world.spawn((manual, s));
        ctx.update_animators();
        near(ctx.world.get::<Skeleton>(automatic).unwrap().pose[0].position.x, 3.);
        near(ctx.world.get::<Skeleton>(manual).unwrap().pose[0].position.x, 0.);
    }
    #[test]
    fn first_matching_transition_wins_and_target_offset_is_applied() {
        let mut s = skeleton();
        let states = vec![AnimationState::new(Motion::Clip(clip(&s, 0.))); 3];
        let mut t = transition(Some(0), 1, vec![AnimationCondition::Less(ParamId(0), 1.)]);
        t.offset = 0.5;
        let mut a = Animator::new(graph(
            vec![AnimationParameter::Float(0.)],
            states,
            vec![t, transition(None, 2, vec![])],
            0,
        ));
        a.update(0.1, &mut s);
        assert_eq!(a.state(), StateId(1));
        near(a.normalized_time(), 0.6);
    }
    #[test]
    fn ordinary_fades_keep_advancing_the_outgoing_clip() {
        let mut s = skeleton();
        let ramp = AnimationClip::new(
            "ramp",
            &s,
            vec![JointAnimation::new(0, "root").with_translation(AnimationTrack::new(
                vec![0., 1.],
                vec![Vec4::ZERO, Vec4::X],
                Interpolation::Linear,
            ))],
        );
        let mut a = Animator::new(graph(
            vec![],
            vec![
                AnimationState::new(Motion::Clip(ramp)),
                AnimationState::new(Motion::Clip(clip(&s, 2.))),
            ],
            vec![],
            0,
        ));
        a.update(0.25, &mut s);
        a.crossfade(StateId(1), 1., 0.);
        a.update(0.25, &mut s);
        near(s.pose[0].position.x, 0.875);
    }
}
