### Added
- `ponko cook` compresses the images a GLB model holds to KTX2
  (`KHR_texture_basisu`) for their use: base color BC7 sRGB, normal map
  BC5, metallic-roughness BC7 with the occlusion merged in (UASTC on
  `mobile`), with mipmaps.
- `ponko cook` stores a model's positions as 16-bit integers when none
  moves by more than 0.1 mm (meshes up to 13 m across; not skinned,
  instanced or morphed meshes). The scale lives on a child node marked
  `PONKO_quantized_positions`, which the engine folds back as it loads, so
  node trees, transforms, extras, colliders and navigation meshes read as
  in the source.
- `ponko cook` checks shaders with the engine's own hook rules (the hook's
  signature, no entry points and no bindings of its own), from the same
  rule table the engine uses at load.

### Changed
- glTF models load their KTX2 textures (`KHR_texture_basisu`) as GPU blocks
  with the `compressed-textures` feature when the GPU has the format,
  instead of decoding them to RGBA8; such textures no longer need
  `basis-universal` unless they are UASTC.
- A cooked `.gltf` no longer brings its `.bin` buffers along: the cooked
  GLB holds them, and the cook counts them as left out.
- `ponko package --no-pack` cooks the loose assets too (`--no-cook` ships
  the source files, as before with `--no-pack`).
- The cooker's version is 2, so earlier cook caches are cooked again.
