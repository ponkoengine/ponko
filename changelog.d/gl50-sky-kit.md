### Added
- The sky kit: `ctx.scene3d.sky.kit = Some(SkyKit::default())` gives the
  physical atmosphere with raymarched clouds (`Clouds`: coverage, kind,
  wind, altitude, thickness, density, self-shadowing, silver lining), a
  moon with phases (`Moon`) and a star field that turns through the night,
  on the sky shader hook, so the fog, reflections, ambient light and
  reflection probes see it too. The clouds cast shadows on the ground (a
  small projected map) and dim the fog's glow toward the sun.
  `CloudQuality` picks `Low`, `Medium` or `High` (marched at a quarter of
  the frame's pixels with temporal reprojection; measured GPU times in its
  docs) or `Flat`, a 2D cloud layer that WebGL2-class GPUs always get.
  Nothing is made or drawn while `Sky::kit` is `None`. Rust-only for now.
- `DayNight` moves the sun and the moon through the day and the moon's
  phases: `day.update(ctx)` once a frame.
- Sky shaders can call `drawing_background()` to leave sharp detail out
  of the sky texture.
- The `sky_kit` example.
