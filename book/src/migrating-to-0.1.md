# Migrating from 0.0.1 to 0.1.0

> **Maturity: not a feature.** A migration guide for one release.

0.1.0 is the one deliberately breaking release of the rename: it removed
every deprecated alias and forwarder, so there is one name and one path for
each thing. This page maps every Rust name from 0.0.1 (and from `main`
between the two releases) to its 0.1.0 replacement, in one step: where a name
moved twice, only the final name is listed.

It covers the Rust API only. **The C ABI is unchanged**: version 2, the same
`ponko_*` functions, layouts and numeric values (the removed `crabby_*` C names
are in [Naming and compatibility](naming.md)). The changelog's "Breaking
changes" explains the reasons; [STABILITY.md](../../STABILITY.md)
says what the 0.x versions promise from here on.

Most code needs three changes: `use ponko::prelude::*;`,
`#[derive(Component)]` on its components, and the service in front of `Ctx`
calls (`ctx.input.mouse_down(..)`). The compiler names the rest; look them up
below.

## C and C++

Nothing to change for C++ code written against 0.0.1 beyond the rename: include
`ponko.hpp` and use the `ponko::` namespace, as
[Naming and compatibility](naming.md) describes. No C++ call was removed, and
`ponko::error` is still a `std::runtime_error`. 0.1.0 adds C++ wrappers for the
C functions that had none; [C and C++](c-cpp.md#c-functions-without-a-c-wrapper)
lists the few that stay C only.

## Imports

| 0.0.1 | 0.1.0 |
|---|---|
| `use ponko::*;` (for the game types) | `use ponko::prelude::*;` |
| `ponko::Ctx`, `Game`, `Phase`, `World`, `Entity`, `Vec2`, `Color`, `Sprite`, `Transform3D`, ... (any prelude item at the root) | `ponko::prelude::Ctx`, ... or its module (`ponko::game::Game`, `ponko::ecs_core::World`, `ponko::color::Color`, ...) |
| `ponko::SystemConfig` | `ponko::game::SystemConfig` |
| `ponko::Rect` | `ponko::camera::Rect` |
| `ponko::BodyKind`, `CollisionStarted`, `CollisionStopped` | `ponko::collision::*` (also in `d2`, `d3` and the prelude) |
| `ponko::Error`, `ErrorKind` | `ponko::error::Error`, `ErrorKind` |
| `ponko::Key`, `MouseButton` | `ponko::input::Key`, `MouseButton` (also in the prelude) |
| `ponko::json!` | `ponko::prelude::json!` |
| `ponko::RigidBody`, `Collider`, `Velocity`, `Shape`, `Physics`, `TileMap`, `Attached`, `Animation` | `ponko::d2::*` |
| `ponko::RayHit` | `ponko::d2::Hit` |
| `ponko::<Name>3D` (`Collider3D`, `RigidBody3D`, `PointLight3D`, `Hit3D`, `Text3D`, ...) | `ponko::d3::<Name>` (`d3::Collider`, `d3::RigidBody`, ...) |
| `ponko::AudioListener3D`, `AudioSettings3D`, `AudioSource3D` | `d3::AudioListener`, `d3::AudioSettings`, `d3::AudioSource` |
| `ponko::NavMesh3D`, `NavAgent3D`, `NavObstacle3D` | `d3::NavMesh`, ... (or `ponko::nav::NavMesh`, ...) |
| `ponko::d3::Light` | `ponko::d3::PointLight` |
| `ponko::render::ParticleCurve`, `ParticleGradient` | `ponko::d3::ParticleCurve`, `ParticleGradient` |
| `ponko::render::two_bone_ik`, `look_at_joint` | `ponko::anim::two_bone_ik`, `look_at_joint` |
| `ponko::anim::Blend` | `ponko::render::Blend` (also in `d3`) |
| `ponko::residency::{ResidencyKind, ResidencyStats}` | `ponko::streaming::{ResidencyKind, ResidencyStats}` |
| `ponko::ui::layout` (Taffy) | `ponko::ui::UiLayout`; raw Taffy as `ponko::ui::unstable_taffy` |
| `ponko::serde_json`, `ponko::Value` | `ponko::reflect::json`, `ponko::reflect::json::Value` |
| `ponko::steamworks` | `ponko::net::steam` |
| `ponko::QueryFilter`, `ViewQuery` (ECS) | `ponko::ecs_core::QueryFilter`, `ViewQuery` |
| `ponko::Interpolate`, `FieldType`, `ValueSchema`, `RawFlowView`, `RawGridView` | `net::Interpolate`, `reflect::FieldType`, `reflect::ValueSchema`, `flow::RawFlowView`, `spatial::RawGridView` |
| `ponko::step_physics`, `draw_sprites`, `animate_sprites`, ... (built-in systems) | `ponko::systems::step_physics`, ... |
| Any other root name (`ponko::Net`, `ponko::Graphics`, `ponko::Animator`, ...) | its module: `net`, `gfx`, `anim`, `audio`, `input`, `perf`, `render`, `terrain`, `ui`, ... (the changelog lists every one) |
| `MAX_DECALS`, `MAX_DECALS_PER_CLUSTER` | `d3::Decal::MAX`, `d3::Decal::MAX_PER_CLUSTER` |
| `MAX_GI_PROBES`, `MAX_GI_PROBES_PER_FRAME` | `render::GlobalIllumination::MAX_PROBES`, `MAX_PROBES_PER_FRAME` |
| `MAX_LIGHT_SHADOW_VIEWS`, `LIGHT_SHADOW_SIZE`, `LIGHT_SHADOW_ATLAS_SIZE` | `d3::PointLight::MAX_SHADOW_VIEWS`, `SHADOW_SIZE`, `SHADOW_ATLAS_SIZE` |
| `MIN_RENDER_SCALE`, `MAX_RENDER_SCALE`, `GraphicsQuality::MIN_RENDER_SCALE` | `render::Post::MIN_RENDER_SCALE`, `MAX_RENDER_SCALE` |
| `TERRAIN_LODS`, `TERRAIN_LAYERS` | `terrain::TerrainTile::LODS`, `LAYERS` |
| `SPATIAL_SORT_EVERY` | `spatial::SpatialGrid::<()>::SORT_EVERY` |

A dependency named like a root module (`audio`, `input`, `net`, `render`,
`ui`, `random`, `events`, ...) is ambiguous under `use ponko::*;` (E0659):
write `::net::...` for the crate, or import the prelude instead.

## Components

| 0.0.1 | 0.1.0 |
|---|---|
| `struct Health(f32);` used as a component | `#[derive(Component)] struct Health(f32);` (or `impl Component for Health {}`) |
| `world.spawn((5u32,))`, a `String`, `Vec`, `Arc` or `Entity` as a component | a newtype: `#[derive(Component)] struct Score(u32);`, `world.spawn((Score(5),))` |
| `ponko_ecs = { default-features = false }` | add `features = ["derive"]` to keep the derive |

Resources and events need no derive.

## `Ctx` methods

`Ctx` keeps these shortcuts: `dt`, `key_down`, `key_pressed`,
`key_released`, `mouse_pos`, `mouse_world`, `window_size`, `quit`, `draw`,
`draw_text`, `load_texture`, `load_sound`, `play_sound` and `random`, and the
calls that need several services: `free`, `is_live`, `live_counts`,
`load_scene`, `load_scene_lockstep`, `spawn_prefab`, `finish_uploads`,
`start_ragdoll`, `stop_ragdoll`, `draw_skeleton`, `asset_inventory`,
`ecs_diagnostics`, `capabilities_json` and `control_addr`. Every other method
moved to a service.

Moved with the same name (`ctx.name(..)` → `ctx.<service>.name(..)`):

| Service | Methods |
|---|---|
| `ctx.input` | `action_axis`, `action_axis2`, `action_down`, `action_id`, `action_ids`, `action_pressed`, `action_released`, `actions`, `cursor_locked`, `gamepad_axis`, `gamepad_down`, `gamepad_pressed`, `gamepad_released`, `gamepad_stick`, `gamepads`, `mouse_delta`, `mouse_down`, `mouse_motion`, `mouse_pressed`, `mouse_released`, `mouse_wheel`, `set_actions`, `set_cursor_locked`, `try_action_id`, `typed_text` |
| `ctx.gfx` | `clear`, `create_compute`, `create_effect`, `create_gpu_buffer`, `create_instance_batch`, `create_material`, `create_mesh`, `create_plain_mesh`, `create_render_target`, `create_shader`, `create_skinned_mesh`, `create_terrain_material`, `create_terrain_tile`, `create_texture`, `create_texture_array`, `create_ui_panel`, `decal_textures`, `default_material`, `draw_circle`, `draw_ex`, `draw_line`, `draw_rect`, `draw_text_ex`, `gpu_buffer_len`, `gpu_capabilities`, `has_texture_array`, `instance_batch_len`, `is_render_target`, `is_resident`, `load_clip`, `load_clips`, `load_compute`, `load_effect`, `load_font`, `load_mesh`, `load_shader`, `load_skeleton`, `load_texture_array`, `loaded_materials`, `measure_text`, `measure_text_ex`, `mesh_costs`, `mesh_shadows`, `models_drawn`, `morph_target`, `reset_temporal_history`, `residency`, `save_screenshot`, `set_decal_textures`, `set_mesh_gpu_bounds`, `set_mesh_gpu_buffer`, `set_mesh_shadows`, `set_texture_filter`, `shader_status`, `sprite_count`, `supports_decals`, `update_instance_batch`, `update_material`, `update_texture`, `upload_terrain_tile`, `warm_render_pipelines`, `write_gpu_buffer`, and every `try_` form of these |
| `ctx.gfx` (streaming and residency) | `stream_texture`, `stream_mesh`, `resident_mesh`, `try_resident_mesh`, `pin_resident_mesh`, `unpin_resident_mesh`, `resident_texture`, `try_resident_texture`, `pin_resident_texture`, `unpin_resident_texture`, `set_residency_budget` |
| `ctx.time` | `fixed_alpha`, `fixed_dt`, `fixed_steps`, `fps`, `frame`, `is_paused`, `pause`, `resume`, `sim_tick`, `sim_time`, `step`, `turns` |
| `ctx.perf` | `display_wait_ms`, `frame_stats`, `gpu_passes`, `profiling`, `record_trace`, `render_stats`, `scope_timings`, `set_profiling`, `system_timings` |
| `ctx.audio` | `load_stream`, `try_load_stream` |
| `ctx.assets` | `reloaded_files`, `reloaded_textures` |

Renamed or with new arguments:

| 0.0.1 | 0.1.0 |
|---|---|
| `ctx.time()` | `ctx.time.elapsed()` |
| `ctx.sim_speed()`, `set_sim_speed(x)` | `ctx.time.speed()`, `set_speed(x)` |
| `ctx.feed_input(e)` | `ctx.input.feed(e)` |
| `ctx.ui_insert_text(t)` | `ctx.input.insert_text(t)` |
| `ctx.create_sound`, `try_load_sound`, `stop_sound`, `set_sound_effects` | `ctx.audio.create`, `try_load`, `stop`, `set_effects` |
| `ctx.play_sound_ex`, `play_sound_on`, `play_sound_at` | `ctx.audio.play_ex`, `play_on`, `play_at` |
| `ctx.audio_bus`, `set_audio_bus_volume`, `mute_audio_bus` | `ctx.audio.bus`, `set_bus_volume`, `mute_bus` |
| `ctx.audio_listener()`, `set_audio_listener(l)` | `ctx.audio.listener()`, `set_listener(l)` (the camera as it was when the system started) |
| `ctx.mouse_over_ui()` | `ctx.ui.wants_pointer()` (add `\|\| ctx.ui.immediate.covers(ctx.input.mouse_pos())` for the debug panels too) |
| `ctx.ui_wants_keyboard()`, `set_ui_pointer_ray(r)` | `ctx.ui.wants_keyboard()`, `set_pointer_ray(r)` |
| `ctx.ui_panel(..)`, `ui_label(..)`, `ui_button(..)` | `ctx.ui.immediate.panel(&mut ctx.gfx, ..)`, `.label(&mut ctx.gfx, ..)`, `.button(&mut ctx.gfx, &ctx.input, ..)` |
| `ctx.add_asset_source(..)`, `load_asset_with(..)` | `ctx.assets.add_source(..)`, `load_with(..)` |
| `ctx.load_resource::<R>(p)`, `load_resource_json`, `load_component_json` (and `_named`) | the same on `ctx.assets`, with `&mut ctx.world` first |
| `ctx.save_scene(p)`, `save_prefab(e, p)` | `ctx.assets.save_scene(&ctx.world, p)`, `save_prefab(&ctx.world, e, p)` |
| `ctx.set_perf_hud`, `perf_hud`, `perf_report` | `ctx.perf.set_hud`, `hud`, `report` |
| `ctx.profile(name, ..)` | `let _timing = ctx.perf.scope(name);` |
| `ctx.graphics_quality()` | `ctx.scene3d.quality()` (`None` for custom settings) |
| `ctx.set_graphics_quality(q)` | `ctx.scene3d.set_quality(GraphicsQuality::Low)` (or `Medium`, `High`, `Ultra`) |
| `ctx.has_font(f)`, `has_material`, `has_shader`, `has_compute`, `has_effect`, `is_mesh_live` | `ctx.is_live(h)` (or `ctx.gfx.is_live(h)`) |
| `ctx.free_mesh(m)`, `free_instance_batch(b)`, `free_gpu_buffer(b)` | `ctx.free(h)` |
| `ctx.load_mesh_data(..)` | `ctx.gfx.try_load_mesh_data(..)` (or `load_mesh_data`, which panics) |
| `ctx.bake_reflection_probe`, `load_reflection_probe`, `write_animated_morphs` | the same on `ctx.gfx`, with `&mut ctx.world` first |
| `ctx.draw(&t, pos)`, `ctx.play_sound(&s)` | `ctx.gfx.draw(t, pos)`, `ctx.audio.play(s)` (handles by value; also `draw_ex`, `set_texture_filter`, `update_texture`, `play_ex`, `play_on`, `play_at`) |
| `ctx.spawn_prefab(path, (x, y).into())` | `ctx.spawn_prefab(path, vec2(x, y))` (also a `Vec3`, `d2::Transform` or `d3::Transform`) |

## `Ctx` fields

| 0.0.1 | 0.1.0 |
|---|---|
| `ctx.ui_retained` | `ctx.ui` |
| `ctx.ui_style` | `ctx.ui.immediate.style` |
| `ctx.camera3d`, `lighting`, `sky`, `post`, `lod` | `ctx.scene3d.camera`, `.lighting`, `.sky`, `.post`, `.lod` |
| `ctx.compute_passes`, `scene_compute_passes`, `target_cameras`, `target_effects`, `effects` | `ctx.scene3d.passes.compute`, `.scene_compute`, `.target_cameras`, `.target_effects`, `.effects` |
| `ctx.net` (a `Net`) | `ctx.net.lockstep` (`ctx.net` is `Networking`, with `messages`, `snapshots` and `interest`) |

## Systems

| 0.0.1 | 0.1.0 |
|---|---|
| `game.late_system(f)` | `game.add(Phase::Late, f)` |
| `game.sim_system_every(n, f)` | `game.add(Phase::AfterPhysics, f.every(n))` |
| `game.add_with_access(phase, access, f)`, `add_view(phase, access, f)` | `game.add(phase, f.access(access))` |
| `game.add_view_system(phase, access, f)` | `game.add_system(phase, f.access(access))` (returns a `SystemId`) |
| `named_add`, `named_sim_system`, `named_system`, ... | `game.add(phase, f.named(name).every(ticks))` |
| `game.add(phase, \|ctx\| ..)` | `game.add(phase, \|ctx: &mut Ctx\| ..)`; with options, `(\|ctx: &mut Ctx\| ..).every(4)` |
| `game.remove_system(f)`, `remove_named_system(name)` | `game.remove(id)` (a `Systems` constant or the id from `add_system`) |
| `game.add_setup(f)` | `game.setup(f)`, or a `Plugin` |
| `game.order_problem()` | `game.check_order()` (`Result<(), schedule::OrderError>`) |
| `game.msaa(n)` | `ctx.scene3d.post.msaa = n` in a setup callback |
| `ViewState::new(ViewAccess::new().read::<A>())` | `ViewState::new().read::<A>()` |
| `a.access().overlaps(b.access())` | `a.overlaps(&b)` |

`sim_system(f)`, `system(f)` and `run(..)` are unchanged.

In a view system (`sys: &mut SystemView`), the services are fields named as
on `Ctx`:

| 0.0.1 | 0.1.0 |
|---|---|
| `sys.action_down(a)`, `action_pressed`, `action_released`, `action_axis`, `action_axis2` | `sys.input.action_down(a)`, ... |
| `sys.mouse_down(b)`, `mouse_pressed`, `mouse_released` | `sys.input.mouse_down(b)`, ... |
| `sys.sim_tick()`, `fixed_dt()`, `frame()`, `time()` | `sys.time.sim_tick()`, `fixed_dt()`, `frame()`, `elapsed()` |
| `sys.play_sound_ex(s, p)` | `sys.audio.play_ex(s, p)` |
| `sys.camera()` / `camera_mut()`, `camera3d()` / `camera3d_mut()` | `sys.camera`, `sys.camera3d` |
| `sys.spatial()`, `physics()` / `physics_mut()`, `physics3d()` / `physics3d_mut()` | `sys.spatial`, `sys.physics`, `sys.physics3d` |
| `sys.net()` / `net_mut()` | `sys.net.lockstep` |
| `sys.commands()`, `events()` | `sys.commands`, `sys.events` |

## ECS

| 0.0.1 | 0.1.0 |
|---|---|
| `query.with::<T>()`, `.without::<T>()` | `query.filter::<With<T>>()`, `.filter::<Without<T>>()`; several as a tuple |
| a `Changed<T>` / `Added<T>` filter that starts tracking | `world.track_changes::<T>()` in setup first |
| `world.register_component(..)`, `insert_dynamic`, `spawn_dynamic`, `spawn_at_dynamic`, `remove_dynamic`, `insert_resource_dynamic`, `remove_resource_dynamic`, `for_each_chunk` | the same on `world.raw()` |
| `world.track_changes_dynamic(id)`, `mark_changed_dynamic(e, id)` | `world.raw().track_changes_dynamic(id)`, ... |
| `world.get_dynamic`, `get_resource_dynamic`, `resource_layout_dynamic`, `component_by_name`, `archetypes`, `removed_dynamic` | the same on `world.raw_ref()` (read pointers are `*const u8`) |
| writing through `get_dynamic` / `get_resource_dynamic` | `world.raw().get_dynamic_mut(..)`, `get_resource_dynamic_mut(..)` |
| `world.query_chunks(q, f)` | `world.raw().query_chunks(q, f)` |
| `world.query_chunks_filtered(q, &filter, f)` | `world.raw().query_chunks(q.filter(&filter), f)` |
| `world.query_chunks_cached(q, filter, cache, f)` | `world.raw().query_chunks(q.cache(cache), f)` |
| `ChunkQuery { writes: 0, changed: 1, ..ChunkQuery::new(&ids) }` | `ChunkQuery::new(&ids).writes(0).changed(1)` (`any` → `match_any`) |
| `state.prepare(w); state.view(w); state.finish(w)` | `state.run(&mut world, \|view\| ..)`, or `ViewGroup` |
| `ViewState::set_changes_since`, `set_change_tick` | `override_changes_since`, `override_change_tick` |
| custom `QueryData`, `Bundle`, `ResourceSet` impls | the provided references, `Option`, `Entity` and tuples (the traits are sealed) |
| an event type with `Rc` or `RefCell` | `Arc`, a `Mutex`, or plain data (`Events::send` needs `Send + Sync`) |
| `AccessLog { .. }` and other struct literals of `#[non_exhaustive]` types | `AccessLog::default()` and assign the fields |

## Handles and errors

| 0.0.1 | 0.1.0 |
|---|---|
| `Handle::id`, `Handle::from_id`, `CollisionMesh::to_bits` / `from_bits` | keep the handle itself (`Copy`, `Eq`, `Hash`); `ctx.is_live(h)` |
| `from_id(u32::MAX)` | `Handle::NONE` (`Texture::NONE`, `Mesh::NONE`, ...) |
| `Graphics::has_material`, `has_shader`, `has_compute`, `has_effect`, `has_font`, `is_mesh_live` | `ctx.gfx.is_live(h)` |
| `Graphics::free_mesh`, `free_instance_batch`, `free_terrain_tile` | `ctx.gfx.free(h)` (or `ctx.free(h)`) |
| `ctx.physics3d.free_mesh(m)`, `is_mesh_live(m)` | `ctx.free(m)`, `ctx.is_live(m)` |
| `CollisionMesh::default()` | `CollisionMesh::NONE` |
| `tile.lods`, `tile.lod_distance` | `tile.lods()`, `tile.lod_distance()`, `tile.with_lod_distance(d)` |
| `Result<_, String>` from `load_scene`, `spawn_prefab`, `start_ragdoll`, the `try_` loaders, `PerfReport::from_json`, `MeshData::check`, ... | `Result<_, ponko::error::Error>`: `e.message()`, `e.kind()`, `e.to_string()` |

## Rendering

| 0.0.1 | 0.1.0 |
|---|---|
| `GraphicsQuality::low()`, ... (a struct of settings) | `GraphicsQuality::Low`, ... (a preset): `ctx.scene3d.set_quality(..)` |
| `quality.level()`, `QualityLevel::Custom` | `ctx.scene3d.quality()`, `None` |
| `GraphicsQuality::for_tier(t)`, `recommended(caps)` | `ctx.scene3d.set_recommended_quality(caps)` (`tier.report()` for a tier) |
| `quality.fitted_to(caps)`, `quality.clamped()` | `ctx.scene3d.fit_quality_to(caps)`, `ctx.scene3d.clamp_quality()` |
| `ctx.gfx.set_quality_autotune`, `freeze_quality_autotune`, `quality_autotune_status` | `ctx.scene3d.set_quality_autotune`, `freeze_quality_autotune`, `quality_autotune_status(&ctx.gfx)` |
| `pin_render_scale(&mut ctx.post, s)`, `pin_shadow_resolution(&mut ctx.lighting, r)` | `ctx.scene3d.pin_render_scale(s)`, `pin_shadow_resolution(r)` |
| `ctx.gfx.set_msaa(n)` | `ctx.scene3d.post.msaa = n` |
| `Lighting::sky_color`, `ground_color`, `equator_color` | `Sky::sky_color`, `ground_color`, `equator_color` |
| `lighting.shadow_filter = 1` / `2` / `3` | `ShadowFilter::Sharp` / `Soft` / `Softest` |
| `Post { bloom: 0.1, ..Default::default() }` (and `Lighting`, `Sky`, `LodSettings`) | `let mut post = Post::default(); post.bloom = 0.1;` |
| `MaterialDesc { roughness: 0.9, ..MaterialDesc::color(c) }` | `MaterialDesc::color(c).with_roughness(0.9)` |
| `ParticleSettings { rate: 40.0, .. }` | `ParticleSettings::default().with_rate(40.0)` |
| `TerrainMaterialDesc { roughness: 0.8, ..TerrainMaterialDesc::new(..) }` | `TerrainMaterialDesc::new(..).with_roughness(0.8)` |
| `DrawParams { rotation: 0.5, .. }`, `TextParams { size: 24.0, .. }` | `DrawParams::default().with_rotation(0.5)`, `TextParams::default().with_size(24.0)` |
| `MaterialVariantKey`, `MaterialSides`, `MaterialInstancing`, `MaterialDesc::variant_key` | removed (internal) |
| `render::SceneBench`, `LodBench`, `ExtractionBench`, ... | the unstable `bench-internals` feature |

Matches on `Blend`, `AntiAliasing`, `Tonemap`, `ShadowFilter`,
`Interpolation`, `TextureKind`, `ParticleShape`, `ParticleBlend` and the
other `#[non_exhaustive]` enums need a `_` arm.

## Animation

| 0.0.1 | 0.1.0 |
|---|---|
| state, parameter and layer indices (`usize`) | `StateId(n)`, `ParamId(n)`, `LayerId` |
| `AnimationCondition::Greater(0, v)` | `AnimationCondition::Greater(ParamId(0), v)` |
| `Motion::Blend1D { parameter: 0, .. }` | `parameter: ParamId(0)` (also `Blend2D`, `AnimationState::speed_parameter`) |
| `animator.set_float(0, v)`, `set_bool`, `set_trigger`, `parameter` | `animator.set_float(ParamId(0), v)`, ... |
| `animator.play(1, ..)`, `crossfade(1, ..)` | `play(StateId(1), ..)`, `crossfade(StateId(1), ..)` |
| `AnimationTransition { from: Some(0), to: 1, conditions, duration, .. }` | `AnimationTransition::new(StateId(0), StateId(1)).with_condition(c).with_duration(d)` |
| `AnimationTransition { from: None, .. }` | `AnimationTransition::from_any(to)` |
| `AnimationGraph::new(params, states, transitions, 0)` (panics) | `AnimationGraph::new(params, states, transitions, StateId(0))?` |

## UI

| 0.0.1 | 0.1.0 |
|---|---|
| `ui.add(w, Style::default())`, `add_child`, `set_layout`, `UiBuilder::layout` with a Taffy `Style` | a `UiLayout`: `UiLayout::auto()`, `row()`, `column()`, ..., or `UiLayout::custom(style)` |
| `Ui::layout_style(id) -> Option<&Style>` | `Ui::node_layout(id) -> Option<UiLayout>` |
| `Style::from(layout)` | `layout.into_style()` |
| `Display::None` through Taffy | `UiLayout::hidden()` |
| `ui.take_click()` | `ui.events().filter_map(UiEvent::clicked).last()` |

## Physics

| 0.0.1 | 0.1.0 |
|---|---|
| 2D `physics.cast_ray(o, d, max)` | `physics.cast_ray(o, d, max, d2::QueryFilter::default().with_sensors())` |
| `cast_ray_filtered(..)` (2D and 3D) | `cast_ray(..)` |
| 2D `overlap_ball` | `overlap_circle` |
| 3D `overlap_ball` | `overlap_sphere` |
| `entities_at(p, out)` | `overlap_point(p, QueryFilter::default().with_sensors(), out)` |
| `entities_in_circle(c, r, out)` | `overlap_circle(c, r, QueryFilter::default().with_sensors(), out)` |

## Settings, tools and saved files

| 0.0.1 | 0.1.0 |
|---|---|
| `PONKO_CONTROL=1 cargo run --release` | `PONKO_CONTROL=1 cargo run --release --features control-env`, or `Game::control_endpoint(true)` |
| control endpoint paths anywhere (`/tmp/before.json`) | relative paths inside the game's folder (`saves/before.json`); `overwrite: true` to replace a world save |
| `CRABBY_*` environment variables | `PONKO_*` |
| `crabby-world` / `crabby-prefab` save tags, `crabby*` glTF extras | `ponko-world` / `ponko-prefab`, `ponko*` (see [Naming and compatibility](naming.md)) |
| `Color::CRABBY_DARK` | `Color::PONKO_DARK` |
