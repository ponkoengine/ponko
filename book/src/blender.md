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
`ctx.despawn_prefab(root)` removes the scene and its entities.

Every spawn of one file shares the file's meshes and materials: a file of
three meshes spawned a hundred times holds three meshes (1,488 bytes by
the engine's own count, `ctx.asset_inventory()`), where each spawn used
to make its own (300 meshes, 148,800 bytes). They are freed with the last
scene of the file, and when a world load replaces the scenes; a mesh
`load_mesh` holds from the same file keeps the materials.

`GltfRoot::spawn(ctx, path, at, options)` spawns with
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

Editors and scripts check properties against it before exporting: the
Blender add-on below does, and any JSON Schema validator (every language
has one) can.

## The Blender add-on

[`tools/blender/ponko_properties`](../../tools/blender/ponko_properties)
is a small Blender add-on (plain Python, outside the engine's crates)
that reads the game's `ponko-schema.json` and offers its properties:

1. Zip the `ponko_properties` folder, then in Blender **Edit →
   Preferences → Add-ons → Install...** pick the zip and enable **Ponko
   properties** (or copy the folder into Blender's `scripts/addons`).
2. Write the schema next to the `.blend` file (`ponko schema --out
   ponko-schema.json`). The add-on reads `//ponko-schema.json` (the
   folder of the `.blend`); the **Ponko** panel in **Object Properties**
   takes another path, and **Reload schema** reads it again after the
   game reflected new components.
3. **Add Ponko property** offers every `ponko:` key the game reads, with
   its description, searchable: the convention's keys first, then the
   reflected components. A new property starts with a value that fits:
   the first choice of `ponko:collider` and `ponko:body`, a Boolean for
   switches, a Float Array for `ponko:lod`, and a component's required
   fields as JSON text (`{"hp": 0}`).
4. The panel lists the object's `ponko:` properties and, under each one
   that doesn't fit, what is wrong, with the engine's suggestions
   (`"ponko:colider": no property is called "colider"; did you mean
   "ponko:collider"?`). **Check all objects** checks every object of the
   file before you export; the glTF exporter also prints the problems of
   each object it exports (the export still runs).

The checks are the engine's: the same suggestions, JSON in a String
property read as JSON, a component's value against its
[described](reflection.md) fields. They live in
`tools/blender/ponko_properties/schema.py`, which needs no Blender, so a
build script can check `.blend` exports too; its tests run with
`python3 -m unittest discover -s tools/blender`.

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
the game's state (an opened chest, a moved barrel). The save also holds a
record of each scene: its file, where it was spawned, and the entity of
each prefab node's instance. To load, load the save, then spawn the file
again as usual:

```rust
{{#include ../../tests/gltf_scenes.rs:saved_level}}
```

The scene adopts the saved instances: `GltfRoot::entity("BarrelA")` is the
saved entity again, where the game left it and with what it changed, and
an instance the game had despawned (a barrel blown up) isn't spawned
again. Prefab nodes added to the file since the save are spawned. When
the save holds several scenes of one file, a spawn takes the one saved
nearest to where it is spawned.

`GltfOptions::without_saved_objects()` (C: `without_saved_objects = true`),
which earlier versions needed after a load, is no longer needed and still
works: the scene adopts the saved instances and spawns no prefab node the
save has no instance for. A save written before saves held the record
(its instances aren't linked to their nodes) still loads: scenes spawned
after it leave their prefab nodes to the save, as `without_saved_objects`
did, and print a note; the next save keeps it that way.

## Repeated meshes and many spawns

Nodes that use one mesh share it. In Blender, linked duplicates (Alt+D)
share their mesh data and export as one glTF mesh: a fence of forty posts
made that way holds one post mesh on the GPU, drawn where each node stands,
each with its own transform, properties and components. A plain duplicate
(Shift+D) copies the mesh data, so each copy exports, and is uploaded, as
a mesh of its own. Nodes of one mesh with different morph weights get a
mesh each.

Every spawn of one file shares its meshes, materials and collision shapes.
The hull or triangle mesh fitted for a `ponko:collider` is made once for
the file, unscaled, and used again by its other nodes of the same mesh at
every scale and by every later spawn: each collider scales it where it is
used (`d3::Shape::ScaledMesh`), so a node scaled up, flattened or mirrored
(a negative scale) is solid exactly where it is drawn. Boxes, spheres and
capsules are fitted to the scaled mesh, once per scale. All of it goes
when the last scene that uses it is despawned or replaced by a world load.
Exporting the file again while the game runs makes one new set for all its
scenes and frees the old set once no scene holds it.

A file is read once while it is unchanged. The first spawn parses it; the
nodes it read (their transforms, properties and lights, and each glTF
mesh's corners and triangles, once) stay with the file's shared meshes, so
a later spawn only reads the file's bytes and its buffers to see that they
are the same, and parses nothing. A file or buffer that changed on disk
since, a mesh the game freed, or the last scene of the file going makes
the next spawn read it again; an export the game reloads is parsed once
for all its scenes. The corners and triangles kept are a CPU copy of the
file's geometry, without normals, UVs or images, for as long as a scene of
the file lives.

The engine's benchmark (`cargo run --release -p ponko_bench -- gltf/`)
spawns a file of twelve nodes of one rock mesh (1,024 corners), eight of
them with colliders. One spawn holds one mesh (66.5 KiB) instead of twelve
(798 KiB). A hundred spawns hold two collision meshes instead of 600 and
keep 1.5 MiB of heap instead of 115 MiB. They take about 2.3 ms: the first
spawn 0.5 ms, each later one about 18 µs, where it was 80 µs when every
spawn parsed the file (8.4 ms for the hundred); keeping what was read costs
45 KiB. With the rocks at four scales (plain, doubled, flattened and
mirrored), a hundred spawns hold two collision meshes where fitting one per
scale made five.

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
([C and C++ components](reflection.md#c-and-c-components)).

Reading a file without spawning it (`GltfScene::load`) is
`ponko_gltf_scene_load` (C++ `ponko::gltf_scene_json`), which writes the
scene as JSON text: its nodes with their paths, parents, transforms,
custom properties, mesh sizes and lights, and its spawn points. The schema
(`gltf_properties_schema`) is `ponko_gltf_properties_schema` (C++
`ponko::gltf_properties_schema`). Navigation meshes from a file
(`nav_source`) are Rust-only.

```cpp
{{#include ../../examples/cpp/guide.cpp:courtyard_read}}
```

## What it doesn't do (yet)

- Spawns of one file share its meshes and collision shapes, but each
  spawn makes its own entities: for thousands of copies of one object,
  [GPU-driven instances](3d.md#gpu-driven-instances) draw them cheaper.
- A Transform keeps one scale per axis: a non-uniformly scaled parent with
  a rotated child is placed approximately.
- A `.gltf` file's separate `.bin` and images are read again when the
  `.gltf` itself changes (the exporter writes all of them every time), not
  on their own.
