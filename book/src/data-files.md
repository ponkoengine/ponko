# Data files: tune without restarting

Keep the numbers you tweak (speeds, damage, costs, wave sizes) in a JSON
file in `assets/` instead of in code. With [hot reload](drawing.md#hot-reload)
on (the default in debug builds), saving the file changes them in the
running game, within a quarter of a second, so you can tune a
fight while watching it.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/hot_reload.rs:data_files}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:data_files}}
```

</div>
</div>

In Rust the file becomes a resource: any type that implements
`serde::Deserialize`, replaced whole on each reload (use `#[serde(default)]`
for optional fields). In C and C++ the resource is a plain struct described
with `reflect_resource`, and the file's fields merge into it, so the file
may list only some of them (C: `ponko_ctx_load_resource_json`). C++ can
also load a file into a component of one entity: `ctx.load_json<T>(path, e)`.
Rust has the same merging forms for reflected types, with the name taken from
the type: `ctx.assets.load_reflected_resource_json::<T>(&mut ctx.world, path)` and
`ctx.assets.load_reflected_component_json::<T>(&mut ctx.world, path, entity)`.

- Loading fails with a clear message: a missing file lists the folders
  searched, and bad JSON says the line and column.
- A file that doesn't parse while the game runs (often one caught halfway
  through saving) keeps the old values and prints why; the next good save
  applies.
- `ctx.assets.reloaded_files()` names the files reloaded this frame, textures
  included, for work that depends on them (rebuilding a map, say). In C++,
  `ctx.was_reloaded("map.json")` (C: `ponko_ctx_was_reloaded`).
- Headless runs reload too, so an agent or a test can change a file
  between ticks.
- Files can also come from somewhere other than `assets/` (embedded in the
  executable, say); see [Asset sources](asset-sources.md). Those are not
  hot reloaded.

## Loader failure policy

Every `load_*` follows one of two rules, so you can tell from the name what a
missing or broken file does:

- **Setup loaders** (`load_x`) are for code that runs once at the start,
  where a missing file is a bug: they panic with a message that starts with
  `ponko:`, and say what and where. Each has a `try_load_x` twin with the same
  arguments that returns `Result<_, Error>`; match on `error.kind()` to
  handle a file that may be absent. This covers textures, meshes, sounds,
  shaders, the typed data file `load_resource::<R>` (a failed
  `try_load_resource` leaves the old resource alone), string tables
  (`load_strings`) and collision meshes (`load_collision_mesh`,
  `load_collision_mesh_with`).
- **Runtime operations** return a `Result` under their plain name and have no
  panicking twin, because they fail in normal play: loading a scene, world or
  level, `load_with`'s custom decoder, and the reflected JSON loaders
  `load_reflected_resource_json` and `load_reflected_component_json` (the
  entity may be gone, the type may not be reflected). The reflected loaders
  merge a file's fields into what is there; `load_resource` replaces the whole
  resource.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/data_loader_convention.rs:loader_failure}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:loader_failure}}
```

</div>
</div>

C has no panics, so its data-file calls are the `try_` forms: they return a
status and `ponko_last_error_kind()` / `ponko_last_error_path()` say what
failed (`ponko_ctx_load_strings`, `ponko_ctx_load_collision_mesh`,
`ponko_ctx_load_resource_json`, `ponko_ctx_load_json`). In C++ `load_x` throws
`ponko::error` (the counterpart of the panic) and `try_load_x` returns a
`ponko::result` (`try_load_strings`, `try_load_collision_mesh`,
`try_load_json<T>`). Rust's typed `load_resource::<R>` has no native twin: `R`
is any type that implements `serde::Deserialize`, and C and C++ have no way to
hand the engine a deserializer. The native equivalent is the reflected
resource loader (`ponko_ctx_load_resource_json`, `try_load_json<T>`), which
merges the file into a struct described with `reflect_resource`.

Each public Rust `load_*` and its failure behavior is in the table below
(feature availability varies). Here `Error` is `ponko::error::Error`; runtime
rows describe load failures, not user callback panics or unrelated programmer
invariants. A test walks the public API snapshots and fails when a new `load_*`
breaks the rules or is missing from this table.

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

## Reading a failure

Keep the plain setup loader when a missing file is a programming mistake,
and use its `try_` twin for optional files; match `ErrorKind`, not the
message text. [When something fails](errors.md) shows the messages for
missing files, unregistered components and the other common mistakes.
