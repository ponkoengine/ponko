<!-- area: Rendering -->
### Added

- Glass, liquids and other transmitting materials: `MaterialLayers::with_transmission`
  takes a `Transmission` layer (clear and diffuse transmission, volume thickness and
  attenuation, three-channel dispersion, approximate scattering, each with its map).
  Clear transmission keeps the surface's reflections and refracts a mipmapped copy of
  the opaque HDR picture, blurred by roughness and bent by the index of refraction;
  rays that leave the picture read the environment. The copy and its mip pass run
  only while glass is in the camera's draw list. WebGL2 reads a single-level copy.
- glTF: `KHR_materials_transmission`, `KHR_materials_volume`, `KHR_materials_dispersion`
  and `KHR_materials_diffuse_transmission` are read into that layer, with their maps;
  the draft `KHR_materials_scatter` and `KHR_materials_volume_scatter` load,
  approximated, with a warning. A volume's thickness follows the node's scale when a
  file loads as one mesh. `ponko cook` cooks files that use any of them.

### Changed

- A glTF material with `KHR_materials_transmission` keeps its own alpha mode and alpha;
  it was loaded as `Blend::Transparent` with its alpha lowered by the transmission.

### Fixed

- Transmissive glTF materials no longer vanish: the glass, liquid and volume parts of
  the Khronos samples (stained-glass and lamp shades, lenses, jugs, vases, windows,
  attenuation and dispersion tests) are drawn.
