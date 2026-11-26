<!-- area: Physics, navigation and AI -->
### Added
- C and C++: `ponko_collider3d_scaled_mesh(mesh, scale)` and
  `ponko::scaled_mesh_collider(mesh, scale)` make a scaled mesh collider
  (Rust `d3::Collider::scaled_mesh`) without setting `shape` and `size` by
  hand.
- World saves and scenes keep a scaled mesh collider of a mesh file (one from
  `ctx.load_collision_mesh` or a scene) as the file and its scale:
  `{"ScaledMesh": {"mesh": {"file": "rock.glb"}, "scale": [2, 2, 2]}}`.
  Loading makes the file's mesh once for every scale. Colliders of meshes
  made in code are still left out.

### Fixed
- Terrain: a `TerrainCollider` of a triangle mesh on a stretched or mirrored
  tile is the scaled copy `Collider::scaled_mesh` uses, built again from the
  scaled corners, instead of rapier's own scaling, which ignored the mirror
  (the piece stood on the unmirrored side) and bent the surface normals of a
  stretch. A convex hull on a scaled tile now takes the scale too. Tiles at
  one scale share one copy.
