# Shard Grove GPU baseline: Windows, Radeon RX Vega

The 3D reference game (`examples/shard_grove`, issue #454) at a fixed view,
measured with `PONKO_BENCH` on 2026-11-26.

| | |
|---|---|
| Machine | Windows 11 (build 26100), AMD Ryzen 5 3600, 16 GB |
| GPU | Radeon RX Vega (discrete), capability tier `desktop` |
| Driver | DirectX 12: 31.0.12027.9001; Vulkan: AMD proprietary driver 22.20.27.09 (Vulkan 1.3.217) |
| Revision | `main` at `b68c86e3` (the build also carried the examples' Windows stack link argument, which changes no engine code) |
| Build | `ponko package --example shard_grove --features physics,physics3d,navmesh,text-shaping` (release); the packaged program was run |
| Window | 1280 x 720, the game's default settings: the High preset (SMAA, ambient occlusion, screen-space reflections, 4 shadow cascades at 2048, no GI) |
| View | The title screen: the camera circling the whole grove (it turns 0.05 rad a second, under 0.2 rad over a run), the menu drawn over it |
| Run | `PONKO_BENCH=600`: 240 warm-up frames, then 600 measured, vsync off, drawn offscreen; three runs per backend, the backends alternating |
| Load | 0.06 to 0.69 busy logical processors of 12 (`scripts/cpu_load.py`) before and after each run; no other GPU program running |

Medians of three runs, with the spread `(max - min) / median`:

| | DirectX 12 | Vulkan |
|---|---|---|
| fps | 253.3 (0.0%) | 266.4 (1.3%) |
| frame avg, ms | 3.95 (0.0%) | 3.75 (1.3%) |
| frame p50, ms | 3.94 (0.2%) | 3.75 (0.2%) |
| frame p95, ms | 4.10 (1.0%) | 3.88 (1.3%) |
| frame p99, ms | 4.22 (0.9%) | 4.00 (35.5%: one run 5.40) |
| frame max, ms | 5.36 (7.8%) | 4.34 (one run had a 13.5 ms frame) |
| GPU, ms | 3.83 (0.0%) | 3.60 (0.1%) |
| CPU systems, ms | 0.07 | 0.06 |
| CPU render, ms | 1.24 | 0.83 |
| draw calls | 58 | 54 |

The slowest GPU passes (median ms, DirectX 12 / Vulkan): screen reflections
1.23 / 1.24, GTAO bilateral 0.63 / 0.48, opaque 0.54 / 0.59, GTAO horizons
0.36 / 0.35, bloom 0.23 / 0.15, reflection blur 0.23 / 0.20. The frame is
GPU-bound on both backends.

`dx12-<n>.json` and `vulkan-<n>.json` are the `PONKO_BENCH_OUT` reports of each
run; `runs.jsonl` has each run's exit code and the load before and after.

To measure again on another machine, from the package folder:

```sh
WGPU_BACKEND=dx12 PONKO_BENCH=600 PONKO_BENCH_OUT=dx12-1.json ./shard_grove
```
