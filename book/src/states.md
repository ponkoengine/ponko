# Game states: menus, loading, playing, paused

> **Maturity: [proven](maturity.md).** Game: Coin Dash and Ember Hop, packaged
> and played by the list of checks (recorded on macOS) (title, play, pause and end screens).
> Tests: `tests/states.rs`. Platforms: Linux, macOS and Windows, headless (the
> workspace tests in the list of checks). Graphics: not involved.

Most games have screens: a title, a loading screen, the game itself, a
pause menu over it, a results screen. Ponko keeps that in a **state
machine**: an `enum` the game defines, the work to do when a state starts
and ends, and systems that only run in some states.

<div class="lang-pair">
<div>

```rust
# use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen { Title, Playing, Paused }

#[derive(Default)]
struct Score { ticks: u32 }

fn play(ctx: &mut Ctx) {
    ctx.world.resource_mut::<Score>().ticks += 1;
}

// The menus ask for changes; they apply at the end of the tick.
fn menus(ctx: &mut Ctx) {
    match ctx.time.sim_tick() {
        0 => ctx.set_state(Screen::Playing),
        5 => ctx.push_state(Screen::Paused),
        8 => ctx.pop_state::<Screen>(),
        _ => {}
    }
}

let world = Game::new("States", 800, 600)
    .state(Screen::Title)
    .on_enter(Screen::Playing, |ctx: &mut Ctx| ctx.world.insert_resource(Score::default()))
    // Runs only while playing, not under the pause menu.
    .add(Phase::AfterPhysics, play.run_if(in_state(Screen::Playing)))
    .sim_system(menus)
    .run_headless(10, |_| {});
// Playing on ticks 1-5 and 9: six ticks of play.
assert_eq!(world.resource::<Score>().ticks, 6);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:states}}
```

</div>
</div>

The state type is any `Clone + PartialEq + Debug` type that serde can save
(derive `Serialize, Deserialize`): an `enum` of plain names is the usual
choice, and variants may carry data (`Result(Outcome)`). A game can have
several state machines of different types (the screen, the music, a
tutorial's step).

## Asking for a change

`ctx.set_state(s)`, `ctx.push_state(s)` and `ctx.pop_state::<S>()` (or the
same methods on the `State<S>` resource) **ask** for a change. Nothing
changes right away:

- The state **only changes between simulation ticks**. A change asked
  for during a tick applies at its end, after `Phase::SimLast`: a change
  asked for in tick 10 is what all of tick 11 sees, everywhere, and the
  frame drawn after tick 10 already shows it. Two systems of the engine's
  per state type do this, the first and the last of every tick
  (`apply_state<S>` in plans and timings).
- A change asked for **between ticks** (in setup, or by a frame system:
  menus drawn with the UI) applies at the start of the next tick, before
  `Phase::Input`, so that tick runs in the new state from its start.
- The **last request wins**: asking for `Playing` and then for `Title` in
  one tick goes to `Title`.
- While the **simulation is paused** (`ctx.time.pause()`) no tick runs, so
  no change applies either. A pause menu is a state of the game, not a
  paused clock: push `Paused` and let the systems that move things run
  only in `Playing`.
- `ctx.state::<S>()` reads the current state; `State<S>::next()` shows the
  change waiting to apply.

## On enter and on exit

`Game::on_enter(state, work)` and `Game::on_exit(state, work)` run `&mut
Ctx` functions when a state starts and ends: spawn the level when
`Playing` starts, despawn the menu when `Title` ends. They run at the
change, in registration order, with their commands applied after each
one. Inside on-exit work the state is still the old one; inside on-enter
work it is the new one.

- The first state's on-enter work runs at the start of the first tick.
- **Every set is a change.** Setting the state it is in, the same value,
  leaves it and enters it again: a restart (a "play again" button sets
  `Playing` while playing). So on-enter and on-exit work runs exactly once
  per change, and a state value doesn't change while its state lasts.
- A change asked for inside on-enter work waits for the next change
  point: a `Loading` state with nothing to load, entered at the start of
  a tick, moves on at that tick's end.
  `Game::loading_state` makes a loading state that loads an [asset
  group](loading.md) and moves on once it is ready.

## A stack: pause over play

`push_state(Paused)` enters `Paused` **over** `Playing` without leaving
it: no on-exit for `Playing`. `pop_state` leaves `Paused` (its on-exit
runs) and goes back to `Playing`, which is not entered again. `set_state`
leaves every state on the stack, top first, then enters the new one: from
the pause menu, `set_state(Title)` leaves `Paused`, then `Playing`. A pop
with nothing below is dropped.

`State<S>::below()` lists the states under the current one.

## A screen for a state

A pause menu, an options page or a game over screen is up exactly while
its state lasts. `Game::state_screen(state, mode, build)` ties the two
together: each time the game enters `state`, `build` makes the screen's
[UI tree](ui.md) and returns its root, which is pushed in `mode`; each time
the game leaves `state`, the screen is taken down and its nodes removed.

Closing the screen leaves the state too. Escape (or a gamepad's East
button) closes it, and so does a button marked `closes_screen()`, which
sends its `Clicked` and then pops its screen as Escape would. A state that
was pushed over another one is popped; a state that was set has nothing to
go back to, so its screen comes straight back up. A state pushed over
`state` leaves the screen up below its own.

<div class="lang-pair">
<div>

```rust
# use serde::{Deserialize, Serialize};
use ponko::ui::ScreenMode;
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen { Playing, Paused }

let game = Game::new("Pause", 800, 600)
    .state(Screen::Playing)
    .state_screen(Screen::Paused, ScreenMode::Modal, |ctx| {
        let menu = ctx.ui.root().layout(UiLayout::centered(300.0, 160.0)).id();
        // Like Escape: closes the menu, which pops Paused.
        ctx.ui.button(menu, "Resume").focused().closes_screen();
        menu
    });
# let _ = game;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:state_screen}}
```

</div>
</div>

C: `ponko_game_state_screen` takes a `PonkoUiBuildFn` that adds a detached
root and returns its id, and `ponko_ctx_ui_set_closes_screen` marks the
button. The cookbook's [pause menu](cookbook/menus.md#a-pause-menu) is
built this way.

## Timed states

A death animation, a fade between levels, a splash screen: states that
last a while and then move on. Don't keep the timer in the state value
(`Dying { t }`, set again every tick): every set is a change, so that
would leave and enter the state every tick. The state counts its own
time: `State<S>::ticks()` is the number of ticks run with the current
state on top, this one included, and `ctx.seconds_in_state::<S>()` is
that times the fixed step. Every change resets it; a state pushed over it
(a pause) stops its count, which goes on after the pop.

```rust
# use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen { Playing, Dying, Fade { to_level: usize } }
const DEATH_DELAY: f32 = 0.8;

fn rules(ctx: &mut Ctx) {
    match *ctx.state::<Screen>() {
        Screen::Dying if ctx.seconds_in_state::<Screen>() >= DEATH_DELAY => {
            ctx.set_state(Screen::Playing);
        }
        // Data fixed for the whole state may be part of its value.
        Screen::Fade { to_level } if ctx.seconds_in_state::<Screen>() >= 0.5 => {
            let _ = to_level; // load it
            ctx.set_state(Screen::Playing);
        }
        _ => {}
    }
}
# Game::new("t", 1, 1).state(Screen::Dying).sim_system(rules).run_headless(60, |_| {});
```

Data that changes while a state lasts (a score, a wave counter) belongs in
a resource or on entities; data fixed for the whole state (where a fade
leads) may be part of its value. The Ember Hop example works this way:
its `Dying` and `Transition { to }` screens read `seconds_in_state`.

## Run conditions

`.run_if(condition)` runs a system only when the condition holds, checked
right before it would run. Calling it again adds another condition; the
system runs when all hold.

```rust
# use serde::{Deserialize, Serialize};
# #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
# enum Screen { Title, Playing, Paused }
# fn movement(_: &mut Ctx) {} fn draw_level(_: &mut Ctx) {} fn title_music(_: &mut Ctx) {}
# fn cheats(_: &mut Ctx) {}
let game = Game::new("Conditions", 800, 600)
    .state(Screen::Title)
    // Only while the state is Playing (not while Paused is pushed over it).
    .add(Phase::AfterPhysics, movement.run_if(in_state(Screen::Playing)))
    // While Playing is the state or under the one on top: draw under the menu.
    .add(Phase::Draw, draw_level.run_if(in_stack(Screen::Playing)))
    // `not` and `or` combine conditions.
    .add(Phase::Frame, title_music.run_if(in_state(Screen::Playing).not()))
    // A closure may read anything in `ctx`.
    .add(Phase::Frame, cheats.run_if(|ctx: &Ctx| ctx.input.key_down(Key::F1)));
# let _ = game;
```

- **`in_state` and `in_stack`** only read the `State<S>` resource. They add
  a read of it to the system's declared access, so with
  [parallel systems](parallel-systems.md) the scheduler orders the system
  after any system that writes the state, and view systems with one still
  share batches: the state only changes between ticks, so the answer is
  the same anywhere in a tick.
- **A closure** (`|ctx: &Ctx| -> bool`, or `Condition::new`) may read
  anything, so a view system with one runs alone, a stage of its own; the
  schedule plan says why ("has a run condition that takes `&Ctx`").
- A system that a condition **skips** runs nothing. Its `Added` and
  `Changed` filters show everything since it last ran, so a system that
  resumes after a pause sees what happened meanwhile. (Removal logs of
  tracked components keep what such a system hasn't seen yet, as for a
  paused game.)
- The parallel scheduler on or off gives the same world, down to the
  change ticks, with conditions too.
- **Free when unused:** a system without a condition checks an empty list.
  The `schedule-off/` and `conditions/` benchmarks (`crates/ponko_bench/SCHEDULE.md`)
  show no difference above the noise.

A view system reads the state like any resource it declares:
`sys.world.resource::<State<Screen>>().get()` with
`SystemAccess::new().read_resource::<State<Screen>>()`; one that asks for
changes declares `write_resource`.

## Saves, tools and lockstep

`State<S>` is a reflected resource. World saves
([`save_world`](storage.md#save-slots)) hold it, with its stack, its tick
counts and any change still waiting, and loading a save brings it back.

**A load isn't a change.** Loading a world replaces the state with the
saved one at once, like every other resource, and runs no on-exit or
on-enter work: the saved world already holds what its state's on-enter
work did, and running it again would do it twice. Save where the state is
settled (Ember Hop saves its run on entering `Playing`, so loading the run
puts the game back to playing), and ask for a state after a load when the
game needs one. Tools see it
under `State<...>` in the resources, and the lockstep checksum compares it
like any reflected resource, so two peers whose states differ are a
desync. Changes apply on the same tick for every peer as long as the
systems that ask for them are simulation systems driven by the players'
inputs (`ctx.net.input`), as everything in a lockstep game is.

## From C and C++

C games have one state machine whose states are `uint32_t` (a C++ `enum
class screen : std::uint32_t` casts to it): `ponko_game_state`,
`ponko_game_on_enter`, `ponko_game_on_exit`,
`ponko_game_add_system_in_state`, `ponko_game_add_system_in_stack`, and in
systems `ponko_ctx_state`, `ponko_ctx_set_state`, `ponko_ctx_push_state`,
`ponko_ctx_pop_state`, `ponko_ctx_state_in_stack` and
`ponko_ctx_state_ticks`, and `ponko_game_state_screen`. The C++ wrapper
has the same as `game::state`, `on_enter`, `on_exit`, `state_screen`,
`add_in_state`, `add_in_stack` and
`ctx::state<S>()`, `set_state`, `push_state`, `pop_state`,
`in_state_stack`, `state_ticks`.

A condition of your own is a callback: `ponko_game_add_system_if` takes a
`PonkoConditionFn` (`bool (*)(PonkoCtx*, void*)`) checked right before the
system would run, and the C++ wrapper's `add_if` takes any callable. It
needs no state machine. As with a Rust closure condition, the system runs
alone; C and C++ systems take the whole context, so they always do, and
the schedule plan says so. The condition should only read.

<div class="lang-pair">
<div>

```rust
# fn spawn_wave(_: &mut Ctx) {}
let game = Game::new("Waves", 800, 600)
    .add(Phase::AfterPhysics, spawn_wave.run_if(|ctx: &Ctx| ctx.time.sim_tick() % 600 == 0));
# let _ = game;
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:states_condition}}
```

</div>
</div>
