### Changed
- A game times its GPU passes on one frame in 16 instead of every frame,
  except while something reads every frame: profiling
  (`ctx.perf.set_profiling`), the HUD, a trace, a `PONKO_BENCH` run or
  automatic quality, which keep every frame timed (issue #178). Timestamp
  writes cost the graphics library a few heap allocations for every timed
  pass. `ctx.perf.gpu_passes()`, `RenderStats::gpu_ms` and the frame
  statistics stay smoothed as before: each sample now weighs as much as
  the frames it stands for, so they settle at the same values, a little
  later after a change. The render inspector's `gpu_latest_frames_ago`
  shows how old the newest sample is.

### Added
- The render benchmark's `PONKO_RENDER_GPU_EVERY=n` times one frame in `n`
  (default 1, every frame), to measure what timing costs.
