### Added
- Code mods reach the game, only where it lets them: new host functions
  `get_field` and `set_field` (a number or bool field of a reflected
  component, `"Health.current"`, `"Transform2D.position.1"`),
  `spawn_prefab` (at a 2D position) and `random` (`[0, 1)`). They reach only
  the entities the game exposed with `ModCode::expose` (taken back with
  `hide`, `hide_all`) and the prefabs it exposed with
  `ModCode::expose_prefab`; a spawned prefab's root is exposed for its mod.
  `random` draws from each mod's own generator, seeded with
  `ModCode::seed_random` (same seed, same sequence). Refusals return
  `REFUSED_ENTITY`, `REFUSED_FIELD` or `REFUSED_VALUE` (NaN from
  `get_field`) and add a `refused:` line to `ModCode::take_log`. Measured
  (`ponko_bench -- mods/host-call`): see the guide's "Code mods".
- C and C++ call code mods: `ponko_ffi`'s new feature `mods-wasm`,
  `ponko_ctx_mod_function` and `ponko_ctx_mod_call` (`PonkoModFn`,
  `PonkoModValue`), `ponko_ctx_mod_expose_entity`,
  `ponko_ctx_mod_hide_entity`, `ponko_ctx_mod_expose_prefab` and
  `ponko_ctx_mod_seed_random`; in C++ `ctx.mod_function`, `ctx.call_mod`
  (`ponko::mod_i32`, `mod_i64`, `mod_f32`, `mod_f64`), `ctx.mod_expose`,
  `ctx.mod_hide`, `ctx.mod_expose_prefab` and `ctx.mod_seed_random`. Built
  without the feature, each reports `PONKO_UNSUPPORTED`.
- The guide's "Mods" chapter: the host functions, how a game exposes
  entities and prefabs, and what a mod can never do.

### Changed
- `cast_ray` in code mods reads the game's physics directly during
  `with_code` instead of swapping the physics world in and out; results
  are the same.
