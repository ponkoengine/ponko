### Added
- Fallible startup with `Game::try_run`, `ConfiguredGame::try_run`,
  `try_run_headless`, `try_setup` and the default-compatible `Plugin::try_setup`.
  Setup errors stop subsequent callbacks and systems and retain their details.
- All consuming configuration builders remain available after `setup`, guarded
  by a test that checks new `Game` builder declarations for matching forwards.
- GPU startup returns surface, adapter, device and surface-configuration errors
  directly, without unwinding or invoking the panic hook, including abort builds.
  Later device/surface loss is outside the startup error contract.
- C: `ponko_game_run` returns a refused window, GPU or event loop as a status
  with `ponko_last_error()` and kind `PONKO_ERROR_UNSUPPORTED`, where it
  reported a caught panic (`PONKO_PANIC`) before.
