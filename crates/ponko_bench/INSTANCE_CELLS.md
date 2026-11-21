# Instance cells: streaming one cell at a time against making the batch again

What it costs to stream the instances of an open world cell by cell
(`Graphics::add_instance_cell`, `Ctx::free` on an `InstanceCell`), against
keeping them in one `InstanceBatch` that is made again whenever a cell comes
or goes.

## How to run it

```sh
cargo test --release --lib --features 3d instance_cells_benchmark -- --ignored --nocapture
```

`PONKO_CELLS_BENCH=draws` runs only the draw table below, `stream` only
the streaming one.

The benchmark (`src/render/instance_cells_bench.rs`) opens no window: it
draws into an offscreen 1280x720 texture with the real 3D renderer (4x MSAA,
sun shadows with four cascades) and waits for the GPU after every frame.

## The scene

- Cells 32 m wide of 1,024 cubes each (a jittered grid 1 m apart), loaded
  within 160 m of the camera and dropped beyond 200 m by a `CellStreamer`:
  about 120 cells (120,000 instances) live at once.
- The camera flies 2 m a frame, so a new row of cells is due every 16
  frames; at most 2 cells load a frame (a game's upload budget).
- 60 warm-up frames, then 600 measured frames, per row.
- **cells**: each loaded cell is added on its own, each dropped one freed.
  **one batch, made again**: in a frame where any cell came or went, the
  batch is freed and made again from every live cell's instances.
- Both rows of a path use the path forced (`GPU` = compute culling and
  indirect draws, `CPU` = cell culling and direct draws).
- **upload ms per cell** is the game thread's time adding and freeing (or
  making the batch again), divided by the cells that came or went that
  frame: building the cell's clusters and handing it to the renderer,
  which writes its records. **frame ms** is that plus preparing, recording
  and submitting the frame, until the GPU finished it.

## Results

Apple M4 Max, Metal (wgpu 30.0.1), macOS; rustc 1.99.1, release profile,
features `3d` plus the defaults; revision `b688f902`. 2026-11-15.

The machine was shared with several other builds (load average 88 to 105
during the runs), so frame times moved by up to a factor of two between the
two runs below. Only the upload column is steady enough to compare.

Run 1 (load average 88):

| path | streamed as | live cells | upload ms per cell (p50 / p95 / max) | frame ms (p50 / p95 / p99 / max) | frames that changed cells, ms (p50 / p99) |
|---|---|---|---|---|---|
| GPU | cells | 122 | 0.068 / 0.161 / 0.352 | 5.24 / 19.10 / 25.81 / 29.79 | 5.33 / 24.63 |
| GPU | one batch, made again | 122 | 2.897 / 5.649 / 8.544 | 12.63 / 33.77 / 41.88 / 55.24 | 16.12 / 43.74 |
| CPU | cells | 122 | 0.072 / 0.160 / 0.293 | 4.98 / 25.09 / 31.82 / 52.88 | 5.26 / 30.73 |
| CPU | one batch, made again | 122 | 2.026 / 2.401 / 4.718 | 7.65 / 12.04 / 20.24 / 36.08 | 8.74 / 21.93 |

Run 2 (load average 97 to 105):

| path | streamed as | live cells | upload ms per cell (p50 / p95 / max) | frame ms (p50 / p95 / p99 / max) | frames that changed cells, ms (p50 / p99) |
|---|---|---|---|---|---|
| GPU | cells | 122 | 0.083 / 0.158 / 0.352 | 9.73 / 30.63 / 35.40 / 47.22 | 10.33 / 34.03 |
| GPU | one batch, made again | 122 | 4.629 / 9.780 / 39.408 | 21.46 / 51.91 / 77.25 / 95.13 | 25.83 / 82.97 |
| CPU | cells | 122 | 0.090 / 0.186 / 1.626 | 19.01 / 32.74 / 40.22 / 50.96 | 18.37 / 40.17 |
| CPU | one batch, made again | 122 | 4.378 / 5.442 / 65.052 | 21.58 / 42.14 / 62.20 / 151.53 | 26.25 / 69.84 |

## What it says

- **Uploading a cell costs 0.07 to 0.09 ms** at the median on either path,
  against 2 to 4.6 ms per cell that came or went when the batch is made
  again: 30 to 50 times less, in both runs. Making the batch again uploads
  all 120,000 instances, so its cost grows with the world's live instances;
  a cell's grows with its own.
- On the GPU-driven path the cells' frames were faster at the median and
  at p95 and p99 in both runs. On the CPU path the two runs disagree (run 1
  favours the batch made again at p95, run 2 the cells at every
  percentile): with this much load on the machine, no frame-time
  difference on the CPU path is claimed.
- Not measured here: the GPU's own time for the uploads (the frame column
  includes waiting for it), and a quiet machine. When these runs were made
  each cell was drawn as a batch of its own, at least one draw per visible
  cell, material and view; see the draw table below for what changed.

## Draw calls: cells drawn together

What a still field of 100 or 1,000 visible cells costs to draw, each cell
on its own (as before: a bind group and indirect draws per cell on the
GPU-driven path, a vertex buffer per cell on the CPU path) against drawn
together (one bind group and one multi-draw-indirect call per material and
view on the GPU-driven path; ranges of one shared vertex buffer on the CPU
path). "Before" is the same build with `Batching::separate_cells` set,
which takes the old code paths.

- Cells 4 m wide of 16 cubes each (a 4 x 4 grid), 10 x 10 or 40 x 25 of
  them, the whole field in the camera's view from above (counted on the
  CPU path: every cell had instances in the camera's list), sun shadows
  with four cascades, 1280 x 720, 4x MSAA.
- 20 warm-up frames, then 120 measured. The scene stands still, so the
  sun's shadow maps are kept between frames and a frame records the
  camera's two batch passes; the counts are the same every frame.
- **draws per frame** counts draw calls (a multi-draw counts once),
  **bind groups set** and **vertex buffers set** what the batch passes set,
  over every pass of the frame. **frame ms** is preparing, recording and
  submitting the frame until the GPU finished it.

Apple M4 Max, Metal (wgpu 30.0.1), macOS; rustc 1.100.0, release profile,
features `3d` plus the defaults; revision `385e4ead`. 2026-11-21. The
device has `INDIRECT_FIRST_INSTANCE`.

The machine was **not quiet**: other agents were compiling (load average
83 to 108). The draw, bind group and vertex buffer counts are exact and
were the same in both runs; the frame times moved by up to a factor of two
between runs and need repeating on a quiet machine.

Run 1 (load average 83 to 88):

| path | visible cells | cells drawn | draws per frame | bind groups set | vertex buffers set | frame ms (p50 / p95 / p99 / max) |
|---|---|---|---|---|---|---|
| GPU | 100 | each on its own (before) | 200 | 205 | 7 | 2.79 / 10.11 / 92.80 / 102.42 |
| GPU | 100 | together (after) | 2 | 7 | 7 | 2.71 / 19.37 / 106.98 / 141.64 |
| GPU | 1000 | each on its own (before) | 2000 | 2005 | 7 | 7.58 / 69.58 / 147.38 / 154.89 |
| GPU | 1000 | together (after) | 2 | 7 | 7 | 5.66 / 51.89 / 75.83 / 95.42 |
| CPU | 100 | each on its own (before) | 200 | 7 | 205 | 2.93 / 41.10 / 69.12 / 85.74 |
| CPU | 100 | together (after) | 200 | 7 | 7 | 2.51 / 32.81 / 51.86 / 119.63 |
| CPU | 1000 | each on its own (before) | 2000 | 7 | 2005 | 6.71 / 59.88 / 136.10 / 147.70 |
| CPU | 1000 | together (after) | 2000 | 7 | 7 | 5.49 / 52.49 / 110.69 / 212.06 |

Run 2 (load average 94 to 108):

| path | visible cells | cells drawn | draws per frame | bind groups set | vertex buffers set | frame ms (p50 / p95 / p99 / max) |
|---|---|---|---|---|---|---|
| GPU | 100 | each on its own (before) | 200 | 205 | 7 | 5.63 / 55.14 / 89.83 / 98.03 |
| GPU | 100 | together (after) | 2 | 7 | 7 | 4.14 / 62.94 / 71.15 / 110.52 |
| GPU | 1000 | each on its own (before) | 2000 | 2005 | 7 | 17.95 / 113.24 / 133.54 / 139.93 |
| GPU | 1000 | together (after) | 2 | 7 | 7 | 9.38 / 70.58 / 172.09 / 254.52 |
| CPU | 100 | each on its own (before) | 200 | 7 | 205 | 3.17 / 52.10 / 93.77 / 99.50 |
| CPU | 100 | together (after) | 200 | 7 | 7 | 2.51 / 38.29 / 55.60 / 66.65 |
| CPU | 1000 | each on its own (before) | 2000 | 7 | 2005 | 7.53 / 66.75 / 127.28 / 167.66 |
| CPU | 1000 | together (after) | 2000 | 7 | 7 | 6.10 / 58.50 / 76.94 / 93.31 |

What it says:

- **GPU-driven path: one draw per material and view, whatever the cell
  count.** 100 visible cells went from 200 draw calls and 205 bind groups a
  frame to 2 and 7; 1,000 cells from 2,000 and 2,005 to 2 and 7.
- **CPU path: one vertex buffer bound instead of one per cell** (205 to 7
  and 2,005 to 7 a frame). The draws stay one per run of neighbouring
  visible instances, so one per visible cell and pass here.
- The median frame was lower after than before in all four rows of both
  runs, by 3 to 48 %; the tails (p95 and up) go both ways between the runs.
  With this much load no frame-time gain is claimed: the counts above are
  the result, the times need a quiet machine.
- Where it runs natively: wgpu 30's Vulkan backend issues one
  `vkCmdDrawIndexedIndirect` with the draw count when the device has
  `multiDrawIndirect` (and the count is within `maxDrawIndirectCount`),
  DirectX 12 one `ExecuteIndirect`; its Metal and OpenGL backends loop over
  the draws, one indirect draw each, inside wgpu. With wgpu's indirect-call
  validation on, wgpu-core still queues each draw's arguments for its
  checking pass, then issues one backend call for the run. So on Metal the
  GPU still receives a draw per cell: the saving is the engine's per-cell
  draw code, wgpu's per-call work and the bind group changes, which is what
  the CPU side of the frame pays for.
- Devices without `INDIRECT_FIRST_INSTANCE` (some browsers and older GPUs)
  draw each cell on its own, exactly as before.
