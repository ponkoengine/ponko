# Migrating from 0.1 to 0.2

## Gamepad capabilities

`GamepadCapabilities` includes `light`, `player_leds`, `touchpad` and
`adaptive_triggers`. Custom gamepad backends can fill in the capabilities they
provide and default the rest to false. Change an explicit struct literal like

```text
GamepadCapabilities { rumble: true, trigger_rumble: false, motion: false }
```

to

```rust
use ponko::input::GamepadCapabilities;
let caps = GamepadCapabilities { rumble: true, ..Default::default() };
assert!(caps.rumble);
```

C and C++ callers can query the extra capabilities through
`ponko_ctx_gamepad_extras` or `ctx.gamepad_extras`. The existing
`PonkoGamepadInfo` layout is unchanged.
