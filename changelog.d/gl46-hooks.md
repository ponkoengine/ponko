### Added
- Component hooks: `World::on_add::<T>`, `World::on_replace::<T>` and
  `World::on_remove::<T>` register functions the world calls inside the
  operation that adds, overwrites or removes a `T` (every kind of spawn,
  `insert`, `remove`, `despawn`, `clear`, `reset`, loading a save). A hook
  gets a `HookWorld` (in the prelude): components, resources, queries and
  `resource_scope`, with spawns, despawns, inserts and removes recorded to
  `commands()` and applied before the call returns. Hooks run in
  component-id order, then registration order, and never during parallel
  queries. Components without hooks cost nothing extra: spawning runs the
  same code as before, despawn, insert and remove test one flag
  (measurements in `crates/ponko_bench/HOOKS.md`). Rust only for now; C and
  C++ can't register hooks yet, but their structural changes run the
  hooks registered from Rust. See "Component hooks" in the guide's ECS
  chapter.
- `ponko_bench`: `ecs/*/ponko-hooks-elsewhere` and `ecs/*/ponko-hooked`
  rows, and `hierarchy/100k-pairs-parent-churn` (parents despawned every
  update).

### Changed
- The transform hierarchy finds the children of a despawned parent through
  an `on_remove` hook on `Children` instead of scanning every link: a
  despawned parent costs its own children, not one pass over all entities
  (`hierarchy/100k-pairs-parent-churn`: 0.41 ms instead of 0.51 ms at best).
