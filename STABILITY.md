# Stability

What a Ponko release may change, and what it keeps. Ponko is at **0.x**: the
API is still settling, and the rules below say how breaking changes happen
while it does.

Ponko was formerly Crabby Engine. 0.1.0 removed every old name; the
[naming chapter](book/src/naming.md) maps them to the new ones.

## Versions

The crates follow [semver as Cargo applies it to 0.x](https://doc.rust-lang.org/cargo/reference/semver.html):

- **Patch releases (0.1.x)** are compatible with each other: bug fixes and
  additions only. `ponko = "0.1"` in Cargo.toml picks up the newest one, and
  your code keeps compiling and behaving as documented.
- **Minor releases (0.2.0, 0.3.0, ...)** may break things: rename, move or
  remove items, change signatures, behavior or defaults. Every break is
  listed in the CHANGELOG under **Breaking changes** with what to use
  instead, so the CHANGELOG is the migration guide.
- Removing something does not wait for a deprecation period. Where it is
  cheap, a patch release marks the item `#[deprecated]` (Rust) or adds a note
  in the header (C) first, pointing at its replacement.

1.0 will come when the API has stopped moving; from then on, only major
releases break. [Releases](#releases) says when each kind of release happens.

What the versions cover:

| Crate | What is covered |
|---|---|
| `ponko` | Every public item that is not `#[doc(hidden)]`, and the cargo features, except the [unstable surfaces](#unstable) |
| `ponko_ecs` | Every public item that is not `#[doc(hidden)]` |
| `ponko_macros` | The derives (`#[derive(Component)]`) and the attributes they accept, used through `ponko` or `ponko_ecs`. The code they generate is not covered. |
| `ponko_ffi` | The C ABI in `include/ponko.h` and the C++ wrapper `include/ponko.hpp` (see [C ABI](#c-abi)) |
| `ponko_mcp` | The MCP tool names and their parameters |
| `ponko_unity` | Its Rust API. The files it writes (the imported scenes, meshes and materials) are not a stable format: re-import after upgrading. |

Enums and structs that will grow are `#[non_exhaustive]` (`ErrorKind`, `Key`,
`MouseButton`, `GamepadButton`, `NetState`, `BodyKind`, the render
descriptors and settings, ...): matches need a `_` arm and such structs are
built through their constructors or `Default`, so new variants and fields
can arrive in a patch release.

The image-format features (`image-jpeg`, `image-webp`, `image-bmp`, `image-tga`,
`image-gif`, `image-tiff`, `image-ico`, `image-qoi`, `image-pnm`, `image-dds`,
`image-hdr`, `image-exr` and `image-formats` for all of them) are covered like
every cargo feature: they stay, and keep the formats they name. `image-jpeg`
is a default feature, and `3d` turns on `image-jpeg`, `image-hdr` and
`image-exr`. PNG is always built in. A new format arrives as a new feature,
never as a change to what the existing ones turn on.

## Plugins

The `Plugin` trait is the extension point for other people's crates, and
it is covered like every public item: `build`, `setup`, `try_setup`,
`name` and `dependencies` keep their meaning, and a method added to it
comes with a default, so a plugin written for a minor release keeps
compiling in its patch releases. So do what a plugin adds through:
`Game::add` and the other builder calls, `Game::has_plugin`,
`Game::debug_toggle`, `Game::control_method`, and the `Ctx` services.

- **The simple path stays.** A game that adds no plugin is written as
  before (`Game::new`, systems, `run`): plugins need no registration step,
  no feature and no cost when unused.
- **The rules are part of the promise**: a plugin's name is its type's
  path unless it says otherwise; adding a name twice, or before a
  dependency it lists, fails startup with `ErrorKind::Invalid` before any
  setup runs; a dependency's setup runs before its dependent's, and its
  systems first in every phase they share. Error message text is not
  covered ([Unstable](#unstable)).
- **Control methods a game adds** are its own contract, not Ponko's: the
  endpoint lists them as `"added": true`, and their names never use the
  engine's namespaces, so an engine release adds methods without
  colliding with them. ponko-mcp's `call_game_method` tool and its
  parameters are covered like its other tools.
- **For plugin crates**: depending on `ponko = "0.y"` is compatible across
  that minor's patch releases; a new minor may break them, as it may any
  code. The guide's "Plugins for other crates" has the naming and
  versioning rules.

## Releases

| Release | When | Contains |
|---|---|---|
| Patch (0.1.1, 0.1.2, ...) | As needed: within days for a regression or a security fix, otherwise when there are fixes to ship | Bug fixes and additions. Nothing that breaks code written for the same minor. |
| Minor (0.2.0, 0.3.0, ...) | When the P1 scope of its [milestone](https://github.com/ponkoengine/ponko/milestones) is done, roughly every 8 to 12 weeks | New features and breaking changes, each break with its migration entry. |
| 1.0 | When the readiness checklist (#432) is complete | The last 0.x release with the compatibility promise attached. It breaks nothing. |

- **A release is scope, not a date.** P1 issues define a minor; what is not
  ready moves to the next one instead of delaying or rushing the release.
- **Release candidates.** From 0.2.0 on, a minor is published as
  `0.y.0-rc.1` at least a week before the release. Cargo never selects a
  release candidate unless the version is written out
  (`ponko = "0.2.0-rc.1"`). A regression found in one is fixed and a new
  candidate follows; the release is the last candidate, unchanged. Patch
  releases have no candidates.
- **Support.** The newest minor gets patch releases. The minor before it
  gets fixes for security problems, crashes and data loss for four weeks
  after a new minor, so there is time to migrate. Older ones get none.
- **Patch releases come from a release branch** (`release-0.1`, ...), so
  `main` can take breaking changes for the next minor right after a
  release. Fixes land on `main` first and are carried over (the `backport`
  label marks them; CONTRIBUTING.md, "Releasing", has the procedure).
- **Deprecation.** Where it is cheap, a replaced item stays as
  `#[deprecated]`, naming its replacement, until the next minor removes it.
  Where it is not, it goes at once, with its migration entry.
- **Yanking.** A release is yanked only if it loses data, fails to build or
  has a security problem, and a fixed patch release goes out with it.
- **After 1.0.** Minor releases only add, major releases break, and majors
  are at most yearly; the exact support periods are written here before 1.0
  is released.

## Unstable

Not covered by the versions, and allowed to change in any release:

- `#[doc(hidden)]` items, among them `ponko::__ffi` (the engine internals that
  `ponko_ffi` builds on), test helpers such as `Net::loopback_lossy`, and
  macro internals.
- The `bench-internals` cargo feature and what it turns on (benchmark
  drivers for `crates/ponko_bench`, which is not published).
- `ponko::ui::unstable_taffy`, a re-export of the `taffy` layout crate for
  advanced UI layout, and the `UiLayout` methods that hand out or accept its
  types: `UiLayout::unstable_custom`, `UiLayout::unstable_style`,
  `UiLayout::unstable_style_mut` and `UiLayout::into_unstable_style`, plus their
  deprecated old names `UiLayout::custom`, `UiLayout::style`,
  `UiLayout::style_mut` and `UiLayout::into_style`. They sit next to the stable
  presets (`UiLayout::row`, `column`, ...), but their signatures follow
  Taffy's, so a Ponko release may change them, and the Taffy version behind
  them, when Taffy changes. Use the presets in code that must keep compiling.
- **Which version of a re-exported crate** (`glam`, `serde_json`,
  `steamworks`) stands behind Ponko's names, before 1.0: see
  [Re-exported crates](#re-exported-crates).
- Error message text, log output, the inspector's look, and the exact
  frames rendered: tests should check behavior, not strings or pixels.
- Performance. Regressions are bugs, and benchmark numbers go in PRs, but
  timings are not part of the API. A pull request that touches the engine
  compares the instruction counts of a few workloads with its base
  (`crates/ponko_bench/README.md`, "Tracking results over time").

## Errors and panics

Fallible calls follow one convention, built on `ponko::error::Error`:

- `load_x` / `create_x` are for setup code, where a missing file is a bug.
  They panic with the `Error`'s message (which starts with `ponko:`).
- `try_load_x` / `try_create_x` return `Result<T, ponko::error::Error>`. Match on
  `Error::kind()` (a `#[non_exhaustive]` `ErrorKind`), never on the message.
- Operations that fail in normal play (scene loads, prefab spawns, network
  joins) return a `Result` under their plain name and have no panicking twin.

Every setup loader follows it: collision meshes
(`Ctx::load_collision_mesh`, `load_collision_mesh_with`), string tables
(`Assets::load_strings`) and typed data files (`Assets::load_resource`) panic
under the plain name and have a `try_` twin. Runtime operations (scene,
world and level loads, `Assets::load_with`'s custom decoder, and the reflected
JSON loaders `load_reflected_resource_json` and
`load_reflected_component_json`, which merge into existing data and fail for a
dead entity or an unreflected type) return `Result` under their plain name.
There is one name per loader: 0.2.0 removed `load_resource_json` and
`load_component_json` in favor of the `load_reflected_*_json` names, and
changed the plain collision-mesh and string-table loaders from `Result` to
panicking (migration entries in the changelog).

### Loader policy inventory

This table covers every public Rust `load_*` method across engine features
(the API snapshots omit hidden FFI helpers). `Error` means `ponko::error::Error`.
Panicking setup loaders print the engine error, starting with `ponko:`.
Runtime entries describe load failures, not panics from user callbacks or
unrelated programmer invariants. Available methods depend on cargo features.

<!-- loader-policy-table:start -->
| Service and `load_*` method | Plain return | Load failure / fallible form |
|---|---|---|
| `Graphics::load_texture` | `Texture` | Panics; `try_load_texture` returns `Result<Texture, Error>` |
| `Graphics::load_font`, `load_system_font` | `Font` | Panics; matching `try_load_*` returns `Result<Font, Error>` |
| `Graphics::load_mesh` | `Mesh` | Panics; `try_load_mesh` returns `Result<Mesh, Error>` |
| `Graphics::load_mesh_data` | `MeshData` | Panics; `try_load_mesh_data` returns `Result<MeshData, Error>` |
| `Graphics::load_skeleton` | `Skeleton` | Panics; `try_load_skeleton` returns `Result<Skeleton, Error>` |
| `Graphics::load_clip` | `AnimationClip` | Panics; `try_load_clip` returns `Result<AnimationClip, Error>` |
| `Graphics::load_clips` | `Vec<AnimationClip>` | Panics; `try_load_clips` returns `Result<Vec<AnimationClip>, Error>` |
| `Graphics::load_cameras` | `Vec<ModelCamera>` | Panics; `try_load_cameras` returns `Result<Vec<ModelCamera>, Error>` |
| `Graphics::load_lights` | `Vec<ModelLight>` | Panics; `try_load_lights` returns `Result<Vec<ModelLight>, Error>` |
| `Graphics::load_shader` | `Shader` | Panics; `try_load_shader` returns `Result<Shader, Error>` |
| `Graphics::load_sky_shader` | `SkyShader` | Panics; `try_load_sky_shader` returns `Result<SkyShader, Error>` |
| `Graphics::load_sprite_shader` | `SpriteShader` | Panics; `try_load_sprite_shader` returns `Result<SpriteShader, Error>` |
| `Graphics::load_effect` | `Effect` | Panics; `try_load_effect` returns `Result<Effect, Error>` |
| `Graphics::load_compute` | `Compute` | Panics; `try_load_compute` returns `Result<Compute, Error>` |
| `Graphics::load_texture_array` | `TextureArray` | Panics; `try_load_texture_array` returns `Result<TextureArray, Error>` |
| `Graphics::load_baked_lighting` | `BakedLighting` | Panics; `try_load_baked_lighting` returns `Result<BakedLighting, Error>` |
| `Graphics::load_reflection_probe` | `()` | Panics; `try_load_reflection_probe` returns `Result<(), Error>` |
| `Ctx::load_skeleton2d` | `Arc<SkeletonData>` | Panics; `try_load_skeleton2d` returns `Result<Arc<SkeletonData>, Error>` |
| `Audio::load_mixer` | `()` | Panics; `try_load_mixer` returns `Result<(), Error>` |
| `Audio::load_stream` | `Sound` | Panics; `try_load_stream` returns `Result<Sound, Error>` |
| `Assets::load_resource` | `()` | Panics; `try_load_resource` returns `Result<(), Error>`; replaces the whole resource |
| `Assets::load_strings` | `()` | Panics; `try_load_strings` returns `Result<(), Error>` |
| `Assets::load_behavior_tree` | `()` | Panics; `try_load_behavior_tree` returns `Result<(), Error>` |
| `Assets::load_reflected_resource_json`, `load_reflected_component_json` | `Result<(), Error>` | Reflection edits; no panic on load failure; object fields merge |
| `Assets::load_with` | `Result<T, Error>` | Custom decoder operation; propagates source/decoder errors (a user decoder can panic) |
| `Graphics::load_aseprite` / `try_load_aseprite` | `Aseprite` / `Result<Aseprite, Error>` | Named sprite atlas and timeline, cached and hot reloaded |
| `Assets::load_level`, `Assets::load_ldtk_worlds` | `Result<Level, Error>`, `Result<Vec<LdtkWorld>, Error>` | Runtime level operations; no panic on load failure |
| `Ctx::load_scene`, `load_scene_lockstep`, `load_world` | `Result<(), Error>` | Runtime world operations; no panic on load failure |
| `Ctx::load_collision_mesh`, `load_collision_mesh_with` | `CollisionMesh` | Panics; matching `try_load_*` returns `Result<CollisionMesh, Error>` |
| `Ctx::load_texture`, `load_sound` | `Texture`, `Sound` | Deprecated forwarding shortcuts, removed in the next minor; panic; use `gfx.load_texture` / `audio.load` (fallible: `gfx.try_load_texture` / `audio.try_load`) |
<!-- loader-policy-table:end -->

`tests/loader_convention.rs` walks all API snapshots: a `load_x` that returns
`Result` must be one of the runtime operations in its short closed list, and
every other `load_x` must have a `try_load_x` twin returning
`Result<_, ponko::error::Error>`. It also checks that every `load_*` appears
in this table and in the data-files chapter, and that the two tables are
identical. A new conforming pair needs only a table row. The only
`load_*` that don't have a twin are the `Ctx::load_texture` and `load_sound`
shortcuts, deprecated forwarders to `ctx.gfx` and `ctx.audio` (which have the
twins) that the next minor removes. Data reload failures preserve the previous
value and report the error; they do not invoke the setup panic path. Native
bindings: C returns a status with `ponko_last_error_kind()`, C++ throws
`ponko::error` from `load_x` and returns `ponko::result` from `try_load_x`.

Callbacks a game hands the services (`Assets::load_with`'s decoder,
`Assets::scene_asset_field`'s loader) return `ponko::error::Error` too.
`String: From<ponko::error::Error>` lets `?` turn an `Error` into a `String`
(its message, without the prefix).

## The crate root and globs

The crate root holds only modules: every item has one path, in its module,
and the prelude re-exports the common ones as the same items. Games start from `use ponko::prelude::*;`
and import modules as needed; `use ponko::*;` brings in the modules, not the
prelude.
`ponko::ecs_core` is the one way into the ECS crate beyond the prelude, and
`d2`/`d3` are the one path of the types named after a dimension.
`scripts/check-naming.py --paths` fails on an item with a second public
path; a `#[deprecated]` alias of a moved item stays for one minor.

The prelude's contents are API too: removing a name breaks code that used
it through the glob, and adding one can make a game's own glob import of
the same name ambiguous. Both happen only in minor releases, and the
CHANGELOG names every name the prelude gains or loses.
`tests/first_game_imports.rs` lists what the tutorials and reference games
import beyond it, with the reason for each.

The root glob also brings in the root modules (`audio`, `input`, `net`,
`render`, `ui`, ...). For a game that writes it and depends on a crate of the
same name, a path in code then means the Ponko module, and a
`use name::...;` declaration is ambiguous (E0659); the way out is to spell
the crate `::name::...` or to import `ponko::prelude::*` instead. A new root
module is therefore a source change for such games, so the CHANGELOG names
every root module a release adds.

## Re-exported crates

Some of Ponko's API is another crate's types, re-exported as they are, so
values pass between Ponko and code that uses the crate directly:

| Crate | Ponko's names |
|---|---|
| `glam` | `ponko::math` (`Vec2`, `Vec3`, `Vec4`, `Quat`, `Mat4`, `UVec2`, `EulerRot` and the constructors `vec2`, ...), its home; the prelude re-exports the common ones as the same items |
| `serde_json` | `ponko::reflect::JsonValue` and `JsonMap` (its `Value` and `Map<String, Value>`), `ponko::reflect::json` (the crate), `json!` in the prelude; `ponko_mcp::JsonValue`, `JsonMap` and `json!` |
| `steamworks` | `ponko::net::steam` (feature `steam`) |

Ponko promises that these names are those crates' types, not the types'
behavior: their methods and trait implementations (`Vec2::length`,
`Quat::from_euler`, `JsonValue::as_object`, ...) are the crate's and are
documented there. Ponko picks the version:

- **A compatible release** of the crate, one Cargo may select by itself, can
  come with any Ponko release, patch releases too. It may add methods and
  trait implementations, which can rarely make a method call ambiguous with
  a game's own extension trait, as any dependency update can.
- **An incompatible release** (glam 0.34 to 0.35, serde_json 1 to 2) is a
  breaking change. Before 1.0, a Ponko minor release may make it for any of
  the three, and the CHANGELOG lists it under Breaking changes. From 1.0 on,
  an incompatible `glam` or `serde_json` comes only with a Ponko major
  release; `steamworks` follows the Steam SDK, and its rule after 1.0 is
  written here before 1.0.
- A game that also depends on the crate directly names the same version
  range as Ponko (`cargo tree -i glam` shows it), or uses Ponko's names and
  no direct dependency at all.
- The re-export decides only the Rust types: saved worlds and other files
  keep their own compatibility rules ([Saved worlds](#saved-worlds)),
  whatever version of `serde_json` writes them.

## Minimum Rust version (MSRV)

Ponko builds with **Rust 1.95** and newer (`rust-version` in Cargo.toml;
the `msrv` job of `scripts/checks.yml` checks it). The MSRV may rise in a minor release (0.y.0), never in a
patch release, and the CHANGELOG says so. It follows the dependencies
(wgpu, egui, rapier) and stays at least a few releases behind stable.

## C ABI

The C ABI has its own version, `ponko_abi_version()` and `PONKO_ABI_VERSION`
in `ponko.h` (currently **2**). It changes whenever an existing function,
struct layout or enum value changes incompatibly, which only happens in a
minor release (0.y.0), never in a patch release; the CHANGELOG says so.
Within an ABI version, releases only add functions and enum values, and
existing struct layouts never change. Programs that load the library
dynamically check the version at startup:

```c
if (ponko_abi_version() != PONKO_ABI_VERSION) { /* built against another version */ }
```

A failing call returns its `PonkoStatus`, and `ponko_last_error()` has its
message. `ponko_last_error_kind()` says what kind of failure it was, as a
`PonkoErrorKind` that mirrors Rust's `ponko::error::ErrorKind` (`PONKO_ERROR_NOT_FOUND`,
`_IO`, `_DECODE`, `_SHADER`, `_INVALID`, ..., `_OTHER`); new kinds can arrive
in any release, so a `switch` needs a `default:`. Statuses don't change
with it: a call that returned `PONKO_INVALID_ARGUMENT` for a missing file
still does, and says `PONKO_ERROR_NOT_FOUND` as its kind. The C++ wrapper
throws `ponko::error`, a `std::runtime_error` with `kind()`.

New statuses can arrive like new kinds, so treat any status but `PONKO_OK`
as a failure. `PONKO_UNSUPPORTED` means this machine or build can't do it:
`ponko_game_run` returns it when the game can't start here (no usable GPU
or graphics device, no window or event loop, a build without the `window`
feature), so a launcher can show its own message or fall back. Releases
before it returned `PONKO_INVALID_ARGUMENT` there; that is the one status
that changed, and only for those startup failures.

### Versioned descriptors

ABI 2 settings structs (`PonkoMaterialDesc`, `PonkoNavBakeSettings`,
`PonkoLighting`, `PonkoCollider`, ...) have fixed layouts, so a release can't
add a field to them. Settings that will grow get a versioned descriptor
next to them, named `...Ex`, and their own entry points
(`ponko_ctx_create_material_ex`, `ponko_ctx_bake_navmesh_ex`,
`ponko_ctx_set_lighting_ex`, `ponko_set_collider_ex`, the
`ponko_ctx_*_ex` queries taking a `PonkoQueryFilterEx`,
`ponko_ctx_set_post_ex`, `ponko_ctx_set_particle_emitter3d_ex`,
`ponko_ctx_set_camera3d_ex`, `ponko_ctx_set_target_cameras_ex`). The ABI 2 structs
and calls stay as they are, and the layout checks keep them that way. Every
descriptor follows these rules:

- **Header first.** It starts with `uint32_t struct_size` and
  `uint32_t version`. The caller sets them to `sizeof` the struct and its
  `PONKO_..._VERSION` from the header it compiled against, then fills the
  fields with the struct's `..._defaults` (or `..._from`) call, which writes
  only what that version has:

  ```c
  PonkoLightingEx l = {.struct_size = sizeof l, .version = PONKO_LIGHTING_EX_VERSION};
  ponko_ctx_lighting_ex(ctx, &l);
  l.shadow_sun_size = 1.5;
  ponko_ctx_set_lighting_ex(ctx, &l);
  ```

- **Fields only grow at the end.** A new version appends fields after the
  last one and bumps `PONKO_..._VERSION`; existing fields never move or
  change meaning. An ABI 2 struct embedded in a descriptor (its first field
  after the header) stays that struct.
- **Old callers get defaults.** The library reads and writes only the
  fields of the caller's `version`: fields added later get their defaults
  when it reads (the engine's default, or for a `set` call the current
  value, as each call documents), and are left alone in the caller's memory
  when it writes. A newer caller's larger struct is fine: the library
  ignores the fields past the versions it knows.
- **Refused, not guessed.** `version` 0 (the header wasn't set), a version
  newer than the library knows (the program needs a newer library), a
  `struct_size` smaller than its version's fields, and null fail with
  `PONKO_INVALID_ARGUMENT` and a message saying which; nothing changes.
- **No retained pointers.** A descriptor, and anything it points to, only
  has to live for the call: the library copies what it keeps.

- **A list is a list of pointers.** An array of descriptors would need a
  stride every caller agrees on, so a call that takes several
  (`ponko_ctx_set_target_cameras_ex`) takes an array of pointers to them. A
  plain struct that only holds one (`PonkoPostVolume`) points at it.
- **Every new Rust settings field gets a C counterpart, or a reason.**
  `scripts/check-settings-fields.py` fails for a public field of a listed Rust
  settings struct (`scripts/settings-fields.txt`) that has no field of the
  same name in its C struct or descriptor, no line naming where C sets it, and
  no entry on `scripts/rust-only.txt`.

A future ABI version may make the `...Ex` forms the only ones and drop the
suffix; until then both stay.

### Planned for ABI 3

What the next C ABI version changes is written down before it ships, so
programs can prepare:

- **Handle ids carry their kind.** Ids grow to 64 bits: the engine's id in
  the low 32 bits, `PonkoHandleKind` plus one in the top 8, 0 there meaning
  "not tagged" (so `{0}` keeps meaning the built-in font, texture and
  material). Calls check the kind of the ids they are given. The handles
  chapter of the guide, "The kind in the id, from ABI 3", has the details
  and why the kind doesn't fit in 32 bits.

The C and C++ names stay flat (`ponko_ctx_cast_ray_filtered`,
`ctx.graphics_quality()`, ...) even where the Rust API groups things into
modules and services.

## Saved worlds

`World::save` writes JSON tagged `"format": "ponko-world"` and a save version
(currently 1). A release reads every save version that earlier 0.x releases
wrote; a release that changes the format says so in the CHANGELOG and still
loads the older versions. Your own components are yours to migrate: give a
component `#[serde(default)]` and saves written before you added a field
still load.

The binary save format (`SaveFormat::Binary`) holds the same save, so the
same promise covers it, and it starts with a layout number of its own
(currently 1): a release reads every layout earlier 0.x releases wrote. JSON
saves keep loading next to it.

`World::load` refuses entity ids whose index is far beyond what the save
needs (the exact bound is in its docs), so a corrupt or hostile file can't
make the world allocate a huge entity table. Saves the engine writes stay
within it unless the world once had over 4 million entities alive and was
saved with under an eighth of them; such a save still loads back into the
world that wrote it.

## Compatibility checks

The promises above are tested against retained baselines, not only against
what the current revision generates. A baseline is a small fixture recorded at
a named engine revision with its provenance next to it; it is never edited to
make a test pass.

| Contract | Baseline | Check |
|---|---|---|
| Saved worlds, scenes and prefabs | `tests/fixtures/saves/` | `tests/save_compat.rs`, `tests/prefab_compat.rs` |
| MCP tools and parameters | `crates/ponko_mcp/tests/fixtures/` | `crates/ponko_mcp/tests/contract.rs` |
| C ABI header and layouts | `crates/ponko_ffi/tests/fixtures/abi/` | `crates/ponko_ffi/tests/abi_compat.rs`, `examples/cpp/abi_baseline.c` |

Additions pass: new MCP tools and optional parameters, new C functions and enum
constants. A removed or changed tool, parameter, function or struct layout, a
new required MCP parameter, or a parameter that accepts fewer values fails with
a message naming it. Each fixture folder's README says how to retain a new
baseline after an intentional breaking change.

## Determinism

What gives the same world from the same seed and inputs:

| | Same machine, same build | Other OS or CPU, same build | Other engine version |
|---|---|---|---|
| ECS, scheduling, `ctx.random()` | yes | yes, see below | not promised |
| Your sim code with `+ - * /` and `sqrt` on `f32`/`f64` | yes | yes | yes |
| `sin`, `cos`, `powf`, `exp`, ... | yes | **may differ** | — |
| Physics (`physics`) | yes | **no** | not promised |
| Physics (`physics-determinism`) | yes | yes | not promised |

- IEEE-754 makes basic arithmetic and `sqrt` bit-identical everywhere, but
  the standard library's `sin`, `cos`, `powf` and friends call the platform's
  math library, which may round differently. For lockstep between
  different platforms, keep them out of the simulation or use a pure-Rust
  implementation such as the `libm` crate.
- Lockstep peers, and replays, must use the same game build and engine
  version. Lockstep packets carry a protocol version, and a peer with
  another version is ignored and times out.
- Iteration order of queries is deterministic but not part of the API: it
  can change between versions.
- `Game::parallel_systems` doesn't change any of this: a simulation gives
  the same world with it on or off and on any number of threads, so
  lockstep peers may differ in that setting.
- Entity ids are part of that world, also the ones `commands.spawn`
  reserves: the same spawns in the same order get the same ids, with the
  scheduler on or off and on any number of threads. Not inside a parallel
  query (`par_for_each`): spawns recorded from its threads get distinct
  ids and are all applied, but which item gets which id depends on timing.
  To keep ids deterministic there, collect with `par_collect_into` and
  spawn in its order.

## Platforms

The list of checks (`scripts/checks.yml`, run locally by maintainers; there
is no CI) covers Linux x86-64, macOS (Apple Silicon) and Windows x86-64, and
builds the web (wasm32, WebGPU with a WebGL2 fallback); small web builds (2D,
3D and the complete-game reference) are also run in headless Chromium and
WebKit with WebGL2 (`web/smoke/`; Firefox is tried there too, but headless
Firefox on a GPU-less Linux machine has no WebGL2, so it is reported, not
verified). WebGPU is tried there too and reported, but a machine without a
GPU usually can't show it, so WebGPU is verified on real GPUs by hand. Other
targets that wgpu and winit support usually work but aren't tested.
