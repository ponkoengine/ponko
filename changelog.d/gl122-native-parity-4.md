### Added
- Component hooks from C and C++: `ponko_component_on_add`, `_on_replace`
  and `_on_remove` (a `PonkoHookEvent` with the entity and the values) and
  the `PonkoHookWorld` calls `ponko_hook_get`, `_write`, `_spawn_later`,
  `_set_later`, `_remove_later`, `_despawn_later` and `_is_alive`; C++
  `world_view::on_add`, `on_replace`, `on_remove<T>` and `ponko::hook_world`.
  `ponko_ecs` gains `RawWorld::on_add_dynamic`, `on_replace_dynamic`,
  `on_remove_dynamic` and `HookWorld::get_dynamic(_mut)`; Rust and dynamic
  hooks run in the order they were registered.
- The sky kit and the day-night driver from C and C++: the versioned
  `PonkoSkyKit` and `PonkoDayNight`, `ponko_ctx_sky_kit`, `_set_sky_kit`,
  `ponko_day_night_new`, `_advance`, `_moon_phase`,
  `ponko_ctx_apply_day_night`, `_update_day_night` and
  `ponko_moon_illumination`; C++ `sky_kit` and `day_night`.
- Video from C and C++ (the `video` feature of `ponko_ffi`):
  `ponko_ctx_load_video`, `_video_from_bytes`, `_free_video`, play, pause,
  seek, update, advance, volume and bus, the versioned `PonkoVideoState`
  and `ponko_video_frame_rgba`; C++ `ponko::video`. Without the feature the
  calls return `PONKO_UNSUPPORTED`.
- Baked lighting from C and C++: `ponko_mesh_data_generate_lightmap_uvs`,
  a `PonkoLightBakeScene` of models and lights, `ponko_bake_lighting` with
  the versioned `PonkoLightBakeSettings`, and a `PonkoBakedLighting` that
  is saved (`_to_bytes`), loaded (`_from_bytes`, `ponko_ctx_load_baked_lighting`),
  set on the renderer and read (info, lightmap rects, texels, probe light);
  `PonkoLightmapped` is a built-in component. C++ `light_bake_scene`,
  `baked_lighting` and `lightmapped`.
- Replication for components registered from C: inputs
  (`ponko_ctx_replication_input`, `ponko_ctx_set_replication_input`,
  `ponko_ctx_replication_input_of`), prediction with a C step
  (`ponko_ctx_replication_predicted`), interpolation
  (`ponko_ctx_replication_interpolated`, `_position`), events
  (`ponko_ctx_replication_event`, `_send`, `ponko_ctx_net_events` with
  `PonkoNetEvent`); the C++ ctx methods of the same names. In Rust,
  `Replication::raw_input`, `raw_predicted`, `raw_interpolated`,
  `raw_event` and `RawNetEvent` are what they build on.
- Crowds: `ponko_ctx_create_clip` makes a clip from `PonkoJointTrack`s
  (C++ `joint_track_of`), and the versioned `PonkoLiveCountsEx` with
  `ponko_ctx_live_counts_ex` counts crowds, instance cells, collision
  meshes, sprite shaders and materials, and sky shaders.
- Touch scenario builders `ponko_input_scenario_tap`, `_touch_hold` and
  `_touch_drag` (C++ `input_scenario::tap`, `touch_hold`, `touch_drag`).
- Reads inside C loops without a call per item: `ponko_ctx_nav_view` hands
  a read-only navmesh view whose `ponko_nav_view_sample_position`,
  `_sample_positions` and `_find_path` work in `ponko_query` and
  `ponko_query_par`; `ponko_ctx_collisions` (2D and 3D contacts) and
  `ponko_ctx_net_events` hand an array once per system.
- The guide shows C++ next to Rust for hooks, the sky kit, video, baked
  lighting, replication inputs, crowd clips, touch scenarios and loop reads.

### Changed
- The handles chapter and STABILITY.md record the decision for the next C
  ABI: 64-bit ids with the handle kind in the top 8 bits (0 for untagged),
  so a handle of the wrong kind is caught where it is passed.
