### Added
- Code mods (feature `mods-wasm`, off by default): a mod's manifest may name
  a WebAssembly module (`"code": "frost/crawl.wasm"`), run by the wasmi
  interpreter on every platform the engine builds for, the web included.
  `ModSet::mount` compiles and starts every module before mounting anything;
  the game calls exported functions with `ponko::mods::with_code`
  (`ModCode::function`, `exporting`, `call`, `take_log`), passing and
  getting `ModValue` numbers without allocating. Modules import only the
  host functions `log`, `dt` and `cast_ray` (2D physics); each call gets
  `CodeLimits::fuel_per_call` fuel and a module's memory grows to
  `CodeLimits::memory_bytes` at most (`ModSet::code_limits`). A trap is a
  `ModProblem::Trap`, a module that can't be used `ModProblem::Code`; a mod
  with code is refused by a game built without the feature. The module's
  bytes are part of `Mods::fingerprint`. Measured on the frost drip's
  crawl (`ponko_bench -- mods/`): 204 ns per drip as a code mod, 98 ns as
  the game's Rust rule.
- Mounting mods checks every asset their prefabs and scenes name
  (textures, skeletons, meshes, collision meshes, included prefabs, fields
  registered with `Assets::scene_asset_field`) and refuses a mod with
  missing ones in one `ModProblem::MissingAssets` listing each with the file
  that names it, instead of failing at first use.
- C and C++: `ponko_ctx_mount_mods_from_memory` (`PonkoModBytes`) and
  `ctx.mount_mods_from_memory` mount mod packs held in memory.
- The guide's "Mods" chapter: code mods, their sandbox, which backend runs
  where and what it costs.

### Changed
- `ModSet::mount` now refuses a mod whose prefabs or scenes name an asset
  that neither a mod of the set nor the game has; such a mod used to mount
  and fail when the asset was first loaded. Ship the asset in the mod, or
  fix the name.
