<!-- area: Rendering -->
### Fixed
- 3D draws in browsers on WebGPU: the built-in shader's layered material
  branched on transmission through a value that also held screen
  derivatives. A browser's uniformity analysis rejects that
  (`textureSampleBias` outside uniform control flow), so every 3D pipeline
  was invalid and the canvas stayed blank. Native backends accepted it.
