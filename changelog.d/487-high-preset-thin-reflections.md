<!-- area: Rendering -->
### Changed
- `GraphicsQuality::High` leaves `Post::ssr_thin_objects` (the thin-objects
  screen-space reflection trace) off; `Ultra` still turns it on, and it
  stays a setting of its own. It had been on in `High` on `main` since
  its first measurement over a floor and rails (about 0.2 ms at 2560x1440
  on an Apple M4 Max); in the bench's render scenes, where edges are
  everywhere, it cost 0.34 to 2.31 ms of the screen-reflections pass, up to
  27% of the frame, and 20 to 23 allocations a frame in wgpu. To have it
  with `High`, set it after the preset:
  `ctx.scene3d.set_quality(GraphicsQuality::High);
  ctx.scene3d.post.ssr_thin_objects = true;` (the scene then reads as
  custom, `quality()` says `None`). From C and C++, `ssr_thin_objects` of
  `PonkoPostEx` (version 2) does the same after `ponko_ctx_set_graphics_quality`;
  the `high` level through that call leaves it off (`ultra` turns it on). Render benchmark
  baselines recorded on `high` need recording again.
