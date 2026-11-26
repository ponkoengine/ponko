### Breaking changes
- Waves from a spectrum are drawn on the simulation's clock, where floating bodies ride them, instead of the frame's.
  The clock is `sim_time()` plus `fixed_alpha()` of a fixed step, not
  `ctx.time.elapsed()`. Nothing changes until a game pauses or changes the
  simulation's speed; after that the sea stands still while paused and runs
  at the simulation's speed, and the drawn waves are the ones floating bodies
  ride. Waves from the wave table stay on the frame's clock.
  Instead: ask `height_at` or `sample` at `water.drawn_time(&ctx.time)` for
  what is drawn; a shader that drew something in step with a spectrum's
  waves from `frame.time.x` (still the frame's clock) takes the drawn time in
  a material value instead.
  Before:
  ```rust
  let h = water.height_at(xz, ctx.time.elapsed() as f32);
  ```
  After:
  ```rust
  # let (water, xz): (ponko::water::Water, Vec2) = todo!();
  let h = water.height_at(xz, water.drawn_time(&ctx.time));
  ```

### Added
- `Water::drawn_time(&ctx.time)` (C `ponko_ctx_water_drawn_time`, C++
  `ctx.water_drawn_time(water)`): the time a water body is drawn at this
  frame, to pass to `height_at`, `sample` and `is_under` for what the
  player sees.

### Changed
- Waves from a spectrum are drawn on the simulation's clock (see Breaking
  changes). The WebGL2 upload now shares the fixed steps' moments with
  floating bodies after a pause or speed change too (at most one moment a
  step), and a paused game works out and uploads nothing.
- The WebGL2 upload's f32 to f16 conversion uses the CPU's own (F16C, ARM
  FP16, through `half`) where there is one, a chunk at a time, and is
  branch-free elsewhere, with the same bits for every float: one moment's
  upload 0.083 → 0.007 ms at 128 cells and 0.36 → 0.029 ms at 256, a
  blended one 0.11 → 0.013 ms and 0.49 → 0.054 ms (release, one thread's
  CPU time, medians of five on a busy Apple M4 Max; a moment of the field
  costs 0.13 and 0.6 ms).
