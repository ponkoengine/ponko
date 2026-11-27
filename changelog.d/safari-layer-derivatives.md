<!-- area: Rendering -->
### Fixed

- 3D scenes draw again in Safari's WebGPU. Since glTF material layers
  (#644), every material and compute pass failed to compile there ("call
  to 'textureSampleBias' requires uniform control flow"), so the scene
  drew black. The layer code no longer takes screen derivatives itself:
  the fragment shader passes them in, and the layer maps are sampled with
  explicit gradients (#701).
