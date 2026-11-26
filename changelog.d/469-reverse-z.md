### Breaking changes
- The 3D scene depth is reverse-Z: 1 at the camera's near plane, 0 at the far plane and the sky.
  Shaders that read `scene_depth_texture` directly (a material, screen effect or compute pass)
  see the depth flipped; positions and distances from `scene_position` and `scene_distance` are
  unchanged. Shadow maps keep standard depth.
  Instead: use `scene_position` or `scene_distance`, or treat a greater depth as nearer and 0 as
  the sky (standard depth `d` is `1.0 - d` now for the same near and far planes).
  Before:
  ```wgsl
  fn is_sky(texel: vec2<i32>) -> bool {
      return textureLoad(scene_depth_texture, texel, 0) >= 1.0;
  }
  ```
  After:
  ```wgsl
  fn is_sky(texel: vec2<i32>) -> bool {
      return textureLoad(scene_depth_texture, texel, 0) <= 0.0;
  }
  ```

### Changed
- The 3D camera draws with reverse-Z depth (a 32-bit float, near plane at 1, far plane at 0,
  `Greater` test, cleared to 0), so the depth precision hardly depends on `Camera3D::near`. A
  first-person camera can use `near = 0.01` with a far plane kilometres away: one depth step at
  400 m is now about 30 µm instead of about a metre, and ambient occlusion, contact shadows,
  reflections and temporal reprojection there no longer crawl. The world beyond the near plane
  draws the same at `near = 0.01` as at `0.1`. Every pass that reads the depth follows (ambient
  occlusion, contact shadows, reflections, temporal antialiasing, depth of field, the sky,
  soft particles, screen effects, global illumination's views, occlusion culling, target cameras'
  oblique clip planes). `Camera3D::far` still clips and culls.

### Fixed
- Temporal ambient occlusion no longer pulses on distant ground seen at a grazing angle with
  temporal antialiasing on: its history test allows for the sub-pixel jitter there instead of
  rejecting the history every frame.
