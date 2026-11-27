### Fixed
- `ShadowMesh(None)` turns a skinned model's shadow off, as it already did
  for plain models and terrain: skinned models cast with their posed mesh
  regardless. First-person arms (skinned, on the view-model layer) cast a
  shadow into the world that the body already casts.
