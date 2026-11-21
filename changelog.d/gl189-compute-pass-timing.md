### Fixed
- Compute passes (culling, skinning, global illumination, reflections) and
  passes run alongside them read 0 ms in the GPU pass timer on Metal
  (issue #189). Metal writes a compute pass's timestamps, and sometimes the
  last pass's end, after a copy in the same command buffer has read them,
  so the timer read the previous frame's values and dropped them. A
  frame's timestamps are now copied out by a later frame, once the GPU has
  finished it; each frame in flight has its own queries. GPU times arrive
  about two frames later than before.
- A pass the GPU runs entirely alongside others (a compute pass next to a
  render pass it doesn't depend on) shares that time with them evenly
  instead of being charged 0 ms. The passes still add up to no more than
  the frame's GPU time.

### Added
- `ctx.perf.gpu_passes_not_timed()`: the passes of the latest timed frame
  whose timestamps didn't come back (an empty pass the GPU skipped, or a
  backend that writes none). The overlay lists them as "not timed", the
  profiler report adds a `WarningKind::PassesNotTimed` warning, traces mark
  them, and `perf.get` has `gpu_passes_not_timed`; they are no longer shown
  as 0 ms. The guide's "Which passes can be timed" table says which passes
  can be timed on which backend.
