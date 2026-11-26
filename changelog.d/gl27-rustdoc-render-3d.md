<!-- area: Guide and examples -->
### Changed
- Every public item of the rendering, 3D, 2D drawing, particle, lighting,
  post-processing, terrain and streaming modules must now be documented:
  `#![deny(missing_docs)]` keeps it that way. The settings structs of the
  renderer (`Lighting`, `Post`, `Sky`, `MaterialDesc`, `ParticleSettings`,
  `InstanceBatchOptions`, `LodSettings`, `DrawParams` and the others) say
  their defaults and what they cost, the functions that return an error or
  panic have `# Errors` and `# Panics` sections, the features a WebGL2 device
  lacks say what happens there, and the GPU capability report, 2D occluders,
  streaming controls, instance batches, GPU buffers, draw parameters and
  particle settings gained runnable examples.
