### Added
- `Game::state_screen(state, mode, build)` ties a retained UI screen to a game
  state: built and pushed on entering it, popped and removed on leaving it.
  Closing the screen (Escape, or a button marked `closes_screen`) pops the
  state when it was pushed over another one. C: `ponko_game_state_screen`;
  C++: `game::state_screen`.
- `UiBuilder::closes_screen()`, `Ui::set_closes_screen` and `Ui::closes_screen`:
  a Back, Close or Resume button that closes its screen as Escape would.
  C: `ponko_ctx_ui_set_closes_screen`, `ponko_ctx_ui_closes_screen`.
- `InputEvent::MouseMove` and `InputEvent::MouseWheel`, and the scenario
  steps `mouse_move` and `mouse_wheel`: a headless test can put the pointer
  where it clicks. C: `ponko_ctx_feed_mouse_move`, `ponko_ctx_feed_mouse_wheel`,
  `ponko_input_scenario_mouse_move`, `ponko_input_scenario_mouse_wheel`; C++:
  `input_event::mouse_move` and `input_event::wheel`.
- `Audio::started()` lists the sounds started this tick or frame (`SoundStarted`:
  handle, sound, bus, volume, speed, looping), headless too, so tests can check
  what a game played. C: `ponko_ctx_sounds_started`, `ponko_ctx_sound_started`
  (the versioned `PonkoSoundStarted`); C++: `ctx::sounds_started`.
- `Ctx::quit_requested()` is public. C: `ponko_ctx_quit_requested`; C++:
  `ctx::quit_requested`.
- C and C++: `ponko_ctx_camera_zoom_at` / `ctx::camera_zoom_at`, the 2D camera's
  `zoom_at`.
- The cookbook's slow-motion and zoom recipes have C++ halves: every recipe is
  now in both languages. The pause menu recipe is 19 lines instead of 46.
- Guide: "A screen for a state" in Game states, and "The pointer, sounds
  and quitting" in Headless runs.

### Changed
- Headless runs give every sound they start a handle of its own (numbered from
  1) instead of 0, so stopping or changing one in a test acts on that one.
- C and C++: debug shapes, `ponko_ctx_draw`, `ponko_ctx_draw_ex`,
  `ponko_ctx_draw_rect`, `ponko_ctx_draw_line`, `ponko_ctx_draw_circle` and
  `ponko_ctx_play_sound` work inside a `ponko_query` / `world().each` callback
  instead of returning `PONKO_BUSY`; so do the 2D physics queries (rays,
  shape casts, overlaps, `closest_point`, `penetrations`) and
  `ponko_third_person_camera_update_physics`. None of them work in
  `ponko_query_par`. The C++ cookbook draws health bars, the minimap and
  debug shapes, plays footsteps, checks the platformer's ground and follows
  the 3D walker straight from its loops.
- C++: the `set_sim_speed` documentation says what it accepts (any speed of 0
  or more; 0.25 is slow motion), not "1 to 64".
