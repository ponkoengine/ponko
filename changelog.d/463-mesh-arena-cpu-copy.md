### Changed
- The 3D renderer's shared mesh buffers no longer keep a CPU copy of every
  mesh ever uploaded: new geometry waits on the CPU only until the next
  frame uploads it, and growing the buffers copies them on the GPU. On
  WebGL2, where index buffers can't be copied on the GPU, only the indices
  keep a CPU copy.

### Added
- `GpuResidency::mesh_cpu_bytes` (in `Ctx::asset_inventory` and the
  `assets` section of `perf.get`): the CPU memory the renderer holds for
  3D meshes. The render benchmark's memory section reports it too.
