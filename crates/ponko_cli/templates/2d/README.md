# {{project-name}}

A 2D game made with [Ponko](https://ponko.dev/).

```sh
cargo run              # in a window
HEADLESS=1 cargo run   # without one
cargo test             # the headless tests
ponko package          # the download for players (zip, app, tar.gz)
```

On Windows with Direct3D 12, `ponko package` bundles `dxcompiler.dll` (DXC).
Set `windows-dxc = false` in `[package.metadata.ponko]` to leave it out.
Development builds use FXC by default if Windows cannot find DXC. To use
DXC, place the DLL beside the executable, or set `WGPU_DX12_COMPILER=dxc`
with the DLL available on `PATH`. Set `WGPU_DX12_COMPILER=fxc` to force FXC.
See the [Windows shader compilation notes](https://ponko.dev/guide/packaging.html#windows-shader-compilation)
for the measured renderer startup times.

- The guide: <https://ponko.dev/guide/>
- The API: <https://docs.rs/ponko/>
- AI agents: `AGENTS.md`, and `.mcp.json` for the `ponko-mcp` server
  (`cargo install ponko_mcp`).
