### Added
- glTF files as scenes: `Ctx::spawn_prefab` of a `.glb` / `.gltf` file
  (or `GltfRoot::spawn`, which takes options) spawns an entity for every
  node of the file, attached to its parent, with its own mesh, materials and light, and
  with what its custom properties (`extras`, as Blender writes an object's
  custom properties) ask for: reflected components by name with JSON values
  (`ponko:Health`), colliders fitted to the node's mesh (`ponko:collider`:
  box, sphere, capsule, hull or trimesh, with `ponko:body`, `ponko:sensor`
  and collision layers), spawn points (`SpawnPoint`), prefab references,
  level-of-detail groups made of the node's children, render layers,
  navigation mesh inclusion and light shadows. `GltfRoot` finds nodes by
  path, `GltfNode` keeps every custom property for the game's own use, and
  `Ctx::despawn_prefab` removes the scene with the meshes made for it.
- Unknown property or component names, and values of the wrong kind, are
  errors naming the file, the node and the closest known names
  (`did you mean "ponko:collider"?`); nothing is spawned then.
- Hot reload of glTF scenes: exporting the file again updates every scene
  spawned from it in place, keeping the entity of each node whose path
  stays; new nodes spawn, removed ones despawn, a broken file keeps the old
  scene.
- `GltfScene::load` reads and checks a file node by node without
  spawning it; `GltfScene::nav_source` (feature `navmesh`) collects its
  walkable meshes for `NavMesh::bake`.
- `ponko schema` writes the JSON Schema of the custom properties a game
  reads (the convention and every reflected component), for editors to
  check properties before exporting; any game started with `Game::run`
  writes it when `PONKO_SCHEMA=<file>` is set, and
  `ponko::d3::gltf_properties_schema` returns it.
- C and C++: `ponko_ctx_spawn_gltf`, `ponko_ctx_despawn_gltf`,
  `ponko_gltf_node_entity`, `ponko_gltf_node_extras` and
  `ponko_spawn_point_name` (C++ `ponko::spawn_gltf` and friends).
- The guide's "From Blender to the game" chapter (properties, Blender's
  export settings, hot reload, saves) with the sample
  `assets/levels/courtyard.gltf`, and a section of the 3D tutorial.
