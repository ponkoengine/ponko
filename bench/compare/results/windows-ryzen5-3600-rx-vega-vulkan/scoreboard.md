Measured 2026-11-26 at `c9e3c526` (with uncommitted changes) on AMD Ryzen 5 3600 6-Core Processor (6 cores, 12 threads, 16 GB), Windows 11 IoT Enterprise LTSC 2024 24H2 (build 26100.9457), rustc 1.100.0 (0ef1bc386 2026-11-09), Bevy 0.20.1; release builds, 3 runs per engine, one process at a time.

GPU: Radeon RX Vega (driver 31.0.12027.9001); backend: vulkan.

Adapters the programs reported: `Radeon RX Vega (Vulkan)`; `{"backend": "vulkan", "device_type": "discrete_gpu", "driver": "AMD proprietary driver", "driver_info": "22.20.27.09", "name": "Radeon RX Vega"}`; `{"backend": "vulkan", "device_type": "discretegpu", "driver": "AMD proprietary driver", "driver_info": "22.20.27.09", "name": "Radeon RX Vega"}`.

Power: AC Power (no battery).

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |
|---|---:|---:|---:|---|
| ECS: spawn and despawn 100k in a world of 1M | 4.200 (4.113–4.207) | 10.5 (10.3–10.7) | 2.49× | Ponko faster |
| ECS: iterate 1M (one archetype) | 0.426 (0.368–0.516) | 0.962 (0.827–0.981) | 2.26× | Ponko faster |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.019 (0.019–0.019) | 0.043 (0.043–0.047) | 2.26× | Ponko faster |
| ECS: add and remove a component, 1M entities | 87.5 (87.1–87.6) | 164.4 (162.3–165.8) | 1.88× | Ponko faster |
| ECS: parallel iterate 1M (all cores) | 3.977 (3.936–4.258) | 6.413 (4.505–7.092) | 1.61× | Ponko faster |
| ECS: commands, 100k spawns and despawns in a world of 1M | 5.802 (5.391–5.883) | 13.5 (13.2–13.9) | 2.32× | Ponko faster |
| Sprites: 100k moving, one frame drawn | 2.481 (2.473–2.640) | 30.2 (30.2–33.5) | 12.19× | Ponko faster |
| Sprites: 100k moving, a different tint each, one frame drawn | 2.531 (2.478–3.629) | 90.3 (90.2–93.0) | 35.69× | Ponko faster |
| Sprites: 1M moving, one frame drawn | 29.1 (29.0–29.7) | 265.0 (264.5–289.0) | 9.12× | Ponko faster |
| 3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow | 14.8 (14.5–14.9) | 31.2 (30.9–31.6) | 2.11× | Ponko faster |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 8.715 (8.629–8.732) | 8.346 (8.324–8.554) | 0.96× | **Bevy faster** |
| 2D physics: 10k boxes falling into a pile, one tick | 25.6 (24.7–26.1) | 31.5 (31.3–33.0) | 1.23× | Ponko faster |
| UI: 10k nodes, still, one frame drawn | 4.347 (4.103–4.814) | 12.1 (12.0–12.2) | 2.78× | Ponko faster |
| UI: 10k nodes, all recolored every frame, one frame drawn | 7.054 (6.718–7.247) | 20.9 (20.7–21.6) | 2.96× | Ponko faster |
| Headless sim: 1M entities, one tick | 0.357 (0.354–0.396) | 0.935 (0.752–1.256) | 2.62× | Ponko faster |
| Time to first frame (process start to first offscreen frame done, process exit included) | 187.8 (184.1–189.5) | 329.1 (324.2–334.2) | 1.75× | Ponko faster |

Medians of 3 runs, the range in brackets; 1-minute load 0.04 to 3.07.
