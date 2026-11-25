### Fixed
- The `ponko_aseprite` and `ponko_lobby` crates ship the license files and
  the notice (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`) like the other
  published crates; `scripts/release-check.py selftest` failed without them.
- `scripts/check-ffi-coverage.py` passes again: the maturity line of the
  "Threads on the web" chapter read as a "Rust-only" API note.
- The API listing in `api/` names `World::insert_many` and
  `World::remove_many`, and lists the joystick and force feedback items in
  the order `scripts/api-snapshots.py` writes them, so its check passes.
