### Changed
- `par_for_each`, `par_for_each_indexed` and `par_collect_into` with one or
  two `Option` fetches pick the loop per archetype, as `for_each` does, and
  keep the closure's captured values out of the loop: about 0.15× the
  instructions per entity, where they used to test every row.
- A `Changed`/`Added` filter after inserts and removes reads only the
  blocks where something changed: moving an entity keeps its stamps exact
  instead of marking its new block of 64 rows as changed.
- Change filters over many archetypes cost about 0.6–0.8× the instructions:
  the row filter is filled in place for each archetype.
- Despawning entities with tracked components costs less: removal logs
  keep one entry per tick, not one tick per entity (0.94× the instructions
  with two tracked components).
- Hierarchy propagation indexes each level on the calling thread, which
  was as fast as all cores and doesn't wait for busy worker threads.
