### Added
- `examples/cpp/coin_dash`: the reference game Coin Dash written in C++ on
  `ponko.hpp`, with the same rules, screens, settings, rebinding, saves and
  languages. Its scenario test plays the Rust game's menu and key-press
  scenarios headlessly and makes the same assertions; it and a headless
  walk-through (`--smoke headless`) run in the native `ctest` step. The guide's
  "C and C++" chapter has a section on the port.
- The Rust game's scenarios are kept in `examples/coin_dash/scenarios/*.json`;
  its tests check that they still match (`PONKO_UPDATE_SCENARIOS=1` rewrites
  them), so the Rust and C++ games run the same scripts.
- C functions with C++ wrappers: `ponko_world_clear` (`world_view::clear`),
  `ponko_world_state` (`world_view::state<S>()`, the state of a world a
  headless run returned), `ponko_ctx_strings_number` (`ctx::format_number`),
  `ponko_ctx_strings_revision` (`ctx::strings_revision`) and
  `ponko_ctx_storage_is_available` (`ctx::storage_is_available`).
  `ui::paint` can be made from a `PonkoUiPaint`.
