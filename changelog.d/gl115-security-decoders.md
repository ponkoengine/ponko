<!-- area: Assets, scenes and saves -->
### Security
- Image, texture and video decoders refuse sizes in a file's header that
  would allocate without bound, before allocating: 16 384 pixels a side and
  512 MiB of RGBA8 at most, an `ErrorKind::Limit` error past them. Covers
  every `image` format, KTX2 and DDS files, the streamed PNG path and WebM
  video (its container size and the AV1 decoder's frame size).
- A KTX2 file whose supercompressed (Zstandard or zlib) levels unpack to
  more than their size needs is refused, and unpacking stops as soon as it
  passes it: a few bytes could claim terabytes before. A KTX2 file with a
  texel block dimension of 256 in its format descriptor is an error
  instead of a panic in the `ktx2` crate.
- Levels (LDtk, Tiled) refuse tile layers of more than 16 777 216 cells,
  tilesets of more than 1 048 576 tiles and tiles or tileset sheets past
  the image limits, with `ErrorKind::Limit`; Tiled's zlib and gzip layer
  data stops unpacking at the cell limit. Sizes that overflowed in tileset
  arithmetic saturate instead.
