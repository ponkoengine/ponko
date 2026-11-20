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
- **Gerstner waves** for the sea, with a CPU query of the same waves for
  buoyancy and gameplay.
- An **underwater** screen effect when the camera dips below the surface.
- **Contact ripples** where things touch the water.
- **Buoyancy** for 3D physics bodies (the `physics3d` feature).

Run `cargo run --release --example water --features physics3d` to fly
over a lake on a plateau, a river carrying crates down to a beach, and
the sea: 1, 2 and 3 pick the quality, R the lake's reflection, Space
drops a crate.

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
in a small compute simulation over a square of `ripple_area` metres
around the water's centre; three contacts start per frame and the rest
wait their turn. Contact ripples need compute shaders, so on WebGL2 the
pass isn't made and `touch` does nothing.

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
On WebGL2 everything but contact ripples works; the flat-sky reflection
uses the frame's sky colour times the ambient light, as the rest of the
renderer does without a sky shader.

## From C and C++

`PonkoWaterDesc` is a versioned descriptor: fill it with
`ponko_water_desc_lake`, `_river` or `_sea`, change fields (up to
`PONKO_WATER_MAX_WAVES` waves, a flow map as RGBA8 texels, which is
copied) and spawn it with `ponko_ctx_spawn_water`. `ponko_game_water`
adds the plugin; `ponko_ctx_water_sample`, `ponko_ctx_water_touch` and
`ponko_ctx_despawn_water` do the rest. C++ wraps them as
`ponko::water_desc::sea(...)`, `ctx.spawn_water`, `ctx.water_height` and
`game.water()`. `ponko_ctx_set_buoyancy_box` and `_sphere` float boxes
and balls in fresh water; custom sample points (`Buoyancy::points`), the
water's density and the ripple switch are Rust-only for now.
