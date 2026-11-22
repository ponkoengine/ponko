### Added
- A reproducible scoreboard against Bevy: `python3 bench/compare/scoreboard.py`
  runs ECS, sprite, 3D, skinned-character, 2D physics, UI, headless
  simulation and time-to-first-frame workloads on both engines, measures
  build times and sizes, and writes JSON plus a Markdown table (three runs
  each, the median and the range, with the load average). The results are in
  the guide's performance chapter, losses included.
- `bench/compare` (was `bench/bevy_scenes`) is the one comparison crate,
  with its own lock file and Bevy pinned to 0.20.1.
- `ponko_bench`: `compare/...` rows (the scoreboard's Ponko side) and a
  `first_frame` binary.

### Changed
- `ponko_bench` no longer depends on `bevy_ecs`: Bevy is out of the engine
  workspace's `Cargo.lock`. Its ECS rows run in `bench/compare`, which
  compiles the same suite.
