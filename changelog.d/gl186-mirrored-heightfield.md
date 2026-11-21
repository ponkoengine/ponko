### Fixed
- Terrain: a heightfield `TerrainCollider` (or `Collider::scaled_mesh` of a
  heightfield) on a tile with a negative X or Z scale is solid where the
  tile is drawn. It used to take the scale's size only and stood where the
  unmirrored tile would be. The mirrored copy is built again with its
  heights, holes and cell diagonals flipped, centered by the signed scale,
  so rays, surface normals, resting bodies and collider streaming follow
  the drawn ground. Unmirrored tiles are built as before; a mirrored one
  is built once per scale, about 0.1 ms for 257 × 257 heights. A negative Y
  scale still takes the size only (the ground faces up).
