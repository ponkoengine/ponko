### Changed

- The cook drops the bytes of the images it replaced with KTX2 from an
  external `.bin` buffer of a meshopt or Draco model too, when exactly one
  model reads that file and no rule keeps or skips it (before, only the
  GLB's own chunk and data URIs were shortened): it writes the shorter
  `.bin` in the model's place, with the model's offsets rewritten and the
  geometry byte for byte. A buffer two models read is left untouched. A
  `.gltf` model of 6,831 bytes with its `.bin` of 4,364 is now 6,809 and
  316 bytes. The cook cache keeps the shortened buffer with the model
  (cache format and cooker version bumped, so old entries are cooked
  again).
- A file of only tilesets (no layer) that another Aseprite file links to
  gets no sheet in the cook, without a `skip` rule; the report lists it as
  left out. `ponko_aseprite::is_tileset_only` tells such a file.
