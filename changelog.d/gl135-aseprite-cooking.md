### Added

- Aseprite source import composites isolated groups with their authored opacity
  and blend mode, and draws tilemap layers with embedded tilesets.
- The optional `aseprite-source` feature loads loose `.aseprite` and `.ase` files
  with hot reload. A source path also resolves to its cooked JSON export.

### Changed

- The cook caches Aseprite-to-sheet conversion by source content and split mode.
- Embedded textures in meshopt and Draco glTF models can cook to KTX2 while
  retaining their compressed geometry and buffer layout.
