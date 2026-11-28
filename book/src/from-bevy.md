# Coming from Bevy

<!-- Maturity: not a feature. A translation table for Bevy users; the features
     it maps to carry their own levels. -->

This chapter is for people who know [Bevy](https://bevy.org) and want to
write the same game in Ponko. It maps Bevy's concepts to Ponko's, shows the
fifteen most common tasks side by side, says what is different on purpose and
what Ponko does not have, and ends with a porting checklist and one of Bevy's
own examples ported line by line.

Every Bevy snippet here is for **Bevy 0.20.1**, the version Ponko's
[scoreboard](performance.md#ponko-vs-bevy-the-scoreboard) pins. They are not
written from memory: each one is in
[`bench/compare/tests/from_bevy.rs`](../../bench/compare/tests/from_bevy.rs)
and compiles against that Bevy (`cargo check --tests` in `bench/compare`, the
only crate in the repository that depends on Bevy). Every Ponko snippet
compiles with the guide's other snippets. This chapter is Rust only: Bevy has
no C or C++ API, and the Ponko side of each task in C++ is in the chapter the
task links to.

## The short version

- A Bevy system names what it needs in its parameters (`Query`, `Res`,
  `Commands`, `MessageWriter`). A Ponko system takes `&mut Ctx`, which holds
  the world, the commands, the events, input, time, drawing, sound and the
  UI. You reach for what you need inside the function.
- Bevy's `Update` runs once a frame; its `FixedUpdate` runs at a fixed rate.
  Ponko's `system` and `sim_system` are the same pair, but Ponko expects game
  logic in `sim_system`: headless runs and tests run only the fixed ticks.
- Bevy runs compatible systems in parallel and orders them when you ask.
  Ponko runs systems one at a time in the order you add them, inside fixed
  [phases](systems.md#phases); parallel systems are
  [opt-in](parallel-systems.md).
- Ponko's 2D world has **+y pointing down** and the camera's top-left at
  (0, 0) by default. Bevy's has +y up and (0, 0) in the middle.
- Loading is synchronous by default in Ponko: `load_texture` returns a
  handle whose image is ready. Bevy's `AssetServer::load` returns a handle at
  once and loads in the background.

## The concepts, one to one

| Bevy 0.20.1 | Ponko | Notes |
|---|---|---|
| `App::new().add_plugins(DefaultPlugins)...run()` | `Game::new(title, w, h)...run()` | Window, input, drawing, sound and UI are on by default; cargo features turn parts off ([Builds without a window](headless.md#builds-without-a-window)). |
| `Plugin` (a type or an `fn(&mut App)`) | `Plugin` with `build(&self, game: Game) -> Game` and `setup(&self, ctx)` | [Plugins](systems.md#plugins). A plugin is added once, by name; dependencies are declared. |
| `Startup` systems | `Game::setup(|ctx| ...)` or a plugin's `setup` | Runs once, in the order added, commands applied after each. |
| `Update` | `Game::system(f)` (`Phase::Frame`) | Once per rendered frame. |
| `FixedUpdate`, `Time<Fixed>` | `Game::sim_system(f)` (`Phase::AfterPhysics`), `.fixed_hz(n)` | 60 Hz by default. [The simulation clock](sim-clock.md). |
| `PreUpdate`, `PostUpdate`, other schedules | `Phase::Input`, `BeforePhysics`, `Physics`, `AfterPhysics`, `SimLast`, `Frame`, `Animate`, `Late`, `Draw` | A fixed list; no custom schedules. [Phases](systems.md#phases). |
| `.before()`, `.after()`, `.chain()` | Registration order inside a phase; `Game::order(first, second)` | No dependency graph to solve. [Systems and the frame](systems.md). |
| System parameters (`Query`, `Res`, `ResMut`, `Commands`, `Local`) | `&mut Ctx`: `ctx.world`, `ctx.commands`, ...; state in a closure | [Systems with state](systems.md#systems-with-state) replace `Local`. Declared access, for tools and the opt-in scheduler: [Declared system access](system-access.md). |
| `Query<(&A, &mut B), With<C>>` | `ctx.world.query::<(&A, &mut B)>().filter::<With<C>>()` | `for_each`, `iter`, `par_for_each`. [Queries](ecs.md#queries). |
| `Single<D>` | `query(...).iter().next()` | No single-entity parameter. |
| `Commands` | `ctx.commands` | Applied after every system. [Commands](ecs.md#commands-changing-the-world-while-you-iterate). |
| `Resource`, `Res<T>`, `ResMut<T>` | `ctx.world.insert_resource`, `resource::<T>()`, `resource_mut::<T>()` | Any `'static` type; no derive. [Resources](ecs.md#resources). |
| `Message`, `MessageWriter`, `MessageReader`, `add_message` | `ctx.events.send(x)`, `ctx.events.read::<T>()` | Any type, nothing to register; each system sees each event once. [Events](events.md). |
| Observers (`On<Add, T>`, `On<E>`), component hooks | [Component hooks](ecs.md#component-hooks) | No observers or entity events; see [what is missing](#what-is-missing-or-slower). |
| `Changed<T>`, `Added<T>`, `RemovedComponents` | `Changed<T>`, `Added<T>`, `world.removed::<T>()` | Opt in per type with `track_changes::<T>()`. [Change detection](ecs.md#change-detection). |
| Required components (`#[require(...)]`) | `#[component(requires(...))]` | [Required components](ecs.md#required-components). |
| `States`, `init_state`, `NextState::set`, `OnEnter`, `OnExit`, `in_state` | `Game::state`, `ctx.set_state`, `on_enter`, `on_exit`, `in_state`, `in_stack` | Plus a stack (`push_state`, `pop_state`) for pause menus. [Game states](states.md). |
| `Handle<T>`, `AssetServer::load` | Small `Copy` handles: `Texture`, `Sound`, `Font`, ...; `ctx.gfx.load_texture`, `ctx.audio.load` | Loaded in the call; background loading with [asset groups](loading.md) and [streaming](streaming.md); freeing in [Handles](handles.md). |
| Scenes (`bsn!`, world serialization with reflection) | JSON scenes and prefabs of reflected components, `load_scene`, `spawn_prefab` | [Scenes and prefabs](scenes.md); a visual [editor](editor.md) writes them. |
| `Reflect`, Bevy Remote Protocol | [Reflection](reflection.md), the [control endpoint](control.md) and [`ponko-mcp`](agents.md) | |
| `Transform`, `GlobalTransform` | `d2::Transform` (`position`, `rotation`, `scale`) and `d3::Transform` | One transform: a child's is written from its parent's. |
| `ChildOf`, `Children`, `children!` | `Attached` / `Attached3D`, `set_parent`, `despawn_recursive` | [The entity hierarchy](hierarchy.md). |
| `Camera2d` entity | `ctx.camera` (always there) | [The camera](camera-ui.md#the-camera); several cameras: [viewports](viewports.md). |
| `Sprite`, `Mesh3d` + `MeshMaterial3d` | `Sprite` component or `ctx.gfx.draw`; `Model` | [Drawing](drawing.md), [3D](3d.md). |
| `bevy_ui` (`Node`, `Text`, `Button`) | `ctx.ui`, a retained tree with builders; `ctx.gfx.draw_text` for immediate text | [Retained game UI](ui.md). |
| `ButtonInput<KeyCode>` | `ctx.input.key_down(Key::Right)`, `key_pressed`, `key_released` | [Input](input.md); [named actions](actions.md). |
| `Time`, `Timer` | `ctx.time.dt()`, `elapsed()`, `.every(n)` on a system | No `Timer` type; see [task 10](#10-do-something-every-two-seconds). |
| `MinimalPlugins` + `app.update()` in tests | `run_headless(ticks)` | [Headless runs and testing](headless.md). |
| `bevy_rapier2d`, `avian` | Built in: `Game::physics`, `Game::physics3d` (rapier) | [Physics](physics.md), [3D physics](physics3d.md). |

## Fifteen common tasks, side by side

### 1. Start a game with a system

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:app}}
```

</div>
<div>

```rust
# fn setup(_: &mut Ctx) {} fn movement(_: &mut Ctx) {}
Game::new("My game", 1280, 720)
    .system(movement)
    .setup(setup)
    .run();
```

</div>
</div>

`run` opens the window and returns when it closes. There is no
`DefaultPlugins`: what a game does not use is left out with cargo features.

### 2. A camera and a sprite

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:sprite}}
```

</div>
<div>

```rust
fn setup(ctx: &mut Ctx) {
    ctx.camera.look_at(Vec2::ZERO); // (0, 0) in the middle, as in Bevy
    let crab = ctx.gfx.load_texture("crab.png");
    ctx.world.spawn((d2::Transform::at(Vec2::ZERO), Sprite::new(crab)));
}
```

</div>
</div>

The camera is not an entity. Without `look_at`, world (0, 0) is the
window's top-left corner.

### 3. Move things every frame

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:movement}}
```

</div>
<div>

```rust
#[derive(Component)]
struct Velocity(Vec2);

fn movement(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(&Velocity, &mut d2::Transform)>()
        .for_each(|(velocity, transform)| transform.position += velocity.0 * dt);
}
```

</div>
</div>

A 2D transform is 2D: `position` is a `Vec2`, and draw order is the
sprite's `z`.

### 4. Keyboard input

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:input}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Velocity(Vec2);
#[derive(Component)]
struct Player;

fn steer(ctx: &mut Ctx) {
    let mut x = 0.0;
    if ctx.input.key_down(Key::Right) { x += 200.0; }
    if ctx.input.key_down(Key::Left) { x -= 200.0; }
    let jump = ctx.input.key_pressed(Key::Space);
    ctx.world.query::<&mut Velocity>().filter::<With<Player>>().for_each(|v| {
        v.0.x = x;
        if jump { v.0.y = -400.0; } // jump: -y is up
    });
}
```

</div>
</div>

`key_down` is Bevy's `pressed`, `key_pressed` its `just_pressed`. For
remappable controls use [named input actions](actions.md).

### 5. A resource

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:resource}}
```

</div>
<div>

```rust
struct Score(u32);

fn add_point(ctx: &mut Ctx) {
    ctx.world.resource_mut::<Score>().0 += 1;
}

let game = Game::new("Score", 800, 600)
    .system(add_point)
    .setup(|ctx| ctx.world.insert_resource(Score(0)));
# let _ = game;
```

</div>
</div>

### 6. Despawn while iterating

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:despawn}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
fn die(ctx: &mut Ctx) {
    let commands = &mut ctx.commands;
    ctx.world.query::<(Entity, &Health)>().for_each(|(entity, health)| {
        if health.0 <= 0.0 { commands.despawn(entity); }
    });
}
```

</div>
</div>

### 7. Messages between systems

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:messages}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
struct Died {
    who: Entity,
}

fn deaths(ctx: &mut Ctx) {
    ctx.world.query::<(Entity, &Health)>().for_each(|(entity, health)| {
        if health.0 <= 0.0 { ctx.events.send(Died { who: entity }); }
    });
}

fn funerals(ctx: &mut Ctx) {
    for message in ctx.events.read::<Died>() {
        ctx.world.despawn(message.who);
    }
}

// Systems in one phase run in the order they are added: no `chain`.
let game = Game::new("Messages", 800, 600).sim_system(deaths).sim_system(funerals);
# let _ = game;
```

</div>
</div>

What Bevy 0.20 calls a message, Ponko calls an event. Nothing is registered,
and [who sees what](events.md#who-sees-what) is the same rule for every
system: each event exactly once.

### 8. Game states

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:states}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Player;
# fn movement(_: &mut Ctx) {}
# use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
enum Screen {
    Title,
    Playing,
}

fn start(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Enter) {
        ctx.set_state(Screen::Playing);
    }
}

fn spawn_level(ctx: &mut Ctx) {
    ctx.world.spawn((Player, d2::Transform::default()));
}

let game = Game::new("States", 800, 600)
    .state(Screen::Title)
    .on_enter(Screen::Playing, spawn_level)
    .add(Phase::Frame, start.run_if(in_state(Screen::Title)))
    .add(Phase::AfterPhysics, movement.run_if(in_state(Screen::Playing)));
# let _ = game;
```

</div>
</div>

States are saved with the game, so they derive serde's traits instead of
`States`. A change applies between ticks
([Asking for a change](states.md#asking-for-a-change)).

### 9. A fixed time step

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:fixed}}
```

</div>
<div>

```rust
# fn movement(_: &mut Ctx) {}
let game = Game::new("Fixed", 800, 600)
    .fixed_hz(120)
    .sim_system(movement); // ctx.time.dt() is 1/120 s in here
# let _ = game;
```

</div>
</div>

### 10. Do something every two seconds

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:timer}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Health(f32);
fn spawn_on_a_timer(ctx: &mut Ctx) {
    ctx.commands.spawn((Health(10.0), d2::Transform::default()));
}

// Every 120th tick: two seconds at the default 60 ticks a second.
let game = Game::new("Timer", 800, 600)
    .add(Phase::AfterPhysics, spawn_on_a_timer.every(120));
# let _ = game;
```

</div>
</div>

Ponko has no `Timer` type. A system's interval covers repeating work;
a countdown in a component covers fuses and cooldowns
([Time and timers](cookbook/time.md)).

### 11. A parent and a child

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:hierarchy}}
```

</div>
<div>

```rust
# #[derive(Component)]
# struct Player;
use ponko::attach::Attached;

fn spawn_body(ctx: &mut Ctx) {
    let body = ctx.world.spawn((Player, d2::Transform::at(vec2(10.0, 0.0))));
    // A child at (2, 0) in the body's space.
    ctx.world.spawn((d2::Transform::default(), Attached::to(body).offset(vec2(2.0, 0.0))));
}

fn clear_body(ctx: &mut Ctx) {
    let body = ctx.world.query::<Entity>().filter::<With<Player>>().iter().next();
    if let Some(body) = body {
        ctx.world.despawn_recursive(body); // and its children
    }
}
```

</div>
</div>

Add `systems::follow_attachments` as a `sim_system` after the systems that
move things; it places children where their parents and offsets put them
([Transforms](hierarchy.md#transforms)).

### 12. Text on the screen

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:text}}
```

</div>
<div>

```rust
# struct Score(u32);
fn hud(ctx: &mut Ctx) {
    let score = ctx.world.resource::<Score>().0;
    ctx.gfx.draw_text(&format!("Score: {score}"), vec2(16.0, 16.0), 24.0, Color::WHITE);
}

let game = Game::new("Text", 800, 600).add(Phase::Draw, hud);
# let _ = game;
```

</div>
</div>

Immediate text is drawn again every frame, so there is no text entity to
keep up to date. A retained label is `ctx.ui.label(parent, text)`, changed with
`ui.set_text` ([Retained game UI](ui.md)).

### 13. Play a sound

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:sound}}
```

</div>
<div>

```rust
let hit = ctx.audio.load("hit.ogg");
ctx.audio.play(hit);

let theme = ctx.audio.load("theme.ogg");
ctx.audio.play_ex(theme, SoundParams { looping: true, ..Default::default() });
```

</div>
</div>

### 14. A button

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:button}}
```

</div>
<div>

```rust
# use serde::{Deserialize, Serialize};
# #[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
# enum Screen { Title, Playing }
fn spawn_menu(ctx: &mut Ctx) {
    let ui = &mut ctx.ui;
    let root = ui.root().layout(UiLayout::fill()).id();
    ui.button(root, "Play").layout(UiLayout::centered(150.0, 65.0)).name("play");
    ui.push_modeless(root);
}

fn menu(ctx: &mut Ctx) {
    let play = ctx.ui.find("play");
    if ctx.ui.events().filter_map(UiEvent::clicked).any(|id| Some(id) == play) {
        ctx.set_state(Screen::Playing);
    }
}
```

</div>
</div>

Ponko's UI sends events that a system reads; there are no observers on
nodes. Buttons come styled by the [theme](camera-ui.md#one-look-for-both-styles),
and Tab, arrows and a gamepad move between them.

### 15. Test it without a window

<div class="lang-pair from-bevy">
<div>

```rs
{{#include ../../bench/compare/tests/from_bevy.rs:headless}}
```

</div>
<div>

```rust
# struct Score(u32);
# fn add_point(ctx: &mut Ctx) { ctx.world.resource_mut::<Score>().0 += 1; }
#[test]
fn score_counts_up_headless() {
    let world = Game::new("test", 800, 600)
        .sim_system(add_point)
        .setup(|ctx| ctx.world.insert_resource(Score(0)))
        .run_headless(2);
    assert_eq!(world.resource::<Score>().0, 2);
}
```

</div>
</div>

`run_headless(ticks)` runs the fixed ticks and returns the world. Frame
systems do not run headless, which is why game logic belongs in
`sim_system`. Menus can be tested with
[headless frames](headless.md#menus-and-the-ui-headless-frames).

## What is different on purpose

- **One context instead of system parameters.** A Ponko system is a plain
  `fn(&mut Ctx)`: no parameter types to learn, and the same shape in C and
  C++ ([C and C++](c-cpp.md)). The cost is that the engine cannot see from
  the signature what a system touches; a system that wants to run in
  parallel [declares it](system-access.md).
- **Order is the order you write.** Systems run one at a time, in
  registration order inside each phase, so two runs of the same game do the
  same thing in the same order. This is what [lockstep and
  replays](determinism.md) build on. Bevy's
  [`nondeterministic_system_order`](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/ecs/nondeterministic_system_order.rs)
  example shows the other choice.
- **Game logic runs at a fixed rate.** `sim_system` is the default home for
  rules, so a game behaves the same at any frame rate and in tests.
- **Screen-style 2D coordinates.** +y is down and the default camera puts
  (0, 0) at the top-left, as in most 2D tools and image editors. Rotation is
  clockwise on screen.
- **Loads finish in the call.** Small games need no "is it loaded yet"
  state. Bigger ones use [asset groups](loading.md) or
  [streaming](streaming.md), which are asynchronous.
- **The camera, the UI and sound are services, not entities.** `ctx.camera`,
  `ctx.ui` and `ctx.audio` always exist.
- **Fewer, built-in pieces.** Physics (rapier), a retained UI, an editor,
  an inspector and an MCP server for coding agents ship with the engine
  instead of as separate crates.

## What is missing or slower

Measured against Bevy 0.20.1
([the scoreboard](performance.md#ponko-vs-bevy-the-scoreboard)), Ponko is
faster on every run-time row of both tables on an Apple M4 Max, and on every
row but one on a Windows desktop:

- **Skinned characters are slower on Vulkan.** On the Windows desktop with
  Vulkan, 1,000 characters of 16 joints with a sun shadow take 8.72 ms a
  frame in Ponko and 8.35 ms in Bevy. On DirectX 12 (9.65 against 15.5 ms)
  and on the M4 Max (2.38 against 3.83 ms) Ponko is faster.
- **No pipelined rendering.** Bevy simulates the next frame while it draws
  this one; a Ponko frame is the simulation and then the drawing. It is the
  main reason Bevy's GPU rows improve between the two tables
  ([Both engines tuned](performance.md#both-engines-tuned)).

Features Bevy 0.20.1 has and Ponko does not:

- **Observers and entity events** (`On<E>`, `commands.trigger`, event
  propagation up the hierarchy). Ponko has [component hooks](ecs.md#component-hooks)
  and buffered [events](events.md).
- **Generic relationships.** Bevy lets any component pair be a relationship
  (`ChildOf`/`Children` is one); Ponko has one parent link, the
  [hierarchy](hierarchy.md).
- **Automatic parallel scheduling and ordering constraints.** No
  `before`/`after` graph and no automatic parallelism; Ponko's
  [parallel systems](parallel-systems.md) are opt-in and batch neighbouring
  view systems only.
- **Custom schedules, sub-states and computed states.** Ponko has a fixed
  list of phases and plain state machines with a stack.
- **A `Timer` type, `Local` system parameters, `Single` and one-shot
  systems** (`run_system`). The Ponko equivalents are above: intervals,
  closures that own state, `iter().next()` and plain function calls.
- **Sparse-set component storage.** Ponko stores every component in
  archetype tables.
- **A render graph.** Ponko has [named stages for custom GPU
  work](3d.md#custom-passes), not a graph of replaceable passes.
- **Scenes as code** (`bsn!`). Ponko scenes are JSON files.
- **Bevy's plugin ecosystem.** Bevy's
  [assets page](https://bevy.org/assets/) lists the community's crates;
  Ponko's are in the [companion plugins repository](plugins.md#the-companion-plugins-repository).

## A porting checklist

1. **Make the project**: `ponko new my_game` (or `--3d`), then move code
   over one plugin at a time.
2. **Plugins**: each Bevy `Plugin` becomes a Ponko `Plugin`; its
   `Startup` systems go in `setup`, its systems in `build`.
3. **Schedules**: `FixedUpdate` becomes `sim_system`; `Update` game logic
   also becomes `sim_system` (so tests run it), and only input, camera and
   UI work stays in `system`. `PostUpdate` camera work goes in `Phase::Late`,
   drawing in `Phase::Draw`.
4. **Ordering**: list systems in the order they must run; drop `chain`,
   `before` and `after`. Use `Game::order` where two plugins must be ordered.
5. **Parameters**: replace each parameter with the `ctx` field it reads:
   `Query` with `ctx.world.query`, `Res`/`ResMut` with
   `ctx.world.resource`/`resource_mut`, `Commands` with `ctx.commands`,
   messages with `ctx.events`, `Local` with a closure's captured state.
6. **Components and resources**: keep `#[derive(Component)]`; drop
   `#[derive(Resource)]` and `#[derive(Message)]`.
7. **Coordinates**: flip y (`translation.y` up becomes `position.y`
   down), or call `ctx.camera.look_at(Vec2::ZERO)` and negate y where it
   matters. Drop the z of 2D transforms; put draw order in `Sprite::z`.
8. **Cameras, UI and sound**: delete camera entities, use `ctx.camera`,
   rebuild menus with `ctx.ui` and play sounds through `ctx.audio`.
9. **Assets**: `asset_server.load(path)` becomes `ctx.gfx.load_texture(path)`
   (or `ctx.audio.load`, `ctx.gfx.load_mesh` in 3D); delete
   "wait until loaded" states or use [asset groups](loading.md).
10. **Observers**: turn component observers into [hooks](ecs.md#component-hooks)
    and entity events into [events](events.md) read by a system.
11. **States**: derive `Serialize, Deserialize` instead of `States`;
    `NextState::set` becomes `ctx.set_state`.
12. **Tests**: replace `App::update` loops with `run_headless(ticks)`
    ([Headless runs and testing](headless.md)).

## Worked example: `move_sprite`

Bevy's [`examples/2d/move_sprite.rs`](https://github.com/bevyengine/bevy/blob/v0.20.1/examples/2d/move_sprite.rs)
(Bevy 0.20.1) slides one sprite left and right. Its source, unchanged (a
copy in `bench/compare/examples` keeps it compiling against the pinned Bevy):

```rs
{{#include ../../bench/compare/examples/move_sprite.rs:original}}
```

The port is `examples/move_sprite` (`cargo run --example move_sprite`). Its
rules are in `game.rs`, so `tests/move_sprite.rs` can play them headlessly:

```rust
{{#include ../../examples/move_sprite/game.rs:port}}
```

and `main.rs` only runs the game:

```rust
{{#include ../../examples/move_sprite/main.rs:main}}
```

What changed, line by line:

| Bevy | Ponko | Why |
|---|---|---|
| `App::new().add_plugins(DefaultPlugins)` | `Game::new("Ponko: move_sprite", 800, 600)` | A game has a window by default. |
| `.add_systems(Startup, setup)` | `.setup(setup)` | |
| `.add_systems(Update, sprite_movement)` | `.sim_system(sprite_movement)` | Fixed rate, so the headless test runs it. |
| `commands.spawn(Camera2d)` | `ctx.camera.look_at(Vec2::ZERO)` | No camera entity; this puts (0, 0) in the middle as Bevy's camera does. |
| `Sprite::from_image(asset_server.load(...))`, `Transform::from_xyz(0., 0., 0.)` | `Sprite::new(ctx.gfx.load_texture(...))`, `Transform::at(Vec2::ZERO)` | Loaded in the call; a 2D transform. |
| `time: Res<Time>`, `Query<(&mut Direction, &mut Transform)>` | `ctx.time.dt()`, `ctx.world.query::<(&mut Direction, &mut Transform)>()` | Parameters become `ctx` fields. |
| `for (mut logo, mut transform) in &mut sprite_position` | `.for_each(\|(logo, transform)\| ...)` | No `Mut` wrappers: plain `&mut`. |
| `transform.translation.x` | `transform.position.x` | The motion is along x, so y's direction doesn't matter here. |

The three tests in `tests/move_sprite.rs` check that the sprite moves
150 units in one second, turns past both ends and stays within one step of
them over ten seconds, and that the system moves only entities with a
`Direction`.
