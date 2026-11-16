# Instance cells: streaming one cell at a time against making the batch again

What it costs to stream the instances of an open world cell by cell
(`Graphics::add_instance_cell`, `Ctx::free` on an `InstanceCell`), against
keeping them in one `InstanceBatch` that is made again whenever a cell comes
or goes.

## How to run it

```sh
cargo test --release --lib --features 3d instance_cells_benchmark -- --ignored --nocapture
```

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
  includes waiting for it), and a quiet machine. Each cell is drawn as a
  batch of its own, so a view draws at least one draw per visible cell
  and material; a world of very many small cells pays that in draw calls.
