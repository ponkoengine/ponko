### Added
- `ctx.physics3d.set_paused(bool)` and `ctx.physics3d.is_paused()` hold only
  the 3D physics step, the same call as the 2D `ctx.physics.set_paused`:
  bodies keep their pose and velocity, characters keep their velocity but
  don't move, no collision events arrive and no contact hook runs, while
  every other simulation system and the tick counter keep running, headless
  too. In a window, held bodies are drawn at their last pose. C:
  `ponko_ctx_set_physics3d_paused` and `ponko_ctx_is_physics3d_paused`; C++:
  `ctx.set_physics3d_paused(bool)` and `ctx.is_physics3d_paused()`. The
  guide's "Pausing only 3D physics" section shows the pattern.

### Changed
- Shard Grove holds the 3D physics step on its menus with
  `ctx.physics3d.set_paused` instead of zeroing the warden's velocity, so a
  jump goes on where the pause stopped it.
