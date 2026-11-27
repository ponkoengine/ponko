### Changed
- The water kit's waves from a spectrum work out their CPU field (the
  WebGL2 upload, `Water::height_at`, `Water::sample` and buoyancy) with
  rustfft and each wave's speed worked out once, instead of a plain
  radix-2 FFT and a square root and sine per texel each moment. On an Apple M4 Max, busy with other work, one new moment at 256
  cells takes 0.58 ms instead of 2.9 ms (0.9 to 1.2 ms instead of 4.3 to
  9 ms with the sway's speed), and 0.13 instead of 0.63 ms at 128 cells.
  The heights stay within a micrometre of the old field's. The water
  guide's costs and its advice on cells are updated.

### Fixed
- `tests/step_inputs.rs` builds with `--features 3d,physics3d` alone: its
  2D physics tests are gated on `physics`.
