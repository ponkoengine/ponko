### Added
- `AssetGroup::with_sliced` and the `SliceLoader` trait: a game's own loader
  that does a bounded piece of work per call. Where there are no worker
  threads (a browser build without threads) the group step calls it within
  its frame budget, so a large file is spread over several frames; with
  workers it runs on a worker.
- Where there are no worker threads, `stream_texture` and `stream_mesh`
  decode a slice at a time in the streaming step, within what the asset
  group step left of its frame budget: a PNG a few rows a slice, then its
  smaller resolutions; a glTF file's PNG images that way, then its geometry.
  `UploadStats::frame_slices` counts the slices of the last frame.
- The GPU's own time copying uploaded textures, where it has timestamp
  queries: `GroupStats::gpu_upload_ms` and `total_gpu_upload_ms`,
  `UploadStats::gpu_upload_ms`, and `gpu_upload_timing` on both, `false`
  headless and on GPUs and browsers without timestamp queries.
- `GroupStats::pieces`: the pieces of work (reads, decodes or their slices,
  uploads) the last group step ran.

### Fixed
- The test of sliced PNG decodes counts the slices each step runs against
  the frame budget instead of timing them, so it no longer fails on a busy
  machine.
