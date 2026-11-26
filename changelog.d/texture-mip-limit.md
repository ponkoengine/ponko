### Added
- `Graphics::set_texture_mip_limit(levels)` and `texture_mip_limit()`: 3D
  textures loaded afterwards (model images and texture arrays, RGBA and
  block-compressed) drop their `levels` largest mip levels before upload,
  as Unity's texture mipmap limit does. Each level quarters their video
  memory. Textures keep at least 32 pixels a side; sprites, UI textures and
  render targets never change. For GPUs with little video memory, where a
  world that doesn't fit spills into system memory and every frame waits
  on it (measured: a large world on a 4 GB card went from about 130 ms to
  about 17 ms of GPU time a frame at one level).
