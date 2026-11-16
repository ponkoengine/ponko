# From Blender to the game

Build a level in Blender, give its objects a few custom properties, export
it as glTF, and spawn it: every object becomes an entity in its place, with
its look, its light, its collider and your own components, read from the
properties. Export again while the game runs and the level updates in
place.

<div class="lang-pair">
<div>

```rust
{{#include ../../tests/gltf_scenes.rs:courtyard}}
```

</div>
<div>

```cpp
{{#include ../../examples/cpp/guide.cpp:courtyard}}
```

</div>
</div>

The file is [`assets/levels/courtyard.gltf`](../../assets/levels/courtyard.gltf),
written by hand: one cube mesh placed as the ground, a crate, a pillar and
a door, a spawn point and a lantern. A glTF file spawns as a
[prefab](scenes.md) does, with `ctx.spawn_prefab(path, at)` (`at` a `Vec3`
or a `d3::Transform`), and makes

- a **root** entity with a `GltfRoot` (C: the root you get back) whose
  `Transform` places the whole file: spawn it twice in two places, or move
  the root and everything follows;
- an entity for **every node** of the file's scene, attached to its
  parent ([the hierarchy](hierarchy.md)), with its `Transform`, a
  `GltfNode` (its name, its path such as `"Gate/Door"`, and all its custom
  properties, yours included) and a `Model` of its own mesh with the file's
  materials;
- its **light**: a Blender point or spot light becomes a `PointLight` or
  `SpotLight` on the node's entity; a sun sets the scene's sun
  ([Light and shadows](3d.md#light-and-shadows));
- and what its `ponko:` **custom properties** ask for (below).

Find nodes with `GltfRoot::entity("Gate/Door")` (a path, or just a name),
by their `GltfNode`, or by the components the properties put on them.
`ctx.despawn_prefab(root)` removes the scene, its entities and the meshes
made for it. `GltfRoot::spawn(ctx, path, at, options)` spawns with
options (below). Skinned meshes are drawn still, in their bind pose: load animated
characters with `load_mesh` and put them in with a prefab (`ponko:prefab`).
Cameras in the file aren't spawned; `ctx.gfx.load_cameras(path)` reads
them ([glTF extensions, lights and cameras](3d.md#gltf-extensions-lights-and-cameras)).

## In Blender

1. Select an object, open **Object Properties** (the orange square), and
   under **Custom Properties** press **New**. Edit the new property (the
   gear): its **name** (`ponko:collider`), its **type** (String, Integer,
   Float, Boolean) and its **value** (`box`).
   Put properties on the **object**, not on its mesh data: the exporter
   writes an object's custom properties into its node, and the engine
   reads nodes. (Mesh and material properties go to the glTF mesh and
   material, which the engine doesn't read for this.)
2. Name the objects you want to find from code. Blender keeps object
   names unique in a file, so a node's path (`Gate/Door`: its parents'
   names and its own) names the same object export after export.
3. **File → Export → glTF 2.0** with:
   - **Include → Data → Custom Properties** on. Without it the exporter
     writes no `extras` and the file is plain geometry.
   - **Include → Data → Punctual Lights** on, for the lights
     (`KHR_lights_punctual`).
   - **Transform → +Y Up** on (the default): the engine's +Y is up, as
     glTF's is.
   - **Mesh → Apply Modifiers** on, so what you see is what is exported.
   - Format **glTF Binary (`.glb`)** or **glTF Separate**; both load. Save
     it under the game's `assets/`.

How the exporter writes properties, as the glTF 2.0 specification
allows them in `extras` ("application-specific data", which should be a
JSON object of key/value pairs): each custom property of an object is one
key of its node's `extras`, a String as a JSON string, an Integer or Float
as a number, a Boolean as `true` or `false` (older Blender versions store
booleans as 0 and 1, which the engine accepts), an array property as a
JSON array, and a property group made from Python as a JSON object. A few
internal names (`cycles` and the like) are left out. Here is one node of
an exported file:

```json
{
  "name": "Crate",
  "mesh": 0,
  "translation": [2, 0.5, 0],
  "extras": { "ponko:collider": "box", "ponko:body": "dynamic", "ponko:Health": "{\"hp\": 5}" }
}
```

## The properties

Keys the engine reads start with `ponko:`. Other keys are yours: they stay
in `GltfNode::extras` for your systems (and `ponko_gltf_node_extras` in C)
and nothing checks them. A script or a hand-written file can also put the
same keys, without the prefix, in one object under the key `ponko`
(`{"ponko": {"collider": "box"}}`).

| Key | Value | Makes |
|---|---|---|
| `ponko:<Component>` | the component's JSON: an object, or text holding JSON | that [reflected](reflection.md) component, as a scene file would write it (`ponko:Health` = `{"hp": 5}`) |
| `ponko:collider` | `box`, `sphere`, `capsule`, `hull`, `trimesh` | a `Collider` fitted to the node's mesh (with the `physics3d` feature): the box around it, the ball around that box, an upright capsule filling it, its convex hull, or its triangles. The node's scale is in the shape |
| `ponko:body` | `fixed` (default), `dynamic`, `kinematic` | the collider's `RigidBody`. Moving bodies aren't attached to their parent: the physics moves them |
| `ponko:sensor` | true / false | a sensor collider: reports overlaps, blocks nothing |
| `ponko:collision_layers` | a layer 0–31, or a list of them | the collider's `CollisionGroups` memberships |
| `ponko:collides_with` | a layer 0–31, or a list of them | the groups it touches (default: all) |
| `ponko:layers` | a layer 0–31, or a list of them | `RenderLayers` for its model ([several cameras](viewports.md)) |
| `ponko:hidden` | true / false | no `Model`: a collision or navigation stand-in |
| `ponko:spawn` | a name, or true for the object's name | a `SpawnPoint` there instead of a model |
| `ponko:prefab` | a prefab file | that [prefab](scenes.md) spawned in the node's place instead of its mesh |
| `ponko:lod` | screen heights, most detailed first: `[0.3, 0.08, 0.02]` | a `LodGroup` whose levels are the object's children with meshes, by name (`Tree_LOD0`, `Tree_LOD1`, ...) ([Levels of detail](3d.md#levels-of-detail)); its collider comes from the first level |
| `ponko:navmesh` | true / false | whether its mesh is walkable ground for [navigation meshes](navigation3d.md). Not given: meshes of fixed bodies and of nodes without a body are, moving bodies, sensors, spawn points and prefabs aren't |
| `ponko:shadows` | true / false | whether its point or spot light casts shadows |

Blender's property types fit these: text for names and files, numbers for
layers, a Boolean for switches, a Float array (**Edit Property → Type:
Float Array**) for `ponko:lod`. Text that holds JSON is read as JSON
(`[1, 3]`, `{"hp": 5}`), so a String property can hold anything,
including a whole component.

A `ponko:` key that isn't one of these must name a reflected component:
reflect your types in setup, before spawning (`ctx.world.reflect::<Health>()`).

## Mistakes are errors with suggestions

A misspelt key, a component that isn't reflected, or a value of the wrong
kind stops the spawn with an error that names the file, the node and the
closest names the engine knows, and spawns nothing:

```text
ponko: levels/courtyard.glb: node "Crate": "ponko:colider": no property is called "colider"; did you mean "ponko:collider"?
ponko: levels/courtyard.glb: node "Crate": "ponko:collider" is "cube"; it takes one of box, sphere, capsule, hull, trimesh
ponko: levels/courtyard.glb: node "Crate": "ponko:Helth": no reflected component is called "Helth"; did you mean "ponko:Health"?
```

The kind is `ErrorKind::Decode` for the convention, `ErrorKind::Json` for a
component value that doesn't fit its type, and `ErrorKind::NotFound` for a
missing file or prefab; the error's `path` is the glTF file.

## A schema for the editor

`ponko schema` writes a JSON Schema of the properties your game reads: the
`ponko:` keys and every component it reflects, with the component's own
schema where it is [described](reflection.md):

```sh
ponko schema --out ponko-schema.json            # in the game's folder
ponko schema --example yard --out yard-schema.json
```

It builds the game and runs it once without a window: the game's setup
runs (so the components it reflects are in), then it writes the file and
quits. Any game started with `Game::run` (C and C++ ones too) does this
when the environment variable `PONKO_SCHEMA` names a file; that is all
`ponko schema` sets. From Rust, `ponko::d3::gltf_properties_schema(&world)`
returns the same schema.

Editors and scripts check properties against it before exporting. In
Blender's Python console, a check of the names:

```python
import bpy, json
schema = json.load(open(bpy.path.abspath("//ponko-schema.json")))
known = schema["properties"]
for obj in bpy.data.objects:
    for key in obj.keys():
        if key.startswith("ponko:") and key not in known:
            print(f"{obj.name}: {key} is not a property the game reads")
```

A full JSON Schema validator (any language has one) checks the values too.

## Export again while the game runs

With [hot reload](drawing.md#hot-reload) on (`Game::hot_reload(true)`, on by
default in debug builds), exporting over the file updates every scene
spawned from it, a moment after the file changes:

- a node whose **path stays** (same name, same parents) keeps its entity,
  and whatever the game added to it. Its transform, mesh, materials,
  light and what its properties make come from the new file; components a
  removed property had put on it are taken off;
- new nodes are spawned, nodes gone from the file are despawned;
- a file that doesn't load, or breaks the convention, keeps the old scene
  and prints why.

`ctx.assets.reloaded_files()` lists the file that frame. Renaming an
object, or moving it under another parent, is a new node to the engine:
the old entity goes and a new one comes.

## Saves

Everything a file spawns is rebuilt from the file, so it carries
`NotSaved` and stays out of world saves, except prefab instances, which are
the game's state (an opened chest, a moved barrel). After loading a save,
spawn the file again with
`GltfRoot::spawn(ctx, path, at, GltfOptions::default().without_saved_objects())`
(C: `without_saved_objects = true`), so the save's prefab instances aren't
doubled. This is the rule [levels from Tiled and LDtk](tiles.md#levels-and-saves) follow.

## Navigation meshes

`GltfScene::load(ctx, path)` reads the file and checks its properties
without spawning anything or decoding images: its `nodes` with their
transforms, properties and meshes, and its `spawn_points()`. With the
`navmesh` feature, `nav_source()` collects the walkable meshes
(`ponko:navmesh`) to bake a [navigation mesh](navigation3d.md) from:

```rust
# use ponko::prelude::*;
# fn f(ctx: &mut Ctx) -> Result<(), Box<dyn std::error::Error>> {
let scene = ponko::d3::GltfScene::load(ctx, "levels/courtyard.gltf")?;
let nav = ponko::d3::NavMesh::bake(&[scene.nav_source()], ponko::nav::NavBakeSettings::default())?;
# let _ = nav;
# Ok(())
# }
```

## C and C++

`ponko_ctx_spawn_gltf`, `ponko_ctx_despawn_gltf`, `ponko_gltf_node_entity`,
`ponko_gltf_node_extras` (a node's properties as JSON text) and
`ponko_spawn_point_name`; in C++ `ponko::spawn_gltf`,
`ponko::despawn_gltf`, `ponko::gltf_node`, `ponko::gltf_node_extras` and
`ponko::spawn_point_name`. Components named in properties are reflected
ones, so a C or C++ component needs `reflect_fields` first
([C and C++ components](reflection.md#c-and-c-components)). Reading a file
without spawning it (`GltfScene::load`) and the schema as a value
(`gltf_properties_schema`) are Rust-only; `PONKO_SCHEMA` works for every
game.

## What it doesn't do (yet)

- Each spawn makes its own meshes: a file spawned many times holds many
  copies. For many copies of one object, use a prefab or `load_mesh` and
  your own entities.
- A Transform keeps one scale per axis: a non-uniformly scaled parent with
  a rotated child is placed approximately.
- A `.gltf` file's separate `.bin` and images are read again when the
  `.gltf` itself changes (the exporter writes all of them every time), not
  on their own.
