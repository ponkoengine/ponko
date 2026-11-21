### Added
- `Game::restore_window_settings(true)` opens the first window with the
  settings the player saved (`ctx.window.save_settings`, `window.json`): it is
  created at the saved size and position, in the saved mode, with the saved
  limits and flags, so no default-sized window shows first. The saved choice
  wins over `Game::fullscreen` and `Game::new`'s size; the title stays the
  game's. A missing, unreadable or invalid file opens the game's own window
  (the reason is printed), a monitor or mode that is gone opens windowed at
  the saved size. Headless runs apply the same settings to `ctx.window` before
  setup; the web restores once its storage is read, before setup. Guide:
  "Remember the player's choice" in the window chapter. Rust-only for now.
- The guide's plugins chapter points at the companion plugins repository
  (`ponko-plugins`, beside the engine): a workspace of plugin crates made
  from the plugin template, built against an engine checkout beside it,
  with a check script for each crate's layout, clippy and tests. `scripts/project-url.py` knows companion repositories:
  `check` fails on a link to one that isn't beside `PROJECT_URL`, and `set`
  moves those links with the engine's.
- `scripts/bench-aligned.sh`: cargo with every function aligned to 128 bytes
  (`-C llvm-args=-align-all-functions=7`), so two builds compared by a
  benchmark lay out their code the same way and a small difference is the
  code, not where it fell. `ponko_bench` records it as `env.code_align` in
  its JSON, and `bench_track compare` warns when the two sides differ in it.
  The benchmark README ("Two builds, one layout") says how, with a
  measurement: two builds of the same code moved a third of the runner's
  functions without it and none with it.
