Measured 2026-11-22 at `5e5af4ee` (with uncommitted changes) on Apple M4 Max (10 performance + 4 efficiency cores, 36 GB), macOS 27.2.1, rustc 1.100.0 (0ef1bc386 2026-11-09), Bevy 0.20.1; release builds, 3 runs per engine, one process at a time.

Power: AC Power, except Time to first frame (process start to first offscreen frame done, process exit included), builds on Battery Power.

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |
|---|---:|---:|---:|---|
| ECS: spawn and despawn 100k in a world of 1M | 1.758 (1.731–1.918) | 4.272 (4.224–4.326) | 2.43× | Ponko faster |
| ECS: iterate 1M (one archetype) | 0.165 (0.164–0.166) | 0.390 (0.362–0.405) | 2.36× | Ponko faster |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.005 (0.005–0.006) | 0.010 (0.009–0.011) | 1.91× | Ponko faster |
| ECS: add and remove a component, 1M entities | 42.8 (41.6–43.1) | 98.9 (97.5–101.9) | 2.31× | Ponko faster |
| ECS: parallel iterate 1M (all cores) | 0.560 (0.551–0.627) | 0.741 (0.740–0.783) | 1.32× | Ponko faster |
| ECS: commands, 100k spawns and despawns in a world of 1M | 2.400 (2.382–2.448) | 5.394 (5.289–5.422) | 2.25× | Ponko faster |
| Sprites: 100k moving, one frame drawn | 1.103 (1.088–1.131) | 11.6 (11.1–15.0) | 10.49× | Ponko faster |
| Sprites: 100k moving, a different tint each, one frame drawn | 1.093 (1.087–1.094) | 44.8 (42.9–45.1) | 40.95× | Ponko faster |
| Sprites: 1M moving, one frame drawn | 8.324 (8.177–8.345) | 105.7 (104.4–106.5) | 12.70× | Ponko faster |
| 3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow | 4.632 (4.612–4.638) | 15.8 (15.8–15.9) | 3.42× | Ponko faster |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 6.064 (6.022–6.108) | 3.857 (3.855–5.508) | 0.64× | **Bevy faster** |
| 2D physics: 10k boxes falling into a pile, one tick | 6.394 (6.367–6.486) | 8.108 (7.996–8.281) | 1.27× | Ponko faster |
| UI: 10k nodes, still, one frame drawn | 1.630 (1.250–2.088) | 4.580 (4.570–4.808) | 2.81× | Ponko faster |
| UI: 10k nodes, all recolored every frame, one frame drawn | 2.494 (2.468–2.549) | 6.732 (6.699–6.931) | 2.70× | Ponko faster |
| Headless sim: 1M entities, one tick | 0.166 (0.165–0.167) | 0.407 (0.406–0.443) | 2.46× | Ponko faster |
| Time to first frame (process start to first offscreen frame done, process exit included) | 28.9 (27.7–41.2) | 198.7 (197.9–244.6) | 6.87× | Ponko faster |

Medians of 3 runs, the range in brackets; 1-minute load 1.47 to 2.73.

| | Ponko hello | Bevy hello | Ponko small game | Bevy small game |
|---|---:|---:|---:|---:|
| Clean debug build (s) | 46.4 | 117.0 | 45.1 | 118.0 |
| Debug rebuild after a one-line change in src/lib.rs (s) | 0.50 | 0.74 | 0.47 | 0.83 |
| ... in src/main.rs (s) | 0.39 | 0.62 | 0.32 | 0.64 |
| Clean release build (s, one run) | 36.2 | 90.0 | 35.7 | 91.0 |
| Release binary / stripped | 24.8 MB / 19.6 MB | 134.2 MB / 82.2 MB | 24.5 MB / 19.4 MB | 134.4 MB / 82.4 MB |
| wasm (release, before wasm-bindgen) / gzipped | 20.7 MB / 5.8 MB | 120.5 MB / 21.4 MB | 20.7 MB / 5.8 MB | 120.7 MB / 21.5 MB |
