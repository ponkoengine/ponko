//! The 2D state machine: states that play a clip or a 1D blend of clips,
//! with the shared parameters, conditions and transitions of
//! [`anim`](crate::anim).

use std::sync::Arc;

use super::data::{SkeletonError, err};
use crate::anim::graph::{check_parameters, check_transitions, expect_kind};
use crate::anim::{AnimationGraphError, AnimationParameter, AnimationTransition, ParamId, StateId};

/// What a [`SkeletonState`] plays.
///
/// ```
/// use ponko::anim::ParamId;
/// use ponko::skeleton2d::SkeletonMotion;
/// // Clip 0 standing, 1 walking at speed 80, 2 running at 200.
/// let locomotion = SkeletonMotion::Blend1D {
///     parameter: ParamId(0),
///     children: vec![(0.0, 0), (80.0, 1), (200.0, 2)],
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SkeletonMotion {
    /// One clip, by index.
    Clip(usize),
    /// Clips (by index) at increasing thresholds of a float parameter: the
    /// two around its value are blended by how close it is to each; past
    /// the ends, the end clip plays alone. The blended clips stay at the
    /// same point of their cycles (a walk and a run keep their steps in
    /// step), and the state's length is theirs mixed by weight.
    Blend1D {
        /// The float parameter.
        parameter: ParamId,
        /// Threshold and clip pairs, thresholds strictly increasing.
        children: Vec<(f32, usize)>,
    },
}

/// A state of a [`SkeletonGraph`]. Use [`new`](Self::new) for a looping
/// state at normal speed.
///
/// ```
/// use ponko::skeleton2d::{SkeletonMotion, SkeletonState};
/// let jump = SkeletonState::new("jump", SkeletonMotion::Clip(3)).once();
/// assert!(!jump.looping);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SkeletonState {
    /// Its name, for [`SkeletonGraph::state`] and saves.
    pub name: Arc<str>,
    /// What it plays.
    pub motion: SkeletonMotion,
    /// Playback speed multiplier, 0 or more.
    pub speed: f32,
    /// Starts over at the end, or holds the last pose.
    pub looping: bool,
}

impl SkeletonState {
    /// A state named `name` looping `motion` at normal speed.
    #[must_use]
    pub fn new(name: &str, motion: SkeletonMotion) -> Self {
        Self {
            name: name.into(),
            motion,
            speed: 1.0,
            looping: true,
        }
    }

    /// The same state, played once and then held.
    #[must_use]
    pub fn once(self) -> Self {
        Self { looping: false, ..self }
    }

    /// The same state at speed `speed`.
    #[must_use]
    pub fn with_speed(self, speed: f32) -> Self {
        Self { speed, ..self }
    }
}

/// A 2D skeleton's state machine: named parameters, states and the shared
/// [`AnimationTransition`]s, which read the same as the 3D graph's. Shared
/// by every skeleton that plays it, each with its own parameter values and
/// clock.
///
/// ```
/// use ponko::anim::{AnimationCondition, AnimationParameter, AnimationTransition, ParamId, StateId};
/// use ponko::skeleton2d::{SkeletonGraph, SkeletonMotion, SkeletonState};
/// let graph = SkeletonGraph::new(
///     vec![("jump", AnimationParameter::Trigger(false))],
///     vec![
///         SkeletonState::new("idle", SkeletonMotion::Clip(0)),
///         SkeletonState::new("jump", SkeletonMotion::Clip(1)).once(),
///     ],
///     vec![
///         AnimationTransition::new(StateId(0), StateId(1))
///             .with_condition(AnimationCondition::Trigger(ParamId(0)))
///             .with_duration(0.1),
///         AnimationTransition::new(StateId(1), StateId(0)).with_exit_time(1.0).with_duration(0.2),
///     ],
///     StateId(0),
/// )
/// .expect("a valid graph");
/// assert_eq!(graph.state("jump"), Some(StateId(1)));
/// ```
#[derive(Debug, Clone)]
pub struct SkeletonGraph {
    pub(crate) names: Vec<Arc<str>>,
    pub(crate) parameters: Vec<AnimationParameter>,
    pub(crate) states: Vec<SkeletonState>,
    pub(crate) transitions: Vec<AnimationTransition>,
    pub(crate) initial: usize,
}

impl SkeletonGraph {
    /// Checks the graph like the 3D one (ids, parameter kinds, finite
    /// clocks, increasing thresholds) and returns it shared.
    ///
    /// # Errors
    /// What is wrong with it.
    pub fn new(
        parameters: Vec<(&str, AnimationParameter)>,
        states: Vec<SkeletonState>,
        transitions: Vec<AnimationTransition>,
        initial: StateId,
    ) -> Result<Arc<Self>, AnimationGraphError> {
        let (names, parameters): (Vec<Arc<str>>, Vec<AnimationParameter>) =
            parameters.into_iter().map(|(n, p)| (Arc::from(n), p)).unzip();
        if initial.0 >= states.len() {
            return Err(AnimationGraphError::InvalidInitialState(initial));
        }
        check_parameters(&parameters)?;
        for (i, s) in states.iter().enumerate() {
            let id = StateId(i);
            if !(s.speed.is_finite() && s.speed >= 0.0) {
                return Err(AnimationGraphError::InvalidSpeed(id));
            }
            if let SkeletonMotion::Blend1D { parameter, children } = &s.motion {
                expect_kind(&parameters, *parameter, "float")?;
                if children.is_empty()
                    || !children.iter().all(|(v, _)| v.is_finite())
                    || !children.windows(2).all(|c| c[0].0 < c[1].0)
                {
                    return Err(AnimationGraphError::InvalidThresholds(id));
                }
            }
        }
        check_transitions(&parameters, &transitions, states.len())?;
        Ok(Arc::new(Self {
            names,
            parameters,
            states,
            transitions,
            initial: initial.0,
        }))
    }

    /// The parameter named `name`.
    #[must_use]
    pub fn parameter(&self, name: &str) -> Option<ParamId> {
        self.names.iter().position(|n| &**n == name).map(ParamId)
    }

    /// The state named `name`.
    #[must_use]
    pub fn state(&self, name: &str) -> Option<StateId> {
        self.states.iter().position(|s| &*s.name == name).map(StateId)
    }

    /// [`parameter`](Self::parameter) for a name that must be there:
    /// resolve it once and keep the id.
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) for a name
    /// no parameter has; its message lists the parameters.
    pub fn try_parameter(&self, name: &str) -> Result<ParamId, crate::error::Error> {
        self.parameter(name).ok_or_else(|| {
            crate::error::Error::unknown_name("animation parameter", name, self.names.iter().map(|n| &**n))
        })
    }

    /// [`state`](Self::state) for a name that must be there: resolve it
    /// once and keep the id.
    ///
    /// # Errors
    /// [`ErrorKind::NotFound`](crate::error::ErrorKind::NotFound) for a name
    /// no state has; its message lists the states.
    pub fn try_state(&self, name: &str) -> Result<StateId, crate::error::Error> {
        self.state(name).ok_or_else(|| {
            crate::error::Error::unknown_name("animation state", name, self.states.iter().map(|s| &*s.name))
        })
    }

    /// Its states.
    #[must_use]
    pub fn states(&self) -> &[SkeletonState] {
        &self.states
    }

    /// Its parameters' names, by [`ParamId`].
    #[must_use]
    pub fn parameter_names(&self) -> &[Arc<str>] {
        &self.names
    }

    /// The state it starts in.
    #[must_use]
    pub fn initial(&self) -> StateId {
        StateId(self.initial)
    }

    /// `Ok` if every clip its states play is below `clips`.
    pub(crate) fn check_clips(&self, clips: usize) -> Result<(), SkeletonError> {
        for s in &self.states {
            let ok = match &s.motion {
                SkeletonMotion::Clip(c) => *c < clips,
                SkeletonMotion::Blend1D { children, .. } => children.iter().all(|(_, c)| *c < clips),
            };
            if !ok {
                return Err(err(format!(
                    "state \"{}\" plays a clip that isn't in the skeleton",
                    s.name
                )));
            }
        }
        Ok(())
    }
}
