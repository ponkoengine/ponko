# The simulation clock

Simulation systems run on their own clock, separate from the frame rate. The
game can pause, step and fast-forward the sim while rendering and input keep
running normally.

<div class="lang-pair">
<div>

```rust
# fn ants(_: &mut Ctx) {}
fn controls(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Space) {
        if ctx.time.is_paused() { ctx.time.resume() } else { ctx.time.pause() }
    }
    if ctx.input.key_pressed(Key::Period) {
        ctx.time.step();              // one tick while paused
    }
    if ctx.input.key_pressed(Key::Num4) {
        ctx.time.set_speed(64.0); // fast-forward
    }
}

Game::new("Colony", 1280, 720).sim_system(ants).system(controls);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:controls}}
```

</div>
</div>

Put controls in a frame `system`: frame systems keep running while the sim is
paused.

[Headless runs](headless.md#what-runs) ignore `ctx.time.pause()` (unless
the [control endpoint](control.md) is on, which waits for tools): every tick
runs, so a test can play through a game that pauses behind its menus. To
hold the world in a test too, hold only physics, as below.

### Pausing only physics

`ctx.time.pause()` stops every sim system, including the one that would
read the "resume" press. A pause screen that keeps its rules in sim systems
holds only the physics step instead, with `ctx.physics.set_paused(true)`:

<div class="lang-pair">
<div>

```rust
fn pause_menu(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Escape) {
        let held = ctx.physics.is_paused();
        ctx.physics.set_paused(!held); // the step only
    }
}

Game::new("Platformer", 1280, 720)
    .physics(vec2(0.0, 980.0))
    .sim_system(pause_menu);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics_pause}}
```

</div>
</div>

While it is held, every other sim system runs and the tick counter goes on.
Bodies stay where they are and keep their velocity, no collision events
arrive, and ray and overlap queries see the last step's world. Bodies
spawned, moved or despawned meanwhile join at the first step after the
pause. Sim systems run after the step by default, so a pause set in one
holds the next tick's step; put the system in `Phase::BeforePhysics` to hold
this tick's. The flag lives on `ctx.physics`, not in the world: a save
doesn't keep it. The [Ember Hop](ember-hop.md#physics) pause screen works
this way.

### Pausing only 3D physics

3D physics has the same switch on its own world, `ctx.physics3d`:

<div class="lang-pair">
<div>

```rust
fn pause_menu(ctx: &mut Ctx) {
    if ctx.input.key_pressed(Key::Escape) {
        let held = ctx.physics3d.is_paused();
        ctx.physics3d.set_paused(!held); // the 3D step only
    }
}

Game::new("Explorer", 1280, 720)
    .physics3d(vec3(0.0, -9.81, 0.0))
    .sim_system(pause_menu);
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:physics3d_pause}}
```

</div>
</div>

It holds the same things as the 2D one, and a few more that only 3D has:
bodies keep their pose and velocity, a
[character](physics3d.md#a-walking-character) keeps its `velocity` but
doesn't move (a jump goes on where it stopped), vehicles, joints and
ragdolls hold, no collision events arrive and no
[contact hook](physics3d.md#contact-modification) runs. In a window, bodies
are drawn at their last pose instead of being blended between two steps.
Headless runs honour it, and as in 2D a save taken while paused holds the
paused poses and velocities but not the flag, and loading one doesn't clear
it. The [Shard Grove](shard-grove.md#the-warden-and-the-crawlers) menus work
this way.

Why one call per world and not one call for both: the two physics worlds
are separate services with their own steps (`step_physics` and
`step_physics3d`), and a game that has both may want to hold just one of
them (a 2D minimap game inside a 3D world, say). Each world's flag sits
next to its gravity and its queries, so `ctx.physics3d.set_paused` reads
like `ctx.physics.set_paused`, and C and C++ follow the same pattern
(`ponko_ctx_set_physics3d_paused`, `ctx.set_physics3d_paused`). Holding
both is two calls.

## What the clock guarantees

- **Ticks are always the same length.** `ctx.time.dt()` in a sim system is the
  fixed step (`1 / fixed_hz`), at every speed. Speed only changes how many
  ticks run per frame, so a run at 64× gives the same result as one at 1×.
- **The game stays responsive.** At most 64 ticks run per frame. If the sim
  can't keep up, it runs as fast as it can instead of freezing the window.
- `ctx.time.sim_tick()` counts ticks; `ctx.time.sim_time()` is simulated seconds as an
  `f64` (no drift after hours of simulated time).

## Doing work less often

Not everything needs to run 60 times a second. There are two tools, and they
combine.

**A whole system at a lower rate:**

<div class="lang-pair">
<div>

```rust
# fn movement(_: &mut Ctx) {} fn plan_routes(_: &mut Ctx) {} fn grow_food(_: &mut Ctx) {}
Game::new("Colony", 1280, 720)
    .sim_system(movement)                             // every tick
    .add(Phase::AfterPhysics, plan_routes.every(10))  // 6 times a second
    .add(Phase::AfterPhysics, grow_food.every(10))    // also 6/s, but on other ticks
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:every}}
```

</div>
</div>

Inside `plan_routes`, `ctx.time.dt()` is 10 ticks' worth of time, so rates stay
right. Systems with the same interval are spread over different ticks, so
their cost doesn't pile up on one frame.

**Entities taking turns:** each tick, only 1/n of the entities do the
expensive part.

<div class="lang-pair">
<div>

```rust
# #[derive(Component)]
# struct Brain;
# impl Brain { fn decide(&mut self, _dt: f32) {} }
fn think(ctx: &mut Ctx) {
    let turn = ctx.time.turns(8); // each ant thinks every 8th tick
    ctx.world.query::<(Entity, &mut Brain)>().par_for_each(|(e, brain)| {
        if turn.includes(e) {
            brain.decide(turn.dt()); // time since this ant's last turn
        }
    });
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:turns}}
```

</div>
</div>

This keeps the per-tick cost flat instead of spiking every 8th tick, and
works in `par_for_each` (C++: `each_par`).

## Smooth motion between ticks

If the sim rate is lower than the frame rate, `ctx.time.fixed_alpha()` (0..1)
tells a frame system how far it is between the last tick and the next, for
interpolating positions when drawing.
