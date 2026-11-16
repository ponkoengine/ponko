### Added
- Versioned C descriptors for the 3D picture's remaining fixed structs, so
  every field their Rust structs have is reachable from C and C++:
  `PonkoPostEx` (`ponko_ctx_post_ex`, `ponko_ctx_set_post_ex`: split toning,
  depth of field and the sample count `msaa`, which can now change while the
  game runs), `PonkoParticleSettings3DEx`
  (`ponko_ctx_set_particle_emitter3d_ex`: `softness`, soft particles),
  `PonkoCamera3DEx` (`ponko_ctx_camera3d_ex`, `ponko_ctx_set_camera3d_ex`:
  `occlusion_culling` and `particles`) and `PonkoTargetCameraEx`
  (`ponko_ctx_set_target_cameras_ex`, `ponko_ctx_target_camera_ex`:
  `particles` per target camera). The ABI 2 structs and calls are unchanged.
- `Post::lerp` and `PostVolume::blend` from C and C++: `ponko_post_ex_lerp`
  and `ponko_post_blend_volumes` (a `PonkoPostVolume` is a box that points at
  its `PonkoPostEx`); C++ has `ponko::post_ex`, `post_lerp`, `post_volume`,
  `post_volume_around` and `blend_post_volumes`, `camera3d_ex`,
  `target_camera_ex`, `ctx::post_ex`, `ctx::camera3d_ex`,
  `ctx::target_camera_ex`, `ctx::set_target_cameras_ex` and
  `particle_settings::softness`.
- `ParticleEmitter::settings()` returns the settings an emitter was made with.
- `scripts/check-settings-fields.py` (run by `check-ffi-coverage.py`): every
  public field of a Rust settings struct has a field of the same name in its C
  struct or descriptor, a line naming where C sets it
  (`scripts/settings-fields.txt`), or a Rust-only entry with its reason.

### Changed
- The "Still Rust-only" list is shorter: split toning, depth of field,
  `Post::lerp`, `PostVolume`, `ParticleSettings::softness` and
  `Camera::particles` are reachable from C and C++. `ctx::set_particle_emitter3d`
  in C++ calls the versioned entry point.
