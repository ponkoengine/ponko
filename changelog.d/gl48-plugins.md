### Added
- Plugins are the extension point for other people's crates (guide:
  "Plugins for other crates"). `Plugin::name` (the type's path by default)
  and `Plugin::dependencies` (names that must be added first) make
  dependencies and their order explicit; `Game::has_plugin` lets a plugin
  bring a dependency along. A duplicate or a missing dependency fails
  startup with an `ErrorKind::Invalid` error before any setup runs.
- `Game::debug_toggle` adds a named switch for a game's or plugin's own
  debug drawing: `DebugDraw::toggled`, `set_toggle` and `toggles`; the
  control endpoint's `debug.draw` takes and reports `toggles`, and so does
  ponko-mcp's `debug_draw` tool.
- `Game::control_method` adds a namespaced method (`weather.set_rain`) to
  the control endpoint; `methods` and `capabilities` list it with
  `"added": true`, `capabilities` lists the game's plugins, and ponko-mcp's
  new `call_game_method` tool calls it.
- `ponko new --plugin`: a plugin crate (library, headless tests, the engine
  without default features, README with a version table, AGENTS.md).
  `tests/plugin-crate` is that template built as a separate Cargo project
  outside the workspace, so the engine's tests prove a plugin compiles
  against the public API alone.

### Breaking changes
- Adding a plugin with the same name twice makes the game fail to start.
  A plugin's name is its type's path, so two instances of one plugin type
  used to run both; now the second is an error (`ErrorKind::Invalid`).
  Instead: give each instance its own name with `Plugin::name`, or add the
  plugin once.
  Before:
  ```rust
  impl Plugin for Level {
      fn build(&self, game: Game) -> Game { game }
  }
  Game::new("g", 1, 1).plugin(Level("cave")).plugin(Level("tower"));
  ```
  After:
  ```rust
  # struct Level(&'static str);
  impl Plugin for Level {
      fn build(&self, game: Game) -> Game { game }
      fn name(&self) -> &str { self.0 }
  }
  Game::new("g", 1, 1).plugin(Level("cave")).plugin(Level("tower"));
  ```
