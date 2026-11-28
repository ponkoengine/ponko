### Added
- Compute results can be read back on the CPU:
  `ctx.gfx.read_gpu_buffer(buffer, 0..4)` and
  `ctx.gfx.read_render_target(target, at, size)` return a `Readback`, and
  `ctx.gfx.readback(pending)` gives the values (`&[Vec4]`) on a later frame,
  typically one to three frames old. Asynchronous only, the same on native,
  WebGPU and WebGL2 (render targets; a `GpuBuffer` has no GPU copy there,
  so its read fails with `ErrorKind::Unsupported`, as every read does in
  headless runs). Staging buffers are reused, so steady reads allocate
  nothing. Readback values are not deterministic and must not feed
  `sim_system`, replays, rollback or lockstep (STABILITY.md). A result is
  freed by reading it (there is no `free`); when more than 64 finished
  results wait unread, the engine logs one warning per run and keeps them
  all. The guide's
  "Reading compute results on the CPU" shows reducing a grid on the GPU and
  reading a few values. C API: `ponko_ctx_read_gpu_buffer`,
  `ponko_ctx_read_render_target`, `ponko_ctx_readback`; C++:
  `ctx::read_gpu_buffer`, `ctx::read_render_target`, `ctx::readback`.

### Fixed
- Compute passes now run in a 3D frame that has nothing else to draw (no
  models, particles or target cameras). Such a frame used to skip the
  whole 3D frame, compute passes included, so a simulation alone on the
  GPU (drawn only in 2D, or only read back) never ran.
