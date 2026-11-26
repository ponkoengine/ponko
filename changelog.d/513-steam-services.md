### Added

- `ponko::steam::Steam` (feature `steam`): achievements and stats (set,
  clear, store, read back), cloud saves kept in step with `ctx.storage`
  (`sync_cloud`: the newest copy wins; the data folder stays the game's
  copy), rich presence, the overlay's state (pause while it is open), and
  the text input of Steam Deck and Big Picture (`show_floating_keyboard`,
  `show_text_input`, `take_text_input`). Without a running Steam every call
  does nothing and the log says so once, so one build runs from any store.
- `Steam::fake()` and `FakeSteam`: an in-memory Steam for headless tests of
  a game's Steam logic.
- C and C++: `ponko_steam_*`, `ponko_ctx_steam_sync_cloud`,
  `ponko_ctx_steam_delete_cloud_file`; C++ `ponko::steam` and
  `ctx.steam_sync_cloud`.
- `ponko package` puts the dynamic libraries a build script made and linked
  (Valve's `libsteam_api` for the `steam` feature) beside the program.
- Guide: "Shipping on Steam" (the app id, depots from `ponko package`, what
  to check before Steam's review).

### Fixed

- The networking guide said `libsteam_api` was in `target/<profile>/` to
  copy by hand; it is in the build's output folder, and `ponko package` now
  ships it.
