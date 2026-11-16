### Added
- `Commands::despawn_recursive(e)` (trait `CommandsHierarchy`, in the
  prelude) despawns an entity and everything under it when the commands
  are applied: the subtree as it is then. It works from view systems
  (`sys.commands`) and parallel queries (a shared `Mutex<Commands>`). C:
  `ponko_ctx_despawn_recursive_later`; C++: `ctx.despawn_recursive_later(e)`.
- `Changed<T>` and `Added<T>` skip 64 entities at a time where nobody wrote:
  a tracked column keeps the newest change of each run of 64 rows. With 1M
  tracked entities a `Changed` loop that finds nothing takes 0.017–0.018 ms
  instead of 0.50–0.52 ms; 1% changed, spread out: 0.41–0.43 instead of
  0.55–0.56 ms; all changed: 1.0–1.1 instead of 1.3 ms. A still hierarchy
  of 100,000 attached entities updates in 0.002–0.003 ms instead of
  0.051–0.066 ms. Spawns, inserts, despawns, `&mut` loops and plain
  queries cost the same, within the noise of a busy machine (±4%), except
  a despawn from a tracked archetype, 3–4% slower (12.9–13.4 vs 13.5–14.0
  ms for 1M; untracked despawn is unchanged). Release builds, Apple M4
  Max, load average 38–63, the fastest of each benchmark over three
  interleaved runs.

### Changed
- The hierarchy update places a level with many children (a wide tree,
  many moving roots) in one pass over the attached entities on all cores,
  and compares the roots on all cores. 100,000 roots with one child each,
  all moving: 0.63–1.1 ms per update instead of 2.1–3.9 ms. Per tick of a
  game that moves the roots and follows the attachments, that is 0.70–1.3
  ms, against 0.87–1.3 ms for the attachment update the hierarchy replaced
  (busier runs of it up to 2.6 ms). 100,000 children of one moving root:
  0.20–0.40 instead of 0.71–1.1 ms (3D: 0.26–0.52 instead of 0.97–1.3
  ms). A 100,000-long chain is 2.4–2.5 instead of 2.3–2.4 ms, at the edge
  of the noise. Load average 60–90 during these runs.
- `Children` keeps its first four entries in the component: linking
  100,000 new children to 100,000 parents allocates about 100 times
  instead of 100,151, and the first update takes 5.5 ms instead of
  11.3 ms; 100,000 attachments spawned and followed for 10 ticks:
  860 allocations instead of 100,922.
- `World::allocated_bytes` counts the change summary: 4 bytes per 64 rows
  of capacity of each tracked column.
- The offscreen render test rigs keep their `Ctx` and renderer on the heap,
  so test threads stay well below their 2 MiB stacks.
