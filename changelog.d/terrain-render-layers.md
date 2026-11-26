### Fixed
- Terrain tiles respect `RenderLayers`: a camera draws a tile only when they
  share a layer (a tile on none of its layers still casts its shadow), as
  models, level-of-detail groups and skinned models already did. The
  view-model camera, which draws only its own layer, drew the ground within
  its far plane over the world: a band of terrain across the bottom of a
  first-person view while an item was held.
