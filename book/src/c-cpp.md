# C and C++

Games can be written in C++ (or plain C) as well as Rust. They use the same
engine: the same ECS, game loop, renderer, spatial grid and control endpoint
for agents. Wherever a feature exists in the C API, the chapters of this guide
show the C++ version of its examples next to the Rust one; where it doesn't,
the chapter says so.

- `include/ponko.hpp`: the C++17 wrapper, header-only. `ponko::game`,
  `ponko::ctx` and `ponko::world_view` mirror Rust's `Game`, `Ctx` and
  `World`.
- `include/ponko.h`: the C API underneath, generated from the `ponko_ffi`
  crate with cbindgen.
- One shared library, `ponko_ffi` (`libponko_ffi.dylib`/`.so`,
  `ponko_ffi.dll`).

Loops over entities run in your C++ code, over **whole component arrays**,
so a million entities never mean a million calls across the language
boundary, and `each_par` runs them on all cores.

## Get the library

**From a release.** Each release, starting with 0.1.0, has an archive
per platform: `ponko-v0.1.0-linux-x86_64.tar.gz`,
`ponko-v0.1.0-macos-arm64.tar.gz` and `ponko-v0.1.0-windows-x86_64.zip`.
Each holds

- `include/`: `ponko.h` and `ponko.hpp`, matching the library next to them;
- `lib/`: `ponko_ffi` as a shared library (`libponko_ffi.so`,
  `libponko_ffi.dylib`, or `ponko_ffi.dll` with its import library
  `ponko_ffi.dll.lib`) and a static library;
- the licenses, `NOTICE`, and `THIRD-PARTY-NOTICES.txt` with the license of
  every library linked into `ponko_ffi`. Ship that file with your game.

Point CMake at it with `-DPONKO_INCLUDE_DIR=<archive>/include
-DPONKO_LIBRARY_DIR=<archive>/lib`, or use any build system: add the include
directory and link the library.

**From source.** `cargo build -p ponko_ffi --release` builds the same library
into `target/release/`; the headers are in `include/`. Build both from the same
commit. The release libraries have `ponko_ffi`'s default features; turn other
optional parts on with `ponko_ffi`'s features (for example
`--features accessibility`, or `--features compressed-textures` for
`.ktx2` files through `ponko_ctx_load_texture`, see
[Compressed textures](drawing.md#compressed-textures)).

## Build and run the examples

From the repository root, with Rust, CMake, a C11 compiler and a C++17 compiler:

```sh
cargo build -p ponko_ffi --release
cmake -S examples/cpp -B target/native -DCMAKE_BUILD_TYPE=Release -DPONKO_LIBRARY_DIR="$PWD/target/release"
cmake --build target/native --config Release
ctest --test-dir target/native -C Release --output-on-failure
./target/native/ponko_cpp_boids                                  # window
HEADLESS=1 BOIDS=1000000 TICKS=300 ./target/native/ponko_cpp_boids
./target/native/ponko_cpp_battle                                 # the battle demo
HEADLESS=1 UNITS=1000000 TICKS=600 ./target/native/ponko_cpp_battle
```

In PowerShell, replace `$PWD` with the repository's absolute path, using
forward slashes; Visual Studio builds use `--config Release` and put programs
in `target/native/Release/`.

For your own game, copy the recipe in `examples/cpp/CMakeLists.txt`: include
the two headers, link `ponko_ffi`, ship its shared library next to your
executable, and put your textures in an `assets/` folder next to it (the
example copies the repository's `assets/`). Rebuild against the matching
header when you update the engine.

### The ABI version

`ponko_abi_version()` returns `2`, and `ponko.h` defines `PONKO_ABI_VERSION` as
`2`: the first is the loaded library, the second the header your program was
compiled against. They differ when a program is built against one release's
headers and runs with another's `ponko_ffi`. `ponko.hpp` compares them the first
time a `ponko::game` or `ponko::world` is made and throws a `ponko::error`
(kind `PONKO_ERROR_UNSUPPORTED`) that names both numbers. In C, or when you load
the library yourself, make the same check at startup:

```c
{{#include ../../examples/cpp/guide.c:abi_check}}
```

Within a version, existing functions and struct layouts never change and new
releases only add functions and enum values. The version stays 2 for 0.1.0: the
old C spellings were removed before any native artifact was released,
and every `ponko_*` function, layout and numeric value is unchanged.

## A C++ game

```cpp
{{#include ../../examples/cpp/guide.cpp:first_game_full}}
```

- Systems are anything callable as `void(ponko::ctx&)`. Lambdas can capture
  your game state by reference: the game runs inside `run()`, so it outlives
  nothing it captures.
- `run(setup)` opens the window; `run_headless(ticks, setup)` runs the
  simulation alone and returns the world (see [Headless runs](headless.md)).
  Startup code can also go on the builder with `setup(fn)`, followed by
  `run()` or `run_headless(ticks)` (see
  [Setup on the builder](getting-started.md#setup-on-the-builder)); in C,
  `ponko_game_setup`.
- Errors become exceptions. An exception thrown by your system stops the game
  and comes out of `run()`; it never crosses into the engine.

## Components

C++ components are plain structs: standard-layout, trivially copyable and
trivially destructible. No `std::string`, `std::vector` or owning pointers in
components; store ids or indices and keep other objects outside the ECS.

```cpp
{{#include ../../examples/cpp/main.cpp:cpp_components}}
```

There's nothing to register: calls find a component by its C++ type, and
the engine learns about the type the first time it sees one. It's named
after the type, namespaces included (`game::Health`), which is how tools and
saves show it; `world.reflect<T>("Name", fields)` picks another name.

The built-ins are C++ types too: `ponko::transform2d` is `Transform2D`
(the same bytes as Rust), `ponko::attached` is `Attached`,
`ponko::animation` is `Animation`, `ponko::velocity` is `Velocity`, and
`ponko::sprite` is `Sprite` (spawn, get and set, but not a query parameter,
since the engine stores sprites in a Rust layout; the compiler tells you).
[Physics](physics.md) bodies and colliders, `ponko::rigid_body` and
`ponko::collider`, work like sprites.

```cpp
{{#include ../../examples/cpp/main.cpp:cpp_world}}
```

## Queries and threads

`world.each(fn)` calls your function for every entity with the components
it takes (see [Queries](ecs.md#queries)); `world.query<A, B>(fn)` hands you
whole arrays instead. While a loop runs, the world and context are busy:
other world or context calls throw (C: return `PONKO_BUSY`), except

- commands: `despawn_later`, `despawn_recursive_later`, `spawn_later`,
  `set_later`, `remove_later`, applied when the system returns (in `each` and `query`, not in the `_par`
  versions);
- events: `send` and `read` (also not in the `_par` versions; see
  [Events](events.md));
- debug shapes (`debug()`, but not `frustum`), the 2D draw calls `draw`,
  `draw_rect`, `draw_line` and `draw_circle`, and `play_sound` (also not in
  the `_par` versions), so a loop can draw a bar or play a step for each
  entity it visits;
- 2D and 3D physics queries (`cast_ray` and the other casts, overlaps,
  `closest_point`, `penetrations`, and their `3d` forms such as `cast_ray3d`
  and `overlap_sphere3d`) and a third-person camera's `update_physics` (not
  in the `_par` versions), so a loop can check the ground under each body or
  have the camera follow the character. Anything that changes physics
  (gravity, impulses, bodies) is still refused inside a loop;
- reads that hand a whole array or a view once per system, so a loop needs
  no call per item (not in the `_par` versions, except the navigation view):
  `collisions_started()` and `collisions_stopped()` (C
  `ponko_ctx_collisions`, 2D and 3D contacts of this tick), `net_events()`
  (C `ponko_ctx_net_events`, replication events), and `nav_view()` (C
  `ponko_ctx_nav_view`), taken before the loop, whose `sample_position`,
  `sample_positions` and `find_path` work in `each_par` too. Read each once
  per system: a second read of the events gives only newer ones;
- world brains: `ai::each` and `ai::query` visit each entity's brain and
  blackboard with its components, and their rows are read and written in
  the `_par` versions too (C: the brain and blackboard query columns,
  `ponko_chunk_ai_brain` and the `ponko_ai_brain_*` and
  `ponko_ai_blackboard_*` calls; see [Game AI](ai.md));
- spatial queries, and the clock values `dt()`, `sim_tick()`, `random()`.

```cpp
{{#include ../../examples/cpp/guide.cpp:loop_reads}}
```

`each_par` and `query_par` split archetypes into slices and run your function
on all cores at once. It must only touch its own entity and read shared
data; that is the same rule as Rust's `par_for_each`.

To visit only the entities whose component changed, name a filter:
`world.each<ponko::changed<Health>>(fn)`. A parameter taken as `T&` counts
as a change, `const T&` doesn't; see
[Change detection](ecs.md#change-detection).

To visit only the entities that have or lack components, name `with`,
`without`, `or_` and `all_of` the same way, the C++ spelling of Rust's
`With`, `Without`, `Or` and tuples of filters:
`world.each<ponko::with<Player>, ponko::without<Dead>>(fn)`. C builds the
same filters from component ids (`ponko_filter_*`, `ponko_query_filtered`); see
[Queries](ecs.md#queries).

`each` is as fast as a hand-written loop over the arrays: 1M entities,
`position += velocity`, take 0.15 ms either way
(`examples/cpp/bench.cpp`).

Everything else stays on the thread that runs the game.

## Speed

The battle demo is a whole game, written twice: `examples/battle.rs` and
`examples/cpp/battle.cpp`. Both have flow fields, per-team grids, combat on
all cores, reinforcements and rounds. Same seed, same map, same armies
(the C++ `rng` gives the same numbers as Rust's), release builds,
`HEADLESS=1 UNITS=… TICKS=1200`:

| Units | Rust | C++ |
|---:|---:|---:|
| 20k | 0.57 ms/tick | 0.57 ms/tick |
| 200k | 4.04 ms/tick | 4.24 ms/tick |
| 1M | 18.85 ms/tick | 19.97 ms/tick |

The hot loops use views (`grid.view()`, `flow.view()`) that the header reads
inline, so nothing crosses into the engine per unit. The gap for big armies
is in rebuilding the per-team grids: Rust feeds them straight from the
units, C++ copies each team's units into an array first.

`HEADLESS=1 BOIDS=… ./ponko_cpp_boids` and `HEADLESS=1 BOIDS=… cargo run
--release --example boids` run the same flocking, release builds, on the
development Mac (arm64, 2026-11-03):

| Boids | Rust | C++ |
|---:|---:|---:|
| 100k | 3.5 ms/tick | 3.9 ms/tick |
| 1M | 31.3 ms/tick | 34.5 ms/tick |

The remaining gap is mostly the C++ version copying velocities into its own
array each tick before rebuilding its grid. Plain column updates (one million
`p += v`, `cargo run --release --example ffi_bench` vs `ponko_cpp_bench`,
which times `each` and `query`) run at the same speed in both: about 0.15 ms.

Grid queries in hot loops should use views (`ctx.spatial_view()`,
`grid.view()`): the header walks the grid inline, as Rust does. Through the
query functions, C++ boids took 41.5 ms/tick at 1M.

## Plain C

C uses the same functions without the wrapper: status codes instead of
exceptions, a `user` pointer instead of captures.

```c
{{#include ../../examples/cpp/main.c:c_world}}
```

```c
{{#include ../../examples/cpp/main.c:c_columns}}
```

A headless game in C:

```c
{{#include ../../examples/cpp/main.c:c_game}}
```

```c
{{#include ../../examples/cpp/main.c:c_run}}
```

Every call returns `PonkoStatus`; on failure `ponko_last_error()` has the
message and `ponko_last_error_kind()` its kind, a `PonkoErrorKind` that
mirrors Rust's `ponko::error::ErrorKind` (`PONKO_ERROR_NOT_FOUND`, `PONKO_ERROR_SHADER`,
..., per thread, until the next failed call). C++ throws `ponko::error`, a
`std::runtime_error` whose `kind()` is that kind. Null arguments, wrong
sizes, unknown ids, duplicate query columns and busy worlds are rejected.
Pointers you pass must still be valid: the library can't recognize a
dangling pointer or a buffer smaller than you say. Callbacks must return
normally; never let a C++ exception or `longjmp` cross them.

## Constructors and destructors

Two shapes exist, and one rule says which a new function uses.

- **New functions: a status and an out-pointer.** `enum PonkoStatus
  ponko_thing_new(args, struct PonkoThing **out)`. The status carries
  `ponko_last_error_kind()` for the failure, a null pointer can't be mistaken
  for success, and the call looks like every other. Follow this for anything
  added from now on. Today: `ponko_clip_new`, `ponko_snapshot_stream_new`,
  `ponko_interest_new`, `ponko_steam_lobby_create`, and the functions that make
  something inside a game or context and write back an id
  (`ponko_ctx_create_*`, `ponko_ctx_load_*` and `ponko_ctx_try_load_*`).
- **Older standalone objects: the pointer, null on failure.** `struct PonkoThing
  *ponko_thing_new(args)` returns null and sets `ponko_last_error()`.
  `ponko_world_new`, `ponko_game_new`, `ponko_access_new`, `ponko_action_map_new`,
  `ponko_nav_new`, `ponko_flow_new`, `ponko_grid_new`, `ponko_nav_sources_new`,
  `ponko_latest_wins_new`, `ponko_input_scenario_new`, `ponko_cells_new`,
  `ponko_memory_source_new`, `ponko_ui_access_new`, `ponko_heightfield_new`,
  `ponko_far_terrain_new`, `ponko_terrain_tile_data_new`,
  `ponko_animation_graph_builder_new` and the mesh-data and filter builders
  work this way. They keep their shape: changing
  it would break the ABI.

Destructors of owned objects are `void ponko_thing_free(struct PonkoThing *)`.
(`ponko_ctx_handle_free` is not one: it frees an engine handle by id inside a
context and returns a status like the other `ponko_ctx_*` calls.) The exception
is `ponko_world_free`, which returns a status: a world can be busy (a query is
running) or borrowed (it belongs to a game or a context), and then it is *not*
freed. `PONKO_BUSY` tells you the handle is still live and still yours, so you
don't treat it as freed. The C++ `ponko::world` destructor ignores the status
(it can't throw); its move assignment throws it.

## Errors and the `try_` loaders

Rust's `ponko::error::Error` has a kind, the asset path it is about (if any) and a
message. C has the same three for the last failure on the thread:
`ponko_last_error_kind()`, `ponko_last_error_path()` (empty when the failure
names no asset) and `ponko_last_error()`; `ponko_error_kind_name(kind)` gives
a short name for logs ("not found", "decode", ...). C++ throws
`ponko::error`, with `kind()`, `path()` and `what()`, and `ponko::kind_name`.

The `try_` loaders return the error instead of panicking (Rust) or throwing
(C++): `try_load_texture`, `try_load_font`, `try_load_sound`,
`try_load_stream` and `try_load_mesh`, plus `try_load_shader`,
`try_load_effect` and `try_load_compute` in C++. C++ returns a
`ponko::result<T>`: test it like a bool, `*` or `value()` for the value (which
throws the error if it failed), `value_or(fallback)`, `error()` and `kind()`.

<div class="lang-pair">
<div>

```rust
fn gold_skin(ctx: &mut Ctx) -> Option<Texture> {
    match ctx.gfx.try_load_texture("skins/gold.png") {
        Ok(skin) => Some(skin),
        Err(e) => {
            eprintln!("{} ({:?}): {}", e.path().unwrap_or(""), e.kind(), e.message());
            None
        }
    }
}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:try_load}}
```

</div>
</div>

C: `ponko_ctx_try_load_texture`, `_font`, `_sound`, `_stream` and `_mesh`
return `PONKO_NOT_FOUND` for a missing file and `PONKO_INVALID_ARGUMENT` for
any other error, with the kind and path set, and leave the output untouched.
The older `ponko_ctx_load_texture` and friends keep their status codes; the
C++ `load_*` calls now use the `try_` functions, so the `ponko::error` they
throw has the real kind and path.

`PONKO_UNSUPPORTED` means this machine or build can't do it.
`ponko_game_run` returns it when the game can't start here: no usable GPU or
graphics device, no window or event loop, or a library built without the
`window` feature. A launcher can then show its own message or fall back to
another mode; `ponko_last_error()` says what was missing. (Earlier releases
returned `PONKO_INVALID_ARGUMENT` there.) C++'s `game::run` throws a
`ponko::error` of kind `PONKO_ERROR_UNSUPPORTED` as before. Treat any status
but `PONKO_OK` as a failure: new ones can arrive in any release.

## Settings that can grow

A C struct can't gain a field without breaking programs built against the
older header, so the ABI 2 settings structs (`PonkoMaterialDesc`,
`PonkoNavBakeSettings`, `PonkoLighting`, `PonkoCollider`, ...) stay as they
are. Settings that grow get a **versioned descriptor** beside them, named
`...Ex`: it starts with `struct_size` and `version`, holds the ABI 2 struct,
then the fields added since. Set the two header fields, fill it with its
`_defaults` call (or read the current values), change what you need:

```c
{{#include ../../examples/cpp/guide.c:versioned_descriptor}}
```

The library reads only the fields of the version you compiled against, so a
program built today keeps working with a library that has added fields: they
get their defaults. It refuses a descriptor whose header wasn't set, or that
comes from a newer `ponko.h` than the library, and it never keeps a pointer
to it. The full rule is in
[STABILITY.md](../../STABILITY.md) ("Versioned descriptors").

| Descriptor | Calls | What it adds to ABI 2 |
|---|---|---|
| `PonkoMaterialDescEx` | `ponko_ctx_create_material_ex`, `ponko_ctx_update_material_ex` | the stencil state, in the same description |
| `PonkoNavBakeSettingsEx` | `ponko_ctx_bake_navmesh_ex` | nothing yet: the place new bake settings go |
| `PonkoLightingEx` | `ponko_ctx_lighting_ex`, `ponko_ctx_set_lighting_ex` | `shadow_sun_size` (the sun's size for contact-hardening shadows) and `shadow_sun_step` |
| `PonkoColliderEx` | `ponko_set_collider_ex`, `ponko_get_collider_ex` | a 2D collider's collision groups and polygon corners, in one call |
| `PonkoQueryFilterEx` | the `ponko_ctx_*_ex` 2D and 3D queries | one filter for both, collision groups included |
| `PonkoPostEx` | `ponko_ctx_post_ex`, `ponko_ctx_set_post_ex`, `ponko_post_ex_lerp`, `ponko_post_blend_volumes` | split toning, depth of field and the sample count (`msaa`); blending two looks, and post volumes (`PonkoPostVolume` boxes) |
| `PonkoParticleSettings3DEx` | `ponko_particle_settings3d_ex_defaults`, `ponko_ctx_set_particle_emitter3d_ex` | `softness`: soft particles |
| `PonkoCamera3DEx` | `ponko_ctx_camera3d_ex`, `ponko_ctx_set_camera3d_ex`, `ponko_camera3d_ex_from` | `occlusion_culling` and `particles` (whether it draws particles, trails and lines) |
| `PonkoTargetCameraEx` | `ponko_ctx_set_target_cameras_ex`, `ponko_ctx_target_camera_ex`, `ponko_target_camera_ex_from` | `particles`, per target camera |
| `PonkoWindowModeEx`, `PonkoWindowVideoModeEx` | `ponko_ctx_window_mode_ex`, `ponko_ctx_window_set_mode_ex`, `ponko_ctx_window_video_mode_ex` | new in this release (no ABI 2 struct): the [window](window.md#c-and-c) mode and a monitor's exclusive modes |
| `PonkoWindowPropertiesEx`, `PonkoWindowStateEx`, `PonkoWindowMonitorEx`, `PonkoWindowImageEx`, `PonkoWindowEventEx` | `ponko_ctx_window_properties_ex`, `ponko_ctx_window_state_ex`, `ponko_ctx_window_monitor_ex`, `ponko_ctx_window_set_icon_ex`, `ponko_ctx_window_set_cursor_image_ex`, `ponko_ctx_window_event_ex` | new in this release: requested and reported window state, monitors, icon and cursor images, window events |
| `PonkoCrashReports`, `PonkoTouchControlsDesc`, `PonkoSliceLoader` | `ponko_game_crash_reports`, `ponko_ctx_set_touch_controls_ex`, `ponko_asset_group_add_sliced` | new in this release: [crash report](crash-reports.md#c-and-c) settings and hook, touch controls with their [styles and places](touch.md#c-and-c), [sliced loaders](loading.md#sliced-loaders-from-c) |
| `PonkoCrowdDesc`, `PonkoNavCrowdStats` | `ponko_ctx_create_crowd_ex`, `ponko_ctx_navmesh_crowd_stats` | new in this release: a crowd with [crossfading levels](crowds.md#c-and-c), the [navmesh crowd's counts](navigation3d.md#counting-the-crowds-work) |
| `PonkoWaterDesc` version 2 | `ponko_ctx_spawn_water` | `has_spectrum` and `spectrum`: a [sea from a wave spectrum](water.md#from-c-and-c) |
| `PonkoLoopbackLink` | `ponko_loopback_link_defaults`, `ponko_net_messages_loopback_link` | new in this release: an in-memory link that [loses and delays datagrams](replication.md#test-it), for native tests |
| `PonkoBuoyancyDesc` | `ponko_buoyancy_desc_box`, `_sphere`, `_points`, `ponko_ctx_set_buoyancy`, `ponko_ctx_buoyancy` | new in this release: a [floating body's](water.md#from-c-and-c) sample points, the water's density, drag and the ripple switch |
| `PonkoUploadStatsEx` | `ponko_ctx_upload_stats_ex` | the upload counters with the GPU's own time for [made textures](loading.md#c-and-c) (`gpu_created_ms`) and for the latest upload step (`gpu_upload_ms`) |

In C++ the same types are `ponko::material_desc_ex`,
`nav_bake_settings_ex`, `lighting_ex`, `collider_ex`, `query_filter_ex`,
`post_ex`, `camera3d_ex` and `target_camera_ex`.
`material_desc_ex_of(desc)`, `nav_bake_ex_defaults()`,
`lighting_ex_defaults()`, `collider_ex_of(collider)`,
`query_filter_ex_of(filter)`, `post_ex_defaults()`, `camera3d_ex_of(camera)`
and `target_camera_ex_of(camera)` fill the header for you; `ctx::lighting_ex`,
`ctx::set_lighting`, `ctx::create_material`, `ctx::update_material`,
`ctx::bake_navmesh`, `world_view::set`, `world_view::get_collider_ex`,
`ctx::post_ex`, `ctx::set_post`, `ctx::camera3d_ex`, `ctx::set_camera3d`,
`ctx::target_camera_ex` and `ctx::set_target_cameras_ex` take them. The C++
query methods keep taking `filter2d` and `filter3d`, which carry the
collision groups already, and `particle_settings` has `softness` (its
`ctx::set_particle_emitter3d` calls the versioned entry point). The 3D picture's
descriptors are shown in [3D: split toning and depth of
field](3d.md#split-toning-and-depth-of-field), [soft particles](3d.md#soft-particles) and
[render targets](3d.md#render-targets):

```c
{{#include ../../examples/cpp/guide.c:versioned_post}}
```

An array of descriptors can't have a stride that older callers agree on, so a
list of them is passed as an array of pointers (`ponko_ctx_set_target_cameras_ex`),
and a struct that is only a plain box around one (`PonkoPostVolume`) points at
its descriptor.

`scripts/check-settings-fields.py` (part of `check-ffi-coverage.py`) keeps
this complete: every public field of a Rust settings struct (`Post`,
`ParticleSettings`, `Camera`, `Lighting`, `MaterialDesc`, ...) has a field of
the same name in its C struct or descriptor, or a line in
`scripts/settings-fields.txt` that names where C sets it, or a Rust-only entry
with its reason.

## C functions without a C++ wrapper

Every C function in `ponko.h` has a C++ wrapper in `ponko.hpp`, except these,
where C++ does the same thing another way:

| C function | In C++ |
|---|---|
| `ponko_ctx_ui_push_screen` | `ui::push_screen`, including its deprecated bool overload, uses `ponko_ctx_ui_push_screen_with_mode` |
| `ponko_ctx_load_texture`, `ponko_ctx_load_mesh`, `ponko_ctx_load_sound`, `ponko_ctx_load_stream`, `ponko_ctx_load_font` | `ctx::load_*` call the `ponko_ctx_try_load_*` functions, so the `ponko::error` they throw has the kind and path |
| `ponko_query`, `ponko_query_par` | `world_view::each` and `query` (and the `_par` versions) call `ponko_query_with`, `ponko_query_filtered` or `ponko_query_cached`, which also take change filters |
| `ponko_filter_check` | `with`, `without`, `or_` and `all_of` are C++ types; the query checks them when it runs |
| `ponko_quat_mul`, `ponko_quat_rotate` | `quat * quat` and `quat * vec3`, inline, with the same results |
| `ponko_entity_bits` | `ponko::bits(entity)`, inline |
| `ponko_text3d_default` | `ponko::text3d{}`: its member initializers are the same defaults |
| `ponko_game_plugin` | `game::plugin(object)`, for any object with `build(ponko::game&)` and an optional `setup(ponko::ctx&)` |
| `ponko_ctx_spawn_later` | `ctx::spawn_later` calls `ponko_ctx_spawn_later_entity`, so it can return the new entity |
| `ponko_ctx_cast_ray_filtered`, `ponko_ctx_overlap_ball` (2D) | `ctx::cast_ray_filtered` and `ctx::overlap_ball` call `ponko_ctx_cast_ray2d` and `ponko_ctx_overlap_circle2d`, which also take the `filter2d`'s collision groups |
| `ponko_ctx_cast_ray2d_ex`, `ponko_ctx_cast_ray2d_all_ex`, `ponko_ctx_cast_circle2d_ex`, `ponko_ctx_cast_box2d_ex`, `ponko_ctx_cast_capsule2d_ex`, `ponko_ctx_overlap_point2d_ex`, `ponko_ctx_overlap_circle2d_ex`, `ponko_ctx_overlap_box2d_ex`, `ponko_ctx_overlap_capsule2d_ex`, `ponko_ctx_closest_point2d_ex`, `ponko_ctx_penetrations2d_ex`, `ponko_ctx_cast_ray3d_ex`, `ponko_ctx_cast_ray3d_all_ex`, `ponko_ctx_cast_sphere3d_ex`, `ponko_ctx_cast_capsule3d_ex`, `ponko_ctx_cast_box3d_ex`, `ponko_ctx_overlap_point3d_ex`, `ponko_ctx_overlap_sphere3d_ex`, `ponko_ctx_overlap_box3d_ex`, `ponko_ctx_overlap_capsule3d_ex`, `ponko_ctx_closest_point3d_ex`, `ponko_ctx_penetrations3d_ex` | The `ctx` queries of the same names take a `filter2d` or `filter3d`, which carries the collision groups already; `query_filter_ex_of` turns one into a `PonkoQueryFilterEx` for the C calls |
| `ponko_ctx_set_particle_emitter3d` | `ctx::set_particle_emitter3d` calls `ponko_ctx_set_particle_emitter3d_ex`, which also takes the settings' `softness` |
| `ponko_ctx_set_touch_controls` | `ctx::set_touch_controls` calls `ponko_ctx_set_touch_controls_ex`, which also takes the controls' styles and places |
| `ponko_ctx_ui_next_event` | `ui::tree::next_event` calls `ponko_ctx_ui_next_event_ex`, which also gives the widget set's events (`chosen`, `dropped`, `rows_shown`) |

`scripts/check-cpp-api.py` keeps this true: CI fails when a C function has no
C++ wrapper and isn't on this list (`scripts/cpp-c-only.txt`, with the reasons).
The same check requires a `///` doc comment on every public declaration in
`ponko.hpp`, which is the C++ API reference: what each call does, its units,
who owns what, and what it throws. The C and C++ snippets in this guide come
from `examples/cpp` (`guide.cpp`, `guide.c` and the examples), which CI builds
and runs.

## Still Rust-only

C and C++ resources and components are plain structs (no `std::string`
or `std::vector` inside), copied in and out.

Most chapters show Rust and C++ side by side. These features exist only in
Rust today; the chapters say so where they use them.
`scripts/check-ffi-coverage.py` checks this table against
`scripts/rust-only.txt`, and that every name still exists in the Rust
source and has no C function of its own, so a row goes away when its C
binding arrives:

| Rust feature | Where | From C and C++ |
|---|---|---|
| `Assets::scene_asset_field` | [Asset sources](asset-sources.md) | C components have no text fields to name an asset in; load what a scene needs before `load_scene` |
| `PackSource` | [Asset sources](asset-sources.md#packs) | C and C++ games read the default `assets.pack` like Rust games; a pack of their own can be read through `ponko_ctx_add_asset_source` |
| `Query::with_lookup`, `World::get_many_mut` | [Entities, components, queries](ecs.md#borrowing-several-things-at-once) | Generic borrows; component pointers from `get` don't borrow |
| `World::query_ref` | [Entities, components, queries](ecs.md#read-only-queries-on-a-shared-world) | C and C++ queries read through component pointers and need no exclusive borrow of the world |
| `SystemView` | [Declared system access](system-access.md) | C and C++ systems take the whole `ctx`, so they run alone; declare their access by name |
| `Storage::write_json`, `Storage::read_json` | [Saves and settings](storage.md#save-slots) | Generic over Rust types; serialize yourself and pass the bytes to `storage_write` and `storage_read` |
| `SaveFormat`, `Storage::save_world_in_background` | [Saves and settings](storage.md#large-worlds) | Not bound yet: save slots are JSON, written on the game's thread by `ponko_ctx_save_world`; `ponko_ctx_load_world` reads binary slots too |
| `UiAccessAdapter::for_ns_view` | [Retained game UI](ui.md#the-screen-reader-adapter) | It needs a Rust `Ui`; the engine's own window gets the adapter, and `ponko_ui_access_*` give a snapshot for one of your own |
| `NavObstacleShape` | [3D navigation meshes](navigation3d.md#obstacle-shapes) | Obstacles are unturned boxes: `ponko_ctx_set_nav_obstacle` and `ponko_ctx_set_nav_carve_obstacle` |
| `SkyShader` | [A sky of your own](3d.md#a-sky-of-your-own) | The built-in atmosphere or the flat sky (`PonkoSky`); sky shaders have no C binding yet |
| `Graphics::stream_instance_cell` | [Streaming instance cells](3d.md#streaming-instance-cells) | Build a cell's matrices on your own threads, then `ponko_ctx_add_instance_cell` (`ctx::add_instance_cell`) on the game's |
| `InstanceCellData` | [Streaming instance cells](3d.md#streaming-instance-cells) | The same: matrices and colours go straight to `ponko_ctx_add_instance_cell` |
| `GamepadBackend` | [Backends and tests](controllers.md#backends-and-tests) | Native games use the built-in gamepad backend |
| `FakeGamepads` | [Backends and tests](controllers.md#backends-and-tests) | Native tests feed `ponko_ctx_feed_input` (kind 8 connects a pad) and `ponko_ctx_feed_gamepad_motion` |
| `Graphics::instance_cell_state` | [Streaming instance cells](3d.md#streaming-instance-cells) | The same: a cell added from C is on the GPU at once |
| `Ui::set_rich`, `Ui::set_inline_image`, `Ui::set_inline_font`, `Ui::link`, `UiEvent::LinkClicked` | [Retained game UI](ui.md#rich-text) | Not bound yet: several labels side by side, each with its own paint; a button for each link |
| `GltfScene::nav_source` | [From Blender to the game](blender.md#navigation-meshes) | Read the nodes and their places with `ponko_gltf_scene_load`, and bake from mesh data of your own (`ponko_nav_sources_add_mesh`, `ponko_ctx_bake_navmesh`) |
| `Graphics::load_lights`, `Graphics::load_cameras` | [3D](3d.md#gltf-extensions-lights-and-cameras) | Models load with the same glTF extensions; a file's lights and cameras aren't returned yet, so place them yourself |
| `Smoothing` | [Client prediction](networking.md#your-own-player-at-once-client-prediction) | Generic over what is drawn; prediction and the input queue are bound (`ponko::prediction`), so fade the correction offset (`reconciled::before`) yourself |
| `Physics3D::move_character` | [3D physics](physics3d.md#a-walking-character) | Characters walk with the physics step only; one tick can't be replayed on its own |
| `Physics3D::set_terrain_rebuild` | [3D physics](physics3d.md#bodies-colliders-and-terrain) | A loaded tile gets a new terrain collider from its heights |
| `Physics3D::clear_terrain_rebuild` | [3D physics](physics3d.md#bodies-colliders-and-terrain) | Same |
| `Net::accept_web`, `Net::join_web`, `Net::web_addr`, `NetMessages::accept_web`, `NetMessages::join_web`, `NetMessages::web_addr` | [Multiplayer in the browser](networking.md#in-the-browser) | Not bound yet: C and C++ hosts take UDP players only, and C and C++ games don't build for the web yet |
| `LobbyClient::join_web` | [Lobbies, matchmaking and relays](lobby.md#the-client) | C and C++ games don't build for the web yet; their lobby clients join over UDP |
| `Signaling` | [Peer to peer over WebRTC](webrtc.md#with-any-signaling-channel) | C and C++ WebRTC endpoints are signaled through a lobby client (`ponko_net_messages_rtc_host`, `_rtc_join`) |
| `RtcConfig::turn` | [Peer to peer over WebRTC](webrtc.md#stun-turn-and-nats) | TURN works in browsers only, and C and C++ games don't build for the web yet |
| `SkeletonData::add_bone`, `SpineAtlas` | [2D skeletons](skeleton2d.md#from-c-and-c) | Skeletons come from files (Ponko's format or a Spine export and its atlas) through the assets; a memory source holds files kept in memory |
| `Plugin::dependencies` | [Plugins for other crates](plugins.md#from-c-and-c) | C plugins (`ponko_game_plugin`) have no names or dependencies and are never duplicates |
| `Game::debug_toggle` | [Plugins for other crates](plugins.md#debug-toggles) | Draw behind a flag of the game's own; `debug.draw` turns the engine's views on and off |
| `Game::control_method` | [Plugins for other crates](plugins.md#control-methods) | The control endpoint's built-in methods only; a native game's tools read its reflected resources |

System phases, system ids, setup callbacks and plugins are not on this
list. They have C and C++ calls, described in
[Systems and the frame](systems.md):

| Rust | C++ | C |
|---|---|---|
| `Game::add(phase, f)` | `game::add(ponko::phase::..., f)` | `ponko_game_add_system` |
| `Game::add_system(phase, f)` → `SystemId` | `game::add_system` → `ponko::system_id` | `ponko_game_add_system_id` |
| `Game::remove(id)`, `Systems::DRAW_SPRITES` | `game::remove(id)`, `ponko::systems::draw_sprites` | `ponko_game_remove`, `PONKO_SYSTEM_DRAW_SPRITES` |
| `Game::setup(f)` | `game::setup(f)` | `ponko_game_setup` |
| `Plugin`, `Game::plugin` | an object with `build` (and `setup`), `game::plugin` | `ponko_game_plugin` |

The functions in `ponko::systems` themselves add a built-in back in
another place: `ponko_game_add_builtin` (`game::add_builtin` in C++) takes
the phase and the built-in's id, and gives the physics and navmesh steps
their declarations. `ponko_builtin_access` (`access::builtin`) returns a
built-in's declaration.
