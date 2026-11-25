### Fixed
- The `ponko_aseprite` and `ponko_lobby` crates ship the license files and
  the notice (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`) like the other
  published crates; `scripts/release-check.py selftest` failed without them.
- `scripts/check-ffi-coverage.py` passes again: the maturity line of the
  "Threads on the web" chapter read as a "Rust-only" API note.
