### Added
- `tests/ui_access_window.rs`: a check a person runs
  (`cargo test --features accessibility --test ui_access_window -- --ignored`)
  of the accessibility adapter on the engine's own window, on macOS and
  Windows: the window opens far off the screen and the system's
  accessibility API presses a button of the game's UI through it. The
  guide's "Unsafe code" section lists the checks that need a person.
- A test that replicates a component registered from C (every field type,
  at unaligned offsets) over the in-memory scripted network, so the C
  replication read and write code runs under Miri.

### Changed
- `scripts/check-unsafe.py` covers the benchmark binary `crates/ponko_bench`
  (every unsafe item there has a test, most under Miri), names checks of
  test programs without the harness, and no longer accepts an unsafe item
  without a test.
