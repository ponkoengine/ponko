# Dedicated servers

A dedicated server is the game's simulation on a machine with no player at
it: no window, GPU or sound, a fixed tick rate, running until an operator or
an orchestrator (Docker, Kubernetes, systemd) stops it. Ponko has three
pieces for it:

- **`Game::run_server`**: the [headless run](headless.md) paced to wall time,
  with clean shutdown on `SIGTERM`, a tick-rate report and structured logs.
- **`ponko new --server`**: a project with an authoritative UDP server, a
  bot client and its tests.
- **A container recipe**: the project's `Dockerfile`, built with
  [`ponko package`](packaging.md) into a small non-root image.

## A server project

```sh
ponko new my_server --server
cd my_server
cargo test                    # a server and a client over UDP, headless
cargo run                     # the server, on UDP 0.0.0.0:7777
cargo run --example client    # a bot that joins it (another terminal)
```

| File | What it is |
|---|---|
| `src/lib.rs` | The protocol (`ClientMsg`, `ServerMsg`, JSON over the [message endpoint](networking.md)), the server's systems (joins, moves, a state broadcast at `state_hz`, goodbyes on shutdown) and the bot client |
| `src/main.rs` | Opens the UDP endpoint (`SERVER_ADDR`, `MAX_CLIENTS`) and calls `run_server` |
| `examples/client.rs` | The bot client: joins, walks, leaves; exits with an error if it never got in |
| `tests/server.rs` | A client joins and gets the world; stopping the server says goodbye to its clients |
| `Dockerfile`, `.dockerignore` | The container image ([below](#the-container-image)) |
| `assets/settings.json` | Numbers to tune |

The server owns the world: clients only say what they want to do, and every
client sees the server's result. The engine's messages are plain UDP with
reliable and unreliable lanes. The template writes its own small protocol
on them; a game that grows past a few values replicates instead
([below](#a-replicated-game)).

### What it is built with

The template's engine dependency is
`ponko = { version = "...", default-features = false }`: no window, GPU,
audio, gamepad or inspector, and none of their system libraries, so the
binary runs in a minimal image. The "server template"
contract of `scripts/feature-deps.sh` reads the engine features from the
template's manifest and fails if its dependency graph contains the
renderer, audio, input devices, glTF, physics, navigation, dev tools,
Steam, accessibility, text shaping or extra image decoders. Turn a feature
on in your own server when it needs it (`physics` for authoritative
physics, say); the contract only guards the template.

## `run_server`

```rust
use ponko::server::{ServerConfig, ServerStatus};
use std::time::Duration;

fn shut_down(ctx: &mut Ctx) {
    if ctx.world.resource::<ServerStatus>().stopping.is_some() {
        // Tell the clients, save the world...
        ctx.quit();
    }
}

# fn run() {
let config = ServerConfig::new()
    .shutdown_grace(Duration::from_secs(3)) // time to say goodbye
    .report_every(Duration::from_secs(10)); // the tick-rate report
let world = Game::new("my server", 1, 1)
    .fixed_hz(30)
    .sim_system(shut_down)
    .run_server(config, |_| {});
# let _ = world;
# }
```

It runs as [`run_headless`](headless.md#what-runs) does (setup, then sim
systems every tick; no frames unless you ask for
[headless frames](headless.md#menus-and-the-ui-headless-frames)), with
these differences:

- **Pacing.** One tick every `1 / fixed_hz` seconds of wall time. A tick
  that runs long makes the next ones start at once to catch up, up to
  `max_catch_up` ticks (5 by default); past that the backlog is dropped and
  counted as skipped ticks, so clients see a hitch instead of a burst of
  fast-forwarded ticks. Message endpoints are polled before every tick.
- **Stopping.** `SIGINT` (Ctrl+C) and `SIGTERM` (`docker stop`, Kubernetes,
  systemd), a `StopHandle` from `ServerConfig::stop_handle` (an admin
  command, a test), or `max_ticks` set `ServerStatus::stopping`. Ticks keep
  running for the `shutdown_grace` so systems can tell clients and save;
  `ctx.quit()` ends the run sooner. With no grace (the default) the server
  stops after the current tick. A second signal stops it at once. The
  handlers are installed for the run and the previous ones restored after
  it; `signals(false)` leaves signals to the program.
- **The result.** `run_server` returns the world; its `ServerStatus` says
  why the server stopped (`interrupt`, `terminate`, `requested`,
  `max_ticks` or `quit`), how many ticks ran and the last report.
- **Smoke runs.** With `PONKO_SMOKE=<ticks>` it runs that many ticks as
  fast as it can and prints `ponko: smoke run passed`, which is how
  `ponko package` checks a packaged server starts.

On Windows, Ctrl+C and Ctrl+Break stop the server; a service manager that
closes the console sends events the C runtime doesn't turn into signals,
so give such a server a `StopHandle` driven by your service code. Native
(C and C++) games don't have `run_server` yet; they can run
`ponko::game::run_headless` in a loop of their own.

### Logs

The server writes one JSON object per line to standard output, which
`docker logs`, Kubernetes and log collectors read as they are:

```json
{"ts":"2026-11-14T19:05:00.476Z","level":"info","event":"server_start","control":null,"engine":"0.1.0","hz":30,"listen":"0.0.0.0:7777","name":"my_server","pid":1,"shutdown_grace_s":3.0,"signals":true}
{"ts":"2026-11-14T19:05:10.481Z","level":"info","event":"tick_report","entities":4,"hz":30.09,"late_ticks":0,"peak_rss_bytes":3702784,"peers":4,"rss_bytes":null,"skipped_ticks":0,"target_hz":30,"tick_ms_max":2.13,"tick_ms_mean":0.024,"tick_ms_p99":0.091,"ticks":301,"window_s":10.004}
{"ts":"2026-11-14T19:05:21.843Z","level":"info","event":"server_stopping","grace_s":3.0,"reason":"terminate","tick":642}
{"ts":"2026-11-14T19:05:21.912Z","level":"info","event":"server_stopped","reason":"terminate","ticks":644,"uptime_s":21.4}
```

`ts`, `level` and `event` come first; the rest are the event's fields.
`ponko::server::log` writes the game's own events the same way (the
template logs `player_joined`, `client_left`, `bad_message`, ...):

```rust
use ponko::server::{LogLevel, log};

log(LogLevel::Info, "player_joined", json!({ "peer": 3, "name": "ann" }));
```

`ServerConfig::log_format(LogFormat::Text)` writes
`2026-11-14T19:05:10.481Z info tick_report hz=30.09 ...` for a terminal
instead, and `LogFormat::Off` nothing. The engine's own warnings (an asset
that failed to load, say) still go to standard error as plain text.

### The tick-rate report

Every `report_every` (10 seconds by default, `Duration::ZERO` turns it
off), and once more when the server stops, the `tick_report` event and
`ServerStatus::report` hold:

| Field | Meaning | Watch for |
|---|---|---|
| `ticks`, `window_s`, `hz` | Ticks run in the window and the rate reached | `hz` below `target_hz`: ticks take longer than their budget (`1000 / target_hz` ms). The line is logged as `warn` below 95 % |
| `tick_ms_mean`, `tick_ms_p99`, `tick_ms_max` | One tick's work (systems and networking), milliseconds | `p99` approaching the budget: the next busy moment makes ticks late |
| `late_ticks` | Ticks that ended after the next was due | A steady count: the machine is too slow or shared |
| `skipped_ticks` | Ticks dropped after falling more than `max_catch_up` behind | Anything above 0 (also logged as `warn`): clients saw a hitch |
| `entities`, `peers` | Entities in the world, peers connected to the message endpoint | Peers that never leave (clients that don't disconnect time out after 10 s) |
| `rss_bytes`, `peak_rss_bytes` | Resident memory now (Linux) and at most so far (Unix) | Growth over hours: a leak, or state per player that is never freed |

For more detail on a running server, profile the same game with
[`profile_headless`](profiling.md) or a [trace](profiling.md) on a
development machine.

## A replicated game

With [replication](replication.md) there is no protocol to write: the
server is the same game as the client, hosting instead of joining, and
`run_server` is all a dedicated server adds. This is the one in the
[Orb Arena](replication.md#orb-arena-the-reference-game) example:

```rust,no_run
{{#include ../../examples/net_arena/main.rs:dedicated}}
```

`cargo run --example net_arena -- server` runs it; `-- join <address>`
plays against it. The tick-rate report's `peers` counts the joined
clients, and `ctx.net.replication.peer_stats(peer)` has the bytes and
packets each one was sent, for your own log lines.

## The container image

The template's `Dockerfile` has two stages:

1. **Build**, in the official Rust image: installs the `ponko` command
   (without its texture tools), then `ponko package --format folder`, which
   makes the release build, packs `assets/` into `assets.pack`, collects
   the licenses and `THIRD-PARTY-NOTICES.txt`, and runs the packaged server
   once headless (the smoke run). Cargo's registry and `target/` are cache
   mounts, so rebuilds are incremental.
2. **Runtime**, `gcr.io/distroless/cc-debian12:nonroot`: glibc and libgcc,
   no shell or package manager, the `nonroot` user. Only the package goes
   in, at `/app`. `SERVER_ADDR=0.0.0.0:7777`, `EXPOSE 7777/udp`,
   `STOPSIGNAL SIGTERM`.

```sh
docker build -t my_server .
docker run --rm --name my_server -p 7777:7777/udp my_server
cargo run --example client           # from the host
docker stop my_server                # SIGTERM: goodbyes, then exit 0
```

The server is the container's process 1. Linux delivers no signal to
process 1 that it has no handler for, which is why some images need an init
process (`docker run --init`); `run_server` installs its handlers, so the
server needs none and `docker stop` doesn't have to wait for its 10-second
kill. Keep `shutdown_grace` below the orchestrator's timeout (`docker stop
-t`, Kubernetes' `terminationGracePeriodSeconds`, 30 s by default).

To package elsewhere, for example on a Linux CI machine that already built
the server, put the package folder at `package/` and skip the build stage:

```sh
ponko package --format folder --out target/server-package
mv target/server-package/*/ package
docker build --build-arg PACKAGE_FROM=prebuilt -t my_server .
```

Commit `Cargo.lock` and add `--locked` to the `ponko package` line for
reproducible images. `RUST_VERSION` and `PONKO_CLI` are build arguments
too (the Rust image's version, and what `cargo install` installs).

### Size and memory

Measured on the template, unchanged, on an Apple silicon laptop (macOS,
release build from `ponko package`) that was compiling other projects at the
same time, over 21 seconds with four bot clients:

| | |
|---|---|
| Server binary | 5.4 MB (debug info stripped) |
| Rest of the package | `THIRD-PARTY-NOTICES.txt` 0.2 MB, `assets.pack` 131 bytes |
| Tick rate | 30.0 Hz of 30, no late or skipped ticks |
| Tick time | mean 0.02 ms, p99 0.09 ms, max 2.1 ms (the joins) |
| Peak resident memory | 3.7 MB |

The image adds the distroless base to the package. The "Server container"
job of the list of checks (`scripts/checks.yml`) builds the image on Linux from the package, starts it, joins it with
the example client and stops it with `docker stop`; it prints the image's
size and the container's memory with every run. Your game's numbers grow
with its world: measure them with the tick report under a realistic load.

### Running it in production

- **UDP.** Publish the port as UDP (`-p 7777:7777/udp`, a Kubernetes
  `Service` with `protocol: UDP`). Health checks over TCP or HTTP don't
  reach a UDP server; check the process, or run the bot client as the probe
  (`examples/client.rs` exits non-zero when it can't play).
- **One match per process.** A server process is one world. Run several
  containers for several matches and give each its own port. Players find
  them through a lobby: [`ponko-lobby`](lobby.md) runs in the same recipe.
- **Limits.** Give the container a CPU and memory limit
  (`docker run --cpus 1 --memory 128m`). A tick runs on one thread; a
  server that falls behind shows it in `hz` and `skipped_ticks` before
  players complain.
- **Restarts.** A crash (a panic) exits non-zero after printing the panic to
  standard error; let the orchestrator restart it (`--restart
  on-failure`). A clean stop exits 0.
- **Logs.** Keep them as JSON lines; filter on `event` and alert on
  `level` `warn` or `error`.

## The control endpoint on a public host

The [control endpoint](control.md) gives whoever holds its token full
control of a running game: read and change any reflected component or
resource, step, pause and quit it, inject input, save and load worlds. On a
server reachable from the internet, keep it closed:

- **It is off in release builds.** A release build opens it only when the
  code calls `Game::control_endpoint(true)`, or when the engine's
  `control-env` feature is on and `PONKO_CONTROL=1` is set. Ship servers
  with neither; check with `cargo tree -e features -i ponko`. The
  `server_start` log line says `"control":null` when it is closed.
- **It listens on `127.0.0.1` only**, never on a public address. Never
  forward it: no `-p` for its port, no proxy, no SSH tunnel left open, no
  `socat` to `0.0.0.0`.
- **Host networking widens "local".** With `docker run --network host`
  (or a Kubernetes pod with `hostNetwork`), the container's loopback is the
  machine's: every process on the machine can reach the port. The token
  still has to be read from the registry file (mode `0600`, under the
  server user's `~/.ponko/instances`), but a shared machine is no place for
  an open endpoint.
- **To debug a live server**, run a debug build (or one with
  `control-env`) in a separate, private deployment, and reach it with
  `docker exec` into its container, where the registry file and the
  loopback address are.
