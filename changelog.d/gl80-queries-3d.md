### Added
- 3D physics queries on par with 2D: `d3::Physics::cast_box` (an oriented
  box sweep), `overlap_point`, `closest_point` (the nearest collider and
  its nearest point, as `d3::PointHit`) and `penetrations` (how deep a
  shape sinks into each collider and the way out, as `d3::Penetration`,
  deepest first, equal depths by entity id). They take the 3D
  `QueryFilter` with its collision groups and follow the query rules both
  guides share. With reused buffers none of them allocate, also against
  convex hulls, triangle meshes and heightfield terrain.
- C: `ponko_ctx_cast_box3d`, `ponko_ctx_overlap_point3d`,
  `ponko_ctx_closest_point3d` and `ponko_ctx_penetrations3d`, with
  `PonkoPointHit3D` and `PonkoPenetration3D`. C++: `ctx::cast_box3d`,
  `overlap_point3d`, `closest_point3d`, `penetrations3d`, `point_hit3d`
  and `penetration3d`.
- The 2D/3D rule test (`the_rules_match_3d`) asks every query of the same
  scene in both dimensions and compares the answers.
- Benchmark `physics3d/queries-10k`: 10,000 ray casts, all-hit rays,
  sphere and box casts, closest points and penetrations through a 3D
  level, next to the 2D `physics/rays-10k`, with rapier's own ray and box
  casts for comparison.
- The `physics3d` example drops a crate with a right click, where a box
  sweep from the eye stops.

### Fixed
- A 3D sphere, box or capsule cast that starts overlapping a collider hits
  it at distance 0 with the normal that pushes it out, whichever way it
  moves; moving deeper in, it could miss the collider or report a wrong
  normal.
- 3D queries given a non-finite rotation find nothing, as the rules say;
  the rotation was replaced with no rotation.
- 3D `overlap_box` and `overlap_capsule` see heightfield terrain (only
  `overlap_sphere` did).
