### Added
- `Camera::debug_draw` (`d3::Camera`), a flag of its own for whether a
  camera draws `ctx.debug`'s 3D lines and labels: `Some(true)` always,
  `Some(false)` never, whatever its `particles` says. `None`, the default,
  draws what cameras drew before: the window's cameras draw them, a target
  camera while it draws particles. `TargetCamera::with_debug_draw` sets it,
  so a monitor can show the debug drawing without the smoke, or a map be the
  only view that shows it. From C, the `debug_draw` field (a
  `PonkoDebugDraw`) of version 2 of `PonkoCamera3DEx` and
  `PonkoTargetCameraEx`; in C++, `ponko::debug_draw`.
- Web: a lost WebGPU device's failure says why:
  `Failure::device_lost_reason()` is `DeviceLostReason::Destroyed` when the
  device's `destroy()` was called and `Unknown` for anything else (the GPU
  process crashed or was reset), so a game can word the two differently.
  The page's `onPonkoFailure` error has it as `reason` (`"Destroyed"` or
  `"Unknown"`, `null` for other failures), and the engine's message says
  "destroyed" or "lost". `web/smoke/runtime-failures.mjs` checks it.

### Fixed
- See-through first-person held items whose material has a stencil state
  draw in their stencil's `order` and write the stencil also while the
  transparent pass is split off (a see-through surface in the world reads
  the scene behind it, or soft particles are on): there they used to test it
  without writing it, so one that marks it hid nothing from the next.
