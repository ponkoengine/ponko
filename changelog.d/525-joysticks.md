### Added

- Steering wheels, pedals, flight sticks and other joysticks: `ctx.input.joysticks()`,
  `joystick_info` (kind, ids, how many axes, buttons and hats, which effects it can play),
  `joystick_axis`, `joystick_down` / `joystick_pressed` / `joystick_released` and
  `joystick_hat`, by number, in their own slots (`MAX_JOYSTICKS`) kept for a device that
  comes back.
- Force feedback on joysticks: `set_force_feedback(joystick, ForceFeedback { .. })` plays a
  constant push, a centring spring, a damper, friction, a vibration and the device's own
  centring until changed, sent only when it changes.
- The SDL backend (`gamepad-sdl`) reads wheels, flight sticks and throttles as joysticks
  (and any device SDL can't read as a gamepad) and plays force feedback through SDL's
  haptic API.
- `GamepadEvents::joystick_*` for backends, `GamepadBackend::set_force_feedback` (default:
  ignored), and `FakeGamepads::connect_joystick`, `set_joystick_axis`,
  `set_joystick_button`, `set_joystick_hat`, `force_feedback` for tests.
- Not yet: joystick bindings in an `ActionMap`, input scenarios, the web and the C API (#525).
