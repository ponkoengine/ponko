### Added
- 2D skeletal animation, `ponko::skeleton2d`: bone hierarchies with rest
  poses, rigid sprite parts on bones and skinned 2D meshes (up to four
  bones per vertex) drawn by the sprite renderer in the same batches and
  `z` order as sprites; clips keying bone translate/rotate/scale, slot
  attachments (sprite swaps), visibility, color, draw order and events;
  looping, crossfades, 1D blends and a state machine; skins; two-bone IK
  and look-at, as constraints or one-off calls. Free when unused.
- Skeleton files: a documented JSON format (`"format":
  "ponko-skeleton2d"`) and Spine JSON import (4.x and 3.x), loaded through
  the asset system with `Ctx::load_skeleton2d` / `try_load_skeleton2d`,
  with hot reload that keeps each skeleton's clip, clock, parameters and
  skin.
- Bezier curves on clip keys (`Curve::Bezier`, `Curve::Channels`,
  `Bezier`; `{ "bezier": [x1, y1, x2, y2] }` in the file), evaluated
  exactly. Spine's bezier keys import exactly as authored.
- Spine texture atlases (`SpineAtlas`, `SkeletonData::from_spine_json_with_atlas`):
  `load_skeleton2d` reads the `.atlas` beside a Spine export, with regions
  packed rotated or trimmed, premultiplied pages and nearest filtering;
  editing the atlas reloads the skeletons using it.
- Every Spine feature the importer doesn't read is a warning naming the
  feature, where it is and the file (`SkeletonData::warnings`, printed by
  `load_skeleton2d`), never a silently wrong pose.
- Checked state machine calls on `Skeleton`: `try_set_float`,
  `try_set_bool`, `try_set_trigger`, `try_crossfade_state` and
  `try_play_state` return an `AnimationGraphError` instead of panicking,
  sharing the 3D animator's checks.
- C and C++ API for 2D skeletons: `ponko_ctx_load_skeleton2d`,
  `ponko_ctx_set_skeleton2d` and the `ponko_ctx_skeleton2d_*` calls (play,
  crossfade, states, checked parameters, events, IK targets, bones, slots,
  skins, look, manual updates), `PONKO_SYSTEM_UPDATE_SKELETONS2D`, and the
  matching `ponko::ctx` methods in `ponko.hpp`.
- The `Skeleton` component is reflected as `Skeleton2D`, so saves, scenes,
  prefabs and the inspector keep an animated skeleton; loading a scene
  loads the skeleton files it names.
- The built-in system `update_skeletons2d` (`Systems::UPDATE_SKELETONS2D`)
  at the end of `Phase::Animate`.
- The state machine types in `ponko::anim` (`StateId`, `ParamId`,
  `AnimationParameter`, `AnimationCondition`, `AnimationTransition`,
  `AnimationEvent`, `AnimationGraphError`) are available without the `3d`
  feature; the 3D animator and 2D skeletons share them.
- The `skeleton2d` example, the guide chapter "2D skeletons", and the
  `skeleton2d/1k` benchmarks in `ponko_bench`.
- Clip rotation keys go straight from one value to the next (350° to 10°
  turns back 340°), as Spine 4 does; Spine 3.x imports keep 3.x's
  shorter-way turns.
