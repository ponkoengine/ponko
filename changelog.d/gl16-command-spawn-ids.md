### Breaking changes
- `Commands::spawn` returns the new `Entity` instead of `()`.
  A closure or function whose last expression was `commands.spawn(...)`
  now returns an `Entity`, so a system written that way no longer fits
  where `()` is expected.
  Instead: end the call with a semicolon, or use the returned entity.
  Before:
  ```rust
  game.sim_system(|ctx: &mut Ctx| ctx.commands.spawn((Marker,)));
  ```
  After:
  ```rust
  # #[derive(Component)] struct Marker;
  # let game = Game::new("g", 1, 1);
  game.sim_system(|ctx: &mut Ctx| {
      ctx.commands.spawn((Marker,));
  });
  ```
- `Commands::spawn` on a buffer that isn't tied to a world panics.
  The id is reserved in the buffer's world when `spawn` returns, so a
  buffer from `Commands::new()` must know its world first. `ctx.commands`
  and a view system's `sys.commands` already do; applying a buffer once
  ties it to that world.
  Instead: make your own buffers with `world.commands()`, or call
  `commands.bind(&world)` before the first spawn.
  Before:
  ```rust
  let mut commands = Commands::new();
  commands.spawn((Marker,));
  world.apply(&mut commands);
  ```
  After:
  ```rust
  # #[derive(Component)] struct Marker;
  let mut commands = world.commands();
  commands.spawn((Marker,));
  world.apply(&mut commands);
  ```

### Added
- `Commands::spawn` returns the new entity right away: later commands of the
  same buffer (and anything else) can use it before the spawn is applied.
  The id is reserved in the world, so it never collides with other spawns,
  also from buffers on other threads; if the spawn never happens (the
  buffer is cleared or dropped, an earlier command panics, the bundle can't
  be spawned), the id is given back and its handle stays dead.
  `World::commands`, `Commands::bind` and `Commands::is_bound_to` tie a
  buffer to a world. Ids are deterministic like the rest of the world: the
  same spawns in the same order get the same ids, with the parallel
  scheduler on or off and on any number of threads. Spawns recorded from
  the threads of a parallel query get distinct ids, all applied, but which
  item gets which id depends on timing (STABILITY.md says so; spawn after
  `par_collect_into` to keep them deterministic).
  Direct spawns and despawns cost what they did: the world takes new
  indices for its own spawns 256 at a time and touches nothing it shares
  with command buffers. A deferred spawn costs about 2 ns more (its slot
  is set aside at an earlier apply, its id goes through the buffer's list,
  and the slot is touched once more to claim it). Measured with
  `ponko_bench` in release builds (fastest of 10 runs for `spawn`/`despawn`,
  of 50 for `commands`; three interleaved runs of the code before and after;
  M4 Max at load average 58–61 from other builds), milliseconds:

  | Benchmark | Before | Reserved ids |
  |---|---|---|
  | `spawn/ponko` | 13.85–13.99 | 13.74–14.51 |
  | `spawn/ponko-batch` | 10.34–10.64 | 9.96–10.45 |
  | `despawn/ponko` | 8.14–8.54 | 8.23–8.26 |
  | `commands/insert-remove` | 4.59–4.76 | 4.71–4.77 |
  | `commands/spawn-despawn` | 2.38–2.42 | 2.59–2.83 |
  | `commands/spawn-then-despawn` | | 2.39–2.40 |
  | `commands/spawn-link` | | 3.18–3.34 |
  | `commands/spawn-then-link` | | 2.90–3.09 |

  Chosen over a `spawn_then(bundle, |world, entity| ...)` callback (the
  `spawn-then` rows), which costs what a spawn did, because a reserved id
  is an ordinary value at once: it can be stored, returned, linked both
  ways between new entities and passed to C. Lock-free reservation on a
  cursor over the world's free list was ruled out because buffers may
  record on other threads while the world spawns directly, so every direct
  spawn would update that cursor atomically: one uncontended atomic
  operation in `World::spawn` alone cost 3.5% on `spawn/ponko`.
- `World::return_idle_ids` sizes the free slots the world sets aside for
  deferred spawns, once per frame (the engine calls it): as many as command
  buffers took in one frame lately, the rest back to the free list, so a
  game that stops spawning through commands doesn't keep them, and one
  that spawns a burst now and then doesn't grow its slot table.
- C: `ponko_ctx_spawn_later_entity` is `ponko_ctx_spawn_later` that also
  writes the new entity. C++: `ctx::spawn_later` returns the entity.

### Changed
- `World::spawn_at` takes back a free slot that was set aside for command
  buffers, unless a buffer holds it.
