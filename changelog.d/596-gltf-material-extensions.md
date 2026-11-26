### Added
- Material layers: `MaterialDesc::layers` (`MaterialLayers`) adds clear coat
  with its own normal map, sheen, specular strength and color, index of
  refraction, anisotropy and iridescence, each with its maps, and
  `MaterialDesc::unlit` draws a material without lighting. A material with
  layers draws with a layered build of the built-in shader; materials
  without them keep the plain build and cost nothing more.
- The glTF loader reads `KHR_materials_unlit`, `KHR_materials_clearcoat`,
  `KHR_materials_sheen`, `KHR_materials_specular`, `KHR_materials_ior`,
  `KHR_materials_anisotropy`, `KHR_materials_iridescence` and
  `KHR_node_visibility` (a hidden node is left out with everything under
  it). A file that requires `KHR_materials_volume` loads with a warning
  (transmission without refraction or absorption).
- C API: `PonkoMaterialDescEx` version 2 adds `unlit` and `layers`
  (`PonkoMaterialLayers`).
