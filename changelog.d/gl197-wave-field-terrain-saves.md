### Added
- `web/build.sh --simd`: WASM SIMD for a web build, `-C target-feature=+simd128`
  and the engine's `wasm-simd` feature always together (each alone is no
  faster, or slower), added to the rust flags cargo reads like `--threads`
  and keeping the caller's; it goes with `--threads`. The water guide's "On
  the web" uses it instead of setting the environment variable by hand.
- `Physics3D::set_terrain_rebuild` (and `clear_terrain_rebuild`): how a
  terrain tile's heightfield is made again after `ctx.load_world` or
  `ctx.load_scene`, from the tile's saved components, so a loaded world's
  tiles are solid where they are drawn (mirrored ones too) without the game
  making their colliders again.

### Changed
- World saves keep each `TerrainCollider`: its friction, restitution and
  groups, with the heightfield written as `null` (a mesh made in code has no
  file to name) and made again by the terrain rebuild. Saves written before
  load as they did; without a rebuild a loaded tile has no ground until the
  game gives it a new `TerrainCollider`, as before.
- The water's CPU wave field works out a moment once when the WebGL2 upload
  and buoyancy ask for the same time: after a moment worked out without the
  sway's speed is asked again with it, the next ones are worked out with it
  from the start, until such a guess goes unused. At 128 cells a frame of a
  floating body on a WebGL2 sea costs 0.21 ms of CPU field instead of
  0.35 ms (0.93 instead of 1.55 ms at 256 cells; release, Apple M4 Max, busy
  machine, medians of seven alternating runs).

### Fixed
- The water's CPU wave field kept its last moments by the time folded into
  the 200 s period but looked them up by the time asked, so after the first
  200 s of a game every query worked out its moment again.
