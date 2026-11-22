### Added
- An entity with a `TerrainFile` gets its terrain tile however it arrives:
  `spawn_prefab` and the editor's duplicate make it at once, and an entity
  spawned from JSON, by a system or by a mod gets it at the next frame. A
  file that isn't there is an error naming it: the prefab spawn or the
  duplicate fails with nothing left behind, and an entity spawned another
  way is despawned with the error printed. A `TerrainCollider` without a
  live mesh on such an entity gets the file's heightfield.
- Moving a terrain-file tile (by code or with the editor's gizmo) re-places
  its painted layers under it at the next frame, without making the tile
  again; its meshes and collider follow the transform. During a gizmo drag
  the layers settle once, when the drag ends.
