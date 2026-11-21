### Added
- A crash that is not a panic (`CrashKind::Native`) now has a backtrace
  (the guide's "Backtraces of a native crash"). The crash handler writes raw
  addresses with each frame's module and offset, still only
  async-signal-safe work: on Linux and macOS a walk of the frame records
  from the crashed thread's registers, bounds-checked against the thread's
  stack and read through a pipe so a broken chain can't fault again; on
  Windows the system's table-driven unwinder. The loaded modules (range,
  build id, path) are recorded ahead of time and refreshed off the handler.
  The next start names each frame whose binary is still the same (same path
  and build id) with its function, file and line; the others keep module
  and offset for `addr2line`, `llvm-symbolizer` or `atos`. Deeper than the
  crashing frame needs frame pointers on Linux (`-C force-frame-pointers=yes`);
  macOS keeps them always.
- New dependency on desktop: `backtrace` (rust-lang), to resolve addresses
  the engine didn't capture itself (std has no public way to).

### Changed
- The test whose crashing child really dies of the signal stays Linux-only
  by default, now with the reason written down (the guide's "How it is
  tested"); with `PONKO_CRASH_TESTS_DIE=1` on Windows its child turns off the
  error dialog.
