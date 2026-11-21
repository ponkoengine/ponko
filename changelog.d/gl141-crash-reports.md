### Added
- Crash reports for shipped games (`ponko::crash`, the guide's "Crash
  reports"): a panic or a fatal engine error writes a text report into
  `crash-reports/` of the game's data folder (`ctx.storage`): the message,
  location and backtrace, the game's and engine's versions, the OS, the GPU
  and its driver, the graphics settings of the last frame and the last 100
  lines the game printed, with every path made relative to the game's
  folders. `Game::crash_reports(true | false | CrashReports)` is the one
  setting; `CrashReports::on_crash` runs the game's own code with each report
  (its own message, or sending the file: the engine sends nothing),
  `crash::saved_reports` lists earlier runs' reports, and `crash::report`
  writes one for a game's own fatal error. The newest ten are kept.
- Web builds hand a report to the page as a `ponkocrashreport` event (and
  `window.ponkoCrashReport`); the generated page offers it as a file to save.

### Changed
- Release builds of a game with a window now write crash reports by default
  (development builds and headless runs don't), and while they do, standard
  output and error pass through the engine's log capture (the terminal still
  shows everything). `Game::crash_reports(false)` turns this off.
