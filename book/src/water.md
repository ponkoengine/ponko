# Water: lakes, rivers and seas

`ponko::water` (the `3d` feature) draws water and floats things on it.
Pick a preset (`WaterDesc::lake`, `river` or `sea`), change what you like
and spawn it: the kit builds the mesh, the material and any passes the
water needs, and `WaterPlugin` keeps them up to date each frame.

- **Reflections**: the sky, screen-space (marched over the depth buffer
  in the water's own shader), or a planar mirror camera.
- **Refraction** of what is under the surface, with **absorption** by
  depth: shallow water is clear, deep water takes the water's colour.
- **Shore foam** where the water is shallow, and whitecaps on sharp crests.
- **Flow maps** for rivers: the surface detail drifts with the current.
- **Gerstner waves** for the sea, or **waves from a spectrum** (an FFT
  ocean), with a CPU query of the same waves for buoyancy and gameplay.
- An **underwater** screen effect when the camera dips below the surface.
- **Contact ripples** where things touch the water.
- **Buoyancy** for 3D physics bodies (the `physics3d` feature).

Run `cargo run --release --example water --features physics3d` to fly
over a lake on a plateau, a river carrying crates down to a beach, and
the sea: 1, 2 and 3 pick the quality, R the lake's reflection, Space
drops a crate. The terrain example (`examples/terrain`) fills its
valleys with a water-kit lake: a planar mirror of the hills, and the
underwater view when the camera dives in (U toggles it).

## Bodies of water

A `WaterDesc` is plain data. Its presets:

| Preset | What it is |
|---|---|
| `WaterDesc::lake(center, size, level)` | still water over an area: light ripples, screen-space reflections, shore foam, contact ripples |
| `WaterDesc::river(center, size, level, flow)` | a current of `flow` m/s; `with_flow_map` for one that bends |
| `WaterDesc::sea(level, wind, height)` | six Gerstner waves around `wind`, about `height` metres trough to crest; its mesh follows the camera to the horizon |

<div class="lang-pair">
<div>

```rust
{{#include ../../examples/water/game.rs:bodies}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:water_scene}}
```

</div>
</div>

Spawn one with `Water::spawn(ctx, desc)` (`try_spawn` returns the error
when the GPU can't make its textures) and add `WaterPlugin` to the game.
`despawn_water(ctx, entity)` frees everything it made. To change the
quality or the reflection, despawn the water and spawn it again; the
example does that on its keys.

The look comes from a few fields: `absorption` (how much of each colour
the water swallows per metre: red goes first in real water), `scatter`
(the colour of deep water), `foam` and `shore_foam` (how much foam, and
how deep the water is where shore foam ends) and `refraction`.

A `FlowMap` holds one velocity per texel over the water's area, from a
function (`FlowMap::from_fn`) or from RGBA8 texels as an image holds them
(`FlowMap::from_rgba`: red is the speed along x, green along z, 128 is
still). The shader slides two layers of surface detail downstream, each
restarting every two seconds while the other shows, so the detail never
stretches; floating bodies drift with the current too.

## Waves on the CPU

The sea's `Waves` are evaluated by the shader and by the CPU from the
same table, so what floats rides the waves you see. `Water::height_at`
and `Water::sample` (height, normal, velocity and how squeezed the
surface is) take a ground point and a time:

```rust
# use ponko::prelude::*;
# use ponko::water::{Water, WaterDesc};
# Game::new("sea", 320, 180).run_headless(0, |ctx| {
let sea = Water::spawn(ctx, WaterDesc::sea(0.0, vec2(1.0, 0.0), 1.2));
let water = ctx.world.get::<Water>(sea).unwrap();
let t = ctx.time.elapsed() as f32;
let under_the_boat = water.sample(vec2(12.0, -3.0), t);
assert!(under_the_boat.height.abs() < 1.2);
# });
```

The shader draws the waves at `ctx.time.elapsed()`; buoyancy uses the
simulation's time. A GPU test reads the drawn surface back and compares
it with `height_at`: they agree to within 1 cm (5.8 mm worst over the
probes on the test machine).

Far away, waves and ripples shorter than a few pixels fade out of the
normal so the sea doesn't shimmer; the heights the CPU sees don't change.

## Waves from a spectrum

`Waves::spectrum` is the other kind of sea: instead of six Gerstner
waves, thousands of waves of every length, from a wind-driven (Phillips)
spectrum turned back into a surface by an inverse FFT each frame. The
surface is a square of `tile` metres that repeats, worked out on a grid
of 64, 128 or 256 cells; its waves run with the wind and the crests get
sharper with `choppiness`. A seed picks the sea, so a seed always gives
the same waves on every machine, and the sea loops every
`SPECTRUM_PERIOD` (200 s) so the shader's numbers stay small however
long a game runs.

<div class="lang-pair">
<div>

```rust
# use ponko::prelude::*;
# use ponko::water::{Water, WaterDesc, WaveSpectrum, Waves};
# Game::new("sea", 320, 180).run_headless(0, |ctx| {
let storm = WaveSpectrum::new(vec2(12.0, 4.0), 2.5) // wind in m/s, height in m
    .with_tile(256.0, 128)
    .with_choppiness(1.2)
    .with_seed(7);
let sea = Water::spawn(ctx, WaterDesc::sea(0.0, vec2(12.0, 4.0), 2.5).with_waves(Waves::spectrum(storm)));
let water = ctx.world.get::<Water>(sea).unwrap();
let height = water.height_at(vec2(30.0, -8.0), ctx.time.elapsed() as f32);
assert!(height.abs() < water.desc().waves.max_height());
# });
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:water_spectrum}}
```

</div>
</div>

`height` is the significant wave height (trough to crest of the highest
third, four times the heights' spread). The wind's speed sets how long
the biggest waves are (about `0.9 · speed²` metres): a square much
smaller than that leaves them out.

Where it is worked out:

- **With compute shaders** (every native backend and WebGPU), compute
  passes on the GPU: one turns the spectrum to the frame's time, then one
  per FFT stage along x and along z (13 passes at 64 cells, 17 at 256).
- **Without** (WebGL2), the CPU works out the same field and uploads it
  each frame. Its FFT is [rustfft](https://crates.io/crates/rustfft)'s,
  which uses the CPU's SIMD (NEON, SSE or AVX) where there is some, and
  WASM SIMD in a web build made for it (see [On the web](#on-the-web)).
- **The CPU's queries** (`height_at`, `sample`, buoyancy) run the same
  FFT on the CPU at the time asked, once per moment (the last two are
  kept), so what floats rides the waves that are drawn. A GPU test reads
  the drawn surface back and compares it with `height_at`: 9.9 mm worst
  with the GPU's FFT, 9.1 mm with the CPU's upload, on a 1.5 m sea; the
  GPU's field matches the CPU's to 6 mm (it runs in half floats).

Cost, measured on an Apple M4 Max (Metal) while the machine was busy
with other work (load average 70 to 116). The GPU column is from
`spectrum_waves_cost` in the engine's tests, over eleven runs in three
sessions; the CPU columns from `cpu_field_cost`, the median of 15 new
moments in each of five runs, one thread:

| Cells | GPU time over a Gerstner sea (1280x720) | One CPU field | With the sway's speed (buoyancy) |
|---|---|---|---|
| 64 | 0.57 to 0.76 ms | 0.03 to 0.08 ms | 0.05 to 0.06 ms |
| 128 (default) | 0.74 to 0.89 ms | 0.13 to 0.38 ms | 0.21 to 0.54 ms |
| 256 | 0.88 to 1.02 ms | 0.58 ms | 0.9 to 1.2 ms |

The GPU column is the whole frame's GPU time against the same sea with
`Waves::sea` (the copies between FFT passes aren't timed). The CPU
columns are paid in a frame only when something asks the CPU (a WebGL2
upload, a query, a floating body), once per moment however many points
are asked; the underwater check skips the query while the camera is
clearly above or below every wave. The CPU's field used to be a plain
radix-2 FFT, worked out output by output as the GPU's passes do: in the
same runs it took 0.14 to 0.42 ms at 64 cells, 0.63 to 0.80 ms at 128 and
2.9 ms at 256 (4.3 to 9 ms with the sway's speed): the field is now
about five times cheaper. Splitting it across worker threads was
tried and left out: on a busy machine waiting for the threads cost more
than the whole field (0.4 to 40 ms at 256 cells).

On a desktop any size is cheap enough for buoyancy, even 256 cells (about
a millisecond a moment with the sway's speed).

### On the web

In a browser the CPU's FFT runs on the page's one thread, and on WebGL2 it
runs every frame (the upload). `web/smoke/spectrum.mjs` times it in a
headless browser with the fixture `examples/web_spectrum.rs`: one new
moment, and one with the sway's speed, at each size, in three builds. In
Chromium 156 (headless) on the same Apple M4 Max, the machine busy with
other work (load average 80 to 135), the median of three page loads of
fifteen readings each, over two sessions (the ranges cover both):

| Cells | One moment | With the sway's speed | One moment, WASM SIMD | With the sway's speed, WASM SIMD |
|---|---|---|---|---|
| 64 | 0.06 ms | 0.09 ms | 0.04 ms | 0.06 ms |
| 128 (default) | 0.24 to 0.25 ms | 0.35 to 0.36 ms | 0.16 to 0.17 ms | 0.24 to 0.26 ms |
| 256 | 1.10 to 1.14 ms | 1.65 to 1.70 ms | 0.70 to 0.74 ms | 1.04 to 1.07 ms |

The ordinary web build takes up to about twice the desktop's time above
(which has NEON).
A WebGL2 frame pays one moment for the upload, and one with the sway's
speed as well when something floats or asks `sample` at that time, so the
default 128 cells cost about 0.6 ms of a 16.7 ms frame at worst, and 256
cells about 2.8 ms.

**128 cells is the web's default and the cheapest correct choice**: it is
cheap enough on any build, and it is what the GPU path draws, so the same
seed gives the same sea on every machine. Halving the grid (64 cells)
saves under half a millisecond and drops the shortest waves; 256 cells is
fine on a fast machine but costs about 2.8 ms (1.8 ms with WASM SIMD) a
frame on WebGL2. There is no update-rate setting: the field is worked out
at the drawn frame's time, once per moment however many queries ask, and
an older moment would leave floating bodies off the drawn waves.

**WASM SIMD** is optional. Build with the `wasm-simd` feature and
`simd128` for the whole module:

```sh
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+simd128" \
  web/build.sh --features 3d,wasm-simd my_game
```

That makes a moment 1.4 to 1.6 times faster (0.08 ms saved at 128 cells,
0.4 to 0.6 ms at 256), and the module then needs a browser with
fixed-width SIMD (Chrome 91, Firefox 89, Safari 16.4). Each half alone is
no good, which is why the feature refuses to compile without the flag:
`simd128` alone changed nothing (0.99 to 1.05 times), and rustfft's SIMD
code alone made a moment three times slower (0.99 ms instead of 0.24 at
128 cells), its SIMD functions called from code built without SIMD. The
default build leaves both off, for older browsers.

## Floating bodies

Give a 3D physics body a `Buoyancy` (`Buoyancy::cuboid(size)`,
`Buoyancy::sphere(radius)`, or your own sample points with
`Buoyancy::points`) and `WaterPlugin` pushes it up by the water it
displaces at each fixed step, and drags it along with the water's motion
(waves and current). A body half as dense as water settles half under
and follows the waves; the result is deterministic on the fixed step.

```rust
{{#include ../../examples/water/game.rs:crate}}
```

`drag` (4 by default) is how quickly the water pulls a body along: the
share of the velocity gap it closes per second for a floating body.
Bodies crossing the surface start contact ripples (`ripples: false`
turns that off), and `submerged` says how much of the body was under
water at the last step.

## Contact ripples

`Water::touch(at, radius, strength)` starts a ring of ripples. They run
in a small simulation (a damped wave equation on a grid) over a square of
`ripple_area` metres around the water's centre; three contacts start per
frame and the rest wait their turn. With compute shaders a compute pass
steps it on the GPU. Without them (WebGL2) the CPU steps the same
simulation and uploads the grid while anything moves (128 x 128 texels
at `Medium`), and nothing while the water is still; a GPU test checks
the CPU leaves the grid the compute pass does, to 0.002 of a ring 0.12
high.

## Quality and cost

| Quality | Mesh cells | Detail octaves | Refraction | Reflection steps | Planar mirror | Ripple texels |
|---|---|---|---|---|---|---|
| `Low` | 48 | 1 | off | sky only | ¼ of each side | 64 |
| `Medium` (default) | 96 | 2 | on | 12 | ½ of each side | 128 |
| `High` | 160 | 3 | on | 24 | ½ of each side | 256 |

GPU time added by a 1280x720 frame of open sea, against the same frame
without water, measured with timestamp queries (median of three runs,
each the median of its frames; the spread is the three runs):

| Preset | Added GPU time | Runs |
|---|---|---|
| `Low` | 0.68 ms | 0.681–0.682 ms |
| `Medium` | 2.66 ms | 2.60–2.68 ms |
| `High`, screen-space reflection | 3.83 ms | 3.69–4.95 ms |
| `High`, planar mirror | 2.04 ms | 2.00–2.05 ms |

These numbers are to be repeated on a quiet machine: they were taken on
an Apple M4 Max (Metal) under heavy load from other work (load average
90 to 180), which shows in the spread of the `High` row. The planar
mirror looks cheaper only because the test scene is a few boxes: it
draws the scene a second time, so its cost grows with the scene, while
screen-space reflections cost the same whatever is on screen. Measure
with `cargo test --release --features 3d --lib water_quality_gpu_cost --
--ignored --nocapture`.

## Free when unused

The kit makes nothing until you spawn water: no passes, textures or
shaders. A GPU test checks that a game running the kit's update with no
water draws the same image with the same draw calls, passes and GPU
memory as a game without it, and that the kit made no render target,
compute pass, effect or shader. A headless test checks that despawning
water, or remaking it at another quality, leaves none of its textures,
shaders or passes behind.

## Backends and the web

The water's shaders are checked for every native backend (with and
without multisampling) and translated to GLSL ES 3.0 for WebGL2 in tests.
On WebGL2 everything works: contact ripples and waves from a spectrum
are worked out on the CPU and uploaded (see above), and the flat-sky
reflection uses the frame's sky colour times the ambient light, as the
rest of the renderer does without a sky shader. GPU tests run both on a
device limited to what WebGL2 allows.

## From C and C++

`PonkoWaterDesc` is a versioned descriptor: fill it with
`ponko_water_desc_lake`, `_river` or `_sea`, change fields (up to
`PONKO_WATER_MAX_WAVES` waves, a flow map as RGBA8 texels, which is
copied) and spawn it with `ponko_ctx_spawn_water`. `ponko_game_water`
adds the plugin; `ponko_ctx_water_sample`, `ponko_ctx_water_touch` and
`ponko_ctx_despawn_water` do the rest. C++ wraps them as
`ponko::water_desc::sea(...)`, `ctx.spawn_water`, `ctx.water_height` and
`game.water()`. `ponko_ctx_set_buoyancy_box` and `_sphere` float boxes
and balls in fresh water. Everything else about a floating body is a
`PonkoBuoyancyDesc`, a versioned descriptor: `ponko_buoyancy_desc_box`,
`_sphere` or `_points` fills it (your own sample points, up to
`PONKO_BUOYANCY_MAX_POINTS`, are copied into it), then change the
water's density (`PONKO_FRESH_WATER_DENSITY` from the presets), the
drag or the ripple switch and give it to the body with
`ponko_ctx_set_buoyancy`; `ponko_ctx_buoyancy` reads it back. C++ starts
from `ponko::buoyancy_desc::box`, `sphere` or `from_points`:

```cpp
{{#include ../../examples/cpp/guide.cpp:buoyancy_desc}}
```

The ripples that wait for the water's next update (Rust
`Water::waiting_contacts`) are `ponko_ctx_water_waiting_contacts` (C++
`ctx.water_waiting_contacts(water)`). Each update takes three; a body whose
ripple switch is off starts none, so a test sees the switch by its effect.
The update drains the waiting ones every frame, so to ask only whether any
started, read the running total (Rust `Water::contacts_started`,
`ponko_ctx_water_started_contacts`, C++ `ctx.water_started_contacts(water)`)
in a system that runs after physics, once with the switch on and once with it
off: no maximum over the ticks needed.

A sea from
a spectrum is `PonkoWaterDesc` version 2: set `has_spectrum` and fill
`spectrum`, a `PonkoWaveSpectrum` that `ponko_wave_spectrum(wind,
height)` starts with the engine's defaults (C++ `ponko::wave_spectrum`
and `water_desc::with_spectrum`). A program built against version 1 keeps
its Gerstner waves.
