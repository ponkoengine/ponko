### Added
- Legions (`examples/legions`), the simulation showcase: two armies of
  50,000 soldiers and archers on shared flow fields, spatial-grid targeting
  (archers lead their targets from the grid's velocity data), arrows that
  fly and land, the fallen, rounds, and animated sprites from one sheet drawn
  as two instanced draws. Orders are its only input; `--record` writes them
  with the seed and a world hash, and `--replay` plays a recording headless
  and checks that it ends with the same hash (on any thread count; checked
  on aarch64 and x86_64). `--headless` prints tick-time percentiles and each
  system's share. It packages with `ponko package --example legions` (no
  assets; a recorded battle ships beside it) and builds for the web.
  `tests/legions.rs` plays it headless (same hash across runs, thread counts
  and with or without sprites, recorded input through headless frames
  replayed by the rules alone, the committed replay, orders, arrows,
  rounds); `src/render/legions_render_test.rs` draws it offscreen with a
  reference image and measures the frame cost. The guide's chapter
  "Legions: 100,000 units that replay bit for bit" walks through it.
