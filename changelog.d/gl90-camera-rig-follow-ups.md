### Added

- `camera::CameraTarget` (also in the prelude): put it on the entity a camera rig follows, keep the
  rig as a resource (`d2::FollowCamera`, `d3::ThirdPersonCamera`,
  `d3::FirstPersonCamera`, `d3::OrbitCamera`), and the new built-in system
  `systems::update_camera_rigs` (`Systems::UPDATE_CAMERA_RIGS`, at the end of
  `Phase::Animate`) updates the rig every frame. It cuts to a new target, leaves
  the camera alone without one, and the third-person rig avoids the 3D physics'
  walls but not the target's own collider. Saved with the world.
- `d2::FollowCamera::pixel_perfect` and `with_pixel_perfect`: the rig keeps the
  camera on a pixel-art grid itself.
- `d3::FirstPersonCamera::height`: the eye's height above the followed entity.
- First-person held items: `SceneSettings::view_model` (`d3::ViewModel`) and the
  `d3::HeldItem` component draw held models with their own camera and the depth
  range nearest the eye, so they never clip into walls. Off by default, and
  then free: no allocation, pass or draw (tested).
- C API: `ponko_first_person_camera_flat_forward`, `_flat_right`,
  `ponko_third_person_camera_flat_forward` and `_flat_right`.

### Changed

- The C++ rigs' `flat_forward` / `flat_right` call the C functions instead of
  working them out inline.
- Ember Hop, Shard Grove and the 2D and 3D tutorials follow their heroes with the
  engine-fed rigs and no camera system.
