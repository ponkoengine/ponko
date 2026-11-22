### Added
- `Lighting::shadow_lag` (off by default): the far sun cascades may lag
  behind their casters to draw fewer shadow passes. While a far cascade
  holds its box (`shadow_refresh` above 1) its map is kept whatever its
  casters do, so a far shadow trails a moving or animated caster by up to
  `shadow_refresh - 1` frames; near cascades and point and spot lights stay
  exact, and a changed mesh, material, shader or instance batch still
  redraws at once. In the benchmark town (moving and skinned casters in
  every cascade) it saves 0.14 to 0.26 ms of sun shadow GPU time a frame,
  6 to 10% of the GPU frame (`crates/ponko_bench/SHADOW_LAG.md`). Rust
  only for now; C and C++ far cascades are exact.
- The render scene benchmark takes `PONKO_RENDER_SHADOW_REFRESH` and
  `PONKO_RENDER_SHADOW_LAG`, and a `vegetation` shadow workload (slow
  camera, still actors, no skinned walkers).

### Changed
- An instance batch whose shadows are cut by distance tells whether a moved
  camera changes what a kept shadow map holds instance by instance, not
  cell by cell: a cell across a band edge no longer redraws the maps it is
  in on every camera move, only when one of its instances crosses the
  edge. Such a batch on the GPU path keeps its instances' positions on the
  CPU for it (16 bytes an instance).
