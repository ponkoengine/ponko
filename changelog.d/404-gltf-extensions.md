### Added
- glTF extensions: `KHR_lights_punctual`, `KHR_texture_transform`,
  `KHR_materials_emissive_strength`, `KHR_mesh_quantization`,
  `KHR_materials_transmission` (drawn as see-through blending, without
  refraction), `KHR_texture_basisu` (with `basis-universal`) and
  `EXT_texture_webp` (with `image-webp`). The guide's "glTF extensions,
  lights and cameras" lists them.
- Compressed glTF geometry (feature `gltf-meshopt`):
  `EXT_meshopt_compression` and `KHR_meshopt_compression` buffer views are
  decompressed at load with meshoptimizer's decoder (C++; not in web
  builds, where such a file loads only from its uncompressed fallback).
- `Graphics::load_lights` and `load_cameras` (and their `try_` forms):
  the lights and cameras a glTF file places, as `render::ModelLight` and
  `render::ModelCamera`, with `ModelLight::spawn` and `ModelCamera::apply`
  to put them where the model is. Light intensities follow Blender's
  exporter: candela over 683, lux as it is.

### Changed
- A glTF file that requires an extension the loader doesn't read fails
  with an error naming the extension (or the cargo feature that reads it);
  one it only uses loads, with a warning naming it.

### Fixed
- A glTF material whose textures use the second texture coordinate set
  (`texCoord: 1`) is drawn with that set instead of the first.
