### Added
- Controllers: `Input::rumble` with dual motors and trigger motors where the
  backend has them (`Rumble`, timed in real time, stopped on focus loss, sent
  to the backend only when the levels change), `Input::gamepad_info` (name,
  USB ids, `ControllerFamily` detection, `GamepadCapabilities`),
  `Input::gamepad_motion` (gyro and accelerometer, `GamepadMotion::aim_delta`)
  and `gamepads_connected` / `gamepads_disconnected` edges.
- Button prompts: `Input::last_device`, `Input::action_glyphs` and
  `action_glyphs_for` turn an action's bindings into `Glyph`s for the device
  used last, labelled for the pad's family; `GlyphAtlas` draws every glyph
  with the engine's own shapes and draws prompt rows (`draw_prompt`) or
  rich-text inline images.
- Local multiplayer: a reconnecting pad gets its old slot back, and players
  are separate from slots (`assign_player`, `player_device`, `free_player`,
  `join_pressed`, `Action::for_pad`).
- `GamepadBackend` to plug in a gamepad source with `Game::gamepad_backend`,
  and `FakeGamepads`, a scripted backend for tests.
- `InputEvent::GamepadConnected` and `InputEvent::GamepadMotion`, with the
  scenario steps `gamepad_connected` and `gamepad_motion`.
- C and C++: rumble, gamepad info, motion, last device, connection edges,
  players, action glyphs and the glyph atlas (`ponko_ctx_gamepad_rumble`,
  `ponko_ctx_action_glyphs`, `ponko_glyph_atlas_*`, ...; `ponko::glyph_atlas`).
- Guide chapter "Controllers", with what the built-in backend supports on each
  platform.
