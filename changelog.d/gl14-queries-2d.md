### Breaking changes
- `d2::Collider` has a `groups` field (its collision groups).
  Colliders made with `Collider::circle`, `rect`, `capsule` or `polygon`
  are unchanged and start in every group; saved colliders without the field
  load in every group.
  Instead: build colliders with their constructors, or add
  `groups: CollisionGroups::ALL` to a struct literal.
  Before:
  ```rust
  let c = ponko::d2::Collider { shape, density: 1.0, friction: 0.5, restitution: 0.0, sensor: false, offset: Vec2::ZERO };
  ```
  After:
  ```rust
  let c = ponko::d2::Collider { shape, density: 1.0, friction: 0.5, restitution: 0.0, sensor: false, offset: Vec2::ZERO, groups: ponko::d2::CollisionGroups::ALL };
  ```
- `d2::QueryFilter` has a `groups` field, as `d3::QueryFilter` does.
  Instead: build filters with `QueryFilter::default()` and its builders
  (`.exclude(e)`, `.groups(g)`, `.with_sensors()`), or add
  `groups: CollisionGroups::ALL` to a struct literal.
  Before:
  ```rust
  let filter = ponko::d2::QueryFilter { exclude: Some(player), sensors: true };
  ```
  After:
  ```rust
  let filter = ponko::d2::QueryFilter::default().exclude(player).with_sensors();
  ```

### Added
- 2D physics queries on par with 3D: `d2::Physics::cast_ray_all` (every
  hit, nearest first, into a buffer you keep), `cast_circle`, `cast_box`
  and `cast_capsule` (swept shapes), `overlap_box` and `overlap_capsule`,
  `closest_point` (the nearest collider and its nearest point, as
  `d2::PointHit`) and `penetrations` (how deep a shape sinks into each
  collider and the way out, as `d2::Penetration`, deepest first). With
  reused buffers none of them allocate.
- 2D collision groups: `d2::CollisionGroups` (the 2D form of
  `d3::CollisionGroups`, same rules), `Collider::groups` and
  `QueryFilter::groups`. Colliders in groups that don't touch pass through
  each other, and queries see only the colliders their groups touch.
- One set of query rules for 2D and 3D, in both guides and in
  `d2::Physics`'s docs: direction normalization, range, starting inside a
  collider, order with ties broken by entity id.
- C: `ponko_ctx_cast_ray2d`, `ponko_ctx_cast_ray2d_all`,
  `ponko_ctx_cast_circle2d`, `ponko_ctx_cast_box2d`,
  `ponko_ctx_cast_capsule2d`, `ponko_ctx_overlap_point2d`,
  `ponko_ctx_overlap_circle2d`, `ponko_ctx_overlap_box2d`,
  `ponko_ctx_overlap_capsule2d`, `ponko_ctx_closest_point2d`,
  `ponko_ctx_penetrations2d` (they take the collision groups next to the
  `PonkoQueryFilter2D`), `ponko_set_collider_groups` and
  `ponko_collider_groups`, with `PonkoPointHit2D` and `PonkoPenetration2D`.
  C++: `ctx::cast_ray_all`, `cast_circle`, `cast_box`, `cast_capsule`,
  `overlap_point`, `overlap_circle`, `overlap_box`, `overlap_capsule`,
  `closest_point`, `penetrations`, `filter2d::groups`, and
  `world::set_collider_groups` / `collider_groups`.
- Benchmark `physics/rays-10k`: 10,000 ray casts, all-hit rays and circle
  casts through a 2D scene, through Ponko and through rapier directly.

### Changed
- `d3::Physics::cast_ray_all` orders equal distances by entity id, and 3D
  casts and overlaps with a non-finite origin or rotation, or a negative or
  NaN `max_distance`, find nothing, as in 2D.
- C++ `ctx::cast_ray_filtered` and the 2D `ctx::overlap_ball` honour the
  `filter2d`'s collision groups.
