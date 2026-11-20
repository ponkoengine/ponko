### Added
- `ponko cook` reads Aseprite files (`.aseprite`, `.ase`) itself, with no
  export step: `hero.aseprite` becomes `hero.json` and `hero.png`, the JSON
  Array export `load_aseprite` reads (frames composited with layer and cel
  opacity and blend modes, tags, slices; `"layers": true` in `cook.json`
  exports each layer). An export already beside it is replaced.
- `ponko cook` compresses the image files a glTF model reads for their use,
  whether the model is cooked or kept as it is: base color BC7 sRGB, normal
  map BC5, metallic-roughness BC7 with the occlusion merged in. The merged
  file carries a `ponkoOrm` KTX2 key naming its two sources, and the engine
  uploads it as it is, so an uncooked model's occlusion stays compressed
  (a 32 x 32 test model: 16,380 -> 4,176 bytes of video memory).
- `ponko package`: `files.to` in `[package.metadata.ponko]` can name a
  subfolder of the package (`to = "docs/manual.pdf"`).

### Changed
- `ponko cook` quantizes the positions of a mesh too large for one 16-bit
  range (13.1 m within 0.1 mm) with a range per primitive, so large levels
  keep their precision and still shrink: a 48 m test level of 16 primitives
  is 627,716 bytes with float positions, 561,256 cooked this way. The
  engine folds the pieces back into one mesh in their order.
- The cooker's version is 3: a cook after updating cooks every file again.

