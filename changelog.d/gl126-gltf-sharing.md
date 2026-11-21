### Changed
- glTF scenes: nodes that use one glTF mesh (linked duplicates in
  Blender) share one uploaded mesh instead of uploading it per node. Eight
  nodes of one mesh hold 49,152 bytes of GPU buffers instead of 393,216
  (offscreen on Metal); the benchmark's file of twelve nodes of one rock
  mesh holds one mesh (66.5 KiB) instead of twelve (798 KiB).
- glTF scenes: the collision shapes fitted for a file's colliders are
  shared like its meshes: by its nodes of one mesh at one scale, and by
  every later spawn of the file, until the last scene that uses them goes.
  A hundred spawns of the benchmark's file hold two collision meshes
  instead of 600, keep 1.4 MiB of heap instead of 115 MiB, and spawn in
  about 9 ms instead of 0.2–0.9 s (`cargo run --release -p ponko_bench --
  gltf/`).

### Fixed
- Loading a world now frees the collision shapes of the glTF scenes it
  replaced, as it did their meshes.
- A glTF file exported again with a property mistake keeps its scene
  drawing: the meshes of the scene it couldn't update are no longer freed.
