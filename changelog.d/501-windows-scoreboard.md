### Changed
- The Ponko vs Bevy scoreboard (`bench/compare/scoreboard.py`) runs on
  Windows: `.exe` names, UTF-8 file reads, machine and driver detection on
  Windows and Linux, and a load figure without `uptime`
  (`scripts/cpu_load.py`, also used by `scripts/build-times.sh`).
  `--machine NAME` writes results to `bench/compare/results/NAME/`, and
  `--backend` picks the GPU backend for both engines. A run that fails (a
  crash, the GPU out of memory) is recorded as a failed cell with its error,
  rather than stopping the scoreboard.
- The performance chapter and the README show the scoreboard on a second
  machine, a Windows desktop (Ryzen 5 3600, Radeon RX Vega), on DirectX 12
  and Vulkan.
