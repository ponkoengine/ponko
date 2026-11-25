### Added
- `Lighting::shadow_sun_step` (radians, default 0, which keeps the exact
  direction as before): the sun's shadow cascades are drawn from a held sun
  direction that moves only once the true direction has turned past the
  step, so a sun that turns every frame (a day and night cycle) no longer
  makes every shadow edge shimmer, and far cascades stay cached between
  steps. Lighting keeps the exact direction. The hold has hysteresis, so a
  sun wobbling across a step boundary doesn't flip the shadows back and
  forth. From C and C++ it is `shadow_sun_step` of `PonkoLightingEx`.
