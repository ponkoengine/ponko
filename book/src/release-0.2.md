# What's new in 0.2

<!-- release story: written by hand, copied to CHANGELOG.md by the fold -->
Ponko 0.2 grows the engine from "a fast 2D and 3D core" into one you can
ship a whole game with. It adds the tools around the code (an editor, an
asset cooker, packaging, crash reports), the systems most games need
(hierarchies, game states, a widget set, localization, saves that migrate),
and multiplayer from rollback to dedicated servers, while the simple path
stays `Game::new()`, systems and `.run()`. Where it breaks the API, the
[migration guide](migrating-to-0.2.md) lists each change with the code
before and after.
The [CHANGELOG](../../CHANGELOG.md) has every change, grouped by area.

**Highlights**

- **Building scenes**: the [editor](editor.md) (`ponko editor`) edits 2D and
  3D scenes and prefabs, with undo, an asset panel and glTF scenes;
  `ponko cook` turns source assets into runtime files; levels load from
  Tiled and LDtk ([tile maps](tiles.md)); prefabs hold children, and
  [saves](storage.md) migrate between versions of a game and can be binary.
- **ECS and game structure**: an [entity hierarchy](hierarchy.md),
  [game states](states.md), component hooks and required components,
  [plugins](plugins.md) for other people's crates, and `Commands::spawn`
  returning the entity at once.
- **Rendering**: 2D lights and particles, sprite flips and blend modes,
  [several cameras in one window](viewports.md), camera rigs, a sky kit with
  clouds, a [water kit](water.md), baked and probe lighting, temporal
  upscaling, auto exposure, [HDR output](hdr.md), reverse-Z depth and GPU
  timing and memory in the performance report.
- **Animation and crowds**: [2D skeletons](skeleton2d.md), humanoid
  retargeting, and [crowds](crowds.md) of thousands of animated characters
  in a few draws.
- **UI and input**: a [widget set](ui-widgets.md) and
  [UI animation](ui-animation.md), text shaping, IME, rich text,
  [localization](localization.md), screen readers on Windows and Linux,
  [touch controls](touch.md), [controllers](controllers.md) with gyro,
  rumble and joysticks, and a window service for display modes.
- **Audio**: a mixer of nested buses with effects and snapshots, loop
  points and a voice limit ([audio](audio.md)).
- **Game AI**: behaviour trees and brains in the [AI kit](ai.md), and 3D
  navigation obstacles of any shape.
- **Multiplayer**: [rollback](multiplayer.md), declared
  [replication](replication.md), client prediction,
  [dedicated servers](servers.md), a reference [lobby server](lobby.md)
  with relay, [WebRTC](webrtc.md), Steam Networking Sockets and
  [Steam services](steam.md).
- **Platforms**: [iOS](ios.md), [threads on the web](web-threads.md),
  WebGPU first in the browser, [video](video.md), C and C++ games built
  for the web, and C library downloads for Intel Macs, Arm Linux and Arm
  Windows.
- **Shipping and support**: `ponko new` and `ponko package`
  ([packaging](packaging.md)), [crash reports](crash-reports.md) with
  native backtraces, [mods](mods.md), [debug drawing](debug-draw.md), a
  developer console, and error messages that name the fix
  ([errors](errors.md)).
- **Learning it**: two tutorials ([2D](tutorial-2d.md), [3D](tutorial-3d.md)),
  a [cookbook](cookbook/index.md) of fifty recipes,
  [Coming from Bevy](from-bevy.md), [maturity levels](maturity.md) on every
  chapter, and three reference games: [Ember Hop](ember-hop.md),
  [Shard Grove](shard-grove.md) and [Legions](legions.md).
<!-- release story end -->

<!-- changelog.py --notes: generated from changelog.d/, do not edit by hand -->

## Every change, by area

The [CHANGELOG](../../CHANGELOG.md) lists each of these 1322 changes under its area; the numbers count its entries.

| Area | Breaking | Added | Changed | Fixed | Removed | Deprecated | Security |
|---|---|---|---|---|---|---|---|
| API and crate layout | 15 | 28 | 15 | 2 |  | 6 |  |
| ECS and systems | 8 | 39 | 32 | 8 |  |  |  |
| Rendering | 5 | 127 | 66 | 45 |  |  |  |
| Animation | 9 | 35 | 11 | 2 |  |  |  |
| Assets, scenes and saves | 5 | 113 | 43 | 13 |  |  |  |
| UI, text and input | 8 | 94 | 28 | 8 |  | 2 |  |
| Audio | 1 | 24 | 4 | 5 |  |  |  |
| Physics, navigation and AI | 2 | 46 | 15 | 8 |  |  |  |
| Networking | 2 | 60 | 9 | 3 |  |  | 3 |
| Web, mobile and platforms |  | 49 | 10 | 13 |  |  |  |
| Tools: editor, inspector and agents |  | 58 | 12 | 12 |  |  |  |
| C and C++ | 1 | 41 | 5 | 2 |  |  |  |
| Guide and examples |  | 24 | 22 | 8 |  |  |  |
| Development and releases | 1 | 51 | 24 | 33 | 2 |  | 5 |
| **All** | **57** | **789** | **296** | **162** | **2** | **8** | **8** |

## Breaking changes at a glance

Each links to its entry, with the code before and after, in [Migrating from 0.1 to 0.2](migrating-to-0.2.md).

**API and crate layout**

- [`ponko_ffi::PonkoHandleKind` has three new values, `PONKO_HANDLE_SPRITE_SHADER`, `PONKO_HANDLE_SPRITE_MATERIAL` and `PONKO_HANDLE_RENDER_TARGET`](migrating-to-0.2.md#ponko_ffiponkohandlekind-has-three-new-values-ponko_handle_sprite_shader-ponko_handle_sprite_material-and-ponko_handle_render_target)
- [The `ponko::physics` module is gone: 2D physics lives in `ponko::d2`](migrating-to-0.2.md#the-ponkophysics-module-is-gone-2d-physics-lives-in-ponkod2)
- [The `ponko::physics3d` module is gone](migrating-to-0.2.md#the-ponkophysics3d-module-is-gone)
- [`ponko::d3` no longer re-exports the types of topic modules](migrating-to-0.2.md#ponkod3-no-longer-re-exports-the-types-of-topic-modules)
- [`ponko::d2` no longer re-exports the types of topic modules](migrating-to-0.2.md#ponkod2-no-longer-re-exports-the-types-of-topic-modules)
- [Types of one dimension are no longer also public in a topic module under their suffixed name](migrating-to-0.2.md#types-of-one-dimension-are-no-longer-also-public-in-a-topic-module-under-their-suffixed-name)
- [`ponko::nav` holds only the navigation-mesh baking types (`NavBakeSettings`, `NavBakeError`, `NavSource`)](migrating-to-0.2.md#ponkonav-holds-only-the-navigation-mesh-baking-types-navbakesettings-navbakeerror-navsource)
- [`ponko::alloc_count` and `ponko::scope` are private](migrating-to-0.2.md#ponkoalloc_count-and-ponkoscope-are-private)
- [The built-in systems are public only in `ponko::systems`](migrating-to-0.2.md#the-built-in-systems-are-public-only-in-ponkosystems)
- [The Steam API takes and returns Ponko's own types](migrating-to-0.2.md#the-steam-api-takes-and-returns-ponkos-own-types)
- [`ponko::net::steam`, the re-exported `steamworks` crate, is `ponko::net::unstable_steamworks` now](migrating-to-0.2.md#ponkonetsteam-the-re-exported-steamworks-crate-is-ponkonetunstable_steamworks-now)
- [The prelude no longer exports `AssetSource`, `MemorySource`, `PackSource` and `SetupResult`](migrating-to-0.2.md#the-prelude-no-longer-exports-assetsource-memorysource-packsource-and-setupresult)
- [`ponko_image_diff::compare`, `heatmap` and `compare_files` return `ponko_image_diff::Error`](migrating-to-0.2.md#ponko_image_diffcompare-heatmap-and-compare_files-return-ponko_image_differror)
- [Render targets have their own handle type, `RenderTarget`, instead of `Texture`](migrating-to-0.2.md#render-targets-have-their-own-handle-type-rendertarget-instead-of-texture)
- [C++: render targets are `ponko::render_target`, and the calls that draw into one take only that](migrating-to-0.2.md#c-render-targets-are-ponkorender_target-and-the-calls-that-draw-into-one-take-only-that)

**ECS and systems**

- [An index into `RawWorldRef::archetypes` is valid only until the next change](migrating-to-0.2.md#an-index-into-rawworldrefarchetypes-is-valid-only-until-the-next-change)
- [`schedule::Stage::systems` returns `&[usize]` instead of a `Range<usize>`](migrating-to-0.2.md#schedulestagesystems-returns-usize-instead-of-a-rangeusize)
- [`d2::Collider` has a `groups` field (its collision groups)](migrating-to-0.2.md#d2collider-has-a-groups-field-its-collision-groups)
- [`d2::QueryFilter` has a `groups` field, as `d3::QueryFilter` does](migrating-to-0.2.md#d2queryfilter-has-a-groups-field-as-d3queryfilter-does)
- [`Commands::spawn` returns the new `Entity` instead of `()`](migrating-to-0.2.md#commandsspawn-returns-the-new-entity-instead-of-)
- [`Commands::spawn` on a buffer that isn't tied to a world panics](migrating-to-0.2.md#commandsspawn-on-a-buffer-that-isnt-tied-to-a-world-panics)
- [`systems::follow_attachments` places only the subtrees whose root moved or whose attachments changed](migrating-to-0.2.md#systemsfollow_attachments-places-only-the-subtrees-whose-root-moved-or-whose-attachments-changed)
- [Adding a plugin with the same name twice makes the game fail to start](migrating-to-0.2.md#adding-a-plugin-with-the-same-name-twice-makes-the-game-fail-to-start)

**Rendering**

- [`PerfReport` has a new field, `gpu_memory`, and `Budget` a new field, `gpu_memory_mb`](migrating-to-0.2.md#perfreport-has-a-new-field-gpu_memory-and-budget-a-new-field-gpu_memory_mb)
- [`Sprite` has two new public fields, `flip_x` and `flip_y`](migrating-to-0.2.md#sprite-has-two-new-public-fields-flip_x-and-flip_y)
- [The 3D scene depth is reverse-Z: 1 at the camera's near plane, 0 at the far plane and the sky](migrating-to-0.2.md#the-3d-scene-depth-is-reverse-z-1-at-the-cameras-near-plane-0-at-the-far-plane-and-the-sky)
- [`TargetCamera` is `#[non_exhaustive]` and no longer `Copy`](migrating-to-0.2.md#targetcamera-is-non_exhaustive-and-no-longer-copy)
- [Material normal maps no longer lean surfaces the opposite way on both axes](migrating-to-0.2.md#material-normal-maps-no-longer-lean-surfaces-the-opposite-way-on-both-axes)

**Animation**

- [`ponko::anim::AnimationState` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimanimationstate-is-non_exhaustive)
- [`ponko::anim::MotionChild` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimmotionchild-is-non_exhaustive)
- [`ponko::anim::AnimationEvent` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimanimationevent-is-non_exhaustive)
- [`ponko::anim::Joint` is `#[non_exhaustive]`: struct literals outside the engine no longer compile](migrating-to-0.2.md#ponkoanimjoint-is-non_exhaustive-struct-literals-outside-the-engine-no-longer-compile)
- [`ponko::anim::JointAnimation` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimjointanimation-is-non_exhaustive)
- [`ponko::anim::MorphAnimation` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimmorphanimation-is-non_exhaustive)
- [`ponko::anim::MorphTarget` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimmorphtarget-is-non_exhaustive)
- [`ponko::anim::AttachToJoint` is `#[non_exhaustive]`](migrating-to-0.2.md#ponkoanimattachtojoint-is-non_exhaustive)
- [The `Motion::Blend1D` and `Motion::Blend2D` variants are `#[non_exhaustive]`](migrating-to-0.2.md#the-motionblend1d-and-motionblend2d-variants-are-non_exhaustive)

**Assets, scenes and saves**

- [`TileMap` is `#[non_exhaustive]` and has new fields (`parallax`, `parallax_origin`, `animations`, `flips`)](migrating-to-0.2.md#tilemap-is-non_exhaustive-and-has-new-fields-parallax-parallax_origin-animations-flips)
- [The plain collision-mesh loaders panic instead of returning a `Result`](migrating-to-0.2.md#the-plain-collision-mesh-loaders-panic-instead-of-returning-a-result)
- [`Assets::load_strings` panics instead of returning a `Result`](migrating-to-0.2.md#assetsload_strings-panics-instead-of-returning-a-result)
- [`Assets::load_resource_json` and `load_component_json` are `load_reflected_resource_json` and `load_reflected_component_json`](migrating-to-0.2.md#assetsload_resource_json-and-load_component_json-are-load_reflected_resource_json-and-load_reflected_component_json)
- [`ponko_unity::Error`'s public `message` field is the `message()` method now](migrating-to-0.2.md#ponko_unityerrors-public-message-field-is-the-message-method-now)

**UI, text and input**

- [`ScenarioCheck` is `#[non_exhaustive]` and has a new `Ui` variant](migrating-to-0.2.md#scenariocheck-is-non_exhaustive-and-has-a-new-ui-variant)
- [`UiInput` has new fields](migrating-to-0.2.md#uiinput-has-new-fields)
- [`GamepadCapabilities` has four new fields](migrating-to-0.2.md#gamepadcapabilities-has-four-new-fields)
- [`ButtonBinding`, `AxisBinding`, `Axis2Binding`, `Action` and `InputEvent` are `#[non_exhaustive]`](migrating-to-0.2.md#buttonbinding-axisbinding-axis2binding-action-and-inputevent-are-non_exhaustive)
- [`ActionMap::from_json` and `to_json` return the engine's `ponko::error::Error`](migrating-to-0.2.md#actionmapfrom_json-and-to_json-return-the-engines-ponkoerrorerror)
- [`UiRole`, `UiAccessValue` and `UiAccessAction` are `#[non_exhaustive]`](migrating-to-0.2.md#uirole-uiaccessvalue-and-uiaccessaction-are-non_exhaustive)
- [`UiTheme` has new fields](migrating-to-0.2.md#uitheme-has-new-fields)
- [`UiAccessNode` has new fields: `selected`, `expanded` and `position`](migrating-to-0.2.md#uiaccessnode-has-new-fields-selected-expanded-and-position)

**Audio**

- [`ponko_ffi::PonkoBusEffectKind` has a new value, `PONKO_BUS_EFFECT_PITCH_SHIFT`](migrating-to-0.2.md#ponko_ffiponkobuseffectkind-has-a-new-value-ponko_bus_effect_pitch_shift)

**Physics, navigation and AI**

- [`d3::NavObstacle` has `yaw` and `shape` fields and is `#[non_exhaustive]`](migrating-to-0.2.md#d3navobstacle-has-yaw-and-shape-fields-and-is-non_exhaustive)
- [Waves from a spectrum are drawn on the simulation's clock, where floating bodies ride them, instead of the frame's](migrating-to-0.2.md#waves-from-a-spectrum-are-drawn-on-the-simulations-clock-where-floating-bodies-ride-them-instead-of-the-frames)

**Networking**

- [A networked event type that leaves fields out when serializing no longer arrives](migrating-to-0.2.md#a-networked-event-type-that-leaves-fields-out-when-serializing-no-longer-arrives)
- [The message protocol is version 2: hosts and clients of 0.1 and 0.2 can't play together](migrating-to-0.2.md#the-message-protocol-is-version-2-hosts-and-clients-of-01-and-02-cant-play-together)

**C and C++**

- [C: `ponko_game_run` returns `PONKO_UNSUPPORTED` (was `PONKO_INVALID_ARGUMENT`) when the game can't start on this machine](migrating-to-0.2.md#c-ponko_game_run-returns-ponko_unsupported-was-ponko_invalid_argument-when-the-game-cant-start-on-this-machine)

**Development and releases**

- [`ScopeReport` has a new field, `allocs_per_frame`](migrating-to-0.2.md#scopereport-has-a-new-field-allocs_per_frame)

<!-- changelog.py --notes: end -->
