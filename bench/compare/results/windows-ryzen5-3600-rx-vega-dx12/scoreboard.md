Measured 2026-11-26 at `c9e3c526` (with uncommitted changes) on AMD Ryzen 5 3600 6-Core Processor (6 cores, 12 threads, 16 GB), Windows 11 IoT Enterprise LTSC 2024 24H2 (build 26100.9457), rustc 1.100.0 (0ef1bc386 2026-11-09), Bevy 0.20.1; release builds, 3 runs per engine, one process at a time.

GPU: Radeon RX Vega (driver 31.0.12027.9001); backend: dx12.

Adapters the programs reported: `Radeon RX Vega (Dx12)`; `{"backend": "dx12", "device_type": "discrete_gpu", "driver": "31.0.12027.9001", "driver_info": "", "name": "Radeon RX Vega"}`; `{"backend": "dx12", "device_type": "discretegpu", "driver": "31.0.12027.9001", "driver_info": "", "name": "Radeon RX Vega"}`.

Power: AC Power (no battery).

| Workload | Ponko (ms) | Bevy (ms) | Bevy / Ponko | |
|---|---:|---:|---:|---|
| ECS: spawn and despawn 100k in a world of 1M | 4.158 (4.129–4.159) | 10.3 (10.3–10.4) | 2.49× | Ponko faster |
| ECS: iterate 1M (one archetype) | 0.522 (0.513–0.607) | 1.185 (1.120–1.240) | 2.27× | Ponko faster |
| ECS: iterate 16k fragmented over 1,024 archetypes | 0.019 (0.019–0.023) | 0.043 (0.043–0.049) | 2.22× | Ponko faster |
| ECS: add and remove a component, 1M entities | 88.4 (86.6–95.4) | 167.9 (167.2–170.8) | 1.90× | Ponko faster |
| ECS: parallel iterate 1M (all cores) | 4.281 (4.199–4.284) | 7.439 (6.334–7.475) | 1.74× | Ponko faster |
| ECS: commands, 100k spawns and despawns in a world of 1M | 5.505 (5.421–5.510) | 14.2 (13.3–15.6) | 2.58× | Ponko faster |
| Sprites: 100k moving, one frame drawn | 2.907 (2.756–3.035) | 29.2 (28.7–30.0) | 10.05× | Ponko faster |
| Sprites: 100k moving, a different tint each, one frame drawn | 2.844 (2.838–2.942) | failed (3 of 3 runs) | – | **Bevy failed** |
| Sprites: 1M moving, one frame drawn | 27.9 (27.1–28.1) | 244.6 (243.0–246.0) | 8.78× | Ponko faster |
| 3D: 100k moving meshes (16 kinds, instanced) with a 4-cascade sun shadow | 15.6 (15.4–15.9) | 39.7 (39.5–40.1) | 2.55× | Ponko faster |
| 3D: 1,000 skinned characters (16 joints) with a sun shadow | 9.648 (9.569–9.744) | 15.5 (15.3–15.7) | 1.61× | Ponko faster |
| 2D physics: 10k boxes falling into a pile, one tick | 26.2 (25.4–26.5) | 34.3 (32.8–35.1) | 1.31× | Ponko faster |
| UI: 10k nodes, still, one frame drawn | 5.341 (4.467–6.162) | 12.7 (12.7–12.9) | 2.37× | Ponko faster |
| UI: 10k nodes, all recolored every frame, one frame drawn | 7.395 (7.136–8.139) | 21.8 (21.2–22.0) | 2.94× | Ponko faster |
| Headless sim: 1M entities, one tick | 0.619 (0.503–0.632) | 1.239 (1.146–1.320) | 2.00× | Ponko faster |
| Time to first frame (process start to first offscreen frame done, process exit included) | 260.7 (249.6–1338.1) | 1356.3 (1336.1–1496.2) | 5.20× | within noise |

Medians of 3 runs, the range in brackets; 1-minute load 0.55 to 1.91.

Failed runs:

- Sprites: 100k moving, a different tint each, one frame drawn, Bevy: `2026-11-25T23:45:07.282589Z ERROR bevy_render::error_handler: Caught DeviceLost error: Unknown Out of memory`
- Sprites: 100k moving, a different tint each, one frame drawn, Bevy: `2026-11-25T23:52:00.919880Z ERROR bevy_render::error_handler: Caught DeviceLost error: Unknown Out of memory`
- Sprites: 100k moving, a different tint each, one frame drawn, Bevy: `2026-11-25T23:59:18.165595Z ERROR bevy_render::error_handler: Caught DeviceLost error: Unknown Out of memory`
