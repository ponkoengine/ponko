### Fixed
- `ponko cook`: a map that a `"keep"` rule copies as it is now counts as a
  linker of the tileset-only Aseprite files it links to, so those files get
  no empty sheet (a `"skip"` rule still takes a file out of the count).
- `ponko_cli` compiles and passes its tests with `--no-default-features
  --features meshes,textures,shaders` (the shared cook fixture test needs
  the `aseprite` feature and is built only with it), and `textures` alone no
  longer warns about an unused dependency: kept glTF buffers are written
  with a small encoder in the cook instead of the `base64` crate.
- The command line tool's feature subsets (each of `textures`, `meshes`,
  `shaders` and `aseprite` alone, none, and `meshes,textures,shaders`) are
  linted and tested in the list of checks.
