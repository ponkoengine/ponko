# Changelog

All notable changes to Ponko (formerly Crabby Engine) are listed here. The
format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/) as Cargo applies
it to 0.x; [STABILITY.md](STABILITY.md) says what that covers.

## [Unreleased]

Ponko 0.2 grows the engine from "a fast 2D and 3D core" into one you can
ship a whole game with. It adds the tools around the code (an editor, an
asset cooker, packaging, crash reports), the systems most games need
(hierarchies, game states, a widget set, localization, saves that migrate),
and multiplayer from rollback to dedicated servers, while the simple path
stays `Game::new()`, systems and `.run()`. Where it breaks the API, the
migration guide, "Migrating from 0.1 to 0.2", lists each change with the
code before and after.

**Highlights**

- **Building scenes**: the [editor](https://ponko.dev/guide/editor.html) (`ponko editor`) edits 2D and
  3D scenes and prefabs, with undo, an asset panel and glTF scenes;
  `ponko cook` turns source assets into runtime files; levels load from
  Tiled and LDtk ([tile maps](https://ponko.dev/guide/tiles.html)); prefabs hold children, and
  [saves](https://ponko.dev/guide/storage.html) migrate between versions of a game and can be binary.
- **ECS and game structure**: an [entity hierarchy](https://ponko.dev/guide/hierarchy.html),
  [game states](https://ponko.dev/guide/states.html), component hooks and required components,
  [plugins](https://ponko.dev/guide/plugins.html) for other people's crates, and `Commands::spawn`
  returning the entity at once.
- **Rendering**: 2D lights and particles, sprite flips and blend modes,
  [several cameras in one window](https://ponko.dev/guide/viewports.html), camera rigs, a sky kit with
  clouds, a [water kit](https://ponko.dev/guide/water.html), baked and probe lighting, lighting from
  HDR panoramas, material layers (clear coat, sheen, transmission and more,
  read from glTF), temporal upscaling, auto exposure,
  [HDR output](https://ponko.dev/guide/hdr.html), reverse-Z depth and GPU timing and memory in the
  performance report.
- **Animation and crowds**: [2D skeletons](https://ponko.dev/guide/skeleton2d.html), humanoid
  retargeting, and [crowds](https://ponko.dev/guide/crowds.html) of thousands of animated characters
  in a few draws.
- **UI and input**: a [widget set](https://ponko.dev/guide/ui-widgets.html) and
  [UI animation](https://ponko.dev/guide/ui-animation.html), text shaping, IME, rich text,
  [localization](https://ponko.dev/guide/localization.html), screen readers on Windows and Linux,
  [touch controls](https://ponko.dev/guide/touch.html), [controllers](https://ponko.dev/guide/controllers.html) with gyro,
  rumble and joysticks, and a window service for display modes.
- **Audio**: a mixer of nested buses with effects and snapshots, loop
  points and a voice limit ([audio](https://ponko.dev/guide/audio.html)).
- **Game AI**: behaviour trees and brains in the [AI kit](https://ponko.dev/guide/ai.html), and 3D
  navigation obstacles of any shape.
- **Multiplayer**: [rollback](https://ponko.dev/guide/multiplayer.html), declared
  [replication](https://ponko.dev/guide/replication.html), client prediction,
  [dedicated servers](https://ponko.dev/guide/servers.html), a reference [lobby server](https://ponko.dev/guide/lobby.html)
  with relay, [WebRTC](https://ponko.dev/guide/webrtc.html), Steam Networking Sockets and
  [Steam services](https://ponko.dev/guide/steam.html).
- **Platforms**: [iOS](https://ponko.dev/guide/ios.html), [Android](https://ponko.dev/guide/android.html),
  [threads on the web](https://ponko.dev/guide/web-threads.html),
  WebGPU first in the browser, [video](https://ponko.dev/guide/video.html), C and C++ games built
  for the web, and C library downloads for Intel Macs, Arm Linux and Arm
  Windows.
- **Shipping and support**: `ponko new` and `ponko package`
  ([packaging](https://ponko.dev/guide/packaging.html)), [crash reports](https://ponko.dev/guide/crash-reports.html) with
  native backtraces, [mods](https://ponko.dev/guide/mods.html), [debug drawing](https://ponko.dev/guide/debug-draw.html), a
  developer console, error messages that name the fix
  ([errors](https://ponko.dev/guide/errors.html)), and loaders and network hosts hardened against
  hostile files and packets ([security](https://ponko.dev/guide/security.html)).
- **Learning it**: two tutorials ([2D](https://ponko.dev/guide/tutorial-2d.html), [3D](https://ponko.dev/guide/tutorial-3d.html)),
  a [cookbook](https://ponko.dev/guide/cookbook/index.html) of fifty recipes,
  [Coming from Bevy](https://ponko.dev/guide/from-bevy.html), [maturity levels](https://ponko.dev/guide/maturity.html) on every
  chapter, and three reference games: [Ember Hop](https://ponko.dev/guide/ember-hop.html),
  [Shard Grove](https://ponko.dev/guide/shard-grove.html) and [Legions](https://ponko.dev/guide/legions.html).

### Breaking changes

Each entry has its code, before and after, in the [migration guide](https://ponko.dev/guide/migrating-to-0.2.html).

**API and crate layout**
- `ponko_ffi::PonkoHandleKind` has three new values,
  `PONKO_HANDLE_SPRITE_SHADER`, `PONKO_HANDLE_SPRITE_MATERIAL` and
  `PONKO_HANDLE_RENDER_TARGET`, so a Rust `match` over every value no longer
  compiles. C and C++ code is not affected.
  Instead: add arms for the new values, or a `_` arm.
- The `ponko::physics` module is gone: 2D physics lives in `ponko::d2`.
  `physics::{RigidBody, Collider, Velocity, Shape, Physics, QueryFilter}`
  were also public as `d2::*`; `physics::RayHit` is `d2::Hit`,
  `physics::MAX_POLYGON_POINTS` is `d2::Shape::MAX_POLYGON_POINTS`, and the
  step systems are in `ponko::systems`. The types are the same; saved
  component names (`RigidBody`, `Collider`, ...) don't change.
  Instead: import from `ponko::d2` (or `ponko::systems` for the systems).
- `ponko::d3` no longer re-exports the types of topic modules, so each type
  has one path. Most of 0.1's `d3` names stay as deprecated aliases until
  0.3.0 (see "Deprecated", and the removal list there); what breaks now is
  `d3::Model` and `d3::{BodyKind, CollisionStarted, CollisionStopped}`
  (the prelude has them), a tuple-struct constructor or pattern through an
  alias (`ShadowMesh(None)`, `LayerId(0)`), and a module that glob-imports
  `d3` together with `render`, `anim`, `particles` or `terrain`, where those
  names are now ambiguous (E0659). The render types (`AmbientOcclusion`,
  `AntiAliasing`, `BatchCulling`, `Blend`, `Compute`, `ComputePass`, `Effect`,
  `GlobalIllumination`, `GpuBuffer`, `GraphicsQuality`, `InstanceBatch`,
  `InstanceBatchOptions`, `InstanceLevel`, `Lighting`, `LodGroup`,
  `LodHysteresis`, `LodLevel`, `LodPick`, `LodRenderer`, `LodSettings`,
  `LodShadow`, `Material`, `MaterialDesc`, `Mesh`, `MeshCost`, `MeshData`,
  `Model`, `ModelMaterial`, `ModelMaterials`, `Post`, `ReceivesDecals`,
  `RenderPasses`, `SceneSettings`, `ScreenEffect`, `Shader`,
  `ShaderDiagnostic`, `ShaderKind`, `ShaderOutcome`, `ShaderStatus`,
  `ShadowFilter`, `ShadowMesh`, `Sky`, `StreamedLod`, `TargetCamera`,
  `TargetEffect` and `Tonemap`) are in `ponko::render`, the animation types
  (`AnimationClip`, `AnimationCondition`, `AnimationEvent`,
  `AnimationGraph`, `AnimationGraphError`, `AnimationLayer`,
  `AnimationParameter`, `AnimationState`, `AnimationTrack`,
  `AnimationTransition`, `Animator`, `AttachToJoint`, `Interpolation`,
  `JointAnimation`, `JointMask`, `LayerBlend`, `LayerId`, `MorphAnimation`,
  `MorphFrame`, `MorphTarget`, `MorphWeights`, `Motion`, `MotionChild`,
  `ParamId`, `RootMotion`, `Skeleton`, `SkinOf` and `StateId`) in
  `ponko::anim`, terrain types (`Heightfield`, `TerrainTile`,
  `TerrainTileData`, `TerrainMaterialDesc`, `TextureArray`, `TextureKind`,
  `FarTerrain`) in `ponko::terrain` (its `CellStreamer` is a deprecated
  alias of `ponko::streaming::CellStreamer`), `ParticleCurve`, `ParticleGradient`
  and `ParticleSpace` in `ponko::particles`, and `BodyKind`, `CollisionStarted`
  and `CollisionStopped` in `ponko::collision`. `d3` keeps the types named
  after the dimension (`Transform`, `Camera`, lights, decals, text, trails,
  particle emitters, positional audio, navigation agents, `UiPanel`) and the
  3D physics. `Model` stays in the prelude.
  Instead: import these types from their module next to `use ponko::d3::*;`
  (by name, not with a second glob, while the aliases last), and build a
  tuple struct through its new path.
- `ponko::d2` no longer re-exports the types of topic modules: tile maps and
  levels (`TileMap`, `Tileset`, `Level`, `LevelOptions`, `LevelObject`,
  `AutoTile`, ...) are in `ponko::tilemap`, `Animation` and `SpriteSheet` in
  `ponko::sheet` (`SpriteSheet` stays in the prelude), `Attached` in
  `ponko::attach`, `SpatialGrid` in `ponko::spatial`, `NavGrid` and
  `FlowField` in `ponko::flow`, `BlendMode`, `SpriteMaterial`,
  `SpriteMaterialDesc` and `SpriteShader` in `ponko::render`, the particle
  curves in `ponko::particles` and the collision types in
  `ponko::collision`. `d2` keeps `Transform`, `Camera`, `Sprite`, the
  particle emitter types and the 2D physics.
  Instead: import these types from their module.
- Types of one dimension are no longer also public in a topic module under
  their suffixed name: `camera::Camera2D` is `d2::Camera`,
  `particles::{ParticleEmitter2D, ParticleSettings2D, ParticleShape2D}` are
  `d2::{ParticleEmitter, ParticleSettings, ParticleShape}`,
  `audio::{AudioSource3D, AudioListener3D, AudioSettings3D}` are
  `d3::{AudioSource, AudioListener, AudioSettings}` and `ui::UiPanel3D` is
  `d3::UiPanel`. `camera` keeps `Rect` and `camera_controls`, `particles`
  the curves shared by both dimensions, `audio` keeps `AudioRolloff`.
  Saved component names don't change.
  Instead: use the `d2`/`d3` names (the prelude's `d2` and `d3` modules
  reach them).
- `ponko::nav` holds only the navigation-mesh baking types (`NavBakeSettings`,
  `NavBakeError`, `NavSource`): its copies of `d3::{NavMesh, NavAgent,
  NavObstacle}`, of `flow::{NavGrid, FlowField}` and of
  `systems::step_navmesh3d` are gone. `ponko::anim` no longer re-exports
  `sheet::{Animation, SpriteSheet}` and `systems::animate_sprites`.
  Instead: use the paths named here.
- `ponko::alloc_count` and `ponko::scope` are private: their items were
  also in `ponko::perf`, which is now their one path (`CountingAllocator`,
  `AllocCounts`, `alloc_counts`, `note_allocation`, `ProfileScope`,
  `ScopeGuard`, `ScopeTiming`, `scope_named`).
  Instead: import them from `ponko::perf`.
- The built-in systems are public only in `ponko::systems`:
  `spatial::rebuild_spatial_grid`, `sheet::animate_sprites`,
  `tilemap::draw_tilemaps`, `attach::follow_attachments` and
  `net::apply_net_poses` are gone from their topic modules (the names the
  built-ins are listed under, such as `ponko::tilemap::draw_tilemaps` in
  plans and for `ponko_game_remove_system`, don't change).
  Instead: use `ponko::systems::<name>`.
- The Steam API takes and returns Ponko's own types: `ponko::net::SteamClient`,
  `SteamLobbyId` and `SteamUserId` replace `steamworks`' `Client`, `LobbyId`
  and `SteamId` in `Net::steam_host`, `steam_join`, `steam_lobby`,
  `NetMessages::steam_host`, `steam_join`, `steam_peer`, `SteamLobby::create`,
  `join`, `id`, `members`, `steam_lobby_from_args` and
  `steam_wait_for_invite`. A new `steamworks` release is no longer a breaking
  Ponko release. Feature `steam`; the C API is unchanged.
  Instead: start Steam with `SteamClient::init_app` (or `SteamClient::init`)
  and pass it where the `steamworks::Client` went. Lobby and user ids convert
  with `from_raw` and `raw`, print and parse as their 64-bit number.
- `ponko::net::steam`, the re-exported `steamworks` crate, is `ponko::net::unstable_steamworks` now.
  Like `ui::unstable_taffy`, it is outside the versions: its `steamworks`
  version may change in any release.
  Instead: use Ponko's Steam types; for the parts of Steam Ponko doesn't wrap
  (achievements, stats, rich presence), take the client underneath with
  `SteamClient::unstable_steamworks`, and pass ids across as `u64`.
- The prelude no longer exports `AssetSource`, `MemorySource`, `PackSource` and `SetupResult`.
  They are what a game names only when it adds an asset source or writes a
  bound on fallible setup, so they stay in their modules. A game's own type
  named `UiEvent`, `UiLayout`, `Gamepad`, `GamepadAxis`, `GamepadButton` or
  `GamepadStick` that it glob-imports next to the prelude is now ambiguous:
  import it by name, or use the engine's.
  Instead: import them from `ponko::assets` (`ponko::game::SetupResult`);
  `source.read(name)` needs `AssetSource` in scope.
- `ponko_image_diff::compare`, `heatmap` and `compare_files` return `ponko_image_diff::Error`.
  The error keeps the failure's kind, the file and the underlying cause.
  Instead: match `err.kind()` (with a `_` arm), or turn it into a string with `String::from(err)`.
- Render targets have their own handle type, `RenderTarget`, instead of `Texture`.
  `create_render_target`, `try_create_render_target` and a UI panel's `target` (it was `texture`) give a `RenderTarget`; `TargetCamera::new`, `TargetCamera::reflection`, `ComputePass::new`, `TargetEffect::new` and `d3::UiPanel::new` take only that, and their `target` fields are one. Scenes and saves are unaffected (they never stored render targets, and store other handles by name).
  Instead: the migration is mechanical. (1) Keep what `create_render_target` returns as a `RenderTarget` (`use ponko::render::RenderTarget;`), and change helper functions, struct fields and collections that hold a render target from `Texture` to `RenderTarget`. (2) Where a target is read as a texture (`MaterialDesc::with_custom_textures`, `with_texture`, `with_emissive_map`, `with_normal_map`, `Sprite::new`, `ctx.gfx.draw`, `update_texture`, `is_render_target`), write `target.texture()`. (3) `panel.texture` becomes `panel.target.texture()`. (4) Comparing a pass's `target` with a `Texture`: compare `target.texture()`, or keep the `RenderTarget` and compare that. (5) Code reading `c.target.id` reads `c.target.texture().id`. The compiler names every place: each is a type mismatch between `Texture` and `RenderTarget`.
- C++: render targets are `ponko::render_target`, and the calls that draw into one take only that.
  `ctx.create_render_target` and `ctx.create_ui_panel` return a `ponko::render_target`, which converts to `ponko::texture` wherever a texture is read. `target_camera_of`, `compute_pass_of`, `target_effect_of` and `ctx.reflection_camera` no longer accept a `ponko::texture`.
  Instead: hold render targets as `ponko::render_target` (or `auto`) and take it in helpers; for a target read back from the engine (a `PonkoTargetCamera`'s, `PonkoComputePass`'s or `PonkoTargetEffect`'s `target`, which stay plain `PonkoTexture`s in C), call `ctx.render_target_of(t.target)`, which checks it and throws `ponko::error` if it isn't one. C code is unchanged, except that `ponko_ctx_reflection_camera` now refuses a texture that isn't a render target.

**ECS and systems**
- An index into `RawWorldRef::archetypes` is valid only until the next change.
  It names an archetype only until the next structural change: a world now
  reclaims archetypes that stayed empty and builds new component sets in
  their slots; a reclaimed slot shows as an empty archetype without
  components. Query order, which follows the archetype slots, can differ from
  before (it is still the same for the same operations).
  Instead: don't keep an index across spawns, despawns, inserts or removes;
  find the archetype again by its components (`Archetype::has`,
  `Archetype::component_ids`).
- `schedule::Stage::systems` returns `&[usize]` instead of a `Range<usize>`.
  A batch's systems need not be neighbours any more (see Changed), so a
  stage lists them in the new `Stage::members` field, in registration
  order; `first` and `len` stay, as its first system and how many it has.
  Instead: iterate the slice (`.iter().copied()`) or read `stage.members`;
  `stage.systems().contains(&i)` works as before.
- `d2::Collider` has a `groups` field (its collision groups).
  Colliders made with `Collider::circle`, `rect`, `capsule` or `polygon`
  are unchanged and start in every group; saved colliders without the field
  load in every group.
  Instead: build colliders with their constructors, or add
  `groups: CollisionGroups::ALL` to a struct literal.
- `d2::QueryFilter` has a `groups` field, as `d3::QueryFilter` does.
  Instead: build filters with `QueryFilter::default()` and its builders
  (`.exclude(e)`, `.groups(g)`, `.with_sensors()`), or add
  `groups: CollisionGroups::ALL` to a struct literal.
- `Commands::spawn` returns the new `Entity` instead of `()`.
  A closure or function whose last expression was `commands.spawn(...)`
  now returns an `Entity`, so a system written that way no longer fits
  where `()` is expected.
  Instead: end the call with a semicolon, or use the returned entity.
- `Commands::spawn` on a buffer that isn't tied to a world panics.
  The id is reserved in the buffer's world when `spawn` returns, so a
  buffer from `Commands::new()` must know its world first. `ctx.commands`
  and a view system's `sys.commands` already do; applying a buffer once
  ties it to that world.
  Instead: make your own buffers with `world.commands()`, or call
  `commands.bind(&world)` before the first spawn.
- `systems::follow_attachments` places only the subtrees whose root moved or whose attachments changed, so a transform written straight into an attached entity lasts.
  A still subtree costs nothing per tick. A value written into an attached
  entity's `Transform` used to be reset every tick and now stays until its
  subtree is placed again. Chains have no depth limit any more (they were
  broken past 64 links), and an attachment whose parent has no transform is
  left where it is instead of being despawned.
  Instead: to move an attached entity, change its `Attached`/`Attached3D`
  offset (or move its parent), as before.
- Adding a plugin with the same name twice makes the game fail to start.
  A plugin's name is its type's path, so two instances of one plugin type
  used to run both; now the second is an error (`ErrorKind::Invalid`).
  Instead: give each instance its own name with `Plugin::name`, or add the
  plugin once.

**Rendering**
- `PerfReport` has a new field, `gpu_memory`, and `Budget` a new field,
  `gpu_memory_mb`.
  Code that builds either with a struct literal naming every field no
  longer compiles; reading them, `Default` and the builders are unchanged.
  Instead: end the literal with `..Default::default()`, or build a budget
  with `Budget::new()` and its methods.
- `Sprite` has two new public fields, `flip_x` and `flip_y`.
  Code that builds a `Sprite` with a struct literal naming every field no
  longer compiles.
  Instead: start from `Sprite::new` and the `with_*` builders, or add the two
  fields (`false` keeps the old look).
- The 3D scene depth is reverse-Z: 1 at the camera's near plane, 0 at the far plane and the sky.
  Shaders that read `scene_depth_texture` directly (a material, screen effect or compute pass)
  see the depth flipped; positions and distances from `scene_position` and `scene_distance` are
  unchanged. Shadow maps keep standard depth.
  Instead: use `scene_position` or `scene_distance`, or treat a greater depth as nearer and 0 as
  the sky (standard depth `d` is `1.0 - d` now for the same near and far planes).
- `TargetCamera` is `#[non_exhaustive]` and no longer `Copy`.
  It holds a list of screen effects now, and new fields can be added
  without breaking games again.
  Instead: build one with `TargetCamera::new` or `TargetCamera::reflection`
  and its `with_` methods, and borrow or `clone()` one where it was copied.
- Material normal maps no longer lean surfaces the opposite way on both axes.
  A game that made its normal maps look right by flipping them (a negative
  `normal_strength`, or images with red and green inverted) now sees them
  inverted.
  Instead: undo the workaround: a positive `normal_strength`, and the
  images as exported (glTF and OpenGL style, green up).
- `ModelLight` has a new field, `joint`.
  A light from a file with skins or animations names the joint its node is.
  Instead: set `joint: None` where you build a `ModelLight` yourself.

**Animation**
- `ponko::anim::AnimationState` is `#[non_exhaustive]`: struct literals and
  `..AnimationState::new(..)` updates outside the engine no longer compile.
  Its fields stay public to read and change.
  Instead: start from `AnimationState::new(motion)` and chain `with_speed`,
  `with_speed_parameter`, `with_looping`, `with_event` or `with_events`.
- `ponko::anim::MotionChild` is `#[non_exhaustive]`: struct literals outside
  the engine no longer compile.
  Instead: use `MotionChild::new(motion)` (time scale 1) and
  `with_time_scale` for another clock speed.
- `ponko::anim::AnimationEvent` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `AnimationEvent::new(time, name)`; the name takes any
  `Into<Arc<str>>` (`&str`, `String`, `Arc<str>`).
- `ponko::anim::Joint` is `#[non_exhaustive]`: struct literals outside the
  engine no longer compile.
  Instead: use `Joint::new(name, parent)` (`parent` is an index, or `None`
  for a root) and `with_rest` for its rest pose.
- `ponko::anim::JointAnimation` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `JointAnimation::new(joint, name)` and `with_translation`,
  `with_rotation` or `with_scale` for the tracks it has.
- `ponko::anim::MorphAnimation` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile.
  Instead: use `MorphAnimation::new(name, track)`.
- `ponko::anim::MorphTarget` is `#[non_exhaustive]`: struct literals and
  `..Default::default()` updates outside the engine no longer compile.
  Instead: use `MorphTarget::new(name, positions)` and `with_normals` when
  the normals change too.
- `ponko::anim::AttachToJoint` is `#[non_exhaustive]`: struct literals
  outside the engine no longer compile. Saved scenes and prefabs load as
  before.
  Instead: use `AttachToJoint::new(skeleton, joint)` and `with_offset`.
- The `Motion::Blend1D` and `Motion::Blend2D` variants are
  `#[non_exhaustive]`: building them with variant literals outside the
  engine no longer compiles, and patterns need `..`.
  Instead: use `Motion::blend_1d(parameter, children)` and
  `Motion::blend_2d(x, y, children)`; match with `Motion::Blend1D { children, .. }`.

**Assets, scenes and saves**
- `TileMap` is `#[non_exhaustive]` and has new fields (`parallax`,
  `parallax_origin`, `animations`, `flips`), so it can't be built with a
  struct literal outside the engine any more.
  Instead: build it with `TileMap::new(sheet, width, height)` and set the
  fields you need afterwards.
- The plain collision-mesh loaders panic instead of returning a `Result`.
  Instead: call `try_load_collision_mesh` or `try_load_collision_mesh_with` where you handled the error, or drop the `.unwrap()` / `.expect(..)` where a missing file is a bug.
- `Assets::load_strings` panics instead of returning a `Result`.
  Instead: call `try_load_strings` where you handled the error, or drop the `?` / `.unwrap()` where a missing table is a bug.
- `Assets::load_resource_json` and `load_component_json` are `load_reflected_resource_json` and `load_reflected_component_json`.
  One name per loader: they still return a `Result` and merge a file's
  fields into the reflected value, where `load_resource` replaces the whole
  resource. The old names are removed without a deprecated alias.
  Instead: rename the calls; arguments and results are the same.
- `ponko_unity::Error`'s public `message` field is the `message()` method now.
  The error keeps its text, path, line and kind private so more can be added
  without breaking callers; `Error` is still `Clone`, `Eq`, `Display` and
  `std::error::Error`.
  Instead: call `error.message()` for the cause alone (it no longer holds the
  file name or `line N:` prefix), `error.to_string()` for the whole text, and
  match on `error.kind()`, `error.path()` and `error.line()`.

**UI, text and input**
- `ScenarioCheck` is `#[non_exhaustive]` and has a new `Ui` variant.
  Code outside the engine can no longer match it exhaustively.
  Instead: add a `_` arm to a `match` on it, and build checks with
  `ScenarioCheck::action`, `resource`, `count` and `ui`.
- `UiInput` has new fields: `preedit`, `preedit_cursor` and `composing` for IME composition, and `dt`, `secondary`, `menu` and `captured` for the widgets.
  The engine fills them itself when it runs the UI.
  Code that builds a `UiInput` with every field named no longer compiles;
  code that ends with `..Default::default()` is unaffected.
  Instead: end the struct literal with `..Default::default()`, or set the
  new fields (an empty `preedit`, `None` and `false` mean no composition,
  `captured: None` no key-binding field waiting for a key).
- `GamepadCapabilities` has four new fields: `light`, `player_leds`,
  `touchpad` and `adaptive_triggers`.
  A custom gamepad backend that builds it with a struct literal naming every
  field no longer compiles; reading the fields and `Default` are unchanged.
  The C `PonkoGamepadInfo` layout is unchanged: C and C++ read the new flags
  through `ponko_ctx_gamepad_extras` or `ctx.gamepad_extras`.
  Instead: name the capabilities the backend provides and end the literal
  with `..Default::default()`.
- `ButtonBinding`, `AxisBinding`, `Axis2Binding`, `Action` and `InputEvent` are `#[non_exhaustive]`: a `match` on one needs a `_` arm.
  Instead: handle unknown variants with a `_` arm.
- `ActionMap::from_json` and `to_json` return the engine's `ponko::error::Error`.
  Decode messages keep the location and the action map's context; the serde
  traits and the JSON format are unchanged.
  Instead: use `ponko::error::Error` and classify with `ErrorKind::Json`.
- `UiRole`, `UiAccessValue` and `UiAccessAction` are `#[non_exhaustive]`: a `match` on one needs a `_` arm.
  New roles, values and assistive-technology requests can then come in minor
  releases.
  Instead: handle roles, values and requests added later with a `_` arm.
- `UiTheme` has new fields: `spacing`, `radius` and `tooltip_delay` for the widgets, and `transition` and `press_scale` for animated controls.
  Every built-in theme has a radius of 0, no transition and a press scale
  of 1, so it draws exactly as before.
  Instead: build themes with `..UiTheme::default()`, `UiTheme::dark()` or
  `UiTheme::light()` after the fields you set.
- `UiAccessNode` has new fields: `selected`, `expanded` and `position`.
  A struct literal or a pattern that names every field no longer compiles.
  Instead: end the pattern with `..` (or name the new fields).

**Audio**
- `ponko_ffi::PonkoBusEffectKind` has a new value,
  `PONKO_BUS_EFFECT_PITCH_SHIFT`, so a Rust `match` over every value no
  longer compiles. C and C++ code is not affected.
  Instead: add an arm for the new value, or a `_` arm.

**Physics, navigation and AI**
- `d3::NavObstacle` has `yaw` and `shape` fields and is `#[non_exhaustive]`.
  Obstacles can be turned boxes, cylinders and convex prisms now, so struct
  literals no longer build one.
  Instead: build it with `NavObstacle::new`, `NavObstacle::cylinder` or
  `NavObstacle::convex`, then set the fields you need.
- Waves from a spectrum are drawn on the simulation's clock, where floating bodies ride them, instead of the frame's.
  The clock is `sim_time()` plus `fixed_alpha()` of a fixed step, not
  `ctx.time.elapsed()`. Nothing changes until a game pauses or changes the
  simulation's speed; after that the sea stands still while paused and runs
  at the simulation's speed, and the drawn waves are the ones floating bodies
  ride. Waves from the wave table stay on the frame's clock.
  Instead: ask `height_at` or `sample` at `water.drawn_time(&ctx.time)` for
  what is drawn; a shader that drew something in step with a spectrum's
  waves from `frame.time.x` (still the frame's clock) takes the drawn time in
  a material value instead.

**Networking**
- A networked event type that leaves fields out when serializing no longer arrives.
  Events travel with their fields by position, so the receiver can't tell
  which field `#[serde(skip_serializing_if = ...)]` left out, and drops the
  event.
  Instead: remove the attribute and send the field as it is (an `Option`
  travels as `None`).
- The message protocol is version 2: hosts and clients of 0.1 and 0.2 can't play together.
  Packets start with `CRM2`, the Steam lobby's `ponko_protocol` is
  `ponko-messages-2`, and connecting takes one extra round trip on UDP. A
  version-1 client joining a new host, or a new client joining a version-1
  host, times out ("peer timed out"); Steam joins fail with "incompatible
  message protocol". From version 2 on, a host answers a hello of another
  version with a refusal the client reports as `Disconnected` with a reason
  starting "version mismatch".
  Instead: ship the host (a dedicated server too) and every client built
  with the same engine version.

**C and C++**
- C: `ponko_game_run` returns `PONKO_UNSUPPORTED` (was `PONKO_INVALID_ARGUMENT`) when the game can't start on this machine.
  That is: no usable GPU or graphics device, no window or event loop, or a
  library built without the `window` feature. Other statuses are unchanged,
  and C++'s `game::run` still throws a `ponko::error` of kind
  `PONKO_ERROR_UNSUPPORTED`.
  Instead: code that checks `!= PONKO_OK` needs nothing; code that compared
  the result with `PONKO_INVALID_ARGUMENT` to detect a missing GPU compares
  it with `PONKO_UNSUPPORTED`.

**Development and releases**
- `ScopeReport` has a new field, `allocs_per_frame`.
  A struct literal of it no longer compiles; reports read from JSON written
  before it get 0.
  Instead: end the literal with `..Default::default()`.

### Added

**API and crate layout**
- `Ctx::immediate_ui()` returns a `UiImmediateFrame` that draws immediate
  widgets with the context's graphics and input:
  `ctx.immediate_ui().button(rect, "Play")`. No widget call takes another
  service as an argument any more.
- `Ctx::start_scenario`, `Ctx::cancel_scenario` and
  `Ctx::quality_autotune_status` (3d): the calls that need several services
  now live on `Ctx`, the one place that holds them all.
- Resolve-once handles where calls took names, following the action id
  pattern: a name is looked up once, and a typo is an error
  (`ErrorKind::NotFound`) whose message lists the names there are, for
  example `unknown audio bus "musci" (known: music, sfx)`.
  - Audio: `Audio::find_bus(name)` finds a bus that exists without making
    one (`bus(name)` still makes it). `Audio::snapshot_id(name)` gives a
    `SnapshotId`; `start_snapshot`, `stop_snapshot` and `snapshot_weight`
    take it (no lookup) as well as the name (`SnapshotRef`). An id stays
    valid when the snapshot is replaced.
  - 2D skeletons: `skeleton2d::ClipId`, from `SkeletonData::clip_id` or
    `Skeleton::clip_id`; `Skeleton::play`, `play_once` and `crossfade` take
    it as well as the name (`ClipRef`). `Skeleton::try_parameter_id`,
    `SkeletonGraph::try_parameter` and `SkeletonGraph::try_state` are the
    checked lookups.
  - `Ui::try_find(name)` for a node that must be there, and
    `CrowdAnimation::try_clip(name)`.
  - `Input::set_touch_control_shown` and `touch_control_center` take an
    `ActionId` as well as the action's name.
- C: `ponko_ctx_audio_find_bus`, `ponko_ctx_audio_snapshot_id`,
  `ponko_ctx_start_audio_snapshot_id`, `ponko_ctx_stop_audio_snapshot_id`,
  `ponko_ctx_audio_snapshot_weight_id`, `ponko_ctx_skeleton2d_clip_index`,
  `ponko_ctx_skeleton2d_play_index`, `ponko_ctx_skeleton2d_crossfade_index`,
  `ponko_ctx_ui_try_find`, `ponko_ctx_set_touch_control_shown_id` and
  `ponko_ctx_touch_control_center_id`. The ABI version is unchanged: only
  functions were added.
- C++: `ctx.find_audio_bus`, `ponko::audio_snapshot_id` with
  `ctx.audio_snapshot_id` and overloads of the snapshot calls taking it,
  `ctx.skeleton2d_clip_index`, `skeleton2d_play_index`,
  `skeleton2d_crossfade_index`, `tree.try_find`, and
  `set_touch_control_shown`/`touch_control_center` taking a
  `ponko::action_id`.
- `scripts/api-usage.py` lists, from the API snapshots, where every public
  item is called (reference games, examples, cookbook recipes, guide
  snippets, tests, the C and C++ bindings, the other crates) and which items
  nothing calls by name. Its report, `scripts/api-usage.md`, groups those by
  module with the decision recorded for each in
  `scripts/api-usage-decisions.txt`; `api-usage.py check` (in the check
  list) keeps every decision pointing at a real item.
- Tests now call the unused items that are real features: controllers and
  joysticks, snapshot buffers and clock sync, latest-wins filters, prediction
  numbering, AI trees and considerations, plural tables, value schemas,
  world hash parts, tile animations, waves and currents, camera shake, rigs
  and pixel snapping, 2D skeleton graphs and constraints, UI read-back, touch
  control places and styles. The rest are kept, each with its reason in the
  report; nothing was removed or renamed.
- `NetMessages::steam_host_with(client, visibility, max_clients)`: the
  Steam Networking Messages host in a lobby of the given visibility, as
  `steam_sockets_host` takes it. A `SteamLobbyVisibility::Private` lobby is
  invite-only. `steam_host` stays friends-only.
- `SteamLobby::invite_user(SteamUserId)` invites one Steam user without the
  overlay, for a lobby screen with its own friend list; the friend joins as
  from an overlay invite (`steam_wait_for_invite`, `steam_lobby_from_args`).
  Private lobbies and single invites no longer need `unstable_steamworks`
  or `unsafe` code. Rust-only for now; C and C++ invite through the overlay.
- Unstable `From` conversions between `SteamLobbyId` and
  `unstable_steamworks::LobbyId`, and between `SteamUserId` and
  `unstable_steamworks::SteamId`, both ways, for calls through
  `SteamClient::unstable_steamworks()` (friends, matchmaking, rich
  presence) without `raw()`/`from_raw` round trips. They follow the
  `steamworks` version like the rest of the unstable Steam surface
  (STABILITY.md, "Unstable").
- The guide's "When something fails" chapter has a list of the common
  mistakes with their messages: wrong asset, wrong kind of file, a feature the
  build leaves out, a save that won't load, an unregistered component, a scene
  of the wrong kind, no GPU, and systems the schedule refuses.
- `ponko::reflect::world_hash(&world)`: one `u64` for everything reflected in
  a world, for determinism checks (replays, lockstep peers, tests). It reads
  components in place without making JSON (0.5 ms for 100,000 entities with
  three components, against 1.1 s for hashing the JSON save), and doesn't
  depend on memory or reflection order. C `ponko_world_hash`, C++
  `world.hash()`.
- `World::query_ref::<Q>()`: read-only queries on a shared `&World`, several at
  once, with the same filters, `get`, `single`, `count` and `iter`. A
  component the world hasn't seen matches nothing instead of being registered.
- `ctx.input.start_recording()` and `ctx.input.stop_recording()` record live
  input into an `InputScenario` that replays it on the same ticks;
  `ctx.input.is_recording()` says whether one is running. C
  `ponko_ctx_start_input_recording` and `ponko_ctx_stop_input_recording`,
  C++ `ctx.start_input_recording()` and `ctx.stop_input_recording()`.
- `SkyShader` is a handle like the others: `ctx.is_live`, `ctx.free` (a sky
  still set to it shows the atmosphere) and `LiveCounts::sky_shaders`.
- `Graphics::try_update_texture` returns a wrong byte count or a render
  target's texture as `ErrorKind::Invalid`.
- Tests: the frame uniform's fields, in order and at their offsets, are the
  same in the Rust struct, the engine's mesh shader and the cook's copy of
  its prelude; a crowd drawn on a device without a texture slot for the
  reflections' spread image (the baseline tier) reflects as a model does.
- `ponko::math`, a new root module: glam's `Vec2`, `Vec3`, `Vec4`, `Quat`,
  `Mat4`, `UVec2`, `EulerRot` and the constructors `vec2`, `vec3`, `vec4`,
  `uvec2`, re-exported as they are. It is their home; the prelude's math
  names are the same items. A game that writes `use ponko::*;` and depends on
  a crate named `math` now spells that crate `::math::...` (STABILITY.md,
  "The crate root and globs").
- `ponko::reflect::JsonValue` and `ponko::reflect::JsonMap`: `serde_json`'s
  `Value` and `Map<String, Value>` under the engine's names, so a game can
  name reflection and migration values without its own `serde_json`
  dependency. Reflection, save-format and migration signatures use them
  (the same types as before). `ponko_mcp` has `JsonValue`, `JsonMap` and
  `json!` for `Server::handle`.
- The prelude gains `ui::UiEvent`, `ui::UiLayout` and the gamepad types
  `input::Gamepad`, `GamepadAxis`, `GamepadButton` and `GamepadStick`: the
  tutorials' menus and gamepad bindings need no import beyond the prelude
  and their dimension module. `tests/first_game_imports.rs` lists what the
  tutorials and the reference games still import, with the reason for each.
- STABILITY.md, "Re-exported crates": what an upgrade of `glam`,
  `serde_json` or `steamworks` means before and after 1.0 (from 1.0 on, an
  incompatible `glam` or `serde_json` comes only with a Ponko major
  release), and that the prelude's contents change only in minor releases.
- Errors name the fix for the first-hour mistakes: a missing asset lists the
  folders searched (each once) and the near-miss names next to it, an
  unreflected component names `world.reflect::<T>()`, a double mutable query
  says to split it, a left-out format names its Cargo feature, and freed or
  foreign GPU buffers, meshes, materials and texture arrays say what to make
  instead. `should_panic(expected)` tests in `tests/error_messages.rs` pin
  the wording.
- Guide chapter "When something fails: reading the message", and a
  "Panics and error messages" section in Contributing.
- `scripts/collect-panics.py` lists every `panic!`, `assert!`, `unwrap` and
  `expect` in the engine and ECS with its reviewed classification
  (`scripts/panic-audit.json`, internal invariants marked as such);
  `--check` fails on a new or changed site until it is reviewed, `--record`
  writes the decision.
- `ponko::render::RenderTarget` (with `3d`): a render target is its own handle kind, so an image can't be passed where a camera, a compute pass, a target effect or a UI panel draws, and a render target can't be passed where only a plain texture belongs. `target.texture()` reads it as the texture it draws into (free, `const`); `width`, `height` and `size` as on `Texture`; `RenderTarget::NONE`. It is a `#[repr(transparent)]` wrapper of `Texture`, with the same size and layout; `ctx.is_live`, `ctx.free`, `ctx.gfx.is_live` and `ctx.gfx.free` take it like any handle, and a target and its texture live and die together.
- Every handle kind is now listed and documented as its own type (the `Handle` docs, with compile-fail examples, and the guide's Handles chapter, "Each kind is its own type"). `tests/handle_layout.rs` asserts every kind's size and alignment, and `tests/handle_compat.rs` loads scene files with texture, mesh and material handles written before this change (`tests/fixtures/saves/handles_v1.json`, `handles3d_v1.json`).
- C: `PONKO_HANDLE_RENDER_TARGET` (13), the render target kind checked at run time. `ponko_ctx_handle_is_live` with it answers whether a texture is a live render target; `ponko_ctx_handle_free` with it frees one, and refuses a plain texture (`PONKO_INVALID_ARGUMENT`). The C ABI version is unchanged: the enum only gained a value.
- C++: `ponko::render_target`, a `PonkoTexture` that converts to `ponko::texture` and has `texture()`; `ctx.render_target_of(texture)` checks that a texture read back from the engine is a render target and throws `ponko::error` if not. `ponko::is_live`/`free` take it with the render target kind.
- Fallible startup with `Game::try_run`, `ConfiguredGame::try_run`,
  `try_run_headless`, `try_setup` and the default-compatible `Plugin::try_setup`.
  Setup errors stop subsequent callbacks and systems and retain their details.
- All consuming configuration builders remain available after `setup`, guarded
  by a test that checks new `Game` builder declarations for matching forwards.
- GPU startup returns surface, adapter, device and surface-configuration errors
  directly, without unwinding or invoking the panic hook, including abort builds.
  Later device/surface loss is outside the startup error contract.
- C: `ponko_game_run` returns a refused window, GPU or event loop as a status
  with `ponko_last_error()` and kind `PONKO_ERROR_UNSUPPORTED`, where it
  reported a caught panic (`PONKO_PANIC`) before.

**ECS and systems**
- `World::insert_many(&entities, value)` and `World::remove_many::<T>(&entities)`
  give many entities a component, or take it off them, in one call. The
  result is the same as `insert` or `remove` on each entity in turn; entities
  that move between the same two archetypes move together, and a whole
  archetype in row order (as a query visits it) is copied in one piece:
  6 to 13 times faster than single moves in the `ecs/add-remove` benchmark.
  Rust only for now; no C or C++ binding yet.
- Diagnostics that say which system blocked a batch and why:
  `PlannedSystem::boundary` (per system, what kept it out of the stage
  before its own), `Boundary::KeepsOrder`, `SystemDiagnostics::blocked`
  (`BlockedDiagnostics`) and `StageDiagnostics::systems` in
  `ctx.ecs_diagnostics()` and the `ecs_diagnostics` tool. The printed plan
  names each batch member's own reason when it differs from the stage's.
- 2D physics queries on par with 3D: `d2::Physics::cast_ray_all` (every
  hit, nearest first, into a buffer you keep), `cast_circle`, `cast_box`
  and `cast_capsule` (swept shapes), `overlap_box` and `overlap_capsule`,
  `closest_point` (the nearest collider and its nearest point, as
  `d2::PointHit`) and `penetrations` (how deep a shape sinks into each
  collider and the way out, as `d2::Penetration`, deepest first). With
  reused buffers none of them allocate.
- 2D collision groups: `d2::CollisionGroups` (the 2D form of
  `d3::CollisionGroups`, same rules), `Collider::groups` and
  `QueryFilter::groups`. Colliders in groups that don't touch pass through
  each other, and queries see only the colliders their groups touch.
- One set of query rules for 2D and 3D, in both guides and in
  `d2::Physics`'s docs: direction normalization, range, starting inside a
  collider, order with ties broken by entity id.
- C: `ponko_ctx_cast_ray2d`, `ponko_ctx_cast_ray2d_all`,
  `ponko_ctx_cast_circle2d`, `ponko_ctx_cast_box2d`,
  `ponko_ctx_cast_capsule2d`, `ponko_ctx_overlap_point2d`,
  `ponko_ctx_overlap_circle2d`, `ponko_ctx_overlap_box2d`,
  `ponko_ctx_overlap_capsule2d`, `ponko_ctx_closest_point2d`,
  `ponko_ctx_penetrations2d` (they take the collision groups next to the
  `PonkoQueryFilter2D`), `ponko_set_collider_groups` and
  `ponko_collider_groups`, with `PonkoPointHit2D` and `PonkoPenetration2D`.
  C++: `ctx::cast_ray_all`, `cast_circle`, `cast_box`, `cast_capsule`,
  `overlap_point`, `overlap_circle`, `overlap_box`, `overlap_capsule`,
  `closest_point`, `penetrations`, `filter2d::groups`, and
  `world::set_collider_groups` / `collider_groups`.
- Benchmark `physics/rays-10k`: 10,000 ray casts, all-hit rays and circle
  casts through a 2D scene, through Ponko and through rapier directly.
- `Commands::spawn` returns the new entity right away: later commands of the
  same buffer (and anything else) can use it before the spawn is applied.
  The id is reserved in the world, so it never collides with other spawns,
  also from buffers on other threads; if the spawn never happens (the
  buffer is cleared or dropped, an earlier command panics, the bundle can't
  be spawned), the id is given back and its handle stays dead.
  `World::commands`, `Commands::bind` and `Commands::is_bound_to` tie a
  buffer to a world. Ids are deterministic like the rest of the world: the
  same spawns in the same order get the same ids, with the parallel
  scheduler on or off and on any number of threads. Spawns recorded from
  the threads of a parallel query get distinct ids, all applied, but which
  item gets which id depends on timing (STABILITY.md says so; spawn after
  `par_collect_into` to keep them deterministic).
  Direct spawns and despawns cost what they did: the world takes new
  indices for its own spawns 256 at a time and touches nothing it shares
  with command buffers. A deferred spawn costs about 2 ns more (its slot
  is set aside at an earlier apply, its id goes through the buffer's list,
  and the slot is touched once more to claim it). Measured with
  `ponko_bench` in release builds (fastest of 10 runs for `spawn`/`despawn`,
  of 50 for `commands`; three interleaved runs of the code before and after;
  M4 Max at load average 58–61 from other builds), milliseconds:
  | Benchmark | Before | Reserved ids |
  |---|---|---|
  | `spawn/ponko` | 13.85–13.99 | 13.74–14.51 |
  | `spawn/ponko-batch` | 10.34–10.64 | 9.96–10.45 |
  | `despawn/ponko` | 8.14–8.54 | 8.23–8.26 |
  | `commands/insert-remove` | 4.59–4.76 | 4.71–4.77 |
  | `commands/spawn-despawn` | 2.38–2.42 | 2.59–2.83 |
  | `commands/spawn-then-despawn` | | 2.39–2.40 |
  | `commands/spawn-link` | | 3.18–3.34 |
  | `commands/spawn-then-link` | | 2.90–3.09 |
  Chosen over a `spawn_then(bundle, |world, entity| ...)` callback (the
  `spawn-then` rows), which costs what a spawn did, because a reserved id
  is an ordinary value at once: it can be stored, returned, linked both
  ways between new entities and passed to C. Lock-free reservation on a
  cursor over the world's free list was ruled out because buffers may
  record on other threads while the world spawns directly, so every direct
  spawn would update that cursor atomically: one uncontended atomic
  operation in `World::spawn` alone cost 3.5% on `spawn/ponko`.
- `World::return_idle_ids` sizes the free slots the world sets aside for
  deferred spawns, once per frame (the engine calls it): as many as command
  buffers took in one frame lately, the rest back to the free list, so a
  game that stops spawning through commands doesn't keep them, and one
  that spawns a burst now and then doesn't grow its slot table.
- C: `ponko_ctx_spawn_later_entity` is `ponko_ctx_spawn_later` that also
  writes the new entity. C++: `ctx::spawn_later` returns the entity.
- Inspector: the **Debug drawing** section lists every 3D camera (the
  window's, each extra view's, each target camera's) with its
  `Camera::debug_draw` as three buttons: **Default**, **On** and **Off**.
- glTF scenes keep a node attached to the root or a node of another glTF
  scene on it through a save, a reopen and the editor's Play/Stop,
  whichever scene spawns first. A scene's saved record takes an `"id"`
  (older records are numbered on load), and a node attached to another
  scene's root or node takes `"scene_parent": { "scene": id, "node": path,
  "link": offset }` (no `"node"` for a root).
- Game states (`ponko::state`): `Game::state(initial)` gives a game a state
  machine of its own `enum` (title, loading, playing, paused), kept in the
  reflected `State<S>` resource, so world saves, tools and the lockstep
  checksum carry it. `ctx.set_state`, `ctx.push_state` and `ctx.pop_state`
  ask for changes, which apply only between simulation ticks: at the end of
  the tick that asks (or before the next tick, when asked for in setup or
  by a frame system). `Game::on_enter` and `Game::on_exit` run work when a
  state starts and ends; `push_state` puts a pause over play without
  leaving it. Guide: "Game states".
- Run conditions: `IntoSystem::run_if` with `in_state(..)`, `in_stack(..)`,
  a closure `|ctx: &Ctx| -> bool`, and `Condition::not` / `or`. State
  conditions declare a read of the state and keep view systems in parallel
  batches; a closure condition makes a view system run alone
  (`Alone::Condition`, diagnostics reason `condition`). Systems without a
  condition show no measured overhead above the noise (`conditions/` benchmark).
- `Game::has_state`, and `ctx.state::<S>()` to read the current state.
- Timed states: `State::ticks()` counts the ticks run with the current
  state on top (reset by every change, stopped while a state is pushed
  over it, saved with it), and `ctx.seconds_in_state::<S>()` gives it in
  seconds. Every set is a change, the same value included (a restart), so
  a fade or a death animation reads its time here instead of keeping a
  timer in the state value. Loading a world restores the saved state
  without on-exit or on-enter work.
- C and C++: `ponko_game_state`, `ponko_game_on_enter`, `ponko_game_on_exit`,
  `ponko_game_add_system_in_state`, `ponko_game_add_system_in_stack`,
  `ponko_ctx_state`, `ponko_ctx_state_in_stack`, `ponko_ctx_state_ticks`,
  `ponko_ctx_set_state`, `ponko_ctx_push_state`, `ponko_ctx_pop_state`,
  and run conditions as callbacks: `ponko_game_add_system_if` with a
  `PonkoConditionFn`; in C++ `game::state`, `on_enter`, `on_exit`,
  `add_in_state`, `add_in_stack`, `add_if` (any callable) and
  `ctx::state<S>()`, `set_state`, `push_state`, `pop_state`,
  `in_state_stack`, `state_ticks`.
- The Coin Dash and Ember Hop reference games use engine states, including
  a pause pushed over play; Ember Hop's dying and fading screens are timed
  states, and its tests check that on-enter and on-exit work runs once per
  change of screen.
- An entity hierarchy (`ponko::hierarchy`, guide chapter "The entity
  hierarchy"): attachments are its parent links, and each parent keeps its
  children in order in a reflected `Children` component that the engine
  maintains. `WorldHierarchy` (in the prelude) adds `set_parent` (keeps the
  child where it is, refuses loops), `remove_parent`, `set_child_index`,
  `parent`, `children`, `descendants` (depth first, in child order),
  `ancestors`, `despawn_recursive` and `update_hierarchy` to `World`. Saves,
  scenes and prefabs keep the order; the editor's hierarchy panel shows it.
- C: `ponko_set_parent`, `ponko_remove_parent`, `ponko_parent`,
  `ponko_children`, `ponko_descendants`, `ponko_ancestors`,
  `ponko_set_child_index`, `ponko_despawn_recursive`,
  `ponko_update_hierarchy`; C++: the same as `ponko::world` methods.
- Benchmarks `hierarchy/100k-{deep,wide}-{moving,still}`,
  `hierarchy/100k-wide-churn`, `hierarchy/100k-pairs-{first-update,moving}`
  and `hierarchy/100k-wide-moving-3d` in `ponko_bench`.
- Component hooks: `World::on_add::<T>`, `World::on_replace::<T>` and
  `World::on_remove::<T>` register functions the world calls inside the
  operation that adds, overwrites or removes a `T` (every kind of spawn,
  `insert`, `remove`, `despawn`, `clear`, `reset`, loading a save). A hook
  gets a `HookWorld` (in the prelude): components, resources, queries and
  `resource_scope`, with spawns, despawns, inserts and removes recorded to
  `commands()` and applied before the call returns. Hooks run in
  component-id order, then registration order, and never during parallel
  queries. Components without hooks cost nothing extra: spawning runs the
  same code as before, despawn, insert and remove test one flag
  (measurements in `crates/ponko_bench/HOOKS.md`). Rust only for now; C and
  C++ can't register hooks yet, but their structural changes run the
  hooks registered from Rust. See "Component hooks" in the guide's ECS
  chapter.
- `ponko_bench`: `ecs/*/ponko-hooks-elsewhere` and `ecs/*/ponko-hooked`
  rows, and `hierarchy/100k-pairs-parent-churn` (parents despawned every
  update).
- Required components: a component names the components an entity with it
  needs, with their starting values, and every spawn and insert that gives
  an entity the component adds the ones it lacks (every kind of spawn,
  commands, loading a save, the dynamic, C and C++ paths), in the same
  structural change. Given values win; requirements go through (the
  nearest one's value wins). Declare them with
  `#[derive(Component)] #[component(requires(Transform, Health = Health(100)))]`
  or by writing `Component::requires` (`Requires::add`, a new provided
  method with an empty default, so existing impls are unchanged). At run
  time: `World::require::<T, R>(make)` for types you don't own,
  `RawWorld::require_dynamic` (a byte template) for dynamic components,
  `World::requires` to ask. Bundles that lack nothing and worlds without
  requirements spawn through the same code as before (measurements in
  `crates/ponko_bench/REQUIRED.md`). See "Required components" in the
  guide's ECS chapter.
- C and C++: `ponko_component_require(world, component, required, value)`
  and `world.require<T>(value)`, copying a plain value's bytes.
- `ponko_bench`: `ecs/*/ponko-requires-elsewhere` and `ecs/*/ponko-required`
  rows.
- Plugins are the extension point for other people's crates (guide:
  "Plugins for other crates"). `Plugin::name` (the type's path by default)
  and `Plugin::dependencies` (names that must be added first) make
  dependencies and their order explicit; `Game::has_plugin` lets a plugin
  bring a dependency along. A duplicate or a missing dependency fails
  startup with an `ErrorKind::Invalid` error before any setup runs.
- `Game::debug_toggle` adds a named switch for a game's or plugin's own
  debug drawing: `DebugDraw::toggled`, `set_toggle` and `toggles`; the
  control endpoint's `debug.draw` takes and reports `toggles`, and so does
  ponko-mcp's `debug_draw` tool.
- `Game::control_method` adds a namespaced method (`weather.set_rain`) to
  the control endpoint; `methods` and `capabilities` list it with
  `"added": true`, `capabilities` lists the game's plugins, and ponko-mcp's
  new `call_game_method` tool calls it.
- `ponko new --plugin`: a plugin crate (library, headless tests, the engine
  without default features, README with a version table, AGENTS.md).
  `tests/plugin-crate` is that template built as a separate Cargo project
  outside the workspace, so the engine's tests prove a plugin compiles
  against the public API alone.
- 3D physics queries on par with 2D: `d3::Physics::cast_box` (an oriented
  box sweep), `overlap_point`, `closest_point` (the nearest collider and
  its nearest point, as `d3::PointHit`) and `penetrations` (how deep a
  shape sinks into each collider and the way out, as `d3::Penetration`,
  deepest first, equal depths by entity id). They take the 3D
  `QueryFilter` with its collision groups and follow the query rules both
  guides share. With reused buffers none of them allocate, also against
  convex hulls, triangle meshes and heightfield terrain.
- C: `ponko_ctx_cast_box3d`, `ponko_ctx_overlap_point3d`,
  `ponko_ctx_closest_point3d` and `ponko_ctx_penetrations3d`, with
  `PonkoPointHit3D` and `PonkoPenetration3D`. C++: `ctx::cast_box3d`,
  `overlap_point3d`, `closest_point3d`, `penetrations3d`, `point_hit3d`
  and `penetration3d`.
- The 2D/3D rule test (`the_rules_match_3d`) asks every query of the same
  scene in both dimensions and compares the answers.
- Benchmark `physics3d/queries-10k`: 10,000 ray casts, all-hit rays,
  sphere and box casts, closest points and penetrations through a 3D
  level, next to the 2D `physics/rays-10k`, with rapier's own ray and box
  casts for comparison.
- The `physics3d` example drops a crate with a right click, where a box
  sweep from the eye stops.
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
- `Commands::despawn_recursive(e)` (trait `CommandsHierarchy`, in the
  prelude) despawns an entity and everything under it when the commands
  are applied: the subtree as it is then. It works from view systems
  (`sys.commands`) and parallel queries (a shared `Mutex<Commands>`). C:
  `ponko_ctx_despawn_recursive_later`; C++: `ctx.despawn_recursive_later(e)`.
- `Changed<T>` and `Added<T>` skip 64 entities at a time where nobody wrote:
  a tracked column keeps the newest change of each run of 64 rows. With 1M
  tracked entities a `Changed` loop that finds nothing takes 0.017–0.018 ms
  instead of 0.50–0.52 ms; 1% changed, spread out: 0.41–0.43 instead of
  0.55–0.56 ms; all changed: 1.0–1.1 instead of 1.3 ms. A still hierarchy
  of 100,000 attached entities updates in 0.002–0.003 ms instead of
  0.051–0.066 ms. Spawns, inserts, despawns, `&mut` loops and plain
  queries cost the same, within the noise of a busy machine (±4%), except
  a despawn from a tracked archetype, 3–4% slower (12.9–13.4 vs 13.5–14.0
  ms for 1M; untracked despawn is unchanged). Release builds, Apple M4
  Max, load average 38–63, the fastest of each benchmark over three
  interleaved runs.

**Rendering**
- Sprite blend modes: `BlendMode` (`Alpha`, `Additive`, `Multiply`,
  `Premultiplied`) as a component next to a `Sprite`, and
  `DrawParams::with_blend` for immediate draws. They work on every GPU,
  WebGL2 included.
- Sprite shaders and materials: `Graphics::load_sprite_shader` (hot
  reloaded) and `create_sprite_shader` take WGSL with a
  `fn fragment(p: SpritePixel) -> vec4<f32>` hook; `SpriteMaterial`
  (from `create_sprite_material`, changed with `update_sprite_material`)
  gives it four `vec4` parameters and two extra textures. Add it as a
  component, or use `DrawParams::with_material`. Sprites are batched by
  texture, blend mode and material. Sprite shader errors carry the line in
  the shader's own file and are listed in `shader_status` with
  `ShaderKind::Sprite`; shaders are checked against what WebGL2 supports.
- `LiveCounts::sprite_shaders` and `sprite_materials`; `Ctx::free` and
  `is_live` take the new handles.
- The `sprite_effects` example: an additive glow, an outline and a dissolve.
- C and C++: `PonkoSpriteShader`, `PonkoSpriteMaterial`,
  `PonkoSpriteMaterialDesc`, `PONKO_BLEND_*`,
  `ponko_ctx_load_sprite_shader`, `ponko_ctx_create_sprite_shader`,
  `ponko_ctx_create_sprite_material`, `ponko_ctx_update_sprite_material`,
  `ponko_ctx_sprite_material_desc`, `ponko_ctx_draw_styled`, the
  `ponko_blend_mode_component` and `ponko_sprite_material_component`
  components, the `PONKO_HANDLE_SPRITE_SHADER` and
  `PONKO_HANDLE_SPRITE_MATERIAL` handle kinds and `PONKO_SHADER_SPRITE`;
  in C++ `ponko::sprite_blend`, `sprite_material_of` and the matching `ctx`
  methods.
- A headless sprite drawing benchmark (`ponko_bench sprites/`).
- 2D particles, without the `3d` feature: `d2::ParticleEmitter` next to a
  `d2::Transform` spawns particles at a rate or in bursts from a point,
  circle, cone or rectangle, moves them with gravity and drag, and draws them
  in the sprite batch at the emitter's `z`, with a sprite `BlendMode` and an
  optional `SpriteMaterial`. Settings (`d2::ParticleSettings`) use the 3D
  emitter's vocabulary in pixels: lifetime, speed, size and rotation ranges,
  size curves, color gradients, sprite sheets, world or local space, a pool
  limit and a seed. About 100,000 living particles take 0.84 ms of CPU per
  frame on an Apple M4 Max (`ponko_bench particles2d`). The guide's Drawing
  chapter has a "Particles" section and the `particles2d` example.
- `ponko::particles`, the home of `ParticleCurve`, `ParticleGradient` and
  `ParticleSpace`, shared by the 2D and 3D emitters (still in `d3` too), with
  `ParticleCurve::keys` and `ParticleGradient::keys`.
- `Systems::DRAW_PARTICLES`, the built-in system that draws 2D particles
  after the sprites (`systems::draw_particles`; C `PONKO_SYSTEM_DRAW_PARTICLES`,
  C++ `systems::draw_particles`).
- 2D emitters are reflected as `"ParticleEmitter2D"` (inspector, MCP tools,
  scenes), the image saved by file name like a sprite's.
- C and C++: `PonkoParticleSettings2D`, `ponko_particle_settings2d_default`,
  `ponko_ctx_set_particle_emitter2d`, `ponko_ctx_particle_burst2d`,
  `ponko_ctx_particle_live_count2d`, `ponko_ctx_particle_update2d` and
  `ponko_ctx_remove_particle_emitter2d`; C++ `particle_settings2d` and the
  matching `ctx` methods.
- 2D lights, without the `3d` feature: `d2::PointLight`, `d2::SpotLight`
  (a cone along the transform's rotation) and `d2::DirectionalLight`
  components, and an ambient light (`Graphics::set_ambient_light2d`). They
  light sprites, tile maps and particles; UI and text stay unlit. Lights add
  up and multiply the world, up to twice its color.
- 2D shadows: a light with `shadows: true` is blocked by `d2::Occluder`
  shapes (`rect`, `circle`, `polygon`, `line`), which turn, scale and mirror
  with their transform; `TileMap::occluder` outlines a map's wall tiles as one
  occluder with merged edges.
- Normal-mapped sprites and tiles: `Graphics::set_normal_map` gives every
  sprite and tile drawn from a texture a tangent-space normal map; normals
  turn and mirror with the sprite.
- Zero cost without lights: with no light and no ambient light nothing new
  is drawn or allocated. On an Apple M4 Max at 1920x1080 with 2,000 sprites,
  100 lights took 0.55 ms a frame and 100 shadowed lights with 200 occluders
  about 1 ms (`ponko_bench lights2d`). Works on WebGL2 (a `lights2d` browser
  smoke fixture).
- The guide's Drawing chapter has a "2D lights and shadows" section, and the
  `lights2d` example shows a lit tile dungeon.
- Lights and occluders are reflected as `"PointLight2D"`, `"SpotLight2D"`,
  `"DirectionalLight2D"` and `"Occluder2D"`.
- C and C++: `PonkoPointLight2D`, `PonkoSpotLight2D`,
  `PonkoDirectionalLight2D` with their `_default` and `_component`
  functions, `ponko_ctx_set_occluder2d_rect` (`_circle`, `_polygon`,
  `_line`), `ponko_ctx_set_tilemap_occluder`, `ponko_ctx_remove_occluder2d`,
  `ponko_ctx_occluder2d_edge_count`, `ponko_ctx_set_ambient_light2d`,
  `ponko_ctx_ambient_light2d`, `ponko_ctx_set_normal_map` and
  `ponko_ctx_normal_map`; C++ `point_light2d`, `spot_light2d`,
  `directional_light2d` and the matching `ctx` methods.
- GPU memory in the performance report: `PerfReport::gpu_memory` (a
  `perf::GpuMemory`) counts every texture and buffer the engine holds on the
  GPU, once each, by class: textures, meshes, GPU buffers, frame targets,
  shadows, the lights' shadow atlas, skinning, probes, instance batches and
  other, with the total and the platform's own figure where it reports one
  (Vulkan and DirectX 12 through wgpu's allocator, Metal's allocated size;
  `None` with the reason elsewhere, never 0). Headless runs have none.
- `Budget::gpu_memory_mb` limits it (`{"gpu_memory_mb": 512}` from C and
  C++); a report without GPU memory breaks it as "not measured".
  `PerfReport::regressions` compares it, and the text report and the
  profiling overlay show it.
- `perf.get`'s `render_inspector.gpu_memory_bytes` (and the MCP
  `perf_report` tool) report the same classes, total and adapter figure.
- Tests that global illumination's probes see point and spot light
  shadows, instance batches and glowing surfaces, with before/after
  reference images: an interior lit by a shadowed spot light
  (`gi_spot_interior`) and a room built of instance batches
  (`gi_instanced_room`), each rejecting the probes as they were before
  they saw that. The `gi_cost` benchmark also measures the probes
  without light shadows.
- `Post::ssr_transparent` (C: `PonkoPostEx` version 5, `ssr_transparent`):
  screen-space reflections on glossy transparent surfaces such as water and
  glass. Each such pixel marches its reflected ray across the opaque scene
  behind it. Off by default and in every quality preset.
- Stencil state on 3D materials: `MaterialDesc::stencil` takes a `Stencil`
  (reference, read and write masks, and per face a `StencilCompare` with the
  fail, depth-fail and pass `StencilOp`s). `Stencil::mark`, `inside`,
  `outside` and `once` cover masks, portals, outlines and see-through
  surfaces that blend once where they overlap; `Stencil::order` sets the
  order solid stencil materials draw in. It works on WebGL2 too.
- The scene depth gets stencil bits only once a material has a stencil:
  scenes without one keep their plain depth target, passes and pipelines,
  and draw as before.
- From C, `ponko_ctx_create_material_with_stencil`,
  `ponko_ctx_update_material_with_stencil` and the `ponko_stencil_*`
  constructors with `PonkoStencil`; in C++, `ctx::create_material(desc,
  stencil)` and `ponko::stencil_once` and its siblings.
- The `stencil` example (also in the web gallery) and the guide's "Stencil"
  section.
- Sky shaders: a game's own sky in WGSL (`fn sky(dir: vec3<f32>) -> vec3<f32>`),
  made with `Graphics::create_sky_shader` or `load_sky_shader` (hot reloaded) and
  shown with `Sky::shader`. The engine uses it for the background, the fog's
  color, reflections, the ambient light, transparent surfaces and reflection
  probe captures. `Sky::shader_params` passes four values (`frame.sky_params`);
  what fog, reflections and ambient light read is rebuilt at most every
  `SkyShader::REBUILD_FRAMES` frames when they change. `Sky::authored_ambient`
  keeps the authored ambient colors, and the sun keeps its authored color.
  Probes with `ProbeRefresh::LightingChanged` refresh when the sky shader or its
  values change. The `stylized_sky` example paints a sky through a day.
  C and C++ bindings don't have sky shaders yet.
- Reflection probes that refresh by themselves: a `d3::ReflectionRefresh` on
  a probe's entity captures it again `Never`, `OnDemand`
  (`Graphics::refresh_reflection_probe`), `Every(seconds)` or when the sun or
  sky changed past a threshold (`ProbeRefresh::LightingChanged`). A refresh
  is spread over 14 frames, at most one step a frame for all probes (one
  128² face rendered or filtered, the harmonics, or the diffuse light; at
  most 0.7 ms each on the GPU tested), into a copy that then eases in over
  `blend_seconds`, so a changing sky never pops. Each refreshing probe's copy
  (2.5 MiB) counts under `probes` in the GPU memory report. Rust only for
  now: C and C++ games bake again with `ponko_ctx_bake_reflection_probe`.
- Soft particles: `ParticleSettings::softness` (3D, `with_softness`) fades
  particles out over that many world units in front of the scene behind
  them, so smoke meets walls and the ground without a hard edge.
- Target cameras draw particles, trails and lines, sorted and facing each
  camera, so they show in planar reflections and monitors.
  `Camera::particles` (default on) and `TargetCamera::with_particles` turn
  them off per camera; reflection probe captures leave them out.
- Split toning (`Post::split_toning`, `SplitToning`): tints for the
  shadows and the highlights, blended with soft light, and a balance. Off
  by default (grey tints); no pass of its own.
- Depth of field (`Post::depth_of_field`, `DepthOfField`): an optional
  full-screen pass before bloom that blurs what is out of focus by the
  scene depth, as a lens does. Off by default; about 0.8 ms at 1920x1080 on
  an Apple M4 Max, and no extra image.
- `Post::lerp` blends two looks: amounts and colors mix, an effect off at
  one end fades out, choices switch halfway.
- `PostVolume`: boxes of post settings with a blend distance, mixed over a
  base look by the camera's position (`PostVolume::blend`). The
  `post_volumes` example walks between two of them.
- Block-compressed textures (feature `compressed-textures`): `load_texture`,
  `stream_texture`, `load_texture_array` and hot reload read KTX2 files of
  BC1–BC5, BC7, ETC2/EAC and ASTC 4x4 blocks (Zstandard or zlib
  supercompressed) and DDS files of BC blocks, and upload them as blocks
  with their own mipmaps. Color textures in BC7 take a quarter of RGBA8's
  video memory, normal maps in BC5 a quarter (the 3D shader rebuilds Z)
  and masks in BC4 an eighth; the asset inventory and residency budgets
  count their compressed bytes, and streaming keeps a smaller stored mip
  level resident. A GPU without the format, or a size that isn't a
  multiple of 4, gets the texture decoded to RGBA8 at load. Without the
  feature, a KTX2 file fails with an `Unsupported` error naming it.
- Basis Universal UASTC KTX2 files (feature `basis-universal`), transcoded
  at load to what the GPU samples: BC7/BC5, ASTC, ETC2 or RGBA8. Not in web
  builds, where they are an `Unsupported` error.
- `ponko textures`: converts a folder's PNG and JPEG images into KTX2 files
  with mipmaps (BC7, BC5, BC4, UASTC or RGBA8), with per-texture kinds
  (`color`, `linear`, `normal`, `mask`, `ui`, `skip`) from a
  `textures.json` next to them; `--check` fails when a file is missing or
  stale.
- The C library has the same `compressed-textures` and `basis-universal`
  features; `ponko_ctx_load_texture` then loads `.ktx2` files.
- `Sprite::flip_x` and `Sprite::flip_y` (with `with_flip_x` / `with_flip_y`)
  mirror a sprite's image in place: it covers the same rectangle around the
  same pivot, and attached children and their offsets are left alone, unlike
  a negative `Transform` scale. Works with regions, sheets, blend modes,
  materials, render layers and normal-mapped 2D lighting. Prefabs, scenes,
  saves and the world hash keep the flips (saved as `"flip_x": true` and
  `"flip_y": true`, only when set); an unflipped sprite saves and hashes
  exactly as before, and saves and prefabs written before load unflipped.
- Known gap: the C ABI's `PonkoSprite` has no flip fields yet (its layout is
  fixed), so a sprite read and written back through the C API comes back
  unflipped.
- C and C++: `ponko_world_set_shadow_only` and `ponko_world_is_shadow_only`
  (`world::set_shadow_only` and `world::shadow_only` in C++).
- `GpuResidency::mesh_cpu_bytes` (in `Ctx::asset_inventory` and the
  `assets` section of `perf.get`): the CPU memory the renderer holds for
  3D meshes. The render benchmark's memory section reports it too.
- `AssetInventory::texture_alpha` (`TextureAlphaMemory`, also in `perf.get`'s
  `assets`): the CPU bytes held by texture alpha summaries and masks.
- `perf.get` lists each GPU allocation on request: `gpu_allocations: true`
  (or a count for the largest) adds `render_inspector.gpu_allocations`, one
  entry per texture and buffer the engine holds, largest first, with its
  class, label (`3d color`, `3d depth`, `3d shadow map`, `3d light shadow
  atlas`, `3d bloom`, ...), bytes, a texture's size, mips, samples and
  format, and a buffer's usage (uniform, storage, vertex, index, indirect,
  staging, query, copy); with the buffers' bytes by usage and what a count
  left out. Built only when asked for. The MCP `perf_report` tool takes the
  same `gpu_allocations` parameter.
- `Lighting::shadow_sun_step` (radians, default 0, which keeps the exact
  direction as before): the sun's shadow cascades are drawn from a held sun
  direction that moves only once the true direction has turned past the
  step, so a sun that turns every frame (a day and night cycle) no longer
  makes every shadow edge shimmer, and far cascades stay cached between
  steps. Lighting keeps the exact direction. The hold has hysteresis, so a
  sun wobbling across a step boundary doesn't flip the shadows back and
  forth. From C and C++ it is `shadow_sun_step` of `PonkoLightingEx`.
- Item icons: `ctx.gfx.render_icon(mesh, &Icon)` (and `try_render_icon`)
  draws a model once into a render target of its own, framed to fill it,
  lit by the scene's sun and sky light, tone mapped, on a background that
  is transparent by default. It is drawn from a world of its own, so the
  game's world neither shows in it nor gets anything added, and it costs
  nothing after its first two frames.
- C and C++: `PonkoIcon`, `ponko_icon_defaults` and
  `ponko_ctx_render_icon` (`ponko::icon_of`, `ctx.render_icon`).
- Target cameras have a culling box (`TargetCamera::with_cull_box`): they
  draw only the models, LOD groups, terrain tiles, batch cells and crowds
  that reach into it.
- Target cameras have a background of their own (`TargetCamera::with_clear`),
  drawn instead of the frame's clear color and sky; `Color::TRANSPARENT`
  leaves alpha 0 for icons.
- Target cameras run screen effects of their own over their picture
  (`TargetCamera::with_effect`, `TargetCamera::effects`); the main view's
  effects don't reach them.
- Mipmapped render targets (`Graphics::create_mipmapped_render_target`):
  their levels are made on the GPU after every frame that writes them.
- C and C++: `PonkoTargetCameraEx` version 3 (`has_cull_box`,
  `cull_box_min`, `cull_box_max`, `has_clear`, `clear`),
  `ponko_ctx_set_target_camera_effects`, `ponko_ctx_target_camera_effects`
  and `ponko_ctx_create_mipmapped_render_target`
  (`ctx.set_target_camera_effects`, `ctx.target_camera_effects`,
  `ctx.create_mipmapped_render_target`).
- `MaterialDesc::masked_tint` (`with_masked_tint`): the model's colour
  (`Model::color`) tints only as far as the vertex colour's alpha says, so
  one mesh and one material take a colour per model on part of it (shirts,
  car paint), skinned meshes included. C and C++: `masked_tint` in
  `PonkoMaterialDescEx` version 2.
- Auto exposure (`Post::auto_exposure`, an `AutoExposure`): the exposure
  follows the lit frame's brightness, metered as its mean log luminance with
  a center weight and an optional mask texture, held between a min and a
  max in stops, moved by a compensation, and adapting at separate brighten
  and darken speeds. It starts at the target on the first frame and after
  `reset_temporal_history`, adapts per camera in split screen, works with
  every tone map, HDR output, every GPU tier and WebGL2, and costs nothing
  while off. `Post::lerp` blends it.
- C and C++: `PonkoPostEx` version 4 with the `auto_exposure` fields
  (`ponko::post_ex`); older callers leave them as they are.
- `Color::lerp(other, t)` mixes two colors in all four channels (alpha too), on
  the sRGB values the type holds; `t` is clamped to 0 to 1. `Color::hex_rgba`
  takes `0xRRGGBBAA`, and `"#ff8800".parse::<Color>()` reads `#rgb`, `#rrggbb`
  and `#rrggbbaa` (the `#` optional, either case); a bad string is an
  `ErrorKind::Invalid` error that names the problem. Rust only for now: a C or
  C++ `PonkoColor` is four floats.
- Two cookbook recipes, "Invincibility frames after a hit" and "Forgiving
  jumps: coyote time and a jump buffer", each run by a test.
- The guide's GPU regression tests chapter says how to run them on a card with
  little memory (`-- --test-threads=4`).
- Glass, liquids and other transmitting materials: `MaterialLayers::with_transmission`
  takes a `Transmission` layer (clear and diffuse transmission, volume thickness and
  attenuation, three-channel dispersion, approximate scattering, each with its map).
  Clear transmission keeps the surface's reflections and refracts a mipmapped copy of
  the opaque HDR picture, blurred by roughness and bent by the index of refraction;
  rays that leave the picture read the environment. The copy and its mip pass run
  only while glass is in the camera's draw list. WebGL2 reads a single-level copy.
- glTF: `KHR_materials_transmission`, `KHR_materials_volume`, `KHR_materials_dispersion`
  and `KHR_materials_diffuse_transmission` are read into that layer, with their maps;
  the draft `KHR_materials_scatter` and `KHR_materials_volume_scatter` load,
  approximated, with a warning. A volume's thickness follows the node's scale when a
  file loads as one mesh. `ponko cook` cooks files that use any of them.
- Material layers: `MaterialDesc::layers` (`MaterialLayers`) adds clear coat
  with its own normal map, sheen, specular strength and color, index of
  refraction, anisotropy and iridescence, each with its maps, and
  `MaterialDesc::unlit` draws a material without lighting. A material with
  layers draws with a layered build of the built-in shader; materials
  without them keep the plain build and cost nothing more.
- The glTF loader reads `KHR_materials_unlit`, `KHR_materials_clearcoat`,
  `KHR_materials_sheen`, `KHR_materials_specular`, `KHR_materials_ior`,
  `KHR_materials_anisotropy`, `KHR_materials_iridescence` and
  `KHR_node_visibility` (a hidden node is left out with everything under
  it). A file that requires `KHR_materials_volume` loads with a warning
  (transmission without refraction or absorption).
- C API: `PonkoMaterialDescEx` version 3 adds `unlit` and `layers`
  (`PonkoMaterialLayers`).
- Place imported glTF lights with a shared EV100 exposure for directional lux and local candela. A local light without an authored range reaches until its exposed light has faded. The glTF preview helper uses these lights, a neutral studio environment and a hue-preserving highlight curve.
- Light a scene with a linear HDR or EXR panorama or cubemap. The renderer prefilters diffuse irradiance and rough specular reflections using the same atlas as the sky and local probes.
- Select the hue-preserving tone curve in Rust, C and C++ to roll off bright colors without clipping their channels independently.
- glTF `KHR_animation_pointer` animations play. A clip's pointer channels load
  as property tracks (`AnimationClip::property_tracks`, `PropertyAnimation`,
  `AnimatedProperty`, `MaterialProperty`) next to its joint tracks:
  - node translation, rotation and scale through a pointer;
  - node visibility (`KHR_node_visibility`), also on nodes hidden at rest;
  - material values: base color, emissive color and strength, alpha cutoff,
    metallic, roughness, normal scale, and the factors of the clearcoat,
    sheen, specular, IOR, anisotropy and iridescence layers;
  - `KHR_texture_transform` offset, rotation and scale of the material's
    texture transform.
  A pointer the engine doesn't play (a camera's, a morph weight, a texture
  other than the one the material's transform follows, an extension it
  doesn't read) is named in the load warnings, one each.
- `Skeleton::property`, `Skeleton::set_property` and
  `Skeleton::joint_visible`, and `Joint::visible` / `Joint::with_visible`: a
  hidden joint isn't drawn, with everything under it, and what hangs from it
  with `AttachToJoint` (a model, a light) is scaled to nothing.
- `ModelLight::joint`: the joint of the file's skeleton a light's node is.
  Attach the light there and it follows its node's animation and visibility.
- `MaterialDesc::texture_transform` (`TextureTransform`: offset, rotation,
  scale) and `MaterialDesc::alpha_cutoff`.
- Compute results can be read back on the CPU:
  `ctx.gfx.read_gpu_buffer(buffer, 0..4)` and
  `ctx.gfx.read_render_target(target, at, size)` return a `Readback`, and
  `ctx.gfx.readback(pending)` gives the values (`&[Vec4]`) on a later frame,
  typically one to three frames old. Asynchronous only, the same on native,
  WebGPU and WebGL2 (render targets; a `GpuBuffer` has no GPU copy there,
  so its read fails with `ErrorKind::Unsupported`, as every read does in
  headless runs). Staging buffers are reused, so steady reads allocate
  nothing. Readback values are not deterministic and must not feed
  `sim_system`, replays, rollback or lockstep (STABILITY.md). A result is
  freed by reading it (there is no `free`); when more than 64 finished
  results wait unread, the engine logs one warning per run and keeps them
  all. The guide's
  "Reading compute results on the CPU" shows reducing a grid on the GPU and
  reading a few values. C API: `ponko_ctx_read_gpu_buffer`,
  `ponko_ctx_read_render_target`, `ponko_ctx_readback`; C++:
  `ctx::read_gpu_buffer`, `ctx::read_render_target`, `ctx::readback`.
- Waves from a spectrum for the water kit: `Waves::spectrum`
  with `WaveSpectrum` (wind, significant height, a repeating square of 64,
  128 or 256 cells, choppiness, seed) draws an FFT ocean. Compute passes
  work it out on the GPU; without compute shaders (WebGL2) the CPU runs the
  same FFT and uploads it, and `Water::height_at`, `Water::sample` and
  buoyancy query the same field. It adds 0.6 to 1.0 ms of GPU time over the
  Gerstner sea at 1280x720; the water guide has the costs and the advice on
  cells. `SPECTRUM_PERIOD` is how often the sea repeats (200 s).
- `CloudQuality::interleaved` says whether a cloud preset marches a quarter
  of its cloud image's pixels a frame.
- Probe views for global illumination leave out whole instance cells too far
  from every probe for their widest instance to be a pixel wide, on the CPU
  and the GPU culling paths (the GPU path with the widest instance of the
  whole batch), with the same result as testing each instance:
  in a scene of 250,000 rocks over 1 km x 1 km the CPU path's preparing went
  from about 9 ms to about 2.8 ms a frame. `Cell` (instance cells) gains a
  `reach` field.
- `LightBakeScene::add_world(&mut world, &gfx)` adds every model, the first
  level of every LOD group, the instance batches and the lamps of a world
  to a bake, with their materials and base color textures; models whose
  mesh has lightmap coordinates get lightmaps. `Graphics::keep_bake_sources`
  keeps the meshes it reads, the `Unbaked` component leaves a model out,
  and `WorldBake::insert_lightmaps` puts the results on the entities.
- `LightBakeScene::add_gltf(ctx, path, transform)` adds a glTF file's
  static nodes, materials, textures, first LOD levels and lights;
  `GltfBake::insert_lightmaps` lightmaps the spawned scene.
- `bake_lighting_gpu`: the same bake in a compute shader on any adapter
  with compute shaders (not WebGL2), checked against the CPU baker within
  its own noise.
- `BakeTexture` and `BakeModel::with_albedo_map`: bounced light takes the
  color of the base color texture where a ray hits, not only the flat
  material color.
- `LightBakeSettings::compress` (default on): lightmap pages are BC6H
  blocks (a byte a texel instead of 8) in the file and on GPUs that sample
  BC6H, and the shared-exponent format (4 bytes) on the others.
  `BakedLighting::is_compressed`.
- A `Lightmapped` component on a `LodGroup` lights its first level from
  lightmaps (renderer `k` reads entry `first + k`); other levels and
  instance batches take the probe volume's light.
- Temporal upscaling: `Post::upscaler = Upscaler::Temporal` (with
  `AntiAliasing::Taa` and a `render_scale` below 1) rebuilds the detail a
  lower render scale loses over frames, from TAA's jitter, motion vectors
  and history, instead of stretching the frame. It replaces TAA's resolve
  with one pass into a window-sized history; moving models leave no trail.
  `Upscaler::Bilinear`, the stretch, stays the default and is unchanged.
  C: `PonkoPostEx` version 3 adds `upscaler` (`PonkoUpscaler`); C++:
  `ponko::upscaler`. The guide's "Temporal upscaling" section has the
  measured images and GPU times, and what render scale does and doesn't do.
- `crates/ponko_bench/UPSCALING.md`: the evaluation, and what FSR, DLSS,
  XeSS and MetalFX would need from the renderer, their licences, GPUs, and
  how wgpu 30 hands out native textures for them.
- The inspector's quality object has `upscaler`.
- Rectangle and tube lights were measured for the clustered path and are
  not shipped: the guide's "Area lights: measured, not shipped" has the GPU
  cost per light at 1920x1080 (a tube 2.1x a point light, a rectangle 3.5x
  with a representative point and 5.6x with LTC), their error against a
  sampled reference, the WebGL2 check, why their shadows aren't affordable,
  and how to get their look with emissive surfaces, baked lighting, global
  illumination and lamps. The comparison is an offscreen test
  (`src/render/area_lights_render_test.rs`).
- `Camera::debug_draw` (`d3::Camera`), a flag of its own for whether a
  camera draws `ctx.debug`'s 3D lines and labels: `Some(true)` always,
  `Some(false)` never, whatever its `particles` says. `None`, the default,
  draws what cameras drew before: the window's cameras draw them, a target
  camera while it draws particles. `TargetCamera::with_debug_draw` sets it,
  so a monitor can show the debug drawing without the smoke, or a map be the
  only view that shows it. From C, the `debug_draw` field (a
  `PonkoDebugDraw`) of version 2 of `PonkoCamera3DEx` and
  `PonkoTargetCameraEx`; in C++, `ponko::camera_debug_draw`.
- Web: a lost WebGPU device's failure says why:
  `Failure::device_lost_reason()` is `DeviceLostReason::Destroyed` when the
  device's `destroy()` was called and `Unknown` for anything else (the GPU
  process crashed or was reset), so a game can word the two differently.
  The page's `onPonkoFailure` error has it as `reason` (`"Destroyed"` or
  `"Unknown"`, `null` for other failures), and the engine's message says
  "destroyed" or "lost". `web/smoke/runtime-failures.mjs` checks it.
- The render benchmark's `PONKO_RENDER_GPU_EVERY=n` times one frame in `n`
  (default 1, every frame), to measure what timing costs.
- `ctx.perf.set_gpu_timing(GpuTiming)` picks how often a game times its GPU
  passes, separately from profiling: `GpuTiming::Auto` (the default, as
  before: one frame in 16, every frame while profiling is on),
  `Every(n)` (`Every(1)` is every frame, also while profiling is on) or
  `Off` (no GPU times at all). `ctx.perf.gpu_timing()` reads it back. The
  overlay, traces, `PONKO_BENCH` runs and automatic quality still get every
  frame's times while they are on, whatever it says. A game that wants fresh
  `gpu_passes()` every frame no longer has to turn profiling on, which also
  times scopes and counts allocations. Rust-only for now.
- `ctx.perf.gpu_passes_not_timed()`: the passes of the latest timed frame
  whose timestamps didn't come back (an empty pass the GPU skipped, or a
  backend that writes none). The overlay lists them as "not timed", the
  profiler report adds a `WarningKind::PassesNotTimed` warning, traces mark
  them, and `perf.get` has `gpu_passes_not_timed`; they are no longer shown
  as 0 ms. The guide's "Which passes can be timed" table says which passes
  can be timed on which backend.
- `Post::ssr_thin_objects`: screen-space reflections keep thin things
  (rails, cables, poles, window frames) whole without TAA. The trace also
  reads a coarse copy of the depth (the nearest depth of 8 to 64 pixel
  blocks) along its long steps, which jumped over such things and left
  their reflections dotted. Off by default, and then the trace is the same
  shader as before; `GraphicsQuality::Ultra` turns it on. Offscreen tests
  at 1920x1080 without TAA: the nearest of four rails is reflected with
  0.87 of the light it shows itself, from 0.48, and every rail is reflected
  0.86 to 0.98 times as wide as a true mirror shows it. It costs about
  half a millisecond more per frame at 1920x1080 on an Apple M4 Max (the
  trace 0.80 to 0.98 ms, from 0.35 to 0.38; the guide has the table).
- C and C++: version 2 of `PonkoPostEx` (`PONKO_POST_EX_VERSION` 2) has
  `ssr_thin_objects`. Version 1 callers neither read nor change it. The
  ABI 2 `PonkoGraphicsQuality` has no field for it: setting a preset
  through it sets the field as the Rust preset does (on for ultra), and a
  custom struct leaves it as it is.
- The GPU timing choice from C and C++: `ponko_ctx_set_gpu_timing(ctx, kind,
  every)` and `ponko_ctx_gpu_timing(ctx, &kind, &every)` with
  `PONKO_GPU_TIMING_AUTO`, `_EVERY` (every `every`th frame) and `_OFF`; C++
  `ctx.set_gpu_timing(ponko::gpu_timing_every(4))` and `ctx.gpu_timing()`
  (also `gpu_timing_auto()` and `gpu_timing_off()`). A kind that is none of
  the three is `PONKO_INVALID_ARGUMENT` and changes nothing. The guide's
  "How often GPU passes are timed" shows both languages.
- `perf.get`'s `render_inspector.gpu_timing` of the control endpoint has
  `cadence` (`auto`, `every` or `off`, and `every`: the frame count for
  `every`): the game's own choice from `set_gpu_timing`. A tool can tell a
  game that turned GPU timing off (`cadence: "off"`, `available: true`, and
  `unavailable.gpu_ms.frame` says so) from an adapter without timestamp
  queries (`available: false`). Fields are only added: schema 1 stays.
- Instance cells: an `InstanceBatch` can take its instances one spatial cell
  at a time. `Graphics::add_instance_cell(batch, transforms, colors)` uploads
  a cell's instances and returns an `InstanceCell`;
  `replace_instance_cell` gives it new ones and `Ctx::free` removes it,
  without uploading the batch's other instances again. On the GPU-driven
  path a freed cell's records go to the next cells. Cells are culled and
  drawn in every view the batch is (camera, sun cascades, target cameras,
  point and spot light shadows) on either path, and cells of an automatic
  batch take the path the whole batch would. Freeing a batch frees its
  cells. `Graphics::instance_batch_len` counts its cells' instances too.
- `Graphics::stream_instance_cell(batch, job)` takes a `Job` that builds an
  `InstanceCellData` on a worker thread and uploads the cell in the frame's
  upload step, within the upload budget; `instance_cell_state` and
  `instance_cell_len` say where it is.
- Meshes that live instance cells draw are kept under a mesh residency
  budget; a cell added while its mesh is evicted draws once it is back.
- `LiveCounts::instance_cells`.
- C and C++: `ponko_ctx_add_instance_cell`, `ponko_ctx_replace_instance_cell`,
  `ponko_ctx_instance_cell_len`, `ponko_ctx_free_instance_cell` and
  `PonkoInstanceCell`; `ctx::add_instance_cell` and friends in `ponko.hpp`.
- A benchmark of streaming cells against making the batch again
  (`crates/ponko_bench/INSTANCE_CELLS.md`): a cell uploads in about 0.08 ms
  against 2 to 4.6 ms per cell that came or went when the batch is made
  again, on one machine.
- Groups in world-space panels (`UiPanel3D`) fade as one image too, into
  layers in the panel's pixels drawn before the panel.
- `perf::GpuUntimed` and `ctx.perf.gpu_untimed()`: why the latest frame has
  no usable GPU time (no GPU, no timestamp queries, the timer refused by the
  query-set budget, timing off, timestamps not back yet, the device lost),
  instead of only "not timed". The overlay shows it, the report adds a
  `WarningKind::GpuNotTimed` warning, the render-pass inspector has
  `gpu_timing.untimed`, and the editor's GPU panel says it. C:
  `ponko_ctx_gpu_untimed` (`PONKO_GPU_UNTIMED_*`); C++: `ctx.gpu_untimed()`.
- `perf::thread_net_alloc_bytes()`: net bytes for the calling thread (C
  `ponko_thread_net_alloc_bytes`, C++ `ponko::thread_net_alloc_bytes()`).
- `ponko_ecs::ThreadMeter`, `set_thread_meter` and `LoopTally`: per-thread
  counters that the ECS's parallel loops carry from their workers back to
  the thread that started them.
- The check list lints the threaded web build with `3d,bench-internals`.
- The order-independent transparency evaluation measures two ways of
  ordering a see-through model's own triangles (crossed cards, multi-pane
  glass in one mesh): sorting them by centroid every frame halves the
  error (multi-pane glass 10.3 to 6.4 levels) for 11.6 ns a triangle a
  frame; splitting them where they cross at build time and walking the
  BSP tree is exact (0.02) but cut dense card clusters into 37 times as
  many triangles. Neither is shipped; the numbers are in the 3D guide, "A
  model's own triangles: measured, not shipped".
- HDR output switches on while the game runs: `ctx.gfx.set_hdr_output` with
  `enabled` turns a window opened in SDR to HDR from the next frame, with no
  restart (the renderer's pipelines for the window's format are made again
  once; the game's textures and meshes stay). Switching off and on again
  costs a surface configuration.
- HDR10 metadata: `HdrOutput::max_content_light_nits` (MaxCLL) and
  `max_frame_average_nits` (MaxFALL), with the peak in use as the mastering
  peak, sent through DXGI's `SetHDRMetaData` on Windows and
  `CAMetalLayer.EDRMetadata` for PQ on Apple platforms. `HdrMetadata` and
  `HdrStatus::metadata` report what the display was given (`None` on
  Vulkan, in browsers and on macOS's scRGB, where there is no API for it or
  no need). C: `PonkoHdrOutput` version 2 (`max_content_light_nits`,
  `max_frame_average_nits`), `PonkoHdrStatus` version 2 (`metadata_sent`,
  `min_mastering_nits`, `max_mastering_nits`, `max_content_light_nits`,
  `max_frame_average_nits`); C++ through `ponko::hdr_output` and
  `ponko::hdr_status`.
- `perf.get`'s `render_inspector.counts.see_through_changes` (and
  `see_through_changes` in `PONKO_BENCH_OUT` reports and the rendering
  baseline's scene reports): the pipelines, bind groups, vertex and index
  buffers the see-through list set in the last frame, and its draws.
  `counts.state_changes` now includes the particles', ribbons', lines' and
  3D text's.
- `Perf::gpu_passes_over_limit()` says how many passes of the latest timed
  frame the GPU timer had no room for. The overlay's pass list, the HUD,
  `Perf::report()` and the control endpoint's `perf.get` (`gpu_passes_over_limit`)
  say it too, so passes are never dropped silently.
- Material shaders can call `texture_mip_bias()`: the mip bias the engine
  samples material textures with (negative while the temporal upscaler
  runs, else 0), for a shader's own `textureSampleBias`.
- Camera rigs, each a plain struct kept by the game and updated once a frame,
  moving the camera the same way at 30, 60 and 144 frames a second:
  `d2::FollowCamera` (dead zone, look-ahead, offset, level bounds),
  `d2::PixelPerfect` (a whole-number zoom for the window, a camera on the art
  pixel grid, the letterbox rectangle), `d3::OrbitCamera` (with
  `mouse_controls`), `d3::FirstPersonCamera` and `d3::ThirdPersonCamera`
  (follows a character and comes in front of walls: `update_with` for your own
  test, `update_with_physics` with the `physics3d` feature).
- `camera::CameraShake`: trauma-based shake on smooth noise, `apply_2d` in
  window pixels and `apply_3d` as small turns.
- `camera::smooth_damp`, `camera::damp` and `camera::damp_angle`: exact,
  frame-rate independent smoothing for `f32`, `Vec2`, `Vec3` and the like.
- `d3::Camera::world_to_screen`, `screen_to_world`, `screen_to_ground`,
  `right` and `up`; `d3::Camera::viewport` is public. The guide's "Screen and
  world" table lists every conversion for both cameras.
- C and C++: the rigs and the shake as versioned descriptors
  (`PonkoFollowCamera2D`, `PonkoPixelPerfect`, `PonkoCameraShake`,
  `PonkoOrbitCamera`, `PonkoFirstPersonCamera`, `PonkoThirdPersonCamera`, each
  with `_defaults` and update calls; `ponko::follow_camera2d` and the others in
  C++), `ponko_smooth_damp`, `ponko_damp`, `ponko_damp_angle` (and `_vec2`,
  `_vec3`), `ponko_ctx_world_to_screen`, `ponko_ctx_screen_to_world`,
  `ponko_ctx_world_to_screen3d`, `ponko_ctx_screen_to_world3d` and
  `ponko_ctx_screen_to_ground`.
- Several cameras in one window: split screen, minimaps, picture in
  picture. Every 2D and 3D camera has a `rect` (`ponko::camera::Viewport`,
  fractions of the window; `Viewport::split(players, index)` for split
  screen) and `layers` (`RenderLayers`). More cameras go in
  `ctx.views2d` (`d2::View`) and `ctx.scene3d.views` (`d3::View`) and draw
  after the main one, in order, each into its rectangle with its own
  clear colour and, in 3D, its own `Post`.
- 3D views are whole frames of their own camera: their own culling and
  temporal history (TAA, temporal AO, reflections), sharing the frame's
  skinning, compute passes, global illumination and shadow maps. The
  sun's cascades are fitted around every view; point and spot light
  shadows go to the lights nearest any camera, in one atlas drawn once a
  frame. Views draw without occlusion culling (never too little), and the
  main camera's `Post::msaa` applies to all of them.
- 2D lights, shadow casters, ambient light and normal maps light every 2D
  view, each in its own rectangle.
- `ctx.debug` draws on layer 0: its lines and labels (and the built-in
  views) show in every 2D and 3D camera that shows layer 0; window-pixel
  shapes are drawn once.
- `RenderLayers`, a component (saved as its bits) choosing which cameras
  draw a 3D model or a 2D sprite entity; `Gfx::set_layers` for what is
  drawn with `ctx.gfx`. A model off a camera's layers still casts its
  shadow there.
- `d2::View::camera_at` and `SceneSettings::camera_at`: the camera drawing
  at a window point, for picking.
- C and C++: `PonkoView2D`, `PonkoView3D` (versioned) and `PonkoViewport`,
  with `ponko_ctx_set_view2d`, `ponko_ctx_set_view3d`, `ponko_ctx_camera_at`,
  `ponko_ctx_screen_to_world_at`, `ponko_ctx_screen_to_ground_at`,
  `ponko_world_set_render_layers` and the rest; `ponko::view2d`,
  `ponko::view3d` and `ponko::split_viewport` in C++.
- Guide chapter "Several cameras: split screen and minimaps" and the
  `split_screen` example.
- Cost, from the scene benchmark (`PONKO_RENDER_VIEWS`, Apple M4 Max,
  1280 x 720, high quality, load average 62–78): GPU median 3.1–3.5 /
  2.3–3.7 / 3.4–3.9 ms and CPU median 1.2–1.5 / 1.9 / 3.1–3.3 ms for 1, 2
  and 4 cameras. Allocations per frame 1222 / 1843 / 2791 (1222 / 1847 /
  2832 before the views stopped building lists of scenes each frame).
  Collecting a view allocates nothing once warm (a counting-allocator
  test); what a view adds is wgpu recording its render passes.
- The sky kit: `ctx.scene3d.sky.kit = Some(SkyKit::default())` gives the
  physical atmosphere with raymarched clouds (`Clouds`: coverage, kind,
  wind, altitude, thickness, density, self-shadowing, silver lining), a
  moon with phases (`Moon`) and a star field that turns through the night,
  on the sky shader hook, so the fog, reflections, ambient light and
  reflection probes see it too. The clouds cast shadows on the ground (a
  small projected map) and dim the fog's glow toward the sun.
  `CloudQuality` picks `Low`, `Medium` or `High` (marched at a quarter of
  the frame's pixels with temporal reprojection; measured GPU times in its
  docs) or `Flat`, a 2D cloud layer that WebGL2-class GPUs always get.
  Nothing is made or drawn while `Sky::kit` is `None`. Rust-only for now.
- `DayNight` moves the sun and the moon through the day and the moon's
  phases: `day.update(ctx)` once a frame.
- Sky shaders can call `drawing_background()` to leave sharp detail out
  of the sky texture.
- The `sky_kit` example.
- An offscreen evaluation of order-independent transparency
  (`src/render/oit_render_test.rs`): crossing glass panes, particles
  through glass and crossed foliage cards drawn sorted, with weighted
  blended OIT and with depth peeling, each against an exact depth-peeled
  reference, with a GPU cost benchmark on the engine's pass timer and a
  run on WebGL2 limits. Weighted blended OIT was turned down: it trades
  sorting's wrong-order pixels for wider tint errors (crossing panes: mean
  error 7.1 against 8.6 levels, but 65% of pixels visibly off against 35%;
  foliage: 6.9 against 4.6), and costs 0.13 to 0.25 ms more at 1920x1080 and
  10 bytes a pixel (83 MB with 4x multisampling). The numbers are in the
  3D guide, "Order-independent transparency: measured, not shipped".
- Baked lighting for static scenes: `bake_lighting` path traces a scene's
  bounced light (sky, sun, point and spot lights, emissive surfaces) ahead
  of time on the CPU into lightmaps (with the direction light comes from,
  for normal maps) and a probe volume for moving models. `BakedLighting`
  saves to bytes and loads back (`Graphics::load_baked_lighting`, or
  `try_load_baked_lighting` for the error as a value), and
  `Graphics::set_baked_lighting` draws with it: models with a `Lightmapped`
  component read their lightmap, the others the probes, and the sun and
  lamps stay realtime with their shadows. It needs no compute shaders and
  no texture slot of its own, so it works on WebGL2 and the Low preset.
- `MeshData::generate_lightmap_uvs` makes lightmap coordinates (`uvs1`)
  for meshes without a second UV set.
- The `bake_lighting` example bakes a room's light from the command line
  (`-- bake room.lit`) and shows it.
- HDR display output, off by default: `Game::hdr_output(HdrOutput::on())`
  opens the window in an HDR format and color space where the display system
  offers one (scRGB on Windows and macOS, PQ/HDR10 next, the extended sRGB
  canvas in browsers with WebGPU). The 3D frame's tone mapping (ACES, AgX,
  Neutral, or none) keeps its SDR look below near white and opens its
  highlights up to the peak; sprites and UI stay at paper white.
  `ctx.gfx.set_hdr_output` changes paper white and peak and switches between
  HDR and SDR while the game runs; `ctx.gfx.hdr_status()` reports the
  encoding, the brightness in use, what the display reports and why HDR is
  off (`HdrOffReason`). Off, the frame is drawn exactly as before. C:
  `PonkoHdrOutput`, `PonkoHdrStatus`, `ponko_game_hdr_output`,
  `ponko_ctx_set_hdr_output`, `ponko_ctx_hdr_output`, `ponko_ctx_hdr_status`;
  C++: `game::hdr_output`, `ctx::hdr_output`, `ctx::set_hdr_output`,
  `ctx::hdr_status`. The guide's "HDR displays" chapter has the platform
  matrix and what to check on a real HDR display.
- `ShadowFilter::ContactHardening` and `Lighting::shadow_sun_size`:
  contact-hardening sun shadows (as "PCSS" elsewhere), sharp where a shadow
  meets its caster and softer the further it falls, as wide as the sun's
  disc (0.53 degrees by default, the real sun) seen from the caster. Each lit
  pixel searches for casters (16 reads), then filters across the width their
  distance gives (16 lookups). The width is in world units, so an edge keeps
  it where two cascades meet; it works with cascade caching, the held sun
  direction, cut-out casters and temporal antialiasing (the lookup pattern
  turns every frame only with TAA on). Off by default; the C API takes it as
  `shadow_filter` 4 (`shadow_sun_size` is Rust-only for now). On the GL
  backend (WebGL2) it draws as `Softest`, and `fit_quality_to` says so. About
  0.3 ms more GPU time than `Softest` at 1080p in the town benchmark scene
  on an Apple M4 Max. The benchmark's `PONKO_RENDER_SHADOW_FILTER` picks the
  filter for comparisons.
- `Lighting::shadow_lag` (off by default): the far sun cascades may lag
  behind their casters to draw fewer shadow passes. While a far cascade
  holds its box (`shadow_refresh` above 1) its map is kept whatever its
  casters do, so a far shadow trails a moving or animated caster by up to
  `shadow_refresh - 1` frames; near cascades and point and spot lights stay
  exact, and a changed mesh, material, shader or instance batch still
  redraws at once. In the benchmark town (moving and skinned casters in
  every cascade) it saves 0.14 to 0.26 ms of sun shadow GPU time a frame,
  6 to 10% of the GPU frame (`crates/ponko_bench/SHADOW_LAG.md`). Rust
  only for now; C and C++ far cascades are exact.
- The render scene benchmark takes `PONKO_RENDER_SHADOW_REFRESH` and
  `PONKO_RENDER_SHADOW_LAG`, and a `vegetation` shadow workload (slow
  camera, still actors, no skinned walkers).
- `camera::CameraTarget` (also in the prelude): put it on the entity a camera rig follows, keep the
  rig as a resource (`d2::FollowCamera`, `d3::ThirdPersonCamera`,
  `d3::FirstPersonCamera`, `d3::OrbitCamera`), and the new built-in system
  `systems::update_camera_rigs` (`Systems::UPDATE_CAMERA_RIGS`, at the end of
  `Phase::Animate`) updates the rig every frame. It cuts to a new target, leaves
  the camera alone without one, and the third-person rig avoids the 3D physics'
  walls but not the target's own collider. Saved with the world.
- `d2::FollowCamera::pixel_perfect` and `with_pixel_perfect`: the rig keeps the
  camera on a pixel-art grid itself.
- `d3::FirstPersonCamera::height`: the eye's height above the followed entity.
- First-person held items: `SceneSettings::view_model` (`d3::ViewModel`) and the
  `d3::HeldItem` component draw held models with their own camera and the depth
  range nearest the eye, so they never clip into walls. Off by default, and
  then free: no allocation, pass or draw (tested).
- C API: `ponko_first_person_camera_flat_forward`, `_flat_right`,
  `ponko_third_person_camera_flat_forward` and `_flat_right`.
- 3D shaders can read whole and fractional elapsed seconds separately or use
  `ponko_elapsed_time_wrapped` for smooth whole-second cycles after days of
  play. Existing `frame.time` values retain their meaning and precision.
- C and C++ expose double-precision normalized time for 3D animators and
  2D skeletons. Existing float accessors retain their signatures.
- `ShadowOnly`: a model with it is drawn only into the shadow maps, so no
  camera sees it while its shadow still falls (a first-person character's
  body, a caster that stands in for others). Plain and skinned models.
- `Graphics::set_texture_mip_limit(levels)` and `texture_mip_limit()`: 3D
  textures loaded afterwards (model images and texture arrays, RGBA and
  block-compressed) drop their `levels` largest mip levels before upload,
  as Unity's texture mipmap limit does. Each level quarters their video
  memory. Textures keep at least 32 pixels a side; sprites, UI textures and
  render targets never change. For GPUs with little video memory, where a
  world that doesn't fit spills into system memory and every frame waits
  on it (measured: a large world on a 4 GB card went from about 130 ms to
  about 17 ms of GPU time a frame at one level).

**Animation**
- `Graphics::reset_motion(entity)` (C `ponko_ctx_reset_motion`, C++
  `ctx.reset_motion`) gives one model no motion on the next drawn frame and
  keeps the rest of the temporal history: for a teleport or an animation
  that snaps to another pose with no blend.
- The rendering baseline takes `PONKO_RENDER_TAA=1` to draw its scenes with
  TAA (and so with motion vectors), which no quality preset turns on.
- `Graphics::set_gpu_buffer_motion(buffer, keep)` (C `ponko_ctx_set_gpu_buffer_motion`,
  C++ `ctx.set_gpu_buffer_motion`): a GPU buffer keeps a copy of its last frame,
  so temporal antialiasing and temporal AO follow each element's own motion
  (particles moved by a compute pass). It doubles the buffer's memory and copies
  it once a frame on the GPU while motion vectors are drawn.
- Material shaders: `frame.time.zw` is the last drawn frame's time, and
  `drawing_last_frame()` is true while the motion pass runs `vertex` as of the
  last frame, for shaders that animate with their own values.
- Rendering baseline: `PONKO_RENDER_WIND=1` sways the `game-like` scene's trees in
  a `vertex` function and adds a kept GPU buffer; `PONKO_RENDER_VERTEX_MOTION=0`
  is its control.
- Humanoid retargeting (`ponko::anim`): `HumanoidMap::detect` maps a
  skeleton's joints to `HumanBone`s from common naming schemes (editable
  with `set`), and `Retargeter` turns a clip made for one humanoid into an
  ordinary clip for another with other proportions, joint names and axes,
  so it plays in blend trees, layers and with root motion. Hips and root
  motion scale with leg length, and foot IK keeps planted feet planted at
  the target's ankle height (under 1 cm of slide in the test walk, about
  0.02 mm measured). `retarget_pose` follows a live pose every frame
  without allocating. Rust only for now; see the guide's "Retargeting
  humanoid animations" for what is and isn't handled.
- `LodFadeTime`: a `LodGroup` with it fades every change of level over that many seconds, on each camera's frame clock, instead of over a band of screen heights; fades turn back with the camera and keep `LodHysteresis`. In scene files `"LodFadeTime": 0.5`; C `ponko_ctx_set_lod_fade_time` / `ponko_ctx_lod_fade_time`, C++ `set_lod_fade_time` / `lod_fade_time`.
- Unity importer: humanoid clips import. Their muscle and body curves become bone rotations and hips movement for the skeleton they're exported onto, through a `HumanoidAvatar`: the one the character's Animator or the mesh's model names (its `.meta` `humanDescription`, with changed limits and reference pose), one given with `Importer::humanoid_avatar`, or one found by the usual bone names (`HumanoidAvatar::detect`). `AnimationClip::append_to_skin_with_avatar` and `is_humanoid` do the same for skins of your own.
- Fallible animation-data constructors for content from files and tools:
  `AnimationTrack::try_new`, `Skeleton::try_new`, `AnimationClip::try_new`
  and `AnimationClip::try_with_morph_tracks` return `ponko::Error`
  (`ErrorKind::Invalid`, or `ErrorKind::Limit` for more than 65536 joints)
  where the existing constructors panic. The panicking constructors stay and
  call them.
- Checked animator calls for ids from outside the code that built the graph
  (save files, scripts, the network): `Animator::get_layer_mut` returns an
  `Option`; `try_set_float`, `try_set_bool`, `try_set_trigger`, `try_play`
  and `try_crossfade` return an `AnimationGraphError` and change nothing when
  they refuse. `AnimationGraphError` has three new variants for them:
  `UnknownState`, `NonFiniteValue` and `InvalidPlayback`. The fast methods
  stay and document that they panic on a bad id.
- Builders for the extensible structs: `AnimationState::with_speed`,
  `with_speed_parameter`, `with_looping`, `with_event`, `with_events`;
  `MotionChild::new` and `with_time_scale`; `AnimationEvent::new`;
  `Joint::new` and `with_rest`; `JointAnimation::new`, `with_translation`,
  `with_rotation`, `with_scale`; `MorphAnimation::new`; `MorphTarget::new`
  and `with_normals`; `AttachToJoint::new` and `with_offset`;
  `Motion::blend_1d` and `Motion::blend_2d`.
- The guide's 3D chapter has an "Animation data from outside your code"
  section.
- 2D skeletons: path constraints (`PathConstraint` on a `PathShape`
  attachment, with every position, spacing and rotate mode) place a chain
  of bones along a bezier path, and physics constraints
  (`PhysicsConstraint`) make a bone lag, sway and spring back, stepped at
  their own fixed rate with springs kept per `Skeleton`
  (`Skeleton::reset_physics` starts them over). Both are imported from
  Spine, mirrored to y-down.
- 2D skeletons: clips key constraint mixes and settings with `IkTrack`,
  `TransformTrack`, `PathTrack` and `PhysicsTrack`, read from Spine's IK,
  transform, path and physics timelines.
- `SkeletonData::to_json` writes a skeleton in the engine's own format.
  That format now also stores shear, transform, path and physics
  constraints (in their order), clipping masks, paths, deform keys,
  one-axis and shear bone keys and every constraint timeline, so an
  imported Spine export kept in it reads back bit for bit and plays the
  same.
- First-person held items (`d3::ViewModel`, `d3::HeldItem`) write motion
  vectors of their own under temporal antialiasing: an item held still stays
  put on screen while the eye turns and walks, one that sways or recoils
  moves as far as it does, instead of smearing with the world behind it.
- Cut-out held items (`Blend::Cutout`) are drawn by the view model, their
  holes cut, with and without multisampling (a pipeline built the first time
  one is drawn).
- The cookbook's "Follow the player" recipe puts `CameraTarget` on the
  player and keeps the rig as a resource: no camera system.
- 2D skeletal animation, `ponko::skeleton2d`: bone hierarchies with rest
  poses, rigid sprite parts on bones and skinned 2D meshes (up to four
  bones per vertex) drawn by the sprite renderer in the same batches and
  `z` order as sprites; clips keying bone translate/rotate/scale, slot
  attachments (sprite swaps), visibility, color, draw order and events;
  looping, crossfades, 1D blends and a state machine; skins; two-bone IK
  and look-at, as constraints or one-off calls. Free when unused.
- Skeleton files: a documented JSON format (`"format":
  "ponko-skeleton2d"`) and Spine JSON import (4.x and 3.x), loaded through
  the asset system with `Ctx::load_skeleton2d` / `try_load_skeleton2d`,
  with hot reload that keeps each skeleton's clip, clock, parameters and
  skin.
- Bezier curves on clip keys (`Curve::Bezier`, `Curve::Channels`,
  `Bezier`; `{ "bezier": [x1, y1, x2, y2] }` in the file), evaluated
  exactly. Spine's bezier keys import exactly as authored.
- Spine texture atlases (`SpineAtlas`, `SkeletonData::from_spine_json_with_atlas`):
  `load_skeleton2d` reads the `.atlas` beside a Spine export, with regions
  packed rotated or trimmed, premultiplied pages and nearest filtering;
  editing the atlas reloads the skeletons using it.
- Every Spine feature the importer doesn't read is a warning naming the
  feature, where it is and the file (`SkeletonData::warnings`, printed by
  `load_skeleton2d`), never a silently wrong pose.
- Checked state machine calls on `Skeleton`: `try_set_float`,
  `try_set_bool`, `try_set_trigger`, `try_crossfade_state` and
  `try_play_state` return an `AnimationGraphError` instead of panicking,
  sharing the 3D animator's checks.
- C and C++ API for 2D skeletons: `ponko_ctx_load_skeleton2d`,
  `ponko_ctx_set_skeleton2d` and the `ponko_ctx_skeleton2d_*` calls (play,
  crossfade, states, checked parameters, events, IK targets, bones, slots,
  skins, look, manual updates), `PONKO_SYSTEM_UPDATE_SKELETONS2D`, and the
  matching `ponko::ctx` methods in `ponko.hpp`.
- The `Skeleton` component is reflected as `Skeleton2D`, so saves, scenes,
  prefabs and the inspector keep an animated skeleton; loading a scene
  loads the skeleton files it names.
- The built-in system `update_skeletons2d` (`Systems::UPDATE_SKELETONS2D`)
  at the end of `Phase::Animate`.
- The state machine types in `ponko::anim` (`StateId`, `ParamId`,
  `AnimationParameter`, `AnimationCondition`, `AnimationTransition`,
  `AnimationEvent`, `AnimationGraphError`) are available without the `3d`
  feature; the 3D animator and 2D skeletons share them.
- The `skeleton2d` example, the guide chapter "2D skeletons", and the
  `skeleton2d/1k` benchmarks in `ponko_bench`.
- Clip rotation keys go straight from one value to the next (350° to 10°
  turns back 340°), as Spine 4 does; Spine 3.x imports keep 3.x's
  shorter-way turns.
- 2D skeletons draw on all cores: skeletons are split into runs, each
  thread builds its run's vertices into its own chunk, and the chunks are
  copied into the draw list in draw order, so the output matches drawing
  them one by one (a test compares the two across thread counts). Drawing
  10,000 skeletons took 5.6 to 8.4 ms at best instead of 25.6 to 26.6 ms;
  these numbers come from a busy machine (medians within the noise) and are
  to be repeated on a quiet one: see the guide's "2D skeletons", "Cost".
  Posing, drawing and clipping allocate nothing per frame once warm.
- `ponko::skeleton2d` shear (`BonePose::shear`, `BonePose::with_shear`,
  `BoneTrack::shear`), one-axis bone timelines (`ChannelTrack`,
  `BoneChannel`), mesh deform keys (`DeformTrack`, `Clip::with_deform_track`),
  transform constraints (`TransformConstraint`,
  `SkeletonData::add_transform_constraint`, in Spine's four modes, applied
  in order with IK constraints) and clipping masks (`ClippingMask`, an
  `Attachment::Clipping` that clips the slots after it up to an end slot,
  concave polygons included).
- Spine JSON import reads them: bone shear, the 4.x one-axis timelines
  (`translatex` ... `sheary`), shear timelines, deform timelines (4.x
  `attachments`, 3.x `deform`, weighted meshes included), transform
  constraints (4.x and 3.x mix names) and clipping attachments. Path and
  physics constraints still warn, now with the reason they are left out.
- C and C++: one-off `ponko_ctx_skeleton2d_look_at` /
  `ponko_ctx_skeleton2d_two_bone_ik` (`Ctx::skeleton2d_look_at`,
  `Ctx::skeleton2d_two_bone_ik`).
- Benchmarks `skeleton2d/update-1k`, `draw-1k`, `update-10k` and
  `draw-10k` time posing and drawing on their own.

**Assets, scenes and saves**
- `ctx.storage` (`ponko::storage::Storage`): the player's own files in the
  per-user data folder (Windows `%APPDATA%`, macOS `~/Library/Application
  Support`, Linux `$XDG_DATA_HOME` or `~/.local/share`), named by
  `Game::app_id`; `Game::data_dir` and `PONKO_DATA_DIR` move it. `write`
  is crash-safe (temporary file, flushed, previous file kept as
  `<name>.bak`, then a rename); `read`, `read_backup`, `exists`, `list`,
  `delete`, `write_json`, `read_json`.
- Web builds keep the same files in an IndexedDB database, read into memory
  before setup and written through in one transaction per write, so the same
  game code runs in the browser; `is_persistent` and `is_saving` say what
  the browser does with them.
- Save slots: `ctx.storage.save_world(&ctx.world, slot)`, `world_slots`,
  `delete_world` and `ctx.load_world(slot)`.
- C and C++: `ponko_game_app_id`, `ponko_game_data_dir`,
  `ponko_ctx_storage_*`, `ponko_ctx_save_world`, `ponko_ctx_load_world`, and
  the `ponko::game` / `ponko::ctx` methods of the same names.
- The guide's "Saves and settings" chapter.
- The web smoke runner can reload the page and check what the game kept
  (`--after-reload`, fixture field `afterReload`).
- Prefabs can hold children: a version-2 prefab file lists a tree of
  entities, root first, with ids its components use to name each other.
  `Ctx::spawn_prefab` spawns the tree relative to the root, returns the
  root, and remaps every `Entity` value that names an entity of the prefab
  (attachments, joints, skin targets, game components) to the new copies.
  Version-1 files still load and are still written for single entities.
- `PrefabParts` (reflected, in the prelude) lists the parts a prefab root
  was spawned with; `Ctx::despawn_prefab` removes a root with its parts.
- A prefab entity may include another prefab (`"prefab": "<file>"`), with
  its `"components"` changing the included root; loops are refused.
- `Assets::save_prefab` saves an entity's prefab parts and what is attached
  to them (`Attached`, `SkinOf`, `AttachToJoint`, 3D joints between two of
  them), with transforms relative to the root.
- `Entity::with_remap` and `Entity::remapped` in `ponko_ecs`: read saved
  entity ids as other ones while deserializing.
- Control endpoint `prefab.despawn` and MCP tool `despawn_prefab`;
  `prefab.spawn` / `spawn_prefab` also return the root's `parts`.
- C: `ponko_ctx_prefab_parts` and `ponko_ctx_despawn_prefab`; C++:
  `ctx.prefab_parts(root)` and `ctx.despawn_prefab(root)`.
- The `prefabs` example (carts with included wheels), runnable headless.
- Save migrations for a game's own components: `ponko::reflect::SaveMigrations`
  is the game's save version, written into every world save as
  `"game_version"`, plus a `Migration` step from each older version. A step
  renames, removes, converts, splits or merges components; renames, adds,
  removes or converts fields; renames, removes or converts reflected
  resources; or changes each entity or the whole save. Every load
  (`load_world`, `load_scene`, `World::load`, the editor, the control
  endpoint) runs the steps from the save's version up, in a chain, before it
  checks anything. A save from a newer game version is refused with
  `ReflectError::NewerSave` (`ErrorKind::Unsupported`), and a failing step
  is `ReflectError::Migration`, naming the versions and the entity.
  `SaveMigrations::migrate` returns a migrated save without loading it.
- `Ctx::check_old_saves(folder)` loads every old save in a folder through
  the migrations and loads it again after saving, for a game's tests.
- C and C++: `ponko_world_set_save_version`, `ponko_save_migration_*` (C
  conversions take a `PonkoJsonConvertFn` with a user pointer and free
  function), `ponko_world_migrate_save` and `ponko_ctx_check_old_saves`;
  C++ `world::set_save_version`, `world::migration(from)` (a
  `save_migration` builder) and `ctx::check_old_saves`. `ponko_world_load`
  reports a save from a newer game with the error kind
  `PONKO_ERROR_UNSUPPORTED`.
- Guide: "Save migrations" in the saves chapter.
- Image-format cargo features that no longer depend on `3d`: `image-jpeg`
  (on by default), and opt-in `image-webp`, `image-bmp`, `image-tga`,
  `image-gif`, `image-tiff`, `image-ico`, `image-qoi`, `image-pnm`,
  `image-dds`, `image-hdr`, `image-exr`, and `image-formats` for all of them.
  `3d` turns on `image-jpeg`, `image-hdr` and `image-exr`.
- Levels from the Tiled map editor (`.tmx`, `.tmj`, external `.tsx` /
  `.tsj` tilesets, `.tx` templates, groups, infinite maps, CSV, base64,
  zlib and gzip data) and LDtk (`.ldtk` projects, worlds, separate level
  files, tile, auto and IntGrid layers, entities and their fields):
  `Ctx::spawn_level` / `spawn_level_with` spawn every layer as tilemaps,
  every object as an entity with a `LevelObject` (or a prefab instance,
  `LevelOptions::prefab`), custom properties included (`Properties`), and
  with `physics` fixed colliders for the solid tiles. `Ctx::despawn_level`,
  `Assets::load_level` (the level as plain data, `Level`). Saving the level
  (or a tileset or template) with hot reload on spawns it again in place.
  C: `ponko_ctx_spawn_level`, `ponko_level_*`; C++: `ponko::spawn_level`,
  `level_options`, `level_entities`, `level_object_of`.
- Animated tiles (`TileMap::animate`), flipped and rotated tiles
  (`TileMap::set_flipped`, `FLIP_X`, `FLIP_Y`, `FLIP_DIAGONAL`) and parallax
  layers (`TileMap::parallax`, `parallax_origin`). Maps without them draw
  as before. C: `ponko_tilemap_animate`, `ponko_tilemap_set_flipped`,
  `ponko_tilemap_set_parallax`; C++: `ponko::tilemap_animate`,
  `tilemap_set_flipped`, `tilemap_set_parallax`.
- Autotiling for maps built in code: `AutoTile` rules with 4-neighbour (16
  frames) or 8-neighbour (47 "blob" frames, or any mapping) bitmasks,
  `TileMap::autotile` and `TileMap::paint_autotile`. C:
  `ponko_tilemap_autotile`, `ponko_tilemap_paint_autotile`; C++:
  `ponko::autotile_rule`, `tilemap_autotile`, `tilemap_paint_autotile`.
- Tile colliders: `TileMap::solid_rects` merges solid cells into few
  rectangles, and with `physics` `TileMap::colliders` makes a fixed body for
  each. C: `ponko_tilemap_solid_rects`, `ponko_tilemap_spawn_colliders`;
  C++: `ponko::tilemap_solid_rects`, `tilemap_spawn_colliders`.
- The `level` example (a Tiled level with parallax clouds, animated water,
  coins and walls built with an autotile rule), sample level files in
  `assets/levels/`, and the benchmarks `tilemap/draw-1m` and
  `tilemap/draw-1m-animated`.
- New dependencies: `quick-xml` (reads Tiled's XML files) and `flate2`
  (zlib and gzip tile data; already in every build through PNG decoding).
- glTF extensions: `KHR_lights_punctual`, `KHR_texture_transform`,
  `KHR_materials_emissive_strength`, `KHR_mesh_quantization`,
  `KHR_materials_transmission` (drawn as see-through blending, without
  refraction), `KHR_texture_basisu` (with `basis-universal`) and
  `EXT_texture_webp` (with `image-webp`). The guide's "glTF extensions,
  lights and cameras" lists them.
- Compressed glTF geometry (feature `gltf-meshopt`):
  `EXT_meshopt_compression` and `KHR_meshopt_compression` buffer views are
  decompressed at load with meshoptimizer's decoder (C++; not in web
  builds, where such a file loads only from its uncompressed fallback).
- `Graphics::load_lights` and `load_cameras` (and their `try_` forms):
  the lights and cameras a glTF file places, as `render::ModelLight` and
  `render::ModelCamera`, with `ModelLight::spawn` and `ModelCamera::apply`
  to put them where the model is. Light intensities follow Blender's
  exporter: candela over 683, lux as it is.
- Asset packs: many asset files in one archive with an index, each file
  stored or deflated (when that saves a tenth) with its CRC-32, identical
  files stored once, names sorted so the same files give the same bytes.
  `PackSource` (`ponko::assets`, and the prelude) reads one as an asset
  source: `PackSource::open(path)` reads only the index and then a file at a
  time, `PackSource::from_bytes(id, bytes)` takes one in memory; a damaged
  file is an I/O error naming it.
- A game reads `assets.pack` where its `assets/` folder would be (next to the
  program, in a macOS app's `Resources`, next to the web page) when a name
  isn't a loose file there, with no code; loose files win, so development
  keeps hot reload. A web page names its pack on the canvas
  (`data-pack="assets.pack"`) and the engine fetches it before setup with at
  most two HTTP range requests (the head with the index, then the rest), or
  whole from a server without ranges.
- `ponko pack` (`ponko_cli`): packs a folder (`--out`, `--exclude <pattern>`,
  `--no-compress`), converts images to KTX2 into the pack per `textures.json`
  with `--textures`, and lists and checks a pack with `--list`.
- `web/smoke/first-load.mjs`: the requests, bytes, asset phase and first frame
  of web builds in headless Chromium on an emulated network. On Ember Hop's
  web package the pack takes the asset requests from 27 to 2 and the asset
  phase from 292 to 167 ms (40 ms, 20 Mbit/s) and from 1055 to 591 ms
  (150 ms, 5 Mbit/s).
- The inspector edits 3D scenes: a click picks the model under the mouse
  (along the camera's ray, by mesh bounds), and a move / rotate / scale
  gizmo works in world or local space with snapping to a grid, angle and
  scale steps. Right-drag with W A S D Q E flies the camera, F frames the
  selection, W / E / R switch the gizmo. Shift- or Ctrl-click builds a
  multi-selection that moves, duplicates and deletes as one undo step.
- A hierarchy panel in the inspector shows what each entity hangs off
  (attachments, joints, skins, prefab parts); dragging one row onto another
  parents it without moving it, and loops are refused.
- An asset browser in the inspector lists glTF meshes, prefabs, scenes and
  loaded materials from `assets/`: drag a mesh or prefab into the world to
  place it, a material onto a model to paint it, or open a scene.
- Redo in the editor (Ctrl+Shift+Z or Ctrl+Y, and a button); undo and redo
  cover moves, gizmo drags, component edits, spawns, despawns, parenting
  and placed assets, and survive Play / Stop. **Save** writes back to the
  scene loaded or saved last.
- `attach::Attached3D`: keeps an entity at a transform in another
  one's space, placed by `systems::follow_attachments` with the 2D ones,
  saved in scenes and prefabs. C: `PonkoAttached3D` and
  `ponko_attached3d_component`; C++: `ponko::attached3d` and
  `ponko::attach3d_to`.
- Control endpoint `editor.*` methods and MCP tools for every editor action
  (`pick_entity`, `transform_entities`, `drag_gizmo`, `set_parent`,
  `scene_hierarchy`, `list_assets`, `place_asset`, `paint_material`,
  `delete_entities`, `duplicate_entities`, `undo_edit`, `redo_edit`,
  `save_edited_scene`, `set_camera_3d`), sharing the inspector's undo
  history, so an agent can assemble and save a level.
- Binary world saves: `ponko::reflect::SaveFormat::Binary` keeps a world
  save as MessagePack (`rmp-serde`) compressed in an LZ4 frame (`lz4_flex`)
  with a checksum: the same save as the JSON, about 15 times smaller at a
  million entities, and quicker to write and read. `SaveFormat::encode`,
  `decode` (either format, told apart by the first bytes) and `detect`.
- `Storage::set_save_format`: save slots in binary as `saves/<slot>.save`.
  `Ctx::load_world`, `world_slots`, `delete_world` and
  `Ctx::check_old_saves` read both formats, so JSON slots and saves from
  before keep loading, migrations included.
- `Storage::save_world_in_background`: the world's save is taken on the
  game's thread and encoded and written on a job, in order; `finish_saves`
  waits for them, and the game does when it ends. `is_saving` is `true`
  until they are written.
- A retained binary save fixture (`tests/fixtures/saves/world_v1.save`) and
  the save benchmark `cargo run --release -p ponko_bench --bin saves`.
- `Animation` (sprite-sheet animation) is reflected as `"Animation"`: its
  sheet's image by file name, frame size and grid, frames, fps, looping and
  time. Prefabs and scenes can give a sprite its animation (a file needs
  only `texture`, `frame_size` and `fps`), and world saves keep each
  animation's frame. Loading a scene or prefab loads its image first.
- `ponko::reflect::NotSaved`: entities with it are left out of
  `WorldReflect::save` and everything built on it (save slots, scenes, the
  editor, the control endpoint's `world.save`).
- `LevelOptions::without_saved_objects` spawns a level without its prefab
  objects, for spawning it again around a loaded save. C:
  `ponko_ctx_spawn_level_without_saved_objects`, `ponko_world_set_saved`,
  `ponko_world_is_saved`; C++: `level_options::without_saved_objects`,
  `world.set_saved`, `world.saved`.
- `ponko cook` reads Aseprite files (`.aseprite`, `.ase`) itself, with no
  export step: `hero.aseprite` becomes `hero.json` and `hero.png`, the JSON
  Array export `load_aseprite` reads (frames composited with layer and cel
  opacity and blend modes, tags, slices; `"layers": true` in `cook.json`
  exports each layer). An export already beside it is replaced.
- `ponko cook` compresses the image files a glTF model reads for their use,
  whether the model is cooked or kept as it is: base color BC7 sRGB, normal
  map BC5, metallic-roughness BC7 with the occlusion merged in. The merged
  file carries a `ponkoOrm` KTX2 key naming its two sources, and the engine
  uploads it as it is, so an uncooked model's occlusion stays compressed
  (a 32 x 32 test model: 16,380 -> 4,176 bytes of video memory).
- `ponko package`: `files.to` in `[package.metadata.ponko]` can name a
  subfolder of the package (`to = "docs/manual.pdf"`).
- `ctx.assets.restart_group(id)` loads the rest of a failed or cancelled
  asset group, reading again only the assets that had not loaded: the one
  that failed and the ones never started. What loaded keeps its handle,
  untaken values stay, and the files a failed group was already reading
  finish on their workers and are taken as they are. C:
  `ponko_ctx_restart_asset_group`; C++: `ctx.restart_asset_group(id)`.
- Asset groups measure what their GPU uploads cost on the game thread:
  `GroupStats::upload_ms`, `max_upload_ms`, `total_upload_ms` and
  `total_upload_bytes`, and per group `GroupProgress::upload_ms` and
  `bytes_uploaded` (also in `PonkoAssetGroupStats` and
  `PonkoAssetGroupProgress`).
- Where there are no worker threads (a web build without threads, or a page
  that doesn't allow them), an asset group decodes a large PNG texture a few
  rows at a time and a sound a few packets at a time, checking its frame
  budget between slices: one large file is spread over several frames
  instead of taking one long frame, with the same pixels and samples as the
  one-piece decode.
- The guide's "Loading screens" chapter: "Trying again", "What uploads
  cost", and the sliced decodes under "In a browser".
- Video in the browser: in the web build `ponko::video::Video` plays the
  same WebM files through the same calls, its pictures decoded by the
  browser's own AV1 decoder (WebCodecs) and its sound through the mixer.
  Frames show once the browser has decoded them, usually by the next
  update. A headless Chromium runner (`web/smoke/video.mjs`, fixture
  `examples/web_video.rs`) checks every frame shown against its time and
  two seeks while paused.
- `ponko_bench`: `video/convert`, converting a decoded frame to RGBA on
  the CPU (what `Video::frame_rgba` does when asked).
- `Ctx::try_load_collision_mesh` and `try_load_collision_mesh_with` return the error for a missing or broken file, as `try_load_mesh` does.
- `Assets::try_load_resource` and `try_load_strings` are the fallible twins of `load_resource` and `load_strings`.
- C++: `ctx.try_load_strings`, `ctx.try_load_collision_mesh` and `ctx.try_load_json<T>` return a `ponko::result` instead of throwing, and `ponko::result<void>` exists for loaders that return nothing. C already returns a status with `ponko_last_error_kind()` and the path for these calls.
- A table of every `load_*` and what it does on failure in STABILITY.md and the data-files chapter, and a test (`tests/loader_convention.rs`) that walks the API snapshots and fails when a new `load_*` has no `try_` twin (or is a `Result`-returning plain name that is not a documented runtime operation), or is missing from the table.
- Aseprite source import composites isolated groups with their authored opacity
  and blend mode, and draws tilemap layers with embedded tilesets (flipped and
  rotated tiles, linked cels).
- The optional `aseprite-source` feature loads loose `.aseprite` and `.ase` files
  with hot reload. A source path also resolves to its cooked JSON export, so the
  same loading code works in cooked builds without the feature; without either,
  the `NotFound` error says how to get one.
- `d3::Shape::ScaledMesh` and `d3::Collider::scaled_mesh(mesh, scale)`: a
  convex hull, triangle mesh or heightfield scaled where it is used, so one
  collision mesh serves colliders of every size; a negative component
  mirrors a hull or triangle mesh. Colliders at one scale share one scaled
  copy, dropped with the last of them. Penetration queries and debug
  drawing take the scale; world saves leave such colliders out. The C API
  reads and writes it as `PONKO_SHAPE3D_SCALED_MESH`, with `size` as the
  scale (no C++ helper yet).
- `AssetGroup::with_sliced` and the `SliceLoader` trait: a game's own loader
  that does a bounded piece of work per call. Where there are no worker
  threads (a browser build without threads) the group step calls it within
  its frame budget, so a large file is spread over several frames; with
  workers it runs on a worker.
- Where there are no worker threads, `stream_texture` and `stream_mesh`
  decode a slice at a time in the streaming step, within what the asset
  group step left of its frame budget: a PNG a few rows a slice, then its
  smaller resolutions; a glTF file's PNG images that way, then its geometry.
  `UploadStats::frame_slices` counts the slices of the last frame.
- The GPU's own time copying uploaded textures, where it has timestamp
  queries: `GroupStats::gpu_upload_ms` and `total_gpu_upload_ms`,
  `UploadStats::gpu_upload_ms`, and `gpu_upload_timing` on both, `false`
  headless and on GPUs and browsers without timestamp queries.
- `GroupStats::pieces`: the pieces of work (reads, decodes or their slices,
  uploads) the last group step ran.
- Aseprite tilesets that link to a tileset in another file (external
  tilesets) are read from that file through the asset sources, by the cook
  and by `aseprite-source` loading, instead of being refused. Links resolve
  relative to the file, may chain up to 8 files deep, and the linked file
  is watched for hot reload and part of the cook's cache key.
  `ponko_aseprite::decode_linked` and `ponko_aseprite::linked_files` are
  the decoder's side of it.
- Editor: selecting a glTF scene's root or node shows the file it came from
  and every override of the scene (changed, added and removed components,
  other parents, deleted nodes), each with a **Revert** button, and
  **Revert all**, as the prefab panel lists a prefab instance's.
- glTF scenes keep nodes deleted in the editor deleted, and nodes attached
  to another parent (another node, the scene's root, an entity the save
  keeps, or none) on it, through a re-export and a reopen. A scene's saved
  record takes `"deleted": true` and `"parent"` (`"root"`,
  `{ "node": path }` or `{ "entity": id }`) beside a node's
  `"components"` and `"remove"`; older records read as before.
- Editor: Play then Stop keeps glTF scenes: Stop spawns them again from
  their files with how they were edited before Play, and the undo history,
  the edits kept from play and the selection point at their new nodes.
- `ponko_unity::Error` can be matched on: `kind()` returns an
  `ErrorKind` (`Io`, `Decode`, `Unsupported`, `Invalid`, `Other`;
  non-exhaustive), `path()` the file, `line()` the line of a text file and
  `message()` the cause alone. The `std::io::Error` of a failed read or
  write is kept as the error's `source()`. Every error the importer makes
  has a kind. The guide's Unity chapter lists what each kind means.
- The editor's web build saves: **Save**, **Save as** and **Save selection
  as prefab** keep the file in the page's `localStorage` (named after the
  page's path and the asset), it is read from there at once, and the next
  visit puts it back over the fetched files before setup. **File >
  Download** saves the document and hands its file over as a browser
  download.
- On the web, the editor's **Open** list and asset panel are filled from
  the build's file manifest: the files `assets/index.json` names, the
  page's asset pack, and the files saved in the browser.
- `UploadStats::gpu_created_ms`: where the GPU has timestamp queries, its
  own time copying the textures made outside the asset group and streaming
  steps (`create_texture`, `create_compressed_texture`), a frame's worth at
  a time.
- Editor: a node attached to the root or a node of another glTF scene
  whose file didn't load (when the scene was opened) is listed among its
  scene's overrides as attached to that parent, "which is missing", while
  it waits on the file's parent. Its **Revert** forgets the link (one undo
  step; undo keeps it again, and it is made as soon as that scene spawns).
- Saves with a node attached to another glTF scene stay readable by
  builds from before such links: they skip the link's field and leave the
  node on its file parent.
- Opening a scene in the editor whose glTF scenes fail to load (a file
  missing or broken) names each file and why in the status bar's error
  line, not only on the console; the scene opens without them.
- The editor's web build asks the browser for persistent storage before
  setup. When the browser refuses, the status bar says it may clear the
  saves kept in the browser, and **File > Saved in this browser** repeats
  it. `Assets::browser_saves_persistent` is the browser's answer from code
  (`None` until it answers, and in other builds).
- The browser smoke runner takes several reload steps (`"reloads"` in
  `fixtures.json`, `--reload` on the command line), each a page load of
  its own with its own expected fields. The web editor's save fixture
  saves, reloads and reverts, then reloads again to check the revert held.
- The editor lists the glTF scenes of the open scene that didn't load in a
  panel above the status bar, one row each with **Retry**, the file and
  why. A retry that loads it spawns the scene where it was saved, with its
  overrides, and hangs the nodes of other scenes saved on its nodes off
  them again; one that fails says why in its row.
- The browser smoke runner takes `--persist granted|refused` (it answers
  `navigator.storage.persist()` for the browser and counts the asks),
  `--click <x>,<y>=<field>:<value>`, and from `fixtures.json` a page
  `query` and `editMode` (a page opened in the editor). The new
  `web_editor_unloaded` fixture clicks **Retry** in the editor's panel.
- `ImageLimits` (longest side in pixels, most decoded bytes) is the one setting every image decoder reads:
  PNG, JPEG and the other `image-*` formats, KTX2, DDS, the pictures of glTF models, tilesets and sprite
  sheets, and the frames of a video. The defaults stay (16 384 pixels, 512 MiB); `Game::image_limits` or
  `Assets::set_image_limits` change them for the process, and `Assets::image_limits` reads them. A picture
  past them is refused with `ErrorKind::Limit` before its pixels are allocated, and the message names the
  limits in force. Lower them for pictures from players or the network, raise them for large source art.
- `Graphics::try_decode_texture(bytes, name, limits)` decodes one untrusted picture under limits of its own,
  without changing the game's.
- C: `ponko_game_image_limits`, `ponko_ctx_set_image_limits`, `ponko_ctx_image_limits` and
  `ponko_ctx_try_decode_texture`; C++: `game::image_limits`, `ctx::set_image_limits`, `ctx::image_limits` and
  `ctx::try_decode_texture`. The security chapter of the guide says when to change them.
- Asset packs have absolute caps on what they may unpack to, next to the deflate
  ratio check: `PackLimits` sets the size of one file (default 1 GiB, enforced
  while inflating, so a file that grows past it fails without holding more than
  the cap), the sum of the sizes a pack's index declares (16 GiB) and its number
  of files (1,000,000), the last two checked when the pack is opened. Errors
  name the file, the cap and how to raise it. Set them for the game with
  `Game::pack_limits` (covers `assets.pack`, mod packs and `PackSource::open`
  and `from_bytes`), or for one pack with `PackSource::open_with_limits` and
  `from_bytes_with_limits`.
- `ponko pack`, `ponko cook --pack` and `ponko package` refuse to write a pack
  the engine's default caps would refuse to read, unless told higher caps with
  `--max-entry-size`, `--max-pack-size` and `--max-entries`.
- An asset source can carry `ImageLimits` of its own: pictures read from it (a mod pack, a download cache, a
  folder) are decoded under them and everything else under the game's. `Assets::set_source_image_limits`,
  `clear_source_image_limits`, `source_image_limits`, `image_limit_sources`, and
  `add_source_with_image_limits` / `add_background_source_with_image_limits`. The limits travel with the decode
  job to worker threads (asset groups, streamed textures), and also cover a model's pictures, texture array
  layers, a level's tilesets, terrain weight maps and Aseprite sheets, by the source their file came from.
  The security chapter says how to give mods tight limits.
- A picture refused by the image limits says where they came from: `the game's limits`, `the limits of asset
  source "mods"`, or `limits given for this picture`.
- The limits in force are read only: the control endpoint method `assets.image_limits` (MCP tool
  `image_limits`) lists the game's and each source's, and the inspector's asset view shows them. No remote call
  changes them.
- C: `ponko_ctx_set_source_image_limits`, `ponko_ctx_clear_source_image_limits`,
  `ponko_ctx_source_image_limits`; C++: `ctx::set_source_image_limits`, `clear_source_image_limits`,
  `source_image_limits`.
- `ponko cook` turns a folder of source assets into runtime-ready files under
  the same names, so a game loads them with the same calls: images become
  KTX2 with mipmaps (BC7, BC5, BC4 on desktop and the web, Basis Universal
  UASTC with `--platform mobile`), glTF and GLB models one GLB with meshes
  reordered for the vertex cache, overdraw and vertex fetch and normals and
  texture coordinates quantized (`KHR_mesh_quantization`), PCM WAV sounds
  lossless FLAC, and WGSL shaders are validated as the engine composes them.
  Per-asset settings come from an optional `cook.json` (paths and
  patterns; `textures.json` kinds too). Results are cached by a SHA-256 of
  the input and its settings (`target/cook-cache`), so a second cook of
  unchanged files reads them back, and the same input always gives the same
  bytes. `ponko pack --cook` packs the cooked files. See the packaging
  chapter's "Cooking assets".
- `tests/fixtures/cook/`: a small source texture, sound and model and what
  `ponko cook` makes of them; `tests/cooked_assets.rs` loads both through the
  same calls and compares them.
- glTF files as scenes: `Ctx::spawn_prefab` of a `.glb` / `.gltf` file
  (or `GltfRoot::spawn`, which takes options) spawns an entity for every
  node of the file, attached to its parent, with its own mesh, materials and light, and
  with what its custom properties (`extras`, as Blender writes an object's
  custom properties) ask for: reflected components by name with JSON values
  (`ponko:Health`), colliders fitted to the node's mesh (`ponko:collider`:
  box, sphere, capsule, hull or trimesh, with `ponko:body`, `ponko:sensor`
  and collision layers), spawn points (`SpawnPoint`), prefab references,
  level-of-detail groups made of the node's children, render layers,
  navigation mesh inclusion and light shadows. `GltfRoot` finds nodes by
  path, `GltfNode` keeps every custom property for the game's own use, and
  `Ctx::despawn_prefab` removes the scene with the meshes made for it.
- Unknown property or component names, and values of the wrong kind, are
  errors naming the file, the node and the closest known names
  (`did you mean "ponko:collider"?`); nothing is spawned then.
- Hot reload of glTF scenes: exporting the file again updates every scene
  spawned from it in place, keeping the entity of each node whose path
  stays; new nodes spawn, removed ones despawn, a broken file keeps the old
  scene.
- `GltfScene::load` reads and checks a file node by node without
  spawning it; `GltfScene::nav_source` (feature `navmesh`) collects its
  walkable meshes for `NavMesh::bake`.
- `ponko schema` writes the JSON Schema of the custom properties a game
  reads (the convention and every reflected component), for editors to
  check properties before exporting; any game started with `Game::run`
  writes it when `PONKO_SCHEMA=<file>` is set, and
  `ponko::d3::gltf_properties_schema` returns it.
- C and C++: `ponko_ctx_spawn_gltf`, `ponko_ctx_despawn_gltf`,
  `ponko_gltf_node_entity`, `ponko_gltf_node_extras` and
  `ponko_spawn_point_name` (C++ `ponko::spawn_gltf` and friends).
- The guide's "From Blender to the game" chapter (properties, Blender's
  export settings, hot reload, saves) with the sample
  `assets/levels/courtyard.gltf`, and a section of the 3D tutorial.
- Aseprite JSON Array sprite sheets load as one texture atlas, with trimmed
  frames restored to the original canvas, optional split layers, named slices
  and variable-duration sprite animations from tags. Edits to the JSON or PNG
  reload live playback. C and C++ can load individual frames or play tags.
- LDtk world placement is available for free, GridVania and linear layouts,
  including projects with several worlds. Games can spawn levels at their
  authored offsets and map entity fields onto registered reflected components
  in Rust, C and C++. Failed level reloads keep the previous entities.
- Asset groups for loading screens: `AssetGroup` declares the textures,
  streamed textures, meshes, sounds and your own formats (`with`, any
  bytes-to-value loader) a level needs; `ctx.assets.start_group` loads them
  on worker threads with progress by count and by bytes
  (`group_progress`), a ready or failed state that names the failing path
  (`group_state`, `group_error`), and `cancel_group`. A per-frame step with
  a time budget (`set_group_frame_budget`, 4 ms by default) and the shared
  upload budget keeps frames short; `group_stats` reports its cost.
- `Game::loading_state(loading, group, then)`: a game state that loads a
  group and moves on once it is ready, cancelling it if left early.
- `ctx.assets.add_background_source` for asset sources that worker threads
  may read, and `AssetSource::size` so loading bars know totals up front
  (`MemorySource` and `PackSource` implement both).
- C and C++: `ponko_asset_group_*`, `ponko_ctx_*asset_group*`,
  `ponko_game_loading_state` and `ponko_ctx_add_background_memory_source`;
  `ponko::asset_group` and the matching `ctx` and `game` methods.
- A guide chapter, "Loading screens: asset groups with progress".
- Video playback (feature `video`, off by default): `ponko::video::Video`
  plays WebM files with AV1 pictures into a texture, to draw as an intro
  or cutscene or to put on a screen in the world, with play, pause, seek,
  looping and an end. The sound (Vorbis) plays through the mixer on a
  chosen bus with its own volume, starts and stops with the picture and
  seeks with it, decoded a packet at a time as it plays. Pictures are
  decoded on the CPU by rav1d (the Rust port of dav1d, without its
  assembly: pure Rust, nothing to install). Other codecs (VP9, H.264,
  Opus sound, 10-bit pictures) fail to load with an `Unsupported` error
  naming the ffmpeg option to use instead. Desktop only: the web build
  compiles the API and loading reports `Unsupported`; the guide's new
  "Video" chapter compares the decoders considered (licenses, patents,
  size, web) and gives the web recipe. Not bound for C and C++ yet.
- `ponko_bench`: `video/frame`, the time to decode and convert one frame
  of the clip named by `PONKO_BENCH_VIDEO` (feature `video`).
- `ponko cook` compresses the images a GLB model holds to KTX2
  (`KHR_texture_basisu`) for their use: base color BC7 sRGB, normal map
  BC5, metallic-roughness BC7 with the occlusion merged in (UASTC on
  `mobile`), with mipmaps.
- `ponko cook` stores a model's positions as 16-bit integers when none
  moves by more than 0.1 mm (meshes up to 13 m across; not skinned,
  instanced or morphed meshes). The scale lives on a child node marked
  `PONKO_quantized_positions`, which the engine folds back as it loads, so
  node trees, transforms, extras, colliders and navigation meshes read as
  in the source.
- `ponko cook` checks shaders with the engine's own hook rules (the hook's
  signature, no entry points and no bindings of its own), from the same
  rule table the engine uses at load.
- Saves link glTF prefab instances to their scene: a world save holds a
  record of each glTF scene (its file, where it was spawned and each
  prefab node's instance), and spawning the file again after loading the
  save adopts the saved instances as its prefab nodes, where the game left
  them, without spawning again the ones the game despawned. Of several
  scenes of one file, a spawn adopts the one saved nearest to it.
  `GltfOptions::without_saved_objects` is no longer needed to load a save
  (it still works).
- C and C++: `ponko_gltf_scene_load` (C++ `ponko::gltf_scene_json`) reads
  a glTF file without spawning it and writes its nodes and spawn points as
  JSON; `ponko_gltf_properties_schema` (C++ `ponko::gltf_properties_schema`)
  returns the schema of the custom properties the game reads.
- A Blender add-on, `tools/blender/ponko_properties`, that reads a game's
  `ponko-schema.json`, offers its `ponko:` properties with fitting
  starting values and shows what is wrong with each, with the engine's
  suggestions; its checks run without Blender
  (`python3 -m unittest discover -s tools/blender`). Guide: "From Blender
  to the game".
- `Graphics::video_memory()`: what the operating system says about the GPU's
  own memory for the game (`perf::VideoMemory`: the card's size, the budget
  the game may use before the system moves some of it to system memory, and
  what it uses now), for settings screens that warn when the chosen quality
  won't fit. Windows (DXGI, any backend) for now; `None` elsewhere.
- `Graphics::gpu_memory()`: the GPU memory the engine holds, by class, as
  the performance report counts it, for games to read directly.

**UI, text and input**
- `Input::pressed_this_frame()` lists the keys, mouse buttons, gamepad
  buttons and gamepad axes that went down this frame as `PressedInput`, which
  wraps the `ButtonBinding` or `AxisBinding` an `ActionMap` accepts, so a
  rebinding screen can bind the answer directly. Axes count when they cross
  `PRESS_THRESHOLD` (0.5) during the frame; `pressed_this_frame_with_threshold`
  takes another. C: `ponko_ctx_pressed_this_frame`; C++:
  `ctx::pressed_this_frame()` and `ponko::pressed_input`.
- `Game::headless_frames(true)`: headless runs (`run_headless`,
  `run_headless_scenario`, `profile_headless`) run whole frames, the retained
  UI and the frame, `Animate`, `Late` and `Draw` systems included, in a virtual
  window of the game's size, with no window or GPU. Off by default; a
  simulation-only headless run is unchanged. C: `ponko_game_headless_frames`;
  C++: `game::headless_frames`.
- Input scenarios act on the retained UI by node name: `InputScenario::click`
  and `type_text` (JSON `ui_click`, `ui_type`), and `ScenarioCheck::ui` checks
  that a node exists, is shown, has the focus or has a text (JSON
  `expect_ui`). With headless frames, a tick's input reaches the UI in the
  same frame and its checks run after the whole frame. C:
  `ponko_input_scenario_click`, `ponko_input_scenario_type_text`,
  `ponko_input_scenario_expect_ui`; C++: `input_scenario::click`, `type_text`,
  `scenario_check::ui`.
- `Ui::is_shown`: the node is on a screen of the stack and not hidden. C:
  `ponko_ctx_ui_is_shown`; C++: `ui::tree::is_shown`.
- `tests/coin_dash_menus.rs` plays the coin_dash menus headlessly, checking
  the UI at every step; the benchmarks `headless/sim-only-10k-ticks` and
  `headless/frames-10k-ticks`.
- Text shaping, the `text-shaping` cargo feature: strings that aren't plain
  ASCII are shaped with harfrust (the HarfBuzz project's Rust port, the
  shaper the inspector's egui already uses): Arabic joining, Devanagari
  conjuncts and vowel reordering, marks, ligatures and OpenType kerning;
  lines are ordered with the Unicode bidirectional algorithm, so
  right-to-left and mixed-direction text reads correctly; wrapped UI text
  breaks where Unicode's line-breaking rules allow (between Japanese
  characters too). It applies to `draw_text`, `measure_text`, retained and
  immediate UI, text fields (caret, clicks and selections follow the
  visual order) and 3D text. ASCII strings keep the per-character path,
  whose cost is unchanged (`text/layout-ascii-400`: 0.152 to 0.155 ms off,
  0.153 to 0.156 ms on, release, Apple M4 Max);
  `Graphics::set_text_shaping(TextShaping::Always)` shapes them too.
  Shaped strings are cached, so redrawing one costs a lookup per glyph.
  The feature adds about 1.3 MB to a window-only release build (desktop
  and web) and 0.5 MB to one with default features. On by default in the
  C library (`ponko_ffi` feature `text-shaping`).
- Font fallback chains: `Graphics::set_font_fallbacks(font, &[..])` and
  `font_fallbacks`; characters a font lacks come from the first font in its
  chain that has them. Works with or without shaping.
- System fonts, the `system-fonts` cargo feature (desktop):
  `Graphics::load_system_font` / `try_load_system_font` by family name, and
  `set_system_font_fallback(true)` to search installed fonts for characters
  no loaded font has. Without it, and on the web, they report
  `ErrorKind::Unsupported`.
- C and C++: `ponko_ctx_set_font_fallbacks`, `ponko_ctx_font_fallbacks`,
  `ponko_ctx_set_text_shaping`, `ponko_ctx_text_shaping`
  (`PonkoTextShaping`), `ponko_ctx_try_load_system_font` and
  `ponko_ctx_set_system_font_fallback`; in C++ `ctx::set_font_fallbacks`,
  `font_fallbacks`, `set_text_shaping`, `text_shaping`,
  `load_system_font`, `try_load_system_font` and
  `set_system_font_fallback`.
- OFL-licensed Noto subsets for Arabic, Devanagari and Japanese in
  `assets/fonts` (a few KB each), used by the tests and the guide.
- IME composition in text inputs, for Chinese, Japanese, Korean and other
  scripts typed through an input method. A focused retained-UI text input
  allows IME for the window and puts the candidate window at its caret;
  the composition (preedit) is drawn at the caret, underlined, with the
  part being converted underlined thicker; the commit is typed into the
  field. Mid-composition, Enter, Escape, Backspace, Delete, Home, End, the
  arrows and select-all belong to the IME (Enter picks a candidate instead
  of submitting, Escape cancels instead of closing the screen). Moving the
  focus mid-composition drops the composition rather than committing it
  into the next field. `UiBuilder::ime_allowed(false)` /
  `Ui::set_ime_allowed` keep IME off for a field; `Ui::ime_allowed`,
  `wants_ime`, `preedit`, `ime_area` and `composition_interrupted` report
  the state for custom hosts.
- `ponko::input::ImeEvent` and, on `Input`, `feed_ime`, `preedit`,
  `preedit_cursor`, `composing`, `set_ime_allowed`, `ime_allowed`,
  `set_ime_cursor_area`, `ime_cursor_area` and `cancel_composition`, for
  games with their own text widgets and for custom hosts. The desktop
  window forwards winit's IME events (it handled only commits before) and
  applies the requests after the frame.
- Web builds compose text too: winit's web backend has no IME, so while
  IME is allowed the engine focuses a hidden `<textarea>` at the caret and
  reads the browser's composition, input and key events from it.
- Input scenarios replay compositions: `InputScenario::ime(tick, event)`,
  and the JSON steps `ime_enabled`, `ime_preedit` (`text`, optional
  `cursor`), `ime_commit` and `ime_disabled`; the MCP
  `run_input_scenario` tool takes them.
- C and C++: `ponko_ctx_feed_ime` (`PonkoImeEvent`, `PonkoImeEventKind`),
  `ponko_ctx_typed_text`, `ponko_ctx_preedit`, `ponko_ctx_preedit_cursor`,
  `ponko_ctx_composing`, `ponko_ctx_set_ime_allowed`,
  `ponko_ctx_ime_allowed`, `ponko_ctx_set_ime_cursor_area`,
  `ponko_ctx_ime_cursor_area`, `ponko_ctx_cancel_composition`,
  `ponko_ctx_ui_set_ime_allowed`, `ponko_ctx_ui_ime_allowed`,
  `ponko_ctx_ui_wants_ime`, `ponko_ctx_ui_preedit`,
  `ponko_ctx_ui_ime_area`, `ponko_ctx_ui_composition_interrupted`,
  `ponko_ctx_ui_frame_ime` (`PonkoUiComposition`) and
  `ponko_input_scenario_ime`; in C++ the matching `ctx`, `ui::tree` and
  `input_scenario` members, `ui::composition`, `ime_event` and
  `ime_preedit`, `ime_commit`, `ime_enabled`, `ime_disabled`.
- Localization, `ponko::locale`: string tables, one JSON file per language,
  loaded with `Assets::load_strings` into the `Strings` resource and
  reloaded with hot reload. `Strings::get` and `format` look keys up in the
  current language (then the reference language, the first table loaded;
  a key no table has shows as itself), with `{name}` and `{name:2}`
  placeholders filled from `Arg`s; plural forms by the Unicode CLDR's
  categories (`PluralCategory`, `plural_category`, `plural_categories`)
  for Arabic, Hebrew, the Slavic and Baltic languages, Romanian, French,
  Portuguese, Hindi, the languages without plurals and English's rule for
  the rest; numbers with each table's separators and digits
  (`Strings::number`); `set_language`, `language`, `languages`,
  `is_rtl`, `revision`, and a font per language (`set_font`, `font`, or a
  table's `font` file, which the engine loads with the built-in font
  behind it). `Strings::missing` and `missing_report` list the keys, plural
  forms and arguments a language lacks, for a test; `Strings::PSEUDO`
  (`"en-XA"`) is a pseudo-locale with accented, longer text for testing
  layouts.
- Retained UI text from the string tables: `Ui::set_text_key` (with
  arguments), `UiBuilder::text_key` and `Ui::text_key`. Keyed nodes show
  the current language's text and font, and follow when the language or a
  table changes from the next frame on; `Ui::localize` does it for a UI you
  drive yourself. `Ui::set_text` replaces a key with plain text.
- Coin Dash ships in English and Arabic: its text comes from
  `assets/coin_dash/lang`, its settings have a language button and save
  the choice, and the Arabic font is a subset of Noto Sans Arabic (OFL).
  Arabic is offered when the game is built with `text-shaping`, which its
  packages and web build now enable. `tests/coin_dash_languages.rs` checks
  that the tables are complete and that the pseudo-locale still fits the
  screen.
- C and C++: `ponko_ctx_load_strings`, `ponko_ctx_strings_insert_json`,
  `ponko_ctx_strings_set_language`, `ponko_ctx_strings_language`,
  `ponko_ctx_strings_language_count`, `ponko_ctx_strings_language_at`,
  `ponko_ctx_strings_is_rtl`, `ponko_ctx_strings_set_font`,
  `ponko_ctx_strings_font`, `ponko_ctx_strings_format` (`PonkoStringArg`,
  `PonkoStringArgKind`), `ponko_ctx_strings_missing_report`,
  `ponko_plural_category` (`PonkoPluralCategory`),
  `ponko_ctx_ui_set_text_key` and `ponko_ctx_ui_localize`; in C++
  `ctx::load_strings`, `insert_strings_json`, `set_language`, `language`,
  `languages`, `language_is_rtl`, `set_language_font`, `language_font`,
  `format` and `missing_strings`, `ponko::arg`, `ponko::plural`,
  `ponko::plural_category`, and `ui::tree::set_text_key` and `localize`.
- The guide's chapter "Localization".
- Rich text in retained UI labels: `Ui::set_rich` (or `UiBuilder::rich`)
  reads a node's text as markup, `[color=#ffcc00]`, `[b]`, `[font=name]`,
  `[size=24]`, `[u]`, `[link=target]` and `[img=name alt="A"]`, with `[[`
  for a bracket; anything else in brackets is shown as it is. Runs wrap,
  align and line up on one baseline; inline images (button glyphs) are as
  tall as the text. `Ui::set_inline_image` and `Ui::set_inline_font` name
  images and fonts for it (the font named `bold` is `[b]`'s, else the
  node's font is drawn heavier). A click on a link sends
  `UiEvent::LinkClicked(node, index)`, and `Ui::link` gives its target.
  `Ui::accessible_name` is the text without markup, images as their alt
  text. Keyed text (`Ui::set_text_key`) can be rich: the string table
  entry holds the markup and text arguments are escaped. With
  `text-shaping` each run is shaped and right-to-left runs are put in
  visual order. Rust-only for now.
- `tests/fixtures/render/rich_text_dialogue.png`: a wrapped dialogue line
  with a colored name and an inline button glyph, and
  `rich_text_arabic.png`, a right-to-left line with a colored first word.
- Screen readers on Windows and Linux: with the `accessibility` feature the
  retained UI (`ctx.ui`) is given to UI Automation on Windows (Narrator,
  NVDA, JAWS) and to AT-SPI on Linux and the BSDs (Orca), through
  AccessKit's adapters, with the same tree, focus and requests (press,
  focus, set value, increment, decrement) as on macOS. The engine's window
  gets the adapter by itself; `UiAccessAdapter::SUPPORTED` is true on all
  three desktop systems.
- `UiAccessAdapter::for_hwnd` (Windows) and `UiAccessAdapter::for_atspi`
  (Linux and the BSDs), for hosts with a window of their own, next to
  `for_ns_view`; `UiAccessAdapter::set_window_bounds` tells AT-SPI where the
  window is.
- `tests/ui_access_platforms.rs`: a UI Automation client (Windows) and an
  AT-SPI client (Linux, under `dbus-run-session` in CI) read and operate the
  guide's sample menu.
- The screen's theme styles the immediate widgets' font and borders too:
  `UiImmediate::font` follows `UiTheme::font`, and `UiImmediate::border_width`
  (a border on buttons, in the accent color) follows `UiTheme::border_width`.
  Both follow `Ui::set_theme` by the same rule as the colors (a value set by
  hand stays), and both default to what the widgets drew before.
- `UiImmediate::scale()` reads the scale the widgets are drawn at.
- Steering wheels, pedals, flight sticks and other joysticks: `ctx.input.joysticks()`,
  `joystick_info` (kind, ids, how many axes, buttons and hats, which effects it can play),
  `joystick_axis`, `joystick_down` / `joystick_pressed` / `joystick_released` and
  `joystick_hat`, by number, in their own slots (`MAX_JOYSTICKS`) kept for a device that
  comes back.
- Force feedback on joysticks: `set_force_feedback(joystick, ForceFeedback { .. })` plays a
  constant push, a centring spring, a damper, friction, a vibration and the device's own
  centring until changed, sent only when it changes.
- The SDL backend (`gamepad-sdl`) reads wheels, flight sticks and throttles as joysticks
  (and any device SDL can't read as a gamepad) and plays force feedback through SDL's
  haptic API.
- `GamepadEvents::joystick_*` for backends, `GamepadBackend::set_force_feedback` (default:
  ignored), and `FakeGamepads::connect_joystick`, `set_joystick_axis`,
  `set_joystick_button`, `set_joystick_hat`, `force_feedback` for tests.
- Not yet: joystick bindings in an `ActionMap`, input scenarios, the web and the C API (#525).
- The SDL gamepad backend reads DualSense and DualShock touchpads (`GamepadButton::Touchpad`, `Input::gamepad_touchpad`, `Input::gamepad_touchpad_events`) and can set supported controllers' light bars and player LEDs (`Input::set_gamepad_light`, `Input::set_gamepad_player_leds`). DualSense adaptive triggers (`Input::set_gamepad_trigger_effects`, `TriggerEffect`) support resistance, a break point, vibration, and a ten-zone resistance profile over USB and Bluetooth. Outputs are cleared when a controller is lost and when the backend shuts down. `GamepadCapabilities` gains `light`, `player_leds`, `touchpad` and `adaptive_triggers`. These controls are available from Rust, C (`ponko_ctx_gamepad_extras` and friends) and C++.
- `Ui::layout_passes` counts how many times the retained UI was laid out,
  so a test can check that a still screen is not laid out every frame.
- `ponko_bench` has `frame2d/ui-still`, `frame2d/ui-counting` and
  `frame2d/no-ui`: the CPU side of a whole 2D frame without a window
  (2,000 sprites under a HUD and a settings screen of about 150 nodes).
- `ButtonState::{Released, Held}` and `InputEvent::key`, `mouse` and
  `gamepad_button` constructors name synthetic held state. Boolean variants
  remain available; repeating held input does not repeat pressed edges.
- `InputEvent` supports `PartialEq` (ordinary float equality), and `ActionMap`
  equality compares persistent bindings, ignoring runtime state.
- `ui::ScreenMode { Modeless, Modal }` (`#[non_exhaustive]`, default
  `Modeless`) names how a pushed screen treats the ones below it:
  `Ui::push_screen_with_mode(root, mode)`, the shorthands `Ui::push_modal` and
  `Ui::push_modeless`, and `Ui::screen_mode(root)` to read it back.
- C: `PonkoUiScreenMode` and `ponko_ctx_ui_push_screen_with_mode(ctx, target,
  root, mode)`; an unknown mode is `PONKO_INVALID_ARGUMENT`. C++:
  `ponko::ui::screen_mode`, `tree::push_screen(root, mode)`, `push_modal` and
  `push_modeless`.
- `UiLayout::unstable_custom`, `unstable_style`, `unstable_style_mut` and
  `into_unstable_style`: the Taffy escape hatch, named so that its exemption
  from the stability promise is visible at the call site. STABILITY.md lists
  them, and the old names, as unstable.
- Controllers: the `gamepad-sdl` feature reads pads through SDL 3 instead of
  gilrs on the desktop: gyro and accelerometer (DualShock 4, DualSense,
  Switch Pro, Joy-Cons), trigger rumble (Xbox One and Series pads) and body
  rumble on macOS too, with the same `ctx.input` API. It links the SDL 3
  library; gilrs stays the default and the fallback. The guide's
  controllers chapter has why SDL was picked and what each platform gets.
- Key labels follow the keyboard layout: `Input::key_labels` (a
  `KeyLabels`) labels the letter, digit and punctuation keys as on the
  player's keyboard (read from the system on Windows and macOS when the
  window opens or regains focus, and learned from each key pressed on every
  desktop system and the web), `KeyLabels::text` for text prompts,
  `GlyphAtlas::set_key_labels` for key caps, `Input::set_key_labels` and
  `KeyLabels::from_layout` for a layout option or a test. The C atlas calls
  (`ponko_glyph_atlas_draw_prompt`, `ponko_glyph_atlas_texture`) label the
  caps for the player's layout themselves.
- `InputScenario::connect_gamepad`, and an optional `info` object on the
  `gamepad_connected` scenario step (name, family, ids, capabilities).
- On-screen controls a game restyles: `ui::touch::TouchStyle` (colors of
  the stick, knob, buttons idle and held and labels, images of the game's
  own in place of the disc, size, label size) with `TouchControls::style`
  and `control_style`, and `TouchControls::place` with `TouchPlace` (a
  point of the view plus an offset, kept from its corner when the window
  changes).
- Players arrange the on-screen controls: `Input::edit_touch_controls`
  lets a finger or the mouse drag them (pressing nothing), and their
  places, sizes and opacity are a `TouchLayout` (`Input::touch_layout`,
  `set_touch_layout`), saved in the player's storage with
  `Input::save_touch_layout` and read back with `restore_touch_layout`.
- `InputEvent::Trackpad` and the `trackpad` scenario step: a Mac
  trackpad's pinch, rotation and two-finger double tap feed the touch
  gestures (`Gesture::Pinch`, `Rotate`, `DoubleTap` and their amounts).
- Windows and Linux desktops with a touch screen count as touch devices
  before the first finger, so on-screen controls set to show on touch
  devices show from the start.
- UI groups can fade as one image: `Ui::set_fade_as_group(node, true)` (or
  `UiBuilder::fade_as_group()`, `Ui::fades_as_group` to read it back). While
  the group is partly transparent (its own opacity or an ancestor's), it is
  drawn opaque into an offscreen layer the size of the window and shown at
  its opacity, so overlapping children no longer show through each other
  and a panel's fill no longer shows through its buttons. Groups inside a
  fading group get their own layer. A UI with no group fading draws exactly
  what it drew before: the `ui/draw-*` benchmark rows are unchanged within
  noise (about 1 µs on 22–31 µs frames). A fading group's nodes keep their
  drawings through the fade: the new `ui/draw-fade-group` row takes 0.022 ms
  a frame where fading the same screen piece by piece (`ui/draw-fade`)
  takes 0.030 ms. World-space panels (`UiPanel3D`) still fade each piece.
  From C: `ponko_ctx_ui_set_fade_as_group` and `ponko_ctx_ui_fades_as_group`;
  in C++ `ui::tree::set_fade_as_group` and `fades_as_group`.
- `ponko_ctx_ui_layout_passes` (C) and `ui::layout_passes()` (C++): how many
  times the tree has been laid out, for tests that a still screen costs no
  layout (`Ui::layout_passes` in Rust).
- `UiPaint` implements `PartialEq`.
- `ctx.input.arrange_touch_controls()` opens a ready-made screen, built with the
  retained UI, where the player arranges the on-screen touch controls with
  a finger or the mouse: drag to move, pick one and press `-` or `+` to
  resize it, `Size` and `Opacity` sliders for all of them, `Reset` to the
  game's placement, and `Save` to the player's storage. `Cancel`, Escape or
  the game closing it put back the arrangement it had when it opened. The
  panel keeps clear of the controls and fits phones and tablets either way
  up; its nodes are named `touch.arrange.<part>` for the game's own words.
- `ctx.window.safe_area()`: the part of the window a phone's notch or camera
  hole, its home bar and its rounded corners leave, in logical pixels
  (the whole window where nothing covers it), and `ctx.window.safe_insets()`,
  how far in from each edge they reach (`ponko::window::SafeInsets`). In
  the browser the engine reads the page's `env(safe-area-inset-*)` (a page
  with `viewport-fit=cover`) at start and whenever the window changes
  size; `ctx.window.feed_safe_insets` feeds them as a phone reports them, for
  tests and platform layers. Not in C or C++ yet.
- UI roots made with `.inside_safe_area()` (`Ui::set_inside_safe_area`)
  are laid out in the safe area: their sizes, fractions, anchors and
  `auto` margins count from its edges. Roots still fill the whole window
  by default. A `Ui` of the game's own takes the area with
  `Ui::set_safe_area`.
- `ctx.window`: switch windowed / borderless / exclusive fullscreen at runtime (`set_mode`, `mode`, `current_monitor`, `monitors`, `exclusive_modes`), set the title, size, position, resize limits, icon, always-on-top and decorations, and remember the choice with `save_settings` / `restore_settings` (a monitor or mode that is gone falls back to windowed with a printed reason).
- Cursors: the system set as `CursorIcon`, custom RGBA cursors, hide, confine and lock (`set_cursor`, `set_cursor_visible`, `set_cursor_grab`); `ctx.input.set_cursor_locked` keeps working.
- Clipboard text on desktop and web (`clipboard_read`, `clipboard_write`), used by the retained UI's text input for copy, cut and paste.
- Window events through `ctx.events`: `WindowEvent::Focused` and `Minimized` (sent only when the state changes), `CloseRequested` (cancel with `ctx.window.cancel_close()`), `FileDropped`, `ScaleFactorChanged`.
- Headless and no-`window` builds keep the API and do nothing; `WindowService::with_backend` takes a fake backend for tests. Errors are `ponko::error::Error` (`Invalid`, `NotFound`, `Unsupported`).
- C and C++: typed window, monitor, cursor, clipboard and event calls with versioned `PonkoWindow*Ex` descriptors (`ponko_ctx_window_set_mode_ex`, `ponko_ctx_window_monitor_ex`, `ponko_ctx_clipboard_read`, ...), and `ponko::ctx::window_*` wrappers.
- Guide: the "Window service" chapter (direct calls, saving, high-DPI rules for pixel art and UI, what each call does on the web).
- Key labels from C and C++: `ponko_ctx_key_label` (a key's label on the
  player's keyboard), `ponko_ctx_glyph_label` (a glyph's),
  `ponko_ctx_key_labels_changed` and `ponko_ctx_set_key_labels` with
  `PonkoKeyLabel`; C++ `ctx.key_label`, `layout_glyph_label`,
  `key_labels_changed` and `set_key_labels`.
- Key labels follow the active xkb layout on Linux and the BSDs, under X11
  (the X server's XKB map and state) and Wayland (the compositor's keymap
  and active layout), from the first frame and as the player switches
  layouts.
- In a browser, key labels come from the Keyboard Layout Map
  (`navigator.keyboard.getLayoutMap()`) where it exists (Chromium-based
  browsers, secure pages), asked when the page opens and regains focus;
  elsewhere they are still learned from the keys pressed. The headless
  runner `web/smoke/key-labels.mjs` checks both and a refused map.
- The retained UI's widget set: `Ui::dropdown`, `tabs` and `tab`,
  `radio_group` and `radio`, `progress_bar`, `spinner`, `number_field`,
  `list` and `grid_view` (virtualized: nodes only for the rows on screen, so
  a million items cost what twenty do), `split` (`UiAxis`; its value is
  the first pane's share of the length the divider leaves, 0.5 for equal
  panes) and `key_binding` (it shows "Press a key" while it listens), with the builder methods `items`, `selected`, `range`,
  `step`, `tooltip`, `context_menu`, `draggable` and `drop_target`. Their
  state is read and set with `selected`/`set_selected`, `items`/`set_items`,
  `number`/`set_number`/`set_number_range`, `set_list_len`, `list_rows`,
  `list_item`, `scroll_to_item`, `binding`/`set_binding`/`is_capturing`,
  `set_tooltip`/`shown_tooltip`, `set_context_menu`/`open_context_menu`/
  `popup_owner`/`close_popup`, `set_draggable`, `set_drop_target`,
  `dragging` and `set_parent`. `Ui::widget(id)` names a node's widget.
- New `UiEvent`s: `Chosen(node, entry)` for a context-menu entry,
  `Dropped(item, onto)` for drag and drop (the game moves the node), and
  `RowsShown(list)` when a list's rows show other items.
- Every widget works with the pointer, the keyboard and a gamepad, and a
  modal screen keeps the focus inside it. Tooltips show after hovering for
  `UiTheme::tooltip_delay`, or at once on keyboard focus.
- Themes: `UiTheme::dark()` and `UiTheme::light()`, and the tokens
  `spacing`, `radius` (rounded corners) and `tooltip_delay`.
- Screen readers: the roles `ComboBox`, `TabList`, `Tab`, `TabPanel`,
  `RadioGroup`, `RadioButton`, `ProgressBar`, `Spinner`, `SpinButton`,
  `List`, `ListItem`, `Grid`, `GridCell`, `Menu`, `MenuItem` and `Splitter`,
  the value `UiAccessValue::Number`, and the node states `selected`,
  `expanded` and `position`.
- The immediate UI uses the same names: `toggle`, `slider` and
  `progress_bar`.
- C: a builder and accessors for every widget (`ponko_ctx_ui_add_dropdown`,
  `..._add_tabs`, `..._add_list`, ...), the descriptors `PonkoUiEventEx`
  (`ponko_ctx_ui_next_event_ex`), `PonkoUiThemeEx` (`ponko_ui_theme_ex_dark`
  and `_light`, `ponko_ctx_ui_theme_ex`, `ponko_ctx_ui_set_theme_ex`),
  `PonkoUiInputEx` (`ponko_ctx_ui_frame_ex`) and `PonkoUiAccessStateEx`
  (`ponko_ui_access_state_ex`), the new roles and event kinds, and
  `ponko_ctx_ui_toggle`, `ponko_ctx_ui_slider` and
  `ponko_ctx_ui_progress_bar`. C++: the matching `ui::tree` methods,
  `ui::theme_ex`, `ui::dark_theme`/`light_theme`, `ui::input_ex`, the new
  `ui::role` and `ui::event_kind` values, `ui::access::state_of`, and
  `ctx::ui_toggle`, `ui_slider` and `ui_progress_bar`.
- A test that every control's text has at least WCAG's 4.5:1 contrast
  against the control's own fill in every state (idle, hovered, focused,
  pressed, disabled) and theme, and GPU tests that check it on the drawn
  pixels.
- `examples/ui_widgets.rs` and the guide's "Widget gallery" page, with GPU
  reference images of every widget and a `ponko_bench` case (`ui/list`) for
  the list's flat cost.
- Retained UI animation, all of it optional and free when unused:
  - State transitions: the theme token `UiTheme::transition`
    (`UiTransition::new(0.12)`) eases a control's fill between its states
    (idle, hovered, focused, pressed, disabled), and `UiTheme::press_scale`
    shrinks a pressed control a little. An interrupted transition turns
    round from what is shown.
  - Screen transitions: `Ui::set_screen_transition(root, enter, exit)` with
    `UiScreenTransition::fade`, `slide`, `zoom` and their combinations. A
    popped screen leaves the stack at once and is drawn, without input,
    while it goes, where it was on the stack.
  - Tweens and timelines: `Ui::animate(node, UiTween)` and
    `Ui::play(node, &UiTimeline)` move a node's opacity, scale, offset,
    background, text color, width or height, in sequence or together, once,
    several times, in a loop or back and forth (`UiRepeat`). A new tween
    takes over its property from where it is. `UiEvent::AnimationDone`
    reports the end; `Ui::stop_animations`, `Ui::is_animating`.
  - `Ui::drawn(node)` (`UiDrawn`) says how a node is drawn now, for tests;
    `Ui::clock()` and `UiTransition::progress` let text drawn by hand
    animate on the same clock; `Ui::set_reduced_motion` makes state and
    screen transitions instant.
  - Every animation runs on the UI clock (`UiInput::dt`): it goes on while
    the simulation is paused, is independent of the frame rate, and adds
    no allocation to a frame.
- `Bezier::EASE`, `EASE_IN`, `EASE_OUT`, `EASE_IN_OUT` and `BACK_OUT`, the
  usual CSS curves (`ponko::skeleton2d::Bezier`, the one curve type of skeletons and UI).
- `DrawList::transform_from`: scale, move and fade the sprites pushed since a
  mark.
- The guide's "UI animation" page. The reference games' menus animate:
  Shard Grove's and Coin Dash's retained screens fade in and out with eased
  controls, and the title and pause text of Coin Dash and Ember Hop fades
  and rises in.
- C: `PonkoUiTransition`, `PonkoUiScreenTransition`, `PonkoUiTween`,
  `PonkoUiDrawn`, `ponko_ui_ease`, `ponko_ui_transition`,
  `ponko_ui_transition_progress`, `ponko_ui_screen_fade` / `_slide` / `_zoom`,
  `ponko_ui_tween_defaults`, `ponko_ctx_ui_animate`, `ponko_ctx_ui_play`,
  `ponko_ctx_ui_stop_animations`, `ponko_ctx_ui_is_animating`,
  `ponko_ctx_ui_set_screen_transition`, `ponko_ctx_ui_screen_transition`,
  `ponko_ctx_ui_drawn`, `ponko_ctx_ui_clock`,
  `ponko_ctx_ui_set_reduced_motion`, `ponko_ctx_ui_reduced_motion` and the
  event kind `PONKO_UI_EVENT_ANIMATION_DONE`. C++: `ui::tree::animate`,
  `play`, `stop_animations`, `is_animating`, `set_screen_transition`,
  `screen_transition`, `drawn`, `clock`, `set_reduced_motion`, and
  `ui::make_transition`, `ui::screen_fade` / `slide` / `zoom`, `ui::tween_to`,
  `ui::named_ease`.
- Touch input: the window feeds every finger (`InputEvent::Touch`, with
  `InputEvent::touch_start`, `touch_move` and `touch_end` for tests), and
  `Input::touches`, `gesture_events`, `drag_delta`, `pinch_scale`,
  `rotation_delta` and `touch_device` read them. Tap, double tap, long
  press, drag, pinch, rotate and four swipes are recognized with thresholds
  in `GestureSettings` (`Input::set_gesture_settings`), measured in game
  time, so headless tests see what a phone sees.
- Gestures bind to actions like keys: `ButtonBinding::Gesture`,
  `AxisBinding::Gesture { axis: GestureAxis, scale }` (pinch, rotation and
  drag amounts) and `Axis2Binding::Drag { scale }`. They are saved in the
  action map file, and `Input::pressed_this_frame` reports a gesture that
  just happened for rebinding screens. The prelude gains `Gesture`.
- On-screen controls: `Input::set_touch_controls` with
  `ui::touch::TouchControls` puts sticks, round buttons and corner buttons
  on the screen, each driving a named action while a finger holds it. They
  show on touch devices only by default (`TouchShow`), take their fingers
  away from gestures and the UI, step aside while a UI screen is open, are
  buttons and groups to assistive technology, and allocate nothing once
  placed. `Input::touch_control_center`, `set_touch_control_shown`,
  `set_touch_controls_show` and `touch_controls_shown` go with them.
- Input scenarios have a `touch` step and the `InputScenario::tap`,
  `touch_hold` and `touch_drag` builders.
- In the browser the game's canvas takes every finger (`touch-action:
  none`), and the platform's touch points say whether it is a touch device.
- Coin Dash, Ember Hop and Shard Grove have on-screen controls and taps on
  their menus, and each has a test that plays it by touch.
- C and C++: `ponko_ctx_feed_touch`, `ponko_ctx_touches`,
  `ponko_ctx_gesture_events`, `ponko_ctx_gesture_deltas`,
  `ponko_ctx_touch_device` and `ponko_ctx_set_touch_device`, the gesture
  settings, `ponko_ctx_set_touch_controls` and its neighbours,
  `ponko_input_scenario_touch`, and the `PONKO_ACTION_GESTURE`,
  `PONKO_ACTION_GESTURE_AXIS` and `PONKO_ACTION_DRAG` binding sources; in
  C++ `ctx.feed_touch`, `ponko::touch_controls` and
  `action_binding::gesture`, `gesture_axis` and `drag`.
- The guide's "Touch: gestures and on-screen controls" chapter.
- Controllers: `Input::rumble` with dual motors and trigger motors where the
  backend has them (`Rumble`, timed in real time, stopped on focus loss, sent
  to the backend only when the levels change), `Input::gamepad_info` (name,
  USB ids, `ControllerFamily` detection, `GamepadCapabilities`),
  `Input::gamepad_motion` (gyro and accelerometer, `GamepadMotion::aim_delta`)
  and `gamepads_connected` / `gamepads_disconnected` edges.
- Button prompts: `Input::last_device`, `Input::action_glyphs` and
  `action_glyphs_for` turn an action's bindings into `Glyph`s for the device
  used last, labelled for the pad's family; `GlyphAtlas` draws every glyph
  with the engine's own shapes and draws prompt rows (`draw_prompt`) or
  rich-text inline images.
- Local multiplayer: a reconnecting pad gets its old slot back, and players
  are separate from slots (`assign_player`, `player_device`, `free_player`,
  `join_pressed`, `Action::for_pad`).
- `GamepadBackend` to plug in a gamepad source with `Game::gamepad_backend`,
  and `FakeGamepads`, a scripted backend for tests.
- `InputEvent::GamepadConnected` and `InputEvent::GamepadMotion`, with the
  scenario steps `gamepad_connected` and `gamepad_motion`.
- C and C++: rumble, gamepad info, motion, last device, connection edges,
  players, action glyphs and the glyph atlas (`ponko_ctx_gamepad_rumble`,
  `ponko_ctx_action_glyphs`, `ponko_glyph_atlas_*`, ...; `ponko::glyph_atlas`).
- Guide chapter "Controllers", with what the built-in backend supports on each
  platform.
- Let retained UI nodes clip their descendants or pass pointer hit tests
  through to other nodes.
- Let a textured widget follow its animated state colors and plain labels
  shrink and center vertically within their content box.

**Audio**
- Audio mixer: buses nest (`Audio::set_bus_parent`), with volumes and mutes
  multiplying down the tree; `bus_volume`, `bus_muted`, `bus_gain`,
  `bus_parent` and `bus_name` read it back.
- Effect chains per bus (`Audio::set_bus_effects`, `BusEffect`): filter,
  reverb, compressor and delay, changed smoothly; changing the chain or a
  parent rebuilds the bus and its sounds carry on where they were.
- Sidechain ducking (`Audio::set_bus_ducking`, `Ducking`): a bus drops while
  up to four key buses are loud, with attack and release, on the audio thread.
- Mixer snapshots (`MixerSnapshot`, `Audio::add_snapshot`, `start_snapshot`,
  `stop_snapshot`, `snapshot_weight`) blending bus volumes and effect
  settings in and out over time, layered.
- Mixer files (`Audio::load_mixer`, `try_load_mixer`, `apply_mixer_json`):
  buses, effects, ducking and snapshots in JSON, hot reloaded; a broken
  file changes nothing.
- `MixerState` (`Audio::mixer_state`, `restore_mixer_state`) to save bus
  volumes, mutes and active snapshots with the game's settings.
- C and C++: `ponko_ctx_set_audio_bus_parent`, `_set_audio_bus_effects`,
  `_set_audio_bus_ducking`, `_add_audio_snapshot`, `_start_audio_snapshot`,
  `_stop_audio_snapshot`, `_load_audio_mixer`, `_apply_audio_mixer_json`,
  `_audio_mixer_state`, `_restore_audio_mixer_state` and getters, with
  `ponko::ctx` wrappers.
- The `audio_mixer` example ducks music under dialogue and switches to an
  underwater snapshot.
- `Audio::unlocked()`: whether the game can be heard yet. Always `true` on the
  desktop; in a browser `false` until the player first clicks, taps or presses
  a key, so a game can show "click to start".
- Fuzz target `sound` for every sound format the engine reads (WAV, IMA and
  Microsoft ADPCM, AIFF, Ogg Vorbis, FLAC, MP3): a whole decode, the
  slice-by-slice decode asset groups use, a stream opened from memory and a
  stream played a little, from a small seed corpus (`fuzz/seeds/sound/`) and
  the example sounds.
- `fuzz/targets.txt` is the written fuzz list: every target with the seconds
  `fuzz/run-all.sh` runs it (`fuzz/run-all.sh` uses them). `python3
  scripts/check-fuzz-list.py` fails when a target has a file, a `[[bin]]` or
  a README row that the list doesn't name, or the reverse.
- Fuzz target `webm_audio` for the sound of WebM videos (feature `video`):
  the tracks a video reads first, then its soundtrack's header and a little
  of it played with seeks and a loop, from seeds cut from the video test
  files (`fuzz/seeds/webm_audio/`) and the whole files. The `sound` target
  now also runs the decoder that desktop streams play through, decoding and
  seeking, with no audio device.
- Loop points: `Audio::set_loop(sound, start, end)` plays an intro once and
  loops the rest; `sound_loop` reads them back.
- Fades on a playing sound: `Audio::set_volume(playing, volume, seconds)`,
  `set_speed` and `fade_out`, linear in amplitude so crossfades add up.
- Music: `Audio::play_music` plays a `Song` of layers that stay in sync,
  with changes on the next beat or bar line (`Quantize`):
  `set_music_layer`, `play_stinger`, `stop_music`, `set_music_speed`,
  `music_beat`, `music_layer_volume` and `set_music_bus`.
  `Song::from_layers` builds a song from data.
- `BusEffect::PitchShift` (`PitchShiftEffect`): pitch without tempo, with
  `semitones` and `undo_speed`.
- Pause groups: `Audio::pause_bus` and `bus_paused` hold a bus's sounds
  where they are; `Game::pause_audio_in(state, buses)` ties it to a game
  state.
- Voice limits: 64 sounds heard at once by default
  (`Audio::set_voice_limit`, up to 256), per-bus limits
  (`set_bus_voice_limit`), priorities (`set_priority`,
  `d3::AudioSource::with_priority`, `DEFAULT_PRIORITY`) and virtual voices
  that are followed while not heard (`is_virtual`, `voice_count`).
- Output devices: `Audio::output_devices`, `set_output_device`,
  `chosen_output_device`, `output_device` and `device_events`. A lost
  device moves sound to the default one, and back when a chosen device
  returns.
- `Audio::open_offline` and `render_offline` mix into memory, also in
  headless runs, so tests can check what a game sounds like.
- IMA ADPCM WAV and AIFF files decode.
- The web's streamed sounds honour loop points, start positions, fades
  and music timing.
- C and C++: all of the above (`ponko_ctx_set_sound_loop`,
  `ponko_ctx_play_music`, `PonkoQuantize`, `ponko_ctx_pause_audio_bus`,
  `ponko_game_pause_audio_in_state`, `ponko_ctx_set_audio_voice_limit`,
  `ponko_ctx_audio_output_devices`, `ponko_ctx_render_audio_offline`, ...),
  and `PONKO_BUS_EFFECT_PITCH_SHIFT`.
- `ponko_bench --features audio -- audio3d`: 1,000 positional sources,
  the game side of a frame and the mix.

**Physics, navigation and AI**
- 3D navigation obstacles take a shape and a turn: `NavObstacle::yaw` turns
  one around world Y, and `NavObstacle::shape` (`d3::NavObstacleShape`) makes
  it a box (the default), a cylinder (`NavObstacle::cylinder`) or a convex
  prism of up to 8 points (`NavObstacle::convex`, `d3::NavConvexPolygon`).
  Carving cuts the exact shape; blocking tests polygons against it instead of
  its world bounds. Rust-only for now: C and C++ obstacles stay unturned boxes.
- `ponko_bench`'s `nav3d/carve-tile-flat` and `nav3d/carve-tile-bumpy` time
  one carved tile rebuild (about 0.1 ms per 32-cell tile).
- `ctx.physics.set_paused(bool)` and `ctx.physics.is_paused()` hold only the
  2D physics step: bodies stay where they are, no collision events arrive,
  and every other simulation system and the tick counter keep running, so a
  pause screen's own sim systems can read the press that resumes. C:
  `ponko_ctx_set_physics_paused` and `ponko_ctx_is_physics_paused`; C++:
  `ctx.set_physics_paused(bool)` and `ctx.is_physics_paused()`. The guide's
  "Pausing only physics" section shows the pattern.
- `ctx.physics3d.set_paused(bool)` and `ctx.physics3d.is_paused()` hold only
  the 3D physics step, the same call as the 2D `ctx.physics.set_paused`:
  bodies keep their pose and velocity, characters keep their velocity but
  don't move, no collision events arrive and no contact hook runs, while
  every other simulation system and the tick counter keep running, headless
  too. In a window, held bodies are drawn at their last pose. C:
  `ponko_ctx_set_physics3d_paused` and `ponko_ctx_is_physics3d_paused`; C++:
  `ctx.set_physics3d_paused(bool)` and `ctx.is_physics3d_paused()`. The
  guide's "Pausing only 3D physics" section shows the pattern.
- `d3::NavAgent` builds with struct-update syntax, like other components:
  `NavAgent { speed: 4.0, destination: Some(goal), ..Default::default() }`.
  Its path, crowd slot and speed reached moved into the new public
  `NavAgent::state` field, of the opaque type `d3::NavAgentState`. That type
  has no public constructor, so a literal always ends with
  `..Default::default()` (or `..other_agent`) and new settings can arrive
  without breaking it. Setting fields on a `NavAgent::default()` keeps
  working; clippy's `field_reassign_with_default` lint now suggests the
  literal there. `d3::NavObstacle` is still built with its constructors
  (`NavObstacle::new`, `::cylinder`, `::convex`), as its shape needs them.
- Behaviour trees (`ponko::ai`, guide: "Game AI"):
  - `{"subtree": "ai/patrol.json"}` runs another tree by name, with its
    blackboard defaults. `BehaviorTrees` links trees in any order and links
    again every tree that uses one that changed; `Assets::load_behavior_tree`
    loads and hot-reloads the files a tree names. A tree that contains
    itself is refused with every name on the cycle
    (`BehaviorTrees::try_insert`; `insert` panics). `BehaviorTree::subtrees`.
  - `{"parallel": [..], "success": "any", "failure": "all"}`: all children
    at once with a stated policy (`"all"`, `"any"` or a count); it stops
    what still runs when it ends. One child at most may hold `do` nodes.
  - `{"random": [..]}` and `{"chance": p}`, drawn from the game's seed, the
    brain's tick and its entity (`Brain::with_seed` / `set_seed` for brains
    run by hand), so the same seed and inputs give the same decisions.
  - `{"wait_for": "signal", "timeout": s, "store": "key"}` waits for an
    `ai::Signal` event (`Signal::to`, `Signal::broadcast`, `.with(value)`)
    instead of polling; the signal wakes the brain whatever its period.
    `ai::forward` turns any event type into signals; `Brain::signal`,
    `BehaviorTree::signals`.
  - A brain's JSON holds its memory (running nodes, waits, cooldowns), so a
    saved world carries on at the same running node, counting ticks on from
    the save; a changed tree (`BehaviorTree::fingerprint`) starts over.
- C and C++: `ponko_behavior_tree_new_linked` (`behavior_tree(json,
  subtrees)`), `ponko_brain_signal`, `ponko_brain_set_seed`,
  `ponko_brain_save_json` and `ponko_brain_load_json` (`brain::signal`,
  `set_seed`, `save_json`, `brain::load_json`).
- Crowds are drawn into global illumination's probes: culled in the
  probes' sun shadow map and six face directions (keeping in a face
  direction only the characters at least one probe pixel wide from a probe
  updated that frame), so they bounce and block light as models do.
- Window views (`SceneSettings::views`) cull and draw crowds for their own
  camera, on both the GPU and the CPU path.
- Crowds take part in occlusion culling: with the camera's
  `occlusion_culling` on, characters the depth grid hides are left out of
  the camera's view (on the GPU path the cull pass reads the grid as a
  uniform).
- Behaviour trees: a brain can run several actions at once. A `parallel`
  node may now hold `do` nodes in several children (move *and* aim); their
  actions run side by side and each ends on its own. `Brain::actions`,
  `started_action`, `action_param`, `succeed_action` and `fail_action`
  name one action; the one-action calls (`action`, `started`, `status`,
  `param`, `succeed`, `fail`) are about the first, so existing trees and
  systems behave as before. Two children of one `parallel` that could run
  an action of the same name at once are refused when the tree compiles.
  A brain's JSON lists `"actions"` when it does several, and its saved
  memory keeps the extra ones (`"also"`).
- C and C++ world brains and signals: `ponko_ctx_ai_signal` (with `_num`,
  `_entity`, `_vec2`, `_vec3` values; `PONKO_AI_EVERY_BRAIN` for all)
  sends `ai::Signal` events to the brains of entities;
  `ponko_ctx_ai_load_tree`, `ponko_ctx_ai_add_tree`,
  `ponko_ctx_ai_add_brain`, `ponko_ctx_ai_think`, `ponko_ctx_ai_action`,
  `_started`, `_finish`, `_param_num` and the blackboard calls
  (`ponko_ctx_ai_set_num` and friends, `ponko_ctx_ai_num`,
  `ponko_ctx_ai_flag`). C++: `ponko::ai::signal`, `think(ctx)`,
  `add_brain` and the rest in `namespace ai`.
- C and C++ native brains with several actions: `ponko_brain_action_at`,
  `ponko_brain_action_started`, `ponko_brain_finish_action`,
  `ponko_brain_action_param_num` (`brain::action_at`, `started_action`,
  `action_param`, `succeed_action`, `fail_action`).
- `CrowdLevel::fade` and `with_fade`: over that distance before its
  `max_distance` a crowd level crossfades into the level that starts there
  (or, with none, fades out to nothing) through complementary dither
  patterns, instead of popping. On the GPU path a crowd with a fade adds
  one indirect draw per view, level and mesh part for its crossfading
  characters; the CPU path draws them in runs of their own. Not in C or
  C++ yet.
- C and C++: `ponko_collider3d_scaled_mesh(mesh, scale)` and
  `ponko::scaled_mesh_collider(mesh, scale)` make a scaled mesh collider
  (Rust `d3::Collider::scaled_mesh`) without setting `shape` and `size` by
  hand.
- World saves and scenes keep a scaled mesh collider of a mesh file (one from
  `ctx.load_collision_mesh` or a scene) as the file and its scale:
  `{"ScaledMesh": {"mesh": {"file": "rock.glb"}, "scale": [2, 2, 2]}}`.
  Loading makes the file's mesh once for every scale. Colliders of meshes
  made in code are still left out.
- World brains inside C and C++ loops: `Brain` and `Blackboard` are query
  columns (`ponko_ai_brain_component`, `ponko_ai_blackboard_component`), so
  a `ponko_query` visits each entity's brain and blackboard with its other
  components, and `ponko_chunk_ai_brain` / `ponko_chunk_ai_blackboard` reach
  a row's. `ponko_ai_brain_action`, `_started`, `_finish`, `_param_num`,
  `ponko_ai_blackboard_set_*` and the `ponko_ai_blackboard_*` getters work
  on a row inside `ponko_query` and from `ponko_query_par` threads: a C
  system needs no list of entities first. C++: `ai::each<Ts...>(ctx, fn)`
  with `ai::brain_row` and `ai::blackboard_row`, `ai::query`,
  `ai::query_par` and `ai::rows`; `world_view::component_id<T>()` gives the
  C id of a component type.
- Blackboard getters for entities and 2D and 3D points: world brains
  `ponko_ctx_ai_entity`, `_vec2`, `_vec3` (C++ `ai::get_entity`,
  `get_vec2`, `get_vec3`), native brains `ponko_brain_entity`, `_vec2`,
  `_vec3` (C++ `brain::get_entity`, `get_vec2`, `get_vec3`).
- Lossy loopback links from C and C++: `PonkoLoopbackLink` (a versioned
  descriptor: loss, delay, jitter, seed), `ponko_loopback_link_defaults`
  and `ponko_net_messages_loopback_link` connect endpoints in memory over a
  link that loses, delays and reorders datagrams, the same ones on every
  run (Rust `NetMessages::loopback_link`). `ponko_net_messages_poll_elapsed`
  polls on the caller's clock (Rust `NetMessages::poll_elapsed`), so a
  native test steps its endpoints frame by frame. C++:
  `ponko::loopback_link`, `loopback_link_defaults()`,
  `net_messages::loopback(players, link)` and `net_messages::poll_elapsed`.
- `ponko_ai_brain_wake` and `ponko_ai_brain_signal` on a query's brain
  row, inside `ponko_query` and from `ponko_query_par` threads (Rust
  `Brain::wake`, `Brain::signal`); C++ `ai::brain_row::wake` and `signal`.
- C++ typed loops take the brain and blackboard columns directly:
  `world_view::each` and `each_par` hand `ai::brain_row` and
  `ai::blackboard_row` parameters beside the components, with filters,
  instead of the separate `ai::each` (which stays). `world_view::count`
  counts them too.
- The guide shows a debug map camera from C and C++: a target camera with
  its own debug-draw flag while the window's camera leaves the drawing out.
- `web/build.sh --simd`: WASM SIMD for a web build, `-C target-feature=+simd128`
  and the engine's `wasm-simd` feature always together (each alone is no
  faster, or slower), added to the rust flags cargo reads like `--threads`
  and keeping the caller's; it goes with `--threads`. The water guide's "On
  the web" uses it instead of setting the environment variable by hand.
- `Physics3D::set_terrain_rebuild` (and `clear_terrain_rebuild`): how a
  terrain tile's heightfield is made again after `ctx.load_world` or
  `ctx.load_scene`, from the tile's saved components, so a loaded world's
  tiles are solid where they are drawn (mirrored ones too) without the game
  making their colliders again.
- C and C++ games can make a terrain tile's ground again after a load:
  `ponko_ctx_physics3d_set_terrain_rebuild` (and `_clear_terrain_rebuild`)
  take a callback that reads the loaded tile with `ponko_loaded_world_get`
  (a read-only `PonkoLoadedWorld`) and returns its heightfield, which the
  engine frees, or NULL; C++ `ctx::set_terrain_rebuild` and
  `clear_terrain_rebuild` with a `ponko::loaded_world`, an exception coming
  out of `run`. The Rust-only rows for `Physics3D::set_terrain_rebuild` and
  `clear_terrain_rebuild` are gone; the 3D physics guide pairs both languages.
- `Water::drawn_time(&ctx.time)` (C `ponko_ctx_water_drawn_time`, C++
  `ctx.water_drawn_time(water)`): the time a water body is drawn at this
  frame, to pass to `height_at`, `sample` and `is_under` for what the
  player sees.
- An entity with a `TerrainFile` gets its terrain tile however it arrives:
  `spawn_prefab` and the editor's duplicate make it at once, and an entity
  spawned from JSON, by a system or by a mod gets it at the next frame. A
  file that isn't there is an error naming it: the prefab spawn or the
  duplicate fails with nothing left behind, and an entity spawned another
  way is despawned with the error printed. A `TerrainCollider` without a
  live mesh on such an entity gets the file's heightfield.
- Moving a terrain-file tile (by code or with the editor's gizmo) re-places
  its painted layers under it at the next frame, without making the tile
  again; its meshes and collider follow the transform. During a gizmo drag
  the layers settle once, when the drag ends.
- Crowds: thousands of animated skinned characters in a few draws.
  `CrowdAnimation::bake(skeleton, clips, fps)` samples clips (`CrowdClip`,
  looping or held) once; `Graphics::create_crowd(animation, levels,
  instances, options)` makes a `Crowd` of `CrowdInstance`s (transform, tint,
  clip and time, a second clip blended in, speed) drawn with up to 8
  `CrowdLevel`s (a skinned mesh per distance band, joints per corner,
  camera and shadow switches). The clips play on by themselves between
  updates; `update_crowd` replaces the characters, `crowd_len` counts them
  and `Ctx::free` frees a crowd. `try_create_crowd` and `try_update_crowd`
  return errors instead of panicking.
- On devices with compute culling a crowd is culled per view in a compute
  pass and drawn with one indirect draw per view, level and mesh part,
  skinned in the vertex shader from the bake, with sun and light shadows
  and motion vectors for temporal antialiasing. Other devices (WebGL2) take
  a CPU path: frames posed when the crowd is made, CPU culling, one draw
  per frame shown. Scenes without crowds make no crowd buffers, passes or
  uploads.
- `LiveCounts::crowds`.
- The `crowd` example: a plaza of 10,000 people, section after section
  cheering. A benchmark (`crowd_benchmark`, ignored) against the same
  characters as skinned models; the guide's "Crowds" chapter has the
  numbers.
- C and C++: `ponko_ctx_bake_crowd_animation`, `ponko_crowd_animation_clip`,
  `ponko_crowd_animation_free`, `ponko_ctx_create_crowd`,
  `ponko_ctx_update_crowd`, `ponko_ctx_crowd_len`, `ponko_ctx_free_crowd`,
  `ponko_crowd_instance_at` and the `PonkoCrowd*` types;
  `ctx::bake_crowd_animation`, `ctx::create_crowd` and friends and
  `ponko::crowd_animation` in `ponko.hpp`.
- `ponko::water`, a water kit for lakes, rivers and seas (`3d` feature):
  `WaterDesc` presets (`lake`, `river`, `sea`), screen-space, planar or
  sky reflections, refraction with depth absorption, shore foam, flow maps
  for rivers, Gerstner waves with a matching CPU height query
  (`Water::height_at`, `Water::sample`), an underwater screen effect,
  contact ripples (`Water::touch`, compute shaders; not on WebGL2) and
  `WaterQuality` presets. `WaterPlugin` keeps it up to date; nothing is
  made until water is spawned.
- `Buoyancy` (`physics3d`): bodies float at the depth their density gives,
  ride the waves and drift with currents, on the fixed step.
- `d3::Physics::rotation` and `d3::Physics::velocity_at`.
- C and C++: `PonkoWaterDesc` (versioned), `ponko_ctx_spawn_water`,
  `ponko_ctx_water_sample`, `ponko_ctx_water_touch`,
  `ponko_ctx_set_buoyancy_box` / `_sphere` and `ponko_game_water`;
  `ponko::water_desc` and `ctx::spawn_water` in `ponko.hpp`.
- The `water` example and the guide's "Water: lakes, rivers and seas" chapter.
- `ponko::ai`, an optional AI kit (guide: "Game AI"): behaviour trees
  written as JSON (`sequence`, `selector`, `utility`, `invert`, `succeed`,
  `check`, `set`, `do`, `wait`, `cooldown`) that read and write a reflected
  per-agent `Blackboard` and pick an action that the game's systems carry
  out (`Brain::doing`, `Brain::succeed`, `Brain::fail`).
  `Assets::load_behavior_tree` (and `try_load_behavior_tree`) loads a tree
  with hot reload; `ai::think`
  runs every due brain on all cores, deterministically, at a per-brain
  think period spread over the ticks (`Brain::with_period`,
  `ai::lod_period`). Utility helpers for plain systems too: `Curve`,
  `Consideration`, `normalize`, `combine`, `best`. The inspector shows the
  selected agent's tree with what each node did at the last think. A tree of
  15 nodes decides for 10,000 agents in about 0.8 ms on one core
  (`ponko_bench ai/10k`). New `ai_guards` example.
- C and C++: `ponko_behavior_tree_new`, `ponko_brain_*`, `ponko_ai_key`,
  `ponko_ai_curve`, `ponko_ai_combine` and `ponko::ai::behavior_tree`,
  `ponko::ai::brain` for brains a native game keeps in its own storage.
- Query whether a dynamic 3D body is sleeping and build fixed compound
  colliders from several primitive shapes.
- Stream static colliders around axis-aligned square foci.
- Advance the local headless frame clock by a chosen duration while
  preserving the simulation step.

**Networking**
- Client prediction and reconciliation helpers in `ponko::net`: `Prediction`
  applies the client's own input at once and keeps the inputs the server
  hasn't confirmed, numbered with a wrapping 16-bit sequence; `reconcile`
  compares the server's state for an input with the prediction and, only if
  they differ, replays the newer inputs on top of it. `InputQueue` is the
  server's side (each input once, in order, with the number to acknowledge),
  and `Smoothing` (with the `Smooth` trait, for `f32`, `Vec2`, `Vec3`, `Quat`
  and `Pose`) slides the drawn position over a correction instead of jumping.
  The wire format stays the game's. Example: `net_prediction` (150 ms round
  trip, packet loss); guide: "Your own player at once: client prediction".
- `Physics3D::move_character`: one tick of a `Character3D`'s walk from any
  feet position, the same as the physics step bit for bit, for predicting and
  replaying a walking character.
- Dedicated servers: `Game::run_server(config, setup)` and
  `ConfiguredGame::run_server(config)` run the simulation headless, one tick
  every `1 / fixed_hz` seconds of wall time, until `SIGINT`/`SIGTERM`
  (Ctrl+C and Ctrl+Break on Windows), a `StopHandle`, `max_ticks` or
  `ctx.quit()`. A stop request sets `ServerStatus::stopping` and the server
  keeps ticking for the `shutdown_grace` so systems can tell clients and
  save; a second signal stops it at once. Slow ticks catch up to
  `max_catch_up` ticks, then the backlog is dropped and counted.
  `PONKO_SMOKE` gives a smoke run, so `ponko package` checks servers too.
- `ponko::server`: `ServerConfig`, `ServerStatus` (a resource the systems
  read and the returned world keeps), `TickReport` (ticks per second, tick
  times mean/p99/max, late and skipped ticks, entities, connected peers,
  resident and peak memory, logged every `report_every`), `StopReason`,
  `StopHandle`, `LogFormat` and `log(level, event, fields)`: structured
  logs as JSON lines on standard output (or text).
- `NetMessages::connected_peers`.
- `ponko new --server` (`ponko_cli`): a dedicated-server project with an
  authoritative UDP server on `run_server` (joins, moves, state broadcasts,
  goodbyes to every client on `SIGTERM`), a bot client
  (`cargo run --example client`), tests over UDP, and a two-stage
  `Dockerfile` that runs `ponko package` and copies only the package into a
  distroless non-root image (`PACKAGE_FROM=prebuilt` uses a package made
  elsewhere).
- The guide's "Dedicated servers" chapter: the template, `run_server`, the
  logs and the tick report, the container image, a size and memory baseline
  (a 5.4 MB binary, 3.7 MB peak memory at 30 Hz with four clients, on
  macOS), production notes and the control endpoint's safety on a public
  host.
- CI: a "server template" contract in `scripts/feature-deps.sh` (the
  template's engine features, with no renderer, audio, input devices,
  models, physics or dev tools), and a "Server container" job that lints
  the Dockerfile, builds the image from the package, joins it with the
  example client and stops it with `docker stop`.
- `NetMessages::steam_sockets_host` (feature `steam`): the message protocol
  over Steam Networking Sockets. Lobby members open a connection to the
  host, relayed through Valve's network; the host refuses connections from
  outside its lobby and closes a member's connection when they leave it,
  and a dropped connection is dialed again. For hosts that run on their own
  (a headless process signed in to a Steam account) and games that want
  connections. `steam_host` (Steam Networking Messages) stays the default.
  Players join either with the same `steam_join`, which follows the host's
  choice; a Sockets host publishes its own protocol value, so older joiners
  refuse it with an error instead of timing out.
- C and C++: `ponko_net_messages_steam_sockets_host`,
  `ponko::net_messages::steam_sockets_host`.
- Guide: "Steam Networking Sockets: connections" in the networking chapter.
- Game sessions through the lobby server's relay: `Net::host_relay` and
  `Net::join_relay` run a lockstep session, `NetMessages::host_relay` and
  `NetMessages::join_relay` a message endpoint (and replication on top),
  between the members of one lobby room, with the same API as over UDP.
  The session owns the `LobbyClient` and polls it itself, also while a
  lockstep tick waits; `relay_lobby()` gives it back for its state and
  events. The scripted latency and loss conformance tests of both
  protocols run through a relay too; tested through the real server and
  over UDP on loopback. A server whose `max_relay_bytes` is under a
  session's largest packet (1504 bytes) is refused at the start.
- A TURN client on the desktop (feature `webrtc`): `RtcConfig::turn` now
  works outside browsers too, over UDP with long-term credentials
  (allocations, refreshes, permissions, Send and Data indications), so two
  desktop peers behind strict NATs connect through a TURN server such as
  coturn. `RtcConfig::relay_only` offers relayed addresses only (a
  browser's `iceTransportPolicy: "relay"`), and `RtcConfig::turn_servers`
  lists the TURN servers. A TURN server that refuses the credentials or
  stays silent is an error from `NetMessages::poll`, once.
- `LobbyLimits::trusted_proxies` and `ponko-lobby`'s `LOBBY_TRUSTED_PROXIES`:
  browser connections from a named reverse proxy count as the client
  address from its `X-Forwarded-For` header, so the limits per address
  apply per player behind a TLS proxy; the header of anyone else is
  ignored. `TrustedProxy` reads addresses and ranges. In the engine,
  `NetMessages::web_peer` gives a browser connection's `WebPeer` (its TCP
  address and header) and `WebPeer::client_ip` resolves the client
  address through the proxies a server trusts.
- The guide's "Putting a game online" chapter: one setup end to end, first
  on one machine (`ponko-lobby`, coturn and the new `online_check`
  example of `ponko_lobby` in two terminals, through the relay and through
  TURN only), then on a server with Caddy for `wss://` and coturn.
- `ponko::steam::Steam` (feature `steam`): achievements and stats (set,
  clear, store, read back), cloud saves kept in step with `ctx.storage`
  (`sync_cloud`: the newest copy wins; the data folder stays the game's
  copy), rich presence, the overlay's state (pause while it is open), and
  the text input of Steam Deck and Big Picture (`show_floating_keyboard`,
  `show_text_input`, `take_text_input`). Without a running Steam every call
  does nothing and the log says so once, so one build runs from any store.
- `Steam::fake()` and `FakeSteam`: an in-memory Steam for headless tests of
  a game's Steam logic.
- C and C++: `ponko_steam_*`, `ponko_ctx_steam_sync_cloud`,
  `ponko_ctx_steam_delete_cloud_file`; C++ `ponko::steam` and
  `ctx.steam_sync_cloud`.
- `ponko package` puts the dynamic libraries a build script made and linked
  (Valve's `libsteam_api` for the `steam` feature) beside the program.
- Guide: "Shipping on Steam" (the app id, depots from `ponko package`, what
  to check before Steam's review).
- `ponko::reflect::WorldHashParts`: `world_hash` part by part (each
  reflected component by name, each resource as `resource:Name`), with
  `total` (equal to `world_hash`) and `differing` to name the parts that
  differ between two worlds.
- A browser smoke test for replication (`web/smoke/replication.mjs`): a
  native dedicated server of the reference game (`examples/net_arena`) and
  a headless browser tab of its web build, which must join, hold its own
  player and see the replicated orbs move. `net_arena server` takes
  `--web <addr>` and prints where it listens; the web build joins the
  server its page names (`?server=ws://host:port`).
- The replication benchmark has a `1 in 20 move` scenario (5% of the
  entities change each tick, and only they are borrowed mutably) and takes
  a scenario filter (`-- 3 "1 in 20"`).
- Replicated events as raw bytes, for C and C++ games or any game that
  encodes its own: `Replication::raw_event(name)` declares one,
  `to_server_raw`, `to_client_raw` and `to_clients_raw` send its bytes
  (returning an `Error` for an undeclared name or past
  `MAX_RAW_EVENT_BYTES`), and they arrive unchanged as
  `NetEvent<RawEvent>` in `ctx.events`, reliable and in one order with
  typed events. The C and C++ calls that wrap them come next.
- `Replication::resend_stats(peer)` on the server: records sent again
  early (the acknowledgements skipped their packet) and on the timeout,
  packets called lost that were only late, the resend timeout now (in
  ticks), the timeouts in a row that doubled it, and the reordering
  tolerated. A new `#[non_exhaustive]` `ResendStats`, so `PeerStats`
  doesn't change. From C, `ponko_ctx_replication_resend_stats` and
  `PonkoReplicationResendStats`; from C++, `ctx::replication_resend_stats`.
- `NetMessages::loopback_link(players, LoopbackLink)`: the in-memory
  loopback over a link that loses a share of the datagrams and delays
  each by a fixed time plus jitter, so some overtake others; seeded, the
  same datagrams on every run.
- The replication benchmark has a scenario on a link that loses 5% of the
  datagrams each way and reorders them (`1 in 10 move, 5% lost,
  reordered`), and reports the records resent per second.
- Rollback from C and C++: `ponko_ctx_rollback_start` (with a
  `PonkoRollbackConfig` from `ponko_rollback_config_default`),
  `ponko_ctx_rollback_set_input`, `_packet`, `_receive`, `_input`,
  `_resimulating`, `_frames`, `_stats`, `_desync` and `_stop` run a
  `GameRollback` whose inputs are plain structs of up to
  `PONKO_ROLLBACK_INPUT_MAX` (32) bytes; the C++ wrapper has them as
  `ctx::rollback_*`. World snapshots too: `ponko_world_snapshot_new`,
  `ponko_world_save_snapshot`, `ponko_world_restore_snapshot`,
  `ponko_world_snapshot_len` and `ponko_world_snapshot_free` (C++
  `ponko::world_snapshot`, `world.save_snapshot`, `world.restore_snapshot`).
  See the guide's "Rollback", "From C and C++".
- Profile scopes for a `GameRollback`'s work: `rollback/save` and
  `rollback/load` (the world, the events, the change positions) and
  `rollback/save physics` and `rollback/load physics`.
- `ponko::net::Rollback` and `RollbackGame`: GGPO-style rollback netcode for
  small deterministic games. Every tick runs at once with predicted inputs
  for players whose inputs haven't arrived; a real input that differs loads
  the state saved before its tick and re-simulates. Input delay, a
  prediction window that stalls a peer too far behind, packets that repeat
  unacknowledged inputs over any transport (2–8 players in a mesh), and
  checksums of confirmed ticks for desync detection. The game saves and
  loads its own state. Rust-only (generic over the game's types).
- The guide's networking chapter has a "Rollback" section: how to use it,
  what state must be captured (and what can't be yet), measured costs of
  typed copies, `World::save`/`load` and rapier state at 100, 1,000 and
  10,000 entities, and the decision to ship it limited.
- `ponko_bench rollback/` measures them.
- Declared replication (`ponko::net::Replication`, `Game::replication`,
  `ctx.net.replication`): declare which components replicate, mark
  entities with `Replicated`, and the engine keeps every client's world in
  step with the server's. The server is the authority. Every tick it sends
  each client what changed, as deltas against the state that client
  acknowledged. Clients get `Replica` copies, spawned, updated and
  despawned for them, named by a `NetId` that is the same on every side.
  Off by default, and then free: nothing runs and nothing travels.
- `#[derive(Replicate)]` with per-field quantization
  (`#[replicate(min, max, step)]`, `angle`, `bits`, `skip`; `Quantize`,
  `Fields`, `ReplicateField`, `replicated_bits`): a position on a
  200-unit arena to the centimetre takes 15 bits instead of 32.
  `d2::Transform` and `d3::Transform` replicate and interpolate as they are.
- Client prediction: `Replication::input` and
  `Replication::predicted::<State, Input>(step)`. A client moves the
  entities it owns at once. The server applies each input once and in
  order. A disagreement is corrected by taking the server's state and
  replaying the newer inputs. Inputs and states are quantized on both
  sides after every step, so a pure step predicts exactly.
- `Replication::interpolated` shows a component between the values
  received, 0.1 s in the past (`interpolated_with` for your own
  `SnapshotConfig`).
- Reliable events both ways: `Replication::event`, then `to_server`,
  `to_client` and `to_clients`. They are read as `NetEvent<T>` (sender,
  tick, event) from `ctx.events`.
- A bandwidth budget per client (`Replication::budget`, 64 KB/s by
  default, `DEFAULT_BUDGET`). When changes don't fit, entities wait in
  priority order (`Replicated::with_priority`), and a client's own
  entities are always sent.
- Interest management: `Replication::interest` with distance rings and
  rates, `Replication::position`, `Replication::set_viewer`, and
  `Replicated::global`. An entity that leaves a client's interest is
  despawned there.
- Ownership (`Replicated::owned_by`, `set_owner`, `Replica::is_mine`), with
  authority epochs so a handoff drops the old owner's predictions.
  `Replicated::set_global` sets the interest flag on an existing marker.
- A schema check on join: a client whose declarations differ is refused,
  with a reason (`Replication::error`).
- Statistics: `Replication::peer_stats` (bytes, packets, records, deltas,
  deferred entities, losses per client) and `Replication::client_stats`
  (received, applied, stale packets, confirmed and corrected predictions).
- `NetMessages::game_clock`: an endpoint the game polls counts the game's
  time instead of the wall clock, so headless tests that run faster than
  real time still see resends and timeouts.
- Orb Arena (`examples/net_arena`), a multiplayer reference game built
  only on replication. It has a listen server, a dedicated server
  (`net_arena server`), clients (`net_arena join <addr>`) and browser
  clients over WebSocket. `tests/net_arena.rs` plays it headless with a
  dedicated server and two bots over a lossy network.
- The replication benchmark: 1,000 entities to 16 clients
  (`cargo run --release -p ponko_bench --bin replication`), with bytes per
  client and server CPU per tick.
- C and C++: `ponko_ctx_replication` declares components field by field.
  Further calls mark entities, read the role, net ids, replicas, clients
  and statistics, and set viewers: `ponko_ctx_replicate`,
  `ponko_ctx_replication_status`, `ponko_ctx_net_id`,
  `ponko_ctx_net_entity`, `ponko_ctx_replica`,
  `ponko_ctx_replication_clients`, `ponko_ctx_replication_set_viewer`,
  `ponko_ctx_replication_peer_stats`,
  `ponko_ctx_replication_client_stats`. `ponko_game_messages` gives a
  game its endpoint, and `ponko_net_messages_game_clock` switches it to
  game time. The C++ wrapper has `ctx::replicated<T>`,
  `ctx::replication`, `ctx::replicate` and friends. Inputs, prediction,
  events and interpolation are Rust-only for now.
- Guide: a new "Multiplayer: pick a model" entry page and a "Replication"
  chapter (declare, mark, host and join, prediction, events, interest and
  the budget, testing, the arena, costs, checks for a person). Lockstep,
  rollback and application messages are now one chapter under it, and so
  are dedicated servers.
- Peer-to-peer message endpoints over WebRTC data channels (feature
  `webrtc`): `NetMessages::host_rtc` and `NetMessages::join_rtc`, with
  `RtcConfig` (STUN servers, TURN in browsers, the desktop UDP socket) and
  `Signaling` (any channel carries the setup messages). Browser to browser,
  browser to desktop and desktop to desktop, with the usual reliable and
  unreliable lanes. The desktop side uses str0m on one UDP socket polled
  with the endpoint; the browser side uses `RTCPeerConnection`.
- `LobbyClient::signaling`: the lobby server's relay carries the WebRTC
  signaling between room members.
- `ponko::net::stun`: STUN binding requests and responses (RFC 8489).
- `ponko-lobby` answers STUN requests with `LOBBY_STUN` (rate limited per
  address), and `ponko_lobby::StunResponder` does it in a server of your own.
- C and C++: `ponko_net_messages_rtc_host`, `_rtc_join` and `_rtc_peer`
  (`lobby::rtc_host`, `lobby::rtc_join`, `net_messages::rtc_peer`), with
  `ponko_ffi` built `--features webrtc`.
- The guide's "Peer to peer over WebRTC" chapter, with deployment, the NAT
  cases, the library comparison and what was and wasn't tested.
- A reference lobby, matchmaking and relay server, `ponko-lobby` (the new
  `ponko_lobby` crate): rooms with six-character codes, join and leave,
  matchmaking queues that fill rooms by size, host migration, and a relay
  that forwards opaque bytes between members of a room for players who
  can't connect directly. Limits per address (requests and relay bytes per
  second, connections), size caps (requests, relay payloads, rooms, room
  text) and idle timeouts for connections and rooms. It runs in
  `Game::run_server` (JSON logs, clean stop on `SIGTERM`, `ponko package`
  smoke run) and has a container recipe (`crates/ponko_lobby/Dockerfile`).
  The library, `LobbyServer`, takes a game's own `Matchmaker` and
  `HostMigration` rules. Guide: "Lobbies, matchmaking and relays".
- `ponko::net::lobby` (feature `lobby`, off by default, no dependencies):
  `LobbyClient` with its events and the documented wire protocol, over UDP
  or WebSocket. C: `ponko_lobby_*`; C++: `ponko::lobby`.
- `NetMessages::peer_addr`: the address a connected peer's packets come
  from.
- A load test of the lobby (`cargo run --release -p ponko_lobby --example
  load_test`): 1000 rooms and 3300 clients in about 5 s, headless.
- `World::save_snapshot` and `World::restore_snapshot` (`ponko::ecs_core::WorldSnapshot`):
  a typed in-memory copy of every component column, the entity allocator,
  the archetype layout and the resources registered with
  `World::snapshot_resource`. A restore keeps entity ids and which ids later
  spawns get, and queries visit entities in the same order. 10,000 entities
  save and restore in about 0.005 ms each, with no allocation after the
  first save. Components must be `Clone` (the derive notices;
  `World::register_clone` for the others).
- `save_state` and `restore_state` on `ctx.physics` and `ctx.physics3d`
  (`d2::PhysicsState`, `d3::PhysicsState`): the whole rapier state with the
  links to entities. A loaded world steps bit for bit like the original.
- `net::GameRollback`: rollback for a whole `Game`. Started with
  `ctx.net.start_rollback`, it runs every simulation tick through a
  `Rollback` session and, when a late input differs from the prediction,
  loads the world, the physics, the events and the tick saved before that
  tick and runs the simulation systems again. Simulation systems read
  `ctx.net.rollback_inputs`; `ctx.net.is_resimulating` tells a tick that
  runs again, whose sounds are dropped. Checksums are world hashes. See the
  guide's "Rolling back a whole `Game`".

**Web, mobile and platforms**
- The release builds and tests the C library for Windows on Arm on a
  GitHub-hosted Windows on Arm runner (`release-matrix/windows-arm64/<version>`
  in the release-time CI), and `c-library.py smoke --docker` runs a Linux
  archive's C and C++ tests in a container whose glibc is the archives' 2.28
  floor, so the `linux-arm64` library is checked by running it.
- C library downloads for three more platforms: Intel Macs
  (`macos-x86_64`), 64-bit Arm Linux (`linux-arm64`, glibc 2.28 or newer)
  and Windows on Arm (`windows-arm64`), next to `linux-x86_64`,
  `macos-arm64` and `windows-x86_64`. Same layout as before; the C/C++
  chapter lists them. `scripts/c-library.py` builds and smoke-tests each
  archive (CONTRIBUTING.md, "The C library archives").
- `ponko package` (`ponko_cli`) turns any game into what players download,
  with one command in its folder: a release build, its assets and license
  files, `THIRD-PARTY-NOTICES.txt` for every linked library, laid out and
  archived for the platform (a Windows zip, a macOS `.app` in a dmg, a Linux
  tar.gz, or with `--web` a folder for any static host), then a headless smoke
  run of the packaged copy from an unrelated folder. The game's
  `[package.metadata.ponko]` table (name, id, icon, assets, files; per example
  under `examples.<name>`) says what goes in; projects from `ponko new` have
  one. `--sign <program>` is the hook for signing and notarization.
  `ponko notices` writes the notices for a library alone.
- `PONKO_SMOKE=<ticks>`: `Game::run` opens no window, runs that many ticks
  headless (frame systems included), prints `ponko: smoke run passed` and
  returns, so a packaged game can be checked on a machine without a display.
- A game started from a macOS app bundle also finds its assets in the
  bundle's `Contents/Resources/assets`.
- A web gallery of the examples, playable in the browser: each release
  publishes the examples that run on the web
  as a gallery, a page each with the demo, what it
  shows, its controls, whether it needs WebGPU and its source at the release.
  A demo that needs WebGPU says so, and what to try, on a browser without it;
  the others fall back to WebGL2. `node web/gallery/build.mjs` builds it
  locally, `web/gallery/examples.json` lists the examples (one line each), and
  `node web/smoke/gallery.mjs` checks every page in a headless browser. Linked
  from the README, the guide's web chapter and the release notes.
- Multiplayer in the browser: a web build joins a session over WebSocket
  with `Net::join_web` (lockstep) or `NetMessages::join_web` (messages), and
  a host takes browser players next to its UDP players with `accept_web`
  (`web_addr` says where). The host side is `tungstenite` on std sockets,
  polled from the game thread like UDP; plain `ws://`, so pages served over
  `https://` need a TLS proxy for `wss://`. A desktop game can join over
  WebSocket too.
- The lockstep example plays with up to 8 players, takes browser players
  with `--web <port>`, joins with `join-web <url>` or, as a web build, from
  its page's `?server=`, and plays headless with a bot (`--headless <ticks>`).
- `web/smoke/multiplayer.mjs`: two headless-browser tabs, a native host and a
  native UDP client play the lockstep example together and must reach the
  same world; part of CI's browser smoke job.
- The web threads fixture's benchmark has a `boids` workload: one tick of
  the boids example's flocking system over its 20,000 boids
  (`node web/smoke/threads.mjs --bench --workloads boids`). The guide's
  "Threads on the web" gives its tick time with threads and without: 1.4 ms
  against 9.7 to 9.9 ms in headless Chromium on a 14-core Apple M4 Max.
- The local check list's `web-gpu` job gates WebGPU on a real GPU: on a Mac
  it runs the 2D and 3D web smoke fixtures in headless Chromium on the
  machine's GPU (Metal) and fails when WebGPU is unsupported, falls back to
  WebGL2, or runs on a software renderer or a fallback adapter.
- `web/smoke/smoke.mjs --require-gpu`, the gate's check, and
  `web/smoke/frame-times.mjs`, which measures the 3D fixture's frame times
  (median and 95th percentile, uncapped or at the display's rate) on WebGPU
  and WebGL2 with the engine's CPU and GPU split. The web guide's new "Frame
  times" section has the numbers.
- Android: the engine builds for `aarch64-linux-android` (the other Android
  ABIs package too, untested) and runs full screen as a native activity, with Vulkan or,
  where wgpu picks it or `WGPU_BACKEND=gl` asks for it, OpenGL ES.
  `ponko package --target android` makes an `.apk` (Android 11, API 30, or
  newer) with the game's library, its asset pack, notices and launcher icons,
  aligned for 16 KB pages and signed with the Android debug key, or left
  unsigned for the `--sign` hook. The `orientation` key sets the
  orientations. The SDK, build tools and NDK are found through
  `ANDROID_HOME`. See the guide's new Android chapter for the toolchain, the
  release-signing steps and what has been checked where.
- On Android: suspend and resume release and recreate the GPU surface;
  `safe_insets` and `keyboard_inset` come from the system bars, cutouts and
  the on-screen keyboard; the game takes audio focus (none for `Ambient`),
  gives it back while away, and holds its sound during a call; storage,
  the clipboard and crash reports work; debuggable builds read `PONKO_*`,
  `WGPU_*` and `RUST_*` intent extras as environment variables.
- `WindowEvent::BackRequested`, `WindowService::back_requested` and
  `WindowService::cancel_back`: the system Back button or gesture as an
  event. By default the app leaves after the frame; a game that cancels it
  handles Back itself.
- C and C++: `PONKO_WINDOW_EVENT_BACK_REQUESTED`,
  `ponko_ctx_window_back_requested` and `ponko_ctx_window_cancel_back`, with
  their C++ methods.
- `scripts/android-emulator.sh`, which runs the engine's unit tests, the
  phone check and a reference game on a headless emulator, and lints the
  Android target with `--clippy`; an `android` job in `scripts/checks.yml`.
- iOS: the engine builds for `aarch64-apple-ios` and `aarch64-apple-ios-sim`
  and runs full screen with Metal, its window placed in the app's scene (the
  iOS 27 SDK requires one). `ponko package --target ios` makes an app bundle
  in an `.ipa` (signed by the `--sign` hook), `--target ios-sim` one for the
  simulator; the `orientation` key (`any`, `landscape`, `portrait`) sets the
  orientations, and the icon is scaled to the sizes iOS asks for. See the
  guide's new iOS chapter for the device signing steps and what has been
  checked where.
- `WindowEvent::Suspended`, `Resumed` and `LowMemory`, and
  `WindowService::suspended`: a phone's app lifecycle. No frames run while
  suspended; the suspend is read in one last frame.
- `WindowService::keyboard_inset` (with `feed_keyboard_inset` for tests): how
  much of the window the on-screen keyboard covers.
- `Audio::set_session_category` and `AudioSessionCategory` (`SoloAmbient`,
  `Ambient`, `Playback`): how the game's sound mixes with other apps' and
  whether the silent switch mutes it, on iOS; kept elsewhere.
- C and C++: `PONKO_WINDOW_EVENT_SUSPENDED`, `_RESUMED`, `_LOW_MEMORY`,
  `ponko_ctx_window_suspended`, `ponko_ctx_window_feed_lifecycle`,
  `ponko_ctx_window_keyboard_inset`, `ponko_ctx_window_feed_keyboard_inset`,
  `PonkoAudioSessionCategory`, `ponko_ctx_audio_set_session_category` and
  `ponko_ctx_audio_session_category`, with their C++ methods.
- `examples/phone.rs`, a phone check, and `scripts/ios-simulator.sh`, which
  runs the engine's iOS unit tests and the phone check on the simulator
  headlessly; an `ios` job in `scripts/checks.yml`.
- The 3D reference game plays its walk-through in the browser:
  `?smoke=walkthrough` in the address of the `shard_grove` web build, and
  the web smoke runner's new `--until <field>:<value>` (fixture fields
  `until`, `untilSeconds`) waits for a game playing itself to get there.
  Its web state also reports clicks, the focused menu button, the tally
  and the engine's adapter.
- A `web-examples` job in `scripts/checks.yml` (slow, a Mac with its GPU):
  every gallery example built for the web and played in headless Chromium
  on WebGL2 and WebGPU and in WebKit, and Shard Grove's keys and
  walk-through on WebGPU and WebGL2.
- Twelve more examples in the web gallery: `skeleton2d`, `ai_guards`,
  `prefabs`, `move_sprite`, `ui_widgets`, `sky_kit`, `stylized_sky`,
  `reflection_refresh`, `post_volumes`, `split_screen`, `crowd` and
  `water`. The gallery's README names every example left out and why, and
  a test keeps it so.
- `ponko package --target android --format aab` makes an Android App
  Bundle, what Google Play takes, with Google's `bundletool` (`BUNDLETOOL`
  names its jar, else `bundletool` on the `PATH`): the library and asset
  pack stored and the library on 16 KiB pages in the apks made from it,
  signed with the debug key or by the `--sign` program (`archive <path>`,
  as for the apk). The Android guide has the release-signing and Google
  Play upload steps; the emulator check installs the phone check from its
  bundle.
- `PONKO_MUTE=1` makes the audio output play silence, for automated runs
  on a machine or phone someone is using; the stream, mixer and music
  clock run as usual. See the guide's Audio chapter.
- `PONKO_IOS_ORIENTATION=landscape` (or `portrait`) asks an iOS app's
  scene to turn as the game starts; the simulator check uses it to check
  the safe area turned.
- Web games report failures that stop them after startup: a panic (in setup
  or later), the WebGL2 context lost while playing, and a worker thread that
  stopped. Pages subscribe with `onPonkoFailure` in the generated
  JavaScript; the callback gets the failure's `kind`, `message`, a panic's
  `location` and the game's own `text`.
- `ponko::web::on_failure` lets a web game write the text the page shows
  for such a failure (`ponko::web::Failure`, `ponko::web::FailureKind`).
- `web/ponko-isolate.js`: a service worker that makes a page cross-origin
  isolated on hosts that can't send the headers (GitHub Pages, GitLab
  Pages), so a web build with threads starts its workers there. It adds
  the two headers to the game's own responses and reloads the page once on
  the first visit; where it can't work (no secure context, no service
  workers, inside another site's frame) it says why and the game runs on
  one thread. `web/build.sh --threads --isolation-worker` and
  `ponko package --web --threads --isolation-worker` add it. The guide's
  "Threads on the web" says when to use it and what it costs, and its
  hosting table now says which common hosts work with headers and which
  with the service worker. `web/smoke/threads.mjs --case service-worker`
  checks it in a browser served without the headers.
- "Threads on the web" lists every parallel loop of the engine, where it
  runs in a web build with threads, and its time in the browser fixture
  with 13 threads and on one; `threads.mjs` prints that table, and its
  fixture now also draws 2D skeletons, walks 3D characters and bakes light.
- Guide: "VR and XR: where the engine stands" (`book/src/xr.md`), the
  headset evaluation: what stereo drawing shares between the eyes today
  (shadows, skinning, global illumination) and what runs per eye (culling,
  every camera pass), the frame timing and input a headset runtime asks
  for, the platforms that could be tested, and the decision (later),
  recorded in `ROADMAP.md`.
- An offscreen stereo prototype (`src/render/stereo_render_test.rs`): the
  3D reference game's level drawn for two eyes through two views, with
  tests for the eyes' disparity and the shared shadow cascades, and an
  ignored benchmark of a stereo frame against one view.
- Controllers in web builds with threads (`web/build.sh --threads`): the
  engine reads `navigator.getGamepads()` on the page's thread every frame,
  copies the buttons and axes into plain numbers and turns the changes into
  its gamepad events, so it keeps no browser object that would have to be
  sent between threads. `ctx.input.gamepads()` and the rest of the gamepad
  input work as in the ordinary web build (no rumble on the web in either).
- `web/smoke/threads.mjs --case gamepads` feeds the threaded build fake
  browser gamepads in headless Chromium and checks each step reaches the
  game; `--case warning` proves a warning fails the threaded build.
- `web/build.sh --print-env` prints the toolchain and the flags a threaded
  build would use, without building.
- Web: a WebGPU device lost while the game runs (the browser's GPU process
  crashed or was reset, or the device was destroyed) now stops the game
  with its own failure kind, `ponko::web::FailureKind::DeviceLost`, and
  reaches the page through `onPonkoFailure` (`kind: "DeviceLost"`), as a
  lost WebGL2 context does. Its message carries the browser's reason. The
  generated page shows it, or the game's own text from
  `ponko::web::on_failure`.
- `web/smoke/runtime-failures.mjs` has a `device-lost` case: it destroys
  the game's `GPUDevice` while the game runs and checks the page. It takes
  `--chromium-mode`, `--browser-arg` and `--allow-unsupported` like
  `smoke.mjs`; where the browser has no WebGPU the case is unsupported.
- Rumble in web builds where the browser has it: the engine plays the two
  body motors through the pad's `vibrationActuator` (`playEffect` with
  `dual-rumble`), in the ordinary web build and in builds with threads. A
  browser ends an effect after at most 5 seconds, so the engine plays short
  effects and plays the same levels again every second until the rumble
  ends; a stalled game stops buzzing within 2 seconds. `GamepadInfo`
  reports `capabilities.rumble` for a pad that has an actuator. Trigger
  motors are not used.
- `web/smoke/threads.mjs --case gamepads` feeds pads with a fake
  `vibrationActuator` and checks the game's rumble reaches it (the levels,
  played again before the cap, stopped at the end); `--case gamepads-plain`
  does the same on the ordinary build, whose pads come from gilrs.
- `web/build.sh --threads --clippy` runs clippy on the threaded web
  configuration with the toolchain and flags of a threaded build.
- Trigger rumble in web builds where the browser has it: a pad whose
  `vibrationActuator` lists the `trigger-rumble` effect (in `effects`, or
  through the older `canPlayEffectType`) reports
  `capabilities.trigger_rumble` and plays the trigger levels with the body
  motors' as one `trigger-rumble` effect, replayed before the browser's cap
  like body rumble. On any other pad the trigger levels are dropped and the
  body motors still play (`dual-rumble`), as on the desktop; a rumble of the
  triggers alone plays nothing there.
- The `wasm-simd` feature: rustfft's WASM SIMD FFT for the water kit's CPU
  wave spectrum in web builds, to be built with
  `-C target-feature=+simd128` (it refuses to compile without: rustfft's
  SIMD code alone made a moment three times slower). In headless Chromium
  a new moment is then 1.4 to 1.6 times faster (0.16 instead of 0.24 ms at
  128 cells, 0.72 instead of 1.12 ms at 256).
- `web/smoke/spectrum.mjs` and the fixture `examples/web_spectrum.rs`: the
  CPU wave spectrum timed in a headless browser at 64, 128 and 256 cells,
  plain, with `simd128` alone and with `wasm-simd`. The water guide's "On
  the web" has the numbers and the web's default (128 cells, about 0.6 ms
  of a WebGL2 frame at worst).
- `web/smoke/threads.mjs --case gamepads` (and `gamepads-plain`) also feed
  two pads, one whose actuator lists `trigger-rumble`, and check that a
  rumble with trigger levels reaches each as the right effect.
- The editor's web build shows thumbnails in the asset panel, made from
  the page's files in memory (fetched, packed or saved in the browser)
  instead of paths on disk.
- **File > Saved in this browser** in the editor's web build lists the
  files kept in the browser, with **Revert** (back to the build's file, at
  once and after a reload) or **Delete** (a file only the browser has).
  `Assets::browser_saves` and `Assets::revert_browser_save` do the same
  from code; other builds list nothing.
- `RUSTFLAGS="--cfg ponko_web_thread_heaps"` gives a web build with
  threads one heap per thread (talc, through `orx-parallel-wasm-allocator`)
  instead of the standard allocator's one lock, and builds world saves on
  the workers. Experimental and not the default: in headless Chromium it
  made a JSON save on the workers 18 times faster (38 ms instead of 700
  ms) and a parallel query that allocates on every item 40 times faster,
  but allocation on the page's thread 10 to 50% slower (the save there
  56 ms instead of 41 ms). The ordinary web build and native builds ignore
  the flag. The guide's "Threads on the web" has the numbers.
- `web/smoke/threads.mjs --case thresholds` times the hierarchy's update,
  the 2D skeleton drawing and the 3D characters' moves at several sizes,
  on the page's thread and shared out, and checks each on both sides of
  its web threshold (the same result as on the page's thread alone);
  `--case allocator` compares the standard allocator with one heap per
  thread. Both are in the check list.
- Threads on the web: a web build with threads (`web/build.sh --threads`,
  nightly Rust) runs work on Web Workers that share the game's memory, on
  a page served with `Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`. What runs there:
  - parallel queries (`par_for_each` and the other `par_*` calls), the
    parallel scheduler's batches and the ECS's parallel sorts;
  - `Job`s, on workers of their own, which carry asset groups' and
    streaming's reads and decodes.
  On a page without the headers the same build runs on the page's thread,
  with a console warning that names them. The ordinary web build is
  unchanged.
- `Game::web_threads(n)` chooses how many frame threads a web build
  starts: by default one fewer than the browser's cores, at most 16; 0
  or 1 means none. `ponko::web::threads()` says how many it runs on.
- `ponko package --web --threads`:
  - builds the web package with threads;
  - adds `ponko-worker.js`, and files that send the two headers on
    Netlify and Cloudflare Pages (`_headers`), Vercel (`vercel.json`) and
    Apache (`.htaccess`).
- `web/smoke/threads.mjs` checks a threaded build in a headless browser:
  - work goes to workers on an isolated page;
  - it falls back to one thread on a page that isn't isolated, with the
    same results;
  - a worker's panic stops the game.
  `--bench` times the simulation workloads with 1 and N threads.
- A guide chapter, "Threads on the web": the build recipe and what the Rust
  toolchain and wasm-bindgen need, the headers for each host, the limits
  and the numbers.
- The web guide's "WebGPU first, WebGL2 as the fallback": the order, the
  console lines, and a table of what each backend draws, generated from the
  GPU tier code and checked by a test (`PONKO_UPDATE_BOOK=1` rewrites it).
- `web/smoke/webgpu-fallback.mjs`: each way into the WebGL2 fallback, and
  the WebGPU path itself, in a headless browser, with the time to the first
  frame on each.
- C and C++ games build for the browser: `web/build-cpp.sh` compiles them
  with the WASI SDK's clang (33 or newer, for C++ exceptions) and links them
  into the engine's ordinary web build, writing the same static site as
  `web/build.sh` (the web guide's "C and C++"). In a browser
  `ponko_game_run` doesn't return while the game plays, and nothing in the
  calling functions is destroyed, so locals the systems capture by
  reference stay valid as on the desktop. `printf` and `std::cout` go to the
  browser's console. `web/smoke/cpp.mjs` checks the guide's first C++ game in
  headless Chromium.
- Web games expose their startup result through `ponko::web::on_startup` in
  Rust and `onPonkoStartup` in the generated JavaScript. Startup errors include
  the engine's error kind and message.

**Tools: editor, inspector and agents**
- `ponko_cli`, a new crate with the `ponko` command: `ponko new <path>
  [--2d | --3d | --headless]` makes a game project from a template, against
  the released engine or, with `--path <checkout>` / `--git <url>`, another
  one. Each project has the engine features for its kind, a fast debug
  profile, an `assets/` folder, headless tests, an `.mcp.json` for
  `ponko-mcp` and an `AGENTS.md` for coding agents. The templates
  (`crates/ponko_cli/templates`) also work with `cargo generate`. The check
  list's `templates` job builds and tests each one against the current
  engine. Getting started uses it.
- An agent acceptance suite for the MCP tools: six game-development tasks
  (tuning, building a level, a seeded bug, a tick-time regression, a broken
  shader, a hidden HUD label), each with a starting project, a goal, a
  checker and negative controls. `cargo test -p ponko_mcp --test
  agent_suite` replays scripted solutions with no model and compares the
  scores with a published baseline; `cargo run -p ponko_mcp --example
  agent_suite -- mcp TASK` serves one task to a real agent over MCP, with a
  `submit_task` tool and a result file. See "The agent acceptance suite" in
  the guide's agents chapter.
- The opt-in `web-control` feature and `ponko-mcp --web-bridge` let agents inspect, step and screenshot a game running in a local browser tab with the existing MCP tools.
- `scripts/checklists/`: checklists and record sheets for the checks that need
  a person: hardware checks for Linux, macOS and Windows, a newcomer test kit,
  input method and screen reader checks, a low-end and Steam Deck benchmark
  script with its sheet, and a Linux full-run guide with a helper
  (`record_run.py`) that turns `run-checks.py`'s result into a record.
- `ponko_bench` rows `physics3d/heightfield-257-copy-x20/*`: what a mirrored
  or stretched heightfield copy costs when its tile enters physics (about
  0.07 ms for 257 × 257 heights).
- `PONKO_RENDER_SSR_THIN` in the render scenes bench, and GPU tests holding a
  steady 3D frame's allocations to a budget and to the same count for ten
  times the models.
- Games can register typed commands and variables in a retained developer
  console with help, completion, history and scrollback. Control and MCP
  use the same registry. C and C++ can register commands and variables.
- The agent acceptance suite records time: a real agent's result file has
  the seconds to its last submission and when each tool call came, and
  `agent_suite scores FILE...` prints the runs as a Markdown table for a
  release pull request.
- `scripts/mcp-session.py` drives a stdio MCP server one tool call per shell
  command, so an agent with a shell but no MCP client can try the suite.
  The guide's agents chapter has the procedure for a release ("At a
  release"), and the first real-agent run is recorded in the macOS check
  results.
- The guide's chapter "The `ponko` command" (`book/src/cli.md`) lists every command and option `--help` shows;
  a test of `ponko_cli` fails when one is missing there or isn't accepted, and runs `pack`, `cook`,
  `textures`, `new`, `editor --print` and `notices` for real in a temporary folder.
- Headless tests open every menu of the editor, list its entries and use each one, and use every
  button of the menu bar and the gizmo row, every entry of a hierarchy row's menu, every shortcut,
  and every section and button of the overlay inspector (in an empty game too): a new entry that
  does nothing fails them. What only a person can judge is in
  `scripts/checklists/editor-and-inspector.md`.
- Five harder tasks in the agent acceptance suite: a bug with two causes
  where fixing the visible one leaves the symptom (`lost-points`), a value
  that has to be measured in the game (`cannon-range`), one change across a
  scene, a prefab and a data file (`tough-bats`), a choice that needs
  `save_world` and `load_world` to try alternatives (`hold-the-gate`), and a
  code task, a Rust game crate rebuilt once per attempt (`stuck-guard`).
  Each has negative controls, among them "fixed only the visible cause".
- Each suite task names the tools it leans on (`agent_suite list`), and
  `agent_suite mcp TASK --hide TOOL,...` takes tools away from a real
  agent's run, to compare tool surfaces.
- The inspector has a **Debug drawing** section that shows and flips debug
  drawing, each built-in view, the colliders view's distance and every
  toggle added with `Game::debug_toggle` (open by default when the game has
  toggles).
- Target cameras that draw particles and show the default layer draw
  `ctx.debug`'s 3D lines and depth-tested labels into their render target,
  against their own depth, from the same line buffer as the main camera.
- Editor: **Apply to prefab** works on an instance of a prefab that includes
  other prefabs, writing each included prefab back as its link with how
  this instance changes it, and a selected part of an included prefab has
  **Apply to *name***, which writes it into the included prefab's own file;
  every instance in the scene using it follows, in one undo step.
- Prefab files: an included prefab's entry takes `"remove"` (components its
  root doesn't get) and `"parts"` (changes to its other entities, by their
  place in its `PrefabParts`) beside `"components"`.
- glTF scenes keep how their nodes were edited (moved, components changed,
  added or removed) as overrides in the scene's saved record, put back when
  the file is exported again (hot reload) and when a loaded save spawns the
  file again; the editor saves them with the scene and spawns a scene's
  glTF files again when it opens it.
- Editor: model thumbnails are drawn by the engine's renderer on the game's
  GPU (offscreen, one a frame, the software drawing first) and kept in the
  user's cache folder under a hash of the model and the files it names
  (`PONKO_THUMBNAIL_CACHE` names another folder).
- Editor: a texture dropped on a model in the 3D view paints it; a
  `ModelMaterial` can be `{ "texture": "path" }`, a material of that image
  made when the scene or prefab naming it loads.
- Editor authoring tools, all through the editor's undo history and saved in
  the formats the runtime loads:
  - a **Particles** section in the inspector for 2D and 3D `ParticleEmitter`s:
    every setting, a size curve and a colour gradient, and a live preview
    while the game is paused;
  - **Author → Tilemap…**: paint, erase and autotile Tiled `.tmj` maps from
    a tileset palette, with the autotile rule kept as a layer property;
  - **Author → Animation graph…**: a 2D skeleton's states and transitions as
    a graph, edited in place, with a preview driven by test parameter values;
  - **Author → Materials…** and **Shader…**: glTF material values and
    textures edited live, material shaders edited with their errors shown at
    the line, custom values edited live with **Copy as Rust**;
  - **Author → Terrain…**: raise, lower and smooth brushes and painted
    ground layers.
- A terrain file format (`"format": "ponko-terrain"`, JSON with a 16-bit
  height PNG and RGBA layer-weight PNGs) and `TerrainTile::spawn_from_file(ctx, path, at)`,
  which spawns it as a `TerrainTile`.
- Crash reports for shipped games (`ponko::crash`, the guide's "Crash
  reports"): a panic or a fatal engine error writes a text report into
  `crash-reports/` of the game's data folder (`ctx.storage`): the message,
  location and backtrace, the game's and engine's versions, the OS, the GPU
  and its driver, the graphics settings of the last frame and the last 100
  lines the game printed, with every path made relative to the game's
  folders. `Game::crash_reports(true | false | CrashReports)` is the one
  setting; `CrashReports::on_crash` runs the game's own code with each report
  (its own message, or sending the file: the engine sends nothing),
  `crash::saved_reports` lists earlier runs' reports, and `crash::report`
  writes one for a game's own fatal error. The newest ten are kept.
- Web builds hand a report to the page as a `ponkocrashreport` event (and
  `window.ponkoCrashReport`); the generated page offers it as a file to save.
  The failures that stop a running browser game (a lost WebGL2 context, a
  stopped worker thread) write a report too.
- Allocation-free inspection: `StringTable::iter_keys`, `Strings::iter_languages`, `SchedulePlan::iter_stages_in` and `iter_names` borrow instead of collecting.
- Reusable inspection output with replace semantics and retained capacity: `Graphics::loaded_materials_into`, `mesh_costs_into`, `streamed_assets_into`, `asset_handles_into` and `Audio::asset_handles_into`.
- `DrawSpace { World, Screen }` and `DrawParams::with_space` name the coordinates of sprite draws. It is the same type as `ponko::debug::DrawSpace` (now defined once, public at `ponko::debug::DrawSpace` and in the prelude). C: `PonkoDrawSpace` and `ponko_draw_params_with_space`; C++: `ponko::draw_space` and `ponko::with_space`. The `PonkoDrawParams` layout is unchanged.
- A crash that is not a panic (`CrashKind::Native`) now has a backtrace
  (the guide's "Backtraces of a native crash"). The crash handler writes raw
  addresses with each frame's module and offset, still only
  async-signal-safe work: on Linux and macOS a walk of the frame records
  from the crashed thread's registers, bounds-checked against the thread's
  stack and read through a pipe so a broken chain can't fault again; on
  Windows the system's table-driven unwinder. The loaded modules (range,
  build id, path) are recorded ahead of time and refreshed off the handler.
  The next start names each frame whose binary is still the same (same path
  and build id) with its function, file and line; the others keep module
  and offset for `addr2line`, `llvm-symbolizer` or `atos`. Deeper than the
  crashing frame needs frame pointers on Linux (`-C force-frame-pointers=yes`);
  macOS keeps them always.
- New dependency on desktop: `backtrace` (rust-lang), to resolve addresses
  the engine didn't capture itself (std has no public way to).
- The render-pass inspector (`perf.get`'s `render_inspector`) has
  `gpu_ms.not_timed`: the names of the passes that ran in the latest timed
  frame but couldn't be timed, as `perf.get`'s `gpu_passes_not_timed` already
  listed them. They are not in `gpu_ms.passes` as 0 ms. It is `[]` when every
  pass was timed, and `null` (with a reason in `unavailable`) when nothing
  could be timed.
- The runtime inspector and the editor's Performance section list each GPU
  pass's time, and show a pass that ran but couldn't be timed as "not timed"
  instead of 0 ms.
- A glTF material can name its custom material shader and values in its
  `extras` (`"ponko:shader"`, relative to the file, and `"ponko:params"`:
  up to four values, each with a name, a type, `float`, `vec2`, `vec3`,
  `vec4` or `color`, and its numbers, `material.custom[0]` to `[3]` in
  order). Loading the file draws the material with them; the editor's
  materials window edits them in a **Custom shader** section and saves them
  in the file.
- Terrain tiles in scenes: `ponko::terrain::TerrainFile`, the file a tile
  is made from, is reflected, and `TerrainTile::spawn_from_file` adds it.
  A scene saves the file and the tile's corner; loading it makes the tile
  from the file (a missing file stops the load with the world unchanged),
  and keeps a live tile from the same file at the same corner. The terrain
  window has **Add to the scene**.
- C and C++: `ponko_ctx_terrain_tile_spawn_from_file` and
  `ponko::spawn_terrain_from_file`.
- The animation graph window saves where its nodes are with the graph, as
  the skeleton file's `graph.layout`, which the runtime ignores; a file
  without one opens with its nodes in rows.
- Code mods (feature `mods-wasm`, off by default): a mod's manifest may name
  a WebAssembly module (`"code": "frost/crawl.wasm"`), run by the wasmi
  interpreter on every platform the engine builds for, the web included.
  `ModSet::mount` compiles and starts every module before mounting anything;
  the game calls exported functions with `ponko::mods::with_code`
  (`ModCode::function`, `exporting`, `call`, `take_log`), passing and
  getting `ModValue` numbers without allocating. Modules import only the
  host functions `log`, `dt` and `cast_ray` (2D physics); each call gets
  `CodeLimits::fuel_per_call` fuel and a module's memory grows to
  `CodeLimits::memory_bytes` at most (`ModSet::code_limits`). A trap is a
  `ModProblem::Trap`, a module that can't be used `ModProblem::Code`; a mod
  with code is refused by a game built without the feature. The module's
  bytes are part of `Mods::fingerprint`. Measured on the frost drip's
  crawl (`ponko_bench -- mods/`): 204 ns per drip as a code mod, 98 ns as
  the game's Rust rule.
- Mounting mods checks every asset their prefabs and scenes name
  (textures, skeletons, meshes, collision meshes, included prefabs, fields
  registered with `Assets::scene_asset_field`) and refuses a mod with
  missing ones in one `ModProblem::MissingAssets` listing each with the file
  that names it, instead of failing at first use.
- C and C++: `ponko_ctx_mount_mods_from_memory` (`PonkoModBytes`) and
  `ctx.mount_mods_from_memory` mount mod packs held in memory.
- The guide's "Mods" chapter: code mods, their sandbox, which backend runs
  where and what it costs.
- Code mods reach the game, only where it lets them: new host functions
  `get_field` and `set_field` (a number or bool field of a reflected
  component, `"Health.current"`, `"Transform2D.position.1"`),
  `spawn_prefab` (at a 2D position) and `random` (`[0, 1)`). They reach only
  the entities the game exposed with `ModCode::expose` (taken back with
  `hide`, `hide_all`) and the prefabs it exposed with
  `ModCode::expose_prefab`; a spawned prefab's root is exposed for its mod.
  `random` draws from each mod's own generator, seeded with
  `ModCode::seed_random` (same seed, same sequence). Refusals return
  `REFUSED_ENTITY`, `REFUSED_FIELD` or `REFUSED_VALUE` (NaN from
  `get_field`) and add a `refused:` line to `ModCode::take_log`. Measured
  (`ponko_bench -- mods/host-call`, a busy laptop): a call alone 46 ns,
  `random` about 14 ns more, `get_field` about 310 ns and `set_field`
  about 645 ns (they go through reflection's JSON).
- C and C++ call code mods: `ponko_ffi`'s new feature `mods-wasm`,
  `ponko_ctx_mod_function` and `ponko_ctx_mod_call` (`PonkoModFn`,
  `PonkoModValue`), `ponko_ctx_mod_expose_entity`,
  `ponko_ctx_mod_hide_entity`, `ponko_ctx_mod_expose_prefab` and
  `ponko_ctx_mod_seed_random`; in C++ `ctx.mod_function`, `ctx.call_mod`
  (`ponko::mod_i32`, `mod_i64`, `mod_f32`, `mod_f64`), `ctx.mod_expose`,
  `ctx.mod_hide`, `ctx.mod_expose_prefab` and `ctx.mod_seed_random`. Built
  without the feature, each reports `PONKO_UNSUPPORTED`.
- The guide's "Mods" chapter: the host functions, how a game exposes
  entities and prefabs, and what a mod can never do.
- Debug drawing, `ctx.debug` (and `sys.debug` in view systems): lines, rays,
  arrows, polylines, boxes, circles, spheres, capsules, camera frustums,
  grids, axes and text labels, in 2D or 3D, in world or screen space, kept
  until the drawing system runs again or for a number of seconds,
  depth-tested or on top. All 3D lines are one buffer and two draw calls,
  2D lines one sprite batch. On in debug builds, off (one branch per call)
  in release builds; `Game::debug_draw` and `DebugDraw::set_enabled` choose.
- Built-in debug views (`DebugView`): physics colliders (2D and 3D),
  navmesh, bounds, light volumes and hierarchy axes.
- The control endpoint's `debug.draw` method and the MCP `debug_draw` tool
  turn debug drawing and its views on or off.
- C and C++: `ponko_ctx_debug_*` with `PonkoDebugStyle`, and
  `ctx.debug()` returning `ponko::debug_draw`.
- `ponko_bench` cases `debug/100k-lines-3d`, `debug/100k-lines-2d` and
  `debug/100k-lines-off`.
- Book: "Debug drawing".
- The editor: `ponko editor [<project>] [--open <file>]` builds a game with
  the new `editor` cargo feature and opens it in edit mode (`PONKO_EDITOR`).
  The game's window becomes a level editor: a menu bar (new, open, save,
  save as scene or prefab, reload), a hierarchy (drag to reparent the whole
  selection, multi-select, rename with F2 or a double click, a right-click
  menu), the component inspector with add and remove, and the viewport with
  move, rotate and scale gizmos and snapping in 2D as well as 3D. Every edit
  is undoable, drags and dragged numbers are one step each, the window title
  and status bar mark unsaved changes, closing with unsaved changes asks
  first, and a file changed on disk is opened again (or, with unsaved
  changes, offered). See the guide's "The editor" chapter.
- `hierarchy::EntityName`: a reflected name for an entity, shown and edited
  by the editor's hierarchy.
- The control endpoint's `editor.document` (open, reload or start a
  document and report it; MCP tool `editor_document`), `as` (`scene` or
  `prefab`) on `editor.save`, and `dirty` in the editor methods' replies.
- The inspector's 2D editing has a gizmo too: move along an axis, rotate,
  scale (W, E, R), world or local axes, and snapping.
- The editor's asset panel, along the bottom of its window: the assets
  folder as a tree, a thumbnail per texture, model, sound, prefab and scene
  (made on a worker thread), search (Ctrl+F), drag a texture, model or
  prefab into the view to place it (a texture becomes a sprite in 2D),
  double-click a scene or prefab to open it, a sound's preview, and the
  selected asset's import settings: its rule in `ponko cook`'s `cook.json`,
  shown with where it comes from and changed in place.
- Prefab instances in the editor: a prefab placed in the editor keeps its
  file in the new reflected `hierarchy::PrefabInstance` component, and the
  inspector lists how the instance differs from its prefab (changed
  components with their fields, added entities, missing parts), with
  Revert, Revert all, Apply to prefab (which updates the scene's other
  instances, keeping their overrides) and Open prefab. A saved prefab
  document has Apply to scenes, which updates its instances in every scene.
- Play mode in the editor: Play, Pause, Step and Stop (Ctrl+P,
  Ctrl+Shift+P, F10, Ctrl+.) and the speed on a play bar. Stop puts the
  world back exactly as it was at Play; edits made through the editor while
  playing are listed, and the ones ticked are kept as one undo step.
- The editor remembers its panel sizes, the asset panel, recent projects
  and files, and rebound shortcuts in `editor.json` in the user's settings
  folder (`PONKO_CONFIG_DIR` overrides it). File → Open recent, and
  Help → Shortcuts (F1) lists every shortcut and rebinds it.
- `ponko editor --recent` lists the recent projects and their files, and
  `ponko editor --last` opens the project and file opened last.
- CPU-only `ponko_image_diff` library shared by offscreen GPU tests, the `compare_images` MCP tool and `ponko image-diff`: RGBA tolerances, error statistics, global luminance SSIM, differing region boxes and optional PNG heatmaps. CLI thresholds support CI; MCP can compare against a live capture. Existing render-test tolerances and decisions are preserved.
- Data-only mods (`ponko::mods`): `ModPack` (a `.pack` with a `mod.json`
  manifest, or files in memory), `ModSet::mount` (mounts mods in the declared
  load order, moved after the mods each `requires` or loads `after`; a later
  mod's file replaces an earlier one's and the game's own by path), and the
  `Mods` resource (`list`, `provider`, `files_under`, `fingerprint`).
  Every mod is checked before any is mounted, and a bad one is refused whole
  with a `ModError` naming the mod, the file and the reason: an unknown
  component or resource, a field of the wrong type, a missing dependency, a
  cycle in the order, a damaged pack, a name like `../x`, a file over
  `ModLimits`. `ModSet::problems` lists them for a mods menu.
- C and C++: `ponko_ctx_mount_mods`, `ponko_ctx_mod_count`,
  `ponko_ctx_mod_id`, `ponko_ctx_mod_provider`, `ponko_ctx_mod_file_under`
  and `ponko_ctx_mods_fingerprint`; `ctx.mount_mods`, `ctx.mod_ids`,
  `ctx.mod_provider`, `ctx.mod_files_under` and `ctx.mods_fingerprint`.
- The guide's "Mods" chapter, with the comparison of data-only mods,
  WebAssembly modules (wasmtime, wasmi) and scripting (Rhai, Lua) on a mod
  that adds an enemy to Ember Hop: safety, determinism, cost per call, added
  size and web support, and the decision.
- `DebugShape::depth_tested()`: a 3D debug label hidden behind nearer 3D surfaces, as 3D lines are, drawn in the lit 3D frame facing the camera, its size in pixels wherever it is. C: `ponko_ctx_debug_text3d_depth_tested`; C++: `debug_draw::text_depth_tested`.
- `DebugDraw::set_collider_distance` and `collider_distance` (default 50 world units): how far from the 3D camera the colliders view draws mesh and terrain triangles. C: `ponko_ctx_debug_set_collider_distance` and `ponko_ctx_debug_collider_distance`; C++: `debug_draw::set_collider_distance` and `collider_distance`.
- One `DrawSpace` for sprites and debug drawing in every language: `DrawParams::with_space` takes the same `ponko::debug::DrawSpace` (also in the prelude) as debug drawing. C: `ponko_debug_style_with_space` takes the `PonkoDrawSpace` sprite draws take; C++: `ponko::with_space` works on a `debug_style` as on `draw_params`. `PonkoDebugStyle`'s layout is unchanged.
- The `debug/` benchmarks print their heap allocations per frame, and a `debug/shapes-and-labels` case draws spheres, boxes, capsules, circles and every kind of label. `tests/debug_draw_alloc.rs` checks that steady drawing, with every built-in view on, allocates nothing.
- The offscreen test `hundred_thousand_lines_gpu_time` times 100,000 debug lines with GPU timestamps (the opaque pass and the frame), with the lines in view, behind the camera, as dots, all depth-tested and all on top.

**C and C++**
- `examples/cpp/coin_dash`: the reference game Coin Dash written in C++ on
  `ponko.hpp`, with the same rules, screens, settings, rebinding, saves and
  languages. Its scenario test plays the Rust game's menu and key-press
  scenarios headlessly and makes the same assertions; it and a headless
  walk-through (`--smoke headless`) run in the native `ctest` step. The guide's
  "C and C++" chapter has a section on the port.
- The Rust game's scenarios are kept in `examples/coin_dash/scenarios/*.json`;
  its tests check that they still match (`PONKO_UPDATE_SCENARIOS=1` rewrites
  them), so the Rust and C++ games run the same scripts.
- C functions with C++ wrappers: `ponko_world_clear` (`world_view::clear`),
  `ponko_world_state` (`world_view::state<S>()`, the state of a world a
  headless run returned), `ponko_ctx_strings_number` (`ctx::format_number`),
  `ponko_ctx_strings_revision` (`ctx::strings_revision`) and
  `ponko_ctx_storage_is_available` (`ctx::storage_is_available`).
  `ui::paint` can be made from a `PonkoUiPaint`.
- C and C++ bindings for features that were Rust-only:
  - Reading an action map back: `ponko_action_map_count`, `ponko_action_map_name`,
    `ponko_action_map_get` (C++ `action_map::size`, `names`, `get`). Changing one
    installed action: `ponko_ctx_set_action` (C++ `ctx::set_action`).
  - Adding a built-in system back in any phase: `ponko_game_add_builtin` (C++
    `game::add_builtin`); physics and navigation steps keep their declared access.
    A built-in's declaration: `ponko_builtin_access` (C++ `access::builtin`).
    Copying a declaration: `ponko_access_clone`.
  - `LodHysteresis`: `ponko_ctx_set_lod_hysteresis`, `ponko_ctx_lod_hysteresis`.
    `LodGroup::instance_levels`: `ponko_lod_instance_levels` (C++
    `ponko::lod_instance_levels`).
  - The scene compute stage and target effects of the custom passes:
    `ponko_ctx_set_scene_compute_passes`, `ponko_ctx_scene_compute_passes`,
    `ponko_ctx_set_target_effects`, `ponko_ctx_target_effects` and
    `PonkoTargetEffect` (C++ `ponko::target_effect_of`).
  - Keyed 3D audio rolloff curves: `ponko_ctx_play_sound_at_keyed`,
    `ponko_ctx_set_audio_source3d_keyed` and `PonkoAudioKey` (C++ overloads of
    `play_sound_at` and `set_audio_source3d`). Whether audio is unlocked:
    `ponko_ctx_audio_unlocked`.
  - ECS diagnostics as JSON: `ponko_ctx_ecs_diagnostics`.
  - Save slots and storage state: `ponko_ctx_storage_world_slots`,
    `ponko_ctx_delete_world`, `ponko_ctx_storage_is_persistent`,
    `ponko_ctx_storage_is_saving`.
  - The immediate UI's own font, border width and scale:
    `ponko_ctx_ui_immediate_font`, `ponko_ctx_set_ui_immediate_font`,
    `ponko_ctx_ui_immediate_border_width`,
    `ponko_ctx_set_ui_immediate_border_width`, `ponko_ctx_ui_immediate_scale`.
- C and C++ read and set a sprite's flips: `ponko_sprite_set_flip(world, entity, flip_x, flip_y)` and
  `ponko_sprite_flip(world, entity, &flip_x, &flip_y)` (C++: `world_view::set_sprite_flip` and
  `world_view::sprite_flip`, which returns `sprite_flips`). `PonkoSprite` keeps its fixed layout.
- C and C++: glass and diffuse transmission can be authored. `PonkoMaterialDescEx`
  version 4 adds `transmission` (`PonkoMaterialTransmission`, `ponko::material_transmission`):
  the transmission factor, volume thickness and attenuation, dispersion, diffuse
  transmission and approximate scatter, with their six maps. Programs built against
  version 3 keep working and get no transmission.
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
- Crash reports for crashes that are not panics (the guide's "Crashes that
  are not panics"): a segmentation fault, bus error, illegal instruction,
  arithmetic fault or `abort()` in native code on Linux and macOS, and an
  unhandled exception or a C runtime `abort()` on Windows, leave a report of
  the new kind `CrashKind::Native`, with the signal (`CrashReport::signal`),
  the game's and engine's versions, the system, the GPU, the settings of the
  last change and the last lines the game printed. The engine's handler only
  writes bytes prepared ahead of time into a file opened ahead of time
  (`crash-reports/.native-<pid>-<n>`, deleted when a run ends normally), then
  hands the crash on to the handler that was there before; the game's next
  start turns the file into an ordinary report, lists it in
  `crash::saved_reports` and runs the `on_crash` hook with it.
- The browser runner `web/smoke/runtime-failures.mjs` checks that every
  failure that stops a running game, a lost WebGPU device included, hands
  the page its crash report (`window.ponkoCrashReport`).
- Versioned C descriptors: settings structs that can gain fields within an
  ABI version. Each starts with `struct_size` and `version`, holds its ABI 2
  struct, and has its own entry points next to the ABI 2 ones:
  `PonkoMaterialDescEx` (`ponko_ctx_create_material_ex`,
  `ponko_ctx_update_material_ex`, with the stencil state),
  `PonkoNavBakeSettingsEx` (`ponko_ctx_bake_navmesh_ex`), `PonkoLightingEx`
  (`ponko_ctx_lighting_ex`, `ponko_ctx_set_lighting_ex`, with
  `shadow_sun_size` for contact-hardening shadows and `shadow_sun_step`),
  `PonkoColliderEx` (`ponko_set_collider_ex`, `ponko_get_collider_ex`: a 2D
  collider with its collision groups and polygon corners in one call) and
  `PonkoQueryFilterEx` (a `ponko_ctx_*_ex` form of every 2D and 3D query,
  collision groups included). STABILITY.md's "Versioned descriptors" has the
  rule; C++ has `ponko::material_desc_ex` and friends with helpers that fill
  the header.
- Humanoid retargeting from C and C++: `PonkoHumanoidMap`
  (`ponko_ctx_detect_humanoid_map`, `ponko_humanoid_map_get`, `_set`,
  `_missing`), `PonkoRetargeter` (`ponko_ctx_create_retargeter`,
  `ponko_retarget_clip`, `ponko_ctx_retarget_pose`), `PonkoHumanBone`, and
  the C++ `ponko::humanoid_map` and `ponko::retargeter`.
- Client prediction for native games: `PonkoPrediction` and
  `PonkoInputQueue` over fixed-size byte states and inputs, with a step
  callback, and `ponko::prediction<State, Input>` and
  `ponko::input_queue<Input>` in C++ for trivially copyable types. Replay
  is deterministic; a step that throws stops the call and the exception
  comes out of it. A steady predict-and-reconcile loop allocates nothing.
- `PONKO_UNSUPPORTED`, a `PonkoStatus` for "this machine or build can't do
  it".
- C and C++: `PonkoUploadStatsEx` (`ponko_ctx_upload_stats_ex`, C++
  `ctx.upload_stats_ex()`), a versioned form of the upload counters with
  the GPU's own time for textures made outside the upload steps
  (`gpu_created_ms`), for the latest upload step (`gpu_upload_ms`),
  `gpu_upload_timing` and `frame_slices`. `PonkoUploadStats` keeps its layout.
- C and C++: `ponko_net_alloc_bytes()` (C++ `ponko::net_alloc_bytes()`),
  bytes allocated minus bytes freed while profiling is on, for leak checks
  of native games; `ponko_note_deallocation` (Rust
  `perf::note_deallocation`) counts the frees of memory reported with
  `ponko_note_allocation`.
- C and C++: `PonkoBuoyancyDesc`, a versioned buoyancy descriptor with up
  to 32 sample points of your own, the water's density, drag and the ripple
  switch: `ponko_buoyancy_desc_box`, `_sphere`, `_points`,
  `ponko_ctx_set_buoyancy` and `ponko_ctx_buoyancy` (C++
  `ponko::buoyancy_desc`, `ctx.set_buoyancy`, `ctx.buoyancy`).
- The window's safe area from C and C++: `ponko_ctx_window_safe_area`,
  `_safe_insets` and `_feed_safe_insets` (`PonkoSafeInsets`; C++
  `ctx.window_safe_area()`, `window_safe_insets()` and
  `window_feed_safe_insets(insets)`), and UI roots laid out inside it with
  `ponko_ctx_ui_set_inside_safe_area` and `_inside_safe_area` (C++
  `set_inside_safe_area` and `inside_safe_area`).
- A scenario step that feeds safe area insets, so a layout for a notched
  phone is checked without a device: `InputScenario::safe_insets(tick, insets)`,
  the `safe_insets` step of the JSON form (also for the control endpoint and
  the `run_input_scenario` tool), `ponko_input_scenario_safe_insets` and C++
  `input_scenario::safe_insets`.
- C and C++: a brain row in a query loop (also a parallel one) can be signalled with a value, as Rust's `Brain::signal_with`: `ponko_ai_brain_signal_num`, `_entity`, `_vec2` and `_vec3` (C++ `ai::brain_row::signal_num`, `signal_entity`, `signal_vec2`, `signal_vec3`).
- C and C++: a water body's pending contact ripples are readable (Rust `Water::waiting_contacts`): `ponko_ctx_water_waiting_contacts`, `ctx.water_waiting_contacts`, so a test sees a floating body's ripple switch by its effect.
- C and C++: a UI of the game's own, such as a world panel's, has its safe area set and read (Rust `Ui::set_safe_area` and `safe_area`): `ponko_ctx_ui_set_safe_area`, `ponko_ctx_ui_safe_area`, and the UI's `set_safe_area`, `clear_safe_area` and `safe_area` in C++.
- Versioned C descriptors for the 3D picture's remaining fixed structs, so
  every field their Rust structs have is reachable from C and C++:
  `PonkoPostEx` (`ponko_ctx_post_ex`, `ponko_ctx_set_post_ex`: split toning,
  depth of field and the sample count `msaa`, which can now change while the
  game runs), `PonkoParticleSettings3DEx`
  (`ponko_ctx_set_particle_emitter3d_ex`: `softness`, soft particles),
  `PonkoCamera3DEx` (`ponko_ctx_camera3d_ex`, `ponko_ctx_set_camera3d_ex`:
  `occlusion_culling` and `particles`) and `PonkoTargetCameraEx`
  (`ponko_ctx_set_target_cameras_ex`, `ponko_ctx_target_camera_ex`:
  `particles` per target camera). The ABI 2 structs and calls are unchanged.
- `Post::lerp` and `PostVolume::blend` from C and C++: `ponko_post_ex_lerp`
  and `ponko_post_blend_volumes` (a `PonkoPostVolume` is a box that points at
  its `PonkoPostEx`); C++ has `ponko::post_ex`, `post_lerp`, `post_volume`,
  `post_volume_around` and `blend_post_volumes`, `camera3d_ex`,
  `target_camera_ex`, `ctx::post_ex`, `ctx::camera3d_ex`,
  `ctx::target_camera_ex`, `ctx::set_target_cameras_ex` and
  `particle_settings::softness`.
- `ParticleEmitter::settings()` returns the settings an emitter was made with.
- `scripts/check-settings-fields.py` (run by `check-ffi-coverage.py`): every
  public field of a Rust settings struct has a field of the same name in its C
  struct or descriptor, a line naming where C sets it
  (`scripts/settings-fields.txt`), or a Rust-only entry with its reason.
- C and C++ caller-buffer listings for last-frame mesh costs and streamed assets, including packed asset names.
- Versioned C and C++ 2D debug-draw style with world and screen coordinate spaces.
- C and C++ bindings for camera rig parts, named debug flags, control methods, retained UI readers, LDtk level listings, and one-shot asset group decoders.

**Guide and examples**
- Every guide page opens with a note that names the version the guide
  documents (the `version` in the root `Cargo.toml`) and how many unreleased
  changes `main` has on top of it. A guide built from a release tag says it
  documents exactly that release. The note is written by
  `scripts/guide-note.py` from `Cargo.toml` and `changelog.d/`, so a release
  bump updates it without an edit to any page.
- `scripts/release-check.py` also refuses a release while the dependency lines
  the guide's getting-started chapter, the tutorials and the project templates
  show (`ponko = "X.Y"`) name another minor than the tag.
- Two step-by-step tutorials in the guide, each building a small game from
  `ponko new`: "Tutorial: a first 2D game" (Crab Cove: sprites, input
  actions, a Tiled level with solid tiles, sensors and collision events,
  sound, a retained-UI title screen, the best score in `ctx.storage`,
  headless tests, `ponko package` and a web build) and "Tutorial: a first
  3D game" (Lantern Yard: a glTF scene drawn and solid, a character
  controller, a follow camera, the sun and point lights, physics bodies and
  sensors, a retained-UI HUD, headless tests, packaging). Every step's code
  is pulled from the `tutorial_2d` and `tutorial_3d` examples, which
  `tests/tutorial_2d.rs` and `tests/tutorial_3d.rs` play headlessly; both
  build for the web. Rust only so far.
- Ember Hop (`examples/ember_hop`, feature `physics`), a second reference
  game: a small platformer on the engine's own Tiled levels (autotiled
  ground, tile colliders, objects as prefabs, one with children), 2D physics
  with sensors and collision events as its rules, a title scene, world save
  slots at its checkpoints, sound files and streamed music, particles, a
  following camera, and keyboard and gamepad actions. `tests/ember_hop.rs`
  plays it headlessly (moving, jumping, collecting, the attack, losing,
  saving and loading, the level transition, the end of a run, the input
  mapping through headless frames) and its autopilot plays the whole game.
  The check list packages it with `ponko package` and plays the package on
  Linux, macOS and Windows; it builds for the web (`web/build.sh --features physics
  ember_hop`). The guide's chapter "Ember Hop: a platformer, start to
  finish" walks through it.
- Shard Grove (`examples/shard_grove`, features `navmesh` and `physics3d`),
  the 3D reference game: a small third-person game whose glTF level is drawn,
  solid and baked into a navigation mesh; skinned glTF characters whose clips
  are states of animation state machines; a character controller; enemies
  that patrol and chase on the navigation mesh; positional sound; a sun with
  shadows and point lights; title, pause, settings and end menus, a HUD and
  a UI panel in the world; graphics quality presets and volume in a settings
  file; and saves at shrines, with damaged saves set aside.
  `tests/shard_grove.rs` plays it headlessly, and its autopilot plays the
  whole game (`shard_grove --smoke headless|window`). It packages with
  `ponko package --example shard_grove --features navmesh,physics3d`, builds
  for the web (`web/build.sh --features navmesh,physics3d shard_grove`), and
  the web gallery lists it. The guide's chapter "Shard Grove: a 3D game,
  start to finish" walks through it and lists the gaps it found.
- `Ui::set_focus(id)` moves the retained UI's keyboard and gamepad focus from
  code (it returns `false` for a node that can't take it), and
  `Ui::clear_focus()` takes it away. `Ui::set_initial_focus(id, on)`, or the
  builder's `.focused()`, marks the node a screen opens on: every
  `push_screen` of the screen focuses it, so a menu is ready for Enter at
  once. C: `ponko_ctx_ui_set_focus`, `ponko_ctx_ui_clear_focus`,
  `ponko_ctx_ui_set_initial_focus`; C++: `tree.set_focus`, `clear_focus`,
  `set_initial_focus`.
- HUDs: `Ui::show_hud(root)` shows display-only UI (a score, a counter)
  below every screen. Escape never closes it, it takes no pointer or focus,
  and it doesn't make `wants_pointer` or `wants_keyboard` true;
  `Ui::hide_hud` and `Ui::is_hud` go with it. C: `ponko_ctx_ui_show_hud`,
  `ponko_ctx_ui_hide_hud`, `ponko_ctx_ui_is_hud`; C++: `tree.show_hud`,
  `hide_hud`, `is_hud`.
- `ConfiguredGame` takes the settings a test adds to a finished game:
  `headless_frames`, `input_scenario`, `run_headless_scenario(scenario)`,
  `data_dir`, `seed`, and test systems with `system` and `sim_system`. The
  tests of a `ponko new` project can drive it with frames, a scenario and a
  storage folder of their own without changing `src/lib.rs`.
- `Ctx::load_collision_mesh(path)` makes a glTF file solid in one call (a
  triangle mesh of its meshes, for a mesh collider), and
  `load_collision_mesh_with(path, CollisionMeshOptions { scale, convex })`
  scales it or makes a convex hull. It shares the scene loader's code and
  cache, and a collider of it is saved by its file. C:
  `ponko_ctx_load_collision_mesh`; C++: `ctx.load_collision_mesh`.
- `MeshData::triangles()`: the indices three at a time.
- The `ponko new` 2D and 3D templates have a test that drives the game with
  headless frames, an input scenario and a data folder of its own.
- Maturity levels: every guide chapter opens with its level (proven, tested
  or experimental) and what it rests on (the reference game, the platforms
  and the graphics backends the list of checks covers), and the rustdoc of
  every top-level module repeats it. The guide's new "Maturity levels" page
  lists them all; `STABILITY.md` defines the levels and what each promises,
  and names the experimental items that will get `unstable_` names (#529).
  `python3 scripts/maturity.py` fails for a chapter or module without a
  level and regenerates the page.
- The guide's new chapter, **Coming from Bevy** (`book/src/from-bevy.md`):
  Bevy 0.20.1's concepts mapped to Ponko's, fifteen common tasks side by
  side, what is different on purpose, what is missing or slower (with the
  scoreboard's numbers), a porting checklist, and Bevy's `move_sprite`
  example ported line by line. The Bevy snippets compile against the pinned
  Bevy (`bench/compare/tests/from_bevy.rs`, `cargo check --tests` in
  `bench/compare`).
- `examples/move_sprite`: the ported example, tested headlessly by
  `tests/move_sprite.rs`.
- A cookbook in the guide: fifty small recipes (moving, fighting and collecting,
  timers, menus, cameras, 2D and 3D physics, levels, sound and saves), each in
  Rust and, for 48 of them, in C++ (slow motion and zoom need C++ calls that do
  not exist yet). Every recipe is compiled and run headless by a test
  (`tests/cookbook_*.rs`, `examples/cpp/cookbook.cpp`), and the pages include
  the tested code, so none of it can drift. An index by task is in the guide
  and in the README.
- Legions (`examples/legions`), the simulation showcase: two armies of
  50,000 soldiers and archers on shared flow fields, spatial-grid targeting
  (archers lead their targets from the grid's velocity data), arrows that
  fly and land, the fallen, rounds, and animated sprites from one sheet drawn
  as two instanced draws. Orders are its only input; `--record` writes them
  with the seed and a world hash, and `--replay` plays a recording headless
  and checks that it ends with the same hash (on any thread count; checked
  on aarch64 and x86_64). `--headless` prints tick-time percentiles and each
  system's share. It packages with `ponko package --example legions` (no
  assets; a recorded battle ships beside it) and builds for the web.
  `tests/legions.rs` plays it headless (same hash across runs, thread counts
  and with or without sprites, recorded input through headless frames
  replayed by the rules alone, the committed replay, orders, arrows,
  rounds); `src/render/legions_render_test.rs` draws it offscreen with a
  reference image and measures the frame cost. The guide's chapter
  "Legions: 100,000 units that replay bit for bit" walks through it.
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
- The 3D reference game's GPU baseline on Windows (Radeon RX Vega, DirectX 12
  and Vulkan, three runs each) in `bench/shard_grove/`, linked from its guide
  chapter (#454).

**Development and releases**
- `python3 scripts/semver-checks.py run` checks the public API of every
  published crate against its last crates.io release (`cargo-semver-checks`,
  the `semver` job of the check list). A break fails a change to a
  `release-*` branch, and on `main` passes only for a pull request with the
  `breaking` label (`--breaking`) and a `### Breaking changes` fragment.
  CONTRIBUTING.md explains how to read a failure and what the tool does not
  catch.
- `api/` holds a generated listing of the public API of every published crate
  (default, headless, `3d` and docs.rs feature sets), and
  `python3 scripts/api-snapshots.py check` (the `api-snapshots` job of the
  check list) fails with a diff when it is out of date, so every added,
  renamed or removed public item shows up in review.
  `python3 scripts/api-snapshots.py update` regenerates it (CONTRIBUTING.md,
  "API snapshots").
- The release workflow accepts release candidates (`v0.2.0-rc.1`): the crates
  publish as pre-releases that Cargo never selects unless the version is
  written out, the GitHub release is marked pre-release, and the notes may
  still be under `[Unreleased]`. It also accepts `vX.Y.Z` on a `release-X.Y`
  branch as well as on `main`, so patch releases can be cut while `main` holds
  breaking changes. CONTRIBUTING.md's "Releasing" describes both flows and the
  `backport` label's cherry-pick procedure.
- A `### Breaking changes` fragment entry says what changed, what to write
  instead and shows the code before and after; `scripts/changelog.py --check`
  refuses one that doesn't. At release time `scripts/changelog.py --version
  X.Y.Z` folds the entries into the CHANGELOG's Breaking changes block and
  generates `book/src/migrating-to-X.Y.md` from the same entries, so the
  migration chapter of a minor release is no longer written by hand.
- The Semver checks job also fails a pull request that carries the `breaking`
  label without such a fragment, and `scripts/release-check.py` refuses a minor
  release with breaking changes that lacks its migration chapter.
- A dependency audit with cargo-deny (`deny.toml`, `cargo deny check`, in
  the check list and every release's checks): licenses (the set the C
  library's third-party notices carry), RustSec advisories, duplicate crate
  versions (each accepted one listed with a reason), banned crates and
  allowed sources. CONTRIBUTING.md has "Dependency audit" for what to do when
  it fails.
- Benchmark tracking: `bench_track` (in `crates/ponko_bench`) counts the
  instructions of seven small workloads (ECS iteration, a filtered query,
  lookups, spawn, despawn, a headless tick, 3D frame collection) under
  cachegrind, compares repeated runs by their medians against a threshold
  of the larger of a floor and twice their spread, renders a history per
  machine, and compares two folders of runs. Each release adds a wall-clock
  baseline from a named machine (`crates/ponko_bench/releases/`).
- `ponko_bench`'s JSON results record `env.machine` (`PONKO_BENCH_MACHINE`).
- Fuzz targets (`fuzz/`, run with `cargo fuzz`) for the loaders that read
  untrusted data: scenes and world saves, prefabs, glTF models, the Unity
  importer's text and binary readers, action maps, asset packs, and lockstep
  and message packets. `fuzz/run-all.sh` runs each for the time
  `fuzz/targets.txt` gives it. See
  `fuzz/README.md` and CONTRIBUTING.md, "Fuzzing".
- The changelog and the migration chapter are told by area (Rendering,
  Audio, C and C++, ...), breaking changes first, each migration entry under a
  short heading of its own. `changelog.py --areas` shows where each fragment
  goes; `<!-- area: ... -->` on a fragment's first line overrides the guess.
- Every breaking entry's `After:` code compiles: Rust, C, C++, WGSL and TOML
  blocks are built against the current API by `changelog.py --check --compile`
  and by the guide's snippet check.
- A release page for 0.2, "What's new in 0.2" in the guide: a story written by
  hand and a part `changelog.py --notes X.Y.Z` generates (the changes by area
  and every breaking change, linked to its migration entry). The fold copies
  the story to the top of the CHANGELOG section.
- `release-check.py bump X.Y.Z` sets every crate, the requirements between
  them and the guide's and templates' `ponko = "X.Y"` lines in one step.
- `scripts/first-run.py X.Y.Z`: a new user's first run from crates.io
  (install the CLI, make and test each template, build the guide's first
  program), for each OS after a release.
- `scripts/release-0.2.md`: the 0.2 cut as one list of commands, and
  `scripts/release-matrix.yml`, a release-time CI workflow made from the
  check list that is not enabled (it runs only from a branch a maintainer
  pushes at a release, for the Linux checks, the Windows on Arm C library
  and the Linux first run).
- Run Ponko's local check inventory with `python3 scripts/run-checks.py`, including
  job and tag filters, a quick subset, and JSON and text reports for maintainer machines.
- Publish per-OS JSON and Markdown status files from the local check runner and
  reconcile one nightly failure issue per OS with an opt-in updater.
- Well-known content: `scripts/fetch-known-content.py` fetches the Khronos
  glTF Sample Assets and two public scenes (Sponza, Amazon Lumberyard
  Bistro) at pinned revisions into a git-ignored cache and lists each
  asset's license. A load sweep (`tests/known_content.rs`) records whether
  every sample loads, fails or needs an extension the engine doesn't read,
  an offscreen GPU test draws every loaded sample and compares it with the
  sample's own screenshot, and an ignored release benchmark measures the two
  scenes at 1080p and 1440p for each quality preset. The opt-in
  `known-content` job of `scripts/checks.yml` runs them; the guide's
  "Well-known content" chapter has the results.
- glTF: `EXT_mesh_gpu_instancing` is read: each instance draws the node's
  mesh with its own translation, rotation and scale.
- glTF: `MSFT_texture_dds` images (BCn blocks in DDS files) are read with
  the `compressed-textures` feature, as KTX2 ones are.
- glTF: materials of the retired `KHR_materials_pbrSpecularGlossiness`
  workflow load, approximated: the diffuse color and texture as the base
  color and the glossiness as roughness (a warning says so).
- `GOVERNANCE.md` (a draft): how decisions are made, who can merge, how someone
  becomes a maintainer and what happens when the only maintainer is away.
- `CONTRIBUTING.md` opens with a one-screen quick start. GitHub Discussions is
  the place to talk, and maintainers answer new issues and discussions within a
  week.
- Timed reference-game and terrain-streaming soaks (`scripts/soak.py`) with
  repeated play, save and restart paths, resize and minimize/restore cycles,
  CSV samples (memory, live handles by kind, GPU memory, allocations per
  frame, audio voices, frame-time percentiles, load average) and
  post-warm-up slope bounds. The ten-minute offscreen run is the `soak` job
  of the list of checks; the four-hour desktop run is in the release steps.
- Place a glTF file's lights with an EV100 exposure from C and C++: `ponko_ctx_light_gltf` (`ctx::light_gltf`) sets up a glTF preview from a file, and `ponko_ctx_place_light_exposed` (`ctx::place_light_exposed`) places one `PonkoModelLight` the same way.
- Light a scene with an HDR or EXR panorama, six cube faces or the neutral studio from C and C++: `ponko_ctx_load_environment`, `ponko_ctx_load_environment_cube`, `ponko_ctx_set_studio_environment`, `ponko_ctx_clear_environment` and `ponko_ctx_has_environment` (`ctx::load_environment` and friends).
- Fuzz targets for replication, the lobby protocol (server and client),
  the control endpoint's requests, Aseprite files and cooked sheets, LDtk
  and Tiled levels, and cooked textures (`fuzz/README.md`).
- A load test with hostile clients for the lobby server
  (`crates/ponko_lobby/tests/hostile.rs`): its memory stays flat while they
  flood it, and honest rooms keep working.
- The guide's "Security: what reads untrusted bytes": every entry point,
  who can send it bytes, what a bad actor wants and what stops them.
  `SECURITY.md` says what is in scope.
- `scripts/check-unsafe.py` holds every unsafe item outside the ECS to the test
  that runs it: `scripts/unsafe-inventory.txt` lists each item of the engine and
  the tools with its test and whether that test runs under Miri (or why not),
  and the C API crate's helpers are matched to a tested C function that calls
  them. It fails for unsafe code without a test and prints the line to add;
  `--list` shows every item with its test, `--miri` the Miri commands.
- The engine's unsafe code runs under Miri: the counting allocator, the
  threaded sprite and skeleton draw-list fills, components read and written as
  bytes (reflection, world hashes, C events), hierarchies, the retained 3D
  frame's parallel compare and `Ui` across threads. New tests cover what had
  none: `the_counting_allocator_forwards_every_call_to_the_system_allocator`,
  `many_sprites_are_queued_once_each_from_many_threads`, the retained 3D frame's
  `instances_written_as_shared_words_from_several_threads_are_all_kept` and
  `tests/ui_threads.rs`.
- The C and C++ test programs build with AddressSanitizer and
  UndefinedBehaviorSanitizer (`-DPONKO_SANITIZE=ON` in `examples/cpp`), and the
  `ponko_ffi` tests run with AddressSanitizer; the commands are in
  `CONTRIBUTING.md`, "Unsafe code and sanitizers".
- Every C function says who owns each pointer it takes or returns: the rules at
  the top of `ponko.h` ("Pointers and who owns them") cover most by type, and
  returned pointers, `T **out` parameters, callback user data and objects a
  function takes over have an `Ownership:` paragraph in the API docs.
  `scripts/check-ffi-ownership.py` fails for a function that leaves one unsaid
  and says what to write.
- `scripts/check-feature-coverage.py` fails when a cargo feature of a workspace
  crate has no clippy or test line in the check list (`scripts/checks.yml`) (or no doc
  line, when docs.rs builds the crate with it), or when a browser runner in
  `web/smoke` or `web/gallery` is not run there. What may lack a line is
  listed with a reason in `scripts/feature-checks.txt`, and the check fails
  for an entry that is not needed any more. `--list` shows each feature's
  checks, `--selftest` checks the parsing.
- The check list covers the `editor`, `video`, `lobby` and `webrtc` features
  (clippy lines, test steps, and docs with every public feature), and also
  `hotpatch`, `profiling`, the C library's optional features, the ECS's `serde`
  and the benchmark crate's drivers.
- The C and C++ test programs run against a library built with `webrtc` in
  the native job (`-DPONKO_WEBRTC=ON` in `examples/cpp`); without the feature,
  `ponko_ffi`'s `lobby` test holds that the calls fail and name it.
- The component hooks and required components tests run under Miri's Tree
  Borrows model, with the parallel path's.
- The threaded web build (`web/smoke/threads.mjs`) and replication in the
  browser (`web/smoke/replication.mjs`) run in the web smoke job.
- "Adding or changing a cargo feature" in `CONTRIBUTING.md`.
- `Game::restore_window_settings(true)` opens the first window with the
  settings the player saved (`ctx.window.save_settings`, `window.json`): it is
  created at the saved size and position, in the saved mode, with the saved
  limits and flags, so no default-sized window shows first. The saved choice
  wins over `Game::fullscreen` and `Game::new`'s size; the title stays the
  game's. A missing, unreadable or invalid file opens the game's own window
  (the reason is printed), a monitor or mode that is gone opens windowed at
  the saved size. Headless runs apply the same settings to `ctx.window` before
  setup; the web restores once its storage is read, before setup. Guide:
  "Remember the player's choice" in the window chapter. Rust-only for now.
- The guide's plugins chapter points at the companion plugins repository
  (`ponko-extras`, beside the engine): a workspace of plugin crates made
  from the plugin template, built against an engine checkout beside it,
  with a check script for each crate's layout, clippy and tests. `scripts/project-url.py` knows companion repositories:
  `check` fails on a link to one that isn't beside `PROJECT_URL`, and `set`
  moves those links with the engine's.
- `scripts/bench-aligned.sh`: cargo with every function aligned to 128 bytes
  (`-C llvm-args=-align-all-functions=7`), so two builds compared by a
  benchmark lay out their code the same way and a small difference is the
  code, not where it fell. `ponko_bench` records it as `env.code_align` in
  its JSON, and `bench_track compare` warns when the two sides differ in it.
  The benchmark README ("Two builds, one layout") says how, with a
  measurement: two builds of the same code moved a third of the runner's
  functions without it and none with it.
- `tests/ui_access_window.rs`: a check a person runs
  (`cargo test --features accessibility --test ui_access_window -- --ignored`)
  of the accessibility adapter on the engine's own window, on macOS and
  Windows: the window opens far off the screen and the system's
  accessibility API presses a button of the game's UI through it. The
  guide's "Unsafe code" section lists the checks that need a person.
- A test that replicates a component registered from C (every field type,
  at unaligned offsets) over the in-memory scripted network, so the C
  replication read and write code runs under Miri.
- `perf::thread_alloc_counts()`: the allocations made on the calling thread
  while profiling is on. It costs nothing while profiling is off and never
  allocates itself.
- Profiling scopes entered with `profile_scope!` or `ProfileScope::enter`
  count the allocations made on their own thread while they are open
  (`ScopeReport::allocs_per_frame`): a scope inside a parallel loop counts the
  loop's work on every thread, and nothing the thread pool allocates for
  itself. Drawing 2D skeletons in parallel has one, `skeleton2d/draw_chunk`.
- `NavMesh::crowd_stats()` (`nav::NavCrowdStats`): the crowd steps run, the
  agents they moved and the backend's work units, since the mesh was baked.
- A reproducible scoreboard against Bevy: `python3 bench/compare/scoreboard.py`
  runs ECS, sprite, 3D, skinned-character, 2D physics, UI, headless
  simulation and time-to-first-frame workloads on both engines, measures
  build times and sizes, and writes JSON plus a Markdown table (three runs
  each, the median and the range, with the load average). The results are in
  the guide's performance chapter, losses included.
- `bench/compare` (was `bench/bevy_scenes`) is the one comparison crate,
  with its own lock file and Bevy pinned to 0.20.1.
- `ponko_bench`: `compare/...` rows (the scoreboard's Ponko side) and a
  `first_frame` binary.
- `Water::contacts_started`: how many contact ripples have started on a water body since it was made, which the water's update doesn't drain (unlike `Water::waiting_contacts`). C and C++: `ponko_ctx_water_started_contacts`, `ctx.water_started_contacts`, so a test reads it once after the physics step instead of keeping the most of the waiting count over every tick.
- Build times and download sizes in the performance chapter: clean,
  one-line-change and link times of the 2D template and the 3D tutorial
  (debug and release), where a clean build goes (`cargo build --timings`,
  `cargo llvm-lines`), and desktop and wasm sizes under each profile.
  `scripts/build-times.sh <project> [dev|release]` repeats the measurement.
- The game templates have a `.cargo/config.toml` with the faster linkers
  for Linux (mold, lld) and Windows (`rust-lld.exe`) to switch on, and the
  2D and 3D templates two size profiles: `dist` (fat LTO, one codegen unit,
  stripped; 14.8 MB instead of 24.5 MB for the 2D template on macOS) and
  `dist-web` (the same optimized for size: a 10.2 MB `.wasm` instead of
  15.1 MB). `ponko package --profile dist` and `--web --profile dist-web`
  use them.
- `web/build.sh --profile <name>` builds with another Cargo profile than
  `release`.
- The scoreboard against Bevy has a second table with both engines tuned
  (`python3 bench/compare/scoreboard.py --tuned`, and part of the default
  run): one shipping profile on both sides (fat LTO, one codegen unit,
  `panic = "abort"`), Bevy's pipelined rendering on with frames timed from
  one finished frame to the next, only the Bevy features each workload
  needs, rapier's `parallel` and `simd8` features on both sides, and on
  each side the faster of its documented ways to do a workload, measured.
  The build rows are made again for the small game with Bevy's
  `dynamic_linking` and both engines' unused features off. Results in
  `bench/compare/results/scoreboard-tuned.*` and the guide's performance
  chapter; `bench/compare/README.md` names the source of every choice.
- `ponko_bench` features `rapier-parallel` and `rapier-simd8`, and
  `scripts/build-times.sh` reads extra cargo arguments from
  `BUILD_TIMES_CARGO_ARGS`.
- `perf::net_alloc_bytes()`: bytes allocated minus bytes freed while
  profiling is on, counted by `CountingAllocator`. Two readings around a
  stretch of play tell how much of what it allocated is still held, the
  number a leak check watches. The counting allocator now also does one
  relaxed atomic load per free.
- Long-run tests: 200 000 headless ticks of spawning, despawning, deferred
  spawns, families, scene loads, events, handles, sounds, UI screens, game
  states, debug drawing and camera rigs keep memory, archetypes, the query
  cache and ids flat after warm-up; 2000 offscreen frames of textures,
  materials, meshes, instance cells and render targets bring the GPU
  memory report back to its baseline; ten simulated hours keep the clocks
  exact. The guide's "Very long sessions" (in "The simulation clock") says
  what holds and what the limits are.
- `Animator::normalized_time_f64` and `skeleton2d::Skeleton::normalized_time_f64`:
  the unwrapped normalized clock in full precision, exact after any length
  of play (`normalized_time` hands out the same clock as an `f32`).
- Document how to measure workspace test coverage and report mapped lines by
  module. Include an inventory of public Rust functions without observed
  executions and record coverage with each release.

### Changed

**API and crate layout**
- STABILITY.md, "Re-exported crates", has the final rule for 1.0: `glam`
  (`ponko::math`, the prelude) and `serde_json` (`ponko::reflect::json`,
  `JsonValue`, `JsonMap`, `json!`, and the same names in `ponko_mcp`) stay
  re-exported and pinned, so from 1.0 on an incompatible version of either
  comes only with a Ponko major release; `steamworks` is wrapped and `taffy`
  stays `ui::unstable_taffy`. A test checks the API listing for `steamworks`
  and `taffy` types outside `unstable_` items.
- Name lookups that fail now say which names exist: the C calls
  `ponko_ctx_skeleton2d_play`, `ponko_ctx_skeleton2d_crossfade`,
  `ponko_ctx_skeleton2d_state_index` and
  `ponko_ctx_skeleton2d_parameter_index` (and their C++ wrappers) put the
  clip, state or parameter names in `ponko_last_error`.
- `ActionRef` (what the action queries take) accepts any reference to a
  string type (`&Arc<str>`, `&Box<str>`, `&Cow<str>`, ...), not only
  `&str` and `&String`. `SnapshotRef` and `ClipRef` do the same, so the
  calls that took `&str` before still take everything they took.
- The examples, the reference games and the guide's recipes look no names
  up in code that runs every frame: they resolve actions, buses, snapshots,
  animation parameters and UI nodes once and keep the ids (or look UI
  names up only when a click arrives).
- The experimental features' public items are outside the versions' promise
  now and may change in any release: HDR output (`Game::hdr_output`,
  `ponko::render::Hdr*`, the `Graphics` HDR methods), WebRTC
  (`ponko::net::rtc`, `NetMessages::host_rtc`, `join_rtc`, `rtc_signal_peer`,
  `LobbyClient::signaling`) and the threaded web build (`Game::web_threads`,
  `ponko::web::threads`). They keep their names; `scripts/experimental-api.txt`
  lists them with their reasons, and STABILITY.md, "Unstable", says what that
  means. The cargo features `webrtc` and `hotpatch` are unstable too.
- C: the experimental functions (`ponko_*hdr_output*`, `ponko_ctx_hdr_status`,
  `ponko_net_messages_rtc_*`) carry an `/* Unstable: ... */` note in `ponko.h`
  and in their C++ wrappers' docs, and may change within an ABI version (the
  guide's "C and C++", "Unstable functions").
- The API snapshots list experimental items apart, in
  `api/<crate>/experimental.txt`, and the semver check does not count a break
  of them. `scripts/experimental-api.py check` keeps the list in step with the
  guide's maturity levels, `api/` and the C header.
- Error messages on the common mistakes name the mistake, where it is and the
  fix; the error types, their variants and `ErrorKind`s are unchanged (match
  on `kind()`, not on the text), and the guide's "When something fails"
  chapter lists them:
  - A file given to the wrong loader (`load_texture`, `load_font`,
    `Audio::load`, `load_stream`) says what its first bytes are and which
    call loads it; an empty file and a Git LFS pointer say so and how to
    get the real file. The sound loaders name the formats they read.
  - A missing asset name that starts with `assets/` or uses backslashes gets
    a hint with the corrected name.
  - `Ctx::load_world` names the slot and its file. A slot nobody saved lists
    the slots there are; a damaged, empty or non-save file says so and points
    at `Storage::read_backup` and `Storage::delete_world`. A stored file that
    isn't there names the folder it looked in, and `Storage::read_json` names
    the type the file didn't fit.
  - A scene, save or prefab that names a component the game didn't register
    gives the call to register it (`world.reflect::<Name>()`), the first entity
    that names it and the closest registered names. A value that doesn't fit
    names the entity too. A prefab given to the scene loader (or the other way
    round) says what the file is and which call reads it, and a file from a
    newer ponko says to update. The `ReflectError` texts for an unknown
    component or resource, and for a bad save as a whole, are reworded.
  - GPU startup failures (`no compatible GPU found`, `could not open the GPU
    device`, `could not create a GPU surface`, a surface with no format) say
    what to try: the driver, `WGPU_BACKEND`, `Game::run_headless`.
  - `Game::order`'s refusal says which system to move and how. The code-mod
    message names the `mods-wasm` feature to turn on.
- `Rng::unit_vec2` (and C++ `rng::unit_vec2`) no longer uses `sin` and `cos`:
  it is bit-identical on every platform now, and gives a different sequence
  of vectors for the same seed than before.
- `SpatialGrid::nearest` goes outward ring by ring from the query's cell, so
  a long radius costs about what a short one does (10,000 searches among
  100,000 entities: 1.65 ms against 402 ms in a radius of 16 cells). Of two
  entries at the same distance the one with the lower entity id is
  returned, and `accept` is asked about each entry at most once.
- A positional sound's `priority` (`d3::AudioSource::priority`) follows
  changes while it plays, instead of applying only when it starts.
- `ponko_ctx_update_texture` on a render target's texture returns
  `PONKO_INVALID_ARGUMENT` instead of a caught panic.
- Three types keep one public path each. None of the removed paths was in
  a release (they were added and dropped on the development branch before
  this release), so only code written against that branch changes:
  `ponko::texture::DrawSpace` is `ponko::debug::DrawSpace` (also in the
  prelude); `ponko::input::TouchShow` is `ponko::ui::touch::TouchShow`;
  `ponko::ui::Bezier` is `ponko::skeleton2d::Bezier`, the one easing curve of
  skeleton clips and UI transitions. Change the `use` line; the types, their
  values and the C and C++ names are unchanged.
- Caller-reachable panics across the engine and ECS (queries, views,
  resources, reflection, jobs, UI, animation, networking, instances, GPU
  buffers, 2D skeletons) now say how to fix the call, not only what failed.
- `ComputePass::for_buffer` sets `target` to `RenderTarget::NONE` (it was a zero-sized `Texture` with id 0).
- C: `ponko_ctx_reflection_camera` refuses a texture that isn't a render target of the game (`PONKO_INVALID_ARGUMENT`), as the target camera, compute pass and target effect lists already did when set.

**ECS and systems**
- A world reclaims empty archetypes: it keeps at most 64 of them, or as
  many as it has archetypes with entities if that is more, and reclaims
  the one empty longest past that. The reclaimed slot and storage are
  reused for the next new component set, so a world whose entities keep
  visiting new component combinations levels off in archetypes and memory
  and stops allocating once it settles (`crates/ponko_bench/SPARSE_SET.md`).
- Queries that scan skip empty archetypes cheaply: in a world with many
  empty archetypes (more than 16, and more than one per four with
  entities), they visit a list of the archetypes with entities, rebuilt
  after a structural change, instead of reading every archetype. The
  dense loop of the 20 % sparse-set benchmark takes about two thirds of
  the time it did (`crates/ponko_bench/SPARSE_SET.md`).
- The parallel scheduler (`Game::parallel_systems`) batches view systems by
  their dependencies instead of only when they are next to each other.
  Between phases, explicit barriers and systems that run alone, each view
  system goes to the earliest stage after every earlier system whose work
  it must see (a conflict or `Game::order`), and never ahead of an earlier
  system whose order would show (`SystemAccess::conflicts_with`, such as two
  senders of one event type). A system that interacts with nothing
  registered between may so run ahead of systems registered before it.
  Results, change ticks and the order of commands and events are the same
  as with the scheduler off, whatever the number of threads. Producers and
  consumers registered in pairs now take two stages instead of five
  (`schedule/interleaved/4x16000/heavy`: see `crates/ponko_bench/SCHEDULE.md`).
  `&mut Ctx` systems still split batches, and the scheduler stays off by
  default. Systems that talk through something undeclared need
  `Game::order` to keep their order, as they already needed it to stay out
  of one batch.
- Cache required column numbers in indexed ECS queries when matching
  layouts have expensive lookups. Use the cached numbers in typed,
  view, chunk, and parallel queries.
- Spawning is faster: a bundle's values are moved with copies of their
  own, constant size instead of `memcpy` calls sized from the column, and
  `spawn_batch` looks its bundle up once per batch instead of once per
  entity. At 100k entities: one-by-one spawns 0.71× the time they took,
  `spawn_batch` 0.46×, spawn-and-despawn churn 0.83×; against the code
  before required components and the change summary, 0.75×, 0.50× and
  0.84× the instructions.
- A spawn that fills a gap (its bundle lacks a component it requires)
  reads one cache entry instead of two and reserves one row instead of
  two, and a batch of such bundles without `on_add` hooks runs one loop
  like a plain batch. Filling the gap now costs nothing measurable in a
  batch and 0.006 ms per 100k spawns in churn (0.032 before); one by one,
  about 2 ns per spawn, 39 instructions instead of 81. Gap-filling spawns
  take 0.73× (one by one), 0.43× (batch) and 0.78× (churn) the time they
  took.
- Despawning from an archetype with tracked components logs the removals
  and moves the last row's stamps in one pass, and no longer marks the
  despawned row's block as changed: a `Changed` loop afterwards still
  skips that block if nothing in it changed. With one tracked component
  a despawn costs 0.97× what it did before the change summary existed
  (it was 1.07×), with two 1.03× (it was 1.17×); despawns from untracked
  archetypes run 4% fewer instructions.
- Queries with one or two `Option<&T>` / `Option<&mut T>` fetches pick a
  loop per archetype for the components it has, instead of testing them
  on every entity: `ecs/optional` (`(&mut Pos, &Vel, Option<&Boost>)`)
  takes 0.22 ms instead of 0.51 ms at 1M entities and 0.022 instead of
  0.048 ms at 100k, as fast as hecs. This applies to `for_each` in tracked
  worlds and in views too; parallel loops and queries with three or more
  options keep one loop.
- The numbers above: release builds on an Apple M4 Max shared with other
  builds (load 75–185), the code before and after these changes interleaved,
  8 rounds (faster in 7 or
  8 of them on every row above), and instruction counts from one process
  that links both versions. `crates/ponko_bench/ECS.md` ("Costs won back since the
  baselines") and `REQUIRED.md` have the tables; the guide's performance
  chapter states the costs that remain ("What ECS features cost").
- `d3::Physics::cast_ray_all` orders equal distances by entity id, and 3D
  casts and overlaps with a non-finite origin or rotation, or a negative or
  NaN `max_distance`, find nothing, as in 2D.
- C++ `ctx::cast_ray_filtered` and the 2D `ctx::overlap_ball` honour the
  `filter2d`'s collision groups.
- `par_for_each`, `par_for_each_indexed` and `par_collect_into` with one or
  two `Option` fetches pick the loop per archetype, as `for_each` does, and
  keep the closure's captured values out of the loop: about 0.15× the
  instructions per entity, where they used to test every row.
- A `Changed`/`Added` filter after inserts and removes reads only the
  blocks where something changed: moving an entity keeps its stamps exact
  instead of marking its new block of 64 rows as changed.
- Change filters over many archetypes cost about 0.6–0.8× the instructions:
  the row filter is filled in place for each archetype.
- Despawning entities with tracked components costs less: removal logs
  keep one entry per tick, not one tick per entity (0.94× the instructions
  with two tracked components).
- Hierarchy propagation indexes each level on the calling thread, which
  was as fast as all cores and doesn't wait for busy worker threads.
- `World::spawn_at` takes back a free slot that was set aside for command
  buffers, unless a buffer holds it.
- `par_for_each`, `par_for_each_indexed` and `par_collect_into` decide
  between the calling thread and all cores by the work they measure, not
  by the size of their archetypes: every loop of more than one chunk times
  its first rows and stays on the calling thread if the whole loop would
  take under about 50 µs. A cheap loop over a big archetype no longer wakes
  the pool (a 16,000-entity `pos += vel * dt` loop: 0.1× the cycles); a
  heavy loop goes parallel after 64 timed rows.
- Hierarchy updates (attachments) run each pass on all cores only when its
  measured work is worth it, so a small update stays on the calling thread
  and doesn't wait for busy worker threads (2,000 moving roots with four
  children each: 0.27× the cycles).
- `par_collect_into` into a type that needs `Drop` keeps the closure's
  captured values out of the loop and vectorizes: 0.36× the instructions
  per entity.
- `for_each` and the parallel loops with three or four `Option` fetches
  pick the loop per archetype, as with one or two: about 0.6× the cycles
  of testing each row.
- `follow_attachments` turns on change tracking for `Attached`,
  `Attached3D` and `Children` the first time it runs.
- The editor places 2D attachments right after an edit too, not only 3D
  ones. Its 3D transform helper also accepts chains deeper than 64 links.
- The transform hierarchy finds the children of a despawned parent through
  an `on_remove` hook on `Children` instead of scanning every link: a
  despawned parent costs its own children, not one pass over all entities
  (`hierarchy/100k-pairs-parent-churn`: 0.41 ms instead of 0.51 ms at best).
- Physics bodies bring what they need: `d2::RigidBody` and `d3::RigidBody`
  require a transform and a `Velocity` (an entity without them gets one at
  the origin, at rest), and `d2::Collider` and `d3::Collider` require a
  body, `RigidBody::fixed()` unless one is given. A spawn names only what
  differs (`(Model::new(look), Collider::mesh(solid))` is fixed ground);
  the tutorials lost their `Velocity::default()` and `RigidBody::fixed()`
  lines. Every body now has a `Velocity` component, which reads as zero
  where one was missing before, so simulations step the same. An entity
  with a collider and no body, which physics ignored before, is now a
  fixed body (rapier's convention); saves that hold one load as one.
- The guide's tutorial and reference pages, and the service docs, name the
  service calls (`ctx.audio.play`, `ctx.gfx.draw_text`, `ctx.gfx.load_texture`,
  ...) instead of the deprecated `Ctx` shortcuts, which stay until 0.3.
- The hierarchy update places a level with many children (a wide tree,
  many moving roots) in one pass over the attached entities on all cores,
  and compares the roots on all cores. 100,000 roots with one child each,
  all moving: 0.63–1.1 ms per update instead of 2.1–3.9 ms. Per tick of a
  game that moves the roots and follows the attachments, that is 0.70–1.3
  ms, against 0.87–1.3 ms for the attachment update the hierarchy replaced
  (busier runs of it up to 2.6 ms). 100,000 children of one moving root:
  0.20–0.40 instead of 0.71–1.1 ms (3D: 0.26–0.52 instead of 0.97–1.3
  ms). A 100,000-long chain is 2.4–2.5 instead of 2.3–2.4 ms, at the edge
  of the noise. Load average 60–90 during these runs.
- `Children` keeps its first four entries in the component: linking
  100,000 new children to 100,000 parents allocates about 100 times
  instead of 100,151, and the first update takes 5.5 ms instead of
  11.3 ms; 100,000 attachments spawned and followed for 10 ticks:
  860 allocations instead of 100,922.
- `World::allocated_bytes` counts the change summary: 4 bytes per 64 rows
  of capacity of each tracked column.
- The offscreen render test rigs keep their `Ctx` and renderer on the heap,
  so test threads stay well below their 2 MiB stacks.
- C and C++: the 3D physics queries (`ponko_ctx_cast_ray3d`,
  `ponko_ctx_cast_ray3d_all`, `ponko_ctx_cast_sphere3d`,
  `ponko_ctx_cast_capsule3d`, `ponko_ctx_cast_box3d`, the `ponko_ctx_overlap_*3d`
  calls, `ponko_ctx_closest_point3d`, `ponko_ctx_penetrations3d` and their `_ex`
  forms) work inside a `ponko_query` / `world().each` callback instead of
  returning `PONKO_BUSY`, so a loop over the characters can check the ground
  under each. They stay refused in `ponko_query_par`, and so do the calls that
  change physics. Matches the 2D queries.
- The busy-flag handling of commands and events in a query callback shares one
  helper with debug drawing, sounds and the cameras (no behaviour change; the
  "in use" error text is now the same for all of them).

**Rendering**
- `Graphics::shader_status` and the shader status types
  (`ShaderStatus`, `ShaderOutcome`, `ShaderDiagnostic`, `ShaderKind`) no
  longer need the `3d` feature, and are also exported from `ponko::render`
  without it.
- `Vec4` and `vec4` are in the prelude without the `3d` feature.
- `Systems::UPDATE_PARTICLES` (`systems::update_particles`) is installed
  without the `3d` feature too, and advances 2D emitters as well as the 3D
  ones and trails.
- Screen-space reflections follow normal maps, decals and normals set by
  material shaders: a thin pass writes the lit normal while reflections are
  on (about 0.1 ms at 1080p on a fast GPU, 4 bytes a pixel). With `ssr` at 0
  nothing new is allocated or drawn.
- The particles of all alpha emitters are sorted far to near together, so
  overlapping emitters mix correctly; additive emitters are drawn whole at
  their centre's depth. Neighbouring particles of one image and blend share
  a draw.
- See-through instance batches on the CPU path draw the main camera's
  visible instances far to near (across their distance levels). Measured:
  about 2 ms a frame for 100,000 visible instances while the camera moves,
  nothing while it stands still.
- The 3D guide's point and spot light shadows section says why they keep
  the 3×3 filter instead of the sun's contact-hardening edges, with a
  measured spike: up to 1.6 ms more in the opaque pass with nine shadowed
  lights over the screen at 1080p, against the sun's 0.3 ms.
- The 3D renderer's shared mesh buffers no longer keep a CPU copy of every
  mesh ever uploaded: new geometry waits on the CPU only until the next
  frame uploads it, and growing the buffers copies them on the GPU. On
  WebGL2, where index buffers can't be copied on the GPU, only the indices
  keep a CPU copy.
- Textures no longer keep a CPU copy of their alpha channel. Each texture
  uploaded from pixels keeps a 24-byte summary (size, fully opaque, a hash of
  its alpha); a full mask, one byte per texel, is read back from the GPU once
  and kept only for a texture that a cutout material reads as its base map,
  when a region upload first needs it. Uploads that leave alpha alone still
  keep cached cutout shadows. In a measured 300-texture set (157 MB of masks
  before), the bookkeeping holds 3.2 MB, nearly all of it the 12 cutout masks.
- `perf.get`'s `assets.gpu` (the game's assets only) now carries
  `all_gpu_memory_bytes`, the engine's whole GPU total from
  `render_inspector.gpu_memory_bytes`, and a note saying where the frame
  targets, shadow maps and the rest are.
- The 3D camera draws with reverse-Z depth (a 32-bit float, near plane at 1, far plane at 0,
  `Greater` test, cleared to 0), so the depth precision hardly depends on `Camera3D::near`. A
  first-person camera can use `near = 0.01` with a far plane kilometres away: one depth step at
  400 m is now about 30 µm instead of about a metre, and ambient occlusion, contact shadows,
  reflections and temporal reprojection there no longer crawl. The world beyond the near plane
  draws the same at `near = 0.01` as at `0.1`. Every pass that reads the depth follows (ambient
  occlusion, contact shadows, reflections, temporal antialiasing, depth of field, the sky,
  soft particles, screen effects, global illumination's views, occlusion culling, target cameras'
  oblique clip planes). `Camera3D::far` still clips and culls.
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
- GPU skinning poses all skinned models of one mesh with one compute dispatch
  (up to 512 models each) into shared vertex buffers, instead of one dispatch,
  bind group and vertex buffer per model. Passes draw a posed model from the
  shared buffer at its first vertex, so consecutive models of one mesh keep
  their buffers bound. On an Apple M4 Max, 1,000 skinned figures of 16 joints
  (the `animated` render scene at scale 2.5) went from 3.8 to 0.06 ms of
  skinning and from 6.0 to 2.9 ms of GPU time a frame, with 2,847 vertex and
  index buffer changes a frame down to 22. Images are unchanged. The CPU
  fallback (no compute shaders) still poses each model into its own buffer.
- Vertex colours (`MeshData::colors`, glTF `COLOR_0`) are kept as half
  floats instead of 8 bits per channel: red, green and blue above 1 stay
  brighter than white (alpha stays 0 to 1). A mesh corner on the GPU takes
  48 bytes instead of 44 (about 9% more vertex memory); drawing time
  measured the same.
- Ambient occlusion shades open ground seen at a grazing angle a little
  lighter, about 2 levels in the reference scene. Before, it counted part of
  open ground as occluded. Creases stay as dark as before. At half
  resolution (the High and Ultra presets) the AO passes cost the same as
  before, or slightly less. At full resolution the bilateral pass costs
  about 10% more (+0.04 ms at 1280x720 on an Apple M4 Max).
- A glTF material with `KHR_materials_transmission` keeps its own alpha mode and alpha;
  it was loaded as `Blend::Transparent` with its alpha lowered by the transmission.
- A glTF texture transform is a material value now, applied as the model
  is drawn, instead of being baked into the mesh's texture coordinates at
  load. Static files draw as before.
- Cutout materials cut at `MaterialDesc::alpha_cutoff` (glTF's
  `alphaCutoff`), instead of always at 0.5.
- Material descriptions are kept for every material, not only in editor
  builds.
- `Graphics::render_icon` (C `ponko_ctx_render_icon`, C++ `ctx.render_icon`)
  draws an icon in one frame, not two: an icon asked for in setup is in its
  target after the game's first frame.
- A WGSL error the compiler's validator finds (material, sprite, sky and
  screen-effect shaders, compute passes) names types, functions, parameters
  and variables as the source writes them, instead of the compiler's
  internal handles (`Function [4]`, `Some([10])`, `naga::ir::Expression`):
  "`fragment` returns `vec3<f32>` but is declared to return `vec4<f32>`".
  A return type, a call's argument type or count, an operation on two types
  that don't go together (with the conversion or swizzle to write), an
  assignment, `select`, an index, an `if`/`switch` condition, a write to a
  read-only buffer, a uniform buffer's layout and a binding used twice get
  messages of their own; any other keeps the compiler's text without the
  handles. The caret's label gives the expression's type and the note the
  function. The line, column and caret are unchanged, and so is
  `ShaderDiagnostic`: this reaches `shader_status` (Rust, C and MCP), the
  hot-reload notice and the `create_*` panics and errors alike, and
  `ponko cook`'s shader check words its errors the same way.
- Screen-space reflections get their normals from the depth prepass, which
  writes them into a second color target as it draws each solid and
  cut-out surface, instead of from a separate pass that drew every solid
  surface again. The images are the same. On an Apple M4 Max (1280×720,
  4× MSAA, `high`) the render benchmark's frames got up to 0.6 ms shorter
  (`static-world` 5.17 → 4.53 ms) and `multi-material` makes 513 fewer
  draw calls (CPU 2.28 → 1.85 ms). The render baseline is recorded again.
- A material shader's pipelines (about ten per set) are built at once on the
  frame pool instead of one after the other, natively on every backend but
  GL. Each pipeline compiles its shaders when it is created, so this shortens
  the 3D renderer's start and the first use of a shader: on a 6-core desktop
  a water render test (three renderers) went from 9.5 to 4.6 s on Direct3D 12
  with DXC and about halved on Vulkan. With FXC, Direct3D 12's default
  compiler, the gain is smaller (8 to 37%), as it mostly compiles one shader
  at a time. A pipeline the GPU rejects is built again on the calling thread,
  so a game shader's error is reported as before.
- Global illumination's capture no longer collects particles it never
  draws. Transparent surfaces and particles stay out of the probes' views;
  the guide says why and what drawing them would cost.
- Screen-space reflections trace from a single-sampled copy of the depth,
  made by a new full-screen pass just before the trace (`reflection depth`
  in the GPU profiler, 4 bytes a pixel). Reading the multisampled depth at
  every step was most of the trace's cost: with the copy the trace takes 48
  steps instead of 32 and still costs less than before, about 0.38 to
  0.46 ms for copy and trace together at 1920x1080 on an Apple M4 Max, down
  from 0.52 to 0.63 ms (back near its cost before the contact reflections
  were made solid). Contact reflections without TAA are also less noisy.
- The sky kit's `Low` and `Medium` cloud presets march one pixel of each
  2 x 2 block of the cloud image a frame, in turn, and keep the reprojected
  history for the rest. At 1920x1080 on an Apple M4 Max, `Medium` (the
  default) now adds 1.43 ms to a frame instead of 2.87 ms (medians of ten
  runs; 1.1 to 1.65 ms saved in every run). Clouds moving fast across the
  screen are a little softer for a few frames.
- Contact ripples work on WebGL2: without compute shaders the CPU steps the
  ripple simulation and uploads it, so `Water::touch` now ripples the water
  there too.
- The terrain example (`examples/terrain`) draws its lake with the water
  kit (a planar mirror and the underwater view) instead of its own shaders,
  and an offscreen GPU test covers its scene.
- A window's 3D views go to the GPU in one submission with the main camera
  instead of one each: every frame's small buffer and texture writes are
  copied into place by its own first command buffer, so each view still
  reads its own camera, lights and culling data. Growing an
  instance pool or the shared mesh arena joins that submission too. The
  render benchmark prints command buffers and submissions a frame and writes
  `submits` to its JSON.
- Light textures (light and decal records, per-cluster light and decal lists) are written
  through the frame's batched upload instead of one `write_texture` each.
- What remains of a frame's allocations is inside wgpu (about 24 for each
  pass, a few for each copy, about 25 for each submission); the guide's
  numbers for views were measured again (town scene: 767 for one camera, about
  440 to 495 for each full view, 159 for a minimap).
- Baked lighting files are version 2 (packed with LZ4, compressed pages
  as blocks); version 1 files still load. Baked lighting's pages are
  bound in the material's second texture array slot (`custom_array1`)
  when it sets none, and its table and probes in a smaller array in the
  third; a material with its own second array takes the probes' light.
  `BakedLighting::gpu_bytes` counts the compressed pages.
- Screen-space reflections assume a thing as deep as the screen shows it
  tall across the view, up to the old guess that grew with distance: far
  thin things are reflected 1.23 to 1.36 times as wide as in a true
  mirror, from 1.45 to 1.63 (rails 8 to 32 units away, 1920x1080, no
  TAA), and a thin bar near the floor 1.4 times, from 2.3. About 0.1 ms
  more for the trace at 1920x1080 on an Apple M4 Max.
- A game times its GPU passes on one frame in 16 instead of every frame,
  except while something reads every frame: profiling
  (`ctx.perf.set_profiling`), the HUD, a trace, a `PONKO_BENCH` run or
  automatic quality, which keep every frame timed. Timestamp
  writes cost the graphics library a few heap allocations for every timed
  pass. `ctx.perf.gpu_passes()`, `RenderStats::gpu_ms` and the frame
  statistics stay smoothed as before: each sample now weighs as much as
  the frames it stands for, so they settle at the same values, a little
  later after a change. The render inspector's `gpu_latest_frames_ago`
  shows how old the newest sample is.
- Realtime global illumination settles sooner after a light changes: the
  probes whose light is still changing get more of the per-frame updates
  (the renderer reads back how much each probe's light changed, a frame or
  two late). With a lamp switched on in a closed room inside the default
  probe grid, at the default 4 probes a frame, the room is within 5% of
  its settled light after 400 frames instead of 1200.
- `GpuTiming`, `Perf::set_gpu_timing` and `Perf::gpu_timing` are no longer
  Rust-only: their rows leave `scripts/rust-only.txt` and the "Still Rust-only"
  table of the C and C++ chapter.
- The written list of checks (`scripts/checks.yml`) builds
  `tests/ui_access_window.rs` in the accessibility step (built, not run: it
  needs a person), and runs the native crash backtrace tests on Linux in a
  build with frame pointers (`-C force-frame-pointers=yes`,
  `PONKO_CRASH_TESTS_FRAME_POINTERS=1`), which checks the whole chain of
  frames there.
- In a web build with threads, the engine's own parallel passes run on the
  frame's workers, through the same runner as parallel queries, instead of
  calling the thread pool from the page's thread (where they ran alone):
  spatial grid rebuilds, flow field directions, 2D skeleton drawing and its
  copy into the draw list, and 3D character moves. The browser test checks
  that the grid rebuild and the flow field went there.
- In a web build with threads, the 3D renderer's culling (each model
  compared with its last frame and culled again) runs on the frame's
  workers: it sizes its work by the frame pool's threads instead of the
  page's thread pool, which has one thread and made it take its one-pass
  path. Saving a world as JSON (`World::save`, `save_world`,
  `save_world_in_background`) builds its entities' JSON there too; the
  text is byte-identical to a one-thread save. The browser test checks
  that both went there and give the same result as on one thread. On the
  desktop nothing changes.
- The guide's cost table for `Post::ssr_thin_objects` has the coarse depth
  pass's own GPU time (about 0.03 ms at 1920x1080 on an Apple M4 Max; it
  was counted in the frame only), and an offscreen test checks that the
  pass is timed. Three ways to make the thin-objects trace cheaper (two
  coarse levels instead of four, at most four depth reads in a block a
  step, one coarse-depth look for up to four steps ahead) were measured
  and none came near the 0.2 ms over off that would put it in the High
  preset, so it stays on in Ultra only; the guide has the numbers.
- A UI group fading as one image (`Ui::set_fade_as_group`) draws into a
  layer the size of what it draws (children reaching out of its rectangle
  included), snapped outwards to whole pixels and cut to the window, instead
  of a window-sized one: a 400 x 300 group on a 4K screen takes 0.6 MB
  instead of 33 MB. It draws the same pixels (checked against the
  window-sized layer on the GPU). Layer textures are kept from frame to
  frame and made in steps of 64 pixels, so a group that moves or changes
  size a little while it fades makes no texture after its first frame.
  On the GPU the layer's pass (`ui layers` in the pass timer) takes about
  0.05 ms for a 480 x 400 menu at 1080p and at 4K, where the window-sized
  layer took 0.09 ms and 0.11 ms (Apple M4 Max, medians of three runs on a
  busy machine; the guide's "Fading a group as one image" has the table).
- On the CPU path, instance cells of one mesh are drawn together: the
  visible instances of neighbouring ranges of the cells' shared vertex
  buffer join into one draw, so a field of 1,000 visible cells loaded
  together draws a few times a frame instead of once per cell and pass.
  Cells are still culled one by one.
- On the GPU-driven path, instance cells keep their records in pools of
  their own, each cell's block starting on a record (96 bytes) instead of
  on the storage binding alignment and a record (768 bytes where bindings
  align to 256): a small cell's records are no longer padded by up to
  768 bytes.
- The thin-objects reflection trace walks the blocks of its coarse depth
  (a hierarchical, "Hi-Z", march) instead of the trace's long steps: about
  a third of its cost before at 2560x1440 (+0.19 ms over off in High, from
  about +0.6 ms), with the same results in the thin-object tests (4.7% of
  the reflected light differs on the rails, 0.04% on the bars). A smaller
  trace target was measured and rejected (see the 3D guide).
- A system's allocation count is its own thread's while it runs, plus what
  the workers of its ECS parallel loops (`par_for_each`, `par_collect_into`,
  a `ViewGroup` on threads, parallel queries from C and C++) allocate for
  them; it used to be the whole process's, so another thread's allocations
  could land in it. `perf::thread_alloc_counts()` includes those loops'
  workers too. Free while profiling is off; a few counter reads per loop
  while it is on. The tests of allocation-free systems (C++ query filters,
  `ponko_ffi`'s filters and asset handles) no longer retry or take the
  quietest of several runs.
- Particles are drawn among the see-through models, far to near, instead
  of after all of them: a particle behind a pane of glass takes its tint
  (additive emitters too, whole, at their centre's depth), one in front
  is drawn over it as before. In the order-independent transparency
  evaluation's particles-through-glass scene the mean error falls from 4.7
  to 1.3 levels. Each stretch of particles between two see-through models
  costs one more draw, and models of one mesh on either side of it are
  drawn apart; ribbons, lines, 3D text, see-through instance batches and
  crowds keep their place after the see-through models.
- `HdrStatus::reason` is `Off` for a window whose HDR output is off,
  whether or not it was ever on; `HdrOffReason::NotRequested` is no longer
  reported (kept so code matching on it builds).
- wgpu-hal 30.0.2 (from 30.0.1): 30.0.1 crashed configuring a Metal surface
  for PQ output.
- Ribbons and lines (`Trail`, `Line`) and the instances of see-through
  instance batches sorted on the CPU are drawn far to near together with
  the see-through models and the particles, by view depth, instead of
  after all of them: a ribbon or a particle between two panes of glass is
  tinted by the far one and dimmed by the near one, whether the panes are
  models or instances of one batch. Crowds and GPU-driven see-through
  batches follow the last see-through model, and 3D text comes last, as
  before.
- See-through models of one mesh are drawn together, in one instanced
  draw, where the models drawn between them in far-to-near order cover
  none of the same pixels (by screen rectangles from the meshes' bounding
  spheres): the same picture, to the byte, with fewer pipeline, material
  and draw changes. Skinned models, models drawn from a GPU buffer, models
  whose shader has a `vertex` function and meshes with several
  see-through parts are never moved. Particles, ribbons, lines and text
  set a pipeline or image only when it changes.
- The see-through list groups more draws: a model's screen rectangle is
  where its mesh's box, turned and placed by the model, and its bounding
  sphere both land, so thin and flat models near each other or near the
  edge of the view share draws (the `alpha-heavy` benchmark scene: 2,150
  draws a frame instead of 2,480, the same picture to the byte).
- Particles, ribbons and lines are no longer barriers in the see-through
  list: a model moves past one it doesn't overlap on screen, the next
  particle, ribbon or line joins the draw before it past what it doesn't
  overlap, and additive ones pass each other even where they overlap.
  Order that changes the blended picture is kept.
- The GPU timer times as many passes a frame as the frame has, up to 336
  (it timed at most 32): it grows its query sets before the next frame when a
  timed frame had more passes than room, so a frame with several views, such
  as two eyes, keeps every pass's time. The few frames in flight while it grows
  are not timed.
- The profiler reserves its whole frame history when profiling turns on, instead
  of growing it by doubling while it fills, so no frame allocates for it; clearing
  the history keeps its memory.
- While `Upscaler::Temporal` draws the frame smaller than the window,
  material textures and decals are sampled with a mip bias of `log2` of
  the render scale, so they keep the detail of the window's resolution
  instead of reading a coarser mip level. Shadow maps and the stretch
  keep no bias.
- The temporal antialiasing resolve reads one depth sample a pixel
  instead of every sample: about 0.8 ms less GPU time at 2560x1440 on an
  Apple M4 Max (1.08 ms instead of 1.84 to 2.06), with the same pictures.
- The motion pass is skipped on frames where nothing draws into it (no
  moved model, moving `vertex` function, crowd or held item) and its
  image already holds only zeros. The guide's "Temporal antialiasing"
  section has the costs.
- Screen-space reflections blur by a cone: the footprint grows with the
  surface's roughness and with how far each reflected ray went before it
  hit, so a contact reflection on a rough floor stays sharp while a
  distant one blurs, instead of one blur per roughness. Footprints under
  two half-size pixels blur the trace's own level, so thin bright details
  keep their brightness; rays that find nothing take their nearest hits'
  footprint instead of the coarsest level's halo. 1920x1080, Apple M4 Max,
  High preset screen effects: +0.08 ms a frame (+0.14 ms with reflections
  and no ambient occlusion, which now writes the first level every frame).
- Ember Hop's camera is a `FollowCamera` with `PixelPerfect`, and Shard Grove's
  a `ThirdPersonCamera` that no longer goes through walls, with an
  `OrbitCamera` on the title, in place of their hand-written camera code.
- A camera's `viewport()` is the size of its `rect` (the window's size, as
  before, unless the rect is changed), and its screen and world
  conversions work in window pixels inside that rect.
- `Ctx::mouse_world` and the inspector's picking go through the camera
  under the mouse.
- The `High` and `Ultra` graphics quality presets use
  `ShadowFilter::ContactHardening` instead of `Softest` (a game that applies
  them gets contact-hardening sun shadows; C reads `shadow_filter` 4 back).
- `Lighting::shadow_refresh` now sets how many frames the far cascades hold
  the box they are drawn in; whether a map is drawn again depends only on
  whether what it holds changed (the rules are in its API docs). A shadow
  view is kept without comparing its casters while the collector's numbered
  caster list for it is unchanged, so an unchanged frame costs no instance
  comparison.
- An instance batch or instance cell made, changed or freed draws again only
  the shadow maps its box can be seen in, instead of every shadow map: a
  streamed cell no longer redraws the lights' views and cascades it doesn't
  reach.
- Instance cells cost fewer draw calls. On the GPU-driven path, where the
  device has `Features::INDIRECT_FIRST_INSTANCE`, the cells of one batch
  share one bind group and are drawn with one multi-draw-indirect call per
  material and view, however many are visible (each cell was at least one
  draw per material and view); devices without it draw each cell on its
  own, as before. The engine now asks for `INDIRECT_FIRST_INSTANCE` when
  the adapter has it.
- On the CPU path, instance cells take ranges of one shared vertex buffer
  that hands a freed cell's range to the next cells, instead of a vertex
  buffer each; consecutive cells bind it once.
- Screen-space reflections march in steps that grow from 1.5 pixels near
  the surface toward the ray's end, and whether a ray hits no longer
  depends on its per-pixel jitter: without TAA, contact reflections at low
  roughness are solid instead of grainy (an offscreen test bounds the noise
  at 1920x1080). The trace costs about 0.14 ms more at 1920x1080 on an
  Apple M4 Max (0.47 to about 0.6 ms).
- Rays that miss take the mean footprint of the nearest hits from two small
  images of the hits (4x4 and 16x16 of the trace's pixels, made by the
  reflection levels' dispatch) instead of a search in eight directions:
  wide blurred reflections fade out softly where they used to end in
  ragged, specked edges.
- Without ambient occlusion, the opaque pass reads the reflections'
  half-size footprint image directly where the device has a texture slot
  to spare, instead of a full-size pass copying it every frame.
- Global illumination: batches of distance levels on the CPU path keep
  their 4 bytes per instance for the probes' size cut only while global
  illumination is on: built the first frame it is on, dropped once it has
  been off for 120 frames. Off, it costs nothing per instance. The batch's
  bounding spheres they are made from are shared with the renderer, not
  copied.
- Global illumination: on the GPU culling path, the probes' face views
  leave out instances narrower than one probe pixel from every probe
  updated that frame, as the CPU path does per probe. On a field of
  160,000 small instances at 1920x1080 (16 probes a frame), the instances
  drawn in those views fell from 240,578 to none and the GI passes' GPU
  time from about 53 ms to about 2 ms (measured on a busy machine).
- An instance batch whose shadows are cut by distance tells whether a moved
  camera changes what a kept shadow map holds instance by instance, not
  cell by cell: a cell across a band edge no longer redraws the maps it is
  in on every camera move, only when one of its instances crosses the
  edge. Such a batch on the GPU path keeps its instances' positions on the
  CPU for it (16 bytes an instance).
- The C++ rigs' `flat_forward` / `flat_right` call the C functions instead of
  working them out inline.
- Ember Hop, Shard Grove and the 2D and 3D tutorials follow their heroes with the
  engine-fed rigs and no camera system.
- 3D frames make fewer heap allocations and command buffers: the town
  scene of the render benchmark went from 1222 to 745 allocations a frame
  with one camera (1844 to 1255 with two, 2788 to 2113 with four) and from
  13 to 2 command buffers.
  - The small buffer writes made while a frame is prepared are gathered
    into one staging write and copied into place at the start of the
    frame's first command encoder, instead of one `write_buffer` each.
  - Consecutive small point and spot light shadow views are drawn in one
    pass of the shadow atlas.
  - A frame's passes are recorded into a few command encoders of about
    equal work (one for a small frame, at most one per core and eight in
    all), instead of one per pass, so heavy frames still record in parallel.
  - The occlusion grid and its bind group are reused between frames.
- The render benchmark has `PONKO_RENDER_VIEWS=minimap` (a quarter-sized
  view with the costly effects off) and `PONKO_ALLOC_SITES=n`, which prints
  where a frame's allocations are made (each one's call stack, grouped).
- 3D materials, material shaders, compute passes, screen effects and texture
  arrays reuse freed slots under new handle IDs. Existing references to freed
  resources keep their fallback behavior after reuse. Screen effects drop
  their pipelines when freed.
- The shared mesh arena uses less GPU memory. Its buffers grow to a quarter
  more than they need instead of to the next power of two, which could leave
  up to half of each buffer empty; freeing the meshes at the end of the arena
  lowers its high-water mark, and a buffer left far larger than its contents
  (more than about 1.56 times) shrinks again with a GPU copy. In a streaming
  test of 768 live meshes (412 MiB of geometry) the arena's GPU memory fell
  from 542.6 MiB to 423.2 MiB. Drawing is unchanged.

**Animation**
- Motion vectors (TAA, temporal AO) follow skinned and morphing models as
  they bend: the last frame's skin matrices and morph weights are kept per
  model while either is on, a model whose pose changed is posed a second
  time as it was (positions only, on the compute and the CPU skinning
  paths), and the motion pass reads where each corner was from there. A
  limb that swings while the character stands still writes its own
  motion. A held pose costs nothing more; a pose that changes every frame
  costs one more skinning dispatch per model (about 1 ms of GPU time for
  400 moving figures of 16 joints on an Apple M4 Max, see the guide).
- Motion vectors (temporal antialiasing, temporal AO) follow what a material's
  `vertex` function does: the motion pass runs it as of this frame and as of the
  last, with the last frame's `frame.time` and, for a buffer that keeps its last
  frame, its `items`. Swaying foliage, waves and buffer particles no longer
  smear under TAA. Models whose shader's `vertex` reads `frame.time` or `items`
  are drawn into the motion image every frame (measured: 0.15 ms more GPU time
  for 30,000 swaying trees on an Apple M4 Max); other shaders, and frames with
  TAA and temporal AO off, cost nothing more.
- Unity importer: LOD groups with Animate Cross-fading get a `LodFadeTime` (`Importer::lod_fade_time`, Unity's 0.5 s by default) instead of a band crossfade and a report entry.
- Unity importer: a clip whose humanoid curves are only in baked runtime data says to resave it from the editor (those name muscles by index).
- The animator's panicking setters and `play`/`crossfade` name the method,
  the id and the reason in their panic message (for example
  `Animator::set_float: parameter 9 is not a float parameter`).
- glTF animation import and the C API's skeleton, morph-track and animator
  calls use the checked versions, so bad data is an error, never a panic.
  The C animator calls' error messages now name the id (for example
  `parameter 2 is not a bool parameter`).
- 2D skeletons: deforms and constraint mixes blend across crossfades and
  1D blends, where deforms used to switch at the half-way point.
- Spine import evaluates path and physics constraints, path attachments
  and constraint timelines instead of leaving them out with a warning.
- `Skeleton::set_ik_mix` applies while no playing clip keys that
  constraint's mix.
- `skeleton2d::BonePose` is `#[non_exhaustive]` with a `shear` field; build
  one with `BonePose::new`, `BonePose::at` or `BonePose::IDENTITY`.
- `scripts/check-ffi-coverage.py` no longer counts a 3D function (such as
  `ponko_ctx_two_bone_ik`) as the binding of a 2D method with the same name.

**Assets, scenes and saves**
- The `coin_dash` reference game keeps its progress and settings through
  `ctx.storage` (in the browser too) and has no path code of its own.
  `PONKO_DATA_DIR` replaces its `COIN_DASH_SAVE` variable; the files stay
  in the same `coin-dash` folder.
- `SkinOf`, `AttachToJoint` and (with 3D physics) `Joint3D` are reflected,
  so scenes, saved worlds and prefabs keep them; their types implement
  serde's `Serialize` and `Deserialize`.
- The editor's undo of a placed prefab removes its parts too.
- `load_texture` and the other texture loaders find an image's format from
  its first bytes, not its extension (TGA, which has none, from the
  extension), so a JPEG named `.png` loads.
- A file in a format the build leaves out now fails with an
  `ErrorKind::Unsupported` error that names the `image-*` feature to turn on,
  instead of a generic decode failure.
- Builds without default features no longer include a JPEG decoder;
  `scripts/feature-deps.sh` checks it.
- A glTF file that requires an extension the loader doesn't read fails
  with an error naming the extension (or the cargo feature that reads it);
  one it only uses loads, with a warning naming it.
- `ponko package` ships the files under `assets/` as one `assets.pack` (the
  reference games Coin Dash, Ember Hop and Shard Grove included); `--no-pack`
  or `pack = false` in `[package.metadata.ponko]` keeps them loose, and
  `pack = { exclude = [...], textures = true }` sets packing up. A web
  package has no `assets/index.json` when it is packed.
- `scripts/package-coin-dash.py smoke` checks the files in Coin Dash's
  `assets.pack` against its content notices, and that a package without the
  pack fails naming the missing file.
- In the inspector's editor, Ctrl+Shift+Z redoes instead of undoing, and a
  Ctrl-click (or a Shift-click on an entity when no prefab is being placed)
  adds to the selection instead of replacing it.
- `World::save` builds each entity's JSON in parallel.
- `Ctx::spawn_level` marks everything it spawns `NotSaved` except the
  objects it spawns as prefabs, so world saves hold a level's prefab objects
  (the game's state) but not its tiles, colliders, plain objects or root,
  which come back from the level file. Saves written before still load as
  before.
- Ember Hop's prefabs and title scene hold their animations; its
  `decorate` workaround is gone and `rehydrate` only spawns the level again.
- `ponko cook` quantizes the positions of a mesh too large for one 16-bit
  range (13.1 m within 0.1 mm) with a range per primitive, so large levels
  keep their precision and still shrink: a 48 m test level of 16 primitives
  is 627,716 bytes with float positions, 561,256 cooked this way. The
  engine folds the pieces back into one mesh in their order.
- The cooker's version is 3: a cook after updating cooks every file again.
- A failed asset group keeps the assets it was already reading when it
  failed, for a restart, and `cancel_group` now cancels a failed group too,
  dropping them. `AssetGroup::with` takes an `Fn` loader instead of an
  `FnOnce`, since a restart may run it again. Asset groups have not been in
  a release yet.
- Faster video on the desktop: rav1d's assembly is on for arm64 (built by
  the system's C compiler; x86-64 can turn it on with nasm, see the guide),
  and frames are converted to RGBA on the GPU instead of the CPU. A 1080p
  frame took 3.5 to 4.5 ms of the game's thread instead of 18 to 22 ms
  (median, on a busy machine: the guide's "What it costs"). The decoder is
  rav1d as Rerun publishes it (`re_rav1d` 0.1.3), whose package builds the
  arm64 assembly.
- `Video::update` keeps the picture with the sound: more than 0.1 s apart,
  the one behind catches up (after frames stopped while the sound played
  on, or a sound that started late). Before, a frame longer than 0.25 s
  left the picture behind until the game sought.
- `Video::frame_rgba` converts the frame on the CPU when first asked for
  after a new frame, instead of every frame.
- glTF scenes: nodes that use one glTF mesh (linked duplicates in
  Blender) share one uploaded mesh instead of uploading it per node. Eight
  nodes of one mesh hold 49,152 bytes of GPU buffers instead of 393,216
  (offscreen on Metal); the benchmark's file of twelve nodes of one rock
  mesh holds one mesh (66.5 KiB) instead of twelve (798 KiB).
- glTF scenes: the collision shapes fitted for a file's colliders are
  shared like its meshes: by its nodes of one mesh at one scale, and by
  every later spawn of the file, until the last scene that uses them goes.
  A hundred spawns of the benchmark's file hold two collision meshes
  instead of 600, keep 1.4 MiB of heap instead of 115 MiB, and spawn in
  about 9 ms instead of 0.2–0.9 s (`cargo run --release -p ponko_bench --
  gltf/`).
- The cook caches Aseprite-to-sheet conversion by source content and split mode:
  a cook that changed nothing reads the sheet back without converting.
- Embedded textures in meshopt and Draco glTF models cook to KTX2 while
  retaining their compressed geometry and buffer layout; the replaced images'
  bytes in the GLB chunk are zeroed, so a pack deflates them.
- glTF scenes: nodes of one mesh at different or mirrored scales share one
  hull or triangle mesh collider, fitted once unscaled and scaled on each
  collider (`Shape::ScaledMesh` for a node whose scale isn't one), instead
  of one fitted per scale. A hundred spawns of a file of rocks at four
  scales hold 2 collision meshes instead of 5.
- glTF scenes: spawning a file again while it and its buffers are unchanged
  and its meshes are live doesn't parse it: the nodes read by the first
  spawn are kept with the file's shared meshes (each glTF mesh's corners and
  triangles once) until its last scene goes. A re-export the game reloads is
  parsed once for all its scenes.
- A cooked meshopt or Draco model drops the bytes of the images it replaced
  with KTX2 from the buffers it holds (GLB chunk, data URIs), moving later
  views and compressed streams by multiples of 16 bytes, instead of zeroing
  them in place: the file is smaller on disk, not only once packed, and the
  geometry stays byte for byte. The meshopt test fixture went from 10,188
  to 6,128 bytes (its source is 7,116).
- The cook drops the bytes of the images it replaced with KTX2 from an
  external `.bin` buffer of a meshopt or Draco model too, when exactly one
  model reads that file and no rule keeps or skips it (before, only the
  GLB's own chunk and data URIs were shortened): it writes the shorter
  `.bin` in the model's place, with the model's offsets rewritten and the
  geometry byte for byte. A buffer two models read is left untouched. A
  `.gltf` model of 6,831 bytes with its `.bin` of 4,364 is now 6,809 and
  316 bytes. The cook cache keeps the shortened buffer with the model
  (cache format and cooker version bumped, so old entries are cooked
  again).
- A file of only tilesets (no layer) that another Aseprite file links to
  gets no sheet in the cook, without a `skip` rule; the report lists it as
  left out. `ponko_aseprite::is_tileset_only` tells such a file.
- In web builds with the `editor` feature, `Assets::save_scene` and
  `Assets::save_prefab` keep the file in the browser's storage instead of
  failing with `ErrorKind::Unsupported`; without the feature they still
  report that web builds have no files.
- Without worker threads (an ordinary web build), the streaming step now
  decodes a streamed glTF file's JSON, buffers (an inline `data:` buffer a
  bounded number of bytes a slice) and geometry in slices, not only its PNG
  images, and KTX2 textures (Zstandard and zlib levels, decoding to RGBA,
  Basis UASTC transcoding) and DDS textures decoding to RGBA in slices too,
  as glTF images or on their own. The meshes and textures are byte for byte
  what the one-piece decode makes.
- The GPU upload times of the asset group and streaming steps
  (`gpu_upload_ms`) now include their mesh buffer uploads, not only their
  textures. Meshes made in a timed step are copied into the shared mesh
  buffers before the step ends instead of at the next draw.
- The editor's web build no longer asks the browser for persistent storage
  before setup: it only looks whether its storage is persistent, and the
  first save on the page asks (once), as saving is something the person
  did. `Assets::browser_saves_persistent` is `Some(false)` until then.
  File > Saved in this browser says the next save asks, and the warning
  that the browser may clear saves goes once it grants it.
- A pack whose index declares more than 16 GiB, more than a million files or a
  file over 1 GiB is now refused (before, only the 1032:1 deflate ratio bounded
  it). Games shipping a pack that large raise the caps with `Game::pack_limits`.
- A glTF scene that fails to spawn again when the editor's Stop puts the
  world back (its file broke or went missing while playing) is listed in
  the panel of scenes that didn't load, with its reason and a **Retry** that
  works as for scenes that failed at start, and the error line names it;
  before, only the console said. A scene that spawns at Stop leaves the
  panel.
- The editor in a web build sets `<html data-ponko-retry>` to where its
  Retry buttons are (`[x, y]` fractions of the window), and the browser
  smoke runner's `--click @retry=<field>:<value>` clicks the first one it
  finds, so the `web_editor_unloaded` fixture no longer depends on the
  status bar's layout. `ci.yml`'s list of checks gains the fixture's step
  and the `web_editor_save` runs with `--persist granted` and
  `--persist refused`.
- `ponko package` cooks the files it puts in `assets.pack`, after the build:
  images become KTX2 only when the game's engine is built with
  `compressed-textures`. A game that reads its asset files' raw bytes itself
  (`Assets::load_with`, `read`) gets the cooked bytes; `--no-cook` or
  `pack = { cook = false }` in `[package.metadata.ponko]` packs the source
  files as before. Shard Grove's pack: 349.6 KiB before, 198.1 KiB cooked.
- `Ctx::finish_uploads` (`ponko_ctx_finish_uploads`, `ctx.finish_uploads()`
  in C++) also waits for every asset group still loading, reading and
  decoding the rest of it without the budgets. Games that start no groups
  see no change; a game that calls it at a checkpoint while a loading
  screen's group is under way now waits for that group too.
- glTF models load their KTX2 textures (`KHR_texture_basisu`) as GPU blocks
  with the `compressed-textures` feature when the GPU has the format,
  instead of decoding them to RGBA8; such textures no longer need
  `basis-universal` unless they are UASTC.
- A cooked `.gltf` no longer brings its `.bin` buffers along: the cooked
  GLB holds them, and the cook counts them as left out.
- `ponko package --no-pack` cooks the loose assets too (`--no-cook` ships
  the source files, as before with `--no-pack`).
- The cooker's version is 2, so earlier cook caches are cooked again.
- Every spawn of one glTF file shares its meshes and materials, freed
  with the last scene of the file (and when a world load replaces the
  scenes): a file of three meshes spawned 100 times holds 3 meshes
  (1,488 bytes by `asset_inventory`) instead of 300 (148,800 bytes).
  Hot reload makes one new set for every scene of the file.
- World saves of worlds with glTF scenes hold the reflected resource
  `GltfSceneSaves`. Saves written before still load; their prefab
  instances can't be linked, so scenes spawned after loading one leave
  their prefab nodes to the save, as `without_saved_objects` did.

**UI, text and input**
- The reference game's rebinding prompt reads `pressed_this_frame()` instead of
  polling a list of keys and buttons.
- With `text-shaping`, a retained UI paragraph that reads right to left
  starts at the right edge for `UiTextAlign::Start` (and the left for
  `End`). Text fields keep their text at the left edge.
- When AT-SPI reports that assistive technology quit and came back, the
  adapter sends the whole tree again instead of only the changes.
- The event loop's pause while a screen reader reads the window is taken
  only where the system asks through the event loop (macOS, Windows), not
  for AT-SPI, which is answered on AccessKit's own thread.
- The immediate widgets (`ctx.ui.immediate`) follow the screen UI's scale:
  `Ui::set_scale` (C: `ponko_ctx_ui_set_scale`) now scales their positions,
  sizes, text, the button outline and hit testing like the retained tree's,
  so a debug overlay is as large as the menu next to it. The widgets' units
  are the tree's layout units; at the default scale of 1 nothing moves. Only
  a game that called `set_scale` and drew immediate widgets sees them change
  (they were drawn unscaled before).
- The retained UI is laid out only when something changed since the last
  layout: the window size, the scale or the fonts, or a node's layout,
  text, font or size, or a node added, removed, shown or hidden. Before,
  every node's text was measured again two or three times a frame. When a
  label's text changes, only that label is measured again. Scrolling and
  animations of drawn properties still move nodes every frame. In the
  `frame2d` benchmark a frame with a still UI went from 1.07 ms to 0.11 ms
  of CPU, and one where a label's text changes every frame from 1.08 ms to
  0.11 ms.
- `Ui::set_layout` with the layout a node already has changes nothing.
- The editor's **Open** and **Save as** use the system's file picker on
  macOS, Windows and Linux (the `editor` feature, through the `rfd`
  crate). Save as asks for Scene or Prefab, then opens the save picker in
  the document's folder; the chosen file must be inside the project's
  `assets/` folder. Builds without a system picker keep the path field.
- `UiRole::as_str` names any role, including ones added later. From C, a role or value without an ABI-2
  constant reads as `PONKO_UI_ROLE_GROUP` or `PONKO_UI_ACCESS_VALUE_NONE`.
- Input recordings now keep gamepads connecting (with their `GamepadInfo`)
  and gamepad motion readings, and a recording started with pads connected
  begins with them, so a replay needs no gamepad backend. The MCP tool
  `run_input_scenario` describes the `info` object and motion's units.
- The retained UI keeps each node's drawing (its fill, border, image, widget
  parts and glyphs) until something it is drawn from changes: its rectangle,
  clip, paint, text, value, scroll, caret or selection, its state's fill or an
  animation of it, or the window size, scales, theme or fonts for every node.
  A still frame lays out no glyph; a changed label is drawn again alone. What
  is drawn is unchanged. In `ponko_bench`, `frame2d/ui-still` went from about
  0.106 ms to 0.033 ms and `frame2d/ui-counting` from about 0.109 ms to
  0.037 ms (medians of six interleaved runs each).
- The retained UI redraws only the part of its tree that changed. A node
  added, removed, shown or hidden is drawn alone, with the nodes its size
  moves: the nodes after it in drawing order keep their sprites and move in
  depth instead of being drawn again. What is drawn is unchanged.
- A UI node's text keeps its glyphs while its text, font, size, wrapping,
  alignment and place stay the same: a node whose opacity, scale, offset,
  background or text color animates, or whose fill fades on hover, is drawn
  without shaping or laying out its text again (rich text included; a
  focused text field still measures its caret). In `ponko_bench` (medians
  of three interleaved runs on a busy machine), a frame of the settings
  screen fading went from 0.204 ms to 0.030 ms, one with 40 labels pulsing
  in color from 0.047 ms to 0.027 ms, and one where a HUD slot is added or
  removed from 0.207 ms to 0.027 ms; a still frame stays at 0.024 ms, and
  `frame2d/ui-still` and `frame2d/ui-counting` are unchanged.
- `ponko_bench` has `ui/draw-still`, `ui/draw-fade`, `ui/draw-pulse` and
  `ui/draw-insert`: the `frame2d` HUD and settings screen with text drawn as
  with a GPU, still, fading, with 40 labels pulsing in color, and with a HUD
  slot added and removed every frame.
- A UI node that only moves (scrolled, or pushed along by a sibling that
  resized) keeps its laid-out text: its glyphs are placed again and snapped
  to the physical pixels of where it is now, without shaping, measuring or
  wrapping its text again (rich text included). A virtual list scrolled a
  row lays out only the item that came into view. The tooltip measures and
  lays out its text once while it shows, not each frame it follows the
  pointer, and an item dragged with the pointer lays out its text once.
  What is drawn is unchanged. In `ponko_bench` (medians of seven to ten
  interleaved runs on a very busy machine), a frame of the settings screen
  with its list scrolling went from 0.072–0.076 ms to 0.026–0.028 ms (one
  run 0.075 ms during a load spike); the other `ui/draw-...` frames, the
  tooltip one included, are unchanged within the noise (about 1 µs).
- `ponko_bench` has `ui/draw-scroll` (the settings screen's list of 40
  actions scrolling a fraction of a pixel a frame) and `ui/draw-tooltip`
  (its tooltip following the pointer).
- On-screen touch controls are laid out in the window's safe area, a
  `TouchPlace` counts from its corners, a control the player drags stops
  at its edge, and the ready-made arrange screen's panel stays in it.
  Without insets (every desktop, and pages without `viewport-fit=cover`)
  nothing moves.
- Crowds are culled by the box of each clip's frame range, not of the
  whole clip: the bake keeps a box for every 4 frames of a clip (and the
  frame after, which skinning blends with them), and each character is
  tested with the box of the range its clip shows at the frame's time
  (and of the one its blend clip shows), on both paths. In the tests'
  scenes: 13 figures held across a jump below the camera view's bottom
  edge, 10 drawn (13 with a box per clip), the picture unchanged; 26
  jumping behind a low wall, 17 kept by occlusion culling on the GPU path
  (26); global illumination's probe views over 8 frames, 918 kept (1,047).
- A rich text label that doesn't wrap keeps its laid-out glyphs when it
  moves sideways (a sibling in its row resized) or its box's width changes:
  its lines are aligned where they are placed, not when its text is laid
  out, like plain text's. What is drawn is unchanged.
- The UI layout keeps each label's measured size: unwrapped, narrowest, and
  wrapped at the last four widths it was offered (plain and rich text). A
  wrapped label in a row whose sibling resizes back and forth is measured
  once per width, not each time. Rich text that doesn't wrap is measured
  once, as plain text already was.
- `ponko_bench` has `ui/draw-resize`: a panel in a row of the settings
  screen switches between two widths each frame, moving a rich label
  sideways and rewrapping a paragraph. Its frame went from 0.103–0.106 ms
  to 0.067–0.068 ms (medians of three interleaved runs each, on a very busy
  machine); the other `ui/draw-...` frames are unchanged within the noise
  (about 2 µs).
- Wrapped text, plain or rich, keeps its laid-out glyphs while its width
  breaks its lines the same way: laying it out also finds the range of
  widths that would end each line in the same place. A wrapped label that
  moves sideways (its width changing in its last bits) or whose box grows
  or shrinks a little isn't laid out again. A label whose width goes back
  and forth keeps the layouts of its last three widths. What is drawn is
  unchanged.
- The UI layout keeps a wrapped label's height for each of the last four
  ranges of widths that wrap it the same way, not for four exact widths.
- `ponko_bench`'s `ui/draw-resize` frame went from 0.064–0.065 ms to
  0.027–0.029 ms (medians of five interleaved runs each, on a busy
  machine); the other `ui/draw-...` frames are unchanged within the noise
  (about 2 µs).
- On macOS and Windows the system's layout read when the window regains
  focus now reaches `ctx.input.key_labels()` at the start of the next
  frame rather than in the focus event (the change edge is unchanged).
- Controls draw their text in the middle of their height, at least three
  quarters of the theme's `spacing` in from their left and right edges, and buttons,
  number fields and key-binding fields centre it across too
  (`UiTheme::paint` gives them `UiTextAlign::Center`). A button's text used
  to sit at its top left: apps that relied on that set
  `paint.align = UiTextAlign::Start` on it; the default theme draws buttons
  otherwise as before. The reference images with buttons and checkboxes
  were redrawn.
- C++: `ui::tree::next_event` and `events` read `ponko_ctx_ui_next_event_ex`,
  so they now also return the `chosen`, `dropped` and `rows_shown` events.
  C's `ponko_ctx_ui_next_event` is unchanged and still skips those kinds.
- C: `PonkoUiThemeEx` is version 2, with `transition` and `press_scale` at
  its end. Callers that pass version 1 keep working and get no transition.

**Audio**
- In a browser, the audio output opens at the player's first click, tap or key
  press. Looping sounds (music, ambience) played before it wait and start then,
  with their volume, bus, effects and 3D placement, instead of being lost;
  one-shots played before it are dropped. Desktop behaviour is unchanged.
- In a browser, `load_stream` keeps the bytes encoded and decodes them a packet
  at a time as they play, instead of decoding the whole file when loaded: a
  three-minute stereo track holds its encoded size plus at most 1.6 MB instead
  of about 64 MB.
- Sound files are checked before they are decoded, in every loader (whole
  sounds, asset groups, streams, a video's soundtrack): a sample rate over
  768 kHz, zero or more than two channels, or, for a sound decoded whole, a
  length over 64 Mi frames (512 MiB of samples) is an error that names the
  limit, instead of an allocation of that size or an error after decoding
  all of it. A sound decoded whole also stops at the length limit however
  short its header said it was (a few kilobytes of compressed silence can
  unpack to gigabytes); a longer sound is streamed. Files with more than two
  channels were already unplayable; they now fail earlier.
- At most 64 sounds are heard at once unless `Audio::set_voice_limit`
  raises it; past that the quietest, lowest-priority sounds go virtual
  instead of each bus failing to start sounds past 128.

**Physics, navigation and AI**
- The `Game::run_headless` docs and the guide say plainly that headless
  runs without a control endpoint ignore `ctx.time.pause()`, and point to
  `ctx.physics.set_paused` for holding the world in a test.
- Ember Hop holds the built-in physics step with `ctx.physics.set_paused`
  instead of replacing it with its own system.
- Shard Grove holds the 3D physics step on its menus with
  `ctx.physics3d.set_paused` instead of zeroing the warden's velocity, so a
  jump goes on where the pause stopped it.
- `Brain`'s JSON has a `"memory"` field once it has thought; saves without
  it still load (the brain starts its tree over, as before). A brain is
  128 bytes instead of 72.
- A `parallel` node with `do` nodes in more than one child, refused
  before, now compiles and runs the actions together.
- A brain is 152 bytes instead of 128 (its running actions, two inline).
  `ai/10k` measured again with the additions of the last two changes: the
  tree costs ten times the hand-written `match`, as before (guide, "Game
  AI").
- C++ `ai::brain::doing` is true for any of the brain's actions, not only
  the first.
- The water kit's waves from a spectrum work out their CPU field (the
  WebGL2 upload, `Water::height_at`, `Water::sample` and buoyancy) with
  rustfft and each wave's speed worked out once, instead of a plain
  radix-2 FFT and a square root and sine per texel each moment. On an Apple M4 Max, busy with other work, one new moment at 256
  cells takes 0.58 ms instead of 2.9 ms (0.9 to 1.2 ms instead of 4.3 to
  9 ms with the sway's speed), and 0.13 instead of 0.63 ms at 128 cells.
  The heights stay within a micrometre of the old field's. The water
  guide's costs and its advice on cells are updated.
- Crowds are culled by each clip's own bounds: the bake keeps a box per
  clip (every level, every frame of the clip), and each character is
  tested with the box of the clip it plays (and of the one it blends to)
  under its transform, as an oriented box against the views, a world box
  against occlusion culling's depth grid and its half diagonal for global
  illumination's probe size cut, on both paths. Before, every character
  used one sphere around every clip. In the tests' scenes occlusion
  culling now keeps 4 of 26 characters behind a low wall (26 before) and
  the probes' face views 1,047 characters over 8 frames (3,128 before).
- World saves keep each `TerrainCollider`: its friction, restitution and
  groups, with the heightfield written as `null` (a mesh made in code has no
  file to name) and made again by the terrain rebuild. Saves written before
  load as they did; without a rebuild a loaded tile has no ground until the
  game gives it a new `TerrainCollider`, as before.
- The water's CPU wave field works out a moment once when the WebGL2 upload
  and buoyancy ask for the same time: after a moment worked out without the
  sway's speed is asked again with it, the next ones are worked out with it
  from the start, until such a guess goes unused. At 128 cells a frame of a
  floating body on a WebGL2 sea costs 0.21 ms of CPU field instead of
  0.35 ms (0.93 instead of 1.55 ms at 256 cells; release, Apple M4 Max, busy
  machine, medians of seven alternating runs).
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
- Waves from a spectrum are drawn on the simulation's clock (see Breaking
  changes). The WebGL2 upload now shares the fixed steps' moments with
  floating bodies after a pause or speed change too (at most one moment a
  step), and a paused game works out and uploads nothing.
- The WebGL2 upload's f32 to f16 conversion uses the CPU's own (F16C, ARM
  FP16, through `half`) where there is one, a chunk at a time, and is
  branch-free elsewhere, with the same bits for every float: one moment's
  upload 0.083 → 0.007 ms at 128 cells and 0.36 → 0.029 ms at 256, a
  blended one 0.11 → 0.013 ms and 0.49 → 0.054 ms (release, one thread's
  CPU time, medians of five on a busy Apple M4 Max; a moment of the field
  costs 0.13 and 0.6 ms).

**Networking**
- `ponko-lobby` logs `trusted_proxies` in its `lobby_listening` line.
- The `webrtc` feature brings `sha1` (already in the tree) and `md-5` for
  TURN's message integrity.
- Replication's server reads and compares only the entities that changed.
  It tracks the replicated components' changes (once a client has joined)
  and each tick reads only entities whose replicated components changed or
  were removed, new or handed-over ones, and a rolling 1/64 of the rest (so
  a write the world can't see still goes out within about a second); per
  client it compares only those with their baselines. With 1,000 entities
  of which 5% change each tick, 16 clients: 0.58 ms per server tick
  instead of 0.75 ms (medians, alternating builds on a busy machine), with
  identical traffic; with every entity changing, no difference beyond the
  noise. Tracking costs 8 bytes per replicated component of each entity, on
  the server only.
- Replicated events travel in binary: MessagePack with fields by position,
  after a varint event type and tick (they were JSON after a fixed 10-byte
  header). Bytes per event: a hit (`NetId`, `f32`, `Vec2`, `bool`) 26
  instead of 76, a two-letter chat line 9 instead of 24, an enum variant 12
  instead of 22 to 29. Maps with keys that aren't strings (a
  `HashMap<(u8, u8), u8>`), which JSON refused, now travel. Servers and
  clients of different builds refuse each other on join, as before.
- Lockstep's desync check hashes the world with the public `world_hash`
  (through `WorldHashParts`), reading components in place, instead of
  making the world's JSON save every check; a `Desync` still names the
  components that differ. The lockstep protocol version is 2: a peer of an
  older build is ignored (and times out) instead of seeing a desync at
  every check.
- Replication sends each change, spawn and despawn once, and again only
  when the client hasn't acknowledged it within a resend timeout (its
  measured round trip plus a margin, between two ticks and a second) or
  when it changed again meanwhile, instead of in every packet until
  acknowledged. In the replication benchmark, with 1 in 20 of 1,000
  entities moving each tick, a client's traffic fell from the 64 KB/s
  budget (63,893 B/s) to 31,266 B/s.
- Replication resends a record at once when the client acknowledges
  packets sent after the one that carried it but not that one, instead of
  waiting for the resend timeout. On a link that reorders packets, the
  server learns how late packets arrive and waits that long before calling
  one lost. With five changes in a lost packet on a link of 8.6 ticks round
  trip, they went again after 9 ticks instead of 12; on a link losing 10%
  each way, the slowest 5% of changes reached the client in 18.2 ticks on
  average instead of 20.0, for the same bytes.
- While a client acknowledges nothing, each replication resend timeout
  doubles the next one, up to 4 s; the first acknowledgement brings it
  back and everything overdue goes at once. Over a link cut for 9 s, the server
  sent 35 records instead of 300 for five changes.
- `World::restore_snapshot` puts back the change stamps and removal logs
  saved with the snapshot, each as many ticks old as it was at the save,
  instead of counting every restored value as added and changed now. In a
  `GameRollback`, the engine also moves what each simulation system has
  seen by the same amount, so `Added` and `Changed` filters and `removed`
  show a tick that runs again exactly what they showed it the first time.

**Web, mobile and platforms**
- `scripts/package-coin-dash.py package` and `notices` run `ponko package`
  and `ponko notices`; Coin Dash's package is described in the root
  `Cargo.toml`. Its `smoke` walk-through and command line are unchanged.
- The web smoke runner can no longer hang. Once the page is open every browser
  call has a 30 s deadline, a crashed page fails at once (`browser-crash`), and
  the whole run has `--run-timeout` (default 300 s). Both failures name the step
  the run was on and print the end of the browser's own output, which each run
  now saves as `browser.log`.
- On Android, storage keeps a game's files in the app's files folder and
  replaces a file's backup by copying (the system denies hard links there).
- Crash reports list the loaded modules on Android, as on Linux.
- On iOS, storage keeps a game's files in `Library/Application Support/<app id>`
  in the app's container; the clipboard is the general pasteboard.
- The GPU device asks for no more than the adapter allows of the native
  limits it raises, so an adapter below the defaults (the iOS simulator's
  15 inter-stage shader variables) still opens.
- The `arboard` clipboard crate is a desktop-only dependency.
- The web gallery check fails a page whose browser logged a WebGL error (a
  refused call), not only a blank canvas.
- The web smoke runner (`web/smoke`) uses playwright-core 1.65.1 (Chromium 159,
  Firefox 159, WebKit 27.2). The Firefox of 1.64.1 exits at once on macOS 27,
  so Firefox now runs on a Mac too.
- Windows game packages bundle Microsoft's DirectX Shader Compiler
  (`dxcompiler.dll`) beside the executable and include its licence in
  `THIRD-PARTY-NOTICES.txt`. The CLI caches the pinned release ZIP and
  verifies its SHA-256. Set `windows-dxc = false` under
  `[package.metadata.ponko]` to leave the DLL out and use FXC when DXC
  is unavailable. Development builds still use wgpu's default compiler
  selection.
- In a web build with threads, light baking (`bake_lighting`) runs on the
  frame's workers instead of the page's thread alone: 18 ms instead of
  52 ms for the fixture's scene in headless Chromium.
- In a web build with threads, a world save (`World::save`, `save_world`,
  `save_world_in_background`) builds its JSON on the page's thread again:
  on the workers it took 223 ms instead of 22 ms for the fixture's world,
  because the browser's threads share one allocator lock. The text is the
  same; the desktop keeps building it in parallel.
- The ordinary web build reads gamepads with the engine's own backend, the
  one builds with threads used, instead of gilrs (now a desktop-only
  dependency): an 85 KB smaller module (16 KB compressed), the same
  latency (the next frame), and rumble played on the pad's browser index
  instead of finding the pad again by its name. The web chapter of the
  guide has the measurements.
- In a web build with threads, the small parallel passes are shared out
  to the workers only from sizes where that pays in a browser: a hierarchy
  level from 0.4 ms projected on the page's thread (about 14,000 attached
  children; 50 µs natively), 2D skeletons from 512 (64 natively) and 3D
  characters from 512 (16 natively). Below those sizes they were slower on
  the workers than on the page's thread. Native thresholds are unchanged.
- Web builds open WebGPU more carefully before they commit to it: an
  adapter, a device and the device's first round trip to the GPU come
  before the canvas gets its WebGPU context. A browser with no
  `navigator.gpu`, no adapter, a device that won't open or a device lost
  as it opens now draws with WebGL2 instead of failing to start (before, the
  last two stopped the game with a startup error), and the console says why
  in one line (`ponko: falling back to WebGL2: ...`).

**Tools: editor, inspector and agents**
- A code mod's game is checked once per `ModCode::call`, not once per host
  function: code swapped out of its `with_code` call, or into one it wasn't
  handed, still can't reach the game.
- `update_hierarchy` places a flat update (no link changed, every parent a
  root, a few children per moved root) in passes over the attached entities
  on all cores, without building, indexing and gathering a level of roots on
  the calling thread.
- On macOS, `KeyLabels::system()` reads the layout only with the `window`
  feature (`None` without it): linking Carbon mapped 4 MiB at start in every
  headless process.
- Only a hosting network endpoint holds the budget for answering unknown
  addresses (only a host answers them): a client endpoint is 392 bytes
  instead of 2,976. The
  lobby load test peaks at 108–111 MiB instead of 122–124 MiB.
- MCP `entity_stats` (control method `entities.stats`): `fields` is
  optional; without it the answer counts the entities that match `with` and
  `where`.
- MCP `run_input_scenario`: its schema lists the key names (`Right`, not
  `ArrowRight`) and shows an `expect_ui` step.
- `ponko notices` refuses the options only `ponko package` uses, and `ponko package` refuses
  `--title`: both share one parser, so `ponko notices --web` or `ponko package --title Game` were
  accepted and did nothing. Drop such an option from a script; `ponko notices` takes `--out`, `-p`,
  `--title`, `--manifest-path`, `--target`, `--features`, `--no-default-features` and `--locked`.
- MCP `subscribe_events` (control method `events.subscribe`): the reply
  lists the event types collected (all of the game's when none were named)
  and whether that is all, and names a type the game doesn't have under
  `unknown`, with a hint listing those it has. Such names are still
  accepted.
- A tool call to a running game takes a fraction of a millisecond instead
  of about 25 ms. The control endpoint accepts connections the moment they
  arrive instead of looking for them every 20 ms (closing it still stops at
  once), and writes each reply in one piece. `ponko-mcp` keeps one
  connection per game and reuses it across calls; it opens a new one,
  without an error, when the game closed the connection or restarted.
  Replies are matched to requests by their `id`.
- See-through debug lines (alpha below 1) are drawn with alpha to coverage
  instead of blending when the 3D frame has several samples a pixel (the
  default 4x MSAA): their alpha is the share of a pixel's samples they
  cover, in steps of a quarter, and overlapping see-through lines no longer
  add up. 100,000 see-through lines at 1280x720 with 4 samples (Apple M4
  Max, busy machine, four runs): 3.0 to 3.06 ms opaque pass instead of 3.7
  to 4.8 ms. With one sample a pixel they are still blended (unchanged,
  1.67 ms).
- Editor: the 2D and 3D cameras draw into the free view between the panels
  (their `rect`), so what they look at is in its middle; the view follows
  resized panels, but not during a drag.
- Release builds of a game with a window now write crash reports by default
  (development builds and headless runs don't), and while they do, standard
  output and error pass through the engine's log capture (the terminal still
  shows everything). `Game::crash_reports(false)` turns this off.
- `DrawParams::with_screen(bool)` is deprecated in favor of `with_space(DrawSpace)`; existing calls keep working. The drawing chapter explains world units and window pixels in one place.
- The test whose crashing child really dies of the signal stays Linux-only
  by default, now with the reason written down (the guide's "How it is
  tested"); with `PONKO_CRASH_TESTS_DIE=1` on Windows its child turns off the
  error dialog.
- In a web build with threads, a hierarchy update (attachments) pass worth
  sharing out runs on the worker threads, through the same runner as
  parallel queries, instead of calling the thread pool from the page's
  thread (where it ran alone).
- `for_each` and the parallel loops with two to four `Option` fetches
  vectorize for every combination of components, not only when an
  archetype has none of them: two `Option`s whose values the closure
  combines went from 14.0 to 2.8 instructions per entity (none: 2.0), three
  from 17.1 to 3.1 and four from 19.7 to 3.3, about half the cycles.
- `ModSet::mount` now refuses a mod whose prefabs or scenes name an asset
  that neither a mod of the set nor the game has; such a mod used to mount
  and fail when the asset was first loaded. Ship the asset in the mod, or
  fix the name.
- `cast_ray` in code mods reads the game's physics directly during
  `with_code` instead of swapping the physics world in and out; results
  are the same.
- `editor.transform` and `editor.drag` work on 2D games (they needed the
  `3d` feature before).
- `capabilities` lists the `editor` feature.
- The colliders view draws 3D collision meshes (`Collider3D::mesh` triangle meshes and heightfields) and `TerrainCollider` terrain as their triangles within `collider_distance` of the 3D camera, found through the mesh's own tree or grid, instead of a bounding box (terrain wasn't drawn before). Convex hulls show all their edges.

**C and C++**
- Copying a C++ `ponko::access` copies the declaration exactly, including the
  engine types a built-in's declaration names, instead of replaying the names
  it was built from.
- The handles chapter and STABILITY.md record the decision for the next C
  ABI: 64-bit ids with the handle kind in the top 8 bits (0 for untagged),
  so a handle of the wrong kind is caught where it is passed.
- C++: `PONKO_COUNT_CPP_ALLOCATIONS` counts `delete` too (each block keeps
  its size in a header of `alignof(std::max_align_t)` bytes), so
  `net_alloc_bytes` sees what a C++ program gives back.
- C++: `ai::each` forwards to the typed `world().each`, which the guide names as the one way to loop over brains. Existing callers compile as before.
- The "Still Rust-only" list is shorter: split toning, depth of field,
  `Post::lerp`, `PostVolume`, `ParticleSettings::softness` and
  `Camera::particles` are reachable from C and C++. `ctx::set_particle_emitter3d`
  in C++ calls the versioned entry point.
- C asset-handle enumeration fills the caller's array in oldest-first order without allocating internally.

**Guide and examples**
- The API reference chapter links the newest docs.rs release (`latest`)
  instead of a version written into each link, and says how to get the guide
  that matches a release (build it from the tag).
- The roadmap, the releases section of `STABILITY.md` and the guide's text now
  describe 0.2.0 as one release that carries the whole plan after 0.1.0 (2D,
  3D production and text, multiplayer and platforms, and the research that was
  waiting), with only the 1.0 freeze work outside it.
- The comparison chapter no longer says systems only run in registration
  order, describes shipped work (the parallel scheduler, declared access,
  custom render passes, GPU tiers, diagnostics) as shipped, links the pages
  other engines now publish, and drops statements about other engines it
  could not cite.
- The "Still Rust-only" table in the C and C++ chapter lists every Rust API
  the guide names as missing from C and C++ (action read-back, built-in step
  declarations, `SystemView`, LOD hysteresis, scene compute passes and target
  effects, keyed audio curves, storage helpers and others).
  `scripts/check-ffi-coverage.py` now fails when the table and
  `scripts/rust-only.txt` disagree, when a listed item disappears from the
  Rust source, or when a C function appears for it.
- The tutorials' games return a `ConfiguredGame`, as the templates do, so
  `src/main.rs` stays as `ponko new` wrote it. The 2D title screen opens on
  Play, the 3D counter is a HUD, and the 3D yard is made solid with
  `load_collision_mesh`.
- A scene's mesh collider that writes `scale` and `convex` out as their
  defaults now shares its mesh with one that leaves them out.
- `ponko_ctx_ui_push_screen` fails with `PONKO_INVALID_ARGUMENT` for a root
  shown as a HUD.
- The list of checks packages the 3D reference game (`ponko package --example
  shard_grove`) in `package-smoke` and plays the package headless from an
  unrelated folder, as it does for the other two reference games. A sheet in
  `scripts/checklists/` covers what needs a person: the frame-time baseline
  per GPU tier, and opening the game in a WebGPU browser and in a window on
  each OS.
- The web gallery is published on GitHub Pages, by pushing the `gh-pages`
  branch by hand (the decision and the commands are in `web/gallery/README.md`
  and CONTRIBUTING.md, "Releasing"). The gallery's tooling and
  `scripts/release-check.py` read the project's URL from `PROJECT_URL` and
  derive the Pages address from it, and `scripts/project-url.py check` covers
  them.
- The README's capability list and comparison table give each feature's
  maturity level instead of listing what is implemented.
- The reference games use art, sound and music made by people, all CC0:
  Coin Dash and Ember Hop have Kenney's pixel-art tiles, sprites,
  particles and sound effects; Shard Grove has Quaternius' animated
  characters ("Hooded Adventurer", "Crab Enemy") and Poly Haven PBR
  textures with normal and ambient occlusion / roughness / metalness maps;
  the music is by Wolfgang_, Juhani Junkala and Cleyton Kauffman, and the
  grove's wind by IgnasD (OpenGameArt). Each game's `CONTENT-NOTICES.txt`
  and `assets/README.md` credit every file, and the packages carry the CC0
  text as `CC0-1.0.txt`. Coin Dash and Shard Grove now have music.
- Screenshots of the three games in the README and the guide, and short
  clips in `book/src/images`.
- The API docs link again to items that exist only with a feature (`3d`, `physics`, `physics3d`): the links resolve in the docs.rs build, and a build without the feature shows the same names as plain text. A broken link to `Game::web_threads` in the web module docs is fixed.
- Missing documentation is now a compile error (`#![deny(missing_docs)]`)
  in `ponko_ecs`, `ponko_macros` and the engine's core modules (game, ctx,
  schedule, time, events, input, actions, assets, storage, scene, reflect,
  error, handle). A new public item there needs a doc comment.
- Panics of the ECS and startup APIs are documented under `# Panics`
  (spawning a bundle with a component twice, undeclared view access,
  failing setup in `run_headless`), and returned errors under `# Errors`.
- The most used ECS and engine items (`World` entity and resource calls,
  `Entity`, `Bundle`, input state, `Time`, `Storage`, `Events`, `Error`,
  `MemorySource` and the `Game` builder) have doc examples that run
  headless as doctests.
- Every public item of the rendering, 3D, 2D drawing, particle, lighting,
  post-processing, terrain and streaming modules must now be documented:
  `#![deny(missing_docs)]` keeps it that way. The settings structs of the
  renderer (`Lighting`, `Post`, `Sky`, `MaterialDesc`, `ParticleSettings`,
  `InstanceBatchOptions`, `LodSettings`, `DrawParams` and the others) say
  their defaults and what they cost, the functions that return an error or
  panic have `# Errors` and `# Panics` sections, the features a WebGL2 device
  lacks say what happens there, and the GPU capability report, 2D occluders,
  streaming controls, instance batches, GPU buffers, draw parameters and
  particle settings gained runnable examples.
- Every public item of the UI, text, locale, audio, 2D and 3D physics,
  navigation, networking and server modules, and of the `ponko-mcp`,
  `ponko-unity` and `ponko` command-line crates, must now be documented:
  `#![deny(missing_docs)]` keeps it that way. The collision shapes, bodies,
  physics services, string tables, lockstep input calls and server controls
  gained runnable examples, and the functions that return an error or
  panic have `# Errors` and `# Panics` sections.
- **Ponko vs Bevy vs Unity** (`book/src/comparison.md`) names the versions
  it compares (Bevy 0.20.1, Unity 6.3 LTS as the newest) and carries both
  scoreboard tables against Bevy, as a project starts and tuned. Its note
  on web builds no longer mentions a merge queue, which the project does not
  have.
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

**Development and releases**
- Benchmark baselines and the numbers quoted in the docs were measured again
  on an idle machine: `crates/ponko_bench/baselines/` (the ECS
  matrix, the render scenes, and the matched Ponko and Bevy runs), the ECS and
  render-scene write-ups, `README.md`, the performance chapter and the
  chapters on crowds, replication, rollback costs, saves and 3D audio. The
  render baseline now has eight scenes (`single-material` is new). Two scenes
  are slower than the figures the docs quoted before: `alpha-heavy` (9.5 to
  17.5 ms a frame: see-through models are now drawn one by one in sorted order)
  and `animated` (4.2 to 5.8 ms: skinning keeps last frame's joints for motion
  vectors); both also draw more shadow casters than before.
- The published `ponko` crate no longer carries the three reference games'
  art, sound and music (`assets/coin_dash`, `assets/ember_hop`,
  `assets/shard_grove`): nothing builds from them. It packs 800 files, 4.3 MiB
  compressed, instead of 850 files, 7.5 MiB, well under crates.io's 10 MB
  limit. The games' sources stay in the package so it compiles, but running
  one of them needs the repository's `assets/`.
- A GitHub release whose CHANGELOG section is too long for its notes keeps the
  story and the breaking changes and links to the rest of the section.
- `release-check.py check` also checks the version every guide chapter tells
  people to depend on, not only getting-started and the tutorials.
- A release candidate's templates and guide name the candidate
  (`ponko = "0.2.0-rc.1"`): Cargo never picks a candidate for `"0.2"`.
- Crowds on the CPU path (WebGL2's limits) group their characters' draws by
  counting them per view, level and posed frame instead of sorting: for
  10,000 characters the frame's prepare and record went from 0.55 to 0.47 ms
  on an Apple M4 Max, drawing exactly as before.
- The attachments benchmark has a `+100 ticks` row, so the steady tick can
  be read apart from the first update, which links every new attachment to
  its parent.
- `bench/compare/scoreboard.py --only` keeps the date and revision of the
  rows it doesn't measure again, and the table names them.
- The Ponko vs Bevy scoreboard (`bench/compare/scoreboard.py`) runs on
  Windows: `.exe` names, UTF-8 file reads, machine and driver detection on
  Windows and Linux, and a load figure without `uptime`
  (`scripts/cpu_load.py`, also used by `scripts/build-times.sh`).
  `--machine NAME` writes results to `bench/compare/results/NAME/`, and
  `--backend` picks the GPU backend for both engines. A run that fails (a
  crash, the GPU out of memory) is recorded as a failed cell with its error,
  rather than stopping the scoreboard.
- The performance chapter and the README show the scoreboard on a second
  machine, a Windows desktop (Ryzen 5 3600, Radeon RX Vega), on DirectX 12
  and Vulkan.
- GPU tests: at most `PONKO_GPU_TESTS_AT_ONCE` tests (default one) hold a
  device at once in a test process, so the suites survive the default
  thread count on drivers that lose the device with several at once. Tests
  without a GPU still run in parallel.
- GPU tests: a test that finds no adapter or device fails once another test
  of the same run has opened one (the device was lost), instead of skipping
  and passing. Every skip is also appended to
  `target/render-test-output/skips.log`, so a run can count them.
- Numbers taken on a busy machine were measured again on an idle one
  (Apple M4 Max, 2026-11-22, revision `9235ea05`, three runs
  each, load average recorded): the ECS baseline
  (`crates/ponko_bench/baselines/ecs.json`) and the render baseline
  (`baselines/render.json`, `render.csv`), both re-recorded with their
  documented commands, and `ECS.md`, `UPSCALING.md` and `INSTANCE_CELLS.md`; in the
  guide, the chapters on skeletons, debug drawing, behavior trees, spatial
  search, determinism, hierarchy, attachments, replication, the lobby,
  rollback, code mods, video, streaming and loading, UI frames and group
  fades, the sky kit and water, global illumination's culling,
  reflections and thin objects, upscaling, transparency, baked lighting,
  area lights, stereo views, crowds, viewports, the web's time to the
  first frame, ECS iteration and the battle showcase (frames and
  simulation). Most numbers
  dropped to a third or less of what the busy machine gave.
- Numbers that moved the wrong way are written as such: a code mod's call
  costs 58 ns against 46 ns measured before (it came with tying a mod's
  game pointer to its `with_code` call), the lobby load test peaks at
  122 MiB against 104 MiB, and the thin-objects trace in `High` at 1440p
  takes 0.21 to 0.24 ms against its 0.2 ms target, and a crowd's CPU path
  (WebGL2's limits) prepares and records in 0.55 ms against 0.44 ms. Memory per entity in
  the headless example (96 bytes) is explained: `Sprite` gained a 20-byte
  `region` after the 76-byte figure was taken.
- The render baseline shows the GPU slower in all eight scenes than on
  2026-11-20 (frames up to 27% slower, `shadow-heavy` 5.02 to 6.35 ms):
  the `high` preset now turns on screen reflections' thin objects, whose
  pass costs 0.34 to 2.31 ms in these scenes against about 0.2 ms over the
  floor and rails it was measured on. Written up in `RENDER_SCENES.md` and
  the guide; not changed here.
- The web threads benchmark came out slower on an idle machine than on a
  busy one, one-thread runs included (likely the headless browser on
  efficiency cores; not confirmed); `web-threads.md` says so and keeps
  the earlier table.
- The guide's contributing chapter has a section on writing tests that hold on
  a busy machine.
- `scripts/check-unsafe.py` covers the benchmark binary `crates/ponko_bench`
  (every unsafe item there has a test, most under Miri), names checks of
  test programs without the harness, and no longer accepts an unsafe item
  without a test.
- The 3D crowd test asserts counted steps and work per agent instead of a
  30 s wall-clock guard, the 2D skeleton allocation test counts per thread
  and no longer allows for rayon's workers, and the ECS test that a slow
  query reaches several threads counts the rows run on the calling thread
  instead of relying on idle workers waking in time.
- `ponko_bench` no longer depends on `bevy_ecs`: Bevy is out of the engine
  workspace's `Cargo.lock`. Its ECS rows run in `bench/compare`, which
  compiles the same suite.
- The templates' debug profile builds dependencies with line tables only
  (`debug = "line-tables-only"`) and `syn` unoptimized: a clean debug build
  of the 2D template takes 45.8 s instead of 60.0 s on an M4 Max (3D
  tutorial: 63 s instead of 84 s), its target folder is 2.4 GB instead of
  3.9 GB, and rebuilds after an edit stay at 0.46 s. Backtraces keep file
  and line in the engine; a debugger no longer shows the engine's local
  variables. Projects made earlier can copy the lines from the guide's
  "The fast debug profile".
- Textures (render targets included), sounds and fonts reuse a freed one's
  table slot under a new handle, as meshes and GPU buffers already did: a
  game that makes and frees them all session keeps its tables (the engine's,
  the live-handle tables, the window renderer's and the audio backend's) the
  size of the most it held at once. Handles are still never given out twice
  (a slot is retired after 4095 uses), a freed handle stays dead, and a
  sprite or material still showing a freed texture draws white even after a
  new texture takes its slot. Handle ids no longer count up one by one.
- `Ui::remove` gives the removed nodes' slots back: the next nodes take
  them under new ids (a generation in `UiId`'s upper bits), so a UI that
  builds and removes menus all session no longer grows. Ids are still
  never given out twice, and a removed node's id stays stale.
- A looping sprite-sheet `Animation`'s `time` stays within one loop
  (seconds into the current loop) instead of counting up forever.
- `STABILITY.md` has a **Releases** section: when patch and minor releases
  happen, release candidates, which releases get fixes, deprecation and
  yanking, and what 1.0 requires. `ROADMAP.md` is rewritten around the
  releases after 0.1.0 (0.2.0 to 1.0) and their milestones.
- The project's home is GitHub again: `https://github.com/ponkoengine/ponko`
  (issues, pull requests, releases). The crates' `repository` fields, the
  READMEs, the guide, the templates and the web gallery link there, and
  `scripts/project-url.py check` now refuses links to the old GitLab home.
- There is no CI any more: nothing runs on a pull request or on `main` by
  itself. Contributors and maintainers run the checks locally. The list of
  checks moved from `.github/workflows/ci.yml` to `scripts/checks.yml`, a
  document no service runs, which `scripts/check-feature-coverage.py` still
  reads. The leak check runs as the local commit hook
  (`scripts/leak-check.sh install`), and a release is built and published by a
  maintainer from the tag (CONTRIBUTING.md, "Releasing", lists the steps).
- `scripts/semver-checks.py run` works on a local checkout: `--base BRANCH`
  names the branch the change is for, `--breaking` says the pull request has
  the `breaking` label.

### Deprecated

**API and crate layout**
- `ponko::stream` merged into `ponko::streaming`: `stream::Job` and
  `stream::CellStreamer` are `streaming::Job` and `streaming::CellStreamer`.
  The old names stay as deprecated aliases until 0.3.0.
- The prelude's renamed spellings `Transform2D`, `Camera2D`, `Transform3D`
  and `Camera3D` are deprecated aliases until 0.3.0: write `d2::Transform`,
  `d2::Camera`, `d3::Transform` and `d3::Camera` (the prelude brings the `d2`
  and `d3` modules), the names rustdoc shows. Saved and reflected component
  names (`"Transform2D"`, ...) don't change.
- The `Ctx` shortcuts that only forwarded to one service, each with a
  warning naming its one spelling: `ctx.dt()` (`ctx.time.dt()`),
  `ctx.key_down/key_pressed/key_released` and `ctx.mouse_pos()`
  (`ctx.input.*`), `ctx.draw` and `ctx.draw_text` (`ctx.gfx.*`),
  `ctx.load_texture` (`ctx.gfx.load_texture`), `ctx.load_sound` and
  `ctx.play_sound` (`ctx.audio.load` and `ctx.audio.play`). They go in 0.3.
  `Ctx` keeps only the calls that span services (`free`, `is_live`,
  `live_counts`, `mouse_world`, `window_size`, `quit`, the three above and
  `immediate_ui`). The C and C++ API already has one flat spelling per call
  and is unchanged.
- `UiImmediate::panel/label/button(gfx, input, ...)`: use
  `ctx.immediate_ui().panel/label/button(...)`.
- `Input::start_scenario(world, time, ..)` and
  `Input::cancel_scenario(world, time)`: use `ctx.start_scenario(..)` and
  `ctx.cancel_scenario()`.
- `SceneSettings::quality_autotune_status(gfx)`: use
  `ctx.quality_autotune_status()`.
- Paths that 0.2.0 moved keep building, with a warning that names the new
  path, until 0.3.0 removes them. An alias can't build or match a tuple
  struct (`ShadowMesh(None)`, `LayerId(0)`): write the new path there.
  Everything else (types, associated functions, struct literals, enum
  variants) goes through. The `ponko::physics3d` module is back for this:
  a game with a crate of that name and `use ponko::*;` writes
  `::physics3d::...` for the crate.
  - `ponko::physics3d`: `Character3D`, `Collider3D`, `ContactModifier3D`,
    `ContactPair3D`, `Hit3D`, `Joint3D`, `JointAxis3D`, `JointDrive3D`,
    `JointMotion3D`, `Physics3D`, `QueryFilter3D`, `Ragdoll3D`,
    `RigidBody3D`, `Shape3D`, `Vehicle3D`, `Velocity3D` and `Wheel3D` are
    `ponko::d3`'s types without the `3D` (`d3::Collider`, ...);
    `ColliderFocus`, `CollisionGroups`, `CollisionMesh`, `CollisionMeshData`,
    `FrictionCurve`, `RagdollBone` and `TerrainCollider` keep their names in
    `d3`; `PHYSICS3D_HZ` is `d3::Physics::HZ`; `step_physics3d`,
    `interpolate_physics3d` and their `_access` functions are in
    `ponko::systems`. Added by hand under the old path, the step systems
    are still the built-in ones (`Systems::PHYSICS3D`), listed under the
    same names as before.
  - `ponko::terrain::CellStreamer` is `ponko::streaming::CellStreamer`.
  - `ponko::d3`'s copies of render, animation, particle and terrain types:
    `AmbientOcclusion`, `AntiAliasing`, `BatchCulling`, `Blend`, `Compute`,
    `ComputePass`, `Effect`, `GlobalIllumination`, `GpuBuffer`,
    `GraphicsQuality`, `InstanceBatch`, `InstanceBatchOptions`,
    `InstanceLevel`, `Lighting`, `LodGroup`, `LodHysteresis`, `LodLevel`,
    `LodPick`, `LodRenderer`, `LodSettings`, `LodShadow`, `Material`,
    `MaterialDesc`, `Mesh`, `MeshCost`, `MeshData`, `ModelMaterial`,
    `ModelMaterials`, `Post`, `ReceivesDecals`, `RenderPasses`,
    `SceneSettings`, `ScreenEffect`, `Shader`, `ShaderDiagnostic`,
    `ShaderKind`, `ShaderOutcome`, `ShaderStatus`, `ShadowFilter`,
    `ShadowMesh`, `Sky`, `StreamedLod`, `TargetCamera`, `TargetEffect` and
    `Tonemap` are in `ponko::render`; `AnimationClip`, `AnimationCondition`,
    `AnimationEvent`, `AnimationGraph`, `AnimationGraphError`,
    `AnimationLayer`, `AnimationParameter`, `AnimationState`,
    `AnimationTrack`, `AnimationTransition`, `Animator`, `AttachToJoint`,
    `Interpolation`, `JointAnimation`, `JointMask`, `LayerBlend`, `LayerId`,
    `MorphAnimation`, `MorphFrame`, `MorphTarget`, `MorphWeights`, `Motion`,
    `MotionChild`, `ParamId`, `RootMotion`, `Skeleton`, `SkinOf` and
    `StateId` in `ponko::anim`; `ParticleCurve`, `ParticleGradient` and
    `ParticleSpace` in `ponko::particles`; `FarTerrain`, `Heightfield`,
    `TerrainMaterialDesc`, `TerrainTile`, `TerrainTileData`, `TextureArray`
    and `TextureKind` in `ponko::terrain`. `d3::Model` and `d3`'s collision
    types have no alias: the prelude brings them, and an alias would make
    `use ponko::prelude::*; use ponko::d3::*;` ambiguous. For the same
    reason, a module that glob-imports `d3` together with one of these
    topic modules imports these names by name until 0.3.0.
- To be removed in 0.3.0 (with a breaking entry each): the `ponko::physics3d`
  module, `ponko::terrain::CellStreamer`, and the 83 type aliases in
  `ponko::d3` listed above, together with 0.2.0's other deprecated items
  (`ponko::stream`, the prelude's `Transform2D`, `Camera2D`, `Transform3D`
  and `Camera3D`, ...).

**UI, text and input**
- `Ui::push_screen(root, modal: bool)`: use `push_modeless(root)`,
  `push_modal(root)` or `push_screen_with_mode(root, mode)`. C's
  `ponko_ctx_ui_push_screen` stays in ABI 2 (use
  `ponko_ctx_ui_push_screen_with_mode`); C++'s `push_screen(root, bool)`
  overload is `[[deprecated]]`.
- `UiLayout::custom`, `style`, `style_mut` and `into_style`: use
  `unstable_custom`, `unstable_style`, `unstable_style_mut` and
  `into_unstable_style`. Same behaviour.

### Removed

**Development and releases**
- The GitHub Actions workflows (checks, benchmarks and their comments, the
  dependency audit, fuzzing, the release, the release audit, pull request
  events), the shared actions and the Dependabot configuration.
- Scripts that only worked inside Actions: `scripts/bench-history-push.sh`,
  `scripts/audit-issue.py`, `scripts/ci-apt-install.sh`,
  `scripts/ci-retry.sh`, the `ci` command of `scripts/release-check.py` and
  the `--review` label gate of `scripts/check-reference-images.py`.

### Fixed

**API and crate layout**
- The offscreen timing helper of the GPU tests times each frame once: it
  begins the timer's frame itself and submits the timestamps with the frame
  in one submission, as a game does. The debug-line timing test no longer
  begins every frame twice.
- The GPU tests that were seen failing on a heavily loaded machine measure
  what they claim without a timing assumption: the GPU memory total is
  compared with the adapter's figure once the driver has given back what
  was dropped, and automatic quality's fresh sample every frame allows a
  lone frame whose timestamps the GPU left unusable. The C++ test of
  allocation-free query filters asks for one clean profile run of three
  (a system's count is the whole process's, so another thread's
  allocation could land in it), and its net-bytes check likewise retries.

**ECS and systems**
- `World::despawn` no longer runs extra instructions on every call since
  empty archetypes are reclaimed: whether the archetype emptied is decided
  from the removed row, off the common path. Under cachegrind on x86-64 the
  despawn benchmark is back within 1 % of its count before reclaiming.
- Editor: a click on a hierarchy row selects its entity. The row's drag
  area lay over it and egui gave the click to nothing, so only clicks in
  the view selected.
- A 3D sphere, box or capsule cast that starts overlapping a collider hits
  it at distance 0 with the normal that pushes it out, whichever way it
  moves; moving deeper in, it could miss the collider or report a wrong
  normal.
- 3D queries given a non-finite rotation find nothing, as the rules say;
  the rotation was replaced with no rotation.
- 3D `overlap_box` and `overlap_capsule` see heightfield terrain (only
  `overlap_sphere` did).
- 2D circle, box and capsule casts starting inside a collider report a hit
  at distance zero with a push-out normal in either direction, including
  when the maximum distance is zero.
- 2D penetration queries reuse per-thread solver scratch memory after
  warm-up, including against polygon colliders. Reused overlap and hit
  buffers remain allocation-free; polygon query shapes still build their
  convex hull on each call.
- Regression coverage checks non-finite directions and rotations in 2D
  and the shared 2D/3D query rules; 2D already rejected these inputs.

**Rendering**
- Materials that read the scene behind them (`scene_position`, `scene_distance`),
  screen effects and target effects run on WebGL2 instead of stopping the game
  with a validation error. On OpenGL the scene depth is bound as a float
  texture read through a non-filtering sampler (WebGL2's shading language
  can't load a texel of a depth texture), and a game's own
  `textureLoad(scene_depth_texture, ..)` is rewritten to match. Transparent
  surfaces there read a copy of the depth, since OpenGL has no read-only depth
  attachments. Other backends draw as before.
- Texture arrays on WebGL2: one-layer arrays (such as a terrain's weight map)
  read as arrays, and every array's mipmaps are made right, so terrain
  materials show their layers instead of white.
- The `terrain`, `shaders` and `target_effect` examples run on WebGL2. The web
  gallery no longer marks them as needing WebGPU, and the gallery check runs them on WebGL2.
- `ShadowOnly` is kept by world saves, scene files and prefabs, and shows in
  the inspector: it is reflected by default and written as
  `"ShadowOnly": null`.
- Plain shadow-only models hidden by occlusion culling are no longer counted
  in the frame's occluded models, as skinned ones already weren't: no camera
  draws them.
- Temporal ambient occlusion no longer pulses on distant ground seen at a grazing angle with
  temporal antialiasing on: its history test allows for the sub-pixel jitter there instead of
  rejecting the history every frame.
- A target camera's screen effects keep the alpha of its picture, so a
  transparent background stays transparent after them.
- Crowds draw on Direct3D 12: the crowd vertex shader no longer writes to a
  local array at an index known only at run time, which FXC (Direct3D 12's
  default shader compiler) rejected when it built the crowd motion-vector
  pipeline.
- On Direct3D 12, instance cells drawn together match each cell drawn
  alone: the mesh vertex entry points read `instance_index` themselves, so
  naga adds the draw's first instance to it (Direct3D 12's instance ID
  starts at 0 whatever the first instance is).
- A render test that panics on Direct3D 12 no longer leaves the test binary
  waiting forever for its stuck GPU work: the device of a panicked test is
  kept alive instead of dropped.
- Render tests honour `WGPU_BACKEND` (`dx12`, `vulkan`, `metal`, `gl`), so
  either backend can be tested on Windows; the offscreen benchmarks too.
- The Python check scripts read and write their files as UTF-8, so they run
  on Windows, where the default encoding is not.
- `python3 scripts/fmt-workspace.py --check` checks the workspace format in
  batches; the format check uses it, because `cargo fmt --all` passes
  Windows' command-line limit in a checkout with a long path.
- Render tests hold on more adapters: blend rounding a level apart, an
  exact colour compare on a half level, the GPU memory figure counting
  textures as the allocator places them, and the GI negative control.
- `ponko cook`'s copy of the material shader parts matches the engine's
  mesh entry points again.
- Temporal ambient occlusion (`Post::ao_temporal`) no longer pulses every
  eight frames on ground far away. All pixels used to turn their slices in
  lockstep, and the estimate was divided by the slice count. Ground seen at a
  grazing angle then read darker on some turns than on others. Now each
  pixel's occlusion is measured against what its slices would see
  unoccluded, and each pixel's turn is offset by a 4x4 pattern so
  neighbouring pixels sample different turns. The bilateral filter weights a
  neighbour by its distance off the pixel's surface and, along the surface,
  by how far apart the pixels lie, so it averages those turns on distant
  ground too. With only the AO history running, a still field 100 to 400 m
  away changed by a mean of up to 0.77 levels from frame to frame. It now
  changes by under 0.05 (under 0.15 at half resolution).
- Material normal maps lean surfaces the way glTF says: the map's red (+X)
  toward where the texture coordinate u grows, its green (+Y) up the image
  (toward smaller v). They leaned the opposite way on both axes, because the
  tangent frame built from screen derivatives took screen y down; bumps
  looked like dents. This applies to every material path that reads
  `MaterialDesc::normal_map` (models, crowds, skinned meshes and the
  screen-space reflections' normals); decal and terrain normal maps were
  already right. A lit offscreen test pins the direction with 1x1 maps on
  squares with plain, turned and mirrored texture coordinates, and the
  Khronos `NormalTangentTest` and `NormalTangentMirrorTest` sample models
  (fetched at a pinned revision into the git-ignored `.cache/known-content`)
  now show every normal-mapped hemisphere lit as the real one beside it.
- Transmissive glTF materials no longer vanish: the glass, liquid and volume parts of
  the Khronos samples (stained-glass and lamp shades, lenses, jugs, vases, windows,
  attenuation and dispersion tests) are drawn.
- The first frame of an offscreen run (benchmarks, tests, the editor's
  thumbnails) draws its 3D picture, in the window's stand-in and in every
  target camera. Before, it held no 3D picture and not even the
  background.
- A frame drawn while the window's surface changes (shown, hidden,
  recreated, resumed) keeps its 3D picture. The new scene targets and the
  reset history now start with the next frame, instead of being dropped
  from the frame that was already prepared.
- glTF materials draw each map with its own texture coordinates and support independent texture transforms animated through `KHR_animation_pointer`. Materials support up to four distinct transforms, with per-map overrides in Rust and version 5 of the C and C++ material descriptor.
- Direct3D 12: a 3D frame drawn at less than the whole window (a split
  screen, or the main camera beside window views) could show blocks of old
  GPU memory, often spread over the whole picture by bloom. Images that the
  frame only writes by resolving multisampled passes into (the scene image,
  motion vectors, reflection normals, target cameras, global illumination's
  probes) are now cleared once when they are made, as Direct3D 12 requires
  for render targets placed in shared memory.
- `ponko_bench` no longer keeps 128 MiB resident in every run: the bloom
  filter membership benchmark filled its cache-flush buffer before
  checking whether it would run. The render benchmark's peak memory is
  back to 236 MB (it was 363).
- Compute passes now run in a 3D frame that has nothing else to draw (no
  models, particles or target cameras). Such a frame used to skip the
  whole 3D frame, compute passes included, so a simulation alone on the
  GPU (drawn only in 2D, or only read back) never ran.
- Direct3D 12 with FXC (its default shader compiler) no longer spends 18 to
  30 s compiling the global illumination probe update in a debug build, and
  over 8 minutes with a release build's optimizations, each time a renderer
  starts global illumination. wgpu zeroed the shader's workgroup memory from
  one thread, which FXC expanded store by store; the shader writes all of it
  before reading, so the pipeline is now built without that zeroing (1 and
  4 s). A global illumination render test went from 184 to 38 s.
- Global illumination: the probes' views are lit with point and spot light
  shadows (the camera's shadow atlas, nothing drawn twice), so a shadowed
  light behind a wall no longer bounces light into the room past it.
- Global illumination: instance batches are in the probes' views and their
  sun shadow map, so they bounce and block light like models. They are
  culled for a bounded number of views however big the grid is: one per
  probe face on the CPU path (instances under a probe pixel left out), and
  seven on the GPU path, whose views each keep a list as long as the batch.
  Instance cells are in them as their batch is; a freed cell leaves every
  probe view from the next update.
- Global illumination: batches of distance levels on the CPU path leave
  instances under a probe pixel out of the probes' views, as single-mesh
  batches do, instead of drawing whole cells into every probe face. They
  keep 4 more bytes per instance for it.
- Batches of distance levels are bounded by a tighter sphere (one holding
  every level's bounds, instead of one around their bounding box), so
  every view culls them a little more tightly.
- Realtime global illumination no longer fills closed and nearly closed
  rooms with the sky's light. The probes' own views took the sky's ambient
  light wherever their probes hadn't looked around yet or the grid faded
  out, and a room kept that outdoor light for hundreds of bounces (white
  walls pass almost all of it on). Probe views now count such probes as
  dark and use the outermost probes' light past the grid's edges, so light
  gets into the probes only from what they see. A closed white box under
  the sky now stays dark inside (its floor 15 instead of 237, out of 765);
  in the red-walled room lit through a roof opening, the settled error to
  a fully converged path-traced reference fell from 0.33 to 0.08 (flat
  ambient light: 0.69). The GPU cost is unchanged.
- Thin things far away (rails, cables, poles) are no longer reflected as
  wide blobs without TAA: a crossing allows at most two pixels of the ray's
  travel instead of two of its steps, which far along the ray were tens of
  pixels. Their reflections are now 1.6 to 2 times as tall as they are seen,
  down from 2.2 to 4.2 times.
- Realtime global illumination settles much sooner in bright closed rooms.
  Each bounce of the probes' light takes a round of probe updates, and
  white walls keep most of the light, so it took over a thousand frames to
  settle. Surfaces now take the light each probe heads for (the bounces
  still to come, estimated from how much its light still grows), while
  the probes keep bouncing the light as it is. In a closed white room lit
  by a lamp the roof is within 6% of its settled light from 400 frames on,
  where it was 17% short.
- Bounced light no longer leaks through a thin wall that runs through a
  plane of probes. A probe buried in such a wall saw the light on both
  sides of it; its neighbours' distances now give it away and surfaces
  leave it out. The floor beside a 10 cm wall on a plane of probes 2 units
  apart dropped from 192 to 24 (out of 765; the lit side's floor: 423).
  The GPU cost is the same within the measurement noise.
- A target camera clears its render target in a frame with nothing at all
  to draw, instead of leaving the last picture in it, and draws what it
  sees while the main camera sees nothing (that frame used to skip the 3D
  passes, the target cameras' too).
- First-person held items are drawn in a frame that has nothing else to
  draw.
- 3D debug labels that aren't depth-tested show in the render target of
  every target camera that shows debug drawing, on top there as on screen:
  drawn last, over the scene, particles and debug lines (they were left
  out of render targets).
- Held items whose material has a stencil state (`MaterialDesc::stencil`)
  test and write the stencil as in the world, in the stencil's `order`;
  cut-out ones mark it only where they are solid (the view model drew them
  without it).
- Temporal antialiasing with soft particles no longer fails the frame: the
  transparent pass, split to read the scene behind the particles, copies
  the lit frame, which now allows it.
- See-through first-person held items whose material has a stencil state
  draw in their stencil's `order` and write the stencil also while the
  transparent pass is split off (a see-through surface in the world reads
  the scene behind it, or soft particles are on): there they used to test it
  without writing it, so one that marks it hid nothing from the next.
- `ponko textures`, `ponko cook` and `ponko package` no longer risk hanging
  forever after encoding a texture on a busy machine. Each image used to get
  its own Basis Universal encoder, with a thread per core, dropped right
  after; that encoder's job pool can miss waking a worker as it shuts down,
  and then waits for it forever. Encoders are now kept and reused until the
  command exits, and never dropped. Cooks take the same time and peak memory
  as before.
- Compute passes (culling, skinning, global illumination, reflections) and
  passes run alongside them read 0 ms in the GPU pass timer on Metal.
  Metal writes a compute pass's timestamps, and sometimes the
  last pass's end, after a copy in the same command buffer has read them,
  so the timer read the previous frame's values and dropped them. A
  frame's timestamps are now copied out by a later frame, once the GPU has
  finished it; each frame in flight has its own queries. GPU times arrive
  about two frames later than before.
- A pass the GPU runs entirely alongside others (a compute pass next to a
  render pass it doesn't depend on) shares that time with them evenly
  instead of being charged 0 ms. The passes still add up to no more than
  the frame's GPU time.
- Safe area insets fed by a scenario step hold for as long as the scenario
  runs: a windowed web run that re-reads the page's insets on a resize no
  longer overwrites them. The page's insets apply once the scenario ends.
- Realtime global illumination leaks less light through thin walls near
  their top. Probes inside a wall just below its top weren't found to be in
  the wall: part of what their neighbours saw that way passed over the top
  into the next room. They now are, so they no longer light a dark room
  with the light from the room next door. With a 10 cm wall between a lit
  and a dark room on a plane of probes 2 units apart, the dark room's floor
  went from 5% to 1.4% of the lit floor's brightness (sums of the image's
  8-bit channels), and the lit room got no darker.
- A game that makes many devices in one process (tools, previews, tests) no
  longer loses one to the GPU timers on Metal. Metal lets a process hold 32
  timestamp query sets, of any size, across all its devices, and the 33rd
  loses the device. A renderer now holds 2 sets instead of 7 (the upload
  timer keeps its six frames in one set), and the engine takes at most 28,
  so 14 devices are timed at once; a device past that keeps drawing without
  GPU times, with one warning. The guide's "How many devices can be timed"
  section has the measurement.
- Global illumination no longer takes probes in open air just under a
  wall's bottom edge for buried in the wall: the floor beside a gap under
  a raised partition got about half the bounced light it should. Each
  neighbour now reads the four distance texels around the way to the
  probe, a cone centred on it. Every probe inside a raised partition is
  still found, down to 0.1 units above its bottom edge.
- The guide quoted realtime global illumination's error in the red-walled
  room as 0.08; it is 0.05. The tests now check the renderer still gets
  the errors the guide quotes, and that the guide quotes them.
- On Metal, a GPU timer's timestamp query set is counted against the
  process's 32 until its device has really freed it, not only until the
  timer is dropped: dropping and making timers again while their frames
  were still on the GPU could hold more sets than counted, and a set past
  32 loses the device.
- With HDR output on, the inspector keeps its SDR colours and brightness:
  it is drawn as into an SDR window and laid over the frame at paper white
  (it used to look lighter). UI and debug drawing stay within 8-bit
  rounding of the SDR frame, checked by an offscreen test.
- A camera with an infinite far plane (`Camera3D::far = f32::INFINITY`)
  no longer counts every frame as a camera cut: temporal antialiasing,
  temporal ambient occlusion and screen-space reflections kept starting
  over, and the reflections never showed.
- The 3D guide said see-through models aren't sorted; they are drawn far
  to near by each model's distance, with the alpha particles after them.
- Far sun cascades kept between frames (`Lighting::shadow_refresh` above 1)
  no longer lag behind their casters: a model moving, spawned or despawned,
  a level-of-detail switch, an instance batch or cell made, changed or freed,
  an instance level crossing its shadow distance band, or a mesh's data made
  again under a held cascade redraws it on that frame, so kept and redrawn
  shadows match pixel for pixel. Still scenes keep every shadow map and draw
  no shadow pass, as before.
- Screen-space reflections far from the surface no longer vanish at
  render heights of about 1000 pixels and more: the normal rebuilt from
  depth fell below a precision floor and turned toward the camera.
- The GPU memory report counts the 1x1 colour stand-in that 3D passes without
  a scene bind, and the SMAA colour target has a label, so allocator reports
  name every engine texture.
- The texture mip limit no longer shrinks texture arrays made from pixels in
  code (`create_texture_array`), only arrays loaded from image files
  (`load_texture_array`) and model images. Code-made arrays are often data a
  shader reads at the size it was made (texel coordinates worked out from
  that size, packed tables): halving them moved what those shaders read.
  The engine's own terrain-file and water-map arrays are made this way too.
- A target camera drawing for the first time starts from a fresh frame
  instead of taking another target's kept frame (which may have collected an
  icon's separate world or another camera), and kept frames are dropped
  when their render target is freed rather than by count.
- `Camera3D::screen_ray` works its direction out from the projection's slopes
  and the camera's rotation instead of unprojecting a depth. With reverse-Z,
  depth 0.5 is about two near planes from the camera, so a centimetre near
  plane far from the origin put the direction tenths of a degree off and
  made it wobble as the camera turned (0.055° off in the new test).
- The water kit's under-water view builds its view ray through the far plane
  instead of depth 0.5 (about two near planes from the camera under
  reverse-Z), so it holds steady with a small near plane; it and the sky's
  far direction stay homogeneous, so an infinite far plane works too.
- 3D scenes draw again in Safari's WebGPU. Since glTF material layers
  (#644), every material and compute pass failed to compile there ("call
  to 'textureSampleBias' requires uniform control flow"), so the scene
  drew black. The layer code no longer takes screen derivatives itself:
  the fragment shader passes them in, and the layer maps are sampled with
  explicit gradients (#701).
- `ShadowMesh(None)` turns a skinned model's shadow off, as it already did
  for plain models and terrain: skinned models cast with their posed mesh
  regardless. First-person arms (skinned, on the view-model layer) cast a
  shadow into the world that the body already casts.
- Screen-space reflections showed nothing on Direct3D 12 on an AMD Radeon RX
  Vega once the frame was larger than 128 pixels on a side, when the opaque
  pass reads the reflections' spread image directly (a game's texture limits)
  and ambient occlusion is off. The first level of the reflections' image,
  which only says that reflections are there, was set once with a render-pass
  clear; the opaque pass read it back as zero. It is drawn once instead
  (#588).
- A target camera that draws only some frames (a reflection refreshed every
  other frame) no longer collects its whole view from nothing each time it
  draws: its frame is kept between draws. In a scene of tens of thousands of
  models, its collect went from about 15 ms to about 1.4 ms a frame.
- Terrain tiles respect `RenderLayers`: a camera draws a tile only when they
  share a layer (a tile on none of its layers still casts its shadow), as
  models, level-of-detail groups and skinned models already did. The
  view-model camera, which draws only its own layer, drew the ground within
  its far plane over the world: a band of terrain across the bottom of a
  first-person view while an item was held.
- 3D draws in browsers on WebGPU: the built-in shader's layered material
  branched on transmission through a value that also held screen
  derivatives. A browser's uniformity analysis rejects that
  (`textureSampleBias` outside uniform control flow), so every 3D pipeline
  was invalid and the canvas stayed blank. Native backends accepted it.
- Web games no longer stop at random while starting in Safari on iOS
  ("mapped GPU buffer is 0 bytes"). Safari there sometimes gives an empty
  range for a buffer made mapped, so on WebGPU new buffers are now made
  empty and filled through the queue. Native builds still map them.

**Animation**
- A wall nearer than a hundredth of the near plane's distance beyond it no
  longer cuts into held items: the view model pushes it behind them first.
- `ponko_unity`: when a prefab is saved in a posed state, the clips of bones it
  keeps under their original parent no longer get that pose added on top, so a
  rig saved at frame 0 of a clip no longer plays every bone rotated twice.

**Assets, scenes and saves**
- A 2D game could not load a JPEG unless it also enabled the `3d` feature.
- A glTF material whose textures use the second texture coordinate set
  (`texCoord: 1`) is drawn with that set instead of the first.
- Match Unity FBX submesh ordering by first material usage, so renderer material slots stay attached to the correct geometry.
- A video's sound, and every sound streamed a packet at a time (on the
  web), now lands where a seek or a loop asks: it landed up to 33 ms off,
  and up to 180 ms off in places, because the stream's timestamps were
  read as samples (a WebM file's are milliseconds) and a reset Vorbis
  decoder gives nothing for its first packet.
- Loading a world now frees the collision shapes of the glTF scenes it
  replaced, as it did their meshes.
- A glTF file exported again with a property mistake keeps its scene
  drawing: the meshes of the scene it couldn't update are no longer freed.
- glTF scenes: a mirrored node (a negative scale) collides where its mesh is
  drawn: its hull or triangle mesh is mirrored, and its box, sphere or
  capsule sits on the mirrored side, instead of using the unmirrored shape.
- The test of sliced PNG decodes counts the slices each step runs against
  the frame budget instead of timing them, so it no longer fails on a busy
  machine.
- Editor: undoing the delete of a glTF scene's node brings it back as a
  node of its scene, with its mesh, colliders and light from the file (it
  came back as a bare entity that scene saves kept as well).
- Editor: the hierarchy names a glTF scene's nodes by their names in the
  file.
- A failed asset group goes on taking the assets it had started before it
  failed, as they finish, and counts them in its progress; `finish_uploads`
  waits for them. Before, a failed group's `loaded` stopped where the failure
  came back, so it could be short of what had in fact finished, and differed
  from run to run under load. The C API test of a restarted group checks the
  exact count again.
- The `web_editor_save` example run on the desktop no longer writes over
  the shipped `assets/levels/beach.json`.
- `ponko pack --list` no longer inflates a file past the size its index declares.

**UI, text and input**
- Static text (`UiRole::Label`) is given to AccessKit as the node's value,
  where UI Automation and AT-SPI read a label's name; with only a label, it
  would have been read without its text there.
- Screen-space text lands on whole physical pixels at any position and window
  scale factor: `Graphics::draw_text_ex` (and so `draw_text`, the immediate UI,
  retained UI labels and shaped text) rounds each run's origin to the physical
  pixel grid before placing its glyphs. Text placed at fractional positions
  (centred labels, flexbox layouts, a scaled UI) no longer looks smeared or
  unevenly spaced, and a centred label looks the same wherever it sits.
- Rich text runs in different fonts and sizes keep one baseline per line after
  snapping, and inline images get whole-pixel edges.
- The retained UI measures text for layout, wrapping and caret placement at the
  size it is drawn, in physical pixels, so layout and drawing agree.
- Anchor absolute UI children to the box of their preset parent, including
  percentage sizes and offsets inside flex items.
- Input a system feeds (a replay, a test, a bot) reaches the retained UI
  in the frame it was fed: the UI takes what is new before the next
  system, so a tap or click fed by a system clicks, focuses and closes in
  that frame instead of being lost.
- A desktop's mouse events made up for a finger no longer click a UI
  button a second time or take the UI's pointer from the finger (Windows'
  made-up presses, X11's cursor following the first finger).
- Input recordings keep touches (and trackpad gestures), so a recorded
  touch session replays.

**Audio**
- Browser audio tries stereo when WebKit reports zero output channels.
  `Audio::unlocked()` stays false when the browser cannot start the output,
  and the engine logs the reason once.
- A WAV file whose channel count makes the format reader's block size
  overflow (60 000 channels of 16 bits) panicked it in builds with overflow
  checks (debug builds, tests, the fuzzer). A panic inside any sound decoder
  is now an error naming the file as damaged, at every place the engine
  decodes sound.
- A sound decoder that panicked part-way through a streamed sound on the
  desktop ended Kira's decoder thread and left the sound silent, still
  "playing", for good. Desktop streams (from a file or from memory) now
  play through the engine's own decoder, with the same readers, codecs and
  header limits, in which every call is guarded: a panic ends that sound
  with an error in the log (`ponko: a streamed sound stopped: ...`), and
  the other sounds and the game go on. Seeking, looping and stream lengths
  behave as before.
- A looping stream decoded a packet at a time (every stream on the web,
  a video's soundtrack everywhere) whose packets decoded to no frames, or
  whose loop start was past its end, went back to its start forever,
  rebuilding the decoder each time and freezing the thread that mixes:
  found by the new `webm_audio` fuzz target in its first run. It now ends
  like a sound that has nothing left to play.
- A WebM file with a block group whose size overflows the Matroska
  reader's arithmetic panicked it in builds with overflow checks, when a
  video loaded or played (`Video::from_bytes`, `advance`, `seek`): found by
  the new `webm_audio` fuzz target. A panic inside the reader is now a
  `Decode` error ("its data is damaged").

**Physics, navigation and AI**
- Tiles of a carving bake, and tiles rebuilt by carving, keep detail heights:
  agents next to a carved hole on uneven ground follow the ground as closely
  as on an ordinary bake instead of floating or sinking between polygon
  corners.
- Carving no longer stops at 256 obstacles or 8 layers per obstacle, and an
  edit that touches no tile no longer uses a step of the `update_carving`
  budget.
- The 2D and 3D physics worlds no longer keep a record for every entity
  index ever used. Their per-entity links are packed and give memory back
  when bodies leave, so a streaming world that spawns and despawns many
  entities holds memory for its live bodies only. Simulation results are
  unchanged.
- Far wave crests no longer show dark underside streaks when viewed from above.
- A crowd's animation clock stays precise in long sessions: it keeps its
  epoch in 64-bit seconds and moves the characters' clip times on every
  16 seconds of game time, so a day into the game the clips step and write
  motion vectors as finely as at its start (they were 32-bit seconds since
  the game began, about 1/128 s steps after 24 hours).
- `tests/step_inputs.rs` builds with `--features 3d,physics3d` alone: its
  2D physics tests are gated on `physics`.
- Terrain: a `TerrainCollider` of a triangle mesh on a stretched or mirrored
  tile is the scaled copy `Collider::scaled_mesh` uses, built again from the
  scaled corners, instead of rapier's own scaling, which ignored the mirror
  (the piece stood on the unmirrored side) and bent the surface normals of a
  stretch. A convex hull on a scaled tile now takes the scale too. Tiles at
  one scale share one copy.
- Terrain: a heightfield `TerrainCollider` (or `Collider::scaled_mesh` of a
  heightfield) on a tile with a negative X or Z scale is solid where the
  tile is drawn. It used to take the scale's size only and stood where the
  unmirrored tile would be. The mirrored copy is built again with its
  heights, holes and cell diagonals flipped, centered by the signed scale,
  so rays, surface normals, resting bodies and collider streaming follow
  the drawn ground. Unmirrored tiles are built as before; a mirrored one
  is built once per scale, about 0.1 ms for 257 × 257 heights. A negative Y
  scale still takes the size only (the ground faces up).
- The water's CPU wave field kept its last moments by the time folded into
  the 200 s period but looked them up by the time asked, so after the first
  200 s of a game every query worked out its moment again.

**Networking**
- The networking guide said `libsteam_api` was in `target/<profile>/` to
  copy by hand; it is in the build's output folder, and `ponko package` now
  ships it.
- Update the online guide's local coturn command and document the verified TURN
  success and credential-rejection results.
- Message endpoints attempt the first hello on their first poll, even after a
  long startup. Polling caps the time charged toward peer silence so one
  stalled frame cannot consume the whole timeout.

**Web, mobile and platforms**
- `examples/cpp` links the threads library, which programs on glibc before
  2.34 need for `std::thread`, and its asset-copy step no longer crashes CMake
  on arm64 Linux with glibc 2.28.
- The macOS C library in the release archives has the install name
  `@rpath/libponko_ffi.dylib`, so a program linked against it finds it
  through its rpath instead of looking for the path it was built at.
- The web smoke test's WebKit runs no longer hang for an hour. Headless
  WebKit on Linux crashed its content process when a game opened its audio at
  the first click, because the machine had no sound server, and the runner
  waited for the crashed page forever. The web smoke job now starts a PulseAudio
  server with a null sink before the WebKit runs and tells WebKit the output is stereo (its
  device scan reported 0 output channels there, so the engine's audio could
  not open), so the fixtures' music plays there too. `web/smoke/README.md`
  says how to do the same on a headless Linux machine.
- A window surface without sRGB view formats (OpenGL ES on some Android
  devices and emulators) no longer panics; it draws without the sRGB view
  and warns that colors come out darker.
- The C API tests build where `c_char` is unsigned.
- WebGL2: a 3D scene without shadows (`lighting.shadows = false`) drew
  nothing. wgpu's GL backend makes a one-layer texture an ordinary 2D
  texture, which the shaders' shadow-map array sampler can't read, so the
  browser refused every draw ("Mismatch between texture format and sampler
  type"). The sun's shadow map and its 1x1 stand-ins now get a second,
  unused layer on GL, as texture arrays already did. The `lights` example
  draws on WebGL2 again.
- Web: a browser whose WebGPU adapter opens devices but can't show a frame
  on a canvas (such as the software fallback adapter of Chromium's headless
  shell on macOS) no longer stops the game on its first frame. Before giving
  the game's canvas a WebGPU context, the engine clears and presents one test
  frame on a small `OffscreenCanvas` of its own inside error scopes; if it
  fails, the console says `ponko: falling back to WebGL2: a canvas can't show
  WebGPU frames: ...` and the game draws with WebGL2. The test frame adds
  about 1 ms to a WebGPU start on a GPU and about 5 ms on a software adapter.
- `runtime-failures.mjs --case device-lost` reports "unsupported" where the
  browser can't present a WebGPU canvas (Chromium's headless shell on a Mac)
  and the game runs on WebGL2, instead of failing after destroying an unused
  device.
- The runner's zero-channel audio check for WebKit sets the channel count on
  the audio destination's prototype, so it no longer fails now and then when
  WebKit returns a fresh `destination` object.
- A window with no sRGB format and no sRGB view (OpenGL ES whose EGL has
  no sRGB color space, such as Android's emulator with `WGPU_BACKEND=gl`)
  showed colors too dark: the frame is now drawn into an sRGB image and
  encoded into the window by a last pass, so it matches Vulkan. Windows
  that offer sRGB, desktop OpenGL with it and WebGL2 draw as before.
- iOS: `ctx.window.safe_insets()` follows the phone as it turns (it kept
  its upright values, so the touch controls sat under the Dynamic Island
  and the rounded corners in landscape). (#641)
- iOS: the audio output closes while the app is suspended and opens again
  on resume, as on Android; the game's sound no longer plays over the
  home screen or another app. (#642)
- The web page and the gallery's demo page show a panic or other fatal
  failure while the game runs, with its message, instead of leaving a
  frozen canvas (the template had already removed its status line) or
  showing only the `RuntimeError: unreachable` the abort throws.
- `bake_lighting` no longer panics in a browser: it timed itself with
  `std::time::Instant`, which isn't available there.
- `web/build.sh --threads` dropped the caller's flags, or its own: it put
  its flags in `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS`, which cargo
  ignores once `RUSTFLAGS` is set, so with the check list's
  `RUSTFLAGS="-D warnings"` the build lost its atomics and shared memory.
  It now adds its flags to the variable cargo reads and keeps what is
  there, so a warning fails a threaded build. `threads.mjs --build` adds
  `-D warnings` itself.
- The four lints the nightly clippy reports in the threaded web
  configuration (an overindented doc list, two `then` closures that needed
  no closure, and a `Box::pin` that `pin!` does): the check list now lints
  that configuration.
- Web builds with threads apply gilrs's dead zone (a tenth of the travel,
  rescaled) to sticks and triggers, as the ordinary web build and the
  desktop do; small stick movements no longer read as input there.
- 3D scenes drew black on the browser's WebGPU (Chrome): the cut-out
  crossfade fragment shader sampled a texture behind `||`, non-uniform
  control flow that browsers' WGSL compilers reject, which made every 3D
  pipeline of that module invalid. A test now finds any shader that samples
  after `||` or `&&`, which naga accepts and so no native test saw.
- 3D models drew nothing on WebGL2: small mesh index writes joined the
  frame's batched copies, and WebGL2 can't copy into an index buffer from
  another buffer. In a browser, index buffers are written on their own.
- The web page displays a readable error when graphics initialization fails,
  the WebGL2 context is lost during startup, or fallible setup returns an error.
- `c-library.py selftest` passes on Windows: its read-only mount check compares the archive folder as the platform prints it.
- On Windows, a Tiled or LDtk level loaded by an absolute path (`C:\...\level.tmx`)
  finds the tilesets and images it names next to it, instead of looking for them
  in `assets/`; `..` in such references no longer drops the drive. glTF files
  loaded by such a path find their buffers and images next to them too.
- On Windows, `load_skeleton2d` with a file path that uses backslashes
  (`C:\game\hero.json`) finds the atlas pages and images next to the file
  instead of looking for them in `assets/`.
- Windows: a game's windowed startup no longer overflows the main thread's
  1 MB stack in a debug build ("thread 'main' has overflowed its stack" before
  setup ran). The renderer's 3D pass and the `Ctx` are kept on the heap
  instead of being moved through the startup's stack frames, and the windowed
  loop takes the setup closure boxed, so it is compiled with the engine rather
  than in the game's crate. A debug build's windowed startup with a 3D mesh
  now runs in under 272 KB of stack (it needed more than 1 MB), on Direct3D 12
  and Vulkan; a release build too. The examples are no longer linked with an 8 MB stack on Windows:
  they don't need it (#591).
- Tests run startup on a thread with 512 KB of stack: the headless startup
  always, the whole windowed one on Windows as an opt-in check (it opens a
  window).

**Tools: editor, inspector and agents**
- The editor's **Window → Assets panel** hid and showed nothing: the frame wrote back the layout it
  had read before the menu ran.
- A `**/` pattern matches files at the top of the folder too: `--exclude '**/*.psd'`,
  `pack = { exclude = ["**/*.psd"] }`, `cook.json` and `textures.json` patterns (and the editor's
  import settings, which read `cook.json`) left out or matched only files in subfolders before.
- The hierarchy's right-click menu, the gizmo's Move, Rotate and Scale tooltips, and the overlay
  inspector's Duplicate, Despawn and Redo tooltips and help lines show the keys as bound now,
  not the default keys after a rebinding.
- `ponko image-diff` names an unknown option instead of printing only its usage, takes options
  before or between the two images, and has a full `--help`; `ponko help help` and, in a build
  without the `textures` feature, `ponko help textures` answer instead of "no command".
- `ponko --help` lists `ponko new --plugin`, `ponko editor --last` and `--recent`, and
  `ponko pack --list`; `ponko notices --help` describes each of its options.
- The editor's **3D** view button says why it is greyed out in a game without the `3d` feature.
- Accept Windows path separators in crash report tests and document the
  separators that can appear in reports.
- Depth-tested debug labels show in every 3D camera that shows the default
  layer, each laid out facing that camera: split-screen views and target
  cameras, not only the main camera. A main camera that leaves the default
  layer out no longer draws them.
- `d3::Text` shows in split-screen views and in target cameras that draw
  particles (it was drawn by the main camera only).
- Editor: a click on a texture, model or prefab tile in the asset panel
  selects it (it started a drag instead).
- Editor: 3D clicks and gizmos follow a camera whose rect isn't the whole
  window.
- The editor builds for the web with clippy clean (`--features editor`,
  and with `3d`). There, **Open** and **Save as** ask for the path in the
  editor's own field instead of a system dialog, and model thumbnails stay
  the software drawings: the offscreen renderer and its disk cache are
  left out of the web build.
- After the system file dialog closes, or the window's focus leaves and
  returns, the editor no longer keeps Ctrl (⌘), Shift or Enter held: it
  forgets every held key and modifier, so a plain key afterwards isn't
  taken for a shortcut.
- `ponko cook`: a map that a `"keep"` rule copies as it is now counts as a
  linker of the tileset-only Aseprite files it links to, so those files get
  no empty sheet (a `"skip"` rule still takes a file out of the count).
- `ponko_cli` compiles and passes its tests with `--no-default-features
  --features meshes,textures,shaders` (the shared cook fixture test needs
  the `aseprite` feature and is built only with it), and `textures` alone no
  longer warns about an unused dependency: kept glTF buffers are written
  with a small encoder in the cook instead of the `base64` crate.
- The command line tool's feature subsets (each of `textures`, `meshes`,
  `shaders` and `aseprite` alone, none, and `meshes,textures,shaders`) are
  linted and tested in the list of checks.
- The editor's selection rings and gizmo are drawn only over the view, no
  longer over the panels.
- The editor's hierarchy marks prefab instances with a glyph its fonts
  have (it drew a missing-glyph box).

**C and C++**
- Writing a `PonkoSprite` back with `ponko_set`, `ponko_ctx_set_later` or `world_view::set` no longer clears the
  flips of the sprite the entity already has. A sprite added to an entity that had none starts unflipped.
- The C library builds with `video` on arm64 Linux and Android: the shared
  library (`libponko_ffi.so`) failed to link because the AV1 decoder's
  assembly reads its tables with PC-relative addresses. `ponko_ffi` now
  links it with `-Wl,-Bsymbolic` there (only with `video`), keeping the
  assembly. A shared library of your own that takes in `libponko_ffi.a`
  with `video` on those systems needs the same flag (the guide's "Video",
  C and C++).
- C++: with `PONKO_COUNT_CPP_ALLOCATIONS`, `operator new` returned blocks
  aligned to 8 bytes on Apple silicon, where `new` promises 16
  (`max_align_t` is 8 bytes there): the size kept in front of each block
  now takes the default alignment of `new`. Found by the sanitizer build
  of the C++ tests.
- `cargo clippy -p ponko --features compressed-textures` (without `3d`)
  no longer warns about the cooked-ORM note, which only glTF models read.

**Guide and examples**
- The guide note linked the changelog with a path from another host; it uses
  the project's GitHub path.
- The text-shaping snippet in the UI chapter asked for `ponko = "0.3"`; it
  asks for 0.2.
- `ponko_aseprite` and `ponko_lobby` carry the root `LICENSE-MIT`,
  `LICENSE-APACHE` and `NOTICE`, like every published crate; the release check
  needs them and its self-test failed without.
- Shard Grove's HUD no longer paints an opaque panel over the whole 3D view.
- Coin Dash and Ember Hop lay out for the window they get, every frame: on a
  phone upright or turned, Coin Dash's arena is shown whole in the safe area
  (letterboxed) and Ember Hop's title scene whole in the window; the status
  lines and the menus are placed in the safe area (clear of a notch, the
  Dynamic Island, a home bar and the on-screen pause button), centred, and
  drawn smaller rather than cut off. On a touch screen the hints name taps
  and the on-screen buttons. Their rules, levels and desktop look at the
  starting window size don't change.
- Ember Hop's lights no longer show their square's edge against the dark
  cave: the glow picture fades to clear at its edge.
- On-screen touch buttons: a label too wide for its disc ("Jump", "Flare")
  is drawn smaller, inside the rim.
- The guide's `use ponko::...;` lines are checked against the API snapshot:
  `scripts/check-book-snippets.py` fails when a Rust block (or an included
  example region) imports a path that does not exist. The tutorials' prose now
  names where each import lives (`ponko::prelude`, `ponko::d2`/`d3`,
  `ponko::tilemap`, `ponko::render`) and says `game() -> ConfiguredGame`.
- `STABILITY.md` says where the math types live (`ponko::prelude`, not the
  crate root), writes `ctx.random()`, and lists the `UiLayout` methods that
  expose Taffy types (`custom`, `style`, `style_mut`, `into_style`) as unstable.
- `book/src/api.md` and the getting-started page say which engine version the
  guide matches (`main`, the unreleased 0.2) and how to build the matching
  reference locally with the right features.
- Links from the docs to files of the repository are relative, and the built
  guide rewrites them to the project's host (`scripts/project-url.py`, which
  reads the one line in `PROJECT_URL`). Links to old issue and pull request
  numbers are plain text. `project-url.py set <url>` moves every literal copy
  of the project URL, and `project-url.py check` fails on links to the old
  host and on relative links that point nowhere.

**Development and releases**
- A glTF animation channel whose target names a missing node is a decode
  error instead of a panic, and channels with a path this loader doesn't
  read (another extension's, such as `pointer`) are skipped.
- A glTF accessor of a type glTF doesn't allow for its use (a `NORMAL` that
  isn't `VEC3`, float indices, colors of signed bytes), with no elements, a
  stride shorter than an element or an end past 2^64 is a decode error; such
  files panicked the loader, some in release builds too. Positions, normals,
  texture coordinates and morph targets must also fit their buffer view and
  have as many elements as `POSITION`, so a file can't claim a billion
  corners and make the loader set aside room for them.
- A glTF primitive whose `POSITION` names a missing accessor is a decode
  error instead of a panic in glTF validation.
- A glTF file whose nodes don't form trees (a node that is its own
  ancestor, has two parents, or is a scene root and a child) is a decode
  error; a cycle made the loader walk the scene forever.
- A `.glb` whose header gives a length under 12 bytes is a decode error
  instead of a panic in builds with overflow checks.
- A lockstep peer ignores a forged input packet whose ticks run past
  `u64::MAX` instead of panicking (debug builds) or wrapping (release).
- glTF: triangle strips and fans are drawn (they were left out); points and
  lines still aren't.
- glTF: a file with `KHR_animation_pointer` channels no longer fails to
  load: those channels are left out and its node animations play.
- `AGENTS.md` no longer says agent work is based on a `dev` branch; there is
  none, and the base is `main`. `CONTRIBUTING.md` no longer cites the issue
  numbers of the old backlog.
- The `mods-wasm` feature's lockfile moves `wasmi` (and `wasmi_core`,
  `wasmi_ir`, `wasmi_collections`) from the yanked 2.0.2 to 2.0.3, so
  `cargo deny check` passes again.
- Point and area lights no longer give NaN where the light is straight
  behind the viewer on GPUs that compute `pow` as `exp2(y * log2(x))`
  (AMD's Vulkan driver): the GGX Fresnel term's `v·h` could round past 1.
  Through global illumination's probes the NaN turned whole frames black.
- Crash reports on Windows name a function from its own first byte, not the
  symbol before it: `backtrace`'s dbghelp path looks one byte back twice for
  an address.
- The well-known content load test no longer fails in a workspace test run: a sample that needs a glTF extension from a cargo feature the build lacks is expected to skip naming that extension.
- `ponko_cli`'s tests pass at a release candidate's version: the template
  check expects a candidate's own `ponko = "0.y.0-rc.N"` pin (Cargo never
  picks a candidate for `"0.y"`), and the cook fixture tests compare the
  cooked Aseprite sheet and the kept-geometry model apart from the version
  the cook stamps into them (`meta.version`, each KTX2 image's
  `KTXwriter`), checking that stamp is the current version. The helper
  builds only with the features of the test that uses it, so the tool's
  feature subsets still build their tests with warnings denied.
  The packaging test looks for the engine at the current version in the
  third-party notices.
- The workspace docs build again with warnings denied (a broken link to
  `Ui::image` in the docs of `render_icon`), and the engine's tests
  build without warnings with a window but no audio (the clippy lines
  "engine with only a window", "with every image format" and "with text
  shaping and system fonts").
- Keep tracked text files in LF format on Windows checkouts so embedded shaders
  and documentation checks work with Git's default Windows settings.
- A web build with threads (`web/build.sh --threads`) compiled again: the
  built-in controller backend held the browser's gamepad objects, which can't
  be sent between threads once the memory is shared, and `GamepadBackend` is
  `Send`. Such a build now starts no built-in controllers (a game's own
  `GamepadBackend` works); the ordinary web build is unchanged.
- Tests that failed on and off on a loaded machine now count work instead of
  time: the 2D skeleton allocation test fixes and starts its thread pool
  before measuring and allows exactly the pool's own allocations, the C API
  asset group restart test no longer expects a fixed number of loaded assets
  before the restart, and background-loading, streaming, job and server tests
  wait for a state, with long hang-detector limits, instead of a number of
  frames or a short deadline.
- The control endpoint's exit tests check that no reply is still owed when
  the endpoint stops, instead of timing the stop.
- The scoreboard's notes said Ponko draws its 2D rows with 4x MSAA. Its 2D
  renderer draws with one sample, like Bevy's 2D camera in the scoreboard.
- `Ui::remove` kept every removed node's slot (about 400 bytes each): a
  three-node menu built and removed every few frames grew memory by about
  1.3 KB each time.
- A freed 3D material kept its own uniform buffer and bind group (128
  bytes of GPU memory each, for every material ever made); freed materials
  now share one.
- The window renderer made a new bind group for every texture it freed;
  freed textures now share one.
- `Time::elapsed` drifted from real time by the rounding of each frame's
  `dt` to `f32`: 1.9 ms after ten hours at 60 Hz (and away from
  `sim_time` in headless runs). It now adds the frames up in full precision.
- A looping sprite-sheet animation ran slow after hours of play (6% after
  ten hours at 60 Hz) as its `f32` time lost precision.
- Every texture, sound and font made kept memory after it was freed (about
  1, 32 and 280 bytes headless, and a table entry in the window renderer
  and the audio backend): by those figures 175 000 textures and sounds
  made and freed held about 5.8 MB; measured now, 4.7 KB (retired slots).
- A 3D `Animator` or 2D `Skeleton` in one looping state for hours kept its
  clock in an `f32`: after ten hours at 60 Hz it was 545 loops of a
  one-second clip ahead and stepped in 1/256 s. The clocks are `f64` now.
- Reduced-feature documentation no longer links to disabled physics and navigation APIs, and physics-only test builds gate the window-only shader fixture correctly.
- The image comparison crate includes the license and attribution files required by release checks.
- The asset group test `without_workers_other_assets_load_as_before` passes
  with the `image-gif` feature (and `image-formats`): it expected a GIF to be
  unsupported, which it is only without the decoder.
- The macOS screen-reader checks (`tests/ui_access_macos.rs`) count a check
  that didn't run (the window check, without `PONKO_ACCESS_WINDOW=1`) as
  skipped in their summary, not as passed.
- The docs step of `scripts/checks.yml` documents `ponko` with `mods-wasm`,
  as docs.rs does; without it the code mod links in `ponko::mods` failed
  the step.
- The C++ wrapper test of allocation counts passes in a Release build: clang
  removed its `new`/`delete` pair there, so nothing was counted.
- `ponko` builds with `-D warnings` on current nightly Rust (the sanitizer
  job): the crate raises its recursion limit, which proving the renderer's
  texture tables `Send + Sync` through wgpu's types exceeds.
- The C lobby test `a_c_game_makes_joins_and_plays_in_a_room` waits for the
  leaving client's own `PONKO_LOBBY_LEFT` before reading it; under
  AddressSanitizer it could arrive after the others' host change.
- The "Clippy, wasm-simd with simd128" step of `scripts/checks.yml` passes
  `+simd128` in `RUSTFLAGS`: with the list's own `RUSTFLAGS` set, Cargo
  ignored `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS` and the feature
  refused to build.

### Security

**Assets, scenes and saves**
- Image, texture and video decoders refuse sizes in a file's header that
  would allocate without bound, before allocating: 16 384 pixels a side and
  512 MiB of RGBA8 at most, an `ErrorKind::Limit` error past them. Covers
  every `image` format, KTX2 and DDS files, the streamed PNG path and WebM
  video (its container size and the AV1 decoder's frame size).
- A KTX2 file whose supercompressed (Zstandard or zlib) levels unpack to
  more than their size needs is refused, and unpacking stops as soon as it
  passes it: a few bytes could claim terabytes before. A KTX2 file with a
  texel block dimension of 256 in its format descriptor is an error
  instead of a panic in the `ktx2` crate.
- Levels (LDtk, Tiled) refuse tile layers of more than 16 777 216 cells,
  tilesets of more than 1 048 576 tiles and tiles or tileset sheets past
  the image limits, with `ErrorKind::Limit`; Tiled's zlib and gzip layer
  data stops unpacking at the cell limit. Sizes that overflowed in tileset
  arithmetic saturate instead.

**Networking**
- WebSocket connections that finish their handshake and then send nothing
  for 30 s are dropped, so silent sockets can't hold a host's connection
  slots.
- A UDP message host (`NetMessages::host`) checks that a joining client
  can receive at its address before it gives the client anything: a hello
  without a valid cookie gets one challenge (41 bytes, smaller than the
  64-byte padded hello, at most 256 a second) and the host keeps no slot
  and no memory for it. The cookie is SipHash-2-4-128 over the client's
  address, port, protocol version, nonce and key period, with keys made at
  start and every 5 s (a cookie is good for 5 to 10 s), compared in
  constant time. Forged source addresses no longer fill slots or use the
  host as a reflector. Always on; C and C++ need no new call. WebSocket,
  WebRTC and Steam relay clients are unchanged (book/src/security.md,
  "Forged hellos").
- A UDP message host's cap on challenges and version refusals is no
  longer one budget shared by every stranger: each network (IPv4 /24,
  IPv6 /56) gets 16 answers a second of its own, under the global 256, so
  a flood from one network no longer delays joins from others. The
  per-network budgets are a fixed table of 64 entries inside the host
  (no allocation, nothing kept longer than a second); while it is full, a
  new network has the global budget only. Always on; C and C++ need no
  new call (book/src/security.md, "The challenge budget").
- A host still sends nothing to a client of message protocol 1: such a
  client could only report a refusal as "host is full", which would be
  wrong, so its join times out (book/src/networking.md).

**Tools: editor, inspector and agents**
- The control endpoint's `sim.step` queues any number of ticks at once
  instead of one at a time (four billion ticks took seconds of the game
  thread).

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
