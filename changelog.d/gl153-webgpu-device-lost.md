### Added
- Web: a WebGPU device lost while the game runs (the browser's GPU process
  crashed or was reset, or the device was destroyed) now stops the game
  with its own failure kind, `ponko::web::FailureKind::DeviceLost`, and
  reaches the page through `onPonkoFailure` (`kind: "DeviceLost"`), as a
  lost WebGL2 context does. Its message carries the browser's reason. The
  generated page shows it, or the game's own text from
  `ponko::web::on_failure`.
- `web/smoke/runtime-failures.mjs` has a `device-lost` case: it destroys
  the game's `GPUDevice` while the game runs and checks the page. It takes
  `--chromium-mode`, `--browser-arg` and `--allow-unsupported` like
  `smoke.mjs`; where the browser has no WebGPU the case is unsupported.
