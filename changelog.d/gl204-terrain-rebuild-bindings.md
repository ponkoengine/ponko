### Added
- C and C++ games can make a terrain tile's ground again after a load:
  `ponko_ctx_physics3d_set_terrain_rebuild` (and `_clear_terrain_rebuild`)
  take a callback that reads the loaded tile with `ponko_loaded_world_get`
  (a read-only `PonkoLoadedWorld`) and returns its heightfield, which the
  engine frees, or NULL; C++ `ctx::set_terrain_rebuild` and
  `clear_terrain_rebuild` with a `ponko::loaded_world`, an exception coming
  out of `run`. The Rust-only rows for `Physics3D::set_terrain_rebuild` and
  `clear_terrain_rebuild` are gone; the 3D physics guide pairs both languages.

### Changed
- The water's WebGL2 upload shares the fixed steps' wave moments with
  floating bodies: while the frame's clock is on the simulation's steps, it
  draws the blend of the next two steps' moments (within a millimetre of the
  frame's own), which buoyancy asks for anyway, so a frame works out at most
  one moment a step instead of one of its own besides the steps'. With
  something floating at 128 cells, 0.35 → 0.21 ms of CPU field a frame at
  144 frames a second, 0.41 → 0.31 ms at 60 (1.59 → 1.14 and 2.06 → 1.50 ms
  at 256 cells; release, one thread's CPU time, medians of five runs on a
  busy Apple M4 Max). After a pause or a simulation speed change, frames work
  out their own moment as before. The CPU keeps the last three moments asked
  for instead of the last two worked out.
