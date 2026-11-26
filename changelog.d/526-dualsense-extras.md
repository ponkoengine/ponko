### Added

- The SDL gamepad backend reads DualSense and DualShock touchpads (`GamepadButton::Touchpad`, `Input::gamepad_touchpad`, `Input::gamepad_touchpad_events`) and can set supported controllers' light bars and player LEDs (`Input::set_gamepad_light`, `Input::set_gamepad_player_leds`). DualSense adaptive triggers (`Input::set_gamepad_trigger_effects`, `TriggerEffect`) support resistance, a break point, vibration, and a ten-zone resistance profile over USB and Bluetooth. Outputs are cleared when a controller is lost and when the backend shuts down. `GamepadCapabilities` gains `light`, `player_leds`, `touchpad` and `adaptive_triggers`. These controls are available from Rust, C (`ponko_ctx_gamepad_extras` and friends) and C++.

### Breaking changes
- `GamepadCapabilities` has four new fields: `light`, `player_leds`,
  `touchpad` and `adaptive_triggers`.
  A custom gamepad backend that builds it with a struct literal naming every
  field no longer compiles; reading the fields and `Default` are unchanged.
  The C `PonkoGamepadInfo` layout is unchanged: C and C++ read the new flags
  through `ponko_ctx_gamepad_extras` or `ctx.gamepad_extras`.
  Instead: name the capabilities the backend provides and end the literal
  with `..Default::default()`.
  Before:
  ```rust
  let caps = GamepadCapabilities { rumble: true, trigger_rumble: false, motion: false };
  ```
  After:
  ```rust
  use ponko::input::GamepadCapabilities;
  let caps = GamepadCapabilities { rumble: true, ..Default::default() };
  ```
