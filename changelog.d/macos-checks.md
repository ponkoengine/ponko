### Fixed
- The `ponko_aseprite` and `ponko_lobby` crates ship the license files and
  the notice (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`) like the other
  published crates; `scripts/release-check.py selftest` failed without them.
- `scripts/check-ffi-coverage.py` passes again: the maturity line of the
  "Threads on the web" chapter read as a "Rust-only" API note.
- The API listing in `api/` names `World::insert_many` and
  `World::remove_many`, and lists the joystick and force feedback items in
  the order `scripts/api-snapshots.py` writes them, so its check passes.
- The asset group test `without_workers_other_assets_load_as_before` passes
  with the `image-gif` feature (and `image-formats`): it expected a GIF to be
  unsupported, which it is only without the decoder.
- The macOS screen-reader checks (`tests/ui_access_macos.rs`) count a check
  that didn't run (the window check, without `PONKO_ACCESS_WINDOW=1`) as
  skipped in their summary, not as passed.
- The docs step of `scripts/checks.yml` documents `ponko` with `mods-wasm`,
  as docs.rs does; without it the code mod links in `ponko::mods` failed
  the step.
- The C++ wrapper test of allocation counts passes in a Release build: clang
  removed its `new`/`delete` pair there, so nothing was counted.
