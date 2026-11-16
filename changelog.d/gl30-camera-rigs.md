### Added
- Camera rigs, each a plain struct kept by the game and updated once a frame,
  moving the camera the same way at 30, 60 and 144 frames a second:
  `d2::FollowCamera` (dead zone, look-ahead, offset, level bounds),
  `d2::PixelPerfect` (a whole-number zoom for the window, a camera on the art
  pixel grid, the letterbox rectangle), `d3::OrbitCamera` (with
  `mouse_controls`), `d3::FirstPersonCamera` and `d3::ThirdPersonCamera`
  (follows a character and comes in front of walls: `update_with` for your own
  test, `update_with_physics` with the `physics3d` feature).
- `camera::CameraShake`: trauma-based shake on smooth noise, `apply_2d` in
  window pixels and `apply_3d` as small turns.
- `camera::smooth_damp`, `camera::damp` and `camera::damp_angle`: exact,
  frame-rate independent smoothing for `f32`, `Vec2`, `Vec3` and the like.
- `d3::Camera::world_to_screen`, `screen_to_world`, `screen_to_ground`,
  `right` and `up`; `d3::Camera::viewport` is public. The guide's "Screen and
  world" table lists every conversion for both cameras.
- C and C++: the rigs and the shake as versioned descriptors
  (`PonkoFollowCamera2D`, `PonkoPixelPerfect`, `PonkoCameraShake`,
  `PonkoOrbitCamera`, `PonkoFirstPersonCamera`, `PonkoThirdPersonCamera`, each
  with `_defaults` and update calls; `ponko::follow_camera2d` and the others in
  C++), `ponko_smooth_damp`, `ponko_damp`, `ponko_damp_angle` (and `_vec2`,
  `_vec3`), `ponko_ctx_world_to_screen`, `ponko_ctx_screen_to_world`,
  `ponko_ctx_world_to_screen3d`, `ponko_ctx_screen_to_world3d` and
  `ponko_ctx_screen_to_ground`.

### Changed
- Ember Hop's camera is a `FollowCamera` with `PixelPerfect`, and Shard Grove's
  a `ThirdPersonCamera` that no longer goes through walls, with an
  `OrbitCamera` on the title, in place of their hand-written camera code.
