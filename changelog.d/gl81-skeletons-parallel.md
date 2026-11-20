### Added
- 2D skeletons draw on all cores: skeletons are split into runs, each
  thread builds its run's vertices into its own chunk, and the chunks are
  copied into the draw list in draw order, so the output matches drawing
  them one by one (a test compares the two across thread counts). Drawing
  10,000 skeletons took 5.6 to 8.4 ms at best instead of 25.6 to 26.6 ms;
  these numbers come from a busy machine (medians within the noise) and are
  to be repeated on a quiet one: see the guide's "2D skeletons", "Cost".
  Posing, drawing and clipping allocate nothing per frame once warm.
- `ponko::skeleton2d` shear (`BonePose::shear`, `BonePose::with_shear`,
  `BoneTrack::shear`), one-axis bone timelines (`ChannelTrack`,
  `BoneChannel`), mesh deform keys (`DeformTrack`, `Clip::with_deform_track`),
  transform constraints (`TransformConstraint`,
  `SkeletonData::add_transform_constraint`, in Spine's four modes, applied
  in order with IK constraints) and clipping masks (`ClippingMask`, an
  `Attachment::Clipping` that clips the slots after it up to an end slot,
  concave polygons included).
- Spine JSON import reads them: bone shear, the 4.x one-axis timelines
  (`translatex` ... `sheary`), shear timelines, deform timelines (4.x
  `attachments`, 3.x `deform`, weighted meshes included), transform
  constraints (4.x and 3.x mix names) and clipping attachments. Path and
  physics constraints still warn, now with the reason they are left out.
- C and C++: one-off `ponko_ctx_skeleton2d_look_at` /
  `ponko_ctx_skeleton2d_two_bone_ik` (`Ctx::skeleton2d_look_at`,
  `Ctx::skeleton2d_two_bone_ik`).
- Benchmarks `skeleton2d/update-1k`, `draw-1k`, `update-10k` and
  `draw-10k` time posing and drawing on their own.

### Changed
- `skeleton2d::BonePose` is `#[non_exhaustive]` with a `shear` field; build
  one with `BonePose::new`, `BonePose::at` or `BonePose::IDENTITY`.
- `scripts/check-ffi-coverage.py` no longer counts a 3D function (such as
  `ponko_ctx_two_bone_ik`) as the binding of a 2D method with the same name.
