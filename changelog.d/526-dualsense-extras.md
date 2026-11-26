### Added

- The SDL gamepad backend reads DualSense and DualShock touchpads and can set supported controllers' light bars and player LEDs. DualSense adaptive triggers support resistance, a break point, vibration, and a ten-zone resistance profile over USB and Bluetooth. These controls are available from Rust, C and C++.

### Changed

- **Breaking:** Rust `GamepadCapabilities` struct literals must account for the new `light`, `player_leds`, `touchpad` and `adaptive_triggers` fields. Use `..Default::default()` for capabilities a backend does not provide.
