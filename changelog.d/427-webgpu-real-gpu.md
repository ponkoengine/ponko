### Added

- The local check list's `web-gpu` job gates WebGPU on a real GPU: on a Mac
  it runs the 2D and 3D web smoke fixtures in headless Chromium on the
  machine's GPU (Metal) and fails when WebGPU is unsupported, falls back to
  WebGL2, or runs on a software renderer or a fallback adapter.
- `web/smoke/smoke.mjs --require-gpu`, the gate's check, and
  `web/smoke/frame-times.mjs`, which measures the 3D fixture's frame times
  (median and 95th percentile, uncapped or at the display's rate) on WebGPU
  and WebGL2 with the engine's CPU and GPU split. The web guide's new "Frame
  times" section has the numbers.
