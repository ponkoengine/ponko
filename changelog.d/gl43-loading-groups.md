### Added

- Asset groups for loading screens: `AssetGroup` declares the textures,
  streamed textures, meshes, sounds and your own formats (`with`, any
  bytes-to-value loader) a level needs; `ctx.assets.start_group` loads them
  on worker threads with progress by count and by bytes
  (`group_progress`), a ready or failed state that names the failing path
  (`group_state`, `group_error`), and `cancel_group`. A per-frame step with
  a time budget (`set_group_frame_budget`, 4 ms by default) and the shared
  upload budget keeps frames short; `group_stats` reports its cost.
- `Game::loading_state(loading, group, then)`: a game state that loads a
  group and moves on once it is ready, cancelling it if left early.
- `ctx.assets.add_background_source` for asset sources that worker threads
  may read, and `AssetSource::size` so loading bars know totals up front
  (`MemorySource` and `PackSource` implement both).
- C and C++: `ponko_asset_group_*`, `ponko_ctx_*asset_group*`,
  `ponko_game_loading_state` and `ponko_ctx_add_background_memory_source`;
  `ponko::asset_group` and the matching `ctx` and `game` methods.
- A guide chapter, "Loading screens: asset groups with progress".

### Changed

- `Ctx::finish_uploads` (`ponko_ctx_finish_uploads`, `ctx.finish_uploads()`
  in C++) also waits for every asset group still loading, reading and
  decoding the rest of it without the budgets. Games that start no groups
  see no change; a game that calls it at a checkpoint while a loading
  screen's group is under way now waits for that group too.
