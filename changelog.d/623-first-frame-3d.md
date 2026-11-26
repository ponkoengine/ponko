<!-- area: Rendering -->
### Fixed
- The first frame of an offscreen run (benchmarks, tests, the editor's
  thumbnails) draws its 3D picture, in the window's stand-in and in every
  target camera. Before, it held no 3D picture and not even the
  background.
- A frame drawn while the window's surface changes (shown, hidden,
  recreated, resumed) keeps its 3D picture. The new scene targets and the
  reset history now start with the next frame, instead of being dropped
  from the frame that was already prepared.

### Changed
- `Graphics::render_icon` (C `ponko_ctx_render_icon`, C++ `ctx.render_icon`)
  draws an icon in one frame, not two: an icon asked for in setup is in its
  target after the game's first frame.
