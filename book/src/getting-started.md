# Getting started

> **Maturity: not a feature.** Setup steps; the features it uses carry their
> own levels.

You need Rust 1.95 or newer (`rustup update stable`). On Linux, also the
headers for sound and gamepads: `sudo apt-get install libasound2-dev libudev-dev`
(Debian, Ubuntu).

## A new project

The `ponko` command makes a project that builds, tests and can be driven by
an AI agent as it is:

```sh
cargo install ponko_cli --version 0.2.0-rc.1
ponko new my_game        # a 2D game; --3d for a 3D game, --headless for a simulation
cd my_game
cargo test               # headless tests: no window, no GPU
cargo run                # the game, in a window
ponko package            # the download for players: release build, assets, licenses, archive
ponko textures           # GPU-compressed .ktx2 textures from your images (see Drawing)
ponko cook               # runtime-ready assets under the same names (package does it for you)
```

`ponko_cli` is new in 0.2. Until 0.2.0 is released, Cargo installs its
release candidate only when the command names its version, as above; plain
`cargo install ponko_cli` finds no version yet. The newest from the
repository: `cargo install ponko_cli --git https://github.com/ponkoengine/ponko`.
[The `ponko` command](cli.md) lists every command and option.

| Template | Engine features | What's in it |
|---|---|---|
| `--2d` (the default) | the defaults: window, sound, gamepads, inspector | a square moved with the arrow keys, kept inside the window |
| `--3d` | the defaults plus `3d` | a spinning cube on a ground plane, a camera and a sun |
| `--headless` | none (`default-features = false`): no window, GPU or sound | a thousand walkers bouncing around a square, and a summary |

Each project has:

- `Cargo.toml` with the engine, a fast debug profile (below) and the
  `[package.metadata.ponko]` table that tells `ponko package` what goes in the
  game's download ([Packaging any game](packaging.md#packaging-any-game-ponko-package));
- `src/lib.rs` with the rules as sim systems, `src/main.rs` that starts the
  game (`HEADLESS=1 cargo run` runs it without a window), and headless tests
  in `tests/` ([Headless runs and testing](headless.md));
- `assets/` with a `settings.json` of numbers to tune, which a debug build
  reloads when you save it ([Data files](data-files.md));
- `.mcp.json`, which registers [`ponko-mcp`](agents.md) for your coding agent
  (`cargo install ponko_mcp --version 0.2.0-rc.1` once), and `AGENTS.md` (`CLAUDE.md` points at it)
  with the commands, the layout and a pointer to [the agents chapter](agents.md).

To build a whole game from here, step by step, follow [Tutorial: a first 2D
game](tutorial-2d.md) (a platformer with a level from a level editor, a menu
and saves) or [Tutorial: a first 3D game](tutorial-3d.md) (a glTF scene, a
character controller, lights and UI).

The project depends on the released engine from crates.io. `--git <url>` or
`--path <checkout>` points it at an engine repository or a local checkout
instead (for features that are only on `main` so far, see below).

The same templates work with
[cargo-generate](https://github.com/cargo-generate/cargo-generate), if you
have it: `cargo generate --git https://github.com/ponkoengine/ponko
crates/ponko_cli/templates/2d` (or `3d`, `headless`).

### The fast debug profile

The templates' `Cargo.toml` builds debug builds like this:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
debug = "line-tables-only"

[profile.dev.package.syn]
opt-level = 0
```

Your own code gets light optimization, and the engine and every other
dependency full optimization, which they get once and keep: games and tests
run many times faster than in a plain debug build, rebuilding after an edit
stays quick, and your code keeps full debug info and debug checks. The
dependencies keep line tables only, enough for backtraces with files and
lines, and `syn` (which only runs inside derive macros while building) isn't
optimized: together that makes a clean debug build about a quarter faster.
Add it to a project you made by hand too. The templates also have a
`.cargo/config.toml` with faster linkers for Linux and Windows to switch on,
and `dist` and `dist-web` profiles for smaller downloads; [Build times and
download sizes](performance.md#build-times-and-download-sizes) has the
numbers.

## Your first game

To start by hand instead, add the engine to a new project from crates.io:

```sh
cargo new my_game && cd my_game
cargo add ponko@0.2.0-rc.1
```

Until 0.2.0 is released, plain `cargo add ponko` gives 0.1.0, which this guide
doesn't describe. In `Cargo.toml` the command above writes:

```toml
[dependencies]
ponko = "0.2.0-rc.1"
```

Its API reference is [on docs.rs](https://docs.rs/ponko/latest/ponko/).
**This guide matches the `main` branch**, which can be ahead of the release on
crates.io: the note under every page's title names the version `main` is at and
says how many changes it has that no release has yet. A page can describe an API
that is only on `main`, and the tutorials need the newest release (or `main`).
To use `main`, depend on it instead:
`ponko = { git = "https://github.com/ponkoengine/ponko" }` (or
`cargo add ponko --git https://github.com/ponkoengine/ponko`); the guide's
examples compile against that line. The [changelog](../../CHANGELOG.md) lists
what each release added. [The API reference](api.md#which-version-the-guide-matches)
says how to build the reference for `main` locally, with the features the
3D, physics and navigation chapters need.

Then in `src/main.rs` (Rust) or `main.cpp` (C++, see [C and C++](c-cpp.md)
for building):

<div class="lang-pair">
<div>

```rust
use ponko::prelude::*;

fn main() {
    Game::new("My Game", 800, 600).run(|ctx| {
        let crab = ctx.gfx.load_texture("crab.png");
        let mut pos = vec2(400.0, 300.0);

        move |ctx: &mut Ctx| {
            if ctx.input.key_down(Key::Right) { pos.x += 200.0 * ctx.time.dt(); }
            if ctx.input.key_down(Key::Left)  { pos.x -= 200.0 * ctx.time.dt(); }
            if ctx.input.key_pressed(Key::Escape) { ctx.quit(); }
            ctx.gfx.draw(crab, pos);
        }
    });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:first_game_full}}
```

</div>
</div>

The game draws `crab.png`, which the engine looks for in an `assets/` folder
next to `Cargo.toml`. Save any PNG there under that name before `cargo run`,
for example the one in the engine's repository:

```sh
mkdir -p assets
curl -L -o assets/crab.png https://raw.githubusercontent.com/ponkoengine/ponko/main/assets/crab.png
```

Without the file, `load_texture` stops the game at start with an error that
the asset was not found. A project from `ponko new` (above) needs no
picture: its square is drawn in code.

`setup` runs once and returns a closure that runs every frame. In C++, the
per-frame code is a frame `system` and the state lives in variables it
captures. Variables you
create in `setup` and move into the closure are your game state. For small
games that's all you need; when things grow, move to
[entities and systems](ecs.md).

Everything here comes in with `use ponko::prelude::*;`: `Game`, `Ctx`,
`Vec2`, `Color`, `Key`, the ECS types and the common components. Add
`use ponko::d2::*;` or `use ponko::d3::*;` for one dimension's names
([Naming and compatibility](naming.md)).

## Run the examples

Clone the repository and run any example:

```sh
git clone https://github.com/ponkoengine/ponko
cd ponko
cargo run --release --example boids
```

| Example | What it shows |
|---|---|
| `window` | The smallest game: a window, input, one sprite |
| `sprites` | Drawing many sprites with tint, rotation and scale |
| `ecs` | Entities, components and systems, with sim controls |
| `bunnymark` / `bunnymark_ecs` | Sprite throughput, immediate mode vs ECS |
| `boids` | Flocking with a spatial grid on all cores |
| `ants` | Ant colonies: pheromone trails, foraging, growth; up to millions of ants |
| `headless` | A million agents with no window or GPU |
| `coin_dash` | A small complete game (title, settings with UI scale and rebinding, rounds, pause, saved progress, sound), tested headlessly, packaged for the desktop and built for the web; see [A complete game, packaged](packaging.md) and [Headless runs and testing](headless.md#a-complete-game-tested-headlessly) |
| `examples/cpp/coin_dash/` | Coin Dash in C++, with the Rust game's scenario tests (see [C and C++](c-cpp.md#a-complete-game-coin-dash-in-c)) |
| `examples/cpp/boids.cpp` | The boids example in C++ (build with CMake, see [C and C++](c-cpp.md)) |

Always use `--release` for anything with many entities; debug builds are
10–50× slower.

## Setup on the builder

Once a game has systems, its startup work goes on the builder too:
`Game::setup` takes the startup code, and `run()` takes nothing.

<div class="lang-pair">
<div>

```rust
# fn movement(_: &mut Ctx) {} fn spawn_enemies(_: &mut Ctx) {}
# let with_enemies = true;
let game = Game::new("My Game", 800, 600)
    .sim_system(movement)
    .setup(|ctx| {
        let crab = ctx.gfx.load_texture("crab.png");
        ctx.world.spawn((ponko::d2::Transform::at(vec2(400.0, 300.0)), ponko::d2::Sprite::new(crab)));
    });
// Startup chosen at run time: each call adds one more callback.
let game = if with_enemies { game.setup(spawn_enemies) } else { game };
game.run(); // or run_headless(ticks)
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:builder_setup}}
```

</div>
</div>

- Setup callbacks run once, after the window and the renderer exist and
  before any system, in the order they were added. A
  [plugin](systems.md#plugins)'s `setup` is in the same queue, at the place
  the plugin was added.
- Commands recorded in a callback are applied when it returns, so the next
  callback and the first system see what it spawned.
- Add systems and options first: after the first `setup`, the builder
  takes more `setup` and `plugin` calls, the `run` methods, and what a test
  adds to a finished game without changing it: `headless_frames`,
  `input_scenario` and `run_headless_scenario(scenario)`, `data_dir`,
  `seed`, and systems of its own (`system`, `sim_system`). So the tests of a
  `ponko new` project can drive its menus with frames and keep its saves in
  a folder of their own while `game()` stays as it is.
- A game whose startup is all in its [plugins](systems.md#plugins) and
  systems has no `setup` to add: `.configured()` gives it the same
  `run()` and `run_headless(ticks)` (C++ already has `run()` and
  `run_headless(ticks)` on any game; in C, pass a null setup).
- Nothing is returned, so two branches can pick different startup code
  (above). The state a frame closure would have captured goes in a
  resource, or in a [system that owns its state](systems.md#systems-with-state).

`run(|ctx| ...)` and `run_headless(ticks, |ctx| ...)` stay for the small
cases: the one-closure program at the top of this page, and a headless run
whose setup borrows a local variable (a timer, a counter in a test), which a
stored `setup` callback can't do.

## When loading fails

`load_texture`, `load_mesh`, `load_sound`, `load_font` and the other
`load_*` / `create_*` calls are for setup code, where a missing file is a
bug: they panic with a message that says what's wrong and where the engine
looked. Each has a `try_` twin that returns `Result<_, ponko::error::Error>`
instead, for assets that may legitimately be missing (a mod, an optional
skin, a user's file):

```rust
# use ponko::prelude::*;
# fn setup(ctx: &mut Ctx) {
match ctx.gfx.try_load_texture("skins/gold.png") {
    Ok(skin) => { /* use it */ let _ = skin; }
    Err(e) if e.kind() == ErrorKind::NotFound => { /* no skin installed */ }
    Err(e) => eprintln!("{e}"), // "ponko: ..." with the reason
}
# }
```

Match on `e.kind()` (`NotFound`, `Io`, `Decode`, `Shader`, `Invalid`,
`BadHandle`, `Limit`, ...), not on the message text, which may change.
`ponko::error::Error` implements `std::error::Error`, so `?` passes it on into a
`Box<dyn Error>` or your own error type.

Calls that can fail in normal play return `Result<_, ponko::error::Error>` under
their plain name and have no panicking twin: loading a scene and spawning a
prefab (`ctx.load_scene`, `ctx.spawn_prefab`), saving them
(`ctx.assets.save_scene`, `save_prefab`), loading JSON into a component or
resource (`ctx.assets.load_reflected_component_json`, `load_reflected_resource_json`), starting a ragdoll,
baking a reflection probe, and your own decoders (`ctx.assets.load_with`).
In C and C++, `ponko_last_error_kind()` and `ponko::error::kind()` give the
same kinds ([C and C++](c-cpp.md)).

## Recoverable startup

Use `try_setup` to return loader errors with `?`, and a fallible runner to
let the caller decide how to present them. Setup callbacks and plugins run
in registration order; the first error stops startup before any systems run.
Configuration such as `.fixed_hz(60)` can come before or after `.try_setup(...)`.

```rust,no_run
use ponko::prelude::*;

fn main() -> Result<(), Error> {
    Game::new("My game", 800, 600)
        .try_setup(|ctx| {
            let texture = ctx.gfx.try_load_texture("player.png")?;
            ctx.world.insert_resource(texture);
            Ok(())
        })
        .fixed_hz(60)
        .try_run()?;
    Ok(())
}
```

On native platforms `try_run` returns event-loop, window, GPU surface,
adapter and device initialization failures, plus setup/plugin errors.
Startup failures return directly without invoking the panic hook, including
in `panic = "abort"` builds. User panics, later device loss, and the mid-game
"lost the GPU surface" panic are not covered. The ordinary `run` and
`run_headless` methods panic with the error's message.

Browser startup is asynchronous. Only event-loop construction errors can
return to the caller. After submission, winit transfers control using a
JavaScript control-flow exception; the call does not return, even on normal
exit. Window/GPU/setup failures after submission are reported in the browser
console as error diagnostics (without invoking the panic hook) and stop
startup. A Rust `main` returning `Result` cannot receive
those later failures; use the console to diagnose them.

The C `ponko_game_run` is built on `try_run`: a refused window, GPU or
event loop comes back as `PONKO_INVALID_ARGUMENT` (not a caught panic), with
`ponko_last_error_kind()` giving `PONKO_ERROR_UNSUPPORTED` and `ponko_last_error()`
the message. C++ `ponko::game::run` throws `ponko::error` from that status.
