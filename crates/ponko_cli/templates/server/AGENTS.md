# Agent instructions

{{project-name}} is a dedicated game server (no window, GPU or sound) made
with the Ponko engine (<https://ponko.dev/guide/>). The guide's chapter on
servers: <https://ponko.dev/guide/servers.html>.

## Commands

- `cargo test`: the tests in `tests/server.rs` (a server and a client over
  UDP on this machine). Run them after every change.
- `cargo run` starts the server on UDP `0.0.0.0:7777` (`SERVER_ADDR`);
  `cargo run --example client` joins it with a bot. Stop the server with
  Ctrl+C.
- `docker build -t {{project-name}} .` makes the container image (see
  `Dockerfile`); `ponko package --format folder` makes the package it holds.
- Never open a window: there is none. Keep `default-features = false` on the
  engine dependency; the window, audio and gamepad features need system
  libraries the container image doesn't have.

## Layout

- `src/lib.rs`: the protocol (`ClientMsg`, `ServerMsg`), the server's
  systems and the bot client. The server owns the world: clients only send
  what they want to do.
- `src/main.rs`: starts the server with `run_server` (signals, tick
  reports, JSON logs, shutdown grace).
- `examples/client.rs`: the bot client.
- `assets/settings.json`: numbers to tune.

## Operating it

- Logs are JSON lines on standard output (`ponko::server::log` adds your
  own events); `tick_report` lines show the tick rate, tick times, late and
  skipped ticks, peers and memory.
- On `SIGTERM` the `shut_down` system sends `Goodbye` to every client and
  quits once they left, within the shutdown grace.
- Never expose the control endpoint: release builds don't open it unless
  the code asks (`Game::control_endpoint(true)`) or the engine's
  `control-env` feature is on, and it only listens on 127.0.0.1.

## Driving the running server (MCP)

`.mcp.json` registers `ponko-mcp`, which launches a debug build with the
control endpoint (`PONKO_CONTROL=1`), so an agent can step ticks and read
the reflected `Player`, `Settings` and `Lobby`:
<https://ponko.dev/guide/agents.html>.
