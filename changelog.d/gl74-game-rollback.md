### Added
- `World::save_snapshot` and `World::restore_snapshot` (`ponko::ecs_core::WorldSnapshot`):
  a typed in-memory copy of every component column, the entity allocator,
  the archetype layout and the resources registered with
  `World::snapshot_resource`. A restore keeps entity ids and which ids later
  spawns get, and queries visit entities in the same order. 10,000 entities
  save and restore in about 0.005 ms each, with no allocation after the
  first save. Components must be `Clone` (the derive notices;
  `World::register_clone` for the others).
- `save_state` and `restore_state` on `ctx.physics` and `ctx.physics3d`
  (`d2::PhysicsState`, `d3::PhysicsState`): the whole rapier state with the
  links to entities. A loaded world steps bit for bit like the original.
- `net::GameRollback`: rollback for a whole `Game`. Started with
  `ctx.net.start_rollback`, it runs every simulation tick through a
  `Rollback` session and, when a late input differs from the prediction,
  loads the world, the physics, the events and the tick saved before that
  tick and runs the simulation systems again. Simulation systems read
  `ctx.net.rollback_inputs`; `ctx.net.is_resimulating` tells a tick that
  runs again, whose sounds are dropped. Checksums are world hashes. See the
  guide's "Rolling back a whole `Game`".
