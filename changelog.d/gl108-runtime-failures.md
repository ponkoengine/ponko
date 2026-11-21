### Added
- Web games report failures that stop them after startup: a panic (in setup
  or later), the WebGL2 context lost while playing, and a worker thread that
  stopped. Pages subscribe with `onPonkoFailure` in the generated
  JavaScript; the callback gets the failure's `kind`, `message`, a panic's
  `location` and the game's own `text`.
- `ponko::web::on_failure` lets a web game write the text the page shows
  for such a failure (`ponko::web::Failure`, `ponko::web::FailureKind`).

### Fixed
- The web page and the gallery's demo page show a panic or other fatal
  failure while the game runs, with its message, instead of leaving a
  frozen canvas (the template had already removed its status line) or
  showing only the `RuntimeError: unreachable` the abort throws.
