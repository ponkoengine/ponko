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
