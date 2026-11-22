Measured 2026-11-22 at `784d7a2b` on Apple M4 Max (10 performance + 4 efficiency cores, 36 GB), macOS 27.2.1, rustc 1.100.0 (0ef1bc386 2026-11-09), Bevy 0.20.1; the shipping profile (fat LTO, one codegen unit, panic = abort) on both sides, 3 runs per engine, one process at a time.

Power: AC Power.

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | | First table: Bevy / Ponko |
|---|---:|---:|---:|---|---|
| ECS: spawn and despawn 100k in a world of 1M | 1.599 (1.594–1.623) | 4.382 (4.343–4.520) | 2.74× | Ponko faster | 2.43×, Ponko faster |
| ECS: iterate 1M (one archetype) | 0.165 (0.164–0.173) | 0.391 (0.370–0.404) | 2.37× | Ponko faster | 2.36×, Ponko faster |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.005 (0.005–0.006) | 0.010 (0.010–0.012) | 2.18× | Ponko faster | 1.91×, Ponko faster |
| ECS: add and remove a component, 1M entities | 42.1 (42.1–42.1) | 104.0 (102.3–107.8) | 2.47× | Ponko faster | 2.31×, Ponko faster |
| ECS: parallel iterate 1M (all cores) | 0.624 (0.614–0.629) | 0.755 (0.733–0.773) | 1.21× | Ponko faster | 1.32×, Ponko faster |
| ECS: commands, 100k spawns and despawns in a world of 1M | 2.225 (2.219–2.289) | 5.049 (5.025–5.165) | 2.27× | Ponko faster | 2.25×, Ponko faster |
| Sprites: 100k moving, one frame drawn | 1.023 (0.985–1.027) | 6.736 (6.713–6.840) | 6.59× | Ponko faster | 10.49×, Ponko faster |
| Sprites: 100k moving, a different tint each, one frame drawn | 0.985 (0.975–0.985) | 37.4 (36.5–37.9) | 37.97× | Ponko faster | 40.95×, Ponko faster |
| Sprites: 1M moving, one frame drawn | 7.623 (7.345–7.744) | 58.9 (58.8–59.1) | 7.73× | Ponko faster | 12.70×, Ponko faster |
| 3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow | 4.681 (4.662–4.720) | 14.1 (14.0–14.1) | 3.00× | Ponko faster | 3.42×, Ponko faster |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 6.060 (5.959–6.061) | 3.116 (3.066–3.226) | 0.51× | **Bevy faster** | 0.64×, **Bevy faster** |
| 2D physics: 10k boxes falling into a pile, one tick | 3.253 (3.171–3.398) | 4.498 (4.478–4.761) | 1.38× | Ponko faster | 1.27×, Ponko faster |
| UI: 10k nodes, still, one frame drawn | 1.244 (1.201–1.257) | 2.712 (2.621–2.726) | 2.18× | Ponko faster | 2.81×, Ponko faster |
| UI: 10k nodes, all recolored every frame, one frame drawn | 1.662 (1.623–1.828) | 4.598 (4.512–4.670) | 2.77× | Ponko faster | 2.70×, Ponko faster |
| Headless sim: 1M entities, one tick | 0.069 (0.069–0.071) | 0.170 (0.164–0.172) | 2.47× | Ponko faster | 2.46×, Ponko faster |
| Time to first frame (process start to first offscreen frame done, process exit included) | 26.8 (26.0–29.3) | 71.6 (71.0–76.4) | 2.67× | Ponko faster | 6.87×, Ponko faster |

Medians of 3 runs, the range in brackets; 1-minute load 1.73 to 2.89.

| | Ponko small game, tuned | Bevy small game, tuned |
|---|---:|---:|
| Clean debug build (s) | 41.6 (40.1–42.6) | 97.0 (96.0–101.0) |
| Debug rebuild after a one-line change in src/lib.rs (s) | 0.42 | 0.54 |
| ... in src/main.rs (s) | 0.29 | 0.32 |
| Clean shipping-profile build (s, one run) | 108.0 | 212.0 |
| Binary / stripped | 9.8 MB / 8.5 MB | 52.1 MB / 33.1 MB |
| wasm (shipping profile, before wasm-bindgen) / gzipped | 12.7 MB / 3.6 MB | 57.5 MB / 11.9 MB |
