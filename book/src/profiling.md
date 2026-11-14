# Performance and profiling

Fast code starts with measuring. This chapter is the loop to follow when a
game is slow, or before you've decided whether it is: **measure, read the
report, fix the one biggest thing, measure again.** Everything here is built
in; you need no outside tools. The [performance chapter](performance.md) has
the engine's own benchmark numbers and the frame-level tools (the overlay,
GPU passes, traces).

## 1. Measure

A frame takes about as long as the slower of the CPU (your systems, then
recording the draws) and the GPU. Three things are on offer, from cheapest to
most detailed:

| You want | Use |
|---|---|
| a quick look while playing | the overlay: `PONKO_HUD=1`, or `ctx.perf.set_hud(true)` |
| to know *which system or part of it* is slow, and what allocates | scopes and `ctx.perf.report()` |
| a number for a test or CI that fails when things get slower | `Game::profile_headless` and a `Budget` |

The overlay and `PONKO_BENCH` switch the deeper profiling on by themselves.
In your own code, `ctx.perf.set_profiling(true)` does. While it is **off**, a scope
costs a single atomic load and the allocation counter a single load per
allocation, so leaving scopes in shipped code is fine.

### Time your own code: scopes

Every system is timed automatically. To see *inside* one, wrap the parts you
suspect:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/perf_report.rs:profile_scopes}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:profile_scopes}}
```

</div>
</div>

`profile_scope!("name")` (C++: `PONKO_PROFILE_SCOPE("name")`) times the rest
of the block. `let _timing = ctx.perf.scope("name");` times until the guard
is dropped, for a name made at run time. Scopes nest
(times are inclusive), they work on any thread, and times from several threads
add up. A scope named `"steering/neighbors"` sorts next to its parent in the
report.

### Count allocations

An allocation in code that runs every frame is a common reason for stutter.
To count them, install the counting allocator once, in your `main.rs`:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/alloc_counting.rs:counting_allocator}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:count_allocations}}
```

</div>
</div>

Counting happens only while profiling is on. The report then says how many
allocations each frame and each system makes. In C and C++ the engine's own
allocations are counted too; `PONKO_COUNT_CPP_ALLOCATIONS` adds your `new`
calls. (A C game can call `ponko_note_allocation(bytes)` from its own
allocator.)

### In a running game

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/perf_report.rs:profile_live}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:profile_live}}
```

</div>
</div>

Or without changing the game: `PONKO_BENCH=600 cargo run --release` plays a
fixed run offscreen and prints the same report at the end
(`PONKO_BENCH_REPORT=now.json` also saves it); the
[MCP server](agents.md)'s `perf_report` tool takes `profiling=true` for an
agent to do the same on a running game.

### In a test or CI, with no window

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/perf_report.rs:profile_headless}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:profile_headless}}
```

</div>
</div>

`profile_headless(warmup, ticks, setup)` runs the simulation like
`run_headless`, but times every tick and returns the report with the world.
The first `warmup` ticks aren't counted: buffers grow and caches fill.

## 2. Read the report

`println!("{report}")` prints this (shortened):

```text
== performance report: 200 frames, 100 entities ==
frame ms: avg 0.052  p50 0.049  p95 0.061  p99 0.090  max 0.412
allocations per frame: 3.0 (0.2 KB), at most 3
-- systems (avg ms, max ms, allocs/frame) --
   0.047    0.390      3.0  my_game::steering
-- scopes (avg ms, max ms, calls/frame) --
   0.040    0.380      1.0  steering
   0.030    0.350      1.0  steering/neighbors
-- slowest frames --
frame 143: 0.41 ms, 3 allocations  [my_game::steering 0.39, steering 0.38, steering/neighbors 0.35]
-- warnings --
! `my_game::steering` allocates 3 times per frame
    -> Keep the buffer between frames instead: a Vec in a resource ...
```

- **`frame ms`**: the average says how fast it is, **p99 and max** say how
  smooth. A p99 much higher than p50 is stutter: the average hides it.
- **systems**: where the CPU time goes, slowest first. A system's time covers
  everything it did, scopes and allocations included. Scopes sit *inside*
  systems; don't add them up with the systems.
- **allocs/frame**: anything above zero in per-frame code deserves a look.
  Counts, unlike times, are the same on every machine.
- **slowest frames**: what was running in the worst frames. If these share a
  system or scope, that's your hitch.
- **render** (windowed games): CPU time in systems and drawing, GPU time, draw
  calls and triangles for the camera and the shadows, and GPU time per pass.
  Display wait is idle time, not work.
- **warnings**: the report spots common problems (an allocating system, one
  system taking over half the time, spikes, GPU or CPU bound, heavy shadows,
  thousands of draw calls) and says what to try.

The same report is data: `report.systems`, `report.scopes`,
`report.frame_ms.p99`, `report.allocs` and so on (C++: parse
`report.json()`).

## 3. Budgets and comparing runs

A **budget** turns "it should stay fast" into a failing test. Limits are
optional; anything you leave out isn't checked:

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# let report = ponko::perf::PerfReport::default();
let budget = ponko::perf::Budget::new()
    .frame_p99_ms(4.0)
    .allocs_per_frame(0.0)
    .system("steering", 0.5)
    .scope("steering/neighbors", 0.3);
report.assert_within(&budget); // panics, listing what broke
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:budget_check}}
```

</div>
</div>

Timings differ between machines and runs, so give time limits headroom (twice
what you see on your machine), or use them to catch only big regressions.
**Allocation counts are exact**: `allocs_per_frame(0.0)` on a system that
should never allocate is the budget to rely on.

### GPU memory

A windowed game's report also says how much GPU memory the engine holds:
`report.gpu_memory` (`None` in headless runs, which open no GPU). Every
texture and buffer the renderer keeps is counted once, at the size the
engine asked for, in one of ten classes: `textures` (images, render targets,
texture arrays, glyph atlases), `meshes`, `gpu_buffers`, `frame_targets`
(scene color and depth, post-processing, ambient occlusion, reflections,
temporal history), `shadows` (the sun's cascades), `atlases` (the point and
spot lights' shadow atlas), `skinning`, `probes` (global illumination and
image-based lighting), `instance_batches` and `other`, with their `total`.
The text report prints it on one line, largest class first, and the
overlay shows the total and the three largest classes. It is counted again
about twice a second while profiling is on, never every frame.

Where the platform reports its own figure, `adapter_bytes` has it (Vulkan
and DirectX 12: wgpu's allocator; Metal: the device's allocated size) and
`adapter_note` says which; elsewhere (OpenGL, the browser) it is `None`,
never a guess. Expect the driver's figure to be a little higher: it lays
textures out in its own tiles and wgpu keeps some memory for itself. On an
Apple M4 Max at 720p the count was within 2-4% of Metal's figure at a render
scale of 1, and 9% at the Low preset's two-thirds scale, whose odd sizes
tile worst.

A quality preset's cost is the difference between two reports, and a budget
keeps it in check (in mebibytes; a report without GPU memory breaks it as
"not measured", never as 0):

```rust
# use ponko::prelude::*;
# let report = ponko::perf::PerfReport::default();
if let Some(memory) = &report.gpu_memory {
    println!("{memory}"); // 261.4 MB (adapter 269.8 MB): frame_targets 120.0, shadows 64.1, ...
    println!("shadow maps: {} bytes", memory.shadows);
}
let budget = ponko::perf::Budget::new().gpu_memory_mb(512.0);
# let _ = report.check(&budget);
```

C and C++ get the same in the report's JSON (`"gpu_memory"`) and budgets
(`{"gpu_memory_mb": 512}`).

To catch regressions without picking numbers, keep a report from a good run
and compare against it:

```rust
# use ponko::prelude::*;
# let (_w, report) = Game::new("t", 1, 1).profile_headless(0, 5, |_| {});
report.save("target/perf-now.json").unwrap();
if let Ok(before) = ponko::perf::PerfReport::load("perf-baseline.json") {
    for slower in report.regressions(&before, 0.25) {
        println!("slower: {slower}"); // e.g. system `my_game::steering` (ms): 0.047 -> 0.081
    }
}
```

(C++: `perf_report::save`, `load` and `regressions`.) A value counts when it
grew by more than the tolerance and by a little in absolute terms, so noise in
tiny numbers is ignored. GPU memory is compared too when both reports have it.

## 4. Common fixes

**Allocates every frame.** Keep buffers between frames: a `Vec` held in a
resource (or captured by the system), `clear()`ed and refilled. Avoid
`format!`, `to_string`, `collect`, `Box` and `Vec::new` + `push` in code that
runs each tick. C++: reuse a `std::vector` and `reserve`.

**One system dominates.** Wrap its halves in scopes to see which. Then, in
order of effort: iterate with `for_each` / `par_for_each` instead of
`iter()` ([ECS](ecs.md)); do the work for a quarter of the entities per tick
with `ctx.time.turns(4)`; run it less often with an interval, `system.every(n)`
([The simulation clock](sim-clock.md)); use a spatial grid or flow field
instead of checking every pair ([Spatial queries](spatial.md)).

**Spikes.** Look at *slowest frames*: the same system each time usually means
work that sometimes does a lot (a path search, a rebuild, spawning a wave).
Spread it over ticks, or do it ahead of time. Allocation bursts, asset loads
and spawning thousands of entities at once show up the same way.
`ctx.perf.record_trace(path, frames)` shows a slow frame on a timeline in
[Perfetto](https://ui.perfetto.dev).

**GPU bound** (GPU time close to the frame time, display wait large). The
pass list says which pass. Shadows: fewer casters, a simpler
[`ShadowMesh`](3d.md), `set_mesh_shadows(false)` for small things. Pixels:
`ctx.scene3d.post.msaa = 2`, fewer or smaller things drawn. Triangles: simpler meshes far
away. Many draw calls: pack sprites into an atlas, share meshes and
materials.

**CPU bound in drawing.** Compare *draw calls* and *models* with what's
really visible; cull and merge what you can.

When something is still slow after this: `sample <pid> 5` on macOS or a
profiler for the native code, or `--features profiling` with
[`puffin_viewer`](https://github.com/EmbarkStudios/puffin) for a live
flame graph of the systems.

## What isn't measured yet

- GPU memory use and per-mesh memory.
- Rust allocations on other threads are counted for the whole process, so a
  system is charged for what rayon workers allocate during it.
- Scopes are process-wide: two games running side by side in one process
  (only tests do that) would see each other's scopes.
