### Added
- `LightBakeScene::add_world(&mut world, &gfx)` adds every model, the first
  level of every LOD group, the instance batches and the lamps of a world
  to a bake, with their materials and base color textures; models whose
  mesh has lightmap coordinates get lightmaps. `Graphics::keep_bake_sources`
  keeps the meshes it reads, the `Unbaked` component leaves a model out,
  and `WorldBake::insert_lightmaps` puts the results on the entities.
- `LightBakeScene::add_gltf(ctx, path, transform)` adds a glTF file's
  static nodes, materials, textures, first LOD levels and lights;
  `GltfBake::insert_lightmaps` lightmaps the spawned scene.
- `bake_lighting_gpu`: the same bake in a compute shader on any adapter
  with compute shaders (not WebGL2), checked against the CPU baker within
  its own noise.
- `BakeTexture` and `BakeModel::with_albedo_map`: bounced light takes the
  color of the base color texture where a ray hits, not only the flat
  material color.
- `LightBakeSettings::compress` (default on): lightmap pages are BC6H
  blocks (a byte a texel instead of 8) in the file and on GPUs that sample
  BC6H, and the shared-exponent format (4 bytes) on the others.
  `BakedLighting::is_compressed`.
- A `Lightmapped` component on a `LodGroup` lights its first level from
  lightmaps (renderer `k` reads entry `first + k`); other levels and
  instance batches take the probe volume's light.

### Changed
- Baked lighting files are version 2 (packed with LZ4, compressed pages
  as blocks); version 1 files still load. Baked lighting's pages are
  bound in the material's second texture array slot (`custom_array1`)
  when it sets none, and its table and probes in a smaller array in the
  third; a material with its own second array takes the probes' light.
  `BakedLighting::gpu_bytes` counts the compressed pages.
