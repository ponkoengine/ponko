### Added
- C and C++ games build for the browser: `web/build-cpp.sh` compiles them
  with the WASI SDK's clang (33 or newer, for C++ exceptions) and links them
  into the engine's ordinary web build, writing the same static site as
  `web/build.sh` (the web guide's "C and C++"). In a browser
  `ponko_game_run` doesn't return while the game plays, and nothing in the
  calling functions is destroyed, so locals the systems capture by
  reference stay valid as on the desktop. `printf` and `std::cout` go to the
  browser's console. `web/smoke/cpp.mjs` checks the guide's first C++ game in
  headless Chromium.
