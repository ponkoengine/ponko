# Changelog

All notable changes to Ponko (formerly Crabby Engine) are listed here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/) as Cargo applies
it to 0.x; [STABILITY.md](STABILITY.md) says what that covers.

## [Unreleased]

## [0.1.0] - 2026-11-13

The first release with the Ponko API in its intended shape, and the one
deliberately breaking release of the rename. `Ctx` is grouped into services
(`ctx.gfx`, `ctx.input`, `ctx.audio`, `ctx.scene3d`, ...), the crate is
organized into modules with one curated `prelude`, systems are registered one
way (`Game::add`), ECS components opt in with `#[derive(Component)]`, and
every deprecated alias, flat forwarder and `crabby` name is gone. Around that:
optional parallel systems, change detection and typed query filters, a
retained UI with builders, themes and screen-reader support, asset streaming
with memory budgets, GPU tiers with quality presets and automatic tuning, and
inspection tools for agents. Versions now follow a 0.x policy
([STABILITY.md](STABILITY.md)). Every removed or renamed Rust item and its
replacement is in the guide's
[migration table](https://ponko.dev/guide/migrating-to-0.1.html); the C ABI is
unchanged (version 2).

Highlights:

- **Services on `Ctx`**: `ctx.time`, `ctx.input`, `ctx.gfx`, `ctx.audio`,
  `ctx.ui`, `ctx.assets`, `ctx.perf`, `ctx.net` and `ctx.scene3d` are separate
  fields, so they borrow apart (draw with `ctx.gfx` inside a `ctx.world`
  query).
- **One prelude, public modules**: `use ponko::prelude::*;`, then `d2::*` or
  `d3::*`; everything else has one documented path in its module.
- **`#[derive(Component)]`** from the new `ponko_macros` crate.
- **Systems**: `Game::add(phase, system)` with `.named()`, `.every()` and
  `.access()`, plugins, removable built-in systems with stable ids, view
  systems with declared access, and an optional parallel scheduler that gives
  the same results with any number of threads.
- **ECS**: change detection (`Added<T>`, `Changed<T>`, `removed`), typed
  filters (`With`, `Without`, `Or`, tuples), `Query::single`, borrowing
  several resources or entities at once, and an automatic archetype index for
  worlds with many archetypes.
- **Retained UI**: builders, layout presets, themes, nodes found by name, and
  accessibility (VoiceOver on macOS with the `accessibility` feature).
- **Assets**: streaming without stalls, residency budgets, asset sources
  (memory, archives, your own), one handle lifecycle (`ctx.is_live`,
  `ctx.free`) and an asset inventory for finding leaks.
- **Rendering**: GPU tiers, quality presets fitted to the GPU and automatic
  quality tuning, kept shadow maps, incremental frame collection, GPU-driven
  static batches, per-model materials, custom compute passes and target
  effects, and located shader errors with hot reload.
- **Tools**: ECS and render inspectors, input scenarios, validation bundles
  and new MCP tools; a hardened control endpoint.
- **A complete game**: `coin_dash`, packaged, smoke-tested on every desktop
  and in browsers.

### Breaking changes

Each bullet names the replacement; the
[migration table](https://ponko.dev/guide/migrating-to-0.1.html) lists every
old → new pair.

**Imports and crate layout**
- The crate root holds only modules. The prelude's items are no longer also
  at the root, and the old root spellings of moved items are removed (they
  were re-exports, which rustc does not warn through): `ponko::Ctx` →
  `ponko::prelude::Ctx`, `use ponko::*;` for the prelude →
  `use ponko::prelude::*;`, `ponko::RigidBody3D` → `ponko::d3::RigidBody`,
  `ponko::Net` → `ponko::net::Net`, `ponko::Error` → `ponko::error::Error`,
  `ponko::Rect` → `ponko::camera::Rect`, `ponko::BodyKind` →
  `ponko::collision::BodyKind`, and so on.
- The `*3D`-suffixed names are the `d3` names (`Collider3D` → `d3::Collider`,
  `PointLight3D` → `d3::PointLight`, `Hit3D` → `d3::Hit`, ...); the prelude
  keeps `Transform2D`, `Camera2D`, `Transform3D` and `Camera3D`.
- Root constants became associated constants (`MAX_DECALS` →
  `d3::Decal::MAX`, `MIN_RENDER_SCALE` → `render::Post::MIN_RENDER_SCALE`,
  `TERRAIN_LODS` → `terrain::TerrainTile::LODS`, ...); built-in system
  functions moved to `ponko::systems`; re-exported crates moved under their
  module (`serde_json` → `reflect::json`, `steamworks` → `net::steam`,
  `ui_layout` → `ui::unstable_taffy`).
- New root modules (`audio`, `input`, `net`, `render`, `ui`, ...) can clash
  with a dependency of the same name under `use ponko::*;` (E0659): write
  `::name::...` for the crate, or use the prelude.
- `ponko::d3::Light` → `d3::PointLight`; `render::ParticleCurve` /
  `ParticleGradient` → `d3::`; `render::two_bone_ik` / `look_at_joint` →
  `anim::`; `anim::Blend` → `render::Blend`; the `ponko::residency` module is
  merged into `ponko::streaming`. `Animation` (sprite sheets) is `d2::Animation`
  and no longer in the prelude.

**Components**
- A type is a component only with `#[derive(Component)]` (or
  `impl Component for T {}`); using another type in `spawn`, `insert`, a
  query or a filter is a compile error that says so. Numbers, `String`,
  `Vec`, `Arc` and `Entity` are not components: wrap them in a newtype.
  `ponko_ecs` has a default `derive` feature (`--no-default-features` →
  add `features = ["derive"]`). Resources and events need no derive.

**`Ctx` and its services**
- The ~180 flat `Ctx` methods that forwarded to a service are removed: call
  the service (`ctx.mouse_down(b)` → `ctx.input.mouse_down(b)`,
  `ctx.create_mesh(..)` → `ctx.gfx.create_mesh(..)`, `ctx.sim_speed()` →
  `ctx.time.speed()`, `ctx.play_sound_ex(..)` → `ctx.audio.play_ex(..)`,
  `ctx.perf_report()` → `ctx.perf.report()`, `ctx.mouse_over_ui()` →
  `ctx.ui.wants_pointer()`, ...). `Ctx` keeps the everyday shortcuts (`dt`,
  `key_down`, `key_pressed`, `key_released`, `mouse_pos`, `mouse_world`,
  `window_size`, `quit`, `draw`, `draw_text`, `load_texture`, `load_sound`,
  `play_sound`, `random`) and the calls that need several services (`free`,
  `is_live`, `load_scene`, `spawn_prefab`, `finish_uploads`, ...).
- A service call that touches another area takes it as an argument:
  `ctx.assets.save_scene(&ctx.world, path)`,
  `ctx.assets.load_resource::<R>(&mut ctx.world, path)`,
  `ctx.ui.immediate.button(&mut ctx.gfx, &ctx.input, rect, label)`, ...
- `ctx.ui_retained` → `ctx.ui`; `ctx.ui_style` → `ctx.ui.immediate.style`.
- `ctx.net` is a `Networking` service: the lockstep session is
  `ctx.net.lockstep` (`ctx.net = Net::host(..)` → `ctx.net.lockstep = ..`),
  next to `messages`, `snapshots` and `interest`.
- The 3D render settings are one field, `ctx.scene3d`: `ctx.camera3d`,
  `lighting`, `sky`, `post`, `lod` → `ctx.scene3d.camera`, `.lighting`, ...;
  `ctx.compute_passes`, `scene_compute_passes`, `target_cameras`,
  `target_effects`, `effects` → `ctx.scene3d.passes.*`.
- Loading, streaming and residency live on `ctx.gfx`
  (`ctx.resident_mesh(p)` → `ctx.gfx.resident_mesh(p)`, `stream_texture`,
  `set_residency_budget`, ...); `ctx.add_asset_source` / `load_asset_with` →
  `ctx.assets.add_source` / `load_with`. Residency calls no longer evict:
  eviction happens in the streaming step at the start of the next frame.

**Systems**
- One way to register: `Game::add(phase, system)` and `Game::add_system`
  (which returns a `SystemId`) take anything `IntoSystem`, with options from
  `.named(name)`, `.every(ticks)` and `.access(declaration)`. Removed:
  `late_system(f)` → `add(Phase::Late, f)`, `sim_system_every(n, f)` →
  `add(Phase::AfterPhysics, f.every(n))`, `add_with_access` and `add_view` →
  `add(phase, f.access(a))`, and the hidden `named_*` forms. Closures given to
  `add` name their parameter type (`|ctx: &mut Ctx| ...`). `sim_system` and
  `system` stay.
- Removal is by id: `remove_system(f)` / `remove_named_system(name)` →
  `Game::remove(id)` with a `Systems` constant or the id from `add_system`.
  `add_setup(f)` → `Game::setup(f)` or a `Plugin`; `order_problem()` →
  `check_order() -> Result<(), OrderError>`.
- `ViewAccess` is gone: `ViewState::new().read::<A>().write::<B>()` declares
  directly. `SystemView` uses `Ctx`'s names (`sys.action_down(a)` →
  `sys.input.action_down(a)`, `sys.sim_tick()` → `sys.time.sim_tick()`,
  `sys.camera()` → `sys.camera`, `sys.net()` → `sys.net.lockstep`, ...).
- View systems must be `Send`, and their event types `Send + Sync`;
  `Events::send` needs `T: Send + Sync + 'static`.

**ECS**
- `QueryData`, `ReadOnlyQueryData`, `Bundle`, `ResourceSet` and
  `ResourceTuple` are sealed.
- `.with::<T>()` / `.without::<T>()` → `.filter::<With<T>>()` /
  `.filter::<Without<T>>()` (several as a tuple).
- A `Changed<T>` or `Added<T>` filter no longer starts tracking `T`; it
  panics for an untracked component: call `world.track_changes::<T>()` in
  setup (C and C++ filters still start tracking on first use).
- By-id calls moved behind `world.raw()` (changes) and `world.raw_ref()`
  (reads): `register_component`, `insert_dynamic`, `spawn_dynamic`,
  `query_chunks` (with `ChunkQuery::filter` / `cache` instead of the
  `_filtered` / `_cached` variants), `get_dynamic`, `removed_dynamic`, ...
  Neither view derefs to `World`. Read pointers are `*const u8`; writes go
  through `get_dynamic_mut` and chunk loops.
- `ChunkQuery` is built with methods (`ChunkQuery::new(&ids).writes(0).changed(1)`).
  `ViewState::prepare` / `view` / `finish` are private → `state.run(..)` or
  `ViewGroup`; `ViewState::set_changes_since` / `set_change_tick` →
  `override_changes_since` / `override_change_tick`.

**Handles and errors**
- Handles are no longer made from or read as numbers (`Handle::id`,
  `from_id`, `CollisionMesh::to_bits` are removed): keep the handle (it is
  `Copy`, `Eq`, `Hash`) and check it with `ctx.is_live(h)`;
  `Handle::NONE` is the empty handle. Textures and sounds are passed by value
  (`ctx.draw(&t, pos)` → `ctx.draw(t, pos)`, `ctx.play_sound(&s)` →
  `ctx.play_sound(s)`).
- `has_material`, `has_shader`, `has_compute`, `has_effect`, `has_font`,
  `is_mesh_live` → `is_live(h)`; `free_mesh`, `free_instance_batch`,
  `free_gpu_buffer`, `free_terrain_tile`, `physics3d.free_mesh` → `free(h)`.
  `TerrainTile` is an opaque handle (`tile.lods()`); `CollisionMesh` is a
  handle that no longer serializes (`CollisionMesh::default()` →
  `CollisionMesh::NONE`).
- Fallible calls return `ponko::error::Error` instead of `String`
  (`load_scene`, `spawn_prefab`, `start_ragdoll`, the `try_` loaders and
  creators, `PerfReport::from_json`, ...): read the text with `e.message()`
  or match `e.kind()`. `load_mesh_data` and `load_reflection_probe` panic,
  next to new `try_` forms.

**Rendering, assets, UI and physics**
- Graphics quality is a preset enum (`GraphicsQuality::Low` ... `Ultra`)
  applied with `ctx.scene3d.set_quality(..)`, read with
  `ctx.scene3d.quality()`; `set_recommended_quality`, `fit_quality_to`,
  automatic quality and the render-scale and shadow pins moved to
  `ctx.scene3d`. MSAA is `ctx.scene3d.post.msaa` (`Game::msaa` and
  `set_msaa` are removed). The flat sky colors moved from `Lighting` to `Sky`;
  `Lighting::shadow_filter` is a `ShadowFilter` enum.
- `#[non_exhaustive]` structs and enums across the render, animation, UI,
  drawing and ECS APIs (`Post`, `Lighting`, `MaterialDesc`,
  `ParticleSettings`, `DrawParams`, `UiWidget`, `UiEvent`, ...): build from
  `Default` or a constructor and the new `with_*` methods
  (`MaterialDesc::color(c).with_roughness(0.9)`), and give matches a `_` arm.
- Animation graphs use typed `StateId`, `ParamId` and `LayerId`;
  `AnimationTransition::new(from, to).with_condition(..)`, and
  `AnimationGraph::new` returns a `Result` instead of panicking.
- `UiLayout` is the retained UI's layout type (`Ui::add(widget, UiLayout::auto())`,
  `UiLayout::custom(style)` for raw Taffy); `ponko::ui::layout` →
  `ponko::ui::unstable_taffy`; `Ui::take_click()` →
  `ui.events().filter_map(UiEvent::clicked).last()`.
- Physics queries match across dimensions: `cast_ray(origin, dir, max,
  filter)` (`cast_ray_filtered` is removed), `overlap_ball` →
  `overlap_circle` (2D) / `overlap_sphere` (3D), `entities_at` /
  `entities_in_circle` → `overlap_point` / `overlap_circle` with a filter.
  `Ctx::spawn_prefab` takes `impl Into<Placement>`.
- Renderer internals are private (`MaterialVariantKey`, ...); the benchmark
  drivers need the unstable `bench-internals` feature.

**Tools, settings and legacy names**
- In a release build, `PONKO_CONTROL=1` opens the control endpoint only with
  the new `control-env` feature (debug builds and
  `Game::control_endpoint(true)` are unchanged; `ponko-mcp launch` adds the
  feature). Endpoint file paths must be relative and stay inside the game's
  folder; `world.save` needs `overwrite: true` to replace a file.
- The `crabby` names are gone everywhere: the `crabby_*` C functions,
  `Crabby*` types, `CRABBY_*` constants and the `crabby.h` / `crabby.hpp`
  headers (use the `ponko` spellings; layouts and values are unchanged), the
  `CRABBY_*` environment variables, `crabby-world` / `crabby-prefab` save
  tags, `crabby*` glTF extras, the `crabby` canvas id, the old instance
  registry and `Color::CRABBY_DARK`.

### Added

**ECS**
- Change detection, opt in per component (`World::track_changes`):
  `Added<T>` and `Changed<T>` filters, `world.removed::<T>()`, and per-system
  change windows in games, also from C (`ponko_query_with`) and C++
  (`ponko::changed<T>`, `ponko::added<T>`). Untracked components cost
  nothing.
- Typed query filters `With`, `Without`, `Or` (with AND groups) and tuples on
  world and view queries, and the same filters from C (`ponko_filter_*`,
  `ponko_query_filtered`) and C++ (`ponko::with<T>`, `ponko::or_<...>`).
  Queries fetch up to 12 items.
- `Query::single()` and `ViewQuery::single()`; `World::resources_mut::<(A, B)>()`,
  `Query::with_resources`, `Query::with_lookup` and `World::get_many_mut`
  for borrowing several things in one loop without collecting.
- Queries find their archetypes through a bounded world-owned index once a
  world has 64 archetypes (`World::set_query_cache_policy`,
  `query_cache_stats`, also from C and C++); results and order are unchanged.
- `ViewState`, `WorldView` and `ViewGroup`: restricted world views that
  non-overlapping systems can share across threads. `World::id`,
  `World::entity_slots`, `Commands::clear`.

**Systems and the game loop**
- `Plugin` (`Game::plugin`), `Game::setup` with `ConfiguredGame`, and
  `Game::configured()` for games without a setup closure.
- `SystemId`, `Systems` constants for every built-in system, and the
  animator, particle and positional-audio updates as ordinary removable
  systems; `ponko::systems` holds the built-in functions.
- Declared system access (`SystemAccess`, `Service`) checked in every build
  for view systems and in debug builds for `Ctx` systems, with
  `Game::access_report()`. The physics and navigation steps declare theirs.
- Optional parallel scheduler (`Game::parallel_systems(true)`): neighbouring
  view systems without conflicts run as a batch; commands and events apply at
  the batch's end in registration order, so results match the sequential run.
  Batches go to other threads only past a measured cost threshold
  (`Game::parallel_threshold`). `Game::barrier`, `Game::order`,
  `Game::schedule_plan()`; in C and C++ too. Eight heavy systems over
  16,000 entities each: 7.15 → 1.04 ms (M4 Max).
- C and C++ systems in any phase (`ponko_game_add_system` with `PonkoPhase`),
  with ids, setup callbacks, plugins and declared access.

**Rendering**
- GPU capability report (`ctx.gfx.gpu_capabilities()`), GPU tiers
  (`webgl2`, `baseline`, `desktop`), `PONKO_GPU_TIER` to try a weaker tier,
  and quality presets fitted to the GPU.
- Automatic quality tuning from GPU timestamps
  (`ctx.scene3d.set_quality_autotune`), with bounded render-scale and
  shadow steps.
- `ModelMaterial` and `ModelMaterials` for every kind of model (skinned,
  morphing, LOD, terrain), saved in scenes; `ctx.gfx.create_plain_mesh`.
- Custom passes: scene compute passes after the lit frame, target effects
  that write their own render target, hazard checks naming the entry, and
  runtime MSAA changes.
- `LodGroup::instance_levels` and `LodHysteresis`; per-batch culling choice
  (`BatchCulling`) and `InstanceBatchStats`; per-instance colours and shadow
  meshes in persistent batches.
- Shader diagnostics: `ctx.gfx.shader_status()` with errors located in the
  game's own WGSL, and broken shaders that keep a stand-in under hot reload
  until a fix is saved. Explicit unlit shading (`Surface.unlit`).
- Upload accounting (`RenderStats::uploads`, `GpuUploads::reallocations`) and
  pipeline cache statistics (`Graphics::pipeline_stats`).

**Assets and streaming**
- `ctx.gfx.stream_texture` / `stream_mesh` decode on job threads within a
  per-frame upload budget; textures arrive as small stand-ins and sharpen
  under the same handle. `AssetState`, `ctx.finish_uploads()`,
  deterministic uploads for tests, and `StreamedLod`.
- Residency budgets for meshes and textures (`ctx.gfx.resident_mesh`,
  `set_residency_budget`, pinning): least recently used assets are evicted
  and come back when asked for.
- Asset sources (`ctx.assets.add_source`, `MemorySource`, your own
  `AssetSource`) for every kind of asset, `ctx.assets.load_with` for your own
  types, and asset fields in reflected components (`scene_asset_field`).
- One lifecycle for every handle (`ctx.is_live`, `ctx.free`,
  `ctx.live_counts`) with defined behavior for freed resources in use;
  `ctx.asset_inventory()` with `growth_since`, dangling references and
  per-handle ownership (`asset_handles`) for finding leaks;
  `ctx.gfx.unload_mesh(path)`.
- `Audio::try_load_stream`; streamed music from asset sources.

**UI and input**
- Retained UI builders (`Ui::panel`, `button`, `slider`, ... ending in
  `.id()`), `UiLayout` presets, `UiTheme` shared with the immediate widgets,
  titles, nodes found by name, and `UiLayout::hidden()`.
- Accessibility: `Ui::accessibility_tree()`, names, descriptions, live
  regions and actions from assistive technology, `UiAccessTracker`, and the
  `accessibility` feature, which exposes the UI to VoiceOver on macOS (other
  platforms build and do nothing yet).
- `ActionId` and `actions!`, `ActionMap` read-back and `Input::set_action`
  for rebinding screens.
- Input scenarios with checks and reports (`InputScenario::expect`,
  `Game::run_headless_scenario`, a JSON format) and the MCP tools
  `run_input_scenario` and `cancel_input_scenario`.
- `Time::frame_dt` and `Time::sim_dt`; `Ui::wants_pointer` covers every UI.

**Physics, navigation and networking**
- Filtered ray and overlap queries in both dimensions, and 3D prefab
  placement.
- `Ctx::load_scene_lockstep`, and loads that would desynchronize a lockstep
  session are refused.
- `Game::messages(endpoint)`: message endpoints polled by the engine before
  systems.

**Tools and diagnostics**
- `ctx.ecs_diagnostics()` (control `ecs.diagnostics`, MCP `ecs_diagnostics`):
  archetypes, storage, every system with its declared access and stage, and
  the schedule.
- `perf.get`'s `render_inspector`: CPU and GPU time per pass, counts, memory
  estimates, frame history and quality decisions, with `null` and a reason
  for what can't be measured; `quality.set` / MCP `set_graphics_quality`.
- `Ctx::capabilities_json` and GPU data in instance discovery; explicit,
  stricter JSON schemas for built-in components.
- Validation bundles: the `validation_bundle` example launches a game,
  drives it through MCP, records everything and compares runs. The MCP
  `launch` tool takes a prebuilt `program`.
- Retained compatibility baselines for the MCP tool contract, the C ABI and
  saved files, checked in CI.

**C and C++**
- Bindings for action ids, input scenarios, asset sources, resource scopes,
  error kinds and paths (`ponko_last_error_kind`, C++ `ponko::result<T>`),
  handles, streaming, model materials, UI builders, themes and accessibility,
  shader status, GPU tiers and the asset inventory.
- `PONKO_ABI_VERSION` in `ponko.h`, checked by `ponko.hpp` at startup.
- Every C export is called by a test, except an allow-listed few (drawing,
  opening a window, a live Steam lobby), checked by CI.
- Every C function has a C++ wrapper, except a short documented list where C++
  has its own way (`scripts/cpp-c-only.txt`), checked by CI
  (`scripts/check-cpp-api.py`). New: `query_cache_policy_default()` and
  `_off()`, `quat_identity()`, `friction_curve_forward()` and `_sideways()`,
  `collision_groups_all()`, `mesh_data::morph_target_count()` and
  `clear_morph_targets()`.
- Doc comments on every public declaration of `ponko.hpp`: what it does, units,
  ownership and what it throws.
- The guide's C and C++ snippets are all included from `examples/cpp`, which
  CI compiles and runs; the C and C++ chapter says how to get the library from
  a release.

**Web, packaging and the importer**
- 3D games run in browsers with only WebGL2; browser smoke tests in Chromium,
  Firefox and WebKit with 2D and 3D fixtures.
- `coin_dash`, a complete small game with settings, rebinding and saves,
  packaged with its notices and smoke-tested on Linux, macOS, Windows and the
  web.
- `ponko_unity`: model materials (FBX, OBJ) made the way Unity makes them,
  and clip splits read where Unity writes them.
- `ponko_macros`, the derive crate. Every published crate carries its
  license files and a README; release archives of the C library carry
  `THIRD-PARTY-NOTICES.txt`.

**Development**
- Benchmarks for the ECS against bevy_ecs and hecs, rendering scenes (with a
  Bevy counterpart), static batches, LOD, scheduling and streaming in
  `ponko_bench`; evaluations of mesh shaders, ray tracing and sparse-set
  storage (all deferred) with their numbers.
- An image regression suite for the 3D renderer with reviewed reference
  images, and compiled guide and README snippets.

### Changed
- [STABILITY.md](STABILITY.md) is a 0.x policy: 0.1.x releases are
  compatible with each other, a 0.y.0 may break and lists every break, and
  the MSRV (Rust 1.95) and the C ABI version change only in a 0.y.0. It names
  the unstable surfaces: `#[doc(hidden)]` items, `bench-internals`,
  `ui::unstable_taffy` and the re-exported `glam`, `serde_json` and
  `steamworks`.
- The 3D frame keeps its models between frames and updates only what
  changed (100,000 models, still scene: 0.45 → 0.20 ms to collect), uploads
  only changed buffer ranges (a still scene: 15,616 → 80 bytes a frame),
  keeps shadow maps that would be drawn the same, sorts draws by pipeline
  and material (5,393 → 57 pipeline changes in a 2,000-mesh scene), and
  culls GPU-driven static batches in one dispatch per page of views.
- Transparent models are sorted back to front across meshes.
- `ctx.gfx.models_drawn()` counts each model once.
- Parallel queries run all archetypes in one pass (many small archetypes:
  about 9× faster on 14 cores); adding and removing components is faster on
  entities with many components.
- For a tracked component, every mutable fetch marks the entities it
  reaches as changed. Fetch `&T` where you only read.
- Background `Job`s run on their own thread pool, so they never stall the
  frame's parallel work.
- `Character3D`s move on the thread pool from 16 characters, and reuse their
  buffers.
- With hot reload on, a shader, effect or compute file that doesn't compile
  loads as a stand-in reported in `shader_status` instead of failing.
- Custom material shaders that declare engine-owned bindings are rejected at
  load with a located error. WGSL `Surface` gains `unlit` after `emissive`.
- `ponko_bench` is no longer published; it is a workspace tool.
- The guide lives at https://ponko.dev/guide/ and links the API reference on
  docs.rs.
- Every published crate documents every public item, with a crate overview,
  a quickstart and links to the guide. On docs.rs, items behind a cargo
  feature say which one ("Available on crate feature `3d` only"), and the
  `ponko` docs include every optional feature except `hotpatch` and
  `bench-internals`.

### Fixed
- A stale mesh, GPU buffer or collision mesh handle could reach the
  resource made later in its slot; handles are never given out twice in a
  run.
- 3D games panicked at startup in browsers with only WebGL2, and 2D games
  failed to create sprite textures there.
- GPU skinning on devices with fewer than six storage buffers per stage
  falls back to CPU posing instead of failing validation.
- Instance batches and buffers past a device limit fall back or are left
  out with one warning instead of failing GPU validation; GPU memory of
  freed batches is returned.
- A `LodGroup` with a `Skeleton` draws its renderers posed.
- Temporal effects reset on MSAA changes, shader reloads, surface recovery
  and scene loads.
- Sky seams, shadow refreshes of freed shaders and textures, and several
  static-batch ordering and caching problems.
- `World::load` rejects saves with absurd entity ids instead of allocating
  for them.
- Lockstep hosts split oversized relayed inputs across packets; the UDP
  transport no longer loops on a socket that only errors.
- A screen reader no longer finds the game unresponsive; a screen root's
  `auto` margins center it.
- C API calls that panicked on invalid arguments (animators, curves,
  simulation speed, missing skeletons) return `INVALID_ARGUMENT`.
- Windows: output printed just before exit is no longer lost while the
  control endpoint captures it.
- `ponko_unity` binds standalone clips whose curve paths start above the
  root bone.

### Security
- The control endpoint's token is 128 bits from the OS's secure random
  source, compared in constant time; requests are capped at 4 MiB, 32
  connections, and 10 seconds to authenticate.
- Tools can't read or write files outside the game's folder through the
  endpoint, also not through links; a shipped release build doesn't open a
  port when a player sets `PONKO_CONTROL=1`.
- `ponko-mcp` marks destructive tools so clients can ask first.
- `ponko_unity` returns an error instead of panicking, looping or exhausting
  memory on crafted or corrupt files.

## [0.0.1] - 2026-11-08

### Changed
- Rename the Rust packages to Ponko and start a new 0.0.1 release line.
- Publish the Unity importer and benchmark packages alongside the engine.
- Retain existing C/C++ symbols and source-directory paths.


### Added
- Lens post effects: `Post::chromatic_aberration` (color fringes toward
  the edges), `lens_distortion` (barrel or pincushion) and `lens_scale`.
  In C and C++ as `CrabbyPost` fields.
- A playing `AudioSource3D` now follows changes to `settings.effects`
  (low-pass cutoff and reverb send), smoothed, instead of keeping the
  effects it started with.
- Named input actions (behind no feature): bind keyboard, mouse and gamepad
  inputs to names once and query those names in game systems.
  `Action::Button`, `Action::Axis` and `Action::Axis2` take several sources
  each (buttons OR together; axes pick the strongest source), so keyboard and
  controller share one set of names. `ctx.action_down`, `action_pressed`,
  `action_released`, `action_axis` and `action_axis2` read them;
  `ctx.actions()` and `ctx.set_actions` rebind at runtime;
  `ActionMap::to_json`/`from_json` save bindings as JSON; `feed_input`
  feeds input by hand for headless tests and replay. In C
  (`crabby_action_map_new` and friends, `crabby_ctx_action_down` and
  friends) and C++ (`crabby::action_map`, `action_binding`). New
  `action_map` example.
- Frame profiling: `ctx.frame_stats()` (fps, frame-time percentiles, CPU
  split into systems, render and display wait, GPU time), `ctx.gpu_passes()`
  (GPU time per pass: every shadow cascade, prepass, opaque, transparent,
  effects, bloom, post), `render_stats()` now counts models and triangles
  for the camera and the shadow maps separately, `ctx.mesh_costs()` per
  mesh, a performance HUD (`ctx.set_perf_hud`, `CRABBY_HUD=1`), Perfetto
  traces (`ctx.record_trace`, `CRABBY_TRACE`), an offscreen benchmark mode
  (`CRABBY_BENCH=<frames>`, with a JSON and CSV report), and the MCP tools
  `perf_report`, `record_trace` and `perf_hud`. The 3D renderer records its
  frame parts and culls in parallel; `Game::msaa(2)` is an option; on
  tile-based GPUs solid surfaces skip the depth prepass; a `ShadowMesh`
  component gives an entity a simpler shadow caster, a shadow-only shape,
  or none (the Unity importer maps Cast Shadows: Off and Shadows Only to
  it). In C (`crabby_game_msaa`, `crabby_ctx_frame_stats`,
  `crabby_ctx_set_perf_hud`, `crabby_ctx_record_trace`,
  `crabby_ctx_set_shadow_mesh`) and C++ (the same `ctx` methods,
  `game.msaa()`). The `3d` example's 100,000 cubes: 255 → 318 FPS.
- Vehicles (`physics3d` feature): `Vehicle3D` puts a dynamic body on up
  to 8 raycast `Wheel3D`s with spring-damper suspension, slip-based tyre
  grip through `FrictionCurve`s and wheel spin from motor and brake
  torque; the game sets each wheel's torques and steer angle per tick and
  reads back contact, compression, slip, rpm and the wheel's pose for
  drawing. `RigidBody3D::mass` and `::center_of_mass` override a body's
  mass and center of mass; `Physics3D::center_of_mass` reads it. In C
  (`CrabbyVehicle3D`, `crabby_wheel3d`, `crabby_vehicle3d_add_wheel`,
  `crabby_wheel3d_pose`, `crabby_friction_curve_*`,
  `crabby_ctx_body_center_of_mass3d`) and C++ (`crabby::vehicle`,
  `crabby::wheel`, `crabby::wheel_pose`). New `vehicle` example.
- Point and spot lights: `PointLight3D` and `SpotLight3D` components found
  by `Transform3D`, spots aiming along local −Z. Visible lights are binned
  into clusters per camera and a mesh fragment lights only its cluster's
  lights, with the packing WebGL2-friendly (no storage buffers). Sunlight
  and lights share one GGX lighting function; custom shaders call
  `punctual_light` and `direct_light`. 1,000 visible lights cost about
  0.14 ms of CPU and 1 ms of GPU at 720p (M4 Max, shadows off). In C
  (`CrabbyPointLight3D`, `CrabbySpotLight3D`) and C++
  (`crabby::point_light3d`, `spot_light3d`). New `lights` example.
- The Unity importer imports colliders: `Importer::colliders(true)` turns
  enabled box, sphere, capsule and mesh colliders into entities with a
  fixed `RigidBody3D` and a `Collider3D`. Scene files gain
  mesh-collider shapes, built once and shared by `ctx.load_scene`.
- Meshes that cast no shadow: `ctx.set_mesh_shadows(mesh, false)` leaves
  every model using that mesh out of the shadow maps (grass tufts,
  pebbles) while still drawn; `ctx.mesh_shadows(mesh)` reads it. In C
  (`crabby_ctx_set_mesh_shadows`, `crabby_ctx_mesh_shadows`) and C++
  (`ctx.set_mesh_shadows`, `ctx.mesh_shadows`).
- The Unity importer leaves out renderers set to Shadows Only (Unity never
  draws them either) and reports how many.
- 3D physics (`physics3d` feature): `RigidBody3D`, `Collider3D` and
  `Velocity3D` components — sphere, box, capsule, convex hull, triangle
  mesh and heightfield shapes, friction, restitution, sensors and
  collision groups, a fixed 50 Hz step, drawn transforms interpolated
  between steps; setting `Transform3D` teleports the body.
  `Physics3D::heightfield` makes a terrain collider in the exact layout of
  a terrain tile. Queries on `ctx.physics3d`: `cast_ray`, `cast_ray_all`,
  `cast_sphere`, `cast_capsule`, `overlap_sphere`, `overlap_box`,
  `overlap_capsule` (with a `QueryFilter3D`), plus `apply_impulse`.
  `Character3D` is a kinematic walking capsule: slope limit, step height,
  snap to ground, pushes light bodies; set `velocity`, read `grounded`.
  `ctx.set_cursor_locked` grabs the mouse for first-person look and
  `ctx.mouse_motion` reads raw motion. Collision events are shared with
  the 2D physics. In C (`crabby_ctx_cast_ray3d` and friends,
  `crabby_ctx_apply_impulse3d`) and C++ (`dynamic_body_at`, `box_collider`,
  `mesh_collider`, `character`, `filter3d`, `ctx.heightfield_collider()`,
  `game.physics3d()`). New `physics3d` example.
- Distance fog: `Sky::fog_start` and `::fog_end` blend the world into the
  fog from one distance to another, whatever the camera height, so an
  open world fades out before its streamed edges; `Sky::fog_sun_glow`
  (forward scattering of sunlight) makes the haze brighten toward the sun,
  height fog included. The sky itself takes none, so the sun disk stays
  visible. In C and C++ (the four new `Sky` fields), with a Book example
  in both.
- Custom material shaders get `origin` — the model's position — in both
  `vertex` and `surface`, so each instance can vary on its own (how high
  a pixel is above the model's base, a color per model, a sway phase)
  without a material per model.
- A second set of texture coordinates: `MeshData::uvs1` (glTF's
  `TEXCOORD_1`), read in custom shaders from `Vertex.uv1`/`Surface.uv1`;
  the Unity importer writes it for meshes drawn with a game's own
  shaders.
- Materials imported from Unity keep their names:
  `ctx.loaded_materials(name)` finds every material loaded from glTF
  files of that name with the `MaterialDesc` it was made from, so a game
  keeps its colors and textures and swaps in its own shader with
  `ctx.update_material`. `Importer::custom_shader(path)` marks imported
  materials for one of the game's shaders and writes `materials.json`
  with everything they set, textures as PNGs;
  `Importer::vertex_colors(shader, f)` fills a mesh's vertex colors from
  any Unity channel (a second UV set, say) for such shaders. The
  importer is a Rust tool; run it once and keep the files it writes.
- `ponko_unity` imports Unity scenes and prefabs into Crabby scene
  files: a scene's tree in `crabby-world` JSON, a `.glb` per drawn mesh
  with glTF PBR materials (URP Lit, Simple Lit and Unlit, with an ORM map
  packed from Unity's metallic/smoothness/occlusion), PNG textures shared
  across scenes, FBX and OBJ meshes via ufbx, built-in Cube/Plane/Quad.
  `Model` is reflected (scenes save it by its mesh file, and loading a
  scene loads the meshes first); glTF loading shares images between files.
  New `scene_viewer` example.
- `ponko_unity`, a workspace crate (not published yet): reads Unity
  projects as the importer's ground — text serialized files parsed into a
  lossless map, editor/Unity 6 binary files through their type trees, and
  GUIDs mapped to files from the `.meta`s.
- `Heightfield`: the ground's height and facing under a point on the CPU
  (`height_at`, `normal_at`), following the drawn triangles exactly, for
  standing things on terrain; `TerrainTileData::heightfield` keeps one. In
  C (`crabby_heightfield_new`, `crabby_heightfield_at`,
  `crabby_heightfield_free`) and C++ (`crabby::heightfield`).
- Streaming big worlds: `CellStreamer` says which cells of a ground grid
  to load (nearest first, a few per frame) and drop around a point;
  `Job::spawn` runs work on a worker thread and `job.take()` collects it;
  `TerrainTileData` builds a terrain tile on any thread and
  `ctx.upload_terrain_tile` puts it on the GPU; `ctx.free_mesh`,
  `ctx.free_terrain_tile` and `ctx.is_mesh_live` free GPU memory. In C
  (`crabby_cells_*`, `crabby_terrain_tile_data_new`,
  `crabby_ctx_upload_terrain_tile`, `crabby_ctx_free_mesh`,
  `crabby_ctx_free_terrain_tile`, `crabby_ctx_is_mesh_live`) and C++
  (`crabby::cell_streamer`, `crabby::terrain_tile_data`, and the same
  `ctx` methods; C++ uses `std::async` for jobs). New `streaming` example.
- Compute passes: `ctx.create_compute` and `ctx.load_compute` (hot
  reloaded) make a WGSL shader defining `fn compute(c: Cell)`, run once per
  texel of a render target each frame before drawing (ripples, trails in
  snow, wet ground), by adding a `ComputePass` to `ctx.compute_passes`,
  with a material's values and textures. In C
  (`crabby_ctx_create_compute`, `crabby_ctx_load_compute`,
  `crabby_ctx_set_compute_passes`, `crabby_ctx_compute_passes`) and C++
  (`ctx.create_compute`, `ctx.load_compute`, `ctx.set_compute_passes`,
  `ctx.compute_passes()`, `crabby::compute_pass_of`).
- Render targets: `ctx.create_render_target` makes a texture that a
  `TargetCamera` in `ctx.target_cameras` draws the lit 3D world into each
  frame, for materials to read (mirrors, reflections, monitors). An
  optional clip plane tilts the near plane onto a mirror;
  `TargetCamera::reflection` makes the camera for a level one. In C
  (`crabby_ctx_create_render_target`, `crabby_ctx_reflection_camera`,
  `crabby_ctx_set_target_cameras`, `crabby_ctx_target_cameras`) and C++
  (`ctx.create_render_target`, `ctx.reflection_camera`,
  `ctx.set_target_cameras`, `ctx.target_cameras()`,
  `crabby::target_camera_of`).
- 3D screen effects: `ctx.create_effect` and `ctx.load_effect` (hot
  reloaded) make a WGSL shader defining `fn effect(p: Pixel)`, run over the
  lit frame before bloom and tone mapping by adding a `ScreenEffect` to
  `ctx.effects`, with a material's values and textures. In C
  (`crabby_ctx_create_effect`, `crabby_ctx_load_effect`,
  `crabby_ctx_set_effects`, `crabby_ctx_effects`) and C++
  (`ctx.create_effect`, `ctx.load_effect`, `ctx.set_effects`,
  `ctx.effects()`, `crabby::screen_effect_of`).
- Terrain materials: `ctx.create_terrain_material` (`TerrainMaterialDesc`)
  blends up to 8 ground layers by a weight map, with textures lined up
  with the world and optional normal maps. In C
  (`crabby_ctx_create_terrain_material`, `crabby_terrain_material_desc`)
  and C++ (`ctx.create_terrain_material`, `crabby::terrain_material_of`).
- Texture arrays for 3D materials: `ctx.create_texture_array` and
  `ctx.load_texture_array` (`TextureArray`, `TextureKind::Color` or
  `Data`), with mipmaps, read by custom shaders from
  `MaterialDesc::custom_arrays` as `custom_array0` to `custom_array2`. In
  C (`crabby_ctx_create_texture_array`, `crabby_ctx_load_texture_array`)
  and C++ (`ctx.create_texture_array`, `ctx.load_texture_array`).
- Heightfield terrain: `ctx.create_terrain_tile` makes a `TerrainTile`
  from a square grid of heights (NaN cuts a hole), drawn at four levels
  of detail by distance from the camera, with skirts that hide the cracks
  between levels. `MeshData::heightfield` gives the mesh for one level.
  In C (`crabby_ctx_create_terrain_tile`, `crabby_terrain_tile_component`,
  `crabby_mesh_data_heightfield`) and C++ (`ctx.create_terrain_tile`,
  `crabby::terrain_tile`, `mesh_data::heightfield`).
- 3D post-processing, `ctx.post` (`Post`): exposure, bloom, white balance
  (temperature, tint), contrast, saturation, tone mapping (`Tonemap::Aces`,
  `AgX`, `Neutral`, `None`), a color lookup table (`Post::neutral_lut` to
  start one) and vignette. In C (`crabby_ctx_post`, `crabby_ctx_set_post`)
  and C++ (`ctx.post()`, `ctx.set_post`, `crabby::tonemap`).
- Custom 3D material shaders in WGSL: `ctx.load_shader` (hot reloaded)
  and `ctx.create_shader`. A shader defines `vertex` (move vertices)
  and/or `surface` (change each pixel); the engine lights and shadows it.
  `MaterialDesc` gains `shader`, `custom` values, `custom_textures`,
  `normal_map` (with `normal_strength`), `orm_map`, `emissive_map`,
  `blend` (`Blend::Transparent`) and `double_sided`;
  `ctx.update_material` changes a material. In C and C++ too. The
  `shaders` example shows water, wind and a procedural pattern.
- A physical sky and height fog, `ctx.sky` (`Sky`): Rayleigh and Mie
  scattering lit by the sun, a sun disk, stars at night; the sun dims
  and reddens as it sets and the scene's ambient light comes from the
  sky. `Sky::sun_direction_at(hour)` for a day and night cycle. In C
  (`crabby_ctx_sky`, `crabby_ctx_set_sky`, `crabby_sun_direction_at`) and
  C++ (`ctx.sky()`, `ctx.set_sky`, `crabby::sun_direction_at`).
- `Lighting::ambient_intensity` now defaults to 1 (light as bright as the
  sky gives), with the default sky and ground colors halved to match.
- Cascaded sun shadows: `Lighting::shadow_cascades` (1 to 4, default 4)
  shadow maps share `shadow_distance`, sharp near the camera and reaching
  far, blended where they meet. In C and C++ (`shadow_cascades` in the
  lighting struct).
- 3D textures get mipmaps when a material first uses them (made on the
  GPU; sprites still draw the full-size image).
- glTF models load their normal, occlusion, metallic-roughness and
  emissive maps (occlusion packed into the ORM map), vertex colors, alpha
  blending and double-sidedness.
- Vertex colors: `MeshData::colors`, multiplying the material's color and
  readable in shaders. In C (`crabby_mesh_data_set_colors`) and C++
  (`mesh_data::set_colors`).
- Transparent shaders can read the scene behind them: `scene_color`,
  `scene_position` and `scene_distance`, for refraction and water depth.
- `Vec4` and `vec4` are re-exported (feature `3d`).
- The 3D frame is lit in HDR, with a depth prepass (each pixel is lit
  once). `render_stats().gpu_ms` now times the whole frame, 3D and shadow
  passes included.
- Polygon colliders: `Collider::polygon` (3 to 8 corners, convex), in C
  (`crabby_set_polygon_collider`, `crabby_collider_points`) and C++
  (`crabby::polygon_collider`).
- Scenes and prefabs: `ctx.load_scene`/`save_scene` keep a whole world in a
  file under `assets/`; `ctx.spawn_prefab`/`save_prefab` keep one entity's
  components and spawn copies where you say. In C (`crabby_ctx_load_scene`
  and friends) and C++ (`ctx.load_scene` and friends).
- `Sprite` is reflected in every game, its texture written as the image's
  name in `assets/` (`"texture": "crab.png"`); setting or loading one loads
  the image. `Color` and `Rect` implement serde's traits.
- The inspector is an editor while the game is paused: click to select,
  drag to move, pan and zoom, Delete, Ctrl+D duplicate, Ctrl+Z undo, add
  components, place prefabs from `assets/prefabs/`, save and load scenes,
  and Play / Stop (Stop puts the world back as it was at Play).
- 3D, behind the `3d` feature: `Transform3D` and `Model` components on
  the same ECS, meshes (`MeshData::cube`/`plane`/`sphere`, your own
  triangles, or glTF with `ctx.load_mesh`), physically based materials
  (`ctx.create_material`), `ctx.camera3d` with `screen_ray` picking, and
  `ctx.lighting`: a sun with soft shadows that follow the camera, plus sky
  light. Instanced by mesh, culled to the view, 4x multisampled, drawn
  under the 2D layer. 100,000 spinning cubes hold 120 fps. In C (`crabby_ctx_create_mesh`
  and friends; `CrabbyTransform3D` and `CrabbyModel` are plain query
  columns) and C++ (`crabby::transform3d_at`, `crabby::mesh_data`,
  `ctx.create_material`, ...). The `3d` example and the "3D" book chapter.
- The MCP `launch` tool takes `features`, for games behind Cargo features.
- `Game::fullscreen(true)` fills the screen (C: `crabby_game_fullscreen`,
  C++: `game.fullscreen()`).
- The `3d` example's benchmark mode (`BENCH_SECONDS`), and the same scene
  in Unity 6 under `bench/unity`: 100k spinning cubes run at 402 FPS in
  Crabby and 54 in Unity.

### Changed
- Terrain materials lay their layers on from the sides too on steep
  ground (triplanar), so cliffs no longer stretch them; flat ground costs
  the same.
- 3D materials: `Blend::Opaque` is now fully solid, whatever the alpha;
  the new `Blend::Cutout` (2 in C, `crabby::blend::cutout` in C++) cuts
  out pixels with alpha below 0.5, as `Opaque` did before. glTF materials
  with alpha mode MASK load as `Cutout`. Solid materials write depth and
  shadows without running their shader.
- Shadows: each pixel samples only the cascade it falls in (and the next
  one near the edge), not every cascade, and none when it faces away from
  the sun. Distant cascades draw terrain tiles at a coarser detail level.

### Fixed
- The Unity importer honors `bakeAxisConversion` on FBX meshes: positions
  and normals go through the same axis permutation and the winding turns
  where it mirrors, so such meshes no longer stand the wrong way round in
  imported prefabs.
- A custom `surface` shader that reads `sunlight()` or `sky_radiance()`
  could fail pipeline validation when only the shadow or sky pass drew it;
  those passes now bind everything the frame group declares.
- A 3D body that falls asleep reports a zero `Velocity3D` instead of the
  speed it had just before.
- `vsync(false)` no longer waits for the window's redraw events, which
  macOS sends only once per display refresh.
- Control endpoint `scene.save`, `scene.load`, `prefab.spawn`,
  `prefab.save`, and the MCP tools `save_scene`, `load_scene`,
  `spawn_prefab`, `save_prefab`, so agents can build levels.

## Before the rename: 1.0.0 - 2026-11-03

Released under the engine's former name. Those crates were withdrawn from
crates.io and the tag removed; 0.0.1 restarted the line as Ponko. Kept as the
record of what the first version contained.

The first release: a 2D engine for simulations with up to millions of
entities, in Rust, C and C++, with the C library and headers for Linux,
macOS and Windows attached to the GitHub release. Everything below is new.

### Simulation
- Own archetype ECS (`ponko_ecs`): entities with generational ids, queries,
  `par_for_each` over archetype chunks on all cores, commands, resources,
  and components registered at runtime (for C and C++).
- Fixed-rate simulation clock: pause, step one tick, 1–64× speed, systems
  that run every N ticks (`sim_system_every`, `ctx.turns`).
- Spatial grids with radius, rectangle and nearest queries; spatial sorting
  so neighbors on the map are neighbors in memory.
- Flow-field pathfinding (`NavGrid`, `FlowField`).
- Events between systems; attachments (things that follow other things).
- 2D physics with rapier (`physics` feature): `RigidBody`, `Collider` and
  `Velocity` components synced with `Transform2D`, `CollisionStarted` /
  `CollisionStopped` events, ray casts, overlap queries and impulses
  (`ctx.physics`), and `physics-determinism` for bit-identical results across
  platforms. In C (`crabby_game_physics`, `crabby_ctx_cast_ray`, ...) and C++.
- Deterministic lockstep multiplayer for 2–8 players (`Game::lockstep`,
  `Net::host` / `Net::join`, `ctx.net`): plain UDP, input delay, lost
  packets recovered (a peer that stops keeps resending for up to a second
  until the others have its last inputs), and desync checks that name the
  components that differ. `Net::loopback` for tests. In C and C++ too.
- Lockstep over Steam (feature `steam`): `Net::steam_host` / `steam_join`,
  friends-only lobbies, overlay invites, Steam's relays (no port
  forwarding). The Steam library is copied next to examples and tests. In C
  and C++ with ponko_ffi's `steam` feature.
- Determinism: seeded randomness per (seed, tick, system, entity), the same
  with parallel queries. Save and load the whole world, keeping entity ids.
- Headless runs (`Game::run_headless`) and an optional `window` feature for
  servers and CI.

### Drawing, sound and input
- wgpu renderer: an instanced sprite batcher, parallel instance building,
  GPU timings (`ctx.render_stats()`).
- `Camera2D` (pan, zoom, follow), text, shapes, simple UI.
- Sprite sheets, animation and tilemaps.
- Sound through kira (`audio` feature).
- Gamepads through gilrs (`gamepad` feature, on by default): buttons named
  by position, sticks and triggers, up to 8 pads in fixed slots, plugged in
  and out while the game runs; in C and C++ too.

### Fast iteration
- Hot reload of images, sounds, fonts and JSON data files.
- Hot-patching of game code with `dx serve --hot-patch` (`hotpatch` feature).
- The runtime inspector (`inspector` feature, the `` ` `` key).
- Reflection: components and resources as JSON.
- Faster debug builds (dev opt-level 1).

### Tools and agents
- Control endpoint (`CRABBY_CONTROL=1`): localhost, token-protected.
- `ponko_mcp`: an MCP server for AI agents (launch, inspect, edit, step,
  events, logs, stats, screenshots, save and load).

### Web
- Web builds (wasm32): the same game runs in a browser, drawing with WebGPU
  or WebGL2. Assets listed in `assets/index.json` are fetched before setup,
  sound plays through WebAudio, and the inspector opens with `?inspector=open`.
- `web/build.sh` builds an example or a game's binary into a static site.
- `ponko::Instant`: an `Instant` that also works in browsers.
- The docs site publishes playable builds of the battle, flow and ants demos.

### API and stability
- [STABILITY.md](STABILITY.md): semver from 1.0, deprecation policy, MSRV
  (Rust 1.95, checked in CI), C ABI versioning, save compatibility and what
  is deterministic where.
- Every public item is documented (`missing_docs` is enforced), with
  `# Panics` / `# Errors` sections where they apply.
- `#[must_use]` on getters, constructors and builders; `Game` and `Query`
  warn when built and never run.
- `#[non_exhaustive]` on `NetState`, `BodyKind`, `Shape`, `FieldType` and
  `ReflectError`, so new variants aren't breaking changes.

### C and C++
- C++: components are found by their type, with nothing to register:
  `world.spawn(transform_at(p), Health{100})`, `world.get<Health>(e)`,
  `world.each([](transform2d& t, const Vel& v) { ... })` (as fast as a
  hand-written loop), `each_par` on all cores, `ctx.send(Died{e})` and
  `ctx.read<Died>()`. `world.reflect<T>(fields)` describes a type for tools
  and saves. Replaces component tokens (`world.component<T>("Name")`,
  `token.with(value)`).
- Resources from C and C++: `world.set_resource(Score{...})`,
  `world.resource<Score>()`, `try_resource`, `remove_resource`, and
  `reflect_resource<T>(fields)` so agents and saves see them (C:
  `crabby_resource_*`, by name). The C++ battle keeps its score in one.
  `ctx.load_json<T>(path)` loads a data file into a resource, live.
- Fixed: `ctx.load_json<T>(path, e)` failed for C++ types in a namespace.
- C API (`ponko_ffi`) with a generated header, and a header-only C++17
  wrapper (`include/crabby.hpp`) covering nearly the whole engine.

[Unreleased]: https://github.com/ponkoengine/ponko/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/ponkoengine/ponko/compare/v0.0.1...v0.1.0
[0.0.1]: https://github.com/ponkoengine/ponko/releases/tag/v0.0.1
