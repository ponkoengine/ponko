### Added

- Rumble in web builds where the browser has it: the engine plays the two
  body motors through the pad's `vibrationActuator` (`playEffect` with
  `dual-rumble`), in the ordinary web build and in builds with threads. A
  browser ends an effect after at most 5 seconds, so the engine plays short
  effects and plays the same levels again every second until the rumble
  ends; a stalled game stops buzzing within 2 seconds. `GamepadInfo`
  reports `capabilities.rumble` for a pad that has an actuator. Trigger
  motors are not used.
- `web/smoke/threads.mjs --case gamepads` feeds pads with a fake
  `vibrationActuator` and checks the game's rumble reaches it (the levels,
  played again before the cap, stopped at the end); `--case gamepads-plain`
  does the same on the ordinary build, whose pads come from gilrs.
- `web/build.sh --threads --clippy` runs clippy on the threaded web
  configuration with the toolchain and flags of a threaded build.

### Fixed

- The four lints the nightly clippy reports in the threaded web
  configuration (an overindented doc list, two `then` closures that needed
  no closure, and a `Box::pin` that `pin!` does): the check list now lints
  that configuration.
