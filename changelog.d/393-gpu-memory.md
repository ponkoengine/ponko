### Added
- GPU memory in the performance report: `PerfReport::gpu_memory` (a
  `perf::GpuMemory`) counts every texture and buffer the engine holds on the
  GPU, once each, by class: textures, meshes, GPU buffers, frame targets,
  shadows, the lights' shadow atlas, skinning, probes, instance batches and
  other, with the total and the platform's own figure where it reports one
  (Vulkan and DirectX 12 through wgpu's allocator, Metal's allocated size;
  `None` with the reason elsewhere, never 0). Headless runs have none.
- `Budget::gpu_memory_mb` limits it (`{"gpu_memory_mb": 512}` from C and
  C++); a report without GPU memory breaks it as "not measured".
  `PerfReport::regressions` compares it, and the text report and the
  profiling overlay show it.
- `perf.get`'s `render_inspector.gpu_memory_bytes` (and the MCP
  `perf_report` tool) report the same classes, total and adapter figure.

### Breaking changes
- `PerfReport` has a new field, `gpu_memory`, and `Budget` a new field,
  `gpu_memory_mb`.
  Code that builds either with a struct literal naming every field no
  longer compiles; reading them, `Default` and the builders are unchanged.
  Instead: end the literal with `..Default::default()`, or build a budget
  with `Budget::new()` and its methods.
  Before:
  ```rust
  let budget = Budget { frame_avg_ms: None, frame_p99_ms: Some(4.0), frame_max_ms: None,
      allocs_per_frame: Some(0.0), systems: Default::default(), scopes: Default::default() };
  ```
  After:
  ```rust
  let budget = Budget { frame_p99_ms: Some(4.0), allocs_per_frame: Some(0.0), ..Default::default() };
  ```
