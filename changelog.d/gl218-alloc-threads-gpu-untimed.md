### Added
- `perf::GpuUntimed` and `ctx.perf.gpu_untimed()`: why the latest frame has
  no usable GPU time (no GPU, no timestamp queries, the timer refused by the
  query-set budget, timing off, timestamps not back yet, the device lost),
  instead of only "not timed". The overlay shows it, the report adds a
  `WarningKind::GpuNotTimed` warning, the render-pass inspector has
  `gpu_timing.untimed`, and the editor's GPU panel says it. C:
  `ponko_ctx_gpu_untimed` (`PONKO_GPU_UNTIMED_*`); C++: `ctx.gpu_untimed()`.
- `perf::thread_net_alloc_bytes()`: net bytes for the calling thread (C
  `ponko_thread_net_alloc_bytes`, C++ `ponko::thread_net_alloc_bytes()`).
- `ponko_ecs::ThreadMeter`, `set_thread_meter` and `LoopTally`: per-thread
  counters that the ECS's parallel loops carry from their workers back to
  the thread that started them.
- The check list lints the threaded web build with `3d,bench-internals`.

### Changed
- A system's allocation count is its own thread's while it runs, plus what
  the workers of its ECS parallel loops (`par_for_each`, `par_collect_into`,
  a `ViewGroup` on threads, parallel queries from C and C++) allocate for
  them; it used to be the whole process's, so another thread's allocations
  could land in it. `perf::thread_alloc_counts()` includes those loops'
  workers too. Free while profiling is off; a few counter reads per loop
  while it is on. The tests of allocation-free systems (C++ query filters,
  `ponko_ffi`'s filters and asset handles) no longer retry or take the
  quietest of several runs.
