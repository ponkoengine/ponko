### Added
- `ctx.perf.set_gpu_timing(GpuTiming)` picks how often a game times its GPU
  passes, separately from profiling: `GpuTiming::Auto` (the default, as
  before: one frame in 16, every frame while profiling is on),
  `Every(n)` (`Every(1)` is every frame, also while profiling is on) or
  `Off` (no GPU times at all). `ctx.perf.gpu_timing()` reads it back. The
  overlay, traces, `PONKO_BENCH` runs and automatic quality still get every
  frame's times while they are on, whatever it says. A game that wants fresh
  `gpu_passes()` every frame no longer has to turn profiling on, which also
  times scopes and counts allocations. Rust-only for now.
