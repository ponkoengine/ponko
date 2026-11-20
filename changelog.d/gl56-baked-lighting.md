### Added
- Baked lighting for static scenes: `bake_lighting` path traces a scene's
  bounced light (sky, sun, point and spot lights, emissive surfaces) ahead
  of time on the CPU into lightmaps (with the direction light comes from,
  for normal maps) and a probe volume for moving models. `BakedLighting`
  saves to bytes and loads back (`Graphics::load_baked_lighting`, or
  `try_load_baked_lighting` for the error as a value), and
  `Graphics::set_baked_lighting` draws with it: models with a `Lightmapped`
  component read their lightmap, the others the probes, and the sun and
  lamps stay realtime with their shadows. It needs no compute shaders and
  no texture slot of its own, so it works on WebGL2 and the Low preset.
- `MeshData::generate_lightmap_uvs` makes lightmap coordinates (`uvs1`)
  for meshes without a second UV set.
- The `bake_lighting` example bakes a room's light from the command line
  (`-- bake room.lit`) and shows it.
