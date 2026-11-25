### Added
- `World::insert_many(&entities, value)` and `World::remove_many::<T>(&entities)`
  give many entities a component, or take it off them, in one call. The
  result is the same as `insert` or `remove` on each entity in turn; entities
  that move between the same two archetypes move together, and a whole
  archetype in row order (as a query visits it) is copied in one piece:
  6 to 13 times faster than single moves in the `ecs/add-remove` benchmark.
  Rust only for now; no C or C++ binding yet.
