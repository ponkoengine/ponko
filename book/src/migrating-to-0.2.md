# Migrating from 0.1 to 0.2

<!-- Maturity: not a feature. A migration guide for one release. -->

Ponko 0.2 breaks the API in the ways listed here, grouped by area. Each entry says what changed, what to write instead, and shows the code before and after. The compiler names most of them; look the message up below. The code after each change is compiled by the guide's checks, so it builds as shown.

The entries come from the release's changelog entries, so the [CHANGELOG](../../CHANGELOG.md) lists the same changes in short form next to the rest of the release. [STABILITY.md](../../STABILITY.md) says what minor releases may break and how a replaced item is deprecated first where that is cheap.

- [API and crate layout](#api-and-crate-layout): 14 changes
- [ECS and systems](#ecs-and-systems): 8 changes
- [Rendering](#rendering): 6 changes
- [Animation](#animation): 9 changes
- [Assets, scenes and saves](#assets-scenes-and-saves): 5 changes
- [UI, text and input](#ui-text-and-input): 8 changes
- [Audio](#audio): 1 change
- [Physics, navigation and AI](#physics-navigation-and-ai): 2 changes
- [Networking](#networking): 2 changes
- [C and C++](#c-and-c): 1 change
- [Development and releases](#development-and-releases): 1 change

<!-- changelog.py: entries start -->

## API and crate layout

### `ponko_ffi::PonkoHandleKind` has three new values, `PONKO_HANDLE_SPRITE_SHADER`, `PONKO_HANDLE_SPRITE_MATERIAL` and `PONKO_HANDLE_RENDER_TARGET`

So a Rust `match` over every value no longer compiles. C and C++ code is not affected.

**Instead:** add arms for the new values, or a `_` arm.

**Before:**

```rust
match kind {
    PonkoHandleKind::PONKO_HANDLE_TEXTURE => free_texture(id),
    // ... one arm per value up to 0.1.0
}
```

**After:**

```rust
use ponko_ffi::PonkoHandleKind;
# fn free_texture(id: u64) {}
# let (kind, id): (PonkoHandleKind, u64) = todo!();
match kind {
    PonkoHandleKind::PONKO_HANDLE_TEXTURE => free_texture(id),
    // ... the 0.1.0 arms
#   PonkoHandleKind::PONKO_HANDLE_SOUND | PonkoHandleKind::PONKO_HANDLE_FONT
#   | PonkoHandleKind::PONKO_HANDLE_MESH | PonkoHandleKind::PONKO_HANDLE_MATERIAL
#   | PonkoHandleKind::PONKO_HANDLE_SHADER | PonkoHandleKind::PONKO_HANDLE_COMPUTE
#   | PonkoHandleKind::PONKO_HANDLE_EFFECT | PonkoHandleKind::PONKO_HANDLE_GPU_BUFFER
#   | PonkoHandleKind::PONKO_HANDLE_INSTANCE_BATCH
#   | PonkoHandleKind::PONKO_HANDLE_TEXTURE_ARRAY => {}
    PonkoHandleKind::PONKO_HANDLE_SPRITE_SHADER
    | PonkoHandleKind::PONKO_HANDLE_SPRITE_MATERIAL
    | PonkoHandleKind::PONKO_HANDLE_RENDER_TARGET => {}
}
```

### The `ponko::physics` module is gone: 2D physics lives in `ponko::d2`

`physics::{RigidBody, Collider, Velocity, Shape, Physics, QueryFilter}` were also public as `d2::*`; `physics::RayHit` is `d2::Hit`, `physics::MAX_POLYGON_POINTS` is `d2::Shape::MAX_POLYGON_POINTS`, and the step systems are in `ponko::systems`. The types are the same; saved component names (`RigidBody`, `Collider`, ...) don't change.

**Instead:** import from `ponko::d2` (or `ponko::systems` for the systems).

**Before:**

```rust
use ponko::physics::{Collider, RayHit, RigidBody, MAX_POLYGON_POINTS};
```

**After:**

```rust
use ponko::d2::{Collider, Hit, RigidBody, Shape};
let most = Shape::MAX_POLYGON_POINTS;
```

### `ponko::d3` no longer re-exports the types of topic modules, so each type has one path

Most of 0.1's `d3` names stay as deprecated aliases until 0.3.0 (see "Deprecated", and the removal list there); what breaks now is `d3::Model` and `d3::{BodyKind, CollisionStarted, CollisionStopped}` (the prelude has them), a tuple-struct constructor or pattern through an alias (`ShadowMesh(None)`, `LayerId(0)`), and a module that glob-imports `d3` together with `render`, `anim`, `particles` or `terrain`, where those names are now ambiguous (E0659). The render types (`AmbientOcclusion`, `AntiAliasing`, `BatchCulling`, `Blend`, `Compute`, `ComputePass`, `Effect`, `GlobalIllumination`, `GpuBuffer`, `GraphicsQuality`, `InstanceBatch`, `InstanceBatchOptions`, `InstanceLevel`, `Lighting`, `LodGroup`, `LodHysteresis`, `LodLevel`, `LodPick`, `LodRenderer`, `LodSettings`, `LodShadow`, `Material`, `MaterialDesc`, `Mesh`, `MeshCost`, `MeshData`, `Model`, `ModelMaterial`, `ModelMaterials`, `Post`, `ReceivesDecals`, `RenderPasses`, `SceneSettings`, `ScreenEffect`, `Shader`, `ShaderDiagnostic`, `ShaderKind`, `ShaderOutcome`, `ShaderStatus`, `ShadowFilter`, `ShadowMesh`, `Sky`, `StreamedLod`, `TargetCamera`, `TargetEffect` and `Tonemap`) are in `ponko::render`, the animation types (`AnimationClip`, `AnimationCondition`, `AnimationEvent`, `AnimationGraph`, `AnimationGraphError`, `AnimationLayer`, `AnimationParameter`, `AnimationState`, `AnimationTrack`, `AnimationTransition`, `Animator`, `AttachToJoint`, `Interpolation`, `JointAnimation`, `JointMask`, `LayerBlend`, `LayerId`, `MorphAnimation`, `MorphFrame`, `MorphTarget`, `MorphWeights`, `Motion`, `MotionChild`, `ParamId`, `RootMotion`, `Skeleton`, `SkinOf` and `StateId`) in `ponko::anim`, terrain types (`Heightfield`, `TerrainTile`, `TerrainTileData`, `TerrainMaterialDesc`, `TextureArray`, `TextureKind`, `FarTerrain`) in `ponko::terrain` (its `CellStreamer` is a deprecated alias of `ponko::streaming::CellStreamer`), `ParticleCurve`, `ParticleGradient` and `ParticleSpace` in `ponko::particles`, and `BodyKind`, `CollisionStarted` and `CollisionStopped` in `ponko::collision`. `d3` keeps the types named after the dimension (`Transform`, `Camera`, lights, decals, text, trails, particle emitters, positional audio, navigation agents, `UiPanel`) and the 3D physics. `Model` stays in the prelude.

**Instead:** import these types from their module next to `use ponko::d3::*;` (by name, not with a second glob, while the aliases last), and build a tuple struct through its new path.

**Before:**

```rust
use ponko::d3::*;
let desc = MaterialDesc::color(Color::WHITE);
let shadow = ShadowMesh(None);
```

**After:**

```rust
use ponko::d3::*;
use ponko::render::{MaterialDesc, ShadowMesh};
let desc = MaterialDesc::color(Color::WHITE);
let shadow = ShadowMesh(None);
```

### `ponko::d2` no longer re-exports the types of topic modules

Tile maps and levels (`TileMap`, `Tileset`, `Level`, `LevelOptions`, `LevelObject`, `AutoTile`, ...) are in `ponko::tilemap`, `Animation` and `SpriteSheet` in `ponko::sheet` (`SpriteSheet` stays in the prelude), `Attached` in `ponko::attach`, `SpatialGrid` in `ponko::spatial`, `NavGrid` and `FlowField` in `ponko::flow`, `BlendMode`, `SpriteMaterial`, `SpriteMaterialDesc` and `SpriteShader` in `ponko::render`, the particle curves in `ponko::particles` and the collision types in `ponko::collision`. `d2` keeps `Transform`, `Camera`, `Sprite`, the particle emitter types and the 2D physics.

**Instead:** import these types from their module.

**Before:**

```rust
use ponko::d2::*;
let child = (Transform::default(), Attached::to(parent));
let map = TileMap::new(tiles, 32, 32);
```

**After:**

```rust
# let (parent, tiles) = todo!();
use ponko::d2::*;
use ponko::{attach::Attached, tilemap::TileMap};
let child = (Transform::default(), Attached::to(parent));
let map = TileMap::new(tiles, 32, 32);
```

### Types of one dimension are no longer also public in a topic module under their suffixed name

`camera::Camera2D` is `d2::Camera`, `particles::{ParticleEmitter2D, ParticleSettings2D, ParticleShape2D}` are `d2::{ParticleEmitter, ParticleSettings, ParticleShape}`, `audio::{AudioSource3D, AudioListener3D, AudioSettings3D}` are `d3::{AudioSource, AudioListener, AudioSettings}` and `ui::UiPanel3D` is `d3::UiPanel`. `camera` keeps `Rect` and `camera_controls`, `particles` the curves shared by both dimensions, `audio` keeps `AudioRolloff`. Saved component names don't change.

**Instead:** use the `d2`/`d3` names (the prelude's `d2` and `d3` modules reach them).

**Before:**

```rust
use ponko::audio::AudioSource3D;
use ponko::particles::ParticleEmitter2D;
```

**After:**

```rust
use ponko::d2::ParticleEmitter;
use ponko::d3::AudioSource;
```

### `ponko::nav` holds only the navigation-mesh baking types (`NavBakeSettings`, `NavBakeError`, `NavSource`)

Its copies of `d3::{NavMesh, NavAgent, NavObstacle}`, of `flow::{NavGrid, FlowField}` and of `systems::step_navmesh3d` are gone. `ponko::anim` no longer re-exports `sheet::{Animation, SpriteSheet}` and `systems::animate_sprites`.

**Instead:** use the paths named here.

**Before:**

```rust
use ponko::nav::{NavAgent, NavGrid, NavMesh};
use ponko::anim::Animation;
```

**After:**

```rust
use ponko::d3::{NavAgent, NavMesh};
use ponko::flow::NavGrid;
use ponko::sheet::Animation;
```

### `ponko::alloc_count` and `ponko::scope` are private

Their items were also in `ponko::perf`, which is now their one path (`CountingAllocator`, `AllocCounts`, `alloc_counts`, `note_allocation`, `ProfileScope`, `ScopeGuard`, `ScopeTiming`, `scope_named`).

**Instead:** import them from `ponko::perf`.

**Before:**

```rust
#[global_allocator]
static ALLOC: ponko::alloc_count::CountingAllocator = ponko::alloc_count::CountingAllocator;
```

**After:**

```rust
#[global_allocator]
static ALLOC: ponko::perf::CountingAllocator = ponko::perf::CountingAllocator;
```

### The built-in systems are public only in `ponko::systems`

`spatial::rebuild_spatial_grid`, `sheet::animate_sprites`, `tilemap::draw_tilemaps`, `attach::follow_attachments` and `net::apply_net_poses` are gone from their topic modules (the names the built-ins are listed under, such as `ponko::tilemap::draw_tilemaps` in plans and for `ponko_game_remove_system`, don't change).

**Instead:** use `ponko::systems::<name>`.

**Before:**

```rust
game.add(Phase::SimLast, ponko::attach::follow_attachments)
```

**After:**

```rust
# let game = Game::new("g", 1, 1);
game.add(Phase::SimLast, ponko::systems::follow_attachments)
```

### The Steam API takes and returns Ponko's own types

`ponko::net::SteamClient`, `SteamLobbyId` and `SteamUserId` replace `steamworks`' `Client`, `LobbyId` and `SteamId` in `Net::steam_host`, `steam_join`, `steam_lobby`, `NetMessages::steam_host`, `steam_join`, `steam_peer`, `SteamLobby::create`, `join`, `id`, `members`, `steam_lobby_from_args` and `steam_wait_for_invite`. A new `steamworks` release is no longer a breaking Ponko release. Feature `steam`; the C API is unchanged.

**Instead:** start Steam with `SteamClient::init_app` (or `SteamClient::init`) and pass it where the `steamworks::Client` went. Lobby and user ids convert with `from_raw` and `raw`, print and parse as their 64-bit number.

**Before:**

```rust
let client = steamworks::Client::init_app(480).expect("is Steam running?");
let net = match ponko::net::steam_lobby_from_args() {
    Some(lobby) => Net::steam_join(&client, lobby),
    None => Net::steam_host(&client, 2),
};
let lobby = steamworks::LobbyId::from_raw(id);
```

**After:**

```rust
use ponko::net::Net;
# let id: u64 = todo!();
let client = ponko::net::SteamClient::init_app(480)?;
let net = match ponko::net::steam_lobby_from_args() {
    Some(lobby) => Net::steam_join(&client, lobby),
    None => Net::steam_host(&client, 2),
};
let lobby = ponko::net::SteamLobbyId::from_raw(id);
```

### `ponko::net::steam`, the re-exported `steamworks` crate, is `ponko::net::unstable_steamworks` now

Like `ui::unstable_taffy`, it is outside the versions: its `steamworks` version may change in any release.

**Instead:** use Ponko's Steam types; for the parts of Steam Ponko doesn't wrap (achievements, stats, rich presence), take the client underneath with `SteamClient::unstable_steamworks`, and pass ids across as `u64`.

**Before:**

```rust
let client = ponko::net::steam::Client::init_app(480).unwrap();
let _ = client.user_stats().achievement("WIN_ONE").set();
```

**After:**

```rust
let client = ponko::net::SteamClient::init_app(480)?;
let _ = client.unstable_steamworks().user_stats().achievement("WIN_ONE").set();
```

### The prelude no longer exports `AssetSource`, `MemorySource`, `PackSource` and `SetupResult`

They are what a game names only when it adds an asset source or writes a bound on fallible setup, so they stay in their modules. A game's own type named `UiEvent`, `UiLayout`, `Gamepad`, `GamepadAxis`, `GamepadButton` or `GamepadStick` that it glob-imports next to the prelude is now ambiguous: import it by name, or use the engine's.

**Instead:** import them from `ponko::assets` (`ponko::game::SetupResult`); `source.read(name)` needs `AssetSource` in scope.

**Before:**

```rust
use ponko::prelude::*;
ctx.assets.add_source(MemorySource::new("embedded").with("tuning.json", TUNING));
```

**After:**

```rust
# const TUNING: &[u8] = b"{}";
use ponko::assets::MemorySource;
use ponko::prelude::*;
ctx.assets.add_source(MemorySource::new("embedded").with("tuning.json", TUNING));
```

### `ponko_image_diff::compare`, `heatmap` and `compare_files` return `ponko_image_diff::Error`

The error keeps the failure's kind, the file and the underlying cause.

**Instead:** match `err.kind()` (with a `_` arm), or turn it into a string with `String::from(err)`.

**Before:**

```rust
let diff: Result<Diff, String> = compare_files(a, b, tolerance, None);
```

**After:**

```rust
use ponko_image_diff::{compare_files, Diff};
# let (a, b, tolerance) = todo!();
let diff: Result<Diff, String> = compare_files(a, b, tolerance, None).map_err(String::from);
```

### Render targets have their own handle type, `RenderTarget`, instead of `Texture`

`create_render_target`, `try_create_render_target` and a UI panel's `target` (it was `texture`) give a `RenderTarget`; `TargetCamera::new`, `TargetCamera::reflection`, `ComputePass::new`, `TargetEffect::new` and `d3::UiPanel::new` take only that, and their `target` fields are one. Scenes and saves are unaffected (they never stored render targets, and store other handles by name).

**Instead:** the migration is mechanical. (1) Keep what `create_render_target` returns as a `RenderTarget` (`use ponko::render::RenderTarget;`), and change helper functions, struct fields and collections that hold a render target from `Texture` to `RenderTarget`. (2) Where a target is read as a texture (`MaterialDesc::with_custom_textures`, `with_texture`, `with_emissive_map`, `with_normal_map`, `Sprite::new`, `ctx.gfx.draw`, `update_texture`, `is_render_target`), write `target.texture()`. (3) `panel.texture` becomes `panel.target.texture()`. (4) Comparing a pass's `target` with a `Texture`: compare `target.texture()`, or keep the `RenderTarget` and compare that. (5) Code reading `c.target.id` reads `c.target.texture().id`. The compiler names every place: each is a type mismatch between `Texture` and `RenderTarget`.

**Before:**

```rust
let ripples: Texture = ctx.gfx.create_render_target(256, 256);
ctx.scene3d.passes.compute.push(ComputePass::new(waves, ripples));
let water = MaterialDesc::default().with_custom_textures([Some(ripples), None]);
let (panel, model) = ctx.gfx.create_ui_panel(600, 400, vec2(1.2, 0.8));
let screen = MaterialDesc::default().with_emissive_map(panel.texture);
```

**After:**

```rust
# let waves = todo!();
use ponko::render::{ComputePass, MaterialDesc, RenderTarget};
let ripples: RenderTarget = ctx.gfx.create_render_target(256, 256);
ctx.scene3d.passes.compute.push(ComputePass::new(waves, ripples));
let water = MaterialDesc::default().with_custom_textures([Some(ripples.texture()), None]);
let (panel, model) = ctx.gfx.create_ui_panel(600, 400, vec2(1.2, 0.8));
let screen = MaterialDesc::default().with_emissive_map(panel.target.texture());
```

### C++: render targets are `ponko::render_target`, and the calls that draw into one take only that

`ctx.create_render_target` and `ctx.create_ui_panel` return a `ponko::render_target`, which converts to `ponko::texture` wherever a texture is read. `target_camera_of`, `compute_pass_of`, `target_effect_of` and `ctx.reflection_camera` no longer accept a `ponko::texture`.

**Instead:** hold render targets as `ponko::render_target` (or `auto`) and take it in helpers; for a target read back from the engine (a `PonkoTargetCamera`'s, `PonkoComputePass`'s or `PonkoTargetEffect`'s `target`, which stay plain `PonkoTexture`s in C), call `ctx.render_target_of(t.target)`, which checks it and throws `ponko::error` if it isn't one. C code is unchanged, except that `ponko_ctx_reflection_camera` now refuses a texture that isn't a render target.

**Before:**

```cpp
ponko::texture screen = ctx.create_render_target(320, 180);
std::vector<ponko::target_camera> list = ctx.target_cameras();
for (auto& t : list) t = ctx.reflection_camera(ctx.camera3d(), t.target, 0);
```

**After:**

```cpp
ponko::render_target screen = ctx.create_render_target(320, 180);
std::vector<ponko::target_camera> list = ctx.target_cameras();
for (auto& t : list) t = ctx.reflection_camera(ctx.camera3d(), ctx.render_target_of(t.target), 0);
```

## ECS and systems

### An index into `RawWorldRef::archetypes` is valid only until the next change

It names an archetype only until the next structural change: a world now reclaims archetypes that stayed empty and builds new component sets in their slots; a reclaimed slot shows as an empty archetype without components. Query order, which follows the archetype slots, can differ from before (it is still the same for the same operations).

**Instead:** don't keep an index across spawns, despawns, inserts or removes; find the archetype again by its components (`Archetype::has`, `Archetype::component_ids`).

**Before:**

```rust
let index = world.raw_ref().archetypes().iter().position(|a| a.has(id)).unwrap();
world.spawn((Position::default(),));
let count = world.raw_ref().archetypes()[index].len();
```

**After:**

```rust
# #[derive(Component, Default)] struct Position(f32);
# let id = world.component_id::<Position>().unwrap();
world.spawn((Position::default(),));
let count = world.raw_ref().archetypes().iter().find(|a| a.has(id)).map_or(0, |a| a.len());
```

### `schedule::Stage::systems` returns `&[usize]` instead of a `Range<usize>`

A batch's systems need not be neighbours any more (see Changed), so a stage lists them in the new `Stage::members` field, in registration order; `first` and `len` stay, as its first system and how many it has.

**Instead:** iterate the slice (`.iter().copied()`) or read `stage.members`; `stage.systems().contains(&i)` works as before.

**Before:**

```rust
for i in stage.systems() {
    println!("{}", plan.systems[i].name);
}
```

**After:**

```rust
# let plan = Game::new("g", 1, 1).schedule_plan();
# let stage = &plan.stages[0];
for &i in stage.systems() {
    println!("{}", plan.systems[i].name);
}
```

### `d2::Collider` has a `groups` field (its collision groups)

Colliders made with `Collider::circle`, `rect`, `capsule` or `polygon` are unchanged and start in every group; saved colliders without the field load in every group.

**Instead:** build colliders with their constructors, or add `groups: CollisionGroups::ALL` to a struct literal.

**Before:**

```rust
let c = ponko::d2::Collider { shape, density: 1.0, friction: 0.5, restitution: 0.0, sensor: false, offset: Vec2::ZERO };
```

**After:**

```rust
# let shape = todo!();
let c = ponko::d2::Collider { shape, density: 1.0, friction: 0.5, restitution: 0.0, sensor: false, offset: Vec2::ZERO, groups: ponko::d2::CollisionGroups::ALL };
```

### `d2::QueryFilter` has a `groups` field, as `d3::QueryFilter` does

**Instead:** build filters with `QueryFilter::default()` and its builders (`.exclude(e)`, `.groups(g)`, `.with_sensors()`), or add `groups: CollisionGroups::ALL` to a struct literal.

**Before:**

```rust
let filter = ponko::d2::QueryFilter { exclude: Some(player), sensors: true };
```

**After:**

```rust
# let player: Entity = todo!();
let filter = ponko::d2::QueryFilter::default().exclude(player).with_sensors();
```

### `Commands::spawn` returns the new `Entity` instead of `()`

A closure or function whose last expression was `commands.spawn(...)` now returns an `Entity`, so a system written that way no longer fits where `()` is expected.

**Instead:** end the call with a semicolon, or use the returned entity.

**Before:**

```rust
game.sim_system(|ctx: &mut Ctx| ctx.commands.spawn((Marker,)));
```

**After:**

```rust
# #[derive(Component)] struct Marker;
# let game = Game::new("g", 1, 1);
game.sim_system(|ctx: &mut Ctx| {
    ctx.commands.spawn((Marker,));
});
```

### `Commands::spawn` on a buffer that isn't tied to a world panics

The id is reserved in the buffer's world when `spawn` returns, so a buffer from `Commands::new()` must know its world first. `ctx.commands` and a view system's `sys.commands` already do; applying a buffer once ties it to that world.

**Instead:** make your own buffers with `world.commands()`, or call `commands.bind(&world)` before the first spawn.

**Before:**

```rust
let mut commands = Commands::new();
commands.spawn((Marker,));
world.apply(&mut commands);
```

**After:**

```rust
# #[derive(Component)] struct Marker;
let mut commands = world.commands();
commands.spawn((Marker,));
world.apply(&mut commands);
```

### `systems::follow_attachments` places only the subtrees whose root moved or whose attachments changed

So a transform written straight into an attached entity lasts. A still subtree costs nothing per tick. A value written into an attached entity's `Transform` used to be reset every tick and now stays until its subtree is placed again. Chains have no depth limit any more (they were broken past 64 links), and an attachment whose parent has no transform is left where it is instead of being despawned.

**Instead:** to move an attached entity, change its `Attached`/`Attached3D` offset (or move its parent), as before.

**Before:**

```rust
if let Some(t) = ctx.world.get_mut::<Transform2D>(sword) { t.position.y += 4.0; }
```

**After:**

```rust
use ponko::attach::Attached;
# let sword: Entity = todo!();
if let Some(a) = ctx.world.get_mut::<Attached>(sword) { a.offset.y += 4.0; }
```

### Adding a plugin with the same name twice makes the game fail to start

A plugin's name is its type's path, so two instances of one plugin type used to run both; now the second is an error (`ErrorKind::Invalid`).

**Instead:** give each instance its own name with `Plugin::name`, or add the plugin once.

**Before:**

```rust
impl Plugin for Level {
    fn build(&self, game: Game) -> Game { game }
}
Game::new("g", 1, 1).plugin(Level("cave")).plugin(Level("tower"));
```

**After:**

```rust
# struct Level(&'static str);
impl Plugin for Level {
    fn build(&self, game: Game) -> Game { game }
    fn name(&self) -> &str { self.0 }
}
Game::new("g", 1, 1).plugin(Level("cave")).plugin(Level("tower"));
```

## Rendering

### `PerfReport` has a new field, `gpu_memory`, and `Budget` a new field, `gpu_memory_mb`

Code that builds either with a struct literal naming every field no longer compiles; reading them, `Default` and the builders are unchanged.

**Instead:** end the literal with `..Default::default()`, or build a budget with `Budget::new()` and its methods.

**Before:**

```rust
let budget = Budget { frame_avg_ms: None, frame_p99_ms: Some(4.0), frame_max_ms: None,
    allocs_per_frame: Some(0.0), systems: Default::default(), scopes: Default::default() };
```

**After:**

```rust
use ponko::perf::Budget;
let budget = Budget { frame_p99_ms: Some(4.0), allocs_per_frame: Some(0.0), ..Default::default() };
```

### `Sprite` has two new public fields, `flip_x` and `flip_y`

Code that builds a `Sprite` with a struct literal naming every field no longer compiles.

**Instead:** start from `Sprite::new` and the `with_*` builders, or add the two fields (`false` keeps the old look).

**Before:**

```rust
let s = Sprite { texture, color, z: 0.0, pivot: Vec2::splat(0.5), region: None };
```

**After:**

```rust
# let (texture, color): (Texture, Color) = todo!();
let s = Sprite::new(texture).with_color(color);
```

### The 3D scene depth is reverse-Z: 1 at the camera's near plane, 0 at the far plane and the sky

Shaders that read `scene_depth_texture` directly (a material, screen effect or compute pass) see the depth flipped; positions and distances from `scene_position` and `scene_distance` are unchanged. Shadow maps keep standard depth.

**Instead:** use `scene_position` or `scene_distance`, or treat a greater depth as nearer and 0 as the sky (standard depth `d` is `1.0 - d` now for the same near and far planes).

**Before:**

```wgsl
fn is_sky(texel: vec2<i32>) -> bool {
    return textureLoad(scene_depth_texture, texel, 0) >= 1.0;
}
```

**After:**

```wgsl
fn is_sky(texel: vec2<i32>) -> bool {
    return textureLoad(scene_depth_texture, texel, 0) <= 0.0;
}
```

### `TargetCamera` is `#[non_exhaustive]` and no longer `Copy`

It holds a list of screen effects now, and new fields can be added without breaking games again.

**Instead:** build one with `TargetCamera::new` or `TargetCamera::reflection` and its `with_` methods, and borrow or `clone()` one where it was copied.

**Before:**

```rust
let mirror = ctx.scene3d.passes.target_cameras[0];
let camera = TargetCamera { camera, target, clip_plane: None };
```

**After:**

```rust
use ponko::render::TargetCamera;
# let (camera, target) = todo!();
let mirror = &ctx.scene3d.passes.target_cameras[0];
let camera = TargetCamera::new(camera, target);
```

### Material normal maps no longer lean surfaces the opposite way on both axes

A game that made its normal maps look right by flipping them (a negative `normal_strength`, or images with red and green inverted) now sees them inverted.

**Instead:** undo the workaround: a positive `normal_strength`, and the images as exported (glTF and OpenGL style, green up).

**Before:**

```rust
let bumps = MaterialDesc::default().with_normal_map(normals).with_normal_strength(-1.0);
```

**After:**

```rust
use ponko::render::MaterialDesc;
# let normals: Texture = todo!();
let bumps = MaterialDesc::default().with_normal_map(normals).with_normal_strength(1.0);
```

### `ModelLight` has a new field, `joint`

A light from a file with skins or animations names the joint its node is.

**Instead:** set `joint: None` where you build a `ModelLight` yourself.

**Before:**

```rust
let lamp = ModelLight {
    name: "lamp".into(),
    transform: ponko::d3::Transform::at(vec3(0.0, 2.0, 0.0)),
    kind: ModelLightKind::Point(ponko::d3::PointLight::default()),
};
```

**After:**

```rust
# use ponko::render::{ModelLight, ModelLightKind};
let lamp = ModelLight {
    name: "lamp".into(),
    transform: ponko::d3::Transform::at(vec3(0.0, 2.0, 0.0)),
    kind: ModelLightKind::Point(ponko::d3::PointLight::default()),
    joint: None,
};
```

## Animation

### `ponko::anim::AnimationState` is `#[non_exhaustive]`

Struct literals and `..AnimationState::new(..)` updates outside the engine no longer compile. Its fields stay public to read and change.

**Instead:** start from `AnimationState::new(motion)` and chain `with_speed`, `with_speed_parameter`, `with_looping`, `with_event` or `with_events`.

**Before:**

```rust
let once = AnimationState {
    looping: false,
    ..AnimationState::new(Motion::Clip(clip))
};
```

**After:**

```rust
use ponko::anim::{AnimationState, Motion};
# let clip = todo!();
let once = AnimationState::new(Motion::Clip(clip)).with_looping(false);
```

### `ponko::anim::MotionChild` is `#[non_exhaustive]`

Struct literals outside the engine no longer compile.

**Instead:** use `MotionChild::new(motion)` (time scale 1) and `with_time_scale` for another clock speed.

**Before:**

```rust
let child = MotionChild { motion: Motion::Clip(walk), time_scale: 1.5 };
```

**After:**

```rust
use ponko::anim::{Motion, MotionChild};
# let walk = todo!();
let child = MotionChild::new(Motion::Clip(walk)).with_time_scale(1.5);
```

### `ponko::anim::AnimationEvent` is `#[non_exhaustive]`

Struct literals outside the engine no longer compile.

**Instead:** use `AnimationEvent::new(time, name)`; the name takes any `Into<Arc<str>>` (`&str`, `String`, `Arc<str>`).

**Before:**

```rust
state.events.push(AnimationEvent { time: 0.5, name: "step".into() });
```

**After:**

```rust
use ponko::anim::AnimationEvent;
# let mut state: ponko::anim::AnimationState = todo!();
state.events.push(AnimationEvent::new(0.5, "step"));
```

### `ponko::anim::Joint` is `#[non_exhaustive]`: struct literals outside the engine no longer compile

**Instead:** use `Joint::new(name, parent)` (`parent` is an index, or `None` for a root) and `with_rest` for its rest pose.

**Before:**

```rust
let knee = Joint { name: "knee".into(), parent: Some(0), rest: Transform::at(vec3(0.0, -0.4, 0.0)) };
```

**After:**

```rust
use ponko::anim::Joint;
# use ponko::d3::Transform;
let knee = Joint::new("knee", 0).with_rest(Transform::at(vec3(0.0, -0.4, 0.0)));
```

### `ponko::anim::JointAnimation` is `#[non_exhaustive]`

Struct literals outside the engine no longer compile.

**Instead:** use `JointAnimation::new(joint, name)` and `with_translation`, `with_rotation` or `with_scale` for the tracks it has.

**Before:**

```rust
let knee = JointAnimation { joint: 1, name: "knee".into(), translation: None, rotation: Some(bend), scale: None };
```

**After:**

```rust
use ponko::anim::JointAnimation;
# let bend: ponko::anim::AnimationTrack = todo!();
let knee = JointAnimation::new(1, "knee").with_rotation(bend);
```

### `ponko::anim::MorphAnimation` is `#[non_exhaustive]`

Struct literals outside the engine no longer compile.

**Instead:** use `MorphAnimation::new(name, track)`.

**Before:**

```rust
let smile = MorphAnimation { name: "smile".into(), track };
```

**After:**

```rust
use ponko::anim::MorphAnimation;
# let track = todo!();
let smile = MorphAnimation::new("smile", track);
```

### `ponko::anim::MorphTarget` is `#[non_exhaustive]`

Struct literals and `..Default::default()` updates outside the engine no longer compile.

**Instead:** use `MorphTarget::new(name, positions)` and `with_normals` when the normals change too.

**Before:**

```rust
mesh.morph_targets.push(MorphTarget { name: "raise".into(), positions: up, normals: Vec::new() });
```

**After:**

```rust
use ponko::anim::MorphTarget;
# let mut mesh: ponko::render::MeshData = todo!();
# let up = Vec::new();
mesh.morph_targets.push(MorphTarget::new("raise", up));
```

### `ponko::anim::AttachToJoint` is `#[non_exhaustive]`

Struct literals outside the engine no longer compile. Saved scenes and prefabs load as before.

**Instead:** use `AttachToJoint::new(skeleton, joint)` and `with_offset`.

**Before:**

```rust
ctx.world.spawn((Model::new(cup), AttachToJoint { skeleton: person, joint: hand, offset }));
```

**After:**

```rust
use ponko::anim::AttachToJoint;
# let (cup, person, hand, offset) = todo!();
ctx.world.spawn((Model::new(cup), AttachToJoint::new(person, hand).with_offset(offset)));
```

### The `Motion::Blend1D` and `Motion::Blend2D` variants are `#[non_exhaustive]`

Building them with variant literals outside the engine no longer compiles, and patterns need `..`.

**Instead:** use `Motion::blend_1d(parameter, children)` and `Motion::blend_2d(x, y, children)`; match with `Motion::Blend1D { children, .. }`.

**Before:**

```rust
let tree = Motion::Blend1D { parameter: speed, children: vec![(0.0, walk), (1.0, run)] };
```

**After:**

```rust
use ponko::anim::Motion;
# let (speed, walk, run) = todo!();
let tree = Motion::blend_1d(speed, vec![(0.0, walk), (1.0, run)]);
```

## Assets, scenes and saves

### `TileMap` is `#[non_exhaustive]` and has new fields (`parallax`, `parallax_origin`, `animations`, `flips`)

So it can't be built with a struct literal outside the engine any more.

**Instead:** build it with `TileMap::new(sheet, width, height)` and set the fields you need afterwards.

**Before:**

```rust
let map = TileMap { sheet, size: uvec2(64, 64), tile_size: vec2(16.0, 16.0), z: 0.0, color: Color::WHITE, tiles };
```

**After:**

```rust
use ponko::tilemap::TileMap;
# let (sheet, tiles) = todo!();
let mut map = TileMap::new(sheet, 64, 64);
map.tiles = tiles;
```

### The plain collision-mesh loaders panic instead of returning a `Result`

**Instead:** call `try_load_collision_mesh` or `try_load_collision_mesh_with` where you handled the error, or drop the `.unwrap()` / `.expect(..)` where a missing file is a bug.

**Before:**

```rust
let solid = ctx.load_collision_mesh("yard.glb").expect("the yard loads");
let hull = ctx.load_collision_mesh_with("rock.glb", options)?;
```

**After:**

```rust
# let options = ponko::d3::CollisionMeshOptions::default();
let solid = ctx.load_collision_mesh("yard.glb"); // panics with a `ponko:` message if it fails
let hull = ctx.try_load_collision_mesh_with("rock.glb", options)?;
```

### `Assets::load_strings` panics instead of returning a `Result`

**Instead:** call `try_load_strings` where you handled the error, or drop the `?` / `.unwrap()` where a missing table is a bug.

**Before:**

```rust
ctx.assets.load_strings(&mut ctx.world, "lang/en.json")?;
```

**After:**

```rust
ctx.assets.load_strings(&mut ctx.world, "lang/en.json");
// or, for a table that may be missing:
ctx.assets.try_load_strings(&mut ctx.world, "lang/fr.json")?;
```

### `Assets::load_resource_json` and `load_component_json` are `load_reflected_resource_json` and `load_reflected_component_json`

One name per loader: they still return a `Result` and merge a file's fields into the reflected value, where `load_resource` replaces the whole resource. The old names are removed without a deprecated alias.

**Instead:** rename the calls; arguments and results are the same.

**Before:**

```rust
ctx.assets.load_resource_json::<Rules>(&mut ctx.world, "rules.json")?;
ctx.assets.load_component_json::<Stats>(&mut ctx.world, "stats.json", entity)?;
```

**After:**

```rust
# #[derive(Default)] struct Rules; #[derive(Default)] struct Stats;
# let entity: Entity = todo!();
ctx.assets.load_reflected_resource_json::<Rules>(&mut ctx.world, "rules.json")?;
ctx.assets.load_reflected_component_json::<Stats>(&mut ctx.world, "stats.json", entity)?;
```

### `ponko_unity::Error`'s public `message` field is the `message()` method now

The error keeps its text, path, line and kind private so more can be added without breaking callers; `Error` is still `Clone`, `Eq`, `Display` and `std::error::Error`.

**Instead:** call `error.message()` for the cause alone (it no longer holds the file name or `line N:` prefix), `error.to_string()` for the whole text, and match on `error.kind()`, `error.path()` and `error.line()`.

**Before:**

```rust
if err.message.contains("no Assets folder") { /* ... */ }
```

**After:**

```rust
# let err: ponko_unity::Error = todo!();
if err.kind() == ponko_unity::ErrorKind::Invalid { /* ... */ }
```

## UI, text and input

### `ScenarioCheck` is `#[non_exhaustive]` and has a new `Ui` variant

Code outside the engine can no longer match it exhaustively.

**Instead:** add a `_` arm to a `match` on it, and build checks with `ScenarioCheck::action`, `resource`, `count` and `ui`.

**Before:**

```rust
match check {
    ScenarioCheck::Action { name, .. } => println!("action {name}"),
    ScenarioCheck::Resource { name, .. } => println!("resource {name}"),
    ScenarioCheck::Count { component, .. } => println!("count {component}"),
}
```

**After:**

```rust
use ponko::scenario::ScenarioCheck;
# let check: ScenarioCheck = todo!();
match check {
    ScenarioCheck::Action { name, .. } => println!("action {name}"),
    ScenarioCheck::Resource { name, .. } => println!("resource {name}"),
    ScenarioCheck::Count { component, .. } => println!("count {component}"),
    ScenarioCheck::Ui { node, .. } => println!("ui {node}"),
    _ => {}
}
```

### `UiInput` has new fields

`preedit`, `preedit_cursor` and `composing` for IME composition, and `dt`, `secondary`, `menu` and `captured` for the widgets. The engine fills them itself when it runs the UI. Code that builds a `UiInput` with every field named no longer compiles; code that ends with `..Default::default()` is unaffected.

**Instead:** end the struct literal with `..Default::default()`, or set the new fields (an empty `preedit`, `None` and `false` mean no composition, `captured: None` no key-binding field waiting for a key).

**Before:**

```rust
let input = UiInput { pointer, pressed, held, released, wheel, next, previous, direction,
    confirm, cancel, shift, select_all, backspace, delete, home, end, text };
```

**After:**

```rust
use ponko::ui::UiInput;
# let UiInput { pointer, pressed, held, released, wheel, next, previous, direction,
#     confirm, cancel, shift, select_all, backspace, delete, home, end, text, .. } = UiInput::default();
let input = UiInput { pointer, pressed, held, released, wheel, next, previous, direction,
    confirm, cancel, shift, select_all, backspace, delete, home, end, text,
    ..Default::default() };
```

### `GamepadCapabilities` has four new fields

`light`, `player_leds`, `touchpad` and `adaptive_triggers`. A custom gamepad backend that builds it with a struct literal naming every field no longer compiles; reading the fields and `Default` are unchanged. The C `PonkoGamepadInfo` layout is unchanged: C and C++ read the new flags through `ponko_ctx_gamepad_extras` or `ctx.gamepad_extras`.

**Instead:** name the capabilities the backend provides and end the literal with `..Default::default()`.

**Before:**

```rust
let caps = GamepadCapabilities { rumble: true, trigger_rumble: false, motion: false };
```

**After:**

```rust
use ponko::input::GamepadCapabilities;
let caps = GamepadCapabilities { rumble: true, ..Default::default() };
```

### `ButtonBinding`, `AxisBinding`, `Axis2Binding`, `Action` and `InputEvent` are `#[non_exhaustive]`

A `match` on one needs a `_` arm.

**Instead:** handle unknown variants with a `_` arm.

**Before:**

```rust
match action { Action::Button(_) => 0, Action::Axis(_) => 1, Action::Axis2(_) => 2 }
```

**After:**

```rust
use ponko::actions::Action;
# let action: Action = todo!();
match action { Action::Button(_) => 0, Action::Axis(_) => 1, Action::Axis2(_) => 2, _ => 3 }
```

### `ActionMap::from_json` and `to_json` return the engine's `ponko::error::Error`

Decode messages keep the location and the action map's context; the serde traits and the JSON format are unchanged.

**Instead:** use `ponko::error::Error` and classify with `ErrorKind::Json`.

**Before:**

```rust
fn load(s: &str) -> Result<ActionMap, serde_json::Error> { ActionMap::from_json(s) }
```

**After:**

```rust
use ponko::actions::ActionMap;
fn load(s: &str) -> Result<ActionMap, ponko::error::Error> { ActionMap::from_json(s) }
```

### `UiRole`, `UiAccessValue` and `UiAccessAction` are `#[non_exhaustive]`

A `match` on one needs a `_` arm. New roles, values and assistive-technology requests can then come in minor releases.

**Instead:** handle roles, values and requests added later with a `_` arm.

**Before:**

```rust
let polite = match role { UiRole::Button | UiRole::CheckBox | UiRole::Slider | UiRole::TextInput => true, UiRole::Group | UiRole::Dialog | UiRole::Label | UiRole::Image | UiRole::ScrollView => false };
```

**After:**

```rust
use ponko::ui::UiRole;
# let role = UiRole::Button;
let polite = matches!(role, UiRole::Button | UiRole::CheckBox | UiRole::Slider | UiRole::TextInput);
```

### `UiTheme` has new fields

`spacing`, `radius` and `tooltip_delay` for the widgets, and `transition` and `press_scale` for animated controls. Every built-in theme has a radius of 0, no transition and a press scale of 1, so it draws exactly as before.

**Instead:** build themes with `..UiTheme::default()`, `UiTheme::dark()` or `UiTheme::light()` after the fields you set.

**Before:**

```rust
let theme = UiTheme { panel, control, hover, pressed, disabled, accent, text, title, font,
    text_size: 18.0, title_size: 28.0, border_width: 1.0 };
```

**After:**

```rust
use ponko::ui::UiTheme;
# let UiTheme { panel, control, hover, pressed, disabled, accent, text, title, font, .. } = UiTheme::default();
let theme = UiTheme { panel, control, hover, pressed, disabled, accent, text, title, font,
    text_size: 18.0, title_size: 28.0, border_width: 1.0, ..UiTheme::default() };
```

### `UiAccessNode` has new fields: `selected`, `expanded` and `position`

A struct literal or a pattern that names every field no longer compiles.

**Instead:** end the pattern with `..` (or name the new fields).

**Before:**

```rust
let UiAccessNode { id, role, name, value, rect, /* ... every other field */ } = node;
```

**After:**

```rust
use ponko::ui::UiAccessNode;
# let node: UiAccessNode = todo!();
let UiAccessNode { id, role, name, value, rect, .. } = node;
```

## Audio

### `ponko_ffi::PonkoBusEffectKind` has a new value, `PONKO_BUS_EFFECT_PITCH_SHIFT`

So a Rust `match` over every value no longer compiles. C and C++ code is not affected.

**Instead:** add an arm for the new value, or a `_` arm.

**Before:**

```rust
match effect.kind {
    PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => "filter",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => "reverb",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => "compressor",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => "delay",
}
```

**After:**

```rust
use ponko_ffi::PonkoBusEffectKind;
# let effect: ponko_ffi::PonkoBusEffect = todo!();
match effect.kind {
    PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => "filter",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => "reverb",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => "compressor",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => "delay",
    PonkoBusEffectKind::PONKO_BUS_EFFECT_PITCH_SHIFT => "pitch shift",
}
```

## Physics, navigation and AI

### `d3::NavObstacle` has `yaw` and `shape` fields and is `#[non_exhaustive]`

Obstacles can be turned boxes, cylinders and convex prisms now, so struct literals no longer build one.

**Instead:** build it with `NavObstacle::new`, `NavObstacle::cylinder` or `NavObstacle::convex`, then set the fields you need.

**Before:**

```rust
let obstacle = NavObstacle { center, half_extents, carve: true };
```

**After:**

```rust
use ponko::d3::NavObstacle;
# let (center, half_extents) = (Vec3::ZERO, Vec3::ONE);
let mut obstacle = NavObstacle::new(center, half_extents);
obstacle.carve = true;
```

### Waves from a spectrum are drawn on the simulation's clock, where floating bodies ride them, instead of the frame's

The clock is `sim_time()` plus `fixed_alpha()` of a fixed step, not `ctx.time.elapsed()`. Nothing changes until a game pauses or changes the simulation's speed; after that the sea stands still while paused and runs at the simulation's speed, and the drawn waves are the ones floating bodies ride. Waves from the wave table stay on the frame's clock.

**Instead:** ask `height_at` or `sample` at `water.drawn_time(&ctx.time)` for what is drawn; a shader that drew something in step with a spectrum's waves from `frame.time.x` (still the frame's clock) takes the drawn time in a material value instead.

**Before:**

```rust
let h = water.height_at(xz, ctx.time.elapsed() as f32);
```

**After:**

```rust
# let (water, xz): (ponko::water::Water, Vec2) = todo!();
let h = water.height_at(xz, water.drawn_time(&ctx.time));
```

## Networking

### A networked event type that leaves fields out when serializing no longer arrives

Events travel with their fields by position, so the receiver can't tell which field `#[serde(skip_serializing_if = ...)]` left out, and drops the event.

**Instead:** remove the attribute and send the field as it is (an `Option` travels as `None`).

**Before:**

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Hit {
    target: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    critical: Option<f32>,
}
```

**After:**

```rust
#[derive(serde::Serialize, serde::Deserialize)]
struct Hit {
    target: u32,
    critical: Option<f32>,
}
```

### The message protocol is version 2: hosts and clients of 0.1 and 0.2 can't play together

Packets start with `CRM2`, the Steam lobby's `ponko_protocol` is `ponko-messages-2`, and connecting takes one extra round trip on UDP. A version-1 client joining a new host, or a new client joining a version-1 host, times out ("peer timed out"); Steam joins fail with "incompatible message protocol". From version 2 on, a host answers a hello of another version with a refusal the client reports as `Disconnected` with a reason starting "version mismatch".

**Instead:** ship the host (a dedicated server too) and every client built with the same engine version.

**Before:**

```toml
[dependencies]
ponko = "0.1"
```

**After:**

```toml
[dependencies]
ponko = "0.2"
```

## C and C++

### C: `ponko_game_run` returns `PONKO_UNSUPPORTED` (was `PONKO_INVALID_ARGUMENT`) when the game can't start on this machine

That is: no usable GPU or graphics device, no window or event loop, or a library built without the `window` feature. Other statuses are unchanged, and C++'s `game::run` still throws a `ponko::error` of kind `PONKO_ERROR_UNSUPPORTED`.

**Instead:** code that checks `!= PONKO_OK` needs nothing; code that compared the result with `PONKO_INVALID_ARGUMENT` to detect a missing GPU compares it with `PONKO_UNSUPPORTED`.

**Before:**

```c
if (ponko_game_run(game, setup, NULL) == PONKO_INVALID_ARGUMENT)
    fprintf(stderr, "this game needs a GPU\n");
```

**After:**

```c
if (ponko_game_run(game, setup, NULL) == PONKO_UNSUPPORTED)
    fprintf(stderr, "this game needs a GPU\n");
```

## Development and releases

### `ScopeReport` has a new field, `allocs_per_frame`

A struct literal of it no longer compiles; reports read from JSON written before it get 0.

**Instead:** end the literal with `..Default::default()`.

**Before:**

```rust
let s = ScopeReport { name, avg_ms, max_ms, calls_per_frame };
```

**After:**

```rust
use ponko::perf::ScopeReport;
# let (name, avg_ms, max_ms, calls_per_frame) = todo!();
let s = ScopeReport { name, avg_ms, max_ms, calls_per_frame, ..Default::default() };
```

<!-- changelog.py: entries end -->
