### Added
- Controllers: the `gamepad-sdl` feature reads pads through SDL 3 instead of
  gilrs on the desktop: gyro and accelerometer (DualShock 4, DualSense,
  Switch Pro, Joy-Cons), trigger rumble (Xbox One and Series pads) and body
  rumble on macOS too, with the same `ctx.input` API. It links the SDL 3
  library; gilrs stays the default and the fallback. The guide's
  controllers chapter has why SDL was picked and what each platform gets.
- Key labels follow the keyboard layout: `Input::key_labels` (a
  `KeyLabels`) labels the letter, digit and punctuation keys as on the
  player's keyboard (read from the system on Windows and macOS when the
  window opens or regains focus, and learned from each key pressed on every
  desktop system and the web), `KeyLabels::text` for text prompts,
  `GlyphAtlas::set_key_labels` for key caps, `Input::set_key_labels` and
  `KeyLabels::from_layout` for a layout option or a test. The C atlas calls
  (`ponko_glyph_atlas_draw_prompt`, `ponko_glyph_atlas_texture`) label the
  caps for the player's layout themselves.
- `InputScenario::connect_gamepad`, and an optional `info` object on the
  `gamepad_connected` scenario step (name, family, ids, capabilities).

### Changed
- Input recordings now keep gamepads connecting (with their `GamepadInfo`)
  and gamepad motion readings, and a recording started with pads connected
  begins with them, so a replay needs no gamepad backend. The MCP tool
  `run_input_scenario` describes the `info` object and motion's units.
