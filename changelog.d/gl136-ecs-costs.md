### Changed
- Spawning is faster: a bundle's values are moved with copies of their
  own, constant size instead of `memcpy` calls sized from the column, and
  `spawn_batch` looks its bundle up once per batch instead of once per
  entity. At 100k entities: one-by-one spawns 0.71× the time they took,
  `spawn_batch` 0.46×, spawn-and-despawn churn 0.83×; against the code
  before required components and the change summary, 0.75×, 0.50× and
  0.84× the instructions.
- A spawn that fills a gap (its bundle lacks a component it requires)
  reads one cache entry instead of two and reserves one row instead of
  two, and a batch of such bundles without `on_add` hooks runs one loop
  like a plain batch. Filling the gap now costs nothing measurable in a
  batch and 0.006 ms per 100k spawns in churn (0.032 before); one by one,
  about 2 ns per spawn, 39 instructions instead of 81. Gap-filling spawns
  take 0.73× (one by one), 0.43× (batch) and 0.78× (churn) the time they
  took.
- Despawning from an archetype with tracked components logs the removals
  and moves the last row's stamps in one pass, and no longer marks the
  despawned row's block as changed: a `Changed` loop afterwards still
  skips that block if nothing in it changed. With one tracked component
  a despawn costs 0.97× what it did before the change summary existed
  (it was 1.07×), with two 1.03× (it was 1.17×); despawns from untracked
  archetypes run 4% fewer instructions.
- Queries with one or two `Option<&T>` / `Option<&mut T>` fetches pick a
  loop per archetype for the components it has, instead of testing them
  on every entity: `ecs/optional` (`(&mut Pos, &Vel, Option<&Boost>)`)
  takes 0.22 ms instead of 0.51 ms at 1M entities and 0.022 instead of
  0.048 ms at 100k, as fast as hecs. This applies to `for_each` in tracked
  worlds and in views too; parallel loops and queries with three or more
  options keep one loop.
- The numbers above: release builds on an Apple M4 Max shared with other
  builds (load 75–185), the code before and after these changes interleaved,
  8 rounds (faster in 7 or
  8 of them on every row above), and instruction counts from one process
  that links both versions. `crates/ponko_bench/ECS.md` ("Costs won back since the
  baselines") and `REQUIRED.md` have the tables; the guide's performance
  chapter states the costs that remain ("What ECS features cost").
