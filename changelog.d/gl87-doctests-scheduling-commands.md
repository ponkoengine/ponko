### Added
- Doc examples that run headless as doctests for the scheduling, state and
  command-buffer API: `Game::schedule_plan`, `remove` and `access_report`,
  the pieces of a `SchedulePlan` (`Stage`, `PlannedSystem`, `Boundary`,
  `Alone` and the plan's lookups), every `State` call and `IntoCondition`,
  `Ctx::set_state` and `push_state`, and `Commands` (`new`, `bind`,
  `is_bound_to`, `spawn`, `insert`, `remove`, `despawn`, `add`, `len`,
  `is_empty`) with `World::apply`.
- Each deprecated `Ctx` shortcut (`dt`, `key_down`, `key_pressed`,
  `key_released`, `mouse_pos`, `draw`, `draw_text`, `load_texture`,
  `load_sound`, `play_sound`) has a one-line example showing the call on its
  service that replaces it.

### Changed
- The guide's tutorial and reference pages, and the service docs, name the
  service calls (`ctx.audio.play`, `ctx.gfx.draw_text`, `ctx.gfx.load_texture`,
  ...) instead of the deprecated `Ctx` shortcuts, which stay until 0.3.
