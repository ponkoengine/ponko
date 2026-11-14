### Breaking changes
- `d3::NavObstacle` has `yaw` and `shape` fields and is `#[non_exhaustive]`.
  Obstacles can be turned boxes, cylinders and convex prisms now, so struct
  literals no longer build one.
  Instead: build it with `NavObstacle::new`, `NavObstacle::cylinder` or
  `NavObstacle::convex`, then set the fields you need.
  Before:
  ```rust
  let obstacle = NavObstacle { center, half_extents, carve: true };
  ```
  After:
  ```rust
  let mut obstacle = NavObstacle::new(center, half_extents);
  obstacle.carve = true;
  ```

### Added
- 3D navigation obstacles take a shape and a turn: `NavObstacle::yaw` turns
  one around world Y, and `NavObstacle::shape` (`d3::NavObstacleShape`) makes
  it a box (the default), a cylinder (`NavObstacle::cylinder`) or a convex
  prism of up to 8 points (`NavObstacle::convex`, `d3::NavConvexPolygon`).
  Carving cuts the exact shape; blocking tests polygons against it instead of
  its world bounds. Rust-only for now: C and C++ obstacles stay unturned boxes.
- `ponko_bench`'s `nav3d/carve-tile-flat` and `nav3d/carve-tile-bumpy` time
  one carved tile rebuild (about 0.1 ms per 32-cell tile).

### Fixed
- Tiles of a carving bake, and tiles rebuilt by carving, keep detail heights:
  agents next to a carved hole on uneven ground follow the ground as closely
  as on an ordinary bake instead of floating or sinking between polygon
  corners.
- Carving no longer stops at 256 obstacles or 8 layers per obstacle, and an
  edit that touches no tile no longer uses a step of the `update_carving`
  budget.
