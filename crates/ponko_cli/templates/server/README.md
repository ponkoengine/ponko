# {{project-name}}

A dedicated game server made with [Ponko](https://ponko.dev/): no window, GPU
or sound, 30 ticks a second, JSON logs, clean shutdown on `SIGTERM`.

```sh
cargo run                         # the server, on UDP 0.0.0.0:7777
cargo run --example client        # a bot client (another terminal)
cargo test                        # server and client over UDP, headless
docker build -t {{project-name}} .            # the container image
docker run --rm -p 7777:7777/udp {{project-name}}
```

- `SERVER_ADDR` (default `0.0.0.0:7777`) and `MAX_CLIENTS` (default 32) set
  where it listens and how many may join.
- The guide's chapter on dedicated servers: <https://ponko.dev/guide/servers.html>
- The API: <https://docs.rs/ponko/>
- AI agents: `AGENTS.md`, and `.mcp.json` for the `ponko-mcp` server
  (`cargo install ponko_mcp`).
