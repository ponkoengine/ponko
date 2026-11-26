<!-- area: Rendering -->
### Added
- Waves from a spectrum for the water kit (issue #124): `Waves::spectrum`
  with `WaveSpectrum` (wind, significant height, a repeating square of 64,
  128 or 256 cells, choppiness, seed) draws an FFT ocean. Compute passes
  work it out on the GPU; without compute shaders (WebGL2) the CPU runs the
  same FFT and uploads it, and `Water::height_at`, `Water::sample` and
  buoyancy query the same field. It adds 0.6 to 1.0 ms of GPU time over the
  Gerstner sea at 1280x720; the water guide has the costs and the advice on
  cells. `SPECTRUM_PERIOD` is how often the sea repeats (200 s).
- `CloudQuality::interleaved` says whether a cloud preset marches a quarter
  of its cloud image's pixels a frame.

### Changed
- The sky kit's `Low` and `Medium` cloud presets march one pixel of each
  2 x 2 block of the cloud image a frame, in turn, and keep the reprojected
  history for the rest. At 1920x1080 on an Apple M4 Max, `Medium` (the
  default) now adds 1.43 ms to a frame instead of 2.87 ms (medians of ten
  runs; 1.1 to 1.65 ms saved in every run). Clouds moving fast across the
  screen are a little softer for a few frames.
- Contact ripples work on WebGL2: without compute shaders the CPU steps the
  ripple simulation and uploads it, so `Water::touch` now ripples the water
  there too.
- The terrain example (`examples/terrain`) draws its lake with the water
  kit (a planar mirror and the underwater view) instead of its own shaders,
  and an offscreen GPU test covers its scene.
