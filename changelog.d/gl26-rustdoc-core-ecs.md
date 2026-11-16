### Changed
- Missing documentation is now a compile error (`#![deny(missing_docs)]`)
  in `ponko_ecs`, `ponko_macros` and the engine's core modules (game, ctx,
  schedule, time, events, input, actions, assets, storage, scene, reflect,
  error, handle). A new public item there needs a doc comment.
- Panics of the ECS and startup APIs are documented under `# Panics`
  (spawning a bundle with a component twice, undeclared view access,
  failing setup in `run_headless`), and returned errors under `# Errors`.
- The most used ECS and engine items (`World` entity and resource calls,
  `Entity`, `Bundle`, input state, `Time`, `Storage`, `Events`, `Error`,
  `MemorySource` and the `Game` builder) have doc examples that run
  headless as doctests.
