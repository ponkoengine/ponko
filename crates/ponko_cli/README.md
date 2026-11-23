# ponko_cli

The `ponko` command for [Ponko](https://github.com/ponkoengine/ponko) games.

```sh
cargo install ponko_cli
ponko new my_game            # a 2D game (the default)
ponko new my_game --3d       # a 3D game
ponko new my_sim --headless  # a simulation without a window, GPU or sound
ponko new my_server --server # a dedicated server, with a bot client and a Dockerfile
```

Each project depends on the released `ponko`, has a fast dev profile, an
`assets/` folder, headless tests (`cargo test`), an `.mcp.json` for the
`ponko-mcp` server and an `AGENTS.md` for coding agents. `--path <engine>` or
`--git <url>` depends on an engine checkout or repository instead.

In a game's folder, `ponko package` makes what players download: a release
build with its assets, licenses and third-party notices, as a Windows zip, a
macOS app in a dmg, a Linux tar.gz or (`--web`) a folder for any static host,
then starts the packaged copy once headless to check it runs. The game's
`[package.metadata.ponko]` table says what goes in (name, id, icon, assets,
files); see the guide's [packaging chapter](https://ponko.dev/guide/packaging.html).

```sh
ponko package          # for this computer
ponko package --web    # for browsers
ponko cook             # assets/ cooked: compressed textures, optimized meshes, FLAC, checked shaders
```

The package's `assets.pack` holds the assets cooked (as `ponko cook` makes
them, under the same names, cached by content); games run their source
assets while you work.

The templates are also usable with
[cargo-generate](https://github.com/cargo-generate/cargo-generate):

```sh
cargo generate --git https://github.com/ponkoengine/ponko crates/ponko_cli/templates/2d
```

- Getting started: <https://ponko.dev/guide/getting-started.html>
- Packaging: <https://ponko.dev/guide/packaging.html>

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
