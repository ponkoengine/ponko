### Added
- `Perf::gpu_passes_over_limit()` says how many passes of the latest timed
  frame the GPU timer had no room for. The overlay's pass list, the HUD,
  `Perf::report()` and the control endpoint's `perf.get` (`gpu_passes_over_limit`)
  say it too, so passes are never dropped silently.

### Changed
- The GPU timer times as many passes a frame as the frame has, up to 336
  (it timed at most 32): it grows its query sets before the next frame when a
  timed frame had more passes than room, so a frame with several views, such
  as two eyes, keeps every pass's time. The few frames in flight while it grows
  are not timed.
- The profiler reserves its whole frame history when profiling turns on, instead
  of growing it by doubling while it fills, so no frame allocates for it; clearing
  the history keeps its memory.
