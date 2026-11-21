### Added
- Data-only mods (`ponko::mods`): `ModPack` (a `.pack` with a `mod.json`
  manifest, or files in memory), `ModSet::mount` (mounts mods in the declared
  load order, moved after the mods each `requires` or loads `after`; a later
  mod's file replaces an earlier one's and the game's own by path), and the
  `Mods` resource (`list`, `provider`, `files_under`, `fingerprint`).
  Every mod is checked before any is mounted, and a bad one is refused whole
  with a `ModError` naming the mod, the file and the reason: an unknown
  component or resource, a field of the wrong type, a missing dependency, a
  cycle in the order, a damaged pack, a name like `../x`, a file over
  `ModLimits`. `ModSet::problems` lists them for a mods menu.
- C and C++: `ponko_ctx_mount_mods`, `ponko_ctx_mod_count`,
  `ponko_ctx_mod_id`, `ponko_ctx_mod_provider`, `ponko_ctx_mod_file_under`
  and `ponko_ctx_mods_fingerprint`; `ctx.mount_mods`, `ctx.mod_ids`,
  `ctx.mod_provider`, `ctx.mod_files_under` and `ctx.mods_fingerprint`.
- The guide's "Mods" chapter, with the comparison of data-only mods,
  WebAssembly modules (wasmtime, wasmi) and scripting (Rhai, Lua) on a mod
  that adds an enemy to Ember Hop: safety, determinism, cost per call, added
  size and web support, and the decision.
