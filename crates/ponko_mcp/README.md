# ponko_mcp

An [MCP](https://modelcontextprotocol.io/) server that lets AI agents launch,
inspect and drive running [Ponko](https://github.com/ponkoengine/ponko) games:
start a game headless and paused, read the world and reflected components,
step exact ticks, inject input, take screenshots, and read performance
reports and traces.

```sh
cargo install ponko_mcp
# Then add it to an MCP client, for example:
claude mcp add ponko -- ponko-mcp
```

`ponko-mcp --read-only` offers only the tools that don't change a game.
Games are found through the control endpoint, which is on in debug builds
when they run with `PONKO_CONTROL=1` (the `launch` tool sets it).

- Working with AI agents: <https://ponko.dev/guide/agents.html>
- The control endpoint and its security: <https://ponko.dev/guide/control.html>
- The agent acceptance suite (tasks done through these tools, scored):
  <https://ponko.dev/guide/agents.html#the-agent-acceptance-suite>

The tool names and parameters are a stable contract; see
[STABILITY.md](../../STABILITY.md).

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
