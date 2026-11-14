# A complete game, packaged

[`examples/coin_dash`](https://github.com/ponkoengine/ponko/tree/main/examples/coin_dash)
is the engine's reference for a small *finished* game: title screen, settings,
rounds with a win and a loss, pause, restart, result screens, sound, saved
progress, keyboard and gamepad, and a download you can unzip anywhere. It uses
only the public `Game`/`Ctx` API and content made for it: one 32 x 32 picture
(`assets/coin_dash/coin.png`), a disc and the sound effects generated in code,
and the engine's built-in fonts. Rules and tests are described in
[Headless runs and testing](headless.md#a-complete-game-tested-headlessly);
this chapter is about getting it to players and what it shows, starting with
the command that packages any game.

## Packaging any game: `ponko package`

`ponko package` turns a game into what players download, with one command in
the game's folder (`cargo install ponko_cli` gives the `ponko` command; in the
engine's repository, `cargo run -p ponko_cli -- package` runs it):

```sh
cd my_game
ponko package                 # for this computer: zip, app in a dmg, or tar.gz
ponko package --web           # a folder to put on any static web host
```

It builds the game in release, collects its assets and license files, writes
`THIRD-PARTY-NOTICES.txt` for every library linked into it, lays the package
out for the platform, archives it, and starts the packaged copy once to check
it runs (the [smoke run](#the-smoke-run-of-any-package)). Everything goes to
`target/package/` (`--out` elsewhere), named
`<name>-<version>-<os>-<arch>`, for example `coin-dash-0.1.0-macos-aarch64`:

| Platform | Layout | Archive (default) |
|---|---|---|
| Windows | a folder: `my_game.exe`, `assets/`, the licenses and notices | `.zip` |
| macOS | `My Game.app` (the program in `Contents/MacOS`, assets, licenses and notices in `Contents/Resources`, the icon as `icon.icns`, an `Info.plist`), with the licenses and notices also beside the app | `.dmg` with a link to Applications (made with `hdiutil`, so on a Mac; elsewhere a `.zip`) |
| Linux | a folder, like Windows' | `.tar.gz` |
| Web (`--web`) | a folder: `index.html`, `game.js`, `game_bg.wasm`, `assets/` with its `index.json`, the licenses and notices | none: upload the folder |

`--format` picks others (comma-separated `zip`, `tar.gz`, `dmg`, or `folder`:
the plain folder without archive, and on macOS without app bundle). The zip and
tar.gz files are written by the command itself, so no `zip` or `tar` program is
needed on any OS, and they are reproducible: entries sorted, owned by root,
with fixed dates, so packing the same files twice gives the same bytes. The
engine finds the assets next to the program, and inside an app bundle in
`Contents/Resources` ([below](#the-package)), so an unpacked game works from any
folder.

### What goes in: `[package.metadata.ponko]`

The game's `Cargo.toml` says what to ship. Projects from `ponko new` have the
table already; every key is optional:

```toml
[package.metadata.ponko]
name = "My Game"              # the name players see: the app, the web page's title, the file names (my-game-...)
id = "com.example.my-game"    # a reverse-DNS id you own: the macOS bundle identifier
icon = "assets/icon.png"      # a square PNG of 16 to 1024 pixels (a power of two; 512 or 1024 looks best)
assets = ["assets"]           # folders or files, shipped at the same path next to the program
files = [                     # put at the package's top: licenses, a readme for players, notices
    "LICENSE-MIT",
    { from = "docs/PLAYER-README.txt", to = "README.txt" },
]
```

| Key | Default |
|---|---|
| `name` | the package's name (an example's name for `--example`) |
| `id` | `local.<name>`, with a note to set one |
| `icon` | none: macOS shows the generic app icon |
| `assets` | `["assets"]` if that folder exists, else nothing; hidden files (`.DS_Store`) are left out |
| `files` | the `LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` files beside `Cargo.toml` |

Paths are relative to `Cargo.toml`. An unknown key, an id that isn't reverse
DNS, a missing file and two files at one place in the package are errors that
name the key. For an example of a package (`--example <name>`),
`[package.metadata.ponko.examples.<name>]` takes the same keys and overrides
the table above it; this repository's root `Cargo.toml` describes Coin Dash
that way.

The other options follow Cargo: `--bin <name>` or `--example <name>` (by
default the package's only binary, or its `default-run`), `-p <package>` in a
workspace, `--manifest-path`, `--features`, `--no-default-features`,
`--profile <name>` (default `release`), `--locked`, and `--no-build` to
package what is already built (honouring `CARGO_BUILD_TARGET`). `ponko help
package` lists them all.

### The notices

`THIRD-PARTY-NOTICES.txt` lists every library the program links on the
platform it is built for, from `cargo tree` with the same features (so not
every optional dependency), each with the license it declares and its
authors, then the license texts each library's source ships (two folders deep,
code folders left out, so font licenses such as `OFL.txt` are found), each text
once with the libraries it belongs to. A library that declares no license fails
the packaging by name instead of being listed as "unknown". The game's own
package is not a third party and is left out; so are crates of the game's own
workspace that declare no license. An example can link its package's
dev-dependencies, so for `--example` they are listed too. `ponko notices -p
<crate> --out <file>` writes the same file for a library alone; the release
workflow ships the C library's with it.

### The smoke run of any package

After packaging for the computer it runs on, the command copies the package to
a new folder whose path has a space and a non-ASCII letter, and starts it from
another, empty folder, without the `CARGO_*`, `PONKO_*` and `RUST_*` variables,
with `PONKO_SMOKE=<ticks>` and `PONKO_DATA_DIR` pointing at a scratch folder.
With `PONKO_SMOKE` set, `Game::run` opens no window: it runs the setup and that
many ticks headless, frame systems included, prints `ponko: smoke run passed`
and returns. The package passes if the program exits normally within the time
limit, so it started, found the assets it loads next to itself (wherever it
was unpacked) and ran its systems, also on a machine without a display or GPU.
`--smoke-ticks` (default 120) and `--smoke-timeout` (seconds, default 120)
change the run, `--no-smoke` skips it. On macOS the program inside the app
bundle is run, so the `Contents/Resources` lookup is what is tested. A program
that doesn't start through `Game::run` (a simulation calling `run_headless`
itself) just has to exit normally; the report says the smoke setting did not
apply. Coin Dash adds its own, deeper walk-through on top ([below](#the-smoke-test)).

### Signing and notarizing: the hook

Signing needs the publisher's certificates and accounts, so the command leaves
it to a program of your own, `--sign <program>`, called twice:

- `<program> sign <path>` after the package is laid out and before it is
  smoke-run and archived: `<path>` is the `.app` on macOS and the program
  elsewhere (sign it with `codesign --options runtime`, or `signtool`);
- `<program> archive <path>` for each archive made (notarize and staple a
  dmg, sign a zip's checksum, upload).

Both get `PONKO_PACKAGE_STAGE`, `PONKO_PACKAGE_PATH`, `PONKO_PACKAGE_DIR` (the
package folder), `PONKO_PACKAGE_PLATFORM` (`macos-aarch64`, ...),
`PONKO_PACKAGE_TARGET`, `PONKO_PACKAGE_NAME`, `PONKO_PACKAGE_ID` and
`PONKO_PACKAGE_VERSION`; a non-zero exit stops the packaging. Without `--sign`,
a macOS app made on a Mac is sealed ad hoc (`codesign --sign -`), so the bundle
is consistent and not reported as damaged, but it carries no developer
identity: Gatekeeper asks the player to allow it. Windows and Linux packages
are left unsigned. Release signing with the project's certificates is
[#282](https://github.com/ponkoengine/ponko/issues/282).

### The web folder

`ponko package --web` builds for `wasm32-unknown-unknown` and runs
`wasm-bindgen` the way `web/build.sh` does (`rustup target add
wasm32-unknown-unknown` and `cargo install wasm-bindgen-cli --version <the
version in the game's Cargo.lock>` first; a different version is refused with
the command to install the right one). The page is the engine's
`web/index.html`, titled with the game's name and with the icon as favicon;
`assets/index.json` lists the files under `assets/` the engine fetches before
setup. Serve the folder from any static host (`python3 -m http.server -d
<folder> 8000` locally). There is no smoke run for it: the browser runner in
`web/smoke` is the engine's own test, not part of the command.

### Other platforms and limits

`--target <triple>` packages for another platform, if the toolchain can build
for it (`rustup target add <triple>` and a linker for it, such as MinGW for
`x86_64-pc-windows-gnu`): the layout and archive are the target's, the smoke
run is skipped, a macOS app made elsewhere is not sealed, and a dmg needs a
Mac. What the command doesn't do yet:

- **Verified on macOS (Apple silicon) only.** There `ponko package` was run on
  a fresh `ponko new` project and on Coin Dash for every format, the archives
  unpacked with the system's tools, the extracted app's seal verified and the
  program started. The Windows and Linux layouts and archives are checked by
  unit tests on every OS (`cargo test -p ponko_cli`) and CI packages Coin Dash
  with the command on Linux, macOS and Windows, but no Windows or Linux
  package made by it has been opened by a person yet.
- **No installers**: no `.msi`, `.pkg`, Flatpak or AppImage (an AppImage
  needs `appimagetool`; the Linux package is a portable tar.gz), and no
  `.desktop` file for Linux menus.
- **Icons**: the icon goes into the macOS bundle and the web page, and beside
  the program as `icon.png` on Windows and Linux; it is not embedded in the
  Windows `.exe` (that takes a resource compiled into the build).
- **One architecture per package**: no universal macOS binary.
- **A console window on Windows**: a Rust program is a console program
  unless its `main.rs` says `#![windows_subsystem = "windows"]` (best only in
  release builds, `#![cfg_attr(not(debug_assertions), windows_subsystem =
  "windows")]`); the command packages the program as it is built.
- **The smoke run** proves the program starts, finds its assets and runs its
  systems headless; it draws nothing. Coin Dash's window walk-through
  ([below](#the-smoke-test)) is what opens a window.
- **Signing and notarization** are the hook above; the project's own release
  signing is [#282](https://github.com/ponkoengine/ponko/issues/282).
- A C or C++ game is not packaged by it (its program is not a Cargo target);
  see [C and C++](#c-and-c).

## One way to build and run it

From a fresh checkout, with a Rust toolchain (on Linux also the ALSA and udev
development headers, as for any example):

```sh
git clone https://github.com/ponkoengine/ponko && cd ponko
cargo run --release --example coin_dash --features text-shaping   # play it
cargo test --test coin_dash                  # the whole game, headlessly
```

Move with the arrow keys, WASD or the left stick; Enter starts; Esc pauses; `O`
opens the settings. A round is eight coins in twenty seconds. The best time and
the high score are saved (see [Your files](#your-files)).

## The package

```sh
python3 scripts/package-coin-dash.py package              # release build -> target/package/coin-dash-<version>-<os>-<arch>/
python3 scripts/package-coin-dash.py smoke --package target/package
```

`package` builds the example and lays out a folder you can zip, with the same
files on every desktop:

| File | What it is |
|---|---|
| `coin_dash` (`coin_dash.exe`) | The program |
| `assets/coin_dash/coin.png` | The game's one asset, found next to the program |
| `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` | The engine's and the game's license |
| `CONTENT-NOTICES.txt` | What the game ships: the picture's author and license, "no sound files", "no font file" |
| `THIRD-PARTY-NOTICES.txt` | Every library linked into the program, with its declared license and the license texts its source ships |
| `README.txt` | How to run it, the controls, where it saves |

The asset lookup is the engine's own (`asset_roots`: next to `Cargo.toml` under
`cargo run`, next to the executable, in an app bundle's `Contents/Resources`,
then the current folder), so an unpacked
game finds its files wherever it is started from. The third-party notice is made
from `cargo tree -p ponko` for the platform it is built on (so it lists what
that binary links, not every optional feature) and the license files in each
crate's source, including the fonts' (`OFL.txt`, `UFL.txt`) in
`epaint_default_fonts`. Identical texts are printed once, with the libraries
that use them. `tests/coin_dash.rs` fails if a file under `assets/coin_dash/` is
not named in `CONTENT-NOTICES.txt`, and the smoke test fails if it is not in the
package's copy either. A library that declares no `license` (or `license_file`)
fails the packaging by name instead of being listed as "unknown"; an example
can link its package's dev-dependencies, so they are listed too. The script's
`package` is [`ponko package --example coin_dash --format folder
--no-smoke`](#packaging-any-game-ponko-package), whose unit tests check these
decisions with made-up crates (`cargo test -p ponko_cli`);
`python3 scripts/package-coin-dash.py selftest` checks the exit-code decisions
below.

## The smoke test

`coin_dash --smoke headless|window [--report file]` plays the game by itself:
title, settings (open, close), a round won by chasing coins, back to the title.
It is a simulation system like the scripted players in the tests
(`examples/coin_dash/smoke.rs`), and writes a JSON report of the screens it saw,
the rounds saved, how the progress and settings files were found, and the size of
the loaded coin picture. In `headless` mode there is no window or GPU; in
`window` mode a window opens and the report also counts the frames drawn.

`package-coin-dash.py smoke` unpacks the package to a new folder whose name has
a space and a non-ASCII letter (outside the checkout, with `CARGO_*` removed
from the environment), starts the program from a different, empty folder, and
checks:

1. the package's files, licenses and notices (including that the notices name
   the libraries and the shipped asset);
2. a first run: the walk-through finishes, the asset was found and decoded, and
   `progress.json` and `settings.json` exist;
3. a second run starts from them (`Loaded`) and adds a round;
4. a damaged `progress.json` is recovered from (`Recovered`, kept as
   `progress.json.bad`) and written again;
5. with no `PONKO_DATA_DIR`, files go to the operating system's data folder
   (a home folder of the test's own: `%APPDATA%`, `~/Library/Application
   Support`, `$XDG_DATA_HOME`);
6. a copy of the package *without* `assets/` fails, and says which file it
   could not find.

`--mode window` repeats step 2 in a real window. Exit codes: 0 passed, 1 a
check failed, 2 unsupported (no display or GPU, only without
`--allow-unsupported`), 3 no package. "Unsupported" means the engine's own
message that it found no GPU or could not open a window ("ponko: no compatible
GPU found" and the like); any other crash, such as a platform error, is a
failure, and under GitHub Actions a skipped window run prints a warning
annotation.

CI (`package-smoke`, on `main` and with the `full-ci` label) runs both
modes on Linux, macOS and Windows, and the window run must open a window on
all three: Linux under a virtual display with software Vulkan, Windows with
DirectX 12 only (WARP, the software renderer) where the runner has no GPU. It
packages the dev
profile (same layout and notices, and the dependencies the other jobs already
built); a release package is the command above. Locally, the window mode needs
a display, and the headless mode is what runs anywhere. On Linux under X11
a window also needs the system's X11 libraries at run time, among them
`libxkbcommon-x11` (a minimal install, like CI's runner, may lack it).

## Your files

Progress (best time, high score, rounds) and settings live in the player's data
folder: `%APPDATA%\coin-dash\`, `~/Library/Application Support/coin-dash/`, or
`$XDG_DATA_HOME/coin-dash/` (`~/.local/share/coin-dash/`), and the browser's
storage in the web build. The game has no path code of its own: it gives the
engine its app id (`Game::app_id("coin-dash")`) and reads and writes through
`ctx.storage` ([Saves and settings](storage.md)). `PONKO_DATA_DIR=<folder>`
moves the data folder (the files go in `<folder>/coin-dash/`).
Files are written through a temporary file and renamed, and the previous one is
kept as `<name>.bak`, so a crash leaves the old one. A missing file is a first
run. An unreadable one, one that doesn't parse, one with impossible numbers or
another version is copied to `<name>.bad`,
the game starts from the defaults and says so on the title screen. A write that
fails (a read-only disk) is shown and doesn't stop the game. The tests cover
every one of these cases for both files.

## The web build

```sh
web/build.sh --features text-shaping coin_dash && python3 -m http.server -d target/web/coin_dash 8000
```

builds the same game for the browser: the coin picture is fetched with the
asset list, the retained settings screen and the keys work as on the desktop.
Progress and settings are kept in the browser's storage through the same
`ctx.storage` calls ([Saves and settings](storage.md#in-the-browser)); it has
no quit command. The `web-smoke` CI job (the
browser runner of `web/smoke`, [issue
#173](https://github.com/ponkoengine/ponko/issues/173)) builds it after
the fixture and runs it in headless Chromium on WebGL2: it checks that the
asset was fetched and decoded as 32 x 32, that the simulation advances, that a
click and keys arrive, and that the screen the game reports follows them
(the `coin_dash` entry of `web/smoke/fixtures.json`: `--press Enter=screen:Playing
--press Escape=screen:Paused --press Q=screen:Title --press O=screen:Settings
--press Escape=screen:Title`), that after a page reload the settings saved
when the settings screen was left come back from the browser's storage
(`afterReload`: `settings_at_start:Loaded`, `storage:persistent`), then that
the canvas is not blank. The game
publishes its state on `<html data-smoke>` in the fixture's format plus the
screen name, and the GPU the engine opened on `<html data-smoke-gpu>`
(`publish_web_state`).

## UI scaling, rebinding and navigation

The settings screen (`examples/coin_dash/settings.rs`) is the engine's
[retained UI](ui.md), and shows three things the engine already has:

**Navigation.** Tab and Shift-Tab, the arrow keys or the d-pad move the focus;
Enter, Space or South press the focused control; Escape or East closes the
screen; the list of controls scrolls to keep the focus in view; the slider
changes by 0.05 on left and right. The game writes no navigation code, only
the reaction to the events. The tests drive it with the input the engine would
build, and find controls by their accessible names (so they also check that
every control has a useful one: the key buttons read "Confirm key: Enter", not
just "Enter"):

```rust
{{#include ../../tests/coin_dash.rs:ui_drive}}
```

**UI scale.** `Ui::set_scale` scales the screen's geometry, text and hit
testing together; the scale buttons step through 75%, 100%, 125% and 150%. The
text the game draws itself (`hud`) multiplies its sizes by the same factor, and a
test measures every screen's lines at every scale and fails if one is wider or
taller than the window. The scale is saved.

**Rebinding.** The installed `ActionMap` is the one record of what is bound.
At startup `bindings_for` turns the saved `Controls` (a key and a gamepad
button per control) into a map and `ctx.input.set_actions` installs it. The
settings screen shows the keys it reads back from `ctx.input.actions()` with
`Controls::from_bindings`, so a binding installed some other way (a
user-edited actions file, say) shows up as it is. Pressing a control's button
opens a small modal prompt (a screen with nothing to focus, so no key can press
a button behind it), and the next key or button pressed is the answer: the
prompt reads it from `ctx.input.pressed_this_frame()` (see [Named input
actions](actions.md#rebinding-screens-what-was-just-pressed)), so it lists no
keys and polls nothing. The
check runs against what the map binds now, `ctx.input.set_action` replaces
only the action that control feeds (its `ActionId` stays valid), and the copy
saved with the other settings is read back from the map. The hints on the
title, pause and result screens show the current keys.

```rust
{{#include ../../examples/coin_dash/settings.rs:set_key}}
```

```rust
{{#include ../../examples/coin_dash/settings.rs:bindings_for}}
```

```rust
{{#include ../../examples/coin_dash/settings.rs:from_bindings}}
```

```rust
{{#include ../../examples/coin_dash/settings.rs:rebind}}
```

```rust
{{#include ../../tests/coin_dash.rs:rebind_test}}
```

### Limitations

- **Rebinding**: one key and one gamepad button per control, read by the
  game from the first gamepad slot (the prompt listens on every slot, but the
  binding is made for slot 0); the left stick, the arrow keys, Space and P are
  fixed; there are no mouse bindings; a key or button another control uses is
  refused (swap in two steps) rather than swapped.
  **Reserved for the menus**: Escape and Tab on the keyboard, and East and the
  four d-pad directions on the gamepad. They can't be bound to a control
  (the prompt, the model and a hand-edited `settings.json` all refuse them),
  because the retained UI navigates with them and a screen that rebinds its own
  navigation keys could lock the player out. The one exception is Pause: it
  starts on Escape, and once moved elsewhere **Reset to defaults** is the only
  way to give it Escape back. Key names are physical positions on a US layout
  (`Key::Q` is where Q is on QWERTY, whatever is printed on the key). The
  gamepad prompt was exercised only with synthetic events: there is no
  controller on the build machines.
- **Scale**: four steps, 75 to 150%; the window isn't resized, and the largest
  step is what the title screen's text still fits. The engine scales the
  retained UI; the immediate-mode calls (`draw_text`, `ui_label`) aren't scaled
  by it, so the game scales its own.
- **Navigation** is tested with the `UiInput` the engine builds from keys and
  pad buttons, not with operating-system events. The game is built without
  the `accessibility` feature, so no screen reader reads it: its accessible
  names and roles are checked through the platform-neutral snapshot only.
- **Tests** of the retained UI drive `Ui::frame` by hand, because
  `run_headless` runs simulation systems only (no frame systems, no UI frame).
- **The window walk-through** can't be pixel-checked: it proves the program
  opens a window, loads its asset and draws frames.

## C and C++

This game is Rust only. A C or C++ game is packaged the same way: its
executable, the `ponko_ffi` shared library next to it, its `assets/` folder,
and the license files with `THIRD-PARTY-NOTICES.txt` from the release archive
(see [C and C++](c-cpp.md#get-the-library)). The settings screen's calls exist
in C++ too: `ctx.ui_retained().set_scale(scale)` and
`ctx.set_actions(map)` / `ctx.actions()` with a `ponko::action_map`.

## Gap list

What this exercise found that the engine or the project doesn't do yet, with
the issue that owns it (an entry says "no issue" where none does; they belong
in [#207](https://github.com/ponkoengine/ponko/issues/207), the API friction
index, or a new one):

| Gap | Where it is tracked |
|---|---|
| `run_headless` runs no frame systems and no UI frame, so screens built on the retained UI are tested by hand-feeding `Ui::frame`. | Phases: [#198](https://github.com/ponkoengine/ponko/issues/198); scripted input: [#179](https://github.com/ponkoengine/ponko/issues/179) |
| No native screen-reader adapter for the retained UI. | [#183](https://github.com/ponkoengine/ponko/issues/183) |
| Immediate-mode text and UI don't follow the retained UI's scale; the theme and builders are the retained UI's only. | [#203](https://github.com/ponkoengine/ponko/issues/203) |
| Saves and settings are plain files in the data folder; what a scene replacement does to them and to runtime services is undefined. | [#178](https://github.com/ponkoengine/ponko/issues/178); save-file compatibility fixtures [#174](https://github.com/ponkoengine/ponko/issues/174) |
| The browser matrix is Chromium on SwiftShader WebGL2 only (no real-GPU WebGPU, Firefox, Safari or mobile). | [#173](https://github.com/ponkoengine/ponko/issues/173) |
| The window smoke proves "a window opens and frames are drawn", not what they look like; no image regression for a whole game. | [#135](https://github.com/ponkoengine/ponko/issues/135) |
| The C and C++ bindings have no counterpart of this game (it is Rust only). | No native port planned yet |
| Packages aren't attached to a release, signed or notarized, and there is no installer; the third-party notice is generated by a short script (on macOS, 33 of the 188 linked libraries ship no license file in their published source, so only their declared license and authors are listed; the package says which) rather than by a license-policy tool. | [#282](https://github.com/ponkoengine/ponko/issues/282) |
