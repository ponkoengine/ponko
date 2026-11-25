# Handles: freeing what you made

> **Maturity: [tested](maturity.md).** Game: none. Tests: `tests/handles.rs`,
> `tests/handle_layout.rs`, `tests/handle_compat.rs`, `handle_render_test.rs`.
> Platforms: Linux, macOS and Windows, headless (the workspace tests in the
> list of checks). Graphics: offscreen render tests, required on Windows
> (DirectX 12 on the WARP software adapter) and run on Linux and macOS only
> where a GPU is found. Missing: a reference game that uses it.

Textures, meshes, materials, sounds and the other things the engine keeps
for your game come back as small handles: plain `Copy` values, passed by
value (`ctx.gfx.draw(crab, pos)`, `ctx.audio.play(hit)`). Only the engine makes
them: a number can't be turned into a handle, so a handle always names
something your game made. Every kind works the same way:

| Question | Rust | C++ |
|---|---|---|
| Is it still valid? | `ctx.is_live(h)` | `ctx.is_live(h)` |
| Give the memory back | `ctx.free(h)` | `ctx.free(h)` |
| An empty handle | `Texture::NONE`, `Mesh::NONE`, ... | `ponko::none<ponko::texture>()` |
| How many are live? | `ctx.live_counts()` | `ctx.live_counts()` |

The kinds are `Texture`, `Sound`, `Font`, and with the `3d` feature
`Mesh`, `Material`, `Shader`, `Compute`, `Effect`, `GpuBuffer`,
`InstanceBatch`, `TextureArray`, `TerrainTile` and `RenderTarget` (with
`physics3d`, `CollisionMesh` too). Each is its own type (see [Each kind is
its own type](#each-kind-is-its-own-type)). They implement the `Handle`
trait, so a helper can take any of them.

The service that holds a handle answers the same questions:
`ctx.gfx.is_live(h)`, `ctx.gfx.free(h)` and `ctx.gfx.live_counts()` for
everything drawn (the `GfxHandle` trait), `ctx.audio.is_live(s)` and
`ctx.audio.free(s)` for sounds, and `ctx.physics3d.is_live(m)` and
`ctx.physics3d.free(m)` for collision meshes. `ctx.is_live` and `ctx.free`
take any kind and forward to them.

A game that streams its world frees what each cell made when the cell goes
out of range:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/handles.rs:handles}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:handles}}
```

</div>
</div>

## The rules

- **No handle is ever given out twice in a run**, so a freed handle stays
  dead: it never starts naming something made later, and a stale copy kept
  somewhere can't quietly point at the wrong thing. Meshes, GPU buffers and
  collision meshes are made and freed all the time in streamed worlds, so
  the next one made takes a freed one's slot in the engine's tables (which,
  with the per-frame work over every mesh, then don't keep growing), but
  under a new handle that counts how often its slot was given out. After
  4095 times the slot is retired instead of counting from zero again, so a
  stale handle stays dead for good.
- **Freeing twice, or freeing `NONE`, does nothing.** `NONE` is never live.
- **Only what the game made can be freed.** The built-in font, the default
  material's internals and the engine's own textures (shadow maps, the font
  atlas) aren't the game's to free; freeing `Font::DEFAULT` does nothing.
- **Nothing is reference counted.** Freeing is explicit, and the handle you
  hold doesn't keep anything alive.

## Freeing something that is still used

Freeing a resource others still use is allowed, and what they do then is
defined. Nothing panics and nothing reads freed memory:

| Freed | What still uses it does |
|---|---|
| Mesh | A model showing it draws nothing, like `Mesh::NONE`. An instance batch drawing it (or one of its levels) draws nothing from the next frame on, never the mesh made later in the freed one's slot; free the batch first. |
| Texture | Materials and sprites sample white, as if they had no texture. A freed normal, ORM or emissive map is ignored. |
| Material | Meshes draw with the default material. |
| Shader | Materials using it draw with the built-in shader. |
| Texture array | Materials sample a white array; decals using it are turned off. |
| Render target | Cameras, compute passes and 3D panels drawing into it are skipped. |
| Effect, compute pass | They are left out of the frame. |
| GPU buffer | Its compute passes are dropped; meshes drawing from it draw nothing. |
| Sound | Its playing voices stop; playing it again does nothing. |
| Font | Text in it uses the built-in font. |

Updating a freed handle (`update_texture`, `update_material`) does nothing,
and a hot-reloaded file no longer touches it. Loading a freed file again
(`load_mesh`, `load_texture`) loads a fresh copy with a new id.

## Loaded files and their caches

What loads by name is cached by name: loading the same file again returns
the same handle and reads nothing. Freeing the handle also forgets the
cache entry and stops hot reload watching the file, so the next load reads
it afresh. Hot reload replaces an edited texture, sound, font or shader in
place, under the same handle, without making anything new.

A glTF model is more than its mesh. `load_mesh` also makes the file's
materials and the textures they use, and they outlive the mesh:

- **Freeing the mesh** keeps the file's materials and textures, with any
  edits made with `update_material`, for the next `load_mesh` of the same
  file. A level that loads and frees the same models every time stays flat.
- **Freeing one of those materials or textures** and loading the file again
  makes only what was freed again; the rest are reused.
- **`ctx.gfx.unload_mesh(path)`** frees everything loading the file made:
  its mesh, its materials and its textures, except images another loaded
  file shares. Use it in a long-running game that loads many different
  models over time; it returns false when the file wasn't loaded.

To hide something for a while, show `Mesh::NONE` instead of freeing it, and
show the real mesh again later. Free when you won't need it again.

## Each kind is its own type

A mesh is not a texture, and a texture is not a render target: every kind
(a sky shader too) is its own type, so passing the wrong kind is a compile
error instead of a wrong picture at run time. `ctx.gfx.create_render_target(w, h)` returns
a `RenderTarget`, and what draws into one (`TargetCamera::new` and
`TargetCamera::reflection`, `ComputePass::new`, `TargetEffect::new`,
`d3::UiPanel::new`) takes only that, so an image loaded from a file can't
end up where a camera should draw. Where a texture is read (a material, a
sprite, a draw call), `target.texture()` reads the target as the texture
it draws into:

<div class="lang-pair">
<div>

```rust
# fn setup(ctx: &mut Ctx) {
use ponko::render::{MaterialDesc, RenderTarget, TargetCamera};
let screen: RenderTarget = ctx.gfx.create_render_target(320, 180);
ctx.scene3d.passes.target_cameras.push(TargetCamera::new(ctx.scene3d.camera, screen));
// A material reads it as the texture it draws into.
let monitor = ctx.gfx.create_material(MaterialDesc::default().with_emissive_map(screen.texture()));
// An image is not a render target: this doesn't compile.
//     TargetCamera::new(ctx.scene3d.camera, ctx.gfx.load_texture("crab.png"));
# let _ = monitor;
# }
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:typed_handles}}
```

</div>
</div>

The types cost nothing: a handle is its id (a texture also carries its
size, a sound its length), as small as a plain number, and a
`RenderTarget` is the `Texture` it wraps, with the same size and layout.
`tests/handle_layout.rs` checks it. Scenes and saves store handles by
the asset's name, never by type or number, so files written before a
kind got its own type load as they did.

A render target is a texture too, so the two live and die together:
`ctx.is_live(target)` and `ctx.is_live(target.texture())` agree, and
freeing either frees both. Freeing a texture's handle as a render target
does nothing unless it is one.

**Moving to `RenderTarget`** (render targets were `Texture`s before
0.2.0). The change is mechanical:

- Keep what `create_render_target` returns (or `try_create_render_target`,
  or a `d3::UiPanel`'s `target`, which was `texture`) as a `RenderTarget`,
  and take one in helpers that hand it to the calls above.
- Where it was passed as a texture (`with_custom_textures`,
  `with_texture`, `with_emissive_map`, `Sprite::new`, `ctx.gfx.draw`),
  write `target.texture()`.
- A `ComputePass` writing a GPU buffer has `RenderTarget::NONE` as its
  target.

```rust
# use ponko::render::{ComputePass, MaterialDesc};
# fn setup(ctx: &mut Ctx, heights: ponko::render::Compute) {
// Before: let water = ctx.gfx.create_render_target(256, 256);  // a Texture
//         MaterialDesc::default().with_custom_textures([Some(water), None])
let water = ctx.gfx.create_render_target(256, 256); // a RenderTarget
ctx.scene3d.passes.compute.push(ComputePass::new(heights, water));
let look = MaterialDesc::default().with_custom_textures([Some(water.texture()), None]);
# let _ = look;
# }
```

## Counting

`ctx.live_counts()` returns how many handles of each kind are live. Compare
two counts to find a leak: a level that loads and unloads should end where it
started.

```rust
let before = ctx.live_counts();
// ...load and unload a level...
assert_eq!(ctx.live_counts(), before);
```

Textures, render targets, sounds, fonts, meshes, GPU buffers, collision
meshes and instance cells give a freed slot out again under a new id, so a
game that makes and frees them all session keeps tables the size of the
most it held at once; the freed handle stays dead. Materials and the other
kinds never give out an id twice, and each one ever made keeps a small
table entry. "Very long sessions" in
[the simulation clock](sim-clock.md#very-long-sessions) has the numbers.

## Finding a leak

`ctx.asset_inventory()` counts the same handles by class with estimated
bytes (its texture count also includes the engine's own textures), and its
`dangling` list names the sprites and models that still show a freed
texture, mesh or material. The control endpoint's `perf.get` includes it as
`assets`. Take one as a baseline, run the game for a while, and
`growth_since(&baseline)` names the classes that grew, the biggest first.

`ctx.gfx.asset_handles(class)` (for sounds `ctx.audio.asset_handles()`)
then lists every live handle of that class,
oldest first, with its bytes, the frame it was made on, its age in frames
and its owner: game code, the asset file that loaded it (for a glTF model's
materials and textures, the model's file), or the engine. Handles that keep
getting older while the class grows are the ones nothing frees.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/asset_inventory.rs:asset_leaks}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:asset_leaks}}
```

</div>
</div>

Both read the engine's own handle tables, so they also work in headless
tests; a windowed game's inventory adds `gpu`, what the renderer really
holds, to cross-check them, and `texture_alpha`, the CPU memory it keeps
about textures' alpha for cutout shadows (a small summary per texture, and a
full mask only for textures a cutout material reads). Leak tests in the engine load, free, replace
and hot-reload assets in loops and expect `growth_since` to stay empty.

## The older names

The per-kind calls (`free_mesh`, `free_instance_batch`, `has_material`,
`has_shader`, `has_compute`, `has_effect`, `has_font`, `is_mesh_live`) were
removed in 0.1.0: use `ctx.gfx.free(h)` and `ctx.gfx.is_live(h)`. `gpu_buffer_len`, `instance_batch_len`,
`has_texture_array` (which also checks the size) and `is_render_target` stay: they answer more than whether a handle is live.

## Handles of several meshes

A `TerrainTile` holds one mesh per detail level. It is a handle too:
`ctx.is_live(tile)` while all its meshes are, and `ctx.free(tile)` frees
them all. `tile.lods()` gives the meshes, to set their shadows or look up
their cost.

## In C

C passes the kind and the id:

```c
{{#include ../../examples/cpp/guide.c:handles}}
```

An unknown kind is an invalid argument. `PONKO_HANDLE_NONE` is the empty id
for every kind. C API calls that take a handle report a freed one as an
invalid argument.

C keeps one struct per kind with an integer id, so the compiler can't stop
an id of one kind being passed as another; the kind is checked at run time
instead. A render target is a `PonkoTexture`, and its own kind,
`PONKO_HANDLE_RENDER_TARGET`: `ponko_ctx_handle_is_live` with that kind
answers whether a texture is a live render target, freeing a plain texture
as one is an invalid argument, and the calls that draw into a target (a
target camera, a reflection camera, a compute pass, a target effect) refuse
a plain texture:

```c
{{#include ../../examples/cpp/guide.c:render_target_kind}}
```

Ids of different kinds can be equal, so the generic calls trust the kind
they are given: free a mesh as `PONKO_HANDLE_MESH`. (ABI 3 puts the kind
in the id; see [below](#the-kind-in-the-id-from-abi-3).)

C++ gives each kind its own type again: `ctx.create_render_target` returns
a `ponko::render_target`, which converts to `ponko::texture` (and has
`texture()`) wherever a texture is read, while `target_camera_of`,
`compute_pass_of`, `target_effect_of` and `ctx.reflection_camera` take only
a `render_target`. A target read back from the engine (a target camera's
`target`) is a plain texture; `ctx.render_target_of(t)` checks that it is
a render target and throws `ponko::error` if not.

The inventory takes a class (`PonkoAssetClass`, which counts the engine's
textures and lists render targets on their own) instead of a kind:

```c
{{#include ../../examples/cpp/guide.c:asset_inventory}}
```

### The kind in the id, from ABI 3

ABI 2 keeps the kind next to the id because a C id is 32 bits, all of them
used (20 for the slot, 12 for the generation that makes a freed handle stay
dead), and ABI 2 promises those values and layouts. The next C ABI version
(ABI 3) puts the kind into the id. This is decided:

- **Ids grow to 64 bits.** Every handle struct holds a `uint64_t id`. Its
  low 32 bits are the engine's id as today (slot and generation), so a slot
  still comes back as the same handle only after 4096 frees.
- **The top 8 bits are the kind**: `PonkoHandleKind` plus one. Bits 32 to
  55 are zero; an id with any of them set is an invalid argument.
- **Every id the library hands out carries its kind.** A call given an id of
  another kind fails with `PONKO_INVALID_ARGUMENT`, naming both kinds, so
  passing a mesh as a material is caught where it happens. The generic calls
  (`ponko_ctx_handle_free`, `ponko_ctx_handle_is_live`) read the kind from
  the id and lose their `kind` argument.
- **Kind 0 means "not tagged"**: the call takes the id as the kind it
  expects. `{0}` still means the built-in font, the white texture and the
  default material, and a tagged id round-trips through code written for ABI
  2 that only stores and compares ids.
- `PONKO_HANDLE_NONE` becomes `UINT64_MAX`.

Why not four bits of kind in the 32-bit id (kind 4, generation 8, slot 20),
the smaller change sketched before: four bits name 16 kinds, and the engine
already has 14 C kinds plus crowds, instance cells, collision meshes, sky
shaders and terrain tiles waiting for theirs; and eight bits of generation
would bring a freed handle back to life after 256 frees of its slot. Eight
bits of kind leave room for 255, and the generation stays as strong as
Rust's.

What it costs: a handle is 8 bytes in C instead of 4, so the structs that
embed handles (`PonkoSprite`, `PonkoModel`, the 3D descriptors) change
layout. That is what an ABI version is for: ABI 3 changes them all at once,
with the versioned descriptors' `...Ex` forms (see STABILITY.md). Moving a
program over is a rebuild against the new header, plus widening any place
that keeps an id in a `uint32_t` of its own. C++ handles are typed already;
their ids widen with them and C++ code changes only where it spells out
`std::uint32_t`. Until ABI 3 the kind travels next to the id.
