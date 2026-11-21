### Added
- C and C++: `PonkoUploadStatsEx` (`ponko_ctx_upload_stats_ex`, C++
  `ctx.upload_stats_ex()`), a versioned form of the upload counters with
  the GPU's own time for textures made outside the upload steps
  (`gpu_created_ms`), for the latest upload step (`gpu_upload_ms`),
  `gpu_upload_timing` and `frame_slices`. `PonkoUploadStats` keeps its layout.
- C and C++: `ponko_net_alloc_bytes()` (C++ `ponko::net_alloc_bytes()`),
  bytes allocated minus bytes freed while profiling is on, for leak checks
  of native games; `ponko_note_deallocation` (Rust
  `perf::note_deallocation`) counts the frees of memory reported with
  `ponko_note_allocation`.
- C and C++: `PonkoBuoyancyDesc`, a versioned buoyancy descriptor with up
  to 32 sample points of your own, the water's density, drag and the ripple
  switch: `ponko_buoyancy_desc_box`, `_sphere`, `_points`,
  `ponko_ctx_set_buoyancy` and `ponko_ctx_buoyancy` (C++
  `ponko::buoyancy_desc`, `ctx.set_buoyancy`, `ctx.buoyancy`).

### Changed
- C++: `PONKO_COUNT_CPP_ALLOCATIONS` counts `delete` too (each block keeps
  its size in a header of `alignof(std::max_align_t)` bytes), so
  `net_alloc_bytes` sees what a C++ program gives back.

### Fixed
- `cargo clippy -p ponko --features compressed-textures` (without `3d`)
  no longer warns about the cooked-ORM note, which only glTF models read.
