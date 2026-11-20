### Added

- Aseprite source import composites isolated groups with their authored opacity
  and blend mode, and draws tilemap layers with embedded tilesets (flipped and
  rotated tiles, linked cels).
- The optional `aseprite-source` feature loads loose `.aseprite` and `.ase` files
  with hot reload. A source path also resolves to its cooked JSON export, so the
  same loading code works in cooked builds without the feature; without either,
  the `NotFound` error says how to get one.

### Changed

- The cook caches Aseprite-to-sheet conversion by source content and split mode:
  a cook that changed nothing reads the sheet back without converting.
- Embedded textures in meshopt and Draco glTF models cook to KTX2 while
  retaining their compressed geometry and buffer layout; the replaced images'
  bytes in the GLB chunk are zeroed, so a pack deflates them.
