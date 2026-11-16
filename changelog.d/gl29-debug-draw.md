### Added
- Debug drawing, `ctx.debug` (and `sys.debug` in view systems): lines, rays,
  arrows, polylines, boxes, circles, spheres, capsules, camera frustums,
  grids, axes and text labels, in 2D or 3D, in world or screen space, kept
  until the drawing system runs again or for a number of seconds,
  depth-tested or on top. All 3D lines are one buffer and two draw calls,
  2D lines one sprite batch. On in debug builds, off (one branch per call)
  in release builds; `Game::debug_draw` and `DebugDraw::set_enabled` choose.
- Built-in debug views (`DebugView`): physics colliders (2D and 3D),
  navmesh, bounds, light volumes and hierarchy axes.
- The control endpoint's `debug.draw` method and the MCP `debug_draw` tool
  turn debug drawing and its views on or off.
- C and C++: `ponko_ctx_debug_*` with `PonkoDebugStyle`, and
  `ctx.debug()` returning `ponko::debug_draw`.
- `ponko_bench` cases `debug/100k-lines-3d`, `debug/100k-lines-2d` and
  `debug/100k-lines-off`.
- Book: "Debug drawing".
