# Agent instructions

plugin-crate is a plugin crate for the Ponko game engine
(<https://ponko.dev/guide/>): a library that games add with
`Game::plugin`. It has no window and no `main`.

## Commands

- `cargo test`: the headless tests in `tests/plugin.rs` and the doctests.
  Run them after every change.
- `cargo doc --open`: the API as the plugin's users see it.
- Never open a window in tests: use `run_headless(ticks)`.

## Rules for a plugin crate

- Use only Ponko's public API: `ponko::prelude::*` and the public modules.
  Never `#[doc(hidden)]` items (`ponko::__ffi`, ...): they change in any
  release.
- Depend on `ponko` with `default-features = false` and only the features
  the code needs; the game chooses the others.
- Everything the plugin adds is named after the crate: the plugin type
  (`PulsePlugin`), debug toggles and control methods (`plugin_crate.`...).
- A breaking change to the plugin's API is a new minor version before 1.0
  (0.1 -> 0.2), a new major version after. Supporting a new Ponko minor
  version is a breaking change too.
- The guide's chapter "Plugins for other crates"
  (<https://ponko.dev/guide/plugins.html>) has the details.

## Layout

- `src/lib.rs`: the plugin, its component and its systems.
- `tests/plugin.rs`: games built with the plugin, run headless.
