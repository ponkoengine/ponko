### Added
- `ponko cook` turns a folder of source assets into runtime-ready files under
  the same names, so a game loads them with the same calls: images become
  KTX2 with mipmaps (BC7, BC5, BC4 on desktop and the web, Basis Universal
  UASTC with `--platform mobile`), glTF and GLB models one GLB with meshes
  reordered for the vertex cache, overdraw and vertex fetch and normals and
  texture coordinates quantized (`KHR_mesh_quantization`), PCM WAV sounds
  lossless FLAC, and WGSL shaders are validated as the engine composes them.
  Per-asset settings come from an optional `cook.json` (paths and
  patterns; `textures.json` kinds too). Results are cached by a SHA-256 of
  the input and its settings (`target/cook-cache`), so a second cook of
  unchanged files reads them back, and the same input always gives the same
  bytes. `ponko pack --cook` packs the cooked files. See the packaging
  chapter's "Cooking assets".
- `tests/fixtures/cook/`: a small source texture, sound and model and what
  `ponko cook` makes of them; `tests/cooked_assets.rs` loads both through the
  same calls and compares them.

### Changed
- `ponko package` cooks the files it puts in `assets.pack`, after the build:
  images become KTX2 only when the game's engine is built with
  `compressed-textures`. A game that reads its asset files' raw bytes itself
  (`Assets::load_with`, `read`) gets the cooked bytes; `--no-cook` or
  `pack = { cook = false }` in `[package.metadata.ponko]` packs the source
  files as before. Shard Grove's pack: 349.6 KiB before, 198.1 KiB cooked.
