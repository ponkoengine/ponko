### Added
- `perf.get` lists each GPU allocation on request: `gpu_allocations: true`
  (or a count for the largest) adds `render_inspector.gpu_allocations`, one
  entry per texture and buffer the engine holds, largest first, with its
  class, label (`3d color`, `3d depth`, `3d shadow map`, `3d light shadow
  atlas`, `3d bloom`, ...), bytes, a texture's size, mips, samples and
  format, and a buffer's usage (uniform, storage, vertex, index, indirect,
  staging, query, copy); with the buffers' bytes by usage and what a count
  left out. Built only when asked for. The MCP `perf_report` tool takes the
  same `gpu_allocations` parameter.

### Changed
- `perf.get`'s `assets.gpu` (the game's assets only) now carries
  `all_gpu_memory_bytes`, the engine's whole GPU total from
  `render_inspector.gpu_memory_bytes`, and a note saying where the frame
  targets, shadow maps and the rest are.
