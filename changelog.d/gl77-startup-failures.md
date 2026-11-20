### Added
- Web games expose their startup result through `ponko::web::on_startup` in
  Rust and `onPonkoStartup` in the generated JavaScript. Startup errors include
  the engine's error kind and message.

### Fixed
- The web page displays a readable error when graphics initialization fails,
  the WebGL2 context is lost during startup, or fallible setup returns an error.
