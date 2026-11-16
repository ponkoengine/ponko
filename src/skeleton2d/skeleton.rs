//! [`Skeleton`]: one entity's playback and pose of a shared
//! [`SkeletonData`].

use std::sync::Arc;

use glam::{Affine2, Vec2};
use ponko_ecs::Component;

use super::clip::{sample, step};
use super::data::{BonePose, SkeletonData, angle_delta};
use super::graph::{SkeletonGraph, SkeletonMotion};
use crate::anim::graph::{check_command, consume_triggers, select_transition, set_bool, set_float, set_trigger};
use crate::anim::{AnimationGraphError, AnimationParameter, ParamId, StateId};
use crate::color::Color;

/// Events kept until drained; later ones are dropped.
const MAX_EVENTS: usize = 64;

/// What plays: a clip by index, or a state of the graph.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Playing {
    Clip(usize),
    State(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Play {
    what: Playing,
    /// Normalized time, not wrapped: 2.5 is half way through the third loop.
    /// f64: an f32 counting loops would step coarsely after hours in one
    /// looping clip, and the animation would run slow and judder.
    time: f64,
    looping: bool,
}

#[derive(Debug, Clone, Copy)]
struct Fade {
    from: Play,
    elapsed: f32,
    duration: f32,
}

/// A 2D skeleton on an entity: which clip or state plays, its clock, and
/// the resulting pose, which the built-in sprite drawing draws at the
/// entity's [`Transform2D`](crate::d2::Transform), in the same batches and
/// `z` order as sprites.
///
/// The engine advances it every frame at the end of
/// [`Phase::Animate`](crate::game::Phase::Animate)
/// ([`Systems::UPDATE_SKELETONS2D`](crate::game::Systems::UPDATE_SKELETONS2D)):
/// frame systems see the previous frame's pose; late systems (IK of your
/// own, attaching things to bones) and drawing see this frame's.
/// `run_headless` has no frames: there, set [`manual`](Self::manual) and
/// call [`update`](Self::update) yourself. Steady-state updates don't
/// allocate.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::skeleton2d::{BonePose, BoneTrack, Clip, Key, Skeleton, SkeletonData};
/// let mut data = SkeletonData::new();
/// let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
/// let arm = data.add_bone("arm", Some(root), BonePose::at(vec2(10.0, 0.0)), 20.0);
/// let mut swing = BoneTrack::new(arm);
/// swing.rotate = vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0)];
/// data.add_clip(Clip::new("swing", 1.0).with_bone_track(swing));
///
/// let mut skeleton = Skeleton::new(std::sync::Arc::new(data));
/// assert!(skeleton.play("swing"));
/// skeleton.update(0.5);
/// assert!((skeleton.bone_rotation(arm) - 0.5).abs() < 1e-5);
/// ```
#[derive(Debug, Clone, Component)]
pub struct Skeleton {
    data: Arc<SkeletonData>,
    graph: Option<Arc<SkeletonGraph>>,
    parameters: Vec<AnimationParameter>,
    play: Option<Play>,
    fade: Option<Fade>,
    pending: Option<(Playing, f32, f32, bool)>,
    events: Vec<Arc<str>>,
    pose: Vec<BonePose>,
    world: Vec<Affine2>,
    scratch: Vec<BonePose>,
    blend: Vec<BonePose>,
    skin: usize,
    /// Per slot: the attachment's name and its index in the data.
    shown: Vec<(Option<Arc<str>>, Option<usize>)>,
    /// Per slot: what the game set with `set_attachment` (wins over clips).
    overrides: Vec<Option<Option<Arc<str>>>>,
    visible: Vec<bool>,
    colors: Vec<Color>,
    draw_order: Vec<u16>,
    ik_targets: Vec<Option<Vec2>>,
    ik_mix: Vec<f32>,
    /// Mesh vertices posed for drawing, reused every frame.
    pub(crate) deformed: Vec<Vec2>,
    /// Multiplies every clip's and state's speed. Default 1.
    pub speed: f32,
    /// Draw order among sprites: higher is on top. Default 0.
    pub z: f32,
    /// Tint for the whole skeleton. Default white.
    pub color: Color,
    /// Skip the engine's automatic updates; [`update`](Self::update) still works.
    pub manual: bool,
}

impl Skeleton {
    /// A skeleton at rest, in its data's graph's initial state if it has
    /// a graph, otherwise playing nothing until [`play`](Self::play).
    #[must_use]
    pub fn new(data: Arc<SkeletonData>) -> Self {
        let mut s = Self {
            graph: None,
            parameters: Vec::new(),
            play: None,
            fade: None,
            pending: None,
            events: Vec::with_capacity(MAX_EVENTS),
            pose: Vec::new(),
            world: Vec::new(),
            scratch: Vec::new(),
            blend: Vec::new(),
            skin: 0,
            shown: Vec::new(),
            overrides: Vec::new(),
            visible: Vec::new(),
            colors: Vec::new(),
            draw_order: Vec::new(),
            ik_targets: Vec::new(),
            ik_mix: Vec::new(),
            deformed: Vec::new(),
            speed: 1.0,
            z: 0.0,
            color: Color::WHITE,
            manual: false,
            data,
        };
        s.bind();
        s
    }

    /// Sizes everything to the data and goes back to rest.
    fn bind(&mut self) {
        let d = &self.data;
        self.pose = d.bones.iter().map(|b| b.rest).collect();
        self.world = vec![Affine2::IDENTITY; d.bones.len()];
        self.scratch = self.pose.clone();
        self.blend = self.pose.clone();
        self.shown = vec![(None, None); d.slots.len()];
        self.overrides = vec![None; d.slots.len()];
        self.visible = vec![true; d.slots.len()];
        self.colors = d.slots.iter().map(|s| s.color).collect();
        self.draw_order = (0..d.slots.len() as u16).collect();
        self.ik_targets = vec![None; d.ik.len()];
        self.ik_mix = d.ik.iter().map(|c| c.mix).collect();
        self.graph = d.graph.clone();
        self.parameters = self.graph.as_ref().map_or_else(Vec::new, |g| g.parameters.clone());
        self.play = self.graph.as_ref().map(|g| Play {
            what: Playing::State(g.initial),
            time: 0.0,
            looping: g.states[g.initial].looping,
        });
        self.fade = None;
        self.pending = None;
        self.pose_slots();
        self.solve();
    }

    /// Plays new `data` (a hot reload of the file): the same clip or state
    /// by name keeps playing from the same point if it is still there,
    /// parameters keep their values by name, the rest starts over.
    pub(crate) fn rebind(&mut self, data: Arc<SkeletonData>) {
        let old = std::mem::replace(&mut self.data, data);
        let play = self.play;
        let old_graph = self.graph.clone();
        let old_parameters = std::mem::take(&mut self.parameters);
        let skin = old.skins.get(self.skin).cloned();
        self.bind();
        self.skin = skin.and_then(|s| self.data.skin(&s)).unwrap_or(0);
        if let (Some(g), Some(old_g)) = (self.graph.clone(), old_graph) {
            for (name, value) in old_g.names.iter().zip(old_parameters) {
                if let Some(p) = g.parameter(name)
                    && std::mem::discriminant(&self.parameters[p.0]) == std::mem::discriminant(&value)
                {
                    self.parameters[p.0] = value;
                }
            }
        }
        if let Some(play) = play {
            let what = match play.what {
                Playing::Clip(c) => self.data.clip(&old.clips[c].name).map(Playing::Clip),
                Playing::State(s) => old
                    .graph
                    .as_ref()
                    .and_then(|g| self.graph.as_ref()?.state(&g.states[s].name))
                    .map(|s| Playing::State(s.0)),
            };
            if let Some(what) = what {
                self.play = Some(Play { what, ..play });
            }
        }
        self.update(0.0);
    }

    /// What plays, for saves: the clip's or state's name (and whether it
    /// is a state), its normalized time and whether it loops.
    pub(crate) fn playing(&self) -> Option<(bool, &str, f32, bool)> {
        let play = self.play?;
        Some(match play.what {
            Playing::Clip(c) => (false, &self.data.clips[c].name, play.time as f32, play.looping),
            Playing::State(s) => (
                true,
                &self.graph.as_ref()?.states[s].name,
                play.time as f32,
                play.looping,
            ),
        })
    }

    /// Restores what [`playing`](Self::playing) saved, without a fade.
    /// Returns `false` if the clip or state isn't there.
    pub(crate) fn restore(&mut self, state: bool, name: &str, time: f32, looping: bool) -> bool {
        let what = if state {
            self.graph
                .as_ref()
                .and_then(|g| g.state(name))
                .map(|s| Playing::State(s.0))
        } else {
            self.data.clip(name).map(Playing::Clip)
        };
        let Some(what) = what else {
            return false;
        };
        let looping = match what {
            Playing::State(s) => self.graph.as_ref().expect("a graph").states[s].looping,
            Playing::Clip(_) => looping,
        };
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        self.play = Some(Play {
            what,
            time: f64::from(time),
            looping,
        });
        self.fade = None;
        self.pending = None;
        true
    }

    /// The data it plays.
    #[must_use]
    pub fn data(&self) -> &Arc<SkeletonData> {
        &self.data
    }

    /// Plays clip `name` from its start, looping, cutting off what played.
    /// Returns `false` (and changes nothing) if there is no such clip.
    /// Takes effect at the next [`update`](Self::update).
    pub fn play(&mut self, name: &str) -> bool {
        self.crossfade(name, 0.0)
    }

    /// Plays clip `name` once from its start, then holds its last pose
    /// ([`finished`](Self::finished) turns true).
    pub fn play_once(&mut self, name: &str) -> bool {
        self.command(name, 0.0, false)
    }

    /// Fades from what plays now to clip `name` (looping) over `duration`
    /// seconds. Returns `false` if there is no such clip.
    ///
    /// # Panics
    /// If `duration` is negative or not a number.
    pub fn crossfade(&mut self, name: &str, duration: f32) -> bool {
        self.command(name, duration, true)
    }

    fn command(&mut self, name: &str, duration: f32, looping: bool) -> bool {
        assert!(
            duration.is_finite() && duration >= 0.0,
            "ponko: a crossfade's duration must be 0 or more"
        );
        match self.data.clip(name) {
            Some(c) => {
                self.pending = Some((Playing::Clip(c), duration, 0.0, looping));
                true
            }
            None => false,
        }
    }

    /// Switches the graph to `state` (its normalized time `offset`),
    /// fading over `duration` seconds.
    ///
    /// # Panics
    /// Without a graph, for a state that isn't in it, or a negative or
    /// non-number duration or offset;
    /// [`try_crossfade_state`](Self::try_crossfade_state) says why instead.
    pub fn crossfade_state(&mut self, state: StateId, duration: f32, offset: f32) {
        self.try_crossfade_state(state, duration, offset)
            .unwrap_or_else(|e| panic!("ponko: invalid state change: {e}"));
    }

    /// [`crossfade_state`](Self::crossfade_state), or why not:
    /// [`UnknownState`](AnimationGraphError::UnknownState) for a state not in
    /// the graph (or without one),
    /// [`InvalidPlayback`](AnimationGraphError::InvalidPlayback) for a
    /// negative or non-finite duration or offset. Nothing changes on an error.
    /// ```
    /// use ponko::anim::{AnimationGraphError, StateId};
    /// use ponko::skeleton2d::{Skeleton, SkeletonData};
    /// let mut s = Skeleton::new(std::sync::Arc::new(SkeletonData::new()));
    /// assert_eq!(
    ///     s.try_crossfade_state(StateId(0), 0.2, 0.0),
    ///     Err(AnimationGraphError::UnknownState(StateId(0))),
    ///     "no graph, no states"
    /// );
    /// ```
    ///
    /// # Errors
    /// As above.
    pub fn try_crossfade_state(
        &mut self,
        state: StateId,
        duration: f32,
        offset: f32,
    ) -> Result<(), AnimationGraphError> {
        let states = self.graph.as_ref().map_or(0, |g| g.states.len());
        check_command(states, state, duration, offset)?;
        let looping = self.graph.as_ref().is_some_and(|g| g.states[state.0].looping);
        self.pending = Some((Playing::State(state.0), duration, offset, looping));
        Ok(())
    }

    /// Jumps the graph to `state` at normalized time `time`, cutting off
    /// what played, or says why not (as
    /// [`try_crossfade_state`](Self::try_crossfade_state)).
    ///
    /// # Errors
    /// [`UnknownState`](AnimationGraphError::UnknownState) or
    /// [`InvalidPlayback`](AnimationGraphError::InvalidPlayback).
    pub fn try_play_state(&mut self, state: StateId, time: f32) -> Result<(), AnimationGraphError> {
        self.try_crossfade_state(state, 0.0, time)
    }

    /// The state the graph is in (`None` without a graph, or while a clip
    /// played with [`play`](Self::play) is on).
    #[must_use]
    pub fn state(&self) -> Option<StateId> {
        match self.play?.what {
            Playing::State(s) => Some(StateId(s)),
            Playing::Clip(_) => None,
        }
    }

    /// The clip playing with [`play`](Self::play) or as a state's only
    /// clip, by name.
    #[must_use]
    pub fn clip(&self) -> Option<&str> {
        let c = match self.play?.what {
            Playing::Clip(c) => c,
            Playing::State(s) => match self.graph.as_ref()?.states[s].motion {
                SkeletonMotion::Clip(c) => c,
                SkeletonMotion::Blend1D { .. } => return None,
            },
        };
        Some(&self.data.clips[c].name)
    }

    /// How far it is: 0 at the start, 1 at the end of the first loop,
    /// 2.5 half way through the third.
    #[must_use]
    ///
    /// An `f32`: after hours in one looping clip its fraction steps
    /// coarsely; the skeleton keeps the clock in `f64`, so poses, events
    /// and exit times stay exact, and
    /// [`normalized_time_f64`](Self::normalized_time_f64) reads it in full.
    pub fn normalized_time(&self) -> f32 {
        self.normalized_time_f64() as f32
    }

    /// [`normalized_time`](Self::normalized_time) in full precision: whole
    /// loops plus the fraction of the current one.
    #[must_use]
    pub fn normalized_time_f64(&self) -> f64 {
        self.play.map_or(0.0, |p| p.time)
    }

    /// Sets how far it is (see [`normalized_time`](Self::normalized_time)).
    pub fn set_normalized_time(&mut self, time: f32) {
        if let Some(p) = &mut self.play
            && time.is_finite()
        {
            p.time = f64::from(time.max(0.0));
        }
    }

    /// Whether it loops (a clip played with [`play`](Self::play) or a
    /// looping state).
    #[must_use]
    pub fn looping(&self) -> bool {
        self.play.is_some_and(|p| p.looping)
    }

    /// `true` once something played once reached its end.
    #[must_use]
    pub fn finished(&self) -> bool {
        self.play.is_some_and(|p| !p.looping && p.time >= 1.0)
    }

    /// Whether a crossfade is going on.
    #[must_use]
    pub fn fading(&self) -> bool {
        self.fade.is_some()
    }

    /// Its graph's parameter named `name`.
    #[must_use]
    pub fn parameter_id(&self, name: &str) -> Option<ParamId> {
        self.graph.as_ref()?.parameter(name)
    }

    /// A parameter's value, `None` for one that isn't there.
    #[must_use]
    pub fn parameter(&self, p: ParamId) -> Option<AnimationParameter> {
        self.parameters.get(p.0).copied()
    }

    /// Sets a float parameter.
    ///
    /// # Panics
    /// If `p` isn't a float parameter or `value` isn't a number;
    /// [`try_set_float`](Self::try_set_float) says why instead.
    pub fn set_float(&mut self, p: ParamId, value: f32) {
        self.try_set_float(p, value).unwrap_or_else(|e| panic!("ponko: {e}"));
    }

    /// Sets a float parameter, or says why not:
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for one
    /// that isn't a float parameter (or isn't there),
    /// [`NonFiniteValue`](AnimationGraphError::NonFiniteValue) for NaN or
    /// infinity. Nothing changes on an error.
    ///
    /// # Errors
    /// As above.
    pub fn try_set_float(&mut self, p: ParamId, value: f32) -> Result<(), AnimationGraphError> {
        set_float(&mut self.parameters, p, value)
    }

    /// Sets a bool parameter.
    ///
    /// # Panics
    /// If `p` isn't a bool parameter; [`try_set_bool`](Self::try_set_bool)
    /// says why instead.
    pub fn set_bool(&mut self, p: ParamId, value: bool) {
        self.try_set_bool(p, value).unwrap_or_else(|e| panic!("ponko: {e}"));
    }

    /// Sets a bool parameter, or
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for one
    /// that isn't a bool parameter (or isn't there).
    ///
    /// # Errors
    /// As above.
    pub fn try_set_bool(&mut self, p: ParamId, value: bool) -> Result<(), AnimationGraphError> {
        set_bool(&mut self.parameters, p, value)
    }

    /// Sets or clears a trigger (a transition that uses it clears it).
    ///
    /// # Panics
    /// If `p` isn't a trigger; [`try_set_trigger`](Self::try_set_trigger)
    /// says why instead.
    pub fn set_trigger(&mut self, p: ParamId, value: bool) {
        self.try_set_trigger(p, value).unwrap_or_else(|e| panic!("ponko: {e}"));
    }

    /// Sets or clears a trigger, or
    /// [`WrongParameterKind`](AnimationGraphError::WrongParameterKind) for one
    /// that isn't a trigger (or isn't there).
    ///
    /// # Errors
    /// As above.
    pub fn try_set_trigger(&mut self, p: ParamId, value: bool) -> Result<(), AnimationGraphError> {
        set_trigger(&mut self.parameters, p, value)
    }

    /// Takes every event that fired since the last call, oldest first.
    pub fn drain_events(&mut self) -> impl Iterator<Item = Arc<str>> + '_ {
        self.events.drain(..)
    }

    /// Takes the oldest event that fired and wasn't taken yet, if any (for
    /// reading them one at a time, as the C API does).
    pub fn next_event(&mut self) -> Option<Arc<str>> {
        (!self.events.is_empty()).then(|| self.events.remove(0))
    }

    /// The bone named `name`.
    #[must_use]
    pub fn bone(&self, name: &str) -> Option<usize> {
        self.data.bone(name)
    }

    /// Bone `bone`'s pose relative to its parent, as last posed.
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    #[must_use]
    pub fn bone_pose(&self, bone: usize) -> BonePose {
        self.pose[bone]
    }

    /// Where bone `bone`'s origin is, in the skeleton's space (before the
    /// entity's transform).
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    #[must_use]
    pub fn bone_position(&self, bone: usize) -> Vec2 {
        self.world[bone].translation
    }

    /// Which way bone `bone` points (its +x axis), in radians in the
    /// skeleton's space.
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    #[must_use]
    pub fn bone_rotation(&self, bone: usize) -> f32 {
        let x = self.world[bone].matrix2.x_axis;
        x.y.atan2(x.x)
    }

    /// Point `local` of bone `bone`'s space in the skeleton's space: the
    /// tip of a bone of length 20 is `bone_point(b, vec2(20.0, 0.0))`.
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    #[must_use]
    pub fn bone_point(&self, bone: usize, local: Vec2) -> Vec2 {
        self.world[bone].transform_point2(local)
    }

    /// Bone `bone`'s matrix in the skeleton's space.
    pub(crate) fn bone_matrix(&self, bone: usize) -> Affine2 {
        self.world[bone]
    }

    /// Sets bone `bone`'s pose relative to its parent and updates the
    /// bones below it (for procedural poses after the update; the next
    /// update samples clips over it).
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    pub fn set_bone_pose(&mut self, bone: usize, pose: BonePose) {
        self.pose[bone] = pose;
        self.update_world(bone);
    }

    /// Shows skin `name`. Returns `false` (and changes nothing) if there
    /// is no such skin.
    pub fn set_skin(&mut self, name: &str) -> bool {
        match self.data.skin(name) {
            Some(s) => {
                self.skin = s;
                for shown in &mut self.shown {
                    shown.0 = None;
                    shown.1 = None;
                }
                self.pose_slots();
                true
            }
            None => false,
        }
    }

    /// The skin it shows.
    #[must_use]
    pub fn skin(&self) -> &str {
        &self.data.skins[self.skin]
    }

    /// Makes slot `slot` show attachment `name` (`Some(None)`: nothing)
    /// whatever clips key, or with `None` lets clips and the rest pose
    /// decide again.
    ///
    /// # Panics
    /// If `slot` isn't a slot.
    pub fn set_attachment(&mut self, slot: usize, name: Option<Option<&str>>) {
        self.overrides[slot] = name.map(|n| n.map(Into::into));
        self.pose_slots();
    }

    /// The attachment slot `slot` shows, by name.
    ///
    /// # Panics
    /// If `slot` isn't a slot.
    #[must_use]
    pub fn attachment(&self, slot: usize) -> Option<&str> {
        if !self.visible[slot] {
            return None;
        }
        self.shown[slot].0.as_deref()
    }

    /// The slots in the order they are drawn, bottom to top.
    #[must_use]
    pub fn draw_order(&self) -> &[u16] {
        &self.draw_order
    }

    /// Slot `slot`'s tint as last posed.
    pub(crate) fn slot_color(&self, slot: usize) -> Color {
        self.colors[slot]
    }

    /// The data index of what slot `slot` draws, if anything.
    pub(crate) fn shown(&self, slot: usize) -> Option<usize> {
        if self.visible[slot] { self.shown[slot].1 } else { None }
    }

    /// Makes IK constraint `ik` reach for `target` (in the skeleton's
    /// space) instead of its target bone; `None` goes back to the bone.
    ///
    /// # Panics
    /// If `ik` isn't a constraint of the data.
    pub fn set_ik_target(&mut self, ik: usize, target: Option<Vec2>) {
        self.ik_targets[ik] = target.filter(|t| t.is_finite());
    }

    /// Sets how much IK constraint `ik` applies, 0 to 1.
    ///
    /// # Panics
    /// If `ik` isn't a constraint of the data.
    pub fn set_ik_mix(&mut self, ik: usize, mix: f32) {
        self.ik_mix[ik] = if mix.is_finite() { mix.clamp(0.0, 1.0) } else { 0.0 };
    }

    /// Turns bone `bone` so its +x axis points at `target` (skeleton
    /// space), `mix` of the way (0 to 1), and updates the bones below it.
    /// Exact with uniform scales; a mirrored parent turns it the mirrored way.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::{BonePose, Skeleton, SkeletonData};
    /// let mut data = SkeletonData::new();
    /// let head = data.add_bone("head", None, BonePose::IDENTITY, 10.0);
    /// let mut s = Skeleton::new(std::sync::Arc::new(data));
    /// s.look_at(head, vec2(0.0, 50.0), 1.0);
    /// assert!((s.bone_rotation(head) - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    /// ```
    ///
    /// # Panics
    /// If `bone` isn't a bone.
    pub fn look_at(&mut self, bone: usize, target: Vec2, mix: f32) {
        let mix = clamp_mix(mix);
        let from = self.world[bone].translation;
        if mix == 0.0 || !target.is_finite() || (target - from).length_squared() < 1e-12 {
            return;
        }
        let want = (target - from).y.atan2((target - from).x);
        self.turn(bone, angle_delta(self.bone_rotation(bone), want) * mix);
    }

    /// Bends `upper` and its child `lower` so the tip of `lower` (its
    /// length along +x) reaches `target` (skeleton space), or comes as
    /// close as the bones' lengths allow, `mix` of the way. `bend_positive`:
    /// the joint between them turns clockwise on screen; else the other
    /// way. Only rotations change. Exact with uniform scales.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::skeleton2d::{BonePose, Skeleton, SkeletonData};
    /// let mut data = SkeletonData::new();
    /// let thigh = data.add_bone("thigh", None, BonePose::IDENTITY, 10.0);
    /// let shin = data.add_bone("shin", Some(thigh), BonePose::at(vec2(10.0, 0.0)), 10.0);
    /// let mut s = Skeleton::new(std::sync::Arc::new(data));
    /// s.two_bone_ik(thigh, shin, vec2(12.0, 5.0), true, 1.0);
    /// assert!(s.bone_point(shin, vec2(10.0, 0.0)).distance(vec2(12.0, 5.0)) < 1e-3);
    /// ```
    ///
    /// # Panics
    /// If a bone isn't there or `lower`'s parent isn't `upper`.
    pub fn two_bone_ik(&mut self, upper: usize, lower: usize, target: Vec2, bend_positive: bool, mix: f32) {
        assert!(
            self.data.bones[lower].parent == Some(upper),
            "ponko: two-bone IK needs the second bone to be a child of the first"
        );
        let mix = clamp_mix(mix);
        if mix == 0.0 || !target.is_finite() {
            return;
        }
        let a = self.world[upper].translation;
        let b = self.world[lower].translation;
        let tip = |s: &Self| s.world[lower].transform_point2(Vec2::new(s.data.bones[lower].length, 0.0));
        let l1 = a.distance(b);
        let l2 = b.distance(tip(self));
        let to = target - a;
        let d = to.length();
        if l1 < 1e-6 || d < 1e-6 {
            return;
        }
        let reach = d.clamp((l1 - l2).abs(), l1 + l2);
        let cos = ((l1 * l1 + reach * reach - l2 * l2) / (2.0 * l1 * reach)).clamp(-1.0, 1.0);
        let base = to.y.atan2(to.x);
        let upper_angle = if bend_positive {
            base - cos.acos()
        } else {
            base + cos.acos()
        };
        let now = (b - a).y.atan2((b - a).x);
        self.turn(upper, angle_delta(now, upper_angle) * mix);
        if l2 > 1e-6 {
            let b = self.world[lower].translation;
            let want = (target - b).y.atan2((target - b).x);
            let t = tip(self) - b;
            self.turn(lower, angle_delta(t.y.atan2(t.x), want) * mix);
        }
    }

    /// Turns `bone` by `angle` radians in the skeleton's space.
    fn turn(&mut self, bone: usize, angle: f32) {
        let mirrored = self.data.bones[bone]
            .parent
            .is_some_and(|p| self.world[p].matrix2.determinant() < 0.0);
        self.pose[bone].rotation += if mirrored { -angle } else { angle };
        self.update_world(bone);
    }

    /// Recomputes the skeleton-space matrices of `from` and the bones
    /// after it (children always come after their parents).
    fn update_world(&mut self, from: usize) {
        for i in from..self.world.len() {
            let local = self.pose[i].matrix();
            self.world[i] = match self.data.bones[i].parent {
                Some(p) => self.world[p] * local,
                None => local,
            };
        }
    }

    /// Advances `dt` seconds (0 or more): applies a queued
    /// [`play`](Self::play) or crossfade, or takes the graph's first
    /// matching transition, moves the clock, collects events, samples the
    /// clips (blending fades and 1D blends), sets slots and draw order,
    /// and applies the IK constraints.
    ///
    /// # Panics
    /// If `dt` is negative or not a number.
    pub fn update(&mut self, dt: f32) {
        assert!(dt.is_finite() && dt >= 0.0, "ponko: dt must be 0 or more");
        let mut command = self.pending.take();
        if command.is_none()
            && let (
                Some(graph),
                Some(Play {
                    what: Playing::State(state),
                    time,
                    looping,
                }),
            ) = (&self.graph, self.play)
        {
            let play = Play {
                what: Playing::State(state),
                time,
                looping,
            };
            let next = self.advance(play, dt).time;
            let selected = select_transition(&graph.transitions, &self.parameters, state, |exit| {
                next >= f64::from(exit)
            });
            if let Some(i) = selected {
                let graph = graph.clone();
                let t = &graph.transitions[i];
                consume_triggers(&mut self.parameters, t);
                command = Some((
                    Playing::State(t.to.0),
                    t.duration,
                    t.offset,
                    graph.states[t.to.0].looping,
                ));
            }
        }
        if let Some((what, duration, offset, looping)) = command {
            let new = Play {
                what,
                time: f64::from(if looping { offset } else { offset.min(1.0) }),
                looping,
            };
            self.fade = match (self.play, duration > 0.0) {
                (Some(from), true) => Some(Fade {
                    // A fade that interrupts a fade starts from what was
                    // the stronger side.
                    from: match self.fade {
                        Some(f) if f.elapsed < f.duration * 0.5 => f.from,
                        _ => from,
                    },
                    elapsed: 0.0,
                    duration,
                }),
                _ => None,
            };
            self.play = Some(new);
        }
        if let Some(play) = self.play {
            let next = self.advance(play, dt);
            self.collect_events(play.what, play.time, next.time, play.looping);
            self.play = Some(next);
        }
        if let Some(mut fade) = self.fade.take() {
            fade.elapsed += dt;
            fade.from = self.advance(fade.from, dt);
            if fade.elapsed < fade.duration {
                self.fade = Some(fade);
            }
        }
        self.pose_bones();
        self.pose_slots();
        self.solve();
    }

    /// `play` advanced by `dt` seconds.
    fn advance(&self, play: Play, dt: f32) -> Play {
        let (duration, speed) = self.duration_and_speed(play.what);
        let mut time = if duration > 0.0 {
            play.time + f64::from(dt * speed / duration)
        } else {
            1.0
        };
        if !time.is_finite() {
            time = play.time;
        }
        if !play.looping {
            time = time.min(1.0);
        }
        Play { time, ..play }
    }

    /// How long one loop of `what` is, in seconds, and its speed.
    fn duration_and_speed(&self, what: Playing) -> (f32, f32) {
        match what {
            Playing::Clip(c) => (self.data.clips[c].duration, self.speed),
            Playing::State(s) => {
                let state = &self.graph.as_ref().expect("a graph").states[s];
                let duration = match &state.motion {
                    SkeletonMotion::Clip(c) => self.data.clips[*c].duration,
                    SkeletonMotion::Blend1D { parameter, children } => {
                        let (a, b, f) = blend_children(children, float(&self.parameters, *parameter));
                        let da = self.data.clips[a].duration;
                        da + (self.data.clips[b].duration - da) * f
                    }
                };
                (duration, state.speed * self.speed)
            }
        }
    }

    /// The clip that decides slots, draw order and events for `what`
    /// (a blend's stronger side), and where in it `time` is, in seconds.
    fn dominant(&self, what: Playing, time: f64, looping: bool) -> (usize, f32) {
        let clip = match what {
            Playing::Clip(c) => c,
            Playing::State(s) => match &self.graph.as_ref().expect("a graph").states[s].motion {
                SkeletonMotion::Clip(c) => *c,
                SkeletonMotion::Blend1D { parameter, children } => {
                    let (a, b, f) = blend_children(children, float(&self.parameters, *parameter));
                    if f > 0.5 { b } else { a }
                }
            },
        };
        (clip, phase(time, looping) * self.data.clips[clip].duration)
    }

    fn collect_events(&mut self, what: Playing, old: f64, new: f64, looping: bool) {
        if new <= old {
            return;
        }
        let (clip, _) = self.dominant(what, old, looping);
        let events = &self.data.clips[clip].events;
        if events.is_empty() {
            return;
        }
        let (start, end) = (old, new);
        let mut cycle = if looping { start.floor() } else { 0.0 };
        while cycle <= end.floor() && self.events.len() < MAX_EVENTS {
            for e in events {
                let at = cycle + f64::from(e.time);
                if at > start && at <= end && self.events.len() < MAX_EVENTS {
                    self.events.push(e.name.clone());
                }
            }
            if !looping {
                break;
            }
            cycle += 1.0;
        }
    }

    /// Samples what plays (and what it fades from) into `pose`.
    fn pose_bones(&mut self) {
        let data = self.data.clone();
        let rest_pose = |out: &mut Vec<BonePose>| {
            for (o, b) in out.iter_mut().zip(&data.bones) {
                *o = b.rest;
            }
        };
        let mut pose = std::mem::take(&mut self.pose);
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut blend = std::mem::take(&mut self.blend);
        rest_pose(&mut pose);
        if let Some(play) = self.play {
            self.sample(play, &mut pose, &mut scratch);
        }
        if let Some(fade) = self.fade {
            rest_pose(&mut blend);
            self.sample(fade.from, &mut blend, &mut scratch);
            let w = 1.0 - (fade.elapsed / fade.duration).clamp(0.0, 1.0);
            for (p, b) in pose.iter_mut().zip(&blend) {
                *p = p.lerp(*b, w);
            }
        }
        self.pose = pose;
        self.scratch = scratch;
        self.blend = blend;
    }

    /// Samples `play` into `out` (already at rest); `scratch` is space for
    /// a blend's second clip.
    fn sample(&self, play: Play, out: &mut [BonePose], scratch: &mut [BonePose]) {
        let data = &self.data;
        let at = |clip: usize| phase(play.time, play.looping) * data.clips[clip].duration;
        let clip = match play.what {
            Playing::Clip(c) => c,
            Playing::State(s) => match &self.graph.as_ref().expect("a graph").states[s].motion {
                SkeletonMotion::Clip(c) => *c,
                SkeletonMotion::Blend1D { parameter, children } => {
                    let (a, b, f) = blend_children(children, float(&self.parameters, *parameter));
                    data.clips[a].sample_bones(at(a), &data.bones, out);
                    if f > 0.0 && b != a {
                        for (o, bone) in scratch.iter_mut().zip(&data.bones) {
                            *o = bone.rest;
                        }
                        data.clips[b].sample_bones(at(b), &data.bones, scratch);
                        for (o, s) in out.iter_mut().zip(scratch.iter()) {
                            *o = o.lerp(*s, f);
                        }
                    }
                    return;
                }
            },
        };
        data.clips[clip].sample_bones(at(clip), &data.bones, out);
    }

    /// Sets each slot's attachment, visibility and color, and the draw
    /// order, from the rest pose, the playing clip's keys and the game's
    /// overrides.
    fn pose_slots(&mut self) {
        let data = self.data.clone();
        let clip = self.play.map(|p| self.dominant(p.what, p.time, p.looping));
        for (i, slot) in data.slots.iter().enumerate() {
            let mut name = slot.attachment.clone();
            let mut visible = true;
            let mut color = slot.color;
            if let Some((c, seconds)) = clip
                && let Some(track) = data.clips[c].slots.iter().find(|t| t.slot == i)
            {
                if let Some(n) = step(&track.attachment, seconds) {
                    name.clone_from(n);
                }
                if let Some(v) = step(&track.visible, seconds) {
                    visible = *v;
                }
                if let Some(c) = sample(&track.color, seconds) {
                    color = c;
                }
            }
            if let Some(o) = &self.overrides[i] {
                name.clone_from(o);
            }
            let shown = &mut self.shown[i];
            let same = match (&shown.0, &name) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b) || a == b,
                (None, None) => true,
                _ => false,
            };
            if !same || (name.is_some() && shown.1.is_none()) {
                shown.1 = name.as_deref().and_then(|n| data.find(self.skin, i, n));
                shown.0 = name;
            }
            self.visible[i] = visible;
            self.colors[i] = color;
        }
        let order = clip.and_then(|(c, seconds)| step(&data.clips[c].draw_order, seconds).cloned().flatten());
        match order {
            Some(order) => self.draw_order.copy_from_slice(&order),
            None => {
                for (i, o) in self.draw_order.iter_mut().enumerate() {
                    *o = i as u16;
                }
            }
        }
    }

    /// Skeleton-space matrices from the pose, then the IK constraints.
    fn solve(&mut self) {
        self.update_world(0);
        let data = self.data.clone();
        for (i, ik) in data.ik.iter().enumerate() {
            let target = self.ik_targets[i].unwrap_or(self.world[ik.target].translation);
            match ik.bones[..] {
                [bone] => self.look_at(bone, target, self.ik_mix[i]),
                [upper, lower] => self.two_bone_ik(upper, lower, target, ik.bend_positive, self.ik_mix[i]),
                _ => {}
            }
        }
    }
}

fn clamp_mix(mix: f32) -> f32 {
    if mix.is_finite() { mix.clamp(0.0, 1.0) } else { 0.0 }
}

/// Where in one loop normalized `time` is, 0..1.
fn phase(time: f64, looping: bool) -> f32 {
    if looping {
        time.rem_euclid(1.0) as f32
    } else {
        time.clamp(0.0, 1.0) as f32
    }
}

fn float(parameters: &[AnimationParameter], p: ParamId) -> f32 {
    crate::anim::graph::float_parameter(parameters, p)
}

/// The two clips of a 1D blend around `value` and how far toward the second.
pub(crate) fn blend_children(children: &[(f32, usize)], value: f32) -> (usize, usize, f32) {
    let hi = children.partition_point(|(t, _)| *t <= value);
    if hi == 0 {
        (children[0].1, children[0].1, 0.0)
    } else if hi == children.len() {
        (children[hi - 1].1, children[hi - 1].1, 0.0)
    } else {
        let (t0, a) = children[hi - 1];
        let (t1, b) = children[hi];
        (a, b, (value - t0) / (t1 - t0))
    }
}
