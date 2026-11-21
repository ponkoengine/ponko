### Added

- Controllers in web builds with threads (`web/build.sh --threads`): the
  engine reads `navigator.getGamepads()` on the page's thread every frame,
  copies the buttons and axes into plain numbers and turns the changes into
  its gamepad events, so it keeps no browser object that would have to be
  sent between threads. `ctx.input.gamepads()` and the rest of the gamepad
  input work as in the ordinary web build (no rumble on the web in either).
- `web/smoke/threads.mjs --case gamepads` feeds the threaded build fake
  browser gamepads in headless Chromium and checks each step reaches the
  game; `--case warning` proves a warning fails the threaded build.
- `web/build.sh --print-env` prints the toolchain and the flags a threaded
  build would use, without building.

### Fixed

- `web/build.sh --threads` dropped the caller's flags, or its own: it put
  its flags in `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS`, which cargo
  ignores once `RUSTFLAGS` is set, so with the check list's
  `RUSTFLAGS="-D warnings"` the build lost its atomics and shared memory.
  It now adds its flags to the variable cargo reads and keeps what is
  there, so a warning fails a threaded build. `threads.mjs --build` adds
  `-D warnings` itself.
