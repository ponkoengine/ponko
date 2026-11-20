### Added
- Saves link glTF prefab instances to their scene: a world save holds a
  record of each glTF scene (its file, where it was spawned and each
  prefab node's instance), and spawning the file again after loading the
  save adopts the saved instances as its prefab nodes, where the game left
  them, without spawning again the ones the game despawned. Of several
  scenes of one file, a spawn adopts the one saved nearest to it.
  `GltfOptions::without_saved_objects` is no longer needed to load a save
  (it still works).
- C and C++: `ponko_gltf_scene_load` (C++ `ponko::gltf_scene_json`) reads
  a glTF file without spawning it and writes its nodes and spawn points as
  JSON; `ponko_gltf_properties_schema` (C++ `ponko::gltf_properties_schema`)
  returns the schema of the custom properties the game reads.
- A Blender add-on, `tools/blender/ponko_properties`, that reads a game's
  `ponko-schema.json`, offers its `ponko:` properties with fitting
  starting values and shows what is wrong with each, with the engine's
  suggestions; its checks run without Blender
  (`python3 -m unittest discover -s tools/blender`). Guide: "From Blender
  to the game".

### Changed
- Every spawn of one glTF file shares its meshes and materials, freed
  with the last scene of the file (and when a world load replaces the
  scenes): a file of three meshes spawned 100 times holds 3 meshes
  (1,488 bytes by `asset_inventory`) instead of 300 (148,800 bytes).
  Hot reload makes one new set for every scene of the file.
- World saves of worlds with glTF scenes hold the reflected resource
  `GltfSceneSaves`. Saves written before still load; their prefab
  instances can't be linked, so scenes spawned after loading one leave
  their prefab nodes to the save, as `without_saved_objects` did.
