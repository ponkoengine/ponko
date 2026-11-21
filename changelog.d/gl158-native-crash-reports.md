### Added
- Crash reports for crashes that are not panics (the guide's "Crashes that
  are not panics"): a segmentation fault, bus error, illegal instruction,
  arithmetic fault or `abort()` in native code on Linux and macOS, and an
  unhandled exception or a C runtime `abort()` on Windows, leave a report of
  the new kind `CrashKind::Native`, with the signal (`CrashReport::signal`),
  the game's and engine's versions, the system, the GPU, the settings of the
  last change and the last lines the game printed. The engine's handler only
  writes bytes prepared ahead of time into a file opened ahead of time
  (`crash-reports/.native-<pid>-<n>`, deleted when a run ends normally), then
  hands the crash on to the handler that was there before; the game's next
  start turns the file into an ordinary report, lists it in
  `crash::saved_reports` and runs the `on_crash` hook with it.
- The browser runner `web/smoke/runtime-failures.mjs` checks that every
  failure that stops a running game, a lost WebGPU device included, hands
  the page its crash report (`window.ponkoCrashReport`).
