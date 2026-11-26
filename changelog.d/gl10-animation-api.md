### Breaking changes
- `ponko::anim::AnimationState` is `#[non_exhaustive]`: struct literals and
  `..AnimationState::new(..)` updates outside the engine no longer compile.
  Its fields stay public to read and change.
  Instead: start from `AnimationState::new(motion)` and chain `with_speed`,
  `with_speed_parameter`, `with_looping`, `with_event` or `with_events`.
  Before:
  ```rust
  let once = AnimationState {
      looping: false,
      ..AnimationState::new(Motion::Clip(clip))
  };
  ```
  After:
  ```rust
  use ponko::anim::{AnimationState, Motion};
  # let clip = todo!();
  let once = AnimationState::new(Motion::Clip(clip)).with_looping(false);
  ```
- `ponko::anim::MotionChild` is `#[non_exhaustive]`: struct literals outside
  the engine no longer compile.
  Instead: use `MotionChild::new(motion)` (time scale 1) and
  `with_time_scale` for another clock speed.
  Before:
  ```rust
  let child = MotionChild { motion: Motion::Clip(walk), time_scale: 1.5 };
  ```
  After:
  ```rust
  use ponko::anim::{Motion, MotionChild};
  # let walk = todo!();
  let child = MotionChild::new(Motion::Clip(walk)).with_time_scale(1.5);
  ```
- `ponko::anim::AnimationEvent` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `AnimationEvent::new(time, name)`; the name takes any
  `Into<Arc<str>>` (`&str`, `String`, `Arc<str>`).
  Before:
  ```rust
  state.events.push(AnimationEvent { time: 0.5, name: "step".into() });
  ```
  After:
  ```rust
  use ponko::anim::AnimationEvent;
  # let mut state: ponko::anim::AnimationState = todo!();
  state.events.push(AnimationEvent::new(0.5, "step"));
  ```
- `ponko::anim::Joint` is `#[non_exhaustive]`: struct literals outside the
  engine no longer compile.
  Instead: use `Joint::new(name, parent)` (`parent` is an index, or `None`
  for a root) and `with_rest` for its rest pose.
  Before:
  ```rust
  let knee = Joint { name: "knee".into(), parent: Some(0), rest: Transform::at(vec3(0.0, -0.4, 0.0)) };
  ```
  After:
  ```rust
  use ponko::anim::Joint;
  # use ponko::d3::Transform;
  let knee = Joint::new("knee", 0).with_rest(Transform::at(vec3(0.0, -0.4, 0.0)));
  ```
- `ponko::anim::JointAnimation` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `JointAnimation::new(joint, name)` and `with_translation`,
  `with_rotation` or `with_scale` for the tracks it has.
  Before:
  ```rust
  let knee = JointAnimation { joint: 1, name: "knee".into(), translation: None, rotation: Some(bend), scale: None };
  ```
  After:
  ```rust
  use ponko::anim::JointAnimation;
  # let bend: ponko::anim::AnimationTrack = todo!();
  let knee = JointAnimation::new(1, "knee").with_rotation(bend);
  ```
- `ponko::anim::MorphAnimation` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `MorphAnimation::new(name, track)`.
  Before:
  ```rust
  let smile = MorphAnimation { name: "smile".into(), track };
  ```
  After:
  ```rust
  use ponko::anim::MorphAnimation;
  # let track = todo!();
  let smile = MorphAnimation::new("smile", track);
  ```
- `ponko::anim::MorphTarget` is `#[non_exhaustive]`: struct literals and
  `..Default::default()` updates outside the engine no longer compile.
  Instead: use `MorphTarget::new(name, positions)` and `with_normals` when
  the normals change too.
  Before:
  ```rust
  mesh.morph_targets.push(MorphTarget { name: "raise".into(), positions: up, normals: Vec::new() });
  ```
  After:
  ```rust
  use ponko::anim::MorphTarget;
  # let mut mesh: ponko::render::MeshData = todo!();
  # let up = Vec::new();
  mesh.morph_targets.push(MorphTarget::new("raise", up));
  ```
- `ponko::anim::AttachToJoint` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile. Saved scenes and prefabs load as
  before.
  Instead: use `AttachToJoint::new(skeleton, joint)` and `with_offset`.
  Before:
  ```rust
  ctx.world.spawn((Model::new(cup), AttachToJoint { skeleton: person, joint: hand, offset }));
  ```
  After:
  ```rust
  use ponko::anim::AttachToJoint;
  # let (cup, person, hand, offset) = todo!();
  ctx.world.spawn((Model::new(cup), AttachToJoint::new(person, hand).with_offset(offset)));
  ```
- The `Motion::Blend1D` and `Motion::Blend2D` variants are
  `#[non_exhaustive]`: building them with variant literals outside the
  engine no longer compiles, and patterns need `..`.
  Instead: use `Motion::blend_1d(parameter, children)` and
  `Motion::blend_2d(x, y, children)`; match with `Motion::Blend1D { children, .. }`.
  Before:
  ```rust
  let tree = Motion::Blend1D { parameter: speed, children: vec![(0.0, walk), (1.0, run)] };
  ```
  After:
  ```rust
  use ponko::anim::Motion;
  # let (speed, walk, run) = todo!();
  let tree = Motion::blend_1d(speed, vec![(0.0, walk), (1.0, run)]);
  ```

### Added
- Fallible animation-data constructors for content from files and tools:
  `AnimationTrack::try_new`, `Skeleton::try_new`, `AnimationClip::try_new`
  and `AnimationClip::try_with_morph_tracks` return `ponko::Error`
  (`ErrorKind::Invalid`, or `ErrorKind::Limit` for more than 65536 joints)
  where the existing constructors panic. The panicking constructors stay and
  call them.
- Checked animator calls for ids from outside the code that built the graph
  (save files, scripts, the network): `Animator::get_layer_mut` returns an
  `Option`; `try_set_float`, `try_set_bool`, `try_set_trigger`, `try_play`
  and `try_crossfade` return an `AnimationGraphError` and change nothing when
  they refuse. `AnimationGraphError` has three new variants for them:
  `UnknownState`, `NonFiniteValue` and `InvalidPlayback`. The fast methods
  stay and document that they panic on a bad id.
- Builders for the extensible structs: `AnimationState::with_speed`,
  `with_speed_parameter`, `with_looping`, `with_event`, `with_events`;
  `MotionChild::new` and `with_time_scale`; `AnimationEvent::new`;
  `Joint::new` and `with_rest`; `JointAnimation::new`, `with_translation`,
  `with_rotation`, `with_scale`; `MorphAnimation::new`; `MorphTarget::new`
  and `with_normals`; `AttachToJoint::new` and `with_offset`;
  `Motion::blend_1d` and `Motion::blend_2d`.
- The guide's 3D chapter has an "Animation data from outside your code"
  section.

### Changed
- The animator's panicking setters and `play`/`crossfade` name the method,
  the id and the reason in their panic message (for example
  `Animator::set_float: parameter 9 is not a float parameter`).
- glTF animation import and the C API's skeleton, morph-track and animator
  calls use the checked versions, so bad data is an error, never a panic.
  The C animator calls' error messages now name the id (for example
  `parameter 2 is not a bool parameter`).
