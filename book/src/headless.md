# Headless runs and testing

`run_headless(ticks)` after [`Game::setup`](getting-started.md#setup-on-the-builder),
or `Game::run_headless(ticks, setup)` in one call (C++: `ponko::game::run_headless`,
both forms), runs the simulation with no window and no
GPU, as fast as the CPU allows, then hands you the `World`.

Use it for:

- **tests** of game logic in plain `cargo test`, including on CI machines
  without a GPU;
- **dedicated servers** (paced to wall time by `run_server`, see
  [Dedicated servers](servers.md)) and batch runs;
- **benchmarks** of the simulation alone.

<div class="lang-pair">
<div>

```rust
use ponko::prelude::*;

#[derive(Component)]
struct Pos(f32);

fn fall(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world.query::<&mut Pos>().for_each(|p| p.0 -= 10.0 * dt);
}

#[test]
fn things_fall_ten_units_per_second() {
    let mut world = Game::new("test", 800, 600)
        .sim_system(fall)
        .setup(|ctx| {
            ctx.world.spawn((Pos(100.0),));
        })
        .run_headless(60);
    let y = world.query::<&Pos>().iter().next().unwrap().0;
    assert!((y - 90.0).abs() < 1e-3);
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:headless}}
```

</div>
</div>

## What runs

- The setup callbacks run once, in the order they were added, also when
  `ticks` is 0. If the `setup` passed to `run_headless(ticks, setup)`
  returns a frame closure, that closure is dropped.
- Each tick runs every **sim system** once. Frame systems, the UI and
  drawing don't run, because there are no frames, unless you ask for
  [headless frames](#menus-and-the-ui-headless-frames).
- Pause and sim speed are ignored, so every tick runs; `ctx.quit()` stops
  early. `ctx.physics.set_paused(true)` does hold the physics step
  ([Pausing only physics](sim-clock.md#pausing-only-physics)), and
  `ctx.physics3d.set_paused(true)` the 3D one.
- Textures load for their sizes but nothing is uploaded.

The same game can run windowed or headless: the boids example does both
(`HEADLESS=1 cargo run --release --example boids`), and
[`examples/headless.rs`](../../examples/headless.rs)
simulates a million agents this way.

## A complete game, tested headlessly

[`examples/coin_dash`](../../examples/coin_dash)
is a small complete game (title, settings, pause and result screens, win, lose,
restart, saved progress, keyboard and gamepad; one picture file, everything else
made in code) built so that a test can play it. Packaging it, the settings and the
web build are in [A complete game, packaged](packaging.md). It has three layers,
each a [plugin](systems.md#plugins):

1. `Controls`: `read_input`, a frame system, turns the named
   [actions](actions.md) into an `Intent` resource (below).
2. `Rules`: `rules`, a sim system, is the whole game. It reads only the
   `Intent` and the world, never the keyboard.
3. `Drawing`: the built-in sprite drawing plus a `hud` frame system.

```rust
{{#include ../../examples/coin_dash/game.rs:plugins}}
```

`read_input` resolves the action names once, at startup, into `ActionId`s, so
a name that `bindings` doesn't define is logged once instead of reading false on
every frame:

```rust
{{#include ../../examples/coin_dash/game.rs:controls}}
```

```rust
{{#include ../../examples/coin_dash/game.rs:read_input}}
```

Because `rules` reads only the `Intent`, [`tests/coin_dash.rs`](../../tests/coin_dash.rs)
replaces step 1 with a scripted player: a sim system added before the game's
own, which writes the `Intent` each tick. The game's plugin is added
unchanged:

```rust
{{#include ../../tests/coin_dash.rs:script}}
```

```rust
{{#include ../../tests/coin_dash.rs:chase}}
```

A whole round then runs in a few milliseconds, with the same result every time
because the game uses the context's seeded random numbers (see
[Randomness, determinism and saving](determinism.md)):

```rust
{{#include ../../tests/coin_dash.rs:win}}
```

The same file tests a loss, restarts, reproducible layouts, and the input
mapping itself: `feed_input` presses keys and moves sticks, and the test calls
`read_input` and checks the `Intent` it produced.

## Menus and the UI: headless frames

A plain headless run has no frames, so a game's menus, HUD and input mapping
(frame systems and the [retained UI](ui.md)) never run in it. Turn frames on
with `Game::headless_frames(true)` (C++: `game::headless_frames`, C:
`ponko_game_headless_frames`) and every tick becomes a whole frame, in the
order a window runs one:

1. the tick's scenario input is fed (see below);
2. the retained UI takes the frame's input: focus moves, buttons are
   clicked, screens pop, its events are queued;
3. the simulation tick runs;
4. the frame, `Animate`, `Late` and `Draw` systems run, then the closure
   `setup` returned.

The window is virtual: `ctx.window_size()` is the size given to `Game::new`,
and the UI lays out in it. Nothing is drawn (draw calls are dropped) or
played, and no window or GPU is opened, so it runs in plain `cargo test` on
a CI machine. `run_headless`, `run_headless_scenario` and `profile_headless`
all follow the setting.

[Input scenarios](actions.md#input-scenarios) then reach the UI:

- **Keys, buttons and sticks** of a tick are fed before the UI takes the
  frame's input, as a window's are: Tab moves the focus, Enter presses the
  focused button and Escape pops a screen in the frame of their tick.
- **`click(tick, node)`** clicks the node with that
  [name](ui.md) (`.name("menu.play")`, `ui.find`): the pointer presses and
  releases at its centre in that frame, through the UI's own hit testing. A
  name nobody has, a node that isn't shown, or one covered by a modal, a
  disabled one or another node fails the run at that tick, with what was
  under the pointer.
- **`type_text(tick, node, text)`** clicks a text input and types `text` at
  the end of what it holds; the field sends `Changed`. Add key steps for
  Enter or Backspace.
- **`ScenarioCheck::ui(node)`** checks a node: `.exists(..)`, `.shown(..)`
  (on a screen of the stack and not hidden, `Ui::is_shown`), `.focused(..)`
  and `.text(..)`. Only what you ask for is compared, and a failure reports
  what was expected and what the UI had. With frames, every check of a tick
  runs after the whole frame, so it sees what the frame's systems did.
- One UI step is used per frame; a second one on the same tick waits for the
  next frame. In the JSON form they are `ui_click` (`node`), `ui_type`
  (`node`, `text`) and `expect_ui` (`node` and any of `exists`, `shown`,
  `focused`, `text`), so tools can send them too.

<div class="lang-pair">
<div>

```rust
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};

/// Frame system: the menu's Settings button opens the settings screen.
fn menu(ctx: &mut Ctx) {
    let settings = ctx.ui.find("menu.settings");
    if ctx.ui.events().any(|e| e.clicked().is_some() && e.clicked() == settings) {
        ctx.ui.pop_screen();
        let screen = ctx.ui.find("settings").unwrap();
        ctx.ui.push_modeless(screen);
    }
}

#[test]
fn menu_opens_settings() {
    let scenario = InputScenario::new()
        .expect(0, ScenarioCheck::ui("menu.settings").shown(true).text("Settings"))
        .click(2, "menu.settings")
        .expect(2, ScenarioCheck::ui("settings").shown(true))
        .expect(2, ScenarioCheck::ui("menu.settings").shown(false));
    let (_world, report) = Game::new("menu test", 800, 600)
        .headless_frames(true) // frames too: the UI and frame systems run
        .system(menu)
        .run_headless_scenario(scenario, |ctx| {
            let root = ctx.ui.root().layout(ponko::ui::UiLayout::fill()).id();
            ctx.ui
                .button(root, "Settings")
                .layout(ponko::ui::UiLayout::fixed(200.0, 40.0))
                .name("menu.settings");
            ctx.ui.root().layout(ponko::ui::UiLayout::fill()).name("settings");
            ctx.ui.push_modeless(root);
        });
    report.assert_passed();
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:headless_frames}}
```

</div>
</div>

[`tests/coin_dash_menus.rs`](../../tests/coin_dash_menus.rs)
plays the coin_dash menus this way, with the game's real `read_input` and its
retained-UI settings screen, and checks the UI at every step: the title
screen, the settings (Tab to a button, click "+", click "Back"), a round, the
pause screen, back to the title, and the settings again closed with Escape.
The game only had to name the four settings nodes the test touches:

```rust
{{#include ../../tests/coin_dash_menus.rs:menus}}
```

Ticks and frames line up as in a window: a key pressed on tick 10 is read by
`read_input` in frame 10, after that frame's tick, so the rules (a sim
system) change the screen in tick 11, and the settings screen a frame system
pushes is checked at tick 11.

What it doesn't do:

- **No pixels.** Draw calls are dropped, so text a system draws with
  `draw_text` (coin_dash's title and HUD) can't be checked: check the state
  it is drawn from, or use an [offscreen GPU test](render-tests.md).
- **One tick per frame**, at a fixed size. A window runs as many ticks per
  frame as its clock says (one while a scenario runs), and can be resized.
- **UI steps need frames.** In a window, a UI step lands in the frame after
  its tick (the frame's UI input was taken before the tick); a plain
  headless run has no frames and never clicks.

**Cost.** Off, a headless run is the simulation-only loop it was: on the
`headless/sim-only-10k-ticks` benchmark (10,000 ticks of a game that does
nothing, `cargo run --release -p ponko_bench -- headless/`) it took 1.07 to
1.09 ms before and 1.09 to 1.10 ms after, a difference inside the run-to-run
noise of the busy machine it was measured on, with the same 358 allocations
per run. With frames, `headless/frames-10k-ticks` costs about 0.9 µs a frame
for that empty game (the UI frame and the built-in draw systems), and no
allocations per frame.

### Insets as a scenario step

A layout for a notched phone is checked without one: the scenario step
`safe_insets(tick, SafeInsets::new(top, right, bottom, left))` feeds the
window's [safe area](touch.md#notches-home-bars-and-rounded-corners) on that
tick, before its systems run, as `ctx.window.feed_safe_insets` does. With
frames, the UI lays out with it in the frame of that tick (a root made
`inside_safe_area` follows it) and the insets stay until another step changes
them, so a scenario can turn the phone. They hold for as long as the scenario
runs: a windowed web run that re-reads the page's insets on a resize keeps the
step's, and the page's apply once the scenario ends. In the JSON form, which the control
endpoint and tools take, it is `{ "tick": 3, "type": "safe_insets", "top": 0,
"right": 47, "bottom": 21, "left": 0 }` (a side left out is 0). From C it is
`ponko_input_scenario_safe_insets`, from C++ `safe_insets(tick, insets)`.

```rust
use ponko::prelude::*;
use ponko::scenario::InputScenario;
use ponko::window::SafeInsets;

#[test]
fn the_hud_keeps_clear_of_the_notch_when_the_phone_turns() {
    let scenario = InputScenario::new()
        // Held sideways, the notch on the left; at tick 2, turned the other way.
        .safe_insets(0, SafeInsets::new(0.0, 0.0, 21.0, 47.0))
        .safe_insets(2, SafeInsets::new(0.0, 47.0, 21.0, 0.0))
        .duration(4);
    let (_world, report) = Game::new("notch", 844, 390)
        .headless_frames(true)
        .add(Phase::Frame, |ctx: &mut Ctx| {
            let hud = ctx.ui.find("hud").unwrap();
            // The HUD fills what the player can see.
            assert_eq!(ctx.ui.rect(hud), Some(ctx.window.safe_area()));
        })
        .run_headless_scenario(scenario, |ctx| {
            let hud = ctx.ui.root().layout(ponko::ui::UiLayout::fill()).name("hud").inside_safe_area().id();
            ctx.ui.show_hud(hud);
        });
    report.assert_passed();
}
```

## The pointer, sounds and quitting

A headless run has no mouse. Its pointer rests at the window's top-left
corner until a test moves it: `InputEvent::MouseMove(pixel)` puts it on a
window pixel (`ctx.input.mouse_pos()`, `ctx.mouse_world()` and the UI's
hover follow it), and `InputEvent::MouseWheel(lines)` turns the wheel for
one tick. Feed them with `ctx.input.feed(..)` or as scenario steps
(`mouse_move` and `mouse_wheel` in the JSON form), so a click test aims at
the thing it clicks instead of moving the camera under a fixed pointer.

Nothing is heard either, but `ctx.audio.started()` lists the sounds
started in the tick that is running (or, in a frame system, the frame so
far): each with its handle, sound, bus, volume, speed and looping. Every
headless sound gets a handle of its own. `ctx.quit_requested()` says
whether a system called `ctx.quit()`; the run ends before the next tick. A
system added after the game's own records both, so a test asserts what was
played and that the game asked to quit:

```rust
{{#include ../../tests/audio.rs:headless_ears}}
```

C: `ponko_ctx_feed_mouse_move`, `ponko_ctx_feed_mouse_wheel`,
`ponko_input_scenario_mouse_move` and `ponko_input_scenario_mouse_wheel`;
`ponko_ctx_sounds_started` and `ponko_ctx_sound_started` (a versioned
`PonkoSoundStarted`); `ponko_ctx_quit_requested`. C++:
`input_event::mouse_move` and `input_event::wheel`, `ctx::sounds_started()`
and `ctx::quit_requested()`. The [cookbook](cookbook/index.md)'s click,
sound and quit recipes are tested this way.

## Builds without a window

A simulation server or a CI job that only runs headless doesn't need winit
and wgpu. Turn the `window` feature off and keep the rest:

<div class="lang-pair">
<div>

```toml
# Cargo.toml
[dependencies]
ponko = { version = "0.1", default-features = false, features = ["audio"] }
```

</div>
<div>

```sh
# Build the C library without a window
cargo build --release -p ponko_ffi \
  --no-default-features --features audio
```

</div>
</div>

That C build needs 168 crates instead of 360. It always keeps 3D models and
navigation, because the C API exports those functions in every build. The
Rust engine alone is smaller still: 72 crates with no default features, 143
with only `audio`, and 312 with the defaults. The `feature-deps` check makes
sure these reduced
builds never pull in the renderer, an audio device, the inspector, glTF,
physics or navigation by accident, and that builds without the default
features carry no JPEG or other image decoder (`scripts/feature-deps.sh`). Everything in
this chapter works the same. Drawing calls do nothing, and `Game::run`
(C: `ponko_game_run`) stops with a message that says to use `run_headless`.

## Fallible headless startup

`try_run_headless` returns the first stored setup or plugin error, including
when asked to run zero ticks. It opens no window or GPU. For example:

```rust
use ponko::prelude::*;
fn main() -> Result<(), Error> {
    let world = Game::new("Simulation", 1, 1)
        .try_setup(|ctx| {
            let value = "42".parse::<u32>()
                .map_err(|e| Error::new(ErrorKind::Invalid, e.to_string()))?;
            ctx.world.insert_resource(value);
            Ok(())
        })
        .try_run_headless(10)?;
    assert_eq!(*world.resource::<u32>(), 42);
    Ok(())
}
```

The direct `Game::try_run_headless(ticks, setup)` form retains the existing
unit-or-frame-closure callback convention. Register fallible work through
`try_setup`; `SetupResult` has no blanket implementation for `Result`.
