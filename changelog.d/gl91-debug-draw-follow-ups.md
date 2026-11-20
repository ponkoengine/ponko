### Added
- `DebugShape::depth_tested()`: a 3D debug label hidden behind nearer 3D surfaces, as 3D lines are, drawn in the lit 3D frame facing the camera, its size in pixels wherever it is. C: `ponko_ctx_debug_text3d_depth_tested`; C++: `debug_draw::text_depth_tested`.
- `DebugDraw::set_collider_distance` and `collider_distance` (default 50 world units): how far from the 3D camera the colliders view draws mesh and terrain triangles. C: `ponko_ctx_debug_set_collider_distance` and `ponko_ctx_debug_collider_distance`; C++: `debug_draw::set_collider_distance` and `collider_distance`.
- One `DrawSpace` for sprites and debug drawing in every language: `DrawParams::with_space` takes the same `ponko::debug::DrawSpace` (also in the prelude) as debug drawing. C: `ponko_debug_style_with_space` takes the `PonkoDrawSpace` sprite draws take; C++: `ponko::with_space` works on a `debug_style` as on `draw_params`. `PonkoDebugStyle`'s layout is unchanged.
- The `debug/` benchmarks print their heap allocations per frame, and a `debug/shapes-and-labels` case draws spheres, boxes, capsules, circles and every kind of label. `tests/debug_draw_alloc.rs` checks that steady drawing, with every built-in view on, allocates nothing.
- The offscreen test `hundred_thousand_lines_gpu_time` times 100,000 debug lines with GPU timestamps (the opaque pass and the frame), with the lines in view, behind the camera, as dots, all depth-tested and all on top.

### Changed
- The colliders view draws 3D collision meshes (`Collider3D::mesh` triangle meshes and heightfields) and `TerrainCollider` terrain as their triangles within `collider_distance` of the 3D camera, found through the mesh's own tree or grid, instead of a bounding box (terrain wasn't drawn before). Convex hulls show all their edges.
