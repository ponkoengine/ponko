### Added

- Aseprite tilesets that link to a tileset in another file (external
  tilesets) are read from that file through the asset sources, by the cook
  and by `aseprite-source` loading, instead of being refused. Links resolve
  relative to the file, may chain up to 8 files deep, and the linked file
  is watched for hot reload and part of the cook's cache key.
  `ponko_aseprite::decode_linked` and `ponko_aseprite::linked_files` are
  the decoder's side of it.

### Changed

- A cooked meshopt or Draco model drops the bytes of the images it replaced
  with KTX2 from the buffers it holds (GLB chunk, data URIs), moving later
  views and compressed streams by multiples of 16 bytes, instead of zeroing
  them in place: the file is smaller on disk, not only once packed, and the
  geometry stays byte for byte. The meshopt test fixture went from 10,188
  to 6,128 bytes (its source is 7,116).
