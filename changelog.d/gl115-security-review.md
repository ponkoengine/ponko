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
- WebSocket connections that finish their handshake and then send nothing
  for 30 s are dropped, so silent sockets can't hold a host's connection
  slots.
- The control endpoint's `sim.step` queues any number of ticks at once
  instead of one at a time (four billion ticks took seconds of the game
  thread).

### Added
- Fuzz targets for replication, the lobby protocol (server and client),
  the control endpoint's requests, Aseprite files and cooked sheets, LDtk
  and Tiled levels, and cooked textures (`fuzz/README.md`).
- A load test with hostile clients for the lobby server
  (`crates/ponko_lobby/tests/hostile.rs`): its memory stays flat while they
  flood it, and honest rooms keep working.
- The guide's "Security: what reads untrusted bytes": every entry point,
  who can send it bytes, what a bad actor wants and what stops them.
  `SECURITY.md` says what is in scope.
