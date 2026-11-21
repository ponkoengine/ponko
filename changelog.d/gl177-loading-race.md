### Fixed
- A failed asset group goes on taking the assets it had started before it
  failed, as they finish, and counts them in its progress; `finish_uploads`
  waits for them. Before, a failed group's `loaded` stopped where the failure
  came back, so it could be short of what had in fact finished, and differed
  from run to run under load. The C API test of a restarted group checks the
  exact count again.

### Added
- `UploadStats::gpu_created_ms`: where the GPU has timestamp queries, its
  own time copying the textures made outside the asset group and streaming
  steps (`create_texture`, `create_compressed_texture`), a frame's worth at
  a time.

### Changed
- Without worker threads (an ordinary web build), the streaming step now
  decodes a streamed glTF file's JSON, buffers (an inline `data:` buffer a
  bounded number of bytes a slice) and geometry in slices, not only its PNG
  images, and KTX2 textures (Zstandard and zlib levels, decoding to RGBA,
  Basis UASTC transcoding) and DDS textures decoding to RGBA in slices too,
  as glTF images or on their own. The meshes and textures are byte for byte
  what the one-piece decode makes.
- The GPU upload times of the asset group and streaming steps
  (`gpu_upload_ms`) now include their mesh buffer uploads, not only their
  textures. Meshes made in a timed step are copied into the shared mesh
  buffers before the step ends instead of at the next draw.
