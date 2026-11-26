# The simulation clock

> **Maturity: [proven](maturity.md).** Game: Coin Dash and Ember Hop, packaged
> and played on Linux, macOS and Windows (fixed-step rules). Tests:
> `tests/long_session_time.rs`, `tests/step_inputs.rs`,
> `tests/determinism.rs`. Platforms: Linux, macOS and Windows, headless (the
> workspace tests in the list of checks). Graphics: not involved.

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

## Very long sessions

A game left running for hours (a server, a kiosk, an idle game) keeps its
clocks exact and its memory flat. The engine's tests hold it to this: ten
simulated hours headless, ten hours of 144 Hz frames on a 60 Hz clock
(`tests/long_session_time.rs`, `src/time.rs`), 200 000 ticks of a game that
makes and frees everything it can (`tests/long_run.rs`) and 2000 drawn
frames that do the same on the GPU.

**The clocks.**

- `sim_tick()` is a `u64` and `sim_time()` is `sim_tick() * fixed_dt` in
  `f64`: after ten hours it is exact to the nanosecond, after a year to a
  few nanoseconds. Nothing is added up, so nothing drifts.
- `fixed_alpha()` comes from what is left over after the last tick, never
  more than one step, so interpolation is as precise after a week as in the
  first second. No step is ever lost: rounding can hold one back to the next
  frame (`fixed_alpha()` then reads 1.0).
- `elapsed()` adds up the frames' times in `f64`: within microseconds of the
  real time after ten hours.
- `dt()`, `sim_dt()` and `fixed_dt()` are per-step values and don't grow.

**What loses precision: absolute time in an `f32`.** An `f32` holding
seconds since the start steps in 3.9 ms after ten hours, 7.8 ms after a
day, 31 ms after three days. That is what you get from
`ctx.time.elapsed() as f32` in your own code, and what 3D material shaders
see as their legacy time (`frame.time.x`). For periodic Rust code, wrap
the clock before converting it with `(ctx.time.elapsed() % period) as f32`.
In 3D WGSL, use `ponko_elapsed_time_wrapped(4u)` for a four-second cycle.
It reduces the whole seconds before adding the fractional second, so
the phase still advances smoothly after days. The helper also uses the
last drawn frame's time when the motion pass evaluates `vertex` for that
frame. For other period math, `frame.time_seconds.x` is the whole elapsed
seconds as a `u32`, and `frame.time_fraction.x` is the subsecond part as
an `f32`. Their `.y` components hold the same pair for the last drawn
frame. Keep the parts separate until you have reduced the whole seconds
to a small value.

A 3D `Animator` and a 2D `Skeleton` keep their unwrapped normalized
time (loops so far plus the fraction of this one) in `f64`, so a state
looping for days poses, fires events and moves its root on schedule.
Use `normalized_time_f64()` in Rust. In C, use
`ponko_ctx_animator_normalized_time_f64` or
`ponko_ctx_skeleton2d_normalized_time_f64` with a writable `double`.
The C++ `Ctx` wrapper exposes these as
`animator_normalized_time_f64(entity)` and
`skeleton2d_normalized_time_f64(entity)`. The original float accessors
and state calls still return the rounded value.

**Memory.** Entity ids, UI nodes, textures (render targets included),
sounds, fonts, meshes, GPU buffers, collision meshes, instance cells,
3D materials, material shaders, compute passes, screen effects and
texture arrays reuse freed slots under new handles. A slot is given out
4095 times and then retired. A handle kept past its `free` stays dead
even after its slot is reused. Draws still using a freed material use
the plain fallback material. A material with a freed shader uses the
built-in shader, and one sampling a freed texture or texture array
reads white. Freed compute passes and screen effects are skipped.
Popped UI screens keep their nodes until the game removes them (see
[the screen stack](ui.md#screen-stack-and-modal-dialogs)). To check your own game, count with
[`perf::net_alloc_bytes`](profiling.md#count-allocations) across a stretch
of play and compare `ctx.live_counts()` before and after.

### Reference-game soaks

From the checkout, run the four reference workloads sequentially in release
builds. The default is offscreen with GPU drawing and a 48 kHz memory-backed
audio mixer. It uses the same game processes from start to finish and keeps
their saves in temporary folders.

```sh
python3 scripts/soak.py --seconds 600 --sample 20 --warmup 120 --out target/soak-short
```

Set `PONKO_SOAK=1` to enable the slow `soak` job in
`scripts/run-checks.py --job soak`. The sampler defaults to 60 seconds.
Use `--game` to select examples for a shorter diagnostic sweep and
`--headless` to diagnose without drawing. The release checklist uses
`--windowed` on macOS, Windows and Linux for four hours per game. In that
mode the runner requests actual window resizes and timed minimize and
restore. In offscreen mode it drives resize, zero drawable size, suspend
and resume through the renderer and sends the matching window events.
The renderer can still draw offscreen with presentation suspended.

Coin Dash loops through settings and rounds, alternating a timed-out round
with the coin-chasing route, and reads the saved progress back at the title.
Ember Hop plays both caves, invokes the game's death path once per run and
takes the pause, quit and continue route. Shard Grove collects shards, saves
at a shrine, leaves for the title, continues, exhausts the warden's health,
retries and finishes at the gate. Streaming steers and changes altitude
while crossing cells, uploading terrain tiles and evicting old ones. It has
no save or death mechanics. A passing run reaches its requested wall time
and completes at least two workload cycles.

Each game's folder holds `samples.csv`, `report.json` and `game.log`. Frame
percentiles use the completed frames in each interval, including blocking GPU
wait or presentation and, offscreen, the offline audio mix. Timing excludes
the sampler and its pacing sleep. Allocations per frame come from Rust's
counting allocator across all threads. RSS is the resident set on macOS and
Linux and the working set on Windows. The renderer reports its own live GPU
bytes plus the adapter figure when available. A backend that reports no
adapter figure gets a null reading and an explicit unavailable status.
Headless diagnostics also omit the GPU inventory. Handles are grouped by
kind, and real and virtual voices are measured separately. Each sample also
records the machine's one-minute load average (macOS and Linux; empty on
Windows). It is not judged: it shows when a change in frame time came from
other programs. Frame time follows that load, which is why its noise
allowance grows with the frame time itself; a leak shows first in memory,
handles and allocations.

For each quantity, the report fits a least-squares line through the samples
at or after warm-up. The upper bound for its slope is the hourly allowance
plus the noise allowance divided by the observed post-warm-up hours. This
gives short checks room for the same one-time fluctuations as a long run.
At least three post-warm-up readings are needed for each available metric.
The sample interval must be a multiple of 20 seconds, the runner's resize and
minimize cycle, so that every sample holds whole cycles: a cycle allocates
(in Shard Grove about 450,000 times), and at 10-second samples the readings alternated between frames with and without one (about
830 and 1,900 allocations per frame in Shard Grove). Keep at least four
workload cycles after warm-up. Shorter runs are diagnostics: the phase of
each game's loop then shows up as a slope.

| Quantity | Hourly growth allowance | Noise allowance |
|---|---:|---:|
| RSS | 16 MiB/h | 24 MiB |
| Renderer GPU bytes | 8 MiB/h | 16 MiB |
| Adapter GPU bytes | 8 MiB/h | 32 MiB |
| Live handles, each kind | 1/h | 2 (80 for the streaming example's tile meshes) |
| Allocations per frame | 2/h | 4 |
| Real and virtual voices, each | 1/h | 4 |
| Frame p50 | 0.5 ms/h | 2 ms, or half its post-warm-up median if larger |
| Frame p95 | 1 ms/h | 5 ms, or half its post-warm-up median if larger |
| Frame p99 | 2 ms/h | 10 ms, or half its post-warm-up median if larger |

A slope above its bound fails the command and writes a graph of that metric
beside the report. To judge saved samples again (after a change to the
bounds, say), pass the same options plus `--evaluate`; nothing runs. Include the graph, measured slope and bound, game,
revision, adapter, operating system and command when filing the growth.
