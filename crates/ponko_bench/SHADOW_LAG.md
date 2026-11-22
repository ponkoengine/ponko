# Exact far cascades in busy scenes, and `Lighting::shadow_lag` (#85)

Since #75 the sun's far cascades are exact: a far cascade whose box is held
between frames (`Lighting::shadow_refresh` above 1) is drawn again on any
frame a caster in it changes, and on every frame while it holds a skinned
model. Before, a held far cascade kept its map until its turn came, so
moving casters there lagged by up to `shadow_refresh - 1` frames.
`Lighting::shadow_lag` brings that behaviour back by choice; exact stays the
default. These are the numbers behind that choice.

## Setup

Measured on 2026-11-22 on an Apple M4 Max (14 CPU cores, 36 GiB, Metal,
integrated GPU, driver reported as unknown), macOS 27.2.1, on AC power,
Rust 1.100.0, engine `dev` at `0840cd47` plus this change. No other build
or benchmark ran; the 1-minute load average was 1.8 to 2.9 before every
run (Spotlight indexing in the background). Release builds, headless,
offscreen.

Scene: `render/scene game-like` (the benchmark town), 1280×720, 4× MSAA,
the high preset, 4 sun cascades of 2048², shadow distance 100. It has
30,481 batched trees with two distance levels (the coarse crown from 60
units on) and buildings, 400 vehicles driving the streets and 80 skinned
walkers, spread over the whole town: every cascade holds moving and skinned
casters. 120 warm-up frames, 600 measured. Each row is 3 runs, in
alternating order; cells are the median and, in brackets, the range.

- **Moving camera**: the scene's own orbit (0.004 rad a frame, about 0.4
  units), vehicles driving, walkers walking.
- **Fixed camera**: `PONKO_SHADOW_WORKLOAD=actors`, the same actors, the
  camera still.

"Exact" is the engine after #75. "Lag" is `shadow_lag` on, which is the
engine before #75 for this purpose: a held far cascade keeps its map
whatever its casters do, and is drawn on its turn. (#75 landed inside a
large batch of unrelated changes, so the commit before it measures a
different engine; the setting reproduces the old keep rule on the same
engine instead.) At `shadow_refresh` 1 nothing is held, so lag changes
nothing.

Sun shadow passes are the GPU timers `shadows 0` to `shadows 3`, added up
over every timed frame and divided by the frames (a frame that keeps a
cascade adds 0). The point and spot lights' views (`light shadows`) are the
same in every row and not included. CPU columns are the renderer's steps,
medians in ms; "GPU" is the median of the whole frame's GPU passes.

```sh
PONKO_RENDER_SHADOW_REFRESH=2 PONKO_RENDER_SHADOW_LAG=1 \
PONKO_BENCH_JSON=lag.json \
  cargo run --release -p ponko_bench --features render -- 'render/scene game-like'
```

## Results

| Camera | `shadow_refresh` | Mode | Sun passes / frame (far) | Sun shadow GPU ms / frame (far) | GPU frame ms | CPU ms (collect / prepare / encode) | Frame ms |
|---|---:|---|---:|---:|---:|---:|---:|
| moving | 1 | exact | 4 (2) | 0.179 [0.178–0.179] (0.156) | 2.395 [2.393–2.397] | 0.096 / 0.130 / 0.385 | 3.339 [3.310–3.347] |
| moving | 2 | exact | 4 (2) | 0.301 [0.300–0.304] (0.278) | 2.487 [2.483–2.525] | 0.098 / 0.130 / 0.343 | 3.394 [3.388–3.482] |
| moving | 2 | lag | 3 (1) | 0.162 [0.161–0.162] (0.139) | 2.384 [2.384–2.384] | 0.097 / 0.129 / 0.434 | 3.349 [3.336–3.375] |
| moving | 4 | exact | 4 (2) | 0.300 [0.299–0.300] (0.277) | 2.486 [2.486–2.489] | 0.097 / 0.128 / 0.332 | 3.377 [3.376–3.397] |
| moving | 4 | lag | 2.5 (0.5) | 0.092 [0.092–0.092] (0.070) | 2.319 [2.317–2.322] | 0.097 / 0.125 / 0.402 | 3.259 [3.245–3.278] |
| fixed | 1 | exact | 2 (2) | 0.168 [0.165–0.169] | 2.450 [2.450–2.453] | 0.080 / 0.092 / 0.385 | 3.299 [3.293–3.311] |
| fixed | 2 | exact | 2 (2) | 0.342 [0.341–0.343] | 2.541 [2.540–2.547] | 0.080 / 0.092 / 0.388 | 3.392 [3.388–3.424] |
| fixed | 2 | lag | 1 (1) | 0.166 [0.165–0.166] | 2.464 [2.463–2.470] | 0.080 / 0.091 / 0.369 | 3.318 [3.314–3.319] |
| fixed | 4 | exact | 2 (2) | 0.341 [0.340–0.342] | 2.542 [2.539–2.542] | 0.080 / 0.091 / 0.395 | 3.396 [3.390–3.400] |
| fixed | 4 | lag | 0.5 (0.5) | 0.082 [0.081–0.083] | 2.331 [2.324–2.332] | 0.080 / 0.091 / 0.365 | 3.172 [3.156–3.189] |

With the camera fixed the near cascades hold nothing that moves and are
kept, so every sun pass is a far one. These runs used the cell band test
below; in this scene no tree cell lies across the trees' band edge near
the camera, and the per-instance test drew the same passes (checked with
`PONKO_SHADOW_WORKLOAD=slow-camera` on both: 2.183 sun passes a frame).

## What it says

- **Exact far cascades cost more than a few percent of the frame here.**
  At `shadow_refresh` 2, lag saves 0.14 ms (moving camera) and 0.18 ms
  (fixed) of sun shadow GPU time, 5.6% and 7.0% of the GPU frame; at 4 it
  saves 0.21 and 0.26 ms, 8.4% and 10%. The whole-frame median drops in
  every pair, with the ranges apart: -0.05 and -0.07 ms at refresh 2,
  -0.12 and -0.22 ms at 4 (moving, fixed). So `shadow_lag` is worth having, and stays off by
  default: it is a choice to trade exactness for those passes.
- **Exact cascades held between frames cost more than tight ones.** In
  exact mode every far cascade here is drawn every frame (the walkers are
  skinned), at any `shadow_refresh`. Above 1 they are drawn into boxes a
  little bigger than they need, which hold more trees: 0.30 ms instead of
  0.18 ms (moving), 0.34 instead of 0.17 (fixed). In a scene like this one,
  `shadow_refresh` 1 is cheaper than the default 2 unless `shadow_lag` is
  on. The guide says so.
- **CPU.** Collect and prepare don't move (the casters are listed for every
  cascade either way). Encoding with lag and a moving camera is 0.06 to
  0.09 ms slower than exact (ranges apart; fixed camera: within the noise or
  faster). Fewer passes change how the frame's recording is split into
  parallel parts; we did not pin it down further. It is less than the GPU
  time saved, and the frame is faster.

## Instance batches: the band test per instance

A shadow map is kept while the camera moves only if no instance batch cut
by distance (`InstanceLevel::min_distance` / `max_distance` with shadows)
can draw something else from the new eye. That used to be decided per
instance cell: a cell whose box lay across a band edge counted as changed
on every camera move, however far its instances were from the edge. Now
such a cell is tested instance by instance, with the distance the culler
uses (the compute pass's and the CPU path's alike), so a map is drawn again
only when an instance actually crosses an edge. A batch cut by distance for
its shadows keeps its instances' positions on the CPU for this on the GPU
path too (16 bytes an instance; the CPU path had them already).

Workloads: `PONKO_SHADOW_WORKLOAD=vegetation`, the same town without its
walkers, the camera circling low (6 units up) among the 30,481 trees
outside the town, 0.00001 rad a frame (about 0.002 units), actors still:
the trees' band edge, 60 units away, lies all around the camera. And
`vegetation-moving`, the same with the camera at the usual 0.004 rad a
frame (about 0.7 units): the worst case for the new work, as instances
cross the edge on most frames. "Cell" is this branch with the old cell
test (`src/render/gpu3d/instances.rs` from `0840cd47`, everything else
the same), "instance" this branch. Same machine and method as above, 3
runs each, alternating binaries.

| Workload | `shadow_refresh` | Test | Sun passes / frame (far) | Sun shadow GPU ms / frame | GPU frame ms | CPU ms (collect / prepare / encode) | Frame ms |
|---|---:|---|---:|---:|---:|---:|---:|
| vegetation | 1 | cell | 4 (2) | 0.381 [0.378–0.384] | 1.800 [1.798–1.810] | 0.053 / 0.077 / 0.369 | 2.607 [2.606–2.628] |
| vegetation | 1 | instance | 0.825 (0.295) | 0.063 [0.062–0.063] | 1.523 [1.517–1.545] | 0.054 / 0.091 / 0.255 | 2.281 [2.256–2.332] |
| vegetation | 2 | cell | 4 (2) | 0.427 [0.426–0.427] | 1.850 [1.850–1.852] | 0.053 / 0.077 / 0.372 | 2.686 [2.663–2.689] |
| vegetation | 2 | instance | 0.815 (0.285) | 0.068 [0.068–0.068] | 1.504 [1.494–1.507] | 0.054 / 0.092 / 0.254 | 2.251 [2.250–2.263] |
| vegetation-moving | 1 | cell | 4 (2) | 0.531 [0.531–0.533] | 2.378 [2.369–2.379] | 0.067 / 0.102 / 0.404 | 3.275 [3.268–3.277] |
| vegetation-moving | 1 | instance | 4 (2) | 0.525 [0.525–0.536] | 2.367 [2.357–2.416] | 0.068 / 0.105 / 0.403 | 3.261 [3.249–3.340] |
| vegetation-moving | 2 | cell | 4 (2) | 0.573 [0.572–0.574] | 2.419 [2.415–2.423] | 0.068 / 0.106 / 0.410 | 3.319 [3.318–3.331] |
| vegetation-moving | 2 | instance | 4 (2) | 0.575 [0.564–0.576] | 2.411 [2.403–2.415] | 0.067 / 0.105 / 0.407 | 3.316 [3.311–3.321] |

With the slow camera, the cell test drew every cascade on every frame;
testing instances keeps 80% of the sun's passes, takes 0.32 to 0.36 ms
off the sun shadow GPU time and 0.33 to 0.44 ms off the frame. Prepare
takes 0.014 ms longer for the instance tests; encoding fewer passes saves
0.11 ms. With the camera at the usual speed instances cross the edge on
nearly every frame, so every cascade is drawn either way; the instance
tests then cost nothing measurable (every column within the noise).
