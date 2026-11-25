# Importing from Unity

> **Maturity: [tested](maturity.md).** Game: none. Tests: the `ponko_unity`
> crate's tests. Platforms: Linux, macOS and Windows, headless (the workspace
> tests in the list of checks). Graphics: not involved. Missing: a game built
> from an imported project.

`ponko_unity` reads a Unity project and turns its scenes and prefabs into
Ponko [scenes](scenes.md): every drawn mesh becomes a `.glb` with its
materials, its textures become PNGs, and the scene is a `ponko-world`
file of `d3::Transform` and `Model` entities that `load_scene` loads. It
runs once, as a tool, and writes into your game's `assets/`.

```sh
cargo run --release -p ponko_unity --example import -- \
    MyUnityGame my_game/assets/town town/ Assets/Scenes/Town.unity
```

That writes `my_game/assets/town/Town.json` (load it with
`ctx.load_scene("town/Town.json")`), the meshes in `town/meshes/` and their
textures in `town/meshes/textures/`, then says what it left out and which
shaders the materials used. Give it several scenes or prefabs at once and
they share their meshes and textures; `ONE=all.json` puts all their
entities in one scene.

To look around what came out:

```sh
SCENE=town/Town.json cargo run --release --features 3d --example scene_viewer
```

Unity's +Z is Ponko's -Z: everything is mirrored along Z on the way, so a
scene looks the same, and positions and rotations come out in Ponko's
space.

## What comes over

- **Meshes** from mesh assets (`.asset`), FBX and OBJ files (scaled and
  oriented as Unity's model importer does), and Unity's built-in Cube,
  Plane and Quad. Each submesh keeps its material. A model imported with
  **Bake Axis Conversion** has its file's axes (Z-up, say) turned into
  Unity's in the mesh itself, as Unity does, so parts placed one by one
  in a prefab line up.
- **Materials** of URP's Lit, Simple Lit and Unlit shaders, and of other
  shaders that use the same property names: base color and map, normal
  map, metallic and smoothness (packed into glTF's occlusion, roughness
  and metalness image), emission, tiling, cutout, transparency and
  two-sided, plus occlusion. A custom shader is drawn as plain PBR by the
  same rules; the report lists the shaders used, so you know which need a
  Ponko [shader](3d.md) of their own.
- **Textures** from image files, at most as large as their import settings
  allow, and from texture assets saved uncompressed.
- **Prefabs** inside prefabs, with their overrides, baked into one flat
  list of entities placed in the world.
- **Colliders**, if you ask (`importer.colliders(true)`): box, sphere,
  capsule and mesh colliders become entities with a fixed `d3::RigidBody`
  and a `d3::Collider` ([3D physics](physics3d.md)), the object's scale
  worked into the shape. A mesh collider's mesh is written as a `.glb` and
  named in the scene by file; `ctx.load_scene` makes a triangle mesh of
  it (a convex hull for a convex one), once per mesh and scale, and saving
  the scene names the file again. Triggers are left out.

Vertex colors are kept only for shaders you name (see below): URP's
shaders ignore them, so most meshes are drawn as Unity draws them without.

Renderers keep their Cast Shadows setting: Off gets a `ShadowMesh` of
none, and Shadows Only (often a simplified stand-in for the shadows of the
pieces around it) becomes a `ShadowMesh` with no `Model`, drawn only into
the shadow maps.

### Levels of detail

A Unity `LODGroup` comes over as a [`LodGroup`](3d.md) on one entity placed
at the group's object. Every level keeps its renderers (each placed
relative to the group), its screen-relative height as the level's
`screen_height`, and its `fadeTransitionWidth` as `fade_width` when the group's
Fade Mode is on. The group's `m_Size` is the `size` and its reference point
the `center`. A renderer's Cast Shadows setting becomes the level's shadow:
a level with a renderer set to Off or Shadows Only casts the shadow of
the right meshes, not the ones it shows.

```rust
# let project = ponko_unity::Project::open("Project")?;
# let mut import = ponko_unity::Importer::new(&project, "out", "");
import.lod_groups(false); // LOD 0 only, the far levels left out
```

Animated cross-fading (a fade over seconds) isn't supported: those groups
fade over a band of screen heights instead, and the report counts them.
Skinned renderers can't be part of a group: one in LOD 0 stays an entity of
its own, and the ones in far levels are left out (also counted).

FBX and uncompressed Unity `.asset` skinned meshes export inverse bind
matrices and up to four influences per vertex (weights below 0.001 are
dropped, then the largest four are normalized). Mesh assets support legacy
bone weights and VertexData blend channels. Renderer bone references use
the flattened scene's names, hierarchy and overridden rest transforms when
all joints resolve; otherwise the source skeleton is retained (mesh assets
use unnamed bind-pose joints). Left out, and counted in the report: skinned renderers in far levels of detail, inactive objects and disabled renderers, the built-in
Sphere, Capsule and Cylinder, compressed meshes, and meshes or textures
it can't find or read.

## Your own shaders for imported materials

Materials of the project's own shaders come over as plain PBR. To draw
them with a Ponko [shader](3d.md) of your own, tell the importer which
shaders those are:

```rust
# let project = ponko_unity::Project::open("Project")?;
# let mut import = ponko_unity::Importer::new(&project, "out", "");
import.custom_shader("Assets/Shaders/Wall.shader"); // as the report lists it
```

Their meshes then keep their vertex colors and UV set 1 (which custom
shaders often read; in Ponko's shaders they're `s.color` and `s.uv1`,
UV set 1 unflipped, as Unity has it), and the importer writes
`materials.json` next to the scenes: each such material by name, with its
shader and every number, color and texture it sets, the textures written
as PNGs. In the game, `loaded_materials(name)` gives every material loaded
from glTF files under that name, with what it was made from, so you can
keep that and change the shader:

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/three.rs:loaded_materials}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:loaded_materials}}
```

</div>
</div>

Call it after the scene (or model) is loaded: models loaded later aren't
in it yet. C: `ponko_ctx_loaded_materials`.

A shader that reads something else per vertex, such as a second UV set,
can have it put in the vertex colors instead, which is what Ponko's
shaders get per vertex besides one UV set:

```rust
# let project = ponko_unity::Project::open("Project")?;
# let mut import = ponko_unity::Importer::new(&project, "out", "");
// A river shader with its flow in UV set 1: speed up to 4, foam 0 to 1.
import.vertex_colors("Assets/Shaders/River.shader", |mesh, v| {
    let [speed, foam] = mesh.uvs[1].get(v).copied().unwrap_or_default();
    [speed / 4.0, foam, 0.0, 1.0]
});
```

Each channel keeps 8 bits, from 0 to 1, so scale values into that range
and back in the shader.

## Model animation

Skinned FBX models export their takes as glTF animations targeting the same
nodes as the skin. Local translation, quaternion rotation and scale are
sampled at the file's frame rate, including clip endpoints and source key
times. Constant tracks use STEP interpolation; other sampled tracks use
LINEAR. The axis conversion, import scale and Z mirror match the rest pose.

The model's `.fbx.meta` `clipAnimations` entries select a take (`takeName`)
and slice `firstFrame` through `lastFrame` into named clips. Each clip starts
at zero seconds. Without entries, every take exports under its take name.
Loop flags are preserved as animation `extras.loop`; the player chooses
whether to repeat playback. Renderer bone references preserve joint order,
and `m_RootBone` supplies the glTF skeleton root. Missing bone references
retain the unresolved model bones while applying the resolved scene bones.

Standalone `.anim` clips (text or binary serialized) also import:

```rust
# let project = ponko_unity::Project::open("Project")?;
# let mut import = ponko_unity::Importer::new(&project, "out", "");
let clips = [ponko_unity::AnimationClip::read("Assets/Walk.anim")?];
import.animation_clips(clips); // before importing scenes / exporting cached meshes
```

Clips target each exported skin's root (empty path) and child names along
slash-separated Transform paths. Unresolved or ambiguous paths are reported.
`AnimationClip::append_to_skin` also attaches clips directly to a `MeshSkin`
in Unity space. The importer uses the same glTF writer and Z mirror as FBX.
Quaternion, position, scale and Euler curves use cubic Hermite sampling at
`m_SampleRate`, including keys and endpoints; Euler rotations use Unity's
Z-X-Y order. Clip settings select start/stop times (output starts at zero)
and preserve the loop flag. Infinite tangents evaluate as steps; fully step
tracks use glTF STEP. Mixed step/smooth tracks use sampled LINEAR, so jumps
between samples are approximated. Weighted tangents are approximated as
unweighted Hermite and reported.

### Which curves a clip uses

A clip saved by the editor usually carries two copies of its animation: the
curves you authored (`m_PositionCurves`, `m_RotationCurves`, `m_ScaleCurves`,
`m_EulerCurves`, the editor's per-property `m_EditorCurves` such as
`m_LocalPosition.x` and `m_EulerEditorCurves`, and blend-shape float curves)
and the runtime data Unity bakes from them (`m_MuscleClip` with
`m_ClipBindingConstant`). When there are authored curves, the importer uses
them and ignores the baked copy. Properties that a per-property curve leaves
out keep their rest pose: a clip that only animates `m_LocalPosition.x`
leaves y and z alone. Where plain transform curves and per-property curves
describe the same target, the plain ones win.

A clip without authored curves is decoded from the baked data:

- **Streamed**, **dense** and **constant** curves: cubic keys per time,
  samples at a fixed rate (linear between frames), and single values. Their
  channels are bound to targets through the clip's `genericBindings`: the
  CRC-32 of a transform's slash-separated path (below the node the clip is
  applied to) and an attribute (position, rotation, scale or Euler angles).
  Hash and node must match exactly; unmatched or ambiguous paths are
  reported.
- **Compressed** rotation curves (`m_Compressed`, bit-packed keys).
- **Blend shapes** in baked data are only identified by a hash of their
  name. The importer matches it against the blend shapes of the mesh being
  exported (`AnimationClip::with_blend_shape_names` does the same for
  your own use); unmatched ones are reported.

Binary serialized clips (Unity's *Mixed* or *Binary* asset serialization,
2022 or newer, saved by the editor with type trees) read into the same
fields, so they behave like their text twins. `AnimationClip::read` picks
the format; `parse` takes text and `parse_binary` bytes.

What does not import: **humanoid** clips whose curves are muscles and root
motion need retargeting, so a clip with only those is an error, and one that
mixes them with transform curves imports the transform curves and reports the
rest. Float curves on anything but blend shapes, object reference curves,
weighted tangents (approximated), animation events and Animator controllers
are skipped and reported or not imported yet. The compressed-curve tangent
layout and the baked decoders are checked against synthetic data built from
the formats' public descriptions, not against every Unity version's output.

## Terrain

Unity terrain (`TerrainData`, text or binary) reads as a
`UnityTerrain`: its heights in world units, already in Ponko's row order,
ready for a [terrain tile](3d.md), and the layers painted on it with
their weight maps.

```rust
# let ground = ctx.gfx.default_material();
let file = ponko_unity::UnityFile::read("MyUnityGame/Assets/Terrain/Hills.asset")?;
let data = file.of_class(156).next().expect("a TerrainData");
let hills = ponko_unity::UnityTerrain::from_object(data)?;
// In the game (or written to a file the game reads):
let tile = ctx.gfx.create_terrain_tile(&hills.heights, hills.size, ground);
let [x, y, z] = hills.corner([0.0, 0.0, 0.0]); // the Unity Terrain's position
ctx.world.spawn((ponko::d3::Transform::at(vec3(x, y, z)), tile));
```

The weight maps are `Texture2D`s in the same file: read them with
`UnityTexture` and their rows already line up with the tile.

## When an import fails

Every importer call that can fail returns `ponko_unity::Error`. Don't read
its text to decide what to do: ask it. `error.kind()` is an `ErrorKind`:

| Kind | Means | Example |
|---|---|---|
| `Io` | a file or folder couldn't be read or written | the file is missing |
| `Decode` | the data isn't well formed | bad YAML, a truncated binary asset |
| `Unsupported` | well formed, but something the importer doesn't handle | an asset older than Unity 2022, compressed meshes and textures |
| `Invalid` | it parses but doesn't make sense | a missing field, a reference to nothing, prefabs nesting in a loop, a folder with no `Assets` |
| `Other` | anything else | |

`error.path()` is the file when one is known, `error.line()` the line of a
text file, and `error.message()` the cause without either; printing the error
gives all three. For an `Io` error, `std::error::Error::source` is the
`std::io::Error`, so its `kind()` (`NotFound`, `PermissionDenied`, ...) is
there too. `ErrorKind` is `#[non_exhaustive]`: keep a `_` arm.

```rust
use ponko_unity::{ErrorKind, UnityFile};

match UnityFile::read("MyUnityGame/Assets/Scenes/Town.unity") {
    Ok(file) => println!("{} objects", file.objects.len()),
    Err(e) if e.kind() == ErrorKind::Io => println!("can't open {:?}", e.path()),
    Err(e) if e.kind() == ErrorKind::Unsupported => println!("resave it in a newer editor: {e}"),
    Err(e) => println!("{e}"), // Decode and Invalid name the line when they know it
}
```

## From C and C++

The importer is a Rust tool: run it once, as above. What it writes are
ordinary scene, glTF and PNG files, which C and C++ games load with
`ctx.load_scene(path)` (C: `ponko_ctx_load_scene`) like any other.
