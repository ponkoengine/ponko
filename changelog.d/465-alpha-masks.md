### Changed
- Textures no longer keep a CPU copy of their alpha channel. Each texture
  uploaded from pixels keeps a 24-byte summary (size, fully opaque, a hash of
  its alpha); a full mask, one byte per texel, is read back from the GPU once
  and kept only for a texture that a cutout material reads as its base map,
  when a region upload first needs it. Uploads that leave alpha alone still
  keep cached cutout shadows. In a measured 300-texture set (157 MB of masks
  before), the bookkeeping holds 3.2 MB, nearly all of it the 12 cutout masks.

### Added
- `AssetInventory::texture_alpha` (`TextureAlphaMemory`, also in `perf.get`'s
  `assets`): the CPU bytes held by texture alpha summaries and masks.
