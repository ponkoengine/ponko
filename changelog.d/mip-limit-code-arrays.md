### Fixed
- The texture mip limit no longer shrinks texture arrays made from pixels in
  code (`create_texture_array`), only arrays loaded from image files
  (`load_texture_array`) and model images. Code-made arrays are often data a
  shader reads at the size it was made (texel coordinates worked out from
  that size, packed tables): halving them moved what those shaders read.
  The engine's own terrain-file and water-map arrays are made this way too.
- A target camera drawing for the first time starts from a fresh frame
  instead of taking another target's kept frame (which may have collected an
  icon's separate world or another camera), and kept frames are dropped
  when their render target is freed rather than by count.
- `Camera3D::screen_ray` works its direction out from the projection's slopes
  and the camera's rotation instead of unprojecting a depth. With reverse-Z,
  depth 0.5 is about two near planes from the camera, so a centimetre near
  plane far from the origin put the direction tenths of a degree off and
  made it wobble as the camera turned (0.055° off in the new test).
- The water kit's under-water view builds its view ray through the far plane
  instead of depth 0.5 (about two near planes from the camera under
  reverse-Z), so it holds steady with a small near plane; it and the sky's
  far direction stay homogeneous, so an infinite far plane works too.
