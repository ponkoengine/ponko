### Added
- C and C++ crash reports: `ponko_game_crash_reports` takes the versioned
  `PonkoCrashReports` (on, off or the build's default, the game's version and
  a hook that gets a `PonkoCrashReport`), `ponko_crash_report` writes the
  game's own, and `ponko_ctx_saved_crash_reports` lists the saved ones. C++:
  `game::crash_reports`, `ponko::report_crash`, `ctx::saved_crash_reports`.
- Touch controls from C and C++ with their looks and places
  (`ponko_ctx_set_touch_controls_ex`, `PonkoTouchStyle`, `PonkoTouchPlace`),
  the player's layout and its edit mode (`ponko_ctx_edit_touch_controls`,
  `ponko_ctx_touch_layout`, `_touch_layout_control`, `_touch_layout_json`,
  `_save_touch_layout`, `_restore_touch_layout` and their setters), and
  trackpad gestures (`ponko_ctx_feed_trackpad`,
  `ponko_input_scenario_trackpad`).
- Camera targets from C and C++: `ponko_ctx_set_camera_target` marks the
  entity a rig kept in the world follows (`ponko_ctx_set_follow_camera2d_rig`,
  `_orbit_camera_rig`, `_first_person_camera_rig`, `_third_person_camera_rig`
  and their readers); C++ `ctx::set_camera_target` and each rig's `keep`,
  `kept` and `remove_kept`.
- `ponko_game_restore_window_settings` (C++ `game::restore_window_settings`):
  the saved window settings before the window exists.
- Seas from a wave spectrum in C and C++: `PonkoWaterDesc` version 2 adds
  `has_spectrum` and `spectrum` (`PonkoWaveSpectrum`, started by
  `ponko_wave_spectrum`); a version 1 descriptor keeps its Gerstner waves.
  C++: `ponko::wave_spectrum` and `water_desc::with_spectrum`.
- Sliced loaders from C and C++: `ponko_asset_group_add_sliced` takes a
  `PonkoSliceLoader` (start, step and free callbacks over a state of the
  game's own) and `ponko_ctx_take_asset_group_sliced` hands the finished
  state over; C++ `asset_group::sliced` and `ctx::take_asset_group_sliced<T>`.
  The design is in the guide's "Sliced loaders from C".
- The ready-made touch arrange screen from C and C++:
  `ponko_ctx_arrange_touch_controls` (C++ `ctx::arrange_touch_controls`).
- Crowd levels that crossfade, from C and C++: `PonkoCrowdDesc` (with
  `fades`, one per level) and `ponko_ctx_create_crowd_ex`;
  `PonkoCrowdLevel` keeps its layout. C++: `ctx::create_crowd` with fades.
- The navmesh crowd's counts from C and C++: `PonkoNavCrowdStats` and
  `ponko_ctx_navmesh_crowd_stats` (C++ `ctx::navmesh_crowd_stats`).
- Per-thread allocation counts from C and C++: `ponko_thread_alloc_counts`
  (C++ `ponko::thread_alloc_counts`), and scopes timed with
  `ponko_profile_begin`/`_end` (`PONKO_PROFILE_SCOPE`) count the allocations
  made on their thread, as Rust's `profile_scope!` does. Rust:
  `ProfileScope::end_counting` ends a `begin`/`end` scope with its count.
