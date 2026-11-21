### Added
- `d3::Shape::ScaledMesh` and `d3::Collider::scaled_mesh(mesh, scale)`: a
  convex hull, triangle mesh or heightfield scaled where it is used, so one
  collision mesh serves colliders of every size; a negative component
  mirrors a hull or triangle mesh. Colliders at one scale share one scaled
  copy, dropped with the last of them. Penetration queries and debug
  drawing take the scale; world saves leave such colliders out. The C API
  reads and writes it as `PONKO_SHAPE3D_SCALED_MESH`, with `size` as the
  scale (no C++ helper yet).

### Changed
- glTF scenes: nodes of one mesh at different or mirrored scales share one
  hull or triangle mesh collider, fitted once unscaled and scaled on each
  collider (`Shape::ScaledMesh` for a node whose scale isn't one), instead
  of one fitted per scale. A hundred spawns of a file of rocks at four
  scales hold 2 collision meshes instead of 5.
- glTF scenes: spawning a file again while it and its buffers are unchanged
  and its meshes are live doesn't parse it: the nodes read by the first
  spawn are kept with the file's shared meshes (each glTF mesh's corners and
  triangles once) until its last scene goes. A re-export the game reloads is
  parsed once for all its scenes.

### Fixed
- glTF scenes: a mirrored node (a negative scale) collides where its mesh is
  drawn: its hull or triangle mesh is mirrored, and its box, sphere or
  capsule sits on the mirrored side, instead of using the unmirrored shape.
