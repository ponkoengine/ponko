# The `ponko` command

`ponko` is the engine's command-line tool: it makes projects, opens them in
the editor, and turns a game into what players download. Install it once:

```sh
cargo install ponko_cli
```

`ponko` alone, `ponko help` or `ponko --help` lists the commands;
`ponko help <command>` or `ponko <command> --help` lists a command's
options. `ponko --version` (or `-V`) prints its version. Every command and
option below is one that `--help` shows; a test of the `ponko_cli` crate
reads this page and fails if one is missing here, or if the command doesn't
accept one it lists.

| Command | Does |
|---|---|
| [`ponko new`](#ponko-new) | Makes a game project from a template |
| [`ponko editor`](#ponko-editor) | Opens a game in the editor |
| [`ponko package`](#ponko-package) | Builds a game and packages it for players |
| [`ponko notices`](#ponko-notices) | Writes `THIRD-PARTY-NOTICES.txt` for a library |
| [`ponko pack`](#ponko-pack) | Packs a folder of assets into one asset pack |
| [`ponko cook`](#ponko-cook) | Turns source assets into runtime-ready ones |
| [`ponko textures`](#ponko-textures) | Compresses images into KTX2 GPU textures |
| [`ponko image-diff`](#ponko-image-diff) | Compares two PNG images |
| [`ponko schema`](#ponko-schema) | Writes a game's glTF custom properties schema |
| `ponko help` | Lists the commands, or one command's options |

A command that fails prints `ponko: ` and what went wrong on the error
output and exits with 1.

## `ponko new`

```sh
ponko new <path> [--2d | --3d | --headless | --server | --plugin] [--path <engine> | --git <url>]
```

Makes a project in the folder `<path>` (it must not exist, or be empty),
named after the folder. [Getting started](getting-started.md#a-new-project)
walks through what it holds.

| Option | Does |
|---|---|
| `--2d` | A 2D game: a window, input, drawing (the default) |
| `--3d` | A 3D game: a mesh, a camera and a sun |
| `--headless` | A simulation without a window, GPU or sound |
| `--server` | A [dedicated server](servers.md): headless, a bot client, a Dockerfile |
| `--plugin` | A [plugin crate](plugins.md#making-a-plugin-crate) for other people's games |
| `--path <engine>` | Depend on an engine checkout on this machine |
| `--git <url>` | Depend on an engine Git repository (its default branch) |

Without `--path` or `--git` the project depends on the released engine from
crates.io.

## `ponko editor`

```sh
ponko editor [<project> | --last] [--open <file>] [--bin <name> | --example <name>]
             [-p <package>] [--features <list>] [--release] [--print] [-- <game arguments>]
ponko editor --recent
```

Builds the game with the engine's `editor` feature and runs it in edit mode;
[The editor](editor.md) has the rest.

| Option | Does |
|---|---|
| `<project>` | The game's folder (default: the current one) |
| `--open <file>` | A scene or prefab in `assets/` to open (default: the world the game's setup makes) |
| `--bin <name>` | Run this binary of the package |
| `--example <name>` | Run this example instead |
| `-p`, `--package <name>` | The package to run, in a workspace |
| `--features <list>` | More Cargo features to build with |
| `--release` | An optimized build |
| `--print` | Print the command instead of running it |
| `--last` | The project opened last, at the file opened last |
| `--recent` | List the recent projects and their files |
| `-- <game arguments>` | Everything after `--` goes to the game |

## `ponko package`

```sh
ponko package [--bin <name> | --example <name>] [options]
```

Run in the game's folder: builds the game in release, collects its assets
(cooked, in one `assets.pack`), licenses and notices, lays them out for the
platform, archives them and starts the result once headless to check it
runs. [Packaging any game](packaging.md#packaging-any-game-ponko-package)
explains each step and the `[package.metadata.ponko]` table.

| Option | Does |
|---|---|
| `--bin <name>`, `--example <name>` | The program (default: the package's binary) |
| `-p`, `--package <name>` | The package, in a workspace |
| `--manifest-path <path>` | The game's `Cargo.toml` (default: found from here) |
| `--target <triple>` | Build for another platform; `ios` and `ios-sim` are short for the [iOS](ios.md) targets |
| `--web` | Build for browsers ([Web builds](web.md)) |
| `--threads` | With `--web`: worker threads ([Threads on the web](web-threads.md#building-with-threads)) |
| `--isolation-worker` | With `--threads`: add the service worker for [hosts that can't send the headers](web-threads.md#hosts-that-cant-send-the-headers) |
| `--features <list>`, `--no-default-features` | As for `cargo build` |
| `--profile <name>` | The Cargo profile (default: `release`) |
| `--locked` | As for `cargo build` |
| `--no-build` | Package the program as already built |
| `--no-pack` | Ship the assets as loose files instead of one `assets.pack` |
| `--no-cook` | Ship the source assets as they are |
| `--max-entry-size <bytes>`, `--max-pack-size <bytes>`, `--max-entries <n>` | The [caps on what the pack may unpack to](packaging.md#caps-on-what-a-pack-may-unpack-to) |
| `--format <list>` | `folder`, `zip`, `tar.gz`, `dmg`, `ipa`, comma-separated |
| `--out <dir>` | Where to put the package (default: `target/package`) |
| `--no-smoke` | Don't start the packaged game |
| `--smoke-ticks <n>` | Ticks of the [smoke run](packaging.md#the-smoke-run-of-any-package) (default: 120) |
| `--smoke-timeout <seconds>` | How long the smoke run may take (default: 120) |
| `--sign <program>` | The [signing hook](packaging.md#signing-and-notarizing-the-hook) |

## `ponko notices`

```sh
ponko notices --out <file> [-p <package>] [--title <text>] [--manifest-path <path>]
              [--target <triple>] [--features <list>] [--no-default-features] [--locked]
```

Writes the third-party notices of a library the way `ponko package` writes
them for a game ([The notices](packaging.md#the-notices)): for shipping a
library, such as a C library built from Rust. It takes only these options;
`ponko package`'s others are refused.

| Option | Does |
|---|---|
| `--out <file>` | Where to write the notices (required) |
| `-p`, `--package <name>` | The crate, in a workspace |
| `--title <text>` | The heading (default: the crate's name) |
| `--manifest-path <path>` | The crate's `Cargo.toml` |
| `--target <triple>` | The platform it links for (default: this one) |
| `--features <list>`, `--no-default-features` | As for `cargo build` |
| `--locked` | As for `cargo build` |

## `ponko pack`

```sh
ponko pack [<folder>] [--out <file>] [--exclude <pattern>]... [--textures]
           [--cook [--platform <name>]] [--no-compress]
           [--max-entry-size <bytes>] [--max-pack-size <bytes>] [--max-entries <n>]
ponko pack --list <file>
```

Packs every file in `<folder>` (default: `assets`) into one asset pack,
named by its path in the folder; see [Assets in one pack](packaging.md#assets-in-one-pack-ponko-pack).

| Option | Does |
|---|---|
| `--out <file>` | Where to write the pack (default: `target/assets.pack`) |
| `--exclude <pattern>` | Leave out the files matching it (`*` within a folder name, `**` across folders); repeat for more |
| `--textures` | Also convert the images per `textures.json` into `.ktx2` files in the pack |
| `--cook` | Pack the files as `ponko cook` makes them |
| `--platform <name>` | With `--cook`: `desktop` (default), `web` or `mobile` |
| `--no-compress` | Store every file as it is |
| `--max-entry-size <bytes>`, `--max-pack-size <bytes>`, `--max-entries <n>` | The [caps on what the pack may unpack to](packaging.md#caps-on-what-a-pack-may-unpack-to) |
| `--list <file>` | Print what a pack holds instead |

## `ponko cook`

```sh
ponko cook [<folder>] [--out <folder> | --pack <file>] [--platform <name>]
           [--cache <folder> | --no-cache]
           [--max-entry-size <bytes>] [--max-pack-size <bytes>] [--max-entries <n>]
```

Cooks every file in `<folder>` (default: `assets`) under the same names:
compressed textures, optimized meshes, FLAC sounds, checked shaders,
Aseprite sheets; see [Cooking assets](packaging.md#cooking-assets-ponko-cook)
for the kinds and `cook.json`.

| Option | Does |
|---|---|
| `--out <folder>` | Where the cooked files go (default: `target/cooked/<platform>`) |
| `--pack <file>` | Write one asset pack of them instead |
| `--platform <name>` | `desktop` (default), `web` or `mobile` |
| `--cache <folder>` | Where cooked results are kept (default: `target/cook-cache`) |
| `--no-cache` | Cook everything again and keep nothing |
| `--max-entry-size <bytes>`, `--max-pack-size <bytes>`, `--max-entries <n>` | With `--pack`: the [caps on what the pack may unpack to](packaging.md#caps-on-what-a-pack-may-unpack-to) |

## `ponko textures`

```sh
ponko textures [<folder>] [--out <folder>] [--force] [--check]
```

Converts every PNG and JPEG in `<folder>` (default: `assets`) into a `.ktx2`
of the same name; [Compressed textures](drawing.md#compressed-textures)
covers `textures.json` and the kinds. A `ponko` installed without its
default features has no `textures` command and says so.

| Option | Does |
|---|---|
| `--out <folder>` | Write the `.ktx2` files here instead, in the same subfolders |
| `--force` | Convert every image, even those whose `.ktx2` is up to date |
| `--check` | Convert nothing; fail if a `.ktx2` is missing or older than its image or the settings |

## `ponko image-diff`

```sh
ponko image-diff <a.png> <b.png> [--tolerance <0..255>] [--threshold <percent>] [--heatmap <out.png>]
```

Compares two PNG images on the CPU, without a game or a GPU, and prints the
result as JSON. It exits with 1 when more pixels differ than the threshold
allows, or when an image can't be read; see
[Capture, compare, fix](agents.md#capture-compare-fix).

| Option | Does |
|---|---|
| `--tolerance <0..255>` | How far a channel may differ and still count as equal (default 0) |
| `--threshold <percent>` | The share of pixels that may differ (default 0) |
| `--heatmap <out.png>` | Write where they differ as an image; never overwrites a file |

## `ponko schema`

```sh
ponko schema [--out <file>] [--bin <name> | --example <name>] [-p <package>]
             [--features <list>] [--manifest-path <Cargo.toml>]
```

Builds and runs the game once without a window and writes the JSON schema of
a glTF node's custom properties, for editors to check them before
exporting; see [A schema for the editor](blender.md#a-schema-for-the-editor).

| Option | Does |
|---|---|
| `--out <file>` | Where to write it (default: `ponko-schema.json`) |
| `--bin <name>` | Run this binary of the package |
| `--example <name>` | Run this example instead |
| `-p`, `--package <name>` | The package to run, in a workspace |
| `--features <list>` | Cargo features to build with |
| `--manifest-path <path>` | The package's `Cargo.toml` |
