### Added
- Benchmark tracking: `bench_track` (in `crates/ponko_bench`) counts the
  instructions of seven small workloads (ECS iteration, a filtered query,
  lookups, spawn, despawn, a headless tick, 3D frame collection) under
  cachegrind, compares repeated runs by their medians against a threshold
  of the larger of a floor and twice their spread, renders a history per
  machine, and compares two folders of runs. Each release adds a wall-clock
  baseline from a named machine (`crates/ponko_bench/releases/`).
- `ponko_bench`'s JSON results record `env.machine` (`PONKO_BENCH_MACHINE`).
