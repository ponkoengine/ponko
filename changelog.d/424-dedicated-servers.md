### Added
- Dedicated servers: `Game::run_server(config, setup)` and
  `ConfiguredGame::run_server(config)` run the simulation headless, one tick
  every `1 / fixed_hz` seconds of wall time, until `SIGINT`/`SIGTERM`
  (Ctrl+C and Ctrl+Break on Windows), a `StopHandle`, `max_ticks` or
  `ctx.quit()`. A stop request sets `ServerStatus::stopping` and the server
  keeps ticking for the `shutdown_grace` so systems can tell clients and
  save; a second signal stops it at once. Slow ticks catch up to
  `max_catch_up` ticks, then the backlog is dropped and counted.
  `PONKO_SMOKE` gives a smoke run, so `ponko package` checks servers too.
- `ponko::server`: `ServerConfig`, `ServerStatus` (a resource the systems
  read and the returned world keeps), `TickReport` (ticks per second, tick
  times mean/p99/max, late and skipped ticks, entities, connected peers,
  resident and peak memory, logged every `report_every`), `StopReason`,
  `StopHandle`, `LogFormat` and `log(level, event, fields)`: structured
  logs as JSON lines on standard output (or text).
- `NetMessages::connected_peers`.
- `ponko new --server` (`ponko_cli`): a dedicated-server project with an
  authoritative UDP server on `run_server` (joins, moves, state broadcasts,
  goodbyes to every client on `SIGTERM`), a bot client
  (`cargo run --example client`), tests over UDP, and a two-stage
  `Dockerfile` that runs `ponko package` and copies only the package into a
  distroless non-root image (`PACKAGE_FROM=prebuilt` uses a package made
  elsewhere).
- The guide's "Dedicated servers" chapter: the template, `run_server`, the
  logs and the tick report, the container image, a size and memory baseline
  (a 5.4 MB binary, 3.7 MB peak memory at 30 Hz with four clients, on
  macOS), production notes and the control endpoint's safety on a public
  host.
- CI: a "server template" contract in `scripts/feature-deps.sh` (the
  template's engine features, with no renderer, audio, input devices,
  models, physics or dev tools), and a "Server container" job that lints
  the Dockerfile, builds the image from the package, joins it with the
  example client and stops it with `docker stop`.
