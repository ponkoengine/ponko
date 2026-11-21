### Added
- The GPU timing choice from C and C++: `ponko_ctx_set_gpu_timing(ctx, kind,
  every)` and `ponko_ctx_gpu_timing(ctx, &kind, &every)` with
  `PONKO_GPU_TIMING_AUTO`, `_EVERY` (every `every`th frame) and `_OFF`; C++
  `ctx.set_gpu_timing(ponko::gpu_timing_every(4))` and `ctx.gpu_timing()`
  (also `gpu_timing_auto()` and `gpu_timing_off()`). A kind that is none of
  the three is `PONKO_INVALID_ARGUMENT` and changes nothing. The guide's
  "How often GPU passes are timed" shows both languages (issue #199).
- `perf.get`'s `render_inspector.gpu_timing` of the control endpoint has
  `cadence` (`auto`, `every` or `off`, and `every`: the frame count for
  `every`): the game's own choice from `set_gpu_timing`. A tool can tell a
  game that turned GPU timing off (`cadence: "off"`, `available: true`, and
  `unavailable.gpu_ms.frame` says so) from an adapter without timestamp
  queries (`available: false`). Fields are only added: schema 1 stays.

### Changed
- `GpuTiming`, `Perf::set_gpu_timing` and `Perf::gpu_timing` are no longer
  Rust-only: their rows leave `scripts/rust-only.txt` and the "Still Rust-only"
  table of the C and C++ chapter.
- The written list of checks (`.github/workflows/ci.yml`) builds
  `tests/ui_access_window.rs` in the accessibility step (built, not run: it
  needs a person), and runs the native crash backtrace tests on Linux in a
  build with frame pointers (`-C force-frame-pointers=yes`,
  `PONKO_CRASH_TESTS_FRAME_POINTERS=1`), which checks the whole chain of
  frames there (issues #163, #174).
