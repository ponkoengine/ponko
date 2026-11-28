# Examples

Every example in this folder, by area. Run one from the repository root:

```sh
cargo run --release --example boids
cargo run --release --example terrain --features 3d
```

The **Features** column names the Cargo features an example needs; Cargo
refuses to build it without them (they are its `required-features` in
`Cargo.toml`). A dash means the default features are enough. Use
`--release` for anything with many entities: debug builds are much slower.
Most examples also build for the web with [`web/build.sh`](../web/build.sh)
(see the guide's [Web builds](../book/src/web.md)).

## 2D and input

| Example | What it shows | Features |
|---|---|---|
| [window](window.rs) | The smallest game: a window, the game loop and input | – |
| [move_sprite](move_sprite/) | One sprite sliding left and right, ported from Bevy's `move_sprite` ([Coming from Bevy](../book/src/from-bevy.md)) | – |
| [sprites](sprites.rs) | Sprites: position, rotation, scale, tint, draw order and pivot | – |
| [sprite_effects](sprite_effects.rs) | Blend modes and sprite shaders: a glow, an outline and a dissolve | – |
| [particles2d](particles2d.rs) | 2D particles: a campfire, sparks, a trail behind the mouse | – |
| [lights2d](lights2d.rs) | 2D lights with shadows and normal-mapped sprites in a dungeon | – |
| [tiles](tiles.rs) | A large tilemap with animated sprites on it | – |
| [level](level.rs) | A level from the Tiled map editor, with tile colliders | `physics` |
| [skeleton2d](skeleton2d.rs) | 2D skeletal animation: blending, a skinned tail and two-bone IK | – |
| [prefabs](prefabs/) | Prefabs with children, from one file that includes another | – |
| [bunnymark](bunnymark.rs) / [bunnymark_ecs](bunnymark_ecs.rs) | Sprite throughput, drawn directly and from the ECS | – |
| [action_map](action_map.rs) | Named actions shared by keyboard and gamepad, without a window | – |
| [gamepad](gamepad.rs) | Every connected controller moves a crab of its own | – |
| [phone](phone.rs) | What the engine reads from a phone or tablet (used by the iOS and Android chapters) | – |

## 3D

| Example | What it shows | Features |
|---|---|---|
| [3d](3d.rs) | Meshes, materials, a sun and thousands of instanced models | `3d` |
| [lights](lights.rs) | Many coloured lights, a quarter of them spotlights | `3d` |
| [bake_lighting](bake_lighting.rs) | Lighting baked into lightmaps and a probe volume | `3d` |
| [shaders](shaders.rs) | Custom material shaders: waves, grass in the wind, a pattern | `3d` |
| [compute](compute.rs) | A compute pass: a wave simulation that the water shader reads | `3d` |
| [scene_compute](scene_compute.rs) | A compute pass over the lit frame: eye adaptation | `3d` |
| [target_effect](target_effect.rs) | A target effect: a cheap depth of field | `3d` |
| [stencil](stencil.rs) | Stencil state on materials: glass that blends once, an outline | `3d` |
| [post_volumes](post_volumes.rs) | Post-processing that blends between volumes as the camera moves | `3d` |
| [skinning](skinning.rs) | Procedural tentacles: skinned meshes built in code | `3d` |
| [crowd](crowd.rs) | 10,000 animated people in one crowd | `3d` |
| [particles](particles.rs) | CPU billboard particles | `3d` |
| [ribbons](ribbons.rs) | Ribbon trails, lines and decals | `3d` |
| [stylized_sky](stylized_sky.rs) | A painted sky shader through a day | `3d` |
| [sky_kit](sky_kit/) | The ready-made sky: clouds, their shadows, a moon and stars | `3d` |
| [reflection_refresh](reflection_refresh.rs) | Reflection probes that refresh themselves as the sun moves | `3d` |
| [terrain](terrain/) | Heightfield terrain with blended ground and a lake | `3d` |
| [streaming](streaming.rs) | An endless landscape streamed in around the camera | `3d` |
| [scene_viewer](scene_viewer.rs) | Looks around a saved 3D scene, such as one the Unity importer writes | `3d` |
| [split_screen](split_screen.rs) | Two players, two cameras and a minimap | `3d` |

## Physics and navigation

| Example | What it shows | Features |
|---|---|---|
| [physics](physics.rs) | 2D bodies that fall, stack, bounce and slide | `physics` |
| [physics3d](physics3d.rs) | A first-person walk over hilly terrain, pushing crates | `physics3d` |
| [vehicle](vehicle.rs) | A box car on raycast wheels | `physics3d` |
| [water](water/) | A lake, a river and a sea, with crates floating on them | `physics3d` |
| [navmesh3d](navmesh3d.rs) | Paths on a 3D navigation mesh, without a window | `navmesh` |

## UI

| Example | What it shows | Features |
|---|---|---|
| [ui](ui.rs) | Camera, shapes, text and UI in one small scene | – |
| [ui_widgets](ui_widgets.rs) | The retained UI's widgets: a settings page and an inventory | – |
| [ui_menu](ui_menu.rs) | A settings menu a screen reader can read (add `accessibility` for the screen reader) | – |
| [ui_shop](ui_shop.rs) | A shop: item grid, scrolling, input and a confirmation dialog | – |
| [ui_panel](ui_panel.rs) | Retained UI on a screen in the 3D world | `3d` |

## Audio and video

| Example | What it shows | Features |
|---|---|---|
| [audio_mixer](audio_mixer.rs) | A mixer from a data file: ducking and snapshots | – |
| [audio3d](audio3d.rs) | Positional sound | `3d` |
| [video_player](video_player.rs) | A WebM video drawn to a texture, with its sound (add `video` to play one) | – |

## Simulation at scale

| Example | What it shows | Features |
|---|---|---|
| [headless](headless.rs) | A million agents, no window and no GPU; prints ticks per second | – |
| [ecs](ecs.rs) | Components, simulation systems, a resource and commands | – |
| [boids](boids.rs) | Flocking with a spatial grid on all cores | – |
| [ants](ants.rs) | Ant colonies with pheromone trails, up to millions of ants | – |
| [flow](flow.rs) | Crowds following one shared flow field | – |
| [battle](battle.rs) | Two armies on flow fields, fighting over a map | – |
| [legions](legions/) | 100,000 units whose battles replay bit for bit ([the guide](../book/src/legions.md)) | – |
| [ai_guards](ai_guards/) | Guards driven by behaviour trees from a data file | – |

## Networking

| Example | What it shows | Features |
|---|---|---|
| [net_messages](net_messages.rs) | Byte messages between two games | – |
| [net_replication](net_replication.rs) | Snapshot interpolation, authority and interest by distance | – |
| [net_prediction](net_prediction.rs) | Client prediction and reconciliation over a bad link | – |
| [lockstep](lockstep.rs) | Lockstep multiplayer: every window runs the same simulation | – |
| [net_arena](net_arena/) | Orb Arena, the multiplayer reference game: a server, clients and bots | – |
| [net_webrtc](net_webrtc.rs) | Byte messages peer to peer over WebRTC | `webrtc` |

## Tools

| Example | What it shows | Features |
|---|---|---|
| [editor](editor.rs) | A playground for the inspector and the editor (`ponko editor --example editor` opens the editor app) | – |
| [hotpatch](hotpatch.rs) | Changing a system while the game runs, with `dx serve --hot-patch` and the `hotpatch` feature | – |
| [validation_bundle](validation_bundle/) | Writes a validation bundle: a build, a scenario, what was observed and file hashes | – |

## Complete games and tutorials

| Example | What it shows | Features |
|---|---|---|
| [coin_dash](coin_dash/) | Coin Dash: a small complete game, tested headless and packaged ([the guide](../book/src/packaging.md)); add `text-shaping` for its Arabic text | – |
| [ember_hop](ember_hop/) | Ember Hop: a 2D platformer with Tiled levels, physics and saves ([the guide](../book/src/ember-hop.md)) | `physics` |
| [shard_grove](shard_grove/) | Shard Grove: a third-person 3D game with a navigation mesh ([the guide](../book/src/shard-grove.md)) | `navmesh`, `physics3d` |
| [tutorial_2d](tutorial_2d/) | The game [Tutorial: a first 2D game](../book/src/tutorial-2d.md) builds | `physics` |
| [tutorial_3d](tutorial_3d/) | The game [Tutorial: a first 3D game](../book/src/tutorial-3d.md) builds | `physics3d` |

## Browser test fixtures

Small programs the browser checks in [`web/smoke/`](../web/smoke/README.md)
build and drive; each file's first comment says how to build it.

| Example | What it checks | Features |
|---|---|---|
| [web_smoke](web_smoke.rs) / [web_smoke_3d](web_smoke_3d.rs) | That a 2D or 3D web build starts and draws | – / `3d` |
| [web_spectrum](web_spectrum.rs) | The sea's wave spectrum in the browser, as a benchmark | `3d` |
| [web_threads](web_threads.rs) / [web_threads_warning](web_threads_warning.rs) | Web builds with threads, and their warning self-test | – |
| [web_video](web_video.rs) | Video in the browser | `video` |
| [web_editor_save](web_editor_save.rs) / [web_editor_unloaded](web_editor_unloaded.rs) | Saving from the editor in a browser, and its panel of scenes that didn't load | `editor` / `editor`, `3d` |

## C and C++

[`cpp/`](cpp/) holds the programs for the C API and the C++17 wrapper:
[`main.c`](cpp/main.c) and [`main.cpp`](cpp/main.cpp), [`boids.cpp`](cpp/boids.cpp)
and [`battle.cpp`](cpp/battle.cpp) (the Rust examples of the same names),
[Coin Dash in C++](cpp/coin_dash/), the guide's and the cookbook's C++ code
([`guide.cpp`](cpp/guide.cpp), [`cookbook.cpp`](cpp/cookbook.cpp)) and the
tests. [`ffi_bench.rs`](ffi_bench.rs) times one million position updates in
Rust (release builds only), a reference for the C++ benchmark
[`cpp/bench.cpp`](cpp/bench.cpp). To build and run them (CMake, a C11 and a
C++17 compiler):

```sh
cargo build -p ponko_ffi --release
cmake -S examples/cpp -B target/native -DCMAKE_BUILD_TYPE=Release -DPONKO_LIBRARY_DIR="$PWD/target/release"
cmake --build target/native --config Release
./target/native/ponko_cpp_boids
```

[C and C++](../book/src/c-cpp.md) in the guide has the rest.

The folders [`common/`](common/) and [`support/`](support/) hold code that
several examples share; they are not examples of their own.
