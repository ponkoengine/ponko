# Performance and benchmarks

> **Maturity: not a feature.** Benchmarks and how they were measured; the
> features measured carry their own levels.

Numbers from a MacBook Pro (M4 Max), release builds. Reproduce them yourself;
every number here has a command next to it.

The ECS, simulation and example numbers in this chapter were measured again
on 2026-11-20 on an idle machine, at revision `e60e15a8`, one benchmark at a
time. A few rows still carry the date or the conditions they were measured
under (the Unity comparison, the sprite renders, the before-and-after tables
of a change, the C++ row); each says so. They are specific workload results,
not current-version guarantees or proof of overall engine superiority. See
[the engine comparison](comparison.md) and baseline issues #132 and #134 for
measurement scope.

## ECS iteration

`cargo run --release -p ponko_bench` (Ponko and hecs); the bevy_ecs column
comes from `cd bench/compare && cargo run --release -- ecs`, which runs all
three (see [the scoreboard](#ponko-vs-bevy-the-scoreboard)).

| Benchmark (1M entities) | Ponko | bevy_ecs | hecs |
|---|---|---|---|
| iterate `(&mut Pos, &Vel)` | 0.16 ms | 0.36 ms | 0.70 ms |

Medians of three runs on an idle machine on 2026-11-22 (revision
`9235ea05`, 1-minute load 2.2 to 2.3; Ponko 0.151 to 0.164 ms, bevy_ecs
0.362, hecs 0.697 to 0.699). hecs here is its plain `for` loop; with the
`fold` fast path the ECS matrix below measures 0.18 ms (Ponko 0.17,
bevy_ecs 0.37).

`par_for_each` on a heavy per-entity workload, 10M entities: 8.1 ms on all
cores vs 57.6 ms on one (measured 2026-11-02, not run again). The steering
workload of the ECS matrix below, at 1M entities, measured again on
2026-11-22: 0.56 ms on all cores against 4.9 ms on one.

The full ECS matrix (1k/100k/1M entities: iteration, parallel scaling,
spawning, spawn/despawn and add/remove churn, optional and filtered queries,
and a game-like tick, each against bevy_ecs and hecs, with allocation counts)
records machine-readable results with one command:

```sh
PONKO_BENCH_JSON=crates/ponko_bench/baselines/ecs.json \
  cargo run --release -p ponko_bench -- 'ecs/|archetypes-1024|query-build'
```

`crates/ponko_bench/ECS.md` has the methodology, the limits of each
comparison and the committed baselines. `cargo test -p ponko_bench` checks
that every workload does the same work in all three libraries; timings are
never checked in tests.

### What ECS features cost

ECS features cost nothing where they aren't used: spawns, despawns and
queries in a world without required components, hooks or change detection
cost no more than before those features existed. Where they are used,
these costs remain:

| Feature | Cost where it is used |
|---|---|
| A [required component](ecs.md#required-components) the bundle doesn't give | Its constructor runs, on a path of its own: a one-by-one spawn costs about 1.2× one that names the component itself, about 40 instructions or 2 ns more. A batch costs the same as naming it, spawn-and-despawn churn about 1.03×. |
| [Change detection](ecs.md#change-detection), despawning | A despawn from an archetype with tracked components also logs each one's removal and moves its stamps: with one or two tracked components it runs fewer instructions than a despawn did before change detection kept its per-block summary (0.93× with two). |
| Change detection, inserting and removing | The entity keeps its stamps, and its new block's summary takes the newer of its own and the entity's: a move costs about 1.5% more instructions, and a `Changed` loop afterwards reads only blocks where something changed. |
| `Option<&T>` in a query | Nothing per entity in `for_each` and the parallel loops with one to four of them: the loop is chosen per archetype. With five or more, each entity tests whether its archetype has the component. |
| Many archetypes | Matching a query costs about 40 instructions per archetype it tests, filters included: 2.2–2.4 ms for 1000 builds and counts of a query over 1024 archetypes. Worlds with 64 or more archetypes index repeated queries that leave archetypes out. An `Added`/`Changed` filter also reads each matching archetype's block summaries: a `Changed` loop that finds nothing costs about 2.4× the instructions of a plain loop per archetype (about 380 against 160) (`archetypes-1024-changed-none`). |

Numbers are from an Apple M4 Max, release builds; `crates/ponko_bench/REQUIRED.md`
and `crates/ponko_bench/ECS.md` have the runs. Measure your own with
`cargo run --release -p ponko_bench -- 'ecs/spawn|ecs/optional|changes/ponko-despawn|archetypes-1024'`.

## Headless simulation

`AGENTS=1000000 cargo run --release --example headless`

| Agents | ms per tick |
|---|---|
| 1M | 0.91 |
| 10M | 7.2 |

## Boids (neighbor search)

`HEADLESS=1 BOIDS=<n> cargo run --release --example boids`

| Boids | ms per tick |
|---|---|
| 20k | 0.93 |
| 100k | 3.7 |
| 1M | ~33 |

## Ant colonies (whole game loop)

`HEADLESS=1 ANTS=<n> cargo run --release --example ants`: steering with
pheromone sensing, trail laying, foraging, colony growth.

| Ants | ms per tick |
|---|---|
| 5k | 0.21 |
| 50k | 0.93 |
| 1M | 15.6 |

## Flow fields (pathfinding)

`cargo run --release -p ponko_bench -- flow` for the field itself;
`HEADLESS=1 AGENTS=<n> cargo run --release --example flow` for units
following it around walls and mud, with a new goal every 2 seconds.

| What | Time |
|---|---|
| `compute`, 256 × 256 cells | 0.88 ms |
| `compute`, 1024 × 1024 cells | 13.8 ms |
| 1M `direction_at` lookups, parallel | 0.32 ms |
| 20k units, per tick | 0.11 ms |
| 1M units, per tick | 1.7 ms |

## Battle (the flagship demo)

`HEADLESS=1 UNITS=<n> TICKS=1200 cargo run --release --example battle`:
two armies march along flow fields, keep their spacing, find enemies with
per-team spatial grids, fight, and rebuild from their bases. The map grows
with the army, so the crowd density stays the same.

| Units | Map (cells) | ms per tick |
|---|---|---|
| 20k | 400 × 225 | 0.60 |
| 200k | 1264 × 711 | 4.0 |
| 1M | 2828 × 1590 | 19.3 |

With a window, 20k units run at the display's 120 FPS with 1.2 ms of
simulation per frame.

The simulation showcase, [Legions](legions.md), adds archers, arrows and
replays that end with the same world hash; its chapter has its numbers at
10,000, 100,000 and 250,000 units.

How it stays fast, in case your game needs the same tricks:
- Marching units look for enemies only on their turn (`ctx.time.turns(4)`),
  units in a fight every tick.
- Separation uses a fine grid of friends, every other tick; enemy search
  a coarse grid of enemies, at melee reach first and full sight only if
  nothing is in reach.
- The four grids are rebuilt at the same time (`rayon::scope`).
- Once a second, units are sorted by `spatial_key`, so neighbors on the
  map are neighbors in memory.

## Events and attachments

`cargo run --release -p ponko_bench -- events` and `-- attach`:

| What | Per tick |
|---|---|
| 100k [events](events.md) sent, read by another system and dropped | 0.44 ms (4.4 ns per event) |
| 100k [attachments](attachments.md) following 100k moving entities | 1.0 ms |

Both allocate nothing once running: events reuse their buffers, and
attachments are placed in parallel into scratch kept between ticks.

## Rendering a million sprites

`CRABS=1000000 SCALE=<s> cargo run --release --example bunnymark_ecs`:
1M bouncing crabs, drawn by the built-in `draw_sprites`. The example prints
the render stats described below.

| Crab size on screen | FPS | render CPU | GPU |
|---|---|---|---|
| 1.6 px (`SCALE=0.05`) | 120 (display cap) | 3.3 ms | 2.4 ms |
| 8 px (`SCALE=0.25`) | 99 | 3.3 ms | 8.6 ms |
| 32 px (`SCALE=1`) | 15 | 2.8 ms | 71 ms |

The 32 px row is limited by the GPU filling pixels: on a Retina screen a
million 32 px crabs are about 4 billion blended pixels per frame. Fewer or
smaller sprites on screen is the only fix for that; the cost per sprite is
small (about 1.6 ms of GPU time per million).

What keeps the CPU side small:
- `draw_sprites` builds the instance data on all cores (1M sprites: 1.1 ms,
  was 3.4 ms on one core).
- Each sprite is 36 bytes on the GPU (was 52): the per-frame upload for 1M
  sprites is 36 MB. (A 28-byte version without a texture region measured
  the same render CPU time; the region is needed for text and atlases.)

### Is it the CPU or the GPU?

`ctx.perf.render_stats()` tells you what drawing the last frame cost:

```rust
# use ponko::prelude::*;
fn report(ctx: &mut Ctx) {
    let r = ctx.perf.render_stats();
    println!(
        "{} sprites in {} draw calls: {:.2} ms CPU, GPU {:?} ms",
        r.sprites, r.draw_calls, r.cpu_ms, r.gpu_ms
    );
}
```

`gpu_ms` is measured on the GPU with timestamp queries, so the frame rate
the window shows (which the compositor can cap or smooth) can't fool it.
If `gpu_ms` is close to the frame time, the GPU is the limit: draw fewer
or smaller sprites. If `cpu_ms` plus your systems' time
(`ctx.perf.system_timings()`) is close to it, the CPU is. Agents
get the same numbers from the control endpoint's `info` method.

## Profiling a frame

(The [profiling chapter](profiling.md) has the rest: your own timing scopes, counting allocations, headless performance tests with budgets, and what to do about what you find.)

A frame takes about as long as the slower of the CPU (systems, then
recording draws) and the GPU. The profiler shows which, and where the time
goes, in a running game:

- **The overlay** (`ctx.perf.set_hud(true)`, or `PONKO_HUD=1`): fps,
  frame-time percentiles, CPU and GPU time, and the slowest GPU passes.
- **`ctx.perf.frame_stats()`**: the same numbers over the last few seconds:
  fps, average, p50, p95, p99 and worst frame time, CPU time in systems,
  in rendering and waiting on the display, and GPU time. Display wait
  includes pacing; use measured GPU/CPU work to identify the bottleneck.
- **`ctx.perf.gpu_passes()`**: GPU time per pass (each shadow cascade, depth
  prepass, opaque, transparent, screen effects, bloom, post), measured
  with timestamp queries. On GPUs that draw in tiles, passes overlap, so
  each is counted from the end of the one before; a pass the GPU runs
  entirely alongside others (a compute pass next to a render pass it
  doesn't depend on) shares that time with them evenly, so the passes add
  up to no more than the frame's GPU time. A pass that ran but whose times
  didn't come back is listed by `ctx.perf.gpu_passes_not_timed()` and
  marked "not timed" in the overlay, the report and traces, never shown as
  0 ms (see [Which passes can be timed](#which-passes-can-be-timed)).
  When the latest frame has no usable GPU time at all,
  `ctx.perf.gpu_untimed()` says why (see [No GPU time](#no-gpu-time)).
  Timing a pass costs the graphics library a few heap allocations, so a
  game times one frame in 16, and every frame while something reads every
  frame: profiling, the overlay, a trace, a `PONKO_BENCH` run or automatic
  quality. The smoothed times weigh each sample by the frames it stands
  for, so they settle at the same values either way (see
  [Cost](viewports.md#cost) for what it saves). A game can pick how often
  itself: [how often GPU passes are timed](#how-often-gpu-passes-are-timed).
- **`ctx.perf.render_stats()`**: draw calls; 3D models and triangles drawn for
  the camera and, separately, for the shadow maps; and the render CPU
  time split into culling (`collect_ms`), recording (`record_ms`, all
  cores) and submitting. `uploads` counts the bytes sent to the GPU that
  frame, by kind (model transforms, uniforms, lights, ...), the buffers
  created, and the `reallocations` (buffers and textures replaced because
  they were outgrown; the new buffers also count as created): in a scene where nothing changes it should stay at the
  clock's few bytes and no buffers. Models that stand still cost no
  uploads, also while the camera moves; only the models that changed, or
  entered or left a view, are sent (see
  [Models between frames](#models-between-frames) and
  [What a frame sends the GPU](#what-a-frame-sends-the-gpu)).
- **`ctx.gfx.mesh_costs()`**: per mesh, how many models and triangles it drew
  for the camera and the shadows last frame, heaviest first.
- **Traces**: `ctx.perf.record_trace(path, frames)` (or `PONKO_TRACE=path`)
  writes the next frames as a Chrome trace: every system, rendering,
  display waits and the GPU passes on a timeline. Open it in
  [Perfetto](https://ui.perfetto.dev) to see what's in a hitch.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/profiling.rs:profiling}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:profiling}}
```

</div>
</div>

### How often GPU passes are timed

`ctx.perf.set_gpu_timing` picks the cadence, separately from profiling
(which also times scopes and counts allocations):

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
use ponko::perf::GpuTiming;

fn gpu_times(ctx: &mut Ctx) {
    // Fresh `gpu_passes()` every frame, with profiling still off.
    ctx.perf.set_gpu_timing(GpuTiming::Every(1));
    // Or one frame in four, or none at all:
    // ctx.perf.set_gpu_timing(GpuTiming::Every(4));
    // ctx.perf.set_gpu_timing(GpuTiming::Off);
    // `GpuTiming::Auto` is the default: one frame in 16, every frame
    // while profiling is on.
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:gpu_timing}}
```

</div>
</div>

An explicit choice also wins over profiling: `Every(4)` with profiling on
times one frame in four. The overlay, traces, `PONKO_BENCH` runs and
automatic quality read every frame's times, so they get every frame while
they are on, whatever the setting; the choice applies again when they stop.
`Off` leaves `gpu_passes()` empty and `gpu_ms` `None` (with nothing reading
them). A change takes effect on the next frame. C reads and writes the
choice with `ponko_ctx_set_gpu_timing(ctx, PONKO_GPU_TIMING_EVERY, 4)` and
`ponko_ctx_gpu_timing` (`PONKO_GPU_TIMING_AUTO`, `_EVERY`, `_OFF`; the count
matters only for `_EVERY`); a bad kind is `PONKO_INVALID_ARGUMENT`.

What each choice costs the game's thread in heap allocations per frame
(the offscreen test `allocations_per_frame_fall_as_gpu_timing_gets_rarer`;
a debug test build on an Apple M4 Max, Metal, a 64 x 64 offscreen frame of
one cube, so only the differences carry over to a real scene):

| Setting | Allocations per frame | More than `Off` |
|---|---|---|
| `Every(1)` | 282.0 | 45.0 |
| `Every(4)` | 248.2 | 11.2 |
| `Auto` (one in 16) | 239.8 | 2.8 |
| `Off` | 237.0 | |

The rest is the frame itself. These count the game's thread only: the
graphics library allocates on the threads that record passes too.

### Which passes can be timed

| Backend | Render passes | Compute passes (culling, skinning, GI, reflections) |
|---|---|---|
| Metal (macOS, iOS) | Timed | Timed. A frame's timestamps are read once the GPU has finished it: Metal writes a compute pass's late, and reading them sooner gave the previous frame's, so these passes used to read 0 ms. |
| Vulkan, DirectX 12 | Timed | Timed |
| OpenGL (desktop, with `GL_ARB_timer_query`) | Timed | Timed |
| WebGPU in a browser | Timed where the browser offers timestamp queries; browsers round them, so short passes read coarse times | Same |
| WebGL2, and GPUs without timestamp queries | Not timed: no GPU times at all (`gpu_ms` is `None`) | Not timed |

On any backend, a pass with nothing to do (no draws, no dispatches) may
write no timestamps: Metal skips such a pass. It is then listed as not
timed for that frame. The frame's GPU time still covers everything that
ran. The engine's offscreen GPU test of a compute pass
(`a_compute_pass_with_real_work_gets_a_time_within_the_frame` in
`src/render/timer.rs`) has run on Metal; the other rows follow from what
the graphics library supports there and are checked when that test runs on
those machines.

### How many passes a frame can time

The timer has room for 32 passes a frame at first, and makes room for the
passes a frame actually has: when a timed frame had more, the timer makes
bigger query sets before the next frame (with a quarter to spare, in steps of
16 passes), so a frame with several views, such as two eyes, keeps the timing
of every pass. The most it times is **336 passes a frame**: the six frames in
flight share one query set, and an API's query set holds at most 4,096
queries. The few frames in flight while it grows are not timed.

A pass beyond that is drawn untimed, and the frame says so instead of
dropping it quietly: `ctx.perf.gpu_passes_over_limit()` is how many passes of
the latest timed frame had no room (0 for any frame under the limit), the
overlay's pass list starts with "N more passes not timed: the timer has room
for 336 a frame", the HUD and `ctx.perf.report()` carry the same line, and
the control endpoint's `perf.get` has it as `gpu_passes_over_limit`. This is
different from [a pass that ran but wasn't timed](#which-passes-can-be-timed),
which is listed by name. On Metal, growing takes a second query set from the
[budget](#how-many-devices-can-be-timed) until the device lets go of the
first; if the budget has none, the timer stays at its room and counts what it
couldn't time. `two_views_keep_every_gpu_pass_timing_past_the_first_32` in
`src/render/stereo_render_test.rs` is the offscreen test with two views.

C and C++ don't read the pass list (`gpu_passes`), so they have no
counterpart for the count either: a native game sees the whole-frame time
(`ponko_ctx_gpu_timing`, `ponko_ctx_gpu_untimed`) and the control endpoint.

### How many devices can be timed

Each timestamp query set the engine makes counts against a limit on Metal:
a process may hold 32 of them, of any size, shared by all its devices, and
asking for a 33rd loses the device that asked. (Measured on an Apple M4
Max with wgpu 30: sets of 8, 64, 384 and 4,096 queries all fail at the
33rd. Apple's documentation names no number; the ignored test
`metal_allows_32_timestamp_query_sets_per_process_of_any_size` in
`src/render/query_sets.rs` repeats the measurement.) The number of queries
in a set doesn't count, so the engine keeps few sets rather than small
ones.

A device's renderer holds two sets: one for the frame's passes and one for
uploads. On Metal the engine takes at most 28 sets in a process, leaving 4
for a game's own timestamp queries, so up to 14 devices are timed at once
(a game with tool windows, previews or several offscreen renderers). A
device past that keeps working without GPU times, and the engine prints
one line saying so (`ponko: GPU frame timer: GPU timing is off for this
device...`); dropping another device's renderer makes room again
(`ctx.perf.gpu_untimed()` is then `Some(GpuUntimed::QuerySetBudget)`).

A dropped set isn't freed at once: `wgpu` keeps it until its device has
seen the GPU finish the work that used it (at the next poll or
submission), and until then it still counts against Metal's 32. So the
engine counts a timer's set until then too, not until the timer is
dropped. A renderer dropped and made again while its frames are still on
the GPU would otherwise hold more sets than the engine counts: an
offscreen test that drops and makes frame and upload timers 60 times in a
row, on one device and on three, found up to 4 and 6 sets still alive
after their timers were dropped, which the count before left out (with
the engine's 28 taken, that passes the 32 and loses a device). Counted
until freed, no timer was refused and no device lost
(`a_quick_drop_and_create_cycle_never_holds_more_sets_than_it_counts` in
`src/render/query_sets.rs`). When the budget is full, the device asking
for a set is polled once first, which frees its own dropped sets whose
work is done; a game never waits for one.

Other backends have no such limit known, and every device is timed. What
was read (wgpu 30.0.1's source, and each platform's documentation for
making a query set):

| Backend | What a timestamp query set is there | A limit on how many? |
|---|---|---|
| Metal | a counter sample buffer (`MTLCounterSampleBuffer`) | 32 per process, measured; Apple's documentation only says making one fails when the driver has run out of them |
| Vulkan | a query pool (`vkCreateQueryPool`) | none in wgpu or in Vulkan's documentation, which names only running out of memory |
| DirectX 12 | a query heap (`CreateQueryHeap`) | none in wgpu (which refuses a heap that would pass a memory budget, when one is set) or in Direct3D's documentation, which names only running out of memory |
| WebGPU in a browser | the browser's own `createQuerySet`, which wgpu calls as it is | none in the WebGPU API, which limits a set to 4,096 queries, not how many sets there are |

Still needs a machine: a run of the cycle test on Vulkan and DirectX 12
(no count is known there, so the engine doesn't count them, and a limit
would show as a lost device), and in browsers on macOS, whose WebGPU runs
on Metal underneath: a page can't see the browser's own sets, and whether
Metal's 32 per process reach a page's timers there is not known. Vulkan
on a Mac through a translation layer to Metal likewise isn't counted.

### No GPU time

When the latest frame has no usable GPU time (`gpu_ms` is `None`, or the
times shown are from an earlier frame), `ctx.perf.gpu_untimed()` says why
instead of leaving it at "not timed":

| `GpuUntimed` | JSON | What to do |
|---|---|---|
| `NoGpu` | `no_gpu` | A headless run, or the renderer hasn't opened a GPU yet. |
| `NoTimestamps` | `no_timestamps` | The GPU or its backend can't write timestamps (WebGL2, some GPUs): measure CPU time and frame time instead. |
| `QuerySetBudget` | `query_set_budget` | The process already holds every timestamp query set the engine takes ([above](#how-many-devices-can-be-timed)): drop another device's renderer. |
| `Off` | `off` | The game chose `GpuTiming::Off` and nothing else reads GPU times. |
| `NotReady` | `not_ready` | No timed frame's timestamps have come back yet (they arrive a few frames late), or every readback is still in flight. |
| `DeviceLost` | `device_lost` | The GPU device was lost (a driver reset, the GPU removed); nothing is timed from then on. |

The overlay shows it (`gpu not timed: query_set_budget`, or `gpu 2.10
(stale: device_lost)`), the report adds a `GpuNotTimed` warning, the
render-pass inspector's `gpu_timing.untimed` names it, and the editor's GPU
panel says it in words. More reasons may be added, so match with a `_`
arm. C reads it with `ponko_ctx_gpu_untimed(ctx, &why)` (a
`PONKO_GPU_UNTIMED_*` value, `PONKO_GPU_UNTIMED_NONE` when the frame was
timed), C++ with `ctx.gpu_untimed()` (`ponko::gpu_untimed`).

```rust
# use ponko::prelude::*;
use ponko::perf::GpuUntimed;
fn gpu_time(ctx: &mut Ctx) {
    match ctx.perf.gpu_untimed() {
        None => println!("{:?}", ctx.perf.frame_stats().gpu_ms),
        Some(GpuUntimed::NotReady) => {} // in a few frames
        Some(why) => println!("no GPU time: {why}"),
    }
}
# Game::new("t", 1, 1).system(gpu_time).run_headless(1, |_| {});
```

### Benchmark runs

`PONKO_BENCH=<frames>` turns any windowed game into a benchmark: after
`PONKO_BENCH_WARMUP` frames (240), it measures that many with vsync off,
prints a report and quits. The report has the frame-time percentiles, the
CPU split, the GPU passes, the slowest systems and the meshes drawing the
most triangles; `PONKO_BENCH_OUT=file.json` also writes it as JSON, and
`PONKO_BENCH_MESHES=file.csv` writes every mesh's counts. Frames are
drawn offscreen, so the display's refresh rate neither caps nor stalls
them (`PONKO_BENCH_PRESENT=1` shows them instead).

```sh
PONKO_BENCH=600 PONKO_BENCH_OUT=before.json cargo run --release
```

Run it at a fixed camera before and after a change, and compare.

### Tracking benchmarks over time

A slow drift between releases is easy to miss in single pull requests, so the
benchmarks can be tracked over time. `bench_track`, a tool in `crates/ponko_bench`, counts the
instructions of seven small workloads under cachegrind: ECS iteration, a
filtered query, lookups, spawning, despawning, a headless game tick and
collecting a 3D frame. Instruction counts hardly move with the load of a
shared or busy machine, where times jump around; they don't show memory or
parallel effects, which the wall-clock benchmarks still measure.

- **Pull requests** that touch the engine are counted against their base
  by the author or a maintainer (three runs each, with the same workloads),
  and the pull request names every benchmark that moved beyond noise.
- **Releases** add a wall-clock baseline from one named machine
  (`crates/ponko_bench/releases/`), so two releases can be compared from
  files.

A change counts when the medians differ by more than the threshold: the
larger of a floor (1% for instruction counts, 5% for times) and twice the
larger spread (`(max - min) / median`) of the two sides' runs. Every report
prints both spreads and the threshold. To compare your own runs:

```sh
for i in 1 2 3; do PONKO_BENCH_JSON=before/$i.json cargo run --release -p ponko_bench -- ecs/; done
# ... make the change, then the same into after/ ...
cargo run --release -p ponko_bench --bin bench_track -- compare before/ after/
```

The `ponko_bench` README, "Tracking results over time", has the details:
the workloads, the file formats and how a pull request compares them.

### The rendering baseline

For work on the renderer itself there is a fixed set of scenes that needs
no game: seven generated 3D scenes (a static world, a dynamic one, many
materials, heavy shadows, see-through models, skinned figures and a small
town with all of it), drawn offscreen by the real renderer.

```sh
PONKO_BENCH_JSON=render.json PONKO_RENDER_CSV=render.csv \
  cargo run --release -p ponko_bench --features render -- render/scene
```

Each scene reports the CPU steps of a frame (collect, prepare, encode,
submit), the GPU time of every pass, draw calls, triangles, uploads,
allocations, the memory the renderer can count, and the frame-time
distribution (median, 95th and 99th percentile). The JSON also holds the
adapter, backend, driver, the limits and features the device gave, the
resolution, sample count and every quality setting, so two files say
whether they can be compared at all:

```sh
cargo run -p ponko_bench --bin render_compare -- before.json after.json
```

What a device can't measure is written as missing with the reason (GPU
pass times without timestamp queries, allocator totals on Metal), never as
0. It opens no window. `crates/ponko_bench/RENDER_SCENES.md` describes the
scenes, the settings, every field, the committed baseline and the three
scenes built again with Bevy, with what is and is not matched between the
two.

### From an agent

The [control endpoint](control.md) has the same numbers (`perf.get`,
`perf.trace`, `perf.hud`), and the [MCP server](agents.md) offers them as
`perf_report`, `record_trace` and `perf_hud`: an agent can ask a running
game why it's slow, change something, and ask again.

### The render-pass inspector

`perf.get` (and `perf_report`) always includes `render_inspector`, the
last frame's rendering cost in a fixed shape for tools. `schema` is its
version (1); a field is only removed or changes meaning with a new version.
A field's suffix is its unit: `_ms` milliseconds, `_bytes` bytes; counts
are per frame.

| Field | What |
|---|---|
| `gpu_timing` | `available`: the renderer has a GPU timer; `frames_timed`: frames whose GPU times came back (one in 16 outside profiling, see `ctx.perf.gpu_passes()` above; `perf.get` with `profiling: true` times every frame); `cadence`: what the game chose with [`set_gpu_timing`](#how-often-gpu-passes-are-timed), `auto`, `every` (with `every`: the number of frames) or `off`; `untimed`, only when the latest frame has no usable GPU time: why, one of the [reasons below](#no-gpu-time) (`no_gpu`, `no_timestamps`, `query_set_budget`, `off`, `not_ready`, `device_lost`). `cadence: "off"` with `available: true` is the game's choice, so no times are expected (`unavailable` says so); `available: false` is an adapter without timestamps or a timer the query-set budget refused, whatever the cadence (`untimed` tells them apart) |
| `cpu_ms` | the renderer's CPU time: `total`, split into `extract` (culling, collecting models), `prepare` (uploading 3D data), `encode` (recording command buffers), `submit` and `other` (sprites, overlays) |
| `gpu_ms` | `frame`, `passes` (`[{name, ms}]` in the order they run), smoothed, and `not_timed`: the names of the passes that ran in the latest timed frame but couldn't be timed (see [which passes can be timed](#which-passes-can-be-timed)), `[]` when every pass was timed. They are never in `passes` as 0 ms; the frame's `gpu_ms.frame` still covers them. `passes` and `not_timed` are `null` together (with the reason in `unavailable`) when nothing could be timed |
| `counts` | `draw_calls`, `sprites`, `models`, `triangles`, `shadow_models`, `shadow_triangles`, `occluded_models`, `frustum_culled_models` (see below), `frustum_culled_other_models` (always `null`, see below), `pipelines_built`, and `instance_batches`: how the [static instance batches](3d.md#which-path-a-batch-takes) were culled (`batches`, `instances`, `gpu_driven`, `gpu_instances`, `cpu_small`, `cpu_no_compute`, `cpu_over_limit`, `dropped`, `pools`, `pages`, `pool_bytes`, `cull_dispatches`, `cull_jobs`, `views_skipped`) |
| `upload_bytes` | bytes the renderer uploaded to the GPU in the last drawn frame (`render.uploads` in `perf.get` splits them by kind) |
| `memory_estimate_bytes` | what the renderer's tables hold for the game: its `textures` (render-target textures included), `meshes`, its `gpu_buffers`, the `instance_pools` and their `total`. Not counted: the frame's own targets (scene color and depth, post-processing, ambient occlusion and temporal history), shadow maps, the light shadow atlas, skinning buffers and per-frame instance buffers. An estimate from what the engine allocated, not the driver's total; `gpu_memory_bytes` counts everything |
| `gpu_memory_bytes` | every texture and buffer the engine holds on the GPU, each counted once, by class: `textures`, `meshes`, `gpu_buffers`, `frame_targets`, `shadows`, `atlases` (the lights' shadow atlas), `skinning`, `probes`, `instance_batches`, `other`, and their `total` (see [GPU memory](profiling.md#gpu-memory)); `adapter`: the platform's own figure where it reports one (Vulkan, DirectX 12, Metal), `null` elsewhere with the reason in `unavailable`, and `adapter_note` saying which. `null` without a GPU |
| `gpu_allocations` | with `gpu_allocations: true` (or a count) in the request, every texture and buffer the engine holds (or that many of the largest), largest first: `class`, `label` (`3d color`, `3d depth`, `3d shadow map`, ...), `kind` (`texture`, `buffer`, `other`), `bytes`; textures add `size` (width, height, layers), `mips`, `samples` and `format`, buffers `usage` (`uniform`, `storage`, `vertex`, `index`, `indirect`, `staging`, `query`, `copy`); with `count`, `listed`, `unlisted_bytes` and `buffer_bytes_by_usage`. `null` when not asked for, or without a GPU (see [GPU memory](profiling.md#gpu-memory)) |
| `adapter` | the GPU the renderer opened: `name`, `backend` (`vulkan`, `metal`, `dx12`, `gl`, `webgpu`), `device_type`, `driver`, `driver_info`, its [capability](3d.md) `tier` (`webgl2`, `baseline`, `desktop`) and `forced` (`true` when `PONKO_GPU_TIER` set the tier) |
| `history` | with `history: N` in the request, the last N drawn frames one by one, oldest first: `frame`, `cpu_ms`, `gpu_smoothed_ms` (the smoothed GPU time as it stood after that frame), `gpu_latest_ms` and `gpu_latest_frames_ago` (the newest single frame's own GPU time that had come back by then, and how many drawn frames earlier that frame was: timestamps come back a few frames late, so a frame's own GPU time isn't known yet when it is kept), `draw_calls`, `models`, `triangles`, `shadow_triangles` and `upload_bytes`. The GPU fields are `null` when GPU time wasn't measured. Up to 120 frames are kept, in a ring of 88-byte entries allocated once at the first drawn frame; without the parameter it is `[]` |
| `quality` | the active [graphics quality](3d.md#graphics-quality) `level` (`low` ... `ultra`, or `custom`) and every setting (3D builds) |
| `unavailable` | a reason for every metric that is `null` |

`counts.frustum_culled_models` counts plain models (entities with a
`Model`) whose bounds were outside the camera's view in the last frame. It
is worked out when `perf.get` asks, from the bounds the renderer keeps per
model, with the same test the culler uses, so a frame pays nothing for it
and a still camera gives the same number every frame. Models hidden by
occlusion culling are inside the view and counted in `occluded_models`
instead (it counts plain models and each LOD renderer once, a crossfade
between two hidden levels counting both, but not shadow-only casters).
Models whose mesh isn't loaded yet aren't counted in `frustum_culled_models`. While most
models move every frame the renderer stops keeping per-model results, and
the field is `null` with that reason until they settle.
`counts.frustum_culled_other_models` is always `null`: LOD groups, terrain
tiles, skinned models, models with morph targets and shadow-only casters
are culled anew every frame
without a result kept to count from, and static instance batches are
culled on the GPU (`counts.instance_batches` says how).

A metric the engine can't measure is `null`, never `0`, and `unavailable`
says why under its path, for example `"gpu_ms.frame": "the GPU adapter
doesn't support timestamp queries, so GPU time can't be measured"`.
Headless runs and games that haven't drawn yet report `cpu_ms`, `gpu_ms`
and `counts` as `null` the same way. The inspector is built only when
`perf.get` is called, from numbers the renderer keeps anyway, so it
costs a frame nothing; the history is a fixed ring of 120 entries (about
10 KB) filled in place after each drawn frame.

#### Measuring what a quality level costs

`quality.set` (the MCP tool `set_graphics_quality`) switches the
[graphics quality](3d.md#graphics-quality) preset of a running 3D game: `low`, `medium`,
`high`, `ultra`, or `recommended` (the engine's choice for the GPU).
Nothing the GPU lacks is turned on: the preset is fitted to the adapter,
and `fitted_to_gpu` says when it had to be (always `false` for
`recommended`, which already fits); a fitted preset's `quality.level` can
read `custom`. The answer has the settings before (`previous`) and after
(`quality`). It changes only the picture, never the simulation, and pins
render scale and shadow resolution against
[automatic quality tuning](3d.md#optional-scene-quality-autotuning) so the
comparison isn't undone; the pins stay until the game restarts. A headless
game accepts it (the settings change and `perf.get` shows them) but draws
nothing, and has no GPU to recommend for; neither has a windowed game
before its first frame.

An agent compares two levels in four steps: ask for the report, change the
level, wait a few seconds for the smoothed GPU times to settle, ask again.
The excerpts below are illustrative: the field names are the engine's, the
numbers made up for the example (shapes as the engine's tests build them).

1. `perf_report` with `history: 3`:

   ```json
   {
     "fps": 58.9,
     "render_inspector": {
       "schema": 1,
       "adapter": { "name": "Example GPU", "backend": "metal", "device_type": "integrated_gpu", "driver": "", "driver_info": "", "tier": "desktop", "forced": false },
       "gpu_timing": { "available": true, "frames_timed": 412 },
       "gpu_ms": { "frame": 14.8, "passes": [{ "name": "shadows 0", "ms": 1.3 }, { "name": "shadows 1", "ms": 1.0 }, { "name": "shadows 2", "ms": 0.9 }, { "name": "shadows 3", "ms": 0.7 }, { "name": "depth prepass", "ms": 1.2 }, { "name": "GTAO horizons", "ms": 0.6 }, { "name": "GTAO bilateral", "ms": 0.3 }, { "name": "opaque", "ms": 6.2 }, { "name": "post", "ms": 2.6 }] },
       "counts": { "draw_calls": 412, "models": 3810, "triangles": 2954112, "shadow_triangles": 4101870, "occluded_models": 0, "frustum_culled_models": 1270, "frustum_culled_other_models": null },
       "memory_estimate_bytes": { "total": 611319808, "textures": 402653184, "meshes": 150994944, "gpu_buffers": 41943040, "instance_pools": 15728640 },
       "history": [
         { "frame": 1840, "cpu_ms": 4.1, "gpu_smoothed_ms": 14.9, "gpu_latest_ms": 15.1, "gpu_latest_frames_ago": 2, "draw_calls": 412, "models": 3810, "triangles": 2954112, "shadow_triangles": 4101870, "upload_bytes": 65536 },
         { "frame": 1841, "cpu_ms": 4.0, "gpu_smoothed_ms": 14.8, "gpu_latest_ms": 14.6, "gpu_latest_frames_ago": 2, "draw_calls": 412, "models": 3810, "triangles": 2954112, "shadow_triangles": 4101870, "upload_bytes": 65536 },
         { "frame": 1842, "cpu_ms": 4.2, "gpu_smoothed_ms": 14.8, "gpu_latest_ms": 14.7, "gpu_latest_frames_ago": 2, "draw_calls": 412, "models": 3810, "triangles": 2954112, "shadow_triangles": 4101870, "upload_bytes": 65536 }
       ],
       "quality": { "level": "ultra", "render_scale": 1.0, "shadow_cascades": 4, "shadow_resolution": 4096, "ambient_occlusion": true, "anti_aliasing": "smaa" },
       "quality_autotune": null,
       "unavailable": { "counts.frustum_culled_other_models": "not counted: ...", "quality_autotune": "automatic quality tuning is off ..." }
     }
   }
   ```

2. `set_graphics_quality` with `level: "medium"`:

   ```json
   {
     "requested": "medium",
     "fitted_to_gpu": false,
     "previous": { "level": "ultra", "render_scale": 1.0, "shadow_cascades": 4, "shadow_resolution": 4096, "ambient_occlusion": true, "anti_aliasing": "smaa" },
     "quality": { "level": "medium", "render_scale": 1.0, "shadow_cascades": 3, "shadow_resolution": 2048, "ambient_occlusion": false, "anti_aliasing": "fxaa" }
   }
   ```

3. Wait a few seconds: GPU times are smoothed over recent frames, and the
   first frames after the switch build new shadow maps and targets.

4. `perf_report` again, and compare:

   ```json
   {
     "fps": 60.0,
     "render_inspector": {
       "gpu_ms": { "frame": 8.1, "passes": [{ "name": "shadows 0", "ms": 0.6 }, { "name": "shadows 1", "ms": 0.5 }, { "name": "shadows 2", "ms": 0.4 }, { "name": "depth prepass", "ms": 1.0 }, { "name": "opaque", "ms": 5.0 }, { "name": "post", "ms": 0.6 }] },
       "counts": { "draw_calls": 377, "models": 3810, "triangles": 2954112, "shadow_triangles": 3011544, "occluded_models": 0, "frustum_culled_models": 1270, "frustum_culled_other_models": null },
       "memory_estimate_bytes": { "total": 476053504, "textures": 268435456, "meshes": 150994944, "gpu_buffers": 41943040, "instance_pools": 14680064 },
       "quality": { "level": "medium", "render_scale": 1.0, "shadow_cascades": 3, "shadow_resolution": 2048, "ambient_occlusion": false, "anti_aliasing": "fxaa" }
     }
   }
   ```

   Here the GPU frame went from 14.8 to 8.1 ms: the shadow passes
   (`shadows 0` ... `shadows 3`) from 3.9 to 1.5 ms with one cascade fewer
   and smaller maps, the ambient occlusion passes (`GTAO ...`) are gone,
   and `post` is cheaper with FXAA instead of SMAA. The camera's
   triangles didn't change, and
   `fps` barely moved because the display's refresh rate capped it. Under
   vsync, compare `gpu_ms`, not `fps`. If `gpu_timing.available` is
   `false` (an adapter without timestamps) or `gpu_timing.cadence` is
   `off` (the game turned timing off), `gpu_ms` is `null` on both sides:
   compare the `cpu_ms` and the `counts` instead, or benchmark with
   `PONKO_BENCH`.

C and C++ games get the control method and the MCP tool through the same
endpoint, and can read and change the quality level themselves with
`ponko_ctx_graphics_quality` and `ponko_ctx_set_graphics_quality`.

### What usually helps in 3D

- **Shadows**: every caster is drawn again for each shadow cascade it
  falls in, also behind the camera. Compare the camera's triangles with
  the shadows' in `render_stats`. Turn shadows off for small things
  (`set_mesh_shadows`), and give detailed models a simpler
  [`ShadowMesh`](3d.md), especially far away.
- **Distant detail**: swap far models for simpler meshes or billboards.
  `mesh_costs` shows which meshes cost the most.
- **Pixels**: on a large Retina screen, `ctx.scene3d.post.msaa = 2` costs about a
  third less GPU time for the 3D passes than the default 4x.

### Models between frames

The 3D frame remembers the models it collected: each model's place in the
list its mesh is drawn from, in the camera's view and in every shadow map,
and what it was collected from. A frame then pays for what changed:

- **Nothing changed**: one compare per model. Nothing is culled, written
  or uploaded.
- **The camera or the sun moved**: every model is culled again, from the
  bounding sphere kept for it. Models that stay in view keep their place,
  so only the ones that entered or left a view are written and uploaded.
- **A model changed** (its `d3::Transform`, mesh, tint, `ShadowMesh`,
  `ModelMaterial` or `ReceivesDecals`): that model is worked out again
  and its instances rewritten. Changes are found by value, whichever way a
  component is written: a system that sets the same transform every frame
  changes nothing.
- **Models spawned or despawned, or got or lost a component**: the new
  ones are worked out, the gone ones leave their lists.
- **A mesh changed** (new data with other bounds, `set_mesh_shadows`): the
  models of that mesh are worked out again.

`LodGroup` renderers, terrain tiles, skinned models and shadow-only casters
are still worked out every frame (what they draw depends on the camera or
a pose), but keep their places in the lists as well.

While more than half the models change every frame, comparing them costs
more than it saves. The frame then works every model out from its
components alone, as it did before any were kept, and only looks up the
ones a view draws; it compares one model in 64 to notice when they settle.

Collecting 100,000 models of 64 meshes, a tenth of them in view, with sun
shadows (`cargo run --release -p ponko_bench -- extract-100k`: the median
of 120 frames after 30 warm-up frames, then the median of 8 runs on all 14
cores and of 5 runs on one core with `RAYON_NUM_THREADS=1`; M4 Max, while
other builds were running on the machine, so the 14-core times are a
little high on both sides):

| Each frame | 14 cores: before | now | One core: before | now | Instances uploaded: before | now |
|---|---|---|---|---|---|---|
| nothing changes | 0.45 ms | 0.20 ms | 1.21 ms | 0.41 ms | 11 KiB | 0 |
| the camera flies | 0.44 ms | 0.29 ms | 1.20 ms | 0.85 ms | 789 KiB | 25 KiB |
| it flies, 1% of the models move | 0.44 ms | 0.30 ms | 1.20 ms | 0.87 ms | 791 KiB | 73 KiB |
| it flies, every model moves | 0.45 ms | 0.45 ms | 1.20 ms | 1.31 ms | 831 KiB | 906 KiB |
| it flies, 100 models despawn and 100 spawn | 0.48 ms | 0.47 ms | 1.40 ms | 1.13 ms | 795 KiB | 27 KiB |
| the first frame (nothing remembered) | 0.49 ms | 1.33 ms | 1.25 ms | 2.66 ms | all | all |

What it costs:

- **Every model moving, on one core** (the web): about 9% slower than
  before (1.31 ms against 1.20 ms above). The models are culled as before;
  each drawn one is then looked up and written to its place in every view
  that draws it, which costs more than appending it to a list that is
  sorted afterwards. On 14 cores it costs the same as before.
- **The first frame**, and a frame that spawns many models at once, builds
  what later frames keep: two to three times a frame of before for those
  models, once.
- **Memory**: for every model in the world, in view or not, about 150
  bytes on one core and 210 on several (14 and 21 MiB for the 100,000
  above, with their instances; a frame's scratch lists took 3.5 MiB
  before). Every view that collects models keeps its own: a target camera
  with a 3D view, and global illumination's capture, each cost that again
  for every model in the world.
- **Order**: models of one mesh are kept in no particular order, and it
  changes as models enter and leave the view. `Blend::Transparent` models
  of one mesh are drawn far to near by their origins every frame, whatever
  that order is. Opaque models drawn in another order can differ in a few
  edge pixels, by a little.
- **`ctx.gfx.models_drawn()`** counts each model once, including models
  in a shadow cascade whose map is kept this frame.

`ctx.perf.render_stats().uploads.instances` shows what a frame uploaded: with a
moving camera it should be a small part of the instances drawn.

### What a frame sends the GPU

The renderer keeps a copy of what each per-frame buffer holds on the GPU
and uploads only the parts that changed since the last frame: the
models' records, their motion records (temporal antialiasing), the frame
settings of the camera and of every shadow view, the post-processing and
effect settings, skeletons' joints and morph weights, instance batches'
view settings, particles, ribbons, lines and 3D text, and 2D sprites. A
model that slides without turning changes 32 of its 144 motion bytes,
and only those go. A buffer that outgrows its contents is made anew at
the next power of two, so a world that keeps growing replaces it a
logarithmic number of times, and one that stays the same size replaces
none; the new buffer is written whole, and everything that referred to
the old one (bind groups, the copy of its contents) is made again with
it. Buffers never grow past what the device allows: a frame that needs
more is reported once on stderr (`ponko: ... more than this GPU's
buffers hold`), and what doesn't fit is not drawn (particles, ribbons
and text past the limit; with too many models or sprites, the frame's 3D
scene or its sprites) instead of failing.

Each write costs more than its bytes: `wgpu` makes and maps a staging
buffer for every one, about 2 µs of CPU time on an M4 Max, as much as
copying about 30 KB more. So a buffer whose changes can be scattered
(one record per model, sprite, particle or joint) is written in at most
16 writes a frame: when more parts changed, the narrowest gaps between
them are uploaded along. That sends more bytes than the changes alone,
never more than the whole buffer, and keeps the CPU time bounded however
the changes fall.

The copies cost CPU memory: as much as their buffers hold at most, which
for most games is small. Each model's record is 80 bytes in every list
it is drawn in (the camera, each shadow cascade, each target view): 1,024
models in a camera and four cascades keep about 400 KB. Particles keep
48 bytes each, a skeleton 64 bytes a joint, and every view's frame
settings about 3 KB. The copies grow with their buffers and, like them,
never shrink.

`RenderStats::uploads` (`ctx.gpu_uploads()` in C++) counts what was sent,
the buffers made and the buffers and textures replaced:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/profiling.rs:uploads}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:uploads}}
```

</div>
</div>

Measured offscreen on an M4 Max (Metal), release build, 128×128 target,
sun shadows with four cascades, 1,024 cubes (256 for the particles), the
clock running, averages over 120 frames after 10 warm-up ones; before is
the renderer before this change, which uploaded every per-frame buffer
whole:

| Scene | Bytes uploaded per frame, before | After | `prepare` CPU p50, before | After |
|---|---:|---:|---:|---:|
| Nothing moves | 15,616 | 80 | 0.085 ms | 0.053 ms |
| 1% of the models move, temporal antialiasing | 20,640 | 99,019 | 0.157 ms | 0.155 ms |
| The camera pans | 17,498 | 4,994 | 0.079 ms | 0.098 ms |
| 5% of the models despawned and spawned again a frame | 39,715 | 24,243 | 0.098 ms | 0.097 ms |
| 2,000 still particles and 8 lines | 113,440 | 80 | 0.065 ms | 0.049 ms |

The 80 bytes left in a still scene are the clock: one vec4 in the frame
settings of the camera and of each of the four cascades. No scene made a
buffer in its measured frames. With temporal antialiasing, the 11 moving
models' records are spread over the lists of the camera and the
cascades, in more runs than a list's 16 writes (below): without that
limit the frame took 67 writes and 4,331 bytes, with it 48 writes and
99,019 bytes, and no more CPU time (0.155 ms, against 0.160 to 0.212 ms
in runs without the limit). The
byte counts are exact and the same on every run. The CPU times are from
runs on a machine busy with other builds (load 10 to 80), and the same
build moved by up to 0.02 ms between runs: the panning camera took 0.080
and 0.098 ms in two runs of one build, so it shows no clear change.

Changes scattered all over the buffers (every 17th of the 1,024 models
moving and every other of 200 particle emitters), by the most writes a
buffer takes per frame:

| Writes per buffer | Writes per frame | Bytes per frame | `prepare` CPU p50 |
|---|---:|---:|---:|
| No limit | 241 | 98,080 | 0.56 ms |
| 64 | 133 | 207,520 | 0.37 to 0.41 ms |
| 32 | 69 | 263,840 | 0.27 ms |
| 16 (the renderer's) | 37 | 292,000 | 0.17 ms |

Run it with
`cargo test --release --lib --features 3d upload_bench -- --ignored --nocapture`;
it also prints what writes cost on the machine it runs on.

## Ponko vs Bevy: the scoreboard

The same workloads written as an ordinary Ponko game and as an ordinary Bevy
app, timed the same way on the same machine. One command runs everything:

```sh
python3 bench/compare/scoreboard.py              # everything
python3 bench/compare/scoreboard.py --runtime    # only the run-time rows (about 10 minutes)
python3 bench/compare/scoreboard.py --builds     # only build times and sizes
```

It builds both sides in release, runs each workload three times per engine,
one process at a time, changing which engine goes first, and waits for a
1-minute load under 3 before each run. It writes
`bench/compare/results/scoreboard.json` (every run, with its load) and the
table below. Bevy lives in its own crate, `bench/compare`, with its own lock
file, so it is not in the engine's dependency tree. Its version is pinned
there and printed in every result. `bench/compare/README.md` names the Bevy
API each workload uses (its recommended fast path, with links) and lists
everything that is and is not the same on both sides.

**Machine and build.** MacBook Pro, Apple M4 Max (10 performance + 4
efficiency cores, 36 GB), macOS 27.2.1, rustc 1.100.0, Ponko at
`5e5af4ee` plus this change, Bevy 0.20.1 (bevy_rapier2d 0.37.0, rapier
0.36.1 on both sides). Release builds with Cargo's default release profile
on both sides (`opt-level = 3`, no LTO), native target. GPU rows draw into an
offscreen 1280 x 720 target with Metal, and every frame waits for the GPU.
Measured 2026-11-22. The run-time rows ran on AC power with a 1-minute load
of 1.5 to 2.3. The time-to-first-frame row ran on battery (load 2.7).

**Run time.** The median of three runs (each run's own number is the median
frame or tick of 300, after 60 warm-up frames), with the range in brackets.
"Within noise" would mean the two ranges overlap; none do.

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |
|---|---:|---:|---:|---|
| ECS: spawn and despawn 100k in a world of 1M | 1.76 (1.73–1.92) | 4.27 (4.22–4.33) | 2.4× | Ponko faster |
| ECS: iterate 1M (one archetype) | 0.165 (0.164–0.166) | 0.390 (0.362–0.405) | 2.4× | Ponko faster |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.005 (0.005–0.006) | 0.010 (0.009–0.011) | 1.9× | Ponko faster |
| ECS: add and remove a component, 1M entities | 42.8 (41.6–43.1) | 98.9 (97.5–101.9) | 2.3× | Ponko faster |
| ECS: parallel iterate 1M (all cores) | 0.560 (0.551–0.627) | 0.741 (0.740–0.783) | 1.3× | Ponko faster |
| ECS: commands, 100k spawns and despawns in a world of 1M | 2.40 (2.38–2.45) | 5.39 (5.29–5.42) | 2.2× | Ponko faster |
| Sprites: 100k moving, one frame drawn | 1.10 (1.09–1.13) | 11.6 (11.1–15.0) | 10× | Ponko faster |
| Sprites: 100k moving, a different tint each | 1.09 (1.09–1.09) | 44.8 (42.9–45.1) | 41× | Ponko faster |
| Sprites: 1M moving, one frame drawn | 8.32 (8.18–8.35) | 105.7 (104.4–106.5) | 13× | Ponko faster |
| 3D: 100k moving meshes (16 kinds) with a 4-cascade sun shadow | 4.63 (4.61–4.64) | 15.8 (15.8–15.9) | 3.4× | Ponko faster |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 6.06 (6.02–6.11) | 3.86 (3.86–5.51) | 0.64× | **Bevy faster** |
| 2D physics: 10k boxes falling into a pile, one tick | 6.39 (6.37–6.49) | 8.11 (8.00–8.28) | 1.3× | Ponko faster |
| UI: 10k nodes, still, one frame drawn | 1.63 (1.25–2.09) | 4.58 (4.57–4.81) | 2.8× | Ponko faster |
| UI: 10k nodes, all recolored every frame | 2.49 (2.47–2.55) | 6.73 (6.70–6.93) | 2.7× | Ponko faster |
| Headless sim: 1M entities, one tick | 0.166 (0.165–0.167) | 0.407 (0.406–0.443) | 2.5× | Ponko faster |
| Time to first frame (process start to first frame done on the GPU, exit included) | 28.9 (27.7–41.2) | 198.7 (197.9–244.6) | 6.9× | Ponko faster |

How to read it:

- **Bevy is faster at skinned characters**: 3.86 ms against 6.06 ms a frame
  for 1,000 figures of 16 joints with a sun shadow. One of Bevy's three runs
  took 5.51 ms, still under Ponko's fastest. Bevy is the faster engine on
  this workload.
- **Sprites.** Bevy 0.20 draws each `Sprite` as a 2D mesh with a material,
  and equal sprites share one material. One color is therefore Bevy's fast
  path. A different tint for every sprite gives every sprite its own
  material, and Bevy's frame grows from 11.6 to 44.8 ms. Ponko batches tinted
  and untinted sprites the same way. Neither engine multisamples the 2D
  rows: Ponko's 2D renderer draws with one sample, and Bevy's 2D camera has
  `Msaa::Off`. (An earlier version of this page said Ponko used 4x MSAA
  here; it does not.)
- **Pipelined rendering is off for Bevy.** Ponko has no pipelined renderer,
  so both engines simulate and render on one thread and finish each frame
  before the next. With pipelining on, Bevy can overlap a frame's simulation
  with the previous frame's rendering on a multi-core machine: the
  [tuned table](#both-engines-tuned) has it on.
- **Time to first frame** counts one launch after a warm-up launch, so the
  OS file cache and the driver's shader cache are warm for both. The very
  first launch after a build took Ponko 487 ms. Bevy exits with
  `process::exit` right after its frame. Ponko returns from `main` and drops
  its world.
- These are specific workloads on one machine. They are not a ranking of
  the engines (see [the engine comparison](comparison.md)).

**Builds and sizes.** A hello world (an empty window) and a small game (Ponko's
2D template: a square moved with the keys, a text hint, settings) on each
side, each copied to a fresh folder outside the checkout. Debug times come
from `scripts/build-times.sh` (the method of
[Build times and download sizes](#build-times-and-download-sizes)): the median of three
clean builds, and of four rebuilds after a one-line change. Both
templates use the same dev profile (your code at `opt-level = 1`, the
dependencies at 3). Release: one clean build. wasm: `wasm32-unknown-unknown`
release, as Cargo writes it (before `wasm-bindgen` and `wasm-opt`). Default
features on both sides. Measured on battery; the 1-minute load during the
builds (3.5 to 14) is the builds' own.

| | Ponko hello | Bevy hello | Ponko small game | Bevy small game |
|---|---:|---:|---:|---:|
| Clean debug build (s) | 46.4 (46.2–46.5) | 117 (117–118) | 45.2 (45.2–45.5) | 118 (117–119) |
| Debug rebuild, one-line change in `src/lib.rs` (s) | 0.50 | 0.74 | 0.47 | 0.83 |
| ... in `src/main.rs` (s) | 0.39 | 0.62 | 0.32 | 0.64 |
| Clean release build (s, one run) | 36.2 | 90.0 | 35.7 | 91.0 |
| Release binary / stripped | 24.8 / 19.6 MB | 134.2 / 82.2 MB | 24.5 / 19.4 MB | 134.4 / 82.4 MB |
| wasm, release / gzipped | 20.7 / 5.8 MB | 120.5 / 21.4 MB | 20.7 / 5.8 MB | 120.7 / 21.5 MB |

The rebuild ranges are within 0.03 s of the medians. Bevy's default features
include much more than either program uses (3D, glTF, audio formats,
animation, picking and more). A Bevy game that turns off what it doesn't use
builds faster and smaller, and so does a Ponko game that turns off default
features it doesn't use (the inspector, audio, gamepads). The
[tuned table](#both-engines-tuned) measures that.

The scoreboard compares the Rust APIs. The C and C++ API calls the same
engine, but it has no rows here: a C or C++ program of these workloads has
not been written yet.

### Both engines tuned

The table above measures each engine as a new project starts. The second
table measures each engine the way a careful game built on it would ship:

```sh
python3 bench/compare/scoreboard.py --tuned      # the run-time rows (about 10 minutes) and the build rows
```

What changed, on both sides at once unless named (`bench/compare/README.md`,
"The tuned table", has the source of every choice):

- **One shipping profile** for both: fat LTO, one codegen unit,
  `panic = "abort"`.
- **Bevy's pipelined rendering is on** (its default). A frame is timed from
  one frame finished on the GPU to the next, and every frame is still
  waited for, so the GPU never runs ahead. With pipelining Bevy's frame is
  the longer of its simulation and its drawing, not their sum. Ponko has no
  pipelined renderer: its frame is still both, one after the other.
- **Only the Bevy features each workload needs** (`default-features =
  false` and Bevy's feature collections, one program per group of
  workloads).
- **The faster of each engine's documented ways**, measured: Bevy draws
  the sprite rows with `Mesh2d` and a `ColorMaterial` (faster than
  `Sprite`), moves the sprites and the sim with `par_iter_mut`, and keeps
  its CSS grid for the UI (faster than flex rows). Ponko moves the sprites
  and the sim with `par_for_each`.
- **rapier's `parallel` and `simd8` features** on both sides (rapier 0.36
  has no `simd-stable`).
- **Time to first frame** ends both programs the same way (`process::exit`
  right after the frame).
- Unchanged: neither engine multisamples 2D (the same as before), the 3D
  scenes use 4x MSAA on both, and Bevy 0.20 has no sprite path that avoids
  one material per tint (its own `many_sprites` example says tinted sprites
  render "in multiple batches, reducing performance").

**Machine and build.** The machine of the first table, on AC power, Ponko
at `784d7a2b` (the run-time rows were taken from that code before it was
committed; the result file names its parent), Bevy 0.20.1, bevy_rapier2d
0.37.0, rapier 0.36.1. Measured 2026-11-22 with a 1-minute load of 1.7 to
2.9.

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | | Time against the first table |
|---|---:|---:|---:|---|---|
| ECS: spawn and despawn 100k in a world of 1M | 1.60 (1.59–1.62) | 4.38 (4.34–4.52) | 2.7× | Ponko faster | Ponko −9%, Bevy +3% |
| ECS: iterate 1M (one archetype) | 0.165 (0.164–0.173) | 0.391 (0.370–0.404) | 2.4× | Ponko faster | both within noise |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.005 (0.005–0.006) | 0.010 (0.010–0.012) | 2.2× | Ponko faster | both within noise |
| ECS: add and remove a component, 1M entities | 42.1 (42.1–42.1) | 104.0 (102.3–107.8) | 2.5× | Ponko faster | Bevy +5%, Ponko within noise |
| ECS: parallel iterate 1M (all cores) | 0.624 (0.614–0.629) | 0.755 (0.733–0.773) | 1.2× | Ponko faster | both within noise |
| ECS: commands, 100k spawns and despawns in a world of 1M | 2.23 (2.22–2.29) | 5.05 (5.03–5.17) | 2.3× | Ponko faster | Ponko −7%, Bevy −6% |
| Sprites: 100k moving, one frame drawn | 1.02 (0.99–1.03) | 6.74 (6.71–6.84) | 6.6× | Ponko faster | Bevy −42%, Ponko −7% (was 10×) |
| Sprites: 100k moving, a different tint each | 0.99 (0.98–0.99) | 37.4 (36.5–37.9) | 38× | Ponko faster | Bevy −17%, Ponko −10% |
| Sprites: 1M moving, one frame drawn | 7.62 (7.35–7.74) | 58.9 (58.8–59.1) | 7.7× | Ponko faster | Bevy −44%, Ponko −8% (was 13×) |
| 3D: 100k moving meshes (16 kinds) with a 4-cascade sun shadow | 4.68 (4.66–4.72) | 14.1 (14.0–14.1) | 3.0× | Ponko faster | Bevy −11%, Ponko +1% |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 6.06 (5.96–6.06) | 3.12 (3.07–3.23) | 0.51× | **Bevy faster** | Bevy −19%, Ponko within noise (was 0.64×) |
| 2D physics: 10k boxes falling into a pile, one tick | 3.25 (3.17–3.40) | 4.50 (4.48–4.76) | 1.4× | Ponko faster | Ponko −49%, Bevy −45% (rapier `parallel`) |
| UI: 10k nodes, still, one frame drawn | 1.24 (1.20–1.26) | 2.71 (2.62–2.73) | 2.2× | Ponko faster | Bevy −41%, Ponko within noise |
| UI: 10k nodes, all recolored every frame | 1.66 (1.62–1.83) | 4.60 (4.51–4.67) | 2.8× | Ponko faster | Ponko −33%, Bevy −32% |
| Headless sim: 1M entities, one tick | 0.069 (0.069–0.071) | 0.170 (0.164–0.172) | 2.5× | Ponko faster | both −58% (all cores) |
| Time to first frame (process start to first frame done on the GPU, exit included) | 26.8 (26.0–29.3) | 71.6 (71.0–76.4) | 2.7× | Ponko faster | Bevy −64%, Ponko within noise (was 6.9×) |

How to read it, row by row:

- **No row changed sides.** Ponko is faster on every row but the skinned
  characters, where Bevy is faster and its lead grows (3.12 against
  6.06 ms, 0.51×; it was 0.64×).
- **Bevy's GPU rows moved most**, from pipelining and the leaner builds:
  100k sprites 11.6 to 6.74 ms, 1M sprites 105.7 to 58.9 ms, the still UI
  4.58 to 2.71 ms, the moving 3D world 15.8 to 14.1 ms. Ponko is still
  6.6 to 7.7 times faster on sprites, 3.0 times on the 3D world and 2.2 to
  2.8 times on the UI. Ponko's GPU rows moved little: its sprite rows are
  7 to 10% faster with the movement on all cores, its still UI and first frame
  are within noise, and its 3D world is 1% slower (4.68 against 4.63 ms,
  just outside both ranges).
- **Tinted sprites** stay Bevy's slow path (37.4 ms; `Mesh2d` was the
  faster of its two ways), 38 times Ponko's 0.99 ms.
- **Physics**: rapier's `parallel` feature halves the tick on both sides
  (Ponko 6.39 to 3.25 ms, Bevy 8.11 to 4.50 ms). Ponko stays 1.4 times
  faster; the difference is outside rapier (the solver is the same crate).
- **The sim** runs on all cores on both sides: 0.069 against 0.170 ms.
- **Time to first frame**: Bevy's falls from 199 to 72 ms with only the 2D
  features, the shipping profile and the same exit; Ponko's stays at 27 ms.
  Which of the three gives Bevy most of it was not separated.
- **The ECS rows** run the same code with the shipping profile. Two of
  Bevy's got slower: add and remove 98.9 to 104.0 ms (5%) and spawn and
  despawn 4.27 to 4.38 ms (3%), both outside the ranges, with no
  explanation found (the code is the same; only the profile changed).
  Ponko's spawn and commands rows got 7 to 9% faster.
- The ranking is these workloads on one machine, not a verdict on the
  engines (see [the engine comparison](comparison.md)).

**Builds and sizes, tuned.** The small game only (the hello world was not
built again), each engine with the features the game uses and nothing
else: Ponko with `default-features = false, features = ["window"]` (no
audio, gamepads, inspector or JPEG), Bevy with `2d_bevy_render`,
`ui_bevy_render`, `default_app`, `bevy_winit`, `default_font` and the
platform features (`std`, `multi_threaded`, and `webgl2`, `x11`,
`wayland` for the web and Linux). Debug builds use each engine's fast-
compile advice: the templates' dev profile on both sides, and for Bevy its
`dynamic_linking` feature (Bevy's own `config_fast_builds.toml` keeps
macOS on the default linker, as Ponko's guide does). The release and wasm
rows use the shipping profile. Measured on AC power; the load during the
builds (2.3 to 16) is the builds' own.

| | Ponko small game | Bevy small game | First table: Ponko / Bevy |
|---|---:|---:|---|
| Clean debug build (s) | 41.6 (40.1–42.6) | 97.0 (96.0–101.0) | 45.1 / 118 |
| Debug rebuild, one-line change in `src/lib.rs` (s) | 0.42 | 0.54 | 0.47 / 0.83 |
| ... in `src/main.rs` (s) | 0.29 | 0.32 | 0.32 / 0.64 |
| Link (rustc's `link_binary`, debug) (s) | 0.12 | 0.08 | |
| Clean build, shipping profile (s, one run) | 108 | 212 | release: 35.7 / 91.0 |
| Binary / stripped | 9.8 / 8.5 MB | 52.1 / 33.1 MB | release: 24.5 / 19.4 MB, 134.4 / 82.4 MB |
| wasm, shipping profile / gzipped | 12.7 / 3.6 MB | 57.5 / 11.9 MB | release: 20.7 / 5.8 MB, 120.7 / 21.5 MB |

Row by row: Ponko builds faster from clean (41.6 against 97.0 s) and
rebuilds faster after a change in `src/lib.rs` (0.42 against 0.54 s).
After a change in `src/main.rs` the two are 0.29 and 0.32 s; the rebuild
ranges are within 0.03 s, so that is close to the noise. Bevy's
`dynamic_linking` gives it the faster link (0.08 against 0.12 s) and cut
its rebuilds by a third to a half against the first table. Bevy's clean
build is 21 s shorter than in the first table (fewer features and dynamic
linking together, not measured apart), Ponko's 3.5 s (fewer features). The
shipping profile takes two to three times as long as a release build, and
the binaries come out at 40% of their release size or less. Ponko's game is
still the smaller download: 8.5 MB stripped against 33.1 MB, and 3.6 MB
against 11.9 MB of gzipped wasm.

## 3D: Ponko vs Unity

The same 3D scene in Ponko and in Unity 6, built the way a normal Unity
game is: one GameObject per cube with a `MeshRenderer` and a script whose
`Update()` turns it. Both scenes have:

- N spinning, tinted cubes;
- 12 physically based spheres and a ground;
- a sun with soft shadows (one 2048² shadow map, 60 units);
- sky and ground light and 4x MSAA.

Both run fullscreen at the display's native resolution with vsync off, on
the M4 Max. Each number is the median of three 10-second runs, after a
3-second warmup.

![The 100,000-cube scene: Ponko on the left, Unity 6 (URP) on the right](images/3d-vs-unity.jpg)

*Ponko (left) and Unity 6 (right), 100,000 cubes. Unity's axes are
left-handed, so it sees the field mirrored.*

| Spinning cubes | Unity 6 (URP) | Ponko | Ponko's lead |
|---|---|---|---|
| 1,000 | 728 FPS | **750 FPS** | about even |
| 10,000 | 398 FPS | **724 FPS** | 1.8x |
| 100,000 | 54 FPS | **402 FPS** | **7.5x** |

CPU time per frame:

| Spinning cubes | Unity main thread | Ponko (everything but waiting for the display) |
|---|---|---|
| 1,000 | 0.65 ms | **0.17 ms** |
| 10,000 | 2.51 ms | **0.43 ms** |
| 100,000 | 18.7 ms | **2.48 ms** |

Unity's time goes to the per-object work every GameObject brings: 100k
`Update()` calls, transform syncing, and culling and batching 100k
renderers. Ponko's goes to one query that turns every cube and one pass
that culls them, groups them by mesh and uploads them (28 draw calls).

To be fair to Unity:

- SSAO, post-processing and HDR were disabled in this historical
  comparison. The current engine has additional rendering features.
- The cubes share 64 tinted materials. Giving each its own color, as
  Ponko does for free, would break Unity's batching.
- Ponko's fullscreen frame is 3% smaller: macOS keeps the strip by the
  camera notch, so it's 3456 x 2168 against Unity's 3456 x 2234.
- The Unity player uses Mono, Unity's default script backend. IL2CPP
  would make the `Update()` calls somewhat faster.
- Each run started 10 seconds after the last one ended, so neither engine
  inherited the other's fullscreen exit.
- Unity Entities/DOTS, Jobs and Burst were not measured. The result applies
  to this GameObject/Mono workload; its performance gap cannot be generalized
  to Unity's optimized data-oriented workflows.

Run it yourself:

- Ponko: `CUBES=100000 BENCH_SECONDS=10 cargo run --release --features 3d --example 3d`
  (fullscreen; `BENCH_WINDOWED=1` keeps the window).
- Unity: the project and the steps are in
  [`bench/unity`](../../bench/unity).

On macOS, a game in a window is shown at most once per display refresh
even with vsync off. To measure past 120 FPS, run fullscreen
(`Game::fullscreen(true)`).

## Memory

Speed is half of scale; the other half is what each entity costs. The
headless example prints it: 1M agents with `d2::Transform`, `Sprite` and a
small `Agent` component take **96 bytes per entity** allocated, spare
capacity included: 20 for the transform, 60 for the sprite, 8 for `Agent`
and 8 for the entity handle. (Before 2026-11-20 this page said 76 bytes, a
figure taken before `Sprite` gained its 20-byte `region`.)

From code, `world.allocated_bytes()` is the total for all component data,
and `Archetype::bytes_per_entity()` is what one more entity of that exact
component set costs. Agents get both from the control endpoint's
`world.summary` (the MCP `world_summary` tool).

## Build times and download sizes

How long a game made with `ponko new` takes to build, and how big it is.
Measured on 2026-11-22 at revision `0a012240` (the `dev` branch) plus the
template changes below, on the MacBook Pro of this chapter (Apple M4 Max,
10 performance and 4 efficiency cores, 36 GB, macOS 27.2.1, on power),
Rust 1.100.0, nothing else building. Each project was made from the
templates with the engine by path (`ponko new --path`) and built in its own
target folder. Times are cargo's own ("Finished ... in"), the median of
three clean builds and of four rebuilds; the runs of each row were within
4% of each other (clean builds) or 0.05 s (rebuilds and links). The
"3D tutorial" is [`examples/tutorial_3d`](../../examples/tutorial_3d) in a
`--3d` project (`src/lib.rs` is its `game.rs`, with `physics3d`).
`scripts/build-times.sh <project> [dev|release]` repeats every row.

| | 2D template | 3D tutorial |
|---|---|---|
| Clean debug build (the templates' profile) | 45.8 s | 63 s |
| Clean debug build, the profile before 2026-11-22 | 60.0 s | 84 s |
| Clean release build | 36.1 s | 55.7 s |
| Debug rebuild after a one-line change in `src/lib.rs` | 0.46 s | 0.49 s |
| ... in `src/main.rs` | 0.32 s | 0.35 s |
| Release rebuild after a one-line change in `src/lib.rs` | 1.00 s | 0.92 s |
| ... in `src/main.rs` | 0.47 s | 0.52 s |
| Link (rustc's `link_binary`), debug / release | 0.15 / 0.20 s | 0.16 / 0.25 s |
| Debug binary | 33.4 MB | 46.6 MB |
| Release binary / stripped | 24.5 / 19.4 MB | 35.1 / 28.2 MB |
| Target folder, debug / release | 2.4 / 0.9 GB | 3.3 / 1.2 GB |

The edit-to-binary loop is well under the 2 s this project aims for, so
nothing about the engine had to change for it: a change to your code
rebuilds only your crate (incrementally: with `CARGO_INCREMENTAL=0` the
2D debug rebuild takes 1.09 s instead of 0.46 s), and the link is a third
of that time. The engine crate and its dependencies are built once.

### Where a clean build goes

`cargo build --timings` writes a report (`target/cargo-timings/`) of every
crate's build. For the 2D template's clean debug build (the old profile,
304 build units, 354 s of CPU time on 14 cores in 60 s):

| Crate | Time | |
|---|---|---|
| `ponko` (the engine) | 26.9 s | starts at 32 s, when the last of its dependencies is done, and ends the build |
| `naga` (wgpu's shader compiler) | 13.9 s | |
| `syn` 3 and `syn` 2 | 12.5 s, 11.8 s | built first and optimized: the chain `syn` -> `serde_derive` -> `serde` -> ... starts the engine late |
| `wgpu-core` | 10.7 s | |
| `egui` (the inspector) | 10.6 s | |
| `read-fonts`, `skrifa`, `harfrust` (text) | 8.9 s, 7.9 s, 6.6 s | |
| `image`, `moxcms`, `pxfm` (image decoding) | 7.9 s, 6.8 s, 6.4 s | |

Two changes to the templates' debug profile, both in `Cargo.toml`, cut the
clean debug build by a quarter (60.0 to 45.8 s for the 2D template, 84 to
63 s for the 3D tutorial) and its target folder from 3.9 to 2.4 GB, and
leave rebuilds as they were:

- `debug = "line-tables-only"` for the dependencies (they were built with
  full debug info at `opt-level = 3`): 60.0 to 47.5 s. Backtraces still
  name the file and line in the engine; a debugger no longer shows the
  engine's local variables, which full optimization mostly removed anyway.
- `opt-level = 0` for `syn`: 47.5 to 45.8 s. The `"*"` override optimized
  every dependency, also the ones that only run while building, inside
  derive macros (`[profile.dev.build-override]` can't undo it: a
  `package` override wins). Release builds already build them this way.

What was looked at and not changed, with the measurement:

- **Generics instantiated per call site.** `cargo llvm-lines -p ponko
  --lib` (default features): 2.53 million lines of LLVM IR in 84,500
  functions, and no hot spot: the largest function is 0.3% of the total.
  Serde's generated code is 21% of it, spread over hundreds of types
  (the largest family, the scenario steps' (de)serializers, is 2.1%); by
  module the 2D skeletons are 11% and the UI 8%. A game's own code instantiates
  the engine's generics (systems, queries), and that is part of the 0.46 s
  rebuild. Making large modules optional features would be the next step;
  that changes the default features and is left for a separate issue.
- **Proc macros.** Besides `syn`'s optimization (cut above), two major
  versions of `syn` are built. Moving `ponko_macros` to `syn` 3 would not
  remove `syn` 2: `zerocopy-derive` (through `half` and `ppv-lite86`) still
  needs it.
- **Default features.** The inspector (`egui`, `egui-wgpu`, `egui-winit`)
  is one large default feature: a 2D template with `default-features =
  false, features = ["audio", "window", "gamepad", "image-jpeg"]` builds
  clean in 41.6 s instead of 45.8 s (9% less). It stays a default (the backquote
  key opens it in any game); a game that doesn't want it can leave it out.
- **Dynamic linking of the engine** (as Bevy's `dynamic_linking`): the link
  is 0.15 s of the 0.46 s rebuild, so linking the engine as a shared
  library could save at most that. It would need a separate `dylib` crate
  wrapping the engine and a shared library shipped next to the game, so it
  isn't offered.

### Linkers

On macOS the system linker is fast already (0.15 s above). Rust 1.90 and
later link with the bundled `rust-lld` on x86-64 Linux by default. The
templates' `.cargo/config.toml` has commented-out lines for the other
cases: mold or lld on Linux (ARM, or older Rust), and `rust-lld.exe` for
Windows with the MSVC toolchain. They are what the projects of those tools
document; they were not run for this page, which has macOS numbers only.
Uncomment the block for your machine.

### Download sizes

The 2D template, one clean build each. The templates have two size
profiles: `dist` for desktop downloads (`ponko package --profile dist`) and
`dist-web` for the web (`ponko package --web --profile dist-web`, or
`web/build.sh --profile dist-web --bin <game>`):

| Desktop (macOS, arm64) | Build | Binary | gzip -9 |
|---|---|---|---|
| `release` | 36 s | 24.5 MB (19.4 MB stripped) | 8.2 MB (stripped) |
| `release` + `lto = "thin"`, `strip = true` | 38 s | 19.8 MB | 8.3 MB |
| `dist`: `lto = "fat"`, `codegen-units = 1`, `strip = true` | 2 min 18 s | **14.8 MB** | 7.1 MB |
| `dist` + `opt-level = "s"` | 1 min 39 s | 12.1 MB | 5.8 MB |

| Web (`wasm32-unknown-unknown`, after wasm-bindgen) | Build | `.wasm` | gzip -9 | brotli -q 11 |
|---|---|---|---|---|
| `release` | 39 s | 15.1 MB | 5.2 MB | 3.4 MB |
| `release` + fat LTO, one codegen unit | 1 min 58 s | 14.0 MB | 4.8 MB | 3.3 MB |
| `dist-web`: the same + `opt-level = "s"` | 1 min 36 s (with wasm-bindgen) | **10.2 MB** | 3.8 MB | 2.8 MB |
| the same with `opt-level = "z"` | 1 min 20 s | 10.2 MB | 3.6 MB | 2.7 MB |

`dist` keeps `opt-level = 3`, because a game wants its speed, and is a
quarter smaller than a stripped release build. On the web the download
matters more, so `dist-web` optimizes for size: a third smaller than
`release`, 18% smaller after brotli. `"z"` gains under 3% more after
compression. `dist-web` doesn't `strip`: wasm-bindgen reads the section
it would remove (it warns), and drops the names itself
(`--remove-name-section`). `wasm-opt` (binaryen) wasn't installed on the
measuring machine and isn't in these numbers. Panics keep unwinding in
both profiles: the engine catches a panicking job or system batch.

C and C++ games link the prebuilt `ponko_ffi` library; this section covers
Rust projects only.

## Rules the engine follows

- No heap allocation per frame in steady state on hot paths.
- Components live in tightly packed columns per archetype, so `for_each`
  compiles to plain loops the compiler can vectorize.
- Performance claims in pull requests come with numbers from release builds.

## Tips

- Use `--release`. Always.
- Prefer `for_each`/`par_for_each` over `iter()` for big queries.
- Spawn with `spawn_batch`.
- Run expensive logic at a reduced rate with `.add(phase, system.every(n))`,
  or spread per-entity work over ticks with `ctx.time.turns(n)` (see
  [The simulation clock](sim-clock.md)).

## C++

The [battle demo in C++](c-cpp.md#speed) runs at the same speed as the Rust
one for 20k units (0.57 ms/tick) and 6% slower for a million (19.97 against
18.85 ms/tick).
[C++ boids](c-cpp.md#speed) run 1M boids at 34.5 ms/tick headless, against
31.3 ms/tick for the Rust version; plain column updates are the same speed in
both (about 0.15 ms per million).
