//! The parts of an animation state machine that don't depend on what is
//! posed: states and parameters by index, parameter values, transition
//! conditions, transitions and events. The 3D `AnimationGraph` and the 2D
//! [`SkeletonGraph`](crate::skeleton2d::SkeletonGraph) both use them, so a
//! state machine reads the same in both.
use std::fmt;
use std::sync::Arc;

/// A state of an animation graph (the 3D `AnimationGraph`, or the 2D
/// [`SkeletonGraph`](crate::skeleton2d::SkeletonGraph)): its position in the
/// graph's state list, from 0.
/// ```
/// let walk = ponko::anim::StateId(1);
/// assert_eq!(walk.0, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StateId(pub usize);

/// A parameter of an animation graph (3D or 2D): its position in the
/// graph's parameter list, from 0.
/// ```
/// let speed = ponko::anim::ParamId(0);
/// assert_eq!(speed.0, 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParamId(pub usize);

/// Why an animation graph was refused (the 3D `AnimationGraph::new` or the
/// 2D [`SkeletonGraph::new`](crate::skeleton2d::SkeletonGraph::new)), or why
/// a checked call (`try_set_float`, `try_play`, `try_crossfade_state`, ...)
/// refused a command. More variants can arrive in a minor release, so a
/// `match` needs a `_` arm.
/// ```
/// use ponko::anim::{AnimationGraphError, StateId};
/// use ponko::skeleton2d::SkeletonGraph;
/// let err = SkeletonGraph::new(vec![], vec![], vec![], StateId(0)).unwrap_err();
/// assert_eq!(err, AnimationGraphError::InvalidInitialState(StateId(0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AnimationGraphError {
    /// The initial state is not in the state list.
    InvalidInitialState(StateId),
    /// A float parameter's default is NaN or infinite.
    NonFiniteParameter(ParamId),
    /// A state's speed is negative or not finite.
    InvalidSpeed(StateId),
    /// A state's event time is outside 0..1.
    InvalidEventTime(StateId),
    /// A state's blend tree has a child time scale that is not positive and finite.
    InvalidTimeScale(StateId),
    /// A state's 1D blend has no children, or thresholds that are not
    /// finite and strictly increasing.
    InvalidThresholds(StateId),
    /// A state's 2D blend has no children, or a point that is not finite.
    InvalidPoints(StateId),
    /// A transition (by its position in the list) names a state that is not
    /// in the state list.
    InvalidTransitionState(usize),
    /// A transition's duration, offset or exit time is negative or not finite.
    InvalidTransitionClock(usize),
    /// A transition's condition threshold is not finite.
    InvalidThreshold(usize),
    /// A parameter is used as a kind it isn't (or is not in the list).
    WrongParameterKind {
        /// The parameter.
        parameter: ParamId,
        /// The kind it is used as: `"float"`, `"bool"` or `"trigger"`.
        expected: &'static str,
    },
    /// A play or crossfade command names a state that is not in the graph
    /// (`Animator::try_play` and `try_crossfade`,
    /// [`Skeleton::try_crossfade_state`](crate::skeleton2d::Skeleton::try_crossfade_state)).
    UnknownState(StateId),
    /// A value given to a checked float setter (`Animator::try_set_float`,
    /// [`Skeleton::try_set_float`](crate::skeleton2d::Skeleton::try_set_float))
    /// is NaN or infinite.
    NonFiniteValue(ParamId),
    /// A play or crossfade command's normalized time or duration is negative
    /// or not finite.
    InvalidPlayback(StateId),
}

impl fmt::Display for AnimationGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInitialState(s) => write!(f, "initial state {} is not in the graph", s.0),
            Self::NonFiniteParameter(p) => write!(f, "parameter {} has a non-finite default", p.0),
            Self::InvalidSpeed(s) => write!(f, "state {} has an invalid speed", s.0),
            Self::InvalidEventTime(s) => write!(f, "state {} has an event time outside 0..1", s.0),
            Self::InvalidTimeScale(s) => write!(f, "state {} has an invalid child time scale", s.0),
            Self::InvalidThresholds(s) => write!(f, "state {} has invalid 1D blend thresholds", s.0),
            Self::InvalidPoints(s) => write!(f, "state {} has invalid 2D blend points", s.0),
            Self::InvalidTransitionState(t) => write!(f, "transition {t} names a state that is not in the graph"),
            Self::InvalidTransitionClock(t) => write!(f, "transition {t} has an invalid duration, offset or exit time"),
            Self::InvalidThreshold(t) => write!(f, "transition {t} has a non-finite threshold"),
            Self::WrongParameterKind { parameter, expected } => {
                write!(f, "parameter {} is not a {expected} parameter", parameter.0)
            }
            Self::UnknownState(s) => write!(f, "no such animation state {}", s.0),
            Self::NonFiniteValue(p) => write!(f, "value for parameter {} is not finite", p.0),
            Self::InvalidPlayback(s) => {
                write!(
                    f,
                    "command for state {} has a negative or non-finite time or duration",
                    s.0
                )
            }
        }
    }
}

impl std::error::Error for AnimationGraphError {}

/// Parameter defaults; a [`ParamId`] (the position in the graph's list)
/// identifies each.
/// ```
/// use ponko::anim::AnimationParameter;
/// let speed = ponko::anim::AnimationParameter::Float(0.0);
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum AnimationParameter {
    /// Floating point value.
    Float(f32),
    /// Persistent boolean value.
    Bool(bool),
    /// One-shot flag, consumed only by a selected transition.
    Trigger(bool),
}

/// A named event on a state's normalized clock.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new).
/// ```
/// use ponko::anim::AnimationEvent;
/// let step = AnimationEvent::new(0.5, "step");
/// assert_eq!((step.time, &*step.name), (0.5, "step"));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct AnimationEvent {
    /// Normalized time in [0, 1); zero fires on loop wrap, not initial entry.
    pub time: f32,
    /// Shared event name; reporting clones the Arc without allocating.
    pub name: Arc<str>,
}

impl AnimationEvent {
    /// An event called `name` at normalized `time` (0 up to, not including, 1).
    /// Graphs refuse a time outside that range.
    /// ```
    /// use ponko::anim::AnimationEvent;
    /// let land = AnimationEvent::new(0.0, "land");
    /// assert_eq!(&*land.name, "land");
    /// ```
    #[must_use]
    pub fn new(time: f32, name: impl Into<Arc<str>>) -> Self {
        Self {
            time,
            name: name.into(),
        }
    }
}

/// Transition predicates, all of which must hold.
/// ```
/// use ponko::anim::{AnimationCondition, ParamId};
/// let moving = AnimationCondition::Greater(ParamId(0), 0.05);
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum AnimationCondition {
    /// Float parameter strictly greater than threshold.
    Greater(ParamId, f32),
    /// Float parameter strictly less than threshold.
    Less(ParamId, f32),
    /// Boolean parameter equals the given value.
    Bool(ParamId, bool),
    /// Trigger parameter is set.
    Trigger(ParamId),
}

/// An ordered transition. The graph's first matching transition wins, with at
/// most one taken per update; any-state transitions ignore exit time and do
/// not reenter their current target. Exit time is a minimum on the state's
/// unwrapped normalized clock, checked against the end of this update.
///
/// New fields may be added in any release: build one with
/// [`new`](Self::new) or [`from_any`](Self::from_any) and the `with_*`
/// methods.
/// ```
/// use ponko::anim::{AnimationCondition, AnimationTransition, ParamId, StateId};
/// let t = AnimationTransition::new(StateId(0), StateId(1))
///     .with_condition(AnimationCondition::Greater(ParamId(0), 0.5))
///     .with_exit_time(1.0)
///     .with_duration(0.12);
/// assert_eq!(t.from, Some(StateId(0)));
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AnimationTransition {
    /// Source state, or None for any state.
    pub from: Option<StateId>,
    /// Target state.
    pub to: StateId,
    /// Required predicates; an empty list always matches.
    pub conditions: Vec<AnimationCondition>,
    /// Optional minimum normalized time, ignored for any-state transitions.
    pub exit_time: Option<f32>,
    /// Crossfade duration in seconds, independent of playback speed.
    pub duration: f32,
    /// Target normalized starting time.
    pub offset: f32,
}

impl AnimationTransition {
    /// A transition from state `from` to `to`, with no conditions (it always
    /// matches), no exit time, no crossfade and no offset.
    /// ```
    /// use ponko::anim::{AnimationTransition, StateId};
    /// let t = AnimationTransition::new(StateId(0), StateId(1));
    /// assert_eq!((t.duration, t.exit_time), (0.0, None));
    /// ```
    #[must_use]
    pub fn new(from: StateId, to: StateId) -> Self {
        Self {
            from: Some(from),
            ..Self::from_any(to)
        }
    }

    /// A transition from any state to `to` (not reentering `to` itself),
    /// otherwise as [`new`](Self::new).
    /// ```
    /// use ponko::anim::{AnimationTransition, StateId};
    /// assert_eq!(AnimationTransition::from_any(StateId(2)).from, None);
    /// ```
    #[must_use]
    pub fn from_any(to: StateId) -> Self {
        Self {
            from: None,
            to,
            conditions: Vec::new(),
            exit_time: None,
            duration: 0.,
            offset: 0.,
        }
    }

    /// Adds a predicate that must hold (all of them must).
    #[must_use]
    pub fn with_condition(mut self, condition: AnimationCondition) -> Self {
        self.conditions.push(condition);
        self
    }

    /// Sets [`exit_time`](Self::exit_time), the minimum normalized time
    /// before it is taken (`None`: any time).
    #[must_use]
    pub fn with_exit_time(mut self, exit_time: impl Into<Option<f32>>) -> Self {
        self.exit_time = exit_time.into();
        self
    }

    /// Sets [`duration`](Self::duration), the crossfade in seconds.
    #[must_use]
    pub fn with_duration(mut self, duration: f32) -> Self {
        self.duration = duration;
        self
    }

    /// Sets [`offset`](Self::offset), the target's normalized starting time.
    #[must_use]
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }
}

/// `Ok` when parameter `p` exists and is of kind `expected`.
pub(crate) fn expect_kind(
    parameters: &[AnimationParameter],
    p: ParamId,
    expected: &'static str,
) -> Result<(), AnimationGraphError> {
    let kind = match parameters.get(p.0) {
        Some(AnimationParameter::Float(_)) => "float",
        Some(AnimationParameter::Bool(_)) => "bool",
        Some(AnimationParameter::Trigger(_)) => "trigger",
        None => "",
    };
    if kind == expected {
        Ok(())
    } else {
        Err(AnimationGraphError::WrongParameterKind { parameter: p, expected })
    }
}
/// Sets float parameter `p`, or why not: not a float (or not in the list),
/// or a NaN or infinite `value`. Nothing changes on an error.
pub(crate) fn set_float(
    parameters: &mut [AnimationParameter],
    p: ParamId,
    value: f32,
) -> Result<(), AnimationGraphError> {
    let Some(AnimationParameter::Float(v)) = parameters.get_mut(p.0) else {
        return Err(AnimationGraphError::WrongParameterKind {
            parameter: p,
            expected: "float",
        });
    };
    if !value.is_finite() {
        return Err(AnimationGraphError::NonFiniteValue(p));
    }
    *v = value;
    Ok(())
}
/// Sets bool parameter `p`, or why not (not a bool, or not in the list).
pub(crate) fn set_bool(
    parameters: &mut [AnimationParameter],
    p: ParamId,
    value: bool,
) -> Result<(), AnimationGraphError> {
    match parameters.get_mut(p.0) {
        Some(AnimationParameter::Bool(v)) => {
            *v = value;
            Ok(())
        }
        _ => Err(AnimationGraphError::WrongParameterKind {
            parameter: p,
            expected: "bool",
        }),
    }
}
/// Sets trigger `p`, or why not (not a trigger, or not in the list).
pub(crate) fn set_trigger(
    parameters: &mut [AnimationParameter],
    p: ParamId,
    value: bool,
) -> Result<(), AnimationGraphError> {
    match parameters.get_mut(p.0) {
        Some(AnimationParameter::Trigger(v)) => {
            *v = value;
            Ok(())
        }
        _ => Err(AnimationGraphError::WrongParameterKind {
            parameter: p,
            expected: "trigger",
        }),
    }
}
/// `Ok` when `state` is below `states` and the command's duration and
/// normalized offset are finite and nonnegative.
pub(crate) fn check_command(
    states: usize,
    state: StateId,
    duration: f32,
    offset: f32,
) -> Result<(), AnimationGraphError> {
    if state.0 >= states {
        return Err(AnimationGraphError::UnknownState(state));
    }
    if !(duration.is_finite() && duration >= 0. && offset.is_finite() && offset >= 0.) {
        return Err(AnimationGraphError::InvalidPlayback(state));
    }
    Ok(())
}
pub(crate) fn float_parameter(p: &[AnimationParameter], i: ParamId) -> f32 {
    match p.get(i.0) {
        Some(AnimationParameter::Float(v)) => *v,
        _ => panic!("AnimationGraph: expected float parameter; use a Float parameter in Blend1D"),
    }
}

/// `Ok` when every float parameter's default is finite.
pub(crate) fn check_parameters(parameters: &[AnimationParameter]) -> Result<(), AnimationGraphError> {
    for (i, p) in parameters.iter().enumerate() {
        if let AnimationParameter::Float(v) = p
            && !v.is_finite()
        {
            return Err(AnimationGraphError::NonFiniteParameter(ParamId(i)));
        }
    }
    Ok(())
}

/// `Ok` when every transition names states below `states`, has finite,
/// nonnegative clocks and conditions on parameters of the right kind.
pub(crate) fn check_transitions(
    parameters: &[AnimationParameter],
    transitions: &[AnimationTransition],
    states: usize,
) -> Result<(), AnimationGraphError> {
    for (i, t) in transitions.iter().enumerate() {
        if !(t.to.0 < states && t.from.is_none_or(|s| s.0 < states)) {
            return Err(AnimationGraphError::InvalidTransitionState(i));
        }
        if !(t.duration.is_finite()
            && t.duration >= 0.
            && t.offset.is_finite()
            && t.offset >= 0.
            && t.exit_time.is_none_or(|v| v.is_finite() && v >= 0.))
        {
            return Err(AnimationGraphError::InvalidTransitionClock(i));
        }
        for c in &t.conditions {
            match *c {
                AnimationCondition::Greater(p, v) | AnimationCondition::Less(p, v) => {
                    expect_kind(parameters, p, "float")?;
                    if !v.is_finite() {
                        return Err(AnimationGraphError::InvalidThreshold(i));
                    }
                }
                AnimationCondition::Bool(p, _) => expect_kind(parameters, p, "bool")?,
                AnimationCondition::Trigger(p) => expect_kind(parameters, p, "trigger")?,
            }
        }
    }
    Ok(())
}

/// Whether condition `c` holds for `parameters` (a checked graph's).
pub(crate) fn condition_holds(parameters: &[AnimationParameter], c: &AnimationCondition) -> bool {
    match *c {
        AnimationCondition::Greater(i, v) => float_parameter(parameters, i) > v,
        AnimationCondition::Less(i, v) => float_parameter(parameters, i) < v,
        AnimationCondition::Bool(i, v) => matches!(parameters[i.0], AnimationParameter::Bool(b) if b == v),
        AnimationCondition::Trigger(i) => matches!(parameters[i.0], AnimationParameter::Trigger(true)),
    }
}

/// The first transition out of `state` whose conditions hold: any-state
/// transitions that don't reenter their target, or transitions from
/// `state` whose exit time `exit_reached` accepts.
pub(crate) fn select_transition(
    transitions: &[AnimationTransition],
    parameters: &[AnimationParameter],
    state: usize,
    mut exit_reached: impl FnMut(f32) -> bool,
) -> Option<usize> {
    transitions.iter().position(|t| {
        t.from.map_or(t.to.0 != state, |s| s.0 == state)
            && (t.from.is_none() || t.exit_time.is_none_or(&mut exit_reached))
            && t.conditions.iter().all(|c| condition_holds(parameters, c))
    })
}

/// Clears the triggers transition `t` consumed.
pub(crate) fn consume_triggers(parameters: &mut [AnimationParameter], t: &AnimationTransition) {
    for c in &t.conditions {
        if let AnimationCondition::Trigger(p) = *c {
            parameters[p.0] = AnimationParameter::Trigger(false);
        }
    }
}
