### Fixed
- 2D circle, box and capsule casts starting inside a collider report a hit
  at distance zero with a push-out normal in either direction, including
  when the maximum distance is zero.
- 2D penetration queries reuse per-thread solver scratch memory after
  warm-up, including against polygon colliders. Reused overlap and hit
  buffers remain allocation-free; polygon query shapes still build their
  convex hull on each call.
- Regression coverage checks non-finite directions and rotations in 2D
  and the shared 2D/3D query rules; 2D already rejected these inputs.
