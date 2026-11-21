### Added
- `Post::ssr_thin_objects`: screen-space reflections keep thin things
  (rails, cables, poles, window frames) whole without TAA. The trace also
  reads a coarse copy of the depth (the nearest depth of 8 to 64 pixel
  blocks) along its long steps, which jumped over such things and left
  their reflections dotted. Off by default, and then the trace is the same
  shader as before; `GraphicsQuality::Ultra` turns it on. Offscreen tests
  at 1920x1080 without TAA: the nearest of four rails is reflected with
  0.87 of the light it shows itself, from 0.48, and every rail is reflected
  0.86 to 0.98 times as wide as a true mirror shows it. It costs about
  half a millisecond more per frame at 1920x1080 on an Apple M4 Max (the
  trace 0.80 to 0.98 ms, from 0.35 to 0.38; the guide has the table).
- C and C++: version 2 of `PonkoPostEx` (`PONKO_POST_EX_VERSION` 2) has
  `ssr_thin_objects`. Version 1 callers neither read nor change it. The
  ABI 2 `PonkoGraphicsQuality` has no field for it: setting a preset
  through it sets the field as the Rust preset does (on for ultra), and a
  custom struct leaves it as it is.
