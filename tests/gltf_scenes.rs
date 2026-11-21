//! glTF files as scenes (`Ctx::spawn_prefab` of a `.gltf`, `GltfRoot::spawn`), headless: nodes become
//! entities with the components, colliders, lights, spawn points, prefabs
//! and levels of detail their custom properties (`extras`) ask for; wrong
//! names are errors with suggestions; re-exporting updates the scene in
//! place. The fixtures are `.gltf` files written here, with one cube mesh
//! in an embedded buffer.
#![cfg(feature = "3d")]

mod common;

use common::TempAsset;
use ponko::d3::{GltfNode, GltfRoot, GltfScene, SpawnPoint, Transform};
use ponko::prelude::*;
use ponko::render::{LodGroup, Model};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Component)]
struct Health {
    hp: f32,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Component)]
struct Barrel {
    full: bool,
}

/// Standard base64, for the embedded buffer.
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A `.gltf` file with `nodes` (glTF node objects; `"mesh": 0` is a 1 m
/// cube around the node's origin, and `"mesh": 1` another glTF mesh of the
/// same cube), every node without a parent at the top of the scene, and
/// `lights` as `KHR_lights_punctual` lights.
fn gltf(nodes: Value, lights: Value) -> String {
    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let indices: [u16; 36] = [
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
    ];
    let mut bytes = Vec::new();
    for c in corners {
        for v in c {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
    }
    for i in indices {
        bytes.extend_from_slice(&i.to_le_bytes());
    }
    let list = nodes.as_array().unwrap();
    let children: Vec<u64> = list
        .iter()
        .flat_map(|n| n["children"].as_array().cloned().unwrap_or_default())
        .filter_map(|c| c.as_u64())
        .collect();
    let top: Vec<usize> = (0..list.len()).filter(|i| !children.contains(&(*i as u64))).collect();
    json!({
        "asset": { "version": "2.0", "generator": "ponko tests" },
        "extensionsUsed": ["KHR_lights_punctual"],
        "extensions": { "KHR_lights_punctual": { "lights": lights } },
        "scene": 0,
        "scenes": [{ "nodes": top }],
        "nodes": nodes,
        "meshes": [
            { "name": "cube", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] },
            { "name": "cube copy", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] }
        ],
        "materials": [{ "name": "paint", "pbrMetallicRoughness": { "baseColorFactor": [0.8, 0.2, 0.2, 1.0] } }],
        "buffers": [{ "byteLength": bytes.len(), "uri": format!("data:application/octet-stream;base64,{}", base64(&bytes)) }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 96, "target": 34962 },
            { "buffer": 0, "byteOffset": 96, "byteLength": 72, "target": 34963 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5] },
            { "bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR" }
        ]
    })
    .to_string()
}

/// A small level with one node of each kind; `prefab` is the barrel prefab.
fn yard(prefab: &str) -> String {
    gltf(
        json!([
            { "name": "Ground", "mesh": 0, "scale": [10.0, 1.0, 10.0], "translation": [0.0, -0.5, 0.0],
              "extras": { "ponko:collider": "trimesh" } },
            { "name": "Crate", "mesh": 0, "translation": [0.0, 3.0, 0.0],
              "extras": { "ponko:collider": "box", "ponko:body": "dynamic", "ponko:Health": { "hp": 5.0 } } },
            { "name": "PlayerStart", "translation": [1.0, 0.0, 2.0], "extras": { "ponko:spawn": "player" } },
            { "name": "House", "translation": [5.0, 0.0, 0.0], "children": [4] },
            { "name": "Door", "mesh": 0, "translation": [0.0, 0.0, 1.0], "extras": { "door": "front" } },
            { "name": "Tree", "translation": [-4.0, 0.0, 0.0], "children": [6, 7], "extras": { "ponko:lod": [0.3, 0.05] } },
            { "name": "Tree_LOD0", "mesh": 0, "scale": [1.0, 4.0, 1.0] },
            { "name": "Tree_LOD1", "mesh": 0 },
            { "name": "Lamp", "translation": [0.0, 2.5, 0.0], "extensions": { "KHR_lights_punctual": { "light": 0 } },
              "extras": { "ponko:shadows": true } },
            { "name": "Blocker", "mesh": 0, "translation": [3.0, 0.0, -3.0],
              "extras": { "ponko": { "hidden": true, "collider": "hull", "collision_layers": [2], "collides_with": "[0, 1]" } } },
            { "name": "Icon", "mesh": 0, "extras": { "ponko:layers": 1 } },
            { "name": "Barrel", "mesh": 0, "translation": [2.0, 0.0, 2.0], "extras": { "ponko:prefab": prefab } }
        ]),
        json!([{ "type": "point", "color": [1.0, 0.8, 0.6], "intensity": 683.0, "range": 8.0 }]),
    )
}

fn barrel_prefab() -> TempAsset {
    TempAsset::with_contents(
        "barrel.json",
        &json!({
            "format": "ponko-prefab", "version": 1,
            "components": { "Transform3D": {}, "Barrel": { "full": true } }
        })
        .to_string(),
    )
}

fn game() -> Game {
    Game::new("gltf scenes", 1, 1)
}

fn reflect(ctx: &mut Ctx) {
    ctx.world.reflect::<Health>();
    ctx.world.reflect::<Barrel>();
}

/// The entity of the node at `path`.
fn node(world: &mut World, path: &str) -> Entity {
    let root = world.query::<&GltfRoot>().iter().next().expect("a scene").clone();
    root.entity(path).unwrap_or_else(|| panic!("no node {path}"))
}

#[test]
fn every_node_becomes_an_entity_placed_where_the_file_puts_it() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        let at = Transform::at(vec3(100.0, 0.0, 0.0));
        ctx.spawn_prefab(file.name(), at).unwrap();
    });
    let door = node(&mut world, "House/Door");
    assert_eq!(world.get::<Transform>(door).unwrap().position, vec3(105.0, 0.0, 1.0));
    let names = world.get::<GltfNode>(door).unwrap();
    assert_eq!((names.name.as_str(), names.path.as_str()), ("Door", "House/Door"));
    assert_eq!(names.extras["door"], "front", "the game's own properties are kept");
    assert!(world.get::<Model>(door).is_some(), "a node with a mesh is drawn");
    let house = node(&mut world, "House");
    assert_eq!(world.parent(door), Some(house), "attached to its parent");
    assert!(world.get::<Model>(house).is_none(), "an empty draws nothing");
    assert!(
        world.get::<ponko::reflect::NotSaved>(door).is_some(),
        "the file rebuilds it"
    );
}

#[test]
fn properties_attach_reflected_components_spawn_points_and_lights() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
    });
    assert_eq!(
        {
            let e = node(&mut world, "Crate");
            world.get::<Health>(e)
        },
        Some(&Health { hp: 5.0 })
    );
    let start = node(&mut world, "PlayerStart");
    assert_eq!(world.get::<SpawnPoint>(start).unwrap().name, "player");
    assert_eq!(world.get::<Transform>(start).unwrap().position, vec3(1.0, 0.0, 2.0));
    let lamp = {
        let e = node(&mut world, "Lamp");
        world.get::<ponko::d3::PointLight>(e)
    }
    .unwrap();
    assert!(lamp.shadows && (lamp.intensity - 1.0).abs() < 1e-4 && lamp.range == 8.0);
    let icon = node(&mut world, "Icon");
    assert_eq!(world.get::<ponko::camera::RenderLayers>(icon).unwrap().0, 0b10);
    assert!(
        {
            let e = node(&mut world, "Blocker");
            world.get::<Model>(e)
        }
        .is_none(),
        "hidden"
    );
}

#[test]
fn a_lod_group_takes_its_children_as_levels() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
    });
    let tree = node(&mut world, "Tree");
    let group = world.get::<LodGroup>(tree).unwrap();
    assert_eq!(group.levels.len(), 2);
    assert_eq!(group.levels[0].screen_height, 0.3);
    assert_eq!(group.size, 4.0, "the first level is 4 m tall");
    assert_eq!(group.levels[0].renderers[0].offset.scale, vec3(1.0, 4.0, 1.0));
    let root = world.query::<&GltfRoot>().iter().next().unwrap().clone();
    assert!(
        root.entity("Tree/Tree_LOD0").is_none(),
        "levels are no entities of their own"
    );
}

#[test]
fn prefab_nodes_spawn_their_prefab_and_are_saved() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
    });
    let barrel = node(&mut world, "Barrel");
    assert_eq!(world.get::<Barrel>(barrel), Some(&Barrel { full: true }));
    assert_eq!(world.get::<Transform>(barrel).unwrap().position, vec3(2.0, 0.0, 2.0));
    assert!(
        world.get::<Model>(barrel).is_none(),
        "the prefab replaces the node's mesh"
    );
    assert!(
        world.get::<ponko::reflect::NotSaved>(barrel).is_none(),
        "the game's state"
    );

    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        let options = ponko::d3::GltfOptions::default().without_saved_objects();
        GltfRoot::spawn(ctx, file.name(), Transform::default(), options).unwrap();
    });
    assert_eq!(world.query::<&Barrel>().iter().count(), 0, "left to the save");
}

#[test]
fn unknown_names_are_errors_that_suggest_the_right_one_and_spawn_nothing() {
    let cases = [
        (json!({ "ponko:colider": "box" }), "did you mean \"ponko:collider\""),
        (json!({ "ponko:Helth": { "hp": 1 } }), "did you mean \"ponko:Health\""),
        (
            json!({ "ponko:collider": "cube" }),
            "it takes one of box, sphere, capsule, hull, trimesh",
        ),
    ];
    for (extras, expected) in cases {
        let file = TempAsset::with_contents(
            "bad.gltf",
            &gltf(json!([{ "name": "Thing", "mesh": 0, "extras": extras }]), json!([])),
        );
        let mut world = game().run_headless(0, |ctx| {
            reflect(ctx);
            let err = ctx.spawn_prefab(file.name(), Transform::default()).unwrap_err();
            assert_eq!(err.kind(), ErrorKind::Decode);
            assert_eq!(err.path(), Some(file.name()));
            assert!(err.message().contains("node \"Thing\""), "{err}");
            assert!(err.message().contains(expected), "{err}");
        });
        assert_eq!(world.query::<&GltfNode>().iter().count(), 0);
        assert_eq!(world.query::<&GltfRoot>().iter().count(), 0);
    }
}

#[test]
fn a_component_value_that_does_not_fit_is_a_json_error() {
    let file = TempAsset::with_contents(
        "bad.gltf",
        &gltf(
            json!([{ "name": "Thing", "extras": { "ponko:Health": "{\"hp\": \"lots\"}" } }]),
            json!([]),
        ),
    );
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let err = ctx.spawn_prefab(file.name(), Transform::default()).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Json, "{err}");
        assert!(err.message().contains("\"ponko:Health\""), "{err}");
    });
}

#[test]
fn despawning_a_scene_removes_its_nodes_and_frees_its_meshes() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let before = ctx.world.len();
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let door_mesh = {
            let e = node(&mut ctx.world, "House/Door");
            ctx.world.get::<Model>(e)
        }
        .unwrap()
        .mesh;
        let spawned = ctx.world.len() - before;
        assert_eq!(ctx.despawn_prefab(root), spawned);
        assert_eq!(ctx.world.len(), before);
        assert!(!ctx.is_live(door_mesh));
        assert_eq!(ctx.despawn_prefab(root), 0);
        // A glTF scene is 3D: a 2D place is refused, and nothing spawns.
        let err = ctx.spawn_prefab(file.name(), vec2(1.0, 2.0)).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Unsupported);
        assert_eq!(ctx.world.len(), before);
    });
}

#[test]
fn a_read_scene_lists_nodes_and_spawn_points_without_spawning() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let scene = GltfScene::load(ctx, file.name()).unwrap();
        assert_eq!(scene.nodes.len(), 12);
        assert_eq!(scene.node("Door").unwrap().world.position, vec3(5.0, 0.0, 1.0));
        let spawns: Vec<_> = scene.spawn_points().map(|(n, t)| (n.to_string(), t.position)).collect();
        assert_eq!(spawns, [("player".to_string(), vec3(1.0, 0.0, 2.0))]);
        assert_eq!(ctx.world.query::<&GltfNode>().iter().count(), 0);
    });
}

#[cfg(feature = "navmesh")]
#[test]
fn the_navigation_source_holds_the_walkable_meshes_only() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let scene = GltfScene::load(ctx, file.name()).unwrap();
        // Ground, Door, the tree's first level, Blocker and Icon: 12 triangles
        // each; not the dynamic crate, the second tree level or the barrel.
        assert_eq!(scene.nav_source().triangles.len(), 5 * 12);
    });
}

#[cfg(feature = "physics3d")]
#[test]
fn colliders_fit_the_node_meshes_with_their_scale() {
    use ponko::d3::{Collider, RigidBody, Shape};
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("yard.gltf", &yard(prefab.name()));
    let mut world = game().physics3d(vec3(0.0, -9.81, 0.0)).run_headless(30, |ctx| {
        reflect(ctx);
        ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
    });
    let crate_e = node(&mut world, "Crate");
    let collider = world.get::<Collider>(crate_e).unwrap();
    assert_eq!(collider.shape, Shape::Cuboid { size: Vec3::ONE });
    assert_eq!(world.get::<RigidBody>(crate_e).unwrap().kind, BodyKind::Dynamic);
    let fallen = world.get::<Transform>(crate_e).unwrap().position.y;
    assert!(fallen < 2.9, "the crate fell: {fallen}");
    assert!(world.parent(crate_e).is_none(), "moving bodies aren't attached");
    let ground = {
        let e = node(&mut world, "Ground");
        world.get::<Collider>(e)
    }
    .unwrap();
    assert!(
        matches!(ground.shape, Shape::ScaledMesh { scale, .. } if scale == vec3(10.0, 1.0, 10.0)),
        "the ground's triangle mesh, scaled where it is used: {:?}",
        ground.shape
    );
    assert_eq!(
        {
            let e = node(&mut world, "Ground");
            world.get::<RigidBody>(e)
        }
        .unwrap()
        .kind,
        BodyKind::Fixed
    );
    let blocker = {
        let e = node(&mut world, "Blocker");
        world.get::<Collider>(e)
    }
    .unwrap();
    assert_eq!((blocker.groups.memberships, blocker.groups.filter), (0b100, 0b11));
}

// ---- hot reload ----------------------------------------------------------------

/// Spawns `file` with hot reload and writes `after` over it on tick 2;
/// returns the ticks that saw the reload and the world.
fn reexport(file: &TempAsset, after: String, keep: impl Fn(&mut World) + 'static) -> (Vec<u64>, World) {
    reexport_spawned(file, 1, after, keep)
}

/// [`reexport`] of `spawns` scenes of `file`, a metre apart.
fn reexport_spawned(
    file: &TempAsset,
    spawns: usize,
    after: String,
    keep: impl Fn(&mut World) + 'static,
) -> (Vec<u64>, World) {
    use std::sync::{Arc, Mutex};
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let name = file.name().to_string();
    let path = file.path();
    let world = game()
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.assets.reloaded_files().contains(&name) {
                log.lock().unwrap().push(ctx.time.sim_tick());
            }
            if ctx.time.sim_tick() == 2 {
                keep(&mut ctx.world);
                std::thread::sleep(std::time::Duration::from_millis(50));
                std::fs::write(&path, &after).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        })
        .run_headless(5, {
            let name = file.name().to_string();
            move |ctx| {
                reflect(ctx);
                for i in 0..spawns {
                    ctx.spawn_prefab(&name, Transform::at(vec3(i as f32, 0.0, 0.0)))
                        .unwrap();
                }
            }
        });
    let seen = seen.lock().unwrap().clone();
    (seen, world)
}

fn two_boxes(door_x: f32, health: f32, third: bool) -> String {
    let mut nodes = vec![
        json!({ "name": "Door", "mesh": 0, "translation": [door_x, 0.0, 0.0], "extras": { "ponko:Health": { "hp": health } } }),
        json!({ "name": "Wall", "mesh": 0, "translation": [0.0, 0.0, -5.0] }),
    ];
    if third {
        nodes.push(json!({ "name": "Window", "mesh": 0 }));
    }
    gltf(Value::Array(nodes), json!([]))
}

#[test]
fn reexporting_updates_the_scene_and_keeps_entities_whose_names_match() {
    use std::sync::{Arc, Mutex};
    let file = TempAsset::with_contents("live.gltf", &two_boxes(0.0, 5.0, true));
    let before = Arc::new(Mutex::new(None));
    let record = before.clone();
    let (seen, mut world) = reexport(&file, two_boxes(3.0, 9.0, false), move |world| {
        let door = node(world, "Door");
        let window = node(world, "Window");
        let mesh = world.get::<Model>(door).unwrap().mesh;
        *record.lock().unwrap() = Some((door, window, mesh));
    });
    assert_eq!(seen, [3], "reloaded once, on the tick after the export");
    let (door, window, old_mesh) = before.lock().unwrap().unwrap();
    assert_eq!(node(&mut world, "Door"), door, "same entity");
    assert_eq!(world.get::<Transform>(door).unwrap().position, vec3(3.0, 0.0, 0.0));
    assert_eq!(world.get::<Health>(door), Some(&Health { hp: 9.0 }));
    assert_ne!(world.get::<Model>(door).unwrap().mesh, old_mesh, "the new geometry");
    assert!(!world.is_alive(window), "a node gone from the file is despawned");
    assert_eq!(world.query::<&GltfNode>().iter().count(), 2);
}

#[test]
fn reexporting_a_file_spawned_twice_gives_both_scenes_one_new_set_of_meshes() {
    use std::sync::{Arc, Mutex};
    let file = TempAsset::with_contents("live.gltf", &two_boxes(0.0, 5.0, true));
    let before = Arc::new(Mutex::new(Vec::new()));
    let record = before.clone();
    let (seen, mut world) = reexport_spawned(&file, 2, two_boxes(3.0, 9.0, false), move |world| {
        let doors: Vec<_> = world
            .query::<(&GltfNode, &Model)>()
            .iter()
            .map(|(_, m)| m.mesh)
            .collect();
        *record.lock().unwrap() = doors;
    });
    assert_eq!(seen, [3]);
    let old = before.lock().unwrap().clone();
    assert_eq!(old.len(), 6, "two scenes of three meshes");
    let roots: Vec<GltfRoot> = world.query::<&GltfRoot>().iter().cloned().collect();
    let door_mesh = |world: &mut World, root: &GltfRoot| world.get::<Model>(root.entity("Door").unwrap()).unwrap().mesh;
    let (a, b) = (door_mesh(&mut world, &roots[0]), door_mesh(&mut world, &roots[1]));
    assert_eq!(a, b, "both scenes share the new mesh");
    assert!(!old.contains(&a));
    let mut meshes = Vec::new();
    for m in world.query::<&Model>().iter().map(|m| m.mesh) {
        if !meshes.contains(&m) {
            meshes.push(m);
        }
    }
    assert_eq!(meshes.len(), 1, "Door and Wall's one glTF mesh, once for both scenes");
}

#[test]
fn a_broken_export_keeps_the_old_scene() {
    let file = TempAsset::with_contents("live.gltf", &two_boxes(0.0, 5.0, true));
    let broken = gltf(
        json!([{ "name": "Door", "extras": { "ponko:colider": "box" } }]),
        json!([]),
    );
    let (seen, mut world) = reexport(&file, broken, |_| {});
    assert!(seen.is_empty());
    assert_eq!(world.query::<&GltfNode>().iter().count(), 3);
    assert_eq!(
        {
            let e = node(&mut world, "Door");
            world.get::<Health>(e)
        },
        Some(&Health { hp: 5.0 })
    );
}

#[test]
fn the_properties_schema_lists_the_convention_and_reflected_components() {
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let schema = ponko::d3::gltf_properties_schema(&ctx.world);
        let props = &schema["properties"];
        assert_eq!(
            props["ponko:collider"]["enum"],
            json!(["box", "sphere", "capsule", "hull", "trimesh"])
        );
        assert!(props["ponko:Health"].is_object());
        assert!(props["ponko:Transform3D"].is_object());
        assert!(props["ponko"]["properties"]["lod"].is_object());
        let names = schema["propertyNames"]["anyOf"][1]["enum"].as_array().unwrap();
        assert!(names.contains(&json!("ponko:spawn")));
    });
}

#[test]
fn a_schema_setting_writes_the_schema_with_the_setups_components_and_returns() {
    // `PONKO_SCHEMA=<file>`: `Game::run` opens no window, runs the setup,
    // writes the schema there and returns (what `ponko schema` does).
    let out = std::env::temp_dir().join(format!("ponko-schema-{}.json", std::process::id()));
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "schema_child", "--nocapture"])
        .env("PONKO_SCHEMA_CHILD", "1")
        .env("PONKO_SCHEMA", &out)
        .env_remove("PONKO_SMOKE")
        .env_remove("PONKO_CONTROL")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(result.status.success(), "{stderr}");
    assert!(stderr.contains("ponko: wrote the glTF properties schema"), "{stderr}");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let _ = std::fs::remove_file(&out);
    assert!(schema["properties"]["ponko:Health"].is_object(), "reflected in setup");
    assert!(schema["properties"]["ponko:prefab"].is_object());
}

#[test]
fn schema_child() {
    if std::env::var_os("PONKO_SCHEMA_CHILD").is_none() {
        return;
    }
    Game::new("Schema setting", 1, 1)
        .sim_system(|_: &mut Ctx| panic!("a schema run runs no ticks"))
        .run(reflect);
}

// ANCHOR: courtyard
/// Spawns the courtyard from Blender (a glTF file) and puts the player on
/// its spawn point.
fn courtyard(ctx: &mut Ctx) -> Result<Entity, Error> {
    let root = ctx.spawn_prefab("levels/courtyard.gltf", Transform::default())?;
    let start = ctx
        .world
        .query::<(&Transform, &SpawnPoint)>()
        .iter()
        .find(|(_, p)| p.name == "player")
        .map(|(t, _)| *t)
        .unwrap_or_default();
    ctx.world.spawn((start, Player));
    // A node by its path, and a custom property of the game's own.
    let scene = ctx.world.get::<GltfRoot>(root).unwrap();
    let lantern = scene.entity("Lantern").unwrap();
    let flicker = ctx.world.get::<GltfNode>(lantern).unwrap().extras["flicker"].as_f64();
    assert_eq!(flicker, Some(0.2));
    Ok(root)
}
// ANCHOR_END: courtyard

#[derive(Component)]
struct Player;

#[test]
fn the_guide_courtyard_spawns_with_its_player_on_the_spawn_point() {
    let mut world = game().run_headless(0, |ctx| {
        courtyard(ctx).unwrap();
    });
    let player = world.query::<(&Transform, &Player)>().iter().next().unwrap().0.position;
    assert_eq!(player, vec3(0.0, 0.0, 4.0));
    let door = node(&mut world, "Gate/Door");
    assert_eq!(world.get::<Transform>(door).unwrap().position, vec3(0.0, 1.0, -6.0));
    let lantern = node(&mut world, "Lantern");
    assert!(world.get::<ponko::d3::PointLight>(lantern).unwrap().shadows);
}

// ---- saves ----------------------------------------------------------------------

/// A level with three barrel prefabs (`prefab`) on the ground. Part of the
/// retained fixture `tests/fixtures/saves/gltf_instances_v1.json`: its node
/// names and places must not change.
fn barrel_level(prefab: &str) -> String {
    gltf(
        json!([
            { "name": "Ground", "mesh": 0, "scale": [10.0, 1.0, 10.0], "translation": [0.0, -0.5, 0.0] },
            { "name": "BarrelA", "mesh": 0, "translation": [2.0, 0.0, 2.0], "extras": { "ponko:prefab": prefab } },
            { "name": "BarrelB", "mesh": 0, "translation": [-2.0, 0.0, 2.0], "extras": { "ponko:prefab": prefab } },
            { "name": "Cellar", "translation": [0.0, -3.0, 0.0], "children": [4] },
            { "name": "BarrelC", "mesh": 0, "translation": [0.0, 0.0, -2.0], "extras": { "ponko:prefab": prefab } }
        ]),
        json!([]),
    )
}

/// What a game does to [`barrel_level`] before saving: barrel A is moved
/// and emptied, barrel B destroyed, barrel C left alone, and a player
/// spawned. Returns barrel A's entity.
fn play_barrel_level(ctx: &mut Ctx, root: Entity) -> Entity {
    let scene = ctx.world.get::<GltfRoot>(root).unwrap().clone();
    let a = scene.entity("BarrelA").unwrap();
    ctx.world.insert(a, Transform::at(vec3(2.0, 0.0, 5.0)));
    ctx.world.insert(a, Barrel { full: false });
    let b = scene.entity("BarrelB").unwrap();
    ctx.despawn_prefab(b);
    ctx.world
        .spawn((Transform::at(vec3(0.0, 0.0, 8.0)), Health { hp: 3.0 }));
    a
}

/// Writes `tests/fixtures/saves/gltf_instances_v1.json`: a world save, as
/// `dev` wrote it before glTF prefab instances were linked to their scene,
/// of [`barrel_level`] spawned, played ([`play_barrel_level`]) and saved.
/// Run once, at the revision named in its provenance; never regenerated.
#[test]
#[ignore = "writes a retained fixture"]
fn write_gltf_instances_v1_fixture() {
    let out = std::env::var("PONKO_SAVE_FIXTURE_OUT").expect("set PONKO_SAVE_FIXTURE_OUT to the file to write");
    let out = std::path::PathBuf::from(out);
    assert!(
        !out.exists(),
        "{} exists; fixtures are never overwritten",
        out.display()
    );
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    game().run_headless(0, move |ctx| {
        reflect(ctx);
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        play_barrel_level(ctx, root);
        let text = serde_json::to_string_pretty(&ctx.world.save()).unwrap() + "\n";
        std::fs::write(&out, text).unwrap();
    });
}

// ---- shared meshes ---------------------------------------------------------------

/// A prop of three parts (a 1 m cube scaled three ways: three nodes of one
/// glTF mesh, which share one uploaded mesh) and one material.
fn prop() -> String {
    gltf(
        json!([
            { "name": "Body", "mesh": 0 },
            { "name": "Lid", "mesh": 0, "translation": [0.0, 0.6, 0.0], "scale": [1.0, 0.2, 1.0] },
            { "name": "Handle", "mesh": 0, "translation": [0.6, 0.0, 0.0], "scale": [0.2, 0.2, 0.2] }
        ]),
        json!([]),
    )
}

/// Live meshes and materials, and their estimated bytes, from the engine's
/// own accounting.
fn meshes_and_materials(ctx: &Ctx) -> (usize, u64, usize) {
    use ponko::inventory::AssetClass;
    let inventory = ctx.asset_inventory();
    let mesh = inventory.get(AssetClass::Mesh);
    (mesh.live, mesh.bytes, inventory.get(AssetClass::Material).live)
}

#[test]
fn a_hundred_spawns_of_one_file_hold_one_copy_of_its_meshes() {
    let file = TempAsset::with_contents("prop.gltf", &prop());
    game().run_headless(0, |ctx| {
        let empty = meshes_and_materials(ctx);
        let first = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let one = meshes_and_materials(ctx);
        let mut roots = vec![first];
        for i in 1..100 {
            roots.push(
                ctx.spawn_prefab(file.name(), Transform::at(vec3(i as f32 * 3.0, 0.0, 0.0)))
                    .unwrap(),
            );
        }
        let hundred = meshes_and_materials(ctx);
        eprintln!("glTF spawns: none {empty:?}, one {one:?}, a hundred {hundred:?} (meshes, mesh bytes, materials)");
        assert_eq!(empty, (0, 0, 0));
        assert_eq!(one.0, 1, "one mesh for the three nodes of one glTF mesh");
        assert_eq!(
            hundred, one,
            "a hundred spawns share the first one's meshes and material"
        );
        let lid = |ctx: &mut Ctx, root: Entity| {
            let e = ctx.world.get::<GltfRoot>(root).unwrap().entity("Lid").unwrap();
            ctx.world.get::<Model>(e).unwrap().mesh
        };
        let shared = lid(ctx, roots[0]);
        assert_eq!(lid(ctx, roots[99]), shared);

        // Freed with the last one, not before.
        for &root in &roots[..99] {
            ctx.despawn_prefab(root);
        }
        assert_eq!(meshes_and_materials(ctx), one, "the last spawn still draws them");
        assert!(ctx.is_live(shared));
        ctx.despawn_prefab(roots[99]);
        assert_eq!(
            meshes_and_materials(ctx),
            empty,
            "meshes and material freed with the last spawn"
        );
        assert!(!ctx.is_live(shared));

        // Spawned again, the file makes them again.
        let again = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        assert_eq!(meshes_and_materials(ctx).0, 1);
        ctx.despawn_prefab(again);
    });
}

#[test]
fn nodes_of_one_gltf_mesh_share_one_uploaded_mesh_and_other_meshes_get_their_own() {
    // Six nodes of mesh 0 (two in a group, one scaled) and two of mesh 1.
    let file = TempAsset::with_contents(
        "rocks.gltf",
        &gltf(
            json!([
                { "name": "A", "mesh": 0 },
                { "name": "B", "mesh": 0, "translation": [2.0, 0.0, 0.0] },
                { "name": "C", "mesh": 0, "scale": [3.0, 1.0, 3.0] },
                { "name": "Pile", "children": [4, 5] },
                { "name": "D", "mesh": 0 },
                { "name": "E", "mesh": 0, "translation": [0.0, 1.0, 0.0] },
                { "name": "F", "mesh": 1 },
                { "name": "G", "mesh": 1, "translation": [0.0, 0.0, 4.0] },
                { "name": "H", "mesh": 0, "extras": { "ponko:Health": { "hp": 1.0 } } }
            ]),
            json!([]),
        ),
    );
    let one = TempAsset::with_contents("one.gltf", &gltf(json!([{ "name": "A", "mesh": 0 }]), json!([])));
    game().run_headless(0, |ctx| {
        reflect(ctx);
        let single = ctx.spawn_prefab(one.name(), Transform::default()).unwrap();
        let (_, one_mesh_bytes, _) = meshes_and_materials(ctx);
        ctx.despawn_prefab(single);
        assert_eq!(meshes_and_materials(ctx).0, 0);

        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let (meshes, bytes, materials) = meshes_and_materials(ctx);
        eprintln!(
            "nine nodes of two glTF meshes: {meshes} meshes, {bytes} bytes (one mesh: {one_mesh_bytes} bytes; one per node: {} bytes)",
            8 * one_mesh_bytes
        );
        assert_eq!(meshes, 2, "one uploaded mesh per glTF mesh, not one per node");
        assert_eq!(bytes, 2 * one_mesh_bytes);
        assert_eq!(materials, 1);
        let scene = ctx.world.get::<GltfRoot>(root).unwrap().clone();
        let mesh_of = |ctx: &mut Ctx, path: &str| ctx.world.get::<Model>(scene.entity(path).unwrap()).unwrap().mesh;
        let first = mesh_of(ctx, "A");
        for path in ["B", "C", "Pile/D", "Pile/E", "H"] {
            assert_eq!(mesh_of(ctx, path), first, "{path} draws mesh 0's one copy");
        }
        let second = mesh_of(ctx, "F");
        assert_ne!(second, first);
        assert_eq!(mesh_of(ctx, "G"), second);
        // Each node keeps its own place, scale and components.
        let c = scene.entity("C").unwrap();
        assert_eq!(ctx.world.get::<Transform>(c).unwrap().scale, vec3(3.0, 1.0, 3.0));
        assert_eq!(
            ctx.world.get::<Health>(scene.entity("H").unwrap()),
            Some(&Health { hp: 1.0 })
        );

        // The shared meshes go with the scene, once each.
        assert_eq!(ctx.despawn_prefab(root), 10);
        assert!(!ctx.is_live(first) && !ctx.is_live(second));
        assert_eq!(meshes_and_materials(ctx), (0, 0, 0));
    });
}

#[test]
fn a_read_scene_gives_every_node_of_a_shared_mesh_its_triangles() {
    let file = TempAsset::with_contents("prop.gltf", &prop());
    game().run_headless(0, |ctx| {
        let scene = GltfScene::load(ctx, file.name()).unwrap();
        for node in &scene.nodes {
            let mesh = node
                .mesh
                .as_ref()
                .unwrap_or_else(|| panic!("{} has its mesh", node.path));
            assert_eq!((mesh.positions.len(), mesh.indices.len()), (8, 36), "{}", node.path);
        }
    });
}

#[test]
fn a_bad_export_keeps_the_old_scene_drawing_its_meshes() {
    use std::sync::{Arc, Mutex};
    let file = TempAsset::with_contents("live.gltf", &two_boxes(0.0, 5.0, true));
    let broken = gltf(
        json!([{ "name": "Door", "mesh": 0, "extras": { "ponko:colider": "box" } }]),
        json!([]),
    );
    let live = Arc::new(Mutex::new(Vec::new()));
    let log = live.clone();
    let name = file.name().to_string();
    let path = file.path();
    game()
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            if tick == 2 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                std::fs::write(&path, &broken).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            if tick == 4 {
                let door = node(&mut ctx.world, "Door");
                let mesh = ctx.world.get::<Model>(door).unwrap().mesh;
                log.lock().unwrap().push(ctx.is_live(mesh));
            }
        })
        .run_headless(5, move |ctx| {
            reflect(ctx);
            ctx.spawn_prefab(&name, Transform::default()).unwrap();
        });
    assert_eq!(*live.lock().unwrap(), [true], "the old scene's mesh stays live");
}

/// A file of five nodes of one glTF mesh with colliders: two triangle
/// meshes at the same scale, one at another scale, a hull and a box.
#[cfg(feature = "physics3d")]
fn rock_colliders() -> String {
    gltf(
        json!([
            { "name": "A", "mesh": 0, "extras": { "ponko:collider": "trimesh" } },
            { "name": "B", "mesh": 0, "translation": [3.0, 0.0, 0.0], "extras": { "ponko:collider": "trimesh" } },
            { "name": "Big", "mesh": 0, "scale": [4.0, 1.0, 4.0], "extras": { "ponko:collider": "trimesh" } },
            { "name": "Hull", "mesh": 0, "translation": [0.0, 0.0, 3.0], "extras": { "ponko:collider": "hull" } },
            { "name": "Crate", "mesh": 0, "translation": [0.0, 3.0, 0.0], "extras": { "ponko:collider": "box" } }
        ]),
        json!([]),
    )
}

/// The collision mesh of `path`'s collider in the scene of `root`, scaled
/// or not.
#[cfg(feature = "physics3d")]
fn collision_mesh(ctx: &mut Ctx, root: Entity, path: &str) -> ponko::d3::CollisionMesh {
    let e = ctx.world.get::<GltfRoot>(root).unwrap().entity(path).unwrap();
    match ctx.world.get::<ponko::d3::Collider>(e).unwrap().shape {
        ponko::d3::Shape::Mesh(m) | ponko::d3::Shape::ScaledMesh { mesh: m, .. } => m,
        other => panic!("{path} has a {other:?}"),
    }
}

#[cfg(feature = "physics3d")]
#[test]
fn spawning_a_scene_again_reuses_its_collision_shapes_until_the_last_one_goes() {
    let file = TempAsset::with_contents("rocks.gltf", &rock_colliders());
    game().physics3d(Vec3::ZERO).run_headless(0, |ctx| {
        let first = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        assert_eq!(
            ctx.live_counts().collision_meshes,
            2,
            "A, B and Big share a triangle mesh (Big scales it where it is used); the hull makes its own"
        );
        let a = collision_mesh(ctx, first, "A");
        assert_eq!(collision_mesh(ctx, first, "B"), a);
        assert_eq!(collision_mesh(ctx, first, "Big"), a);
        let mut roots = vec![first];
        for i in 1..100 {
            let at = Transform::at(vec3(i as f32 * 20.0, 0.0, 0.0));
            roots.push(ctx.spawn_prefab(file.name(), at).unwrap());
        }
        assert_eq!(ctx.live_counts().collision_meshes, 2, "a hundred spawns share them");
        for path in ["A", "B", "Big", "Hull"] {
            assert_eq!(
                collision_mesh(ctx, roots[99], path),
                collision_mesh(ctx, first, path),
                "{path}"
            );
        }
        let hull = collision_mesh(ctx, first, "Hull");

        // Freed with the last scene that uses them, not before.
        for &root in &roots[..99] {
            ctx.despawn_prefab(root);
        }
        assert_eq!(ctx.live_counts().collision_meshes, 2);
        assert!(ctx.is_live(a) && ctx.is_live(hull));
        ctx.despawn_prefab(roots[99]);
        assert_eq!(ctx.live_counts().collision_meshes, 0, "freed with the last scene");
        assert!(!ctx.is_live(a) && !ctx.is_live(hull));

        // Spawned again, the file makes them again.
        let again = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        assert_eq!(ctx.live_counts().collision_meshes, 2);
        ctx.despawn_prefab(again);
        assert_eq!(ctx.live_counts().collision_meshes, 0);
    });
}

#[cfg(feature = "physics3d")]
#[test]
fn a_collision_shape_the_game_freed_is_made_again_for_the_next_spawn() {
    let file = TempAsset::with_contents("rocks.gltf", &rock_colliders());
    game().physics3d(Vec3::ZERO).run_headless(0, |ctx| {
        let first = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let freed = collision_mesh(ctx, first, "A");
        ctx.free(freed);
        let second = ctx
            .spawn_prefab(file.name(), Transform::at(vec3(20.0, 0.0, 0.0)))
            .unwrap();
        let made = collision_mesh(ctx, second, "A");
        assert_ne!(made, freed);
        assert!(ctx.is_live(made), "a live shape, not the freed one");
        assert_eq!(collision_mesh(ctx, second, "B"), made);
        assert_eq!(collision_mesh(ctx, second, "Hull"), collision_mesh(ctx, first, "Hull"));
        ctx.despawn_prefab(first);
        ctx.despawn_prefab(second);
        assert_eq!(ctx.live_counts().collision_meshes, 0);
    });
}

#[cfg(feature = "physics3d")]
#[test]
fn shared_collision_shapes_collide_in_every_scene() {
    use ponko::d3::{Collider, RigidBody};
    let file = TempAsset::with_contents("rocks.gltf", &rock_colliders());
    let mut world = game().physics3d(vec3(0.0, -9.81, 0.0)).run_headless(60, |ctx| {
        for x in [0.0, 50.0] {
            ctx.spawn_prefab(file.name(), Transform::at(vec3(x, 0.0, 0.0))).unwrap();
            // A ball dropped on each scene's rock A lands on it.
            ctx.world.spawn((
                Transform::at(vec3(x, 2.0, 0.0)),
                RigidBody::dynamic(),
                Collider::sphere(0.25),
            ));
        }
    });
    let balls: Vec<f32> = world
        .query::<(&Transform, &RigidBody)>()
        .iter()
        .filter(|(_, b)| b.kind == BodyKind::Dynamic)
        .map(|(t, _)| t.position.y)
        .collect();
    assert_eq!(balls.len(), 2);
    for y in balls {
        assert!(
            (0.6..1.0).contains(&y),
            "the ball rests on the rock's top (0.5 + 0.25): {y}"
        );
    }
}

#[cfg(feature = "physics3d")]
#[test]
fn reexporting_frees_the_old_collision_shapes_and_shares_the_new_ones() {
    use std::sync::{Arc, Mutex};
    let file = TempAsset::with_contents("rocks.gltf", &rock_colliders());
    let after = gltf(
        json!([
            { "name": "A", "mesh": 0, "scale": [2.0, 2.0, 2.0], "extras": { "ponko:collider": "trimesh" } },
            { "name": "B", "mesh": 0, "scale": [2.0, 2.0, 2.0], "extras": { "ponko:collider": "trimesh" } }
        ]),
        json!([]),
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let path = file.path();
    let name = file.name().to_string();
    game()
        .hot_reload(true)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let roots: Vec<Entity> = ctx
                .world
                .query::<(Entity, &GltfRoot)>()
                .iter()
                .map(|(e, _)| e)
                .collect();
            if tick == 2 {
                let old: Vec<_> = roots.iter().map(|&r| collision_mesh(ctx, r, "A")).collect();
                log.lock()
                    .unwrap()
                    .push((ctx.live_counts().collision_meshes, old, false));
                std::thread::sleep(std::time::Duration::from_millis(50));
                std::fs::write(&path, &after).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
            if tick == 4 {
                let new: Vec<_> = roots.iter().map(|&r| collision_mesh(ctx, r, "B")).collect();
                let old_live = log.lock().unwrap()[0].1.iter().any(|&m| ctx.is_live(m));
                log.lock()
                    .unwrap()
                    .push((ctx.live_counts().collision_meshes, new, old_live));
            }
        })
        .run_headless(5, move |ctx| {
            for x in [0.0, 30.0] {
                ctx.spawn_prefab(&name, Transform::at(vec3(x, 0.0, 0.0))).unwrap();
            }
        });
    let seen = seen.lock().unwrap();
    let (before, old, _) = &seen[0];
    assert_eq!(*before, 2);
    assert_eq!(old[0], old[1], "both scenes share the first export's shape");
    let (after, new, old_live) = &seen[1];
    assert_eq!(*after, 1, "A and B of both scenes share one new triangle mesh");
    assert_eq!(new[0], new[1]);
    assert!(!old_live, "the first export's shapes are freed");
}

#[cfg(feature = "physics3d")]
#[test]
fn loading_a_world_frees_the_collision_shapes_of_the_scenes_it_replaced() {
    let file = TempAsset::with_contents("rocks.gltf", &rock_colliders());
    game().physics3d(Vec3::ZERO).run_headless(0, |ctx| {
        let empty_world = ctx.world.save();
        for i in 0..3 {
            ctx.spawn_prefab(file.name(), Transform::at(vec3(i as f32 * 20.0, 0.0, 0.0)))
                .unwrap();
        }
        assert_eq!(ctx.live_counts().collision_meshes, 2);
        let scene = TempAsset::with_contents("empty.json", &empty_world.to_string());
        ctx.load_scene(scene.name()).unwrap();
        assert_eq!(ctx.live_counts().collision_meshes, 0);
    });
}

#[test]
fn a_mesh_loaded_from_the_same_file_keeps_its_materials_when_the_scenes_go() {
    let file = TempAsset::with_contents("prop.gltf", &prop());
    game().run_headless(0, |ctx| {
        let mesh = ctx.gfx.load_mesh(file.name());
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        ctx.despawn_prefab(root);
        assert!(ctx.is_live(mesh));
        assert_eq!(
            meshes_and_materials(ctx).2,
            1,
            "load_mesh's mesh still draws with the material"
        );
    });
}

#[test]
fn loading_a_world_frees_the_meshes_of_the_scenes_it_replaced() {
    let file = TempAsset::with_contents("prop.gltf", &prop());
    game().run_headless(0, |ctx| {
        let empty_world = ctx.world.save();
        let empty = meshes_and_materials(ctx);
        for i in 0..3 {
            ctx.spawn_prefab(file.name(), Transform::at(vec3(i as f32, 0.0, 0.0)))
                .unwrap();
        }
        let scene = TempAsset::with_contents("empty.json", &empty_world.to_string());
        ctx.load_scene(scene.name()).unwrap();
        assert_eq!(meshes_and_materials(ctx), empty);
    });
}

// ---- saved prefab instances --------------------------------------------------------

/// The barrels in `world`: where each is, whether it is full, and the node
/// it is the instance of (if any).
fn barrels(world: &mut World) -> Vec<(Vec3, bool, Option<String>)> {
    let mut out: Vec<(Vec3, bool, Option<String>)> = world
        .query::<(&Transform, &Barrel, Option<&GltfNode>)>()
        .iter()
        .map(|(t, b, n)| (t.position, b.full, n.map(|n| n.path.clone())))
        .collect();
    out.sort_by(|a, b| a.0.x.total_cmp(&b.0.x).then(a.0.z.total_cmp(&b.0.z)));
    out
}

// ANCHOR: saved_level
/// Saves the game in slot `slot`: the barrels, the player, and a record of
/// the level they belong to.
fn save_game(ctx: &mut Ctx, slot: &str) -> Result<(), Error> {
    ctx.storage.save_world(&ctx.world, slot)
}

/// Loads slot `slot` and spawns the level `level` around what it holds.
fn load_game(ctx: &mut Ctx, slot: &str, level: &str) -> Result<Entity, Error> {
    ctx.load_world(slot)?; // the barrels and the player, as they were...
    ctx.spawn_prefab(level, Transform::default()) // ...and the level, which adopts the barrels
}
// ANCHOR_END: saved_level

#[test]
fn a_reloaded_save_hands_its_prefab_instances_back_to_their_nodes() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let dir = std::env::temp_dir().join(format!("ponko-gltf-saves-{}", std::process::id()));
    let mut world = game().data_dir(&dir).run_headless(0, |ctx| {
        reflect(ctx);
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let a = play_barrel_level(ctx, root);
        save_game(ctx, "barrels").unwrap();

        // Later, or in the next run.
        let root = load_game(ctx, "barrels", file.name()).unwrap();
        let scene = ctx.world.get::<GltfRoot>(root).unwrap().clone();
        assert_eq!(scene.entity("BarrelA"), Some(a), "the saved entity is the node's again");
        assert_eq!(scene.entity("BarrelB"), None, "a despawned instance stays despawned");
        assert!(scene.entity("Cellar/BarrelC").is_some());
        assert!(!scene.options.no_saved_objects);
    });
    let _ = std::fs::remove_dir_all(dir);
    assert_eq!(
        barrels(&mut world),
        [
            (vec3(0.0, -3.0, -2.0), true, Some("Cellar/BarrelC".into())),
            (vec3(2.0, 0.0, 5.0), false, Some("BarrelA".into())),
        ],
        "two barrels, as saved: A where the game moved it and empty, C untouched; none doubled"
    );
}

#[test]
fn a_saved_scene_survives_more_than_one_save_and_load() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let save = TempAsset::new("save.json");
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        play_barrel_level(ctx, root);
        for _ in 0..3 {
            std::fs::write(save.path(), ctx.world.save().to_string()).unwrap();
            ctx.load_scene(save.name()).unwrap();
            ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        }
    });
    assert_eq!(barrels(&mut world).len(), 2);
    assert_eq!(world.query::<&GltfRoot>().iter().count(), 1);
}

#[test]
fn without_saved_objects_still_loads_a_save_and_now_links_its_instances() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let save = TempAsset::new("save.json");
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        play_barrel_level(ctx, root);
        std::fs::write(save.path(), ctx.world.save().to_string()).unwrap();
        ctx.load_scene(save.name()).unwrap();
        let options = ponko::d3::GltfOptions::default().without_saved_objects();
        GltfRoot::spawn(ctx, file.name(), Transform::default(), options).unwrap();
    });
    let found = barrels(&mut world);
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|b| b.2.is_some()), "{found:?}");
}

#[test]
fn of_two_saved_scenes_of_one_file_each_spawn_adopts_the_nearest() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let save = TempAsset::new("save.json");
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        let west = ctx
            .spawn_prefab(file.name(), Transform::at(vec3(-100.0, 0.0, 0.0)))
            .unwrap();
        ctx.spawn_prefab(file.name(), Transform::at(vec3(100.0, 0.0, 0.0)))
            .unwrap();
        let a = ctx.world.get::<GltfRoot>(west).unwrap().entity("BarrelA").unwrap();
        ctx.world.insert(a, Barrel { full: false });
        std::fs::write(save.path(), ctx.world.save().to_string()).unwrap();
        ctx.load_scene(save.name()).unwrap();
        // Spawned again in the other order.
        ctx.spawn_prefab(file.name(), Transform::at(vec3(100.0, 0.0, 0.0)))
            .unwrap();
        let west = ctx
            .spawn_prefab(file.name(), Transform::at(vec3(-100.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(ctx.world.get::<GltfRoot>(west).unwrap().entity("BarrelA"), Some(a));
    });
    assert_eq!(barrels(&mut world).len(), 6, "three in each scene, none doubled");
    assert_eq!(barrels(&mut world).iter().filter(|b| !b.1).count(), 1);
}

#[test]
fn a_save_without_scenes_spawns_prefab_nodes_fresh() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let save = TempAsset::new("save.json");
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.world.spawn((Transform::default(), Health { hp: 1.0 }));
        std::fs::write(save.path(), ctx.world.save().to_string()).unwrap();
        ctx.load_scene(save.name()).unwrap();
        ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
    });
    assert_eq!(barrels(&mut world).len(), 3);
}

/// Migration: `gltf_instances_v1.json` was saved by `dev` before saves
/// linked prefab instances to their scene ([`write_gltf_instances_v1_fixture`]).
/// Spawning the level after loading it, with the default options, leaves
/// the prefab nodes to the save, as `without_saved_objects` did: nothing
/// is doubled and the despawned barrel doesn't come back. Saving again
/// keeps it that way.
#[test]
fn an_old_save_loads_without_doubling_its_prefab_instances() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/saves/gltf_instances_v1.json"
    );
    let old = TempAsset::with_contents("old-save.json", &std::fs::read_to_string(fixture).unwrap());
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let resaved = TempAsset::new("save.json");
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.load_scene(old.name()).unwrap();
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        let scene = ctx.world.get::<GltfRoot>(root).unwrap().clone();
        assert!(scene.options.no_saved_objects, "the old rule, for this scene");
        assert_eq!(scene.entity("BarrelA"), None, "unlinked: they can't be told apart");
        assert_eq!(barrels(&mut ctx.world).len(), 2);

        // Saved in the new format and loaded again: still two.
        std::fs::write(resaved.path(), ctx.world.save().to_string()).unwrap();
        ctx.load_scene(resaved.name()).unwrap();
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        assert!(!ctx.world.get::<GltfRoot>(root).unwrap().options.no_saved_objects);
    });
    assert_eq!(
        barrels(&mut world),
        [(vec3(0.0, -3.0, -2.0), true, None), (vec3(2.0, 0.0, 5.0), false, None)],
        "the saved barrels as they were, none spawned again"
    );
    let player = world
        .query::<(&Transform, &Health)>()
        .iter()
        .next()
        .map(|(t, h)| (t.position, *h));
    assert_eq!(player, Some((vec3(0.0, 0.0, 8.0), Health { hp: 3.0 })));
}

/// Writes `tools/blender/fixtures/ponko-schema.json`, the schema the
/// Blender add-on's tests read: what `ponko schema` writes for a game that
/// reflects `Health` (described: `{"hp": number}`, nothing else) and
/// `Barrel`. Run again when the convention changes:
/// `PONKO_SCHEMA_FIXTURE_OUT=$PWD/tools/blender/fixtures/ponko-schema.json
/// cargo test -p ponko --features 3d --test gltf_scenes -- --ignored --exact write_blender_schema_fixture`.
#[test]
#[ignore = "writes the Blender add-on's test schema"]
fn write_blender_schema_fixture() {
    use ponko::reflect::ValueSchema;
    let out = std::env::var("PONKO_SCHEMA_FIXTURE_OUT").expect("set PONKO_SCHEMA_FIXTURE_OUT to the file to write");
    game().run_headless(0, move |ctx| {
        reflect(ctx);
        ctx.world
            .describe_component("Health", ValueSchema::strict_object([("hp", ValueSchema::Number)]))
            .unwrap();
        let schema = ponko::d3::gltf_properties_schema(&ctx.world);
        std::fs::write(&out, serde_json::to_string_pretty(&schema).unwrap() + "\n").unwrap();
    });
}

#[test]
fn the_blender_add_on_test_schema_matches_what_the_engine_writes() {
    use ponko::reflect::ValueSchema;
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/blender/fixtures/ponko-schema.json");
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(fixture).unwrap()).unwrap();
    game().run_headless(0, move |ctx| {
        reflect(ctx);
        ctx.world
            .describe_component("Health", ValueSchema::strict_object([("hp", ValueSchema::Number)]))
            .unwrap();
        let now = ponko::d3::gltf_properties_schema(&ctx.world);
        // The convention's keys and the game's components; the engine's own
        // reflected components may come and go.
        for key in [
            "ponko:collider",
            "ponko:body",
            "ponko:lod",
            "ponko:layers",
            "ponko:Health",
            "ponko:Barrel",
        ] {
            assert_eq!(
                saved["properties"][key], now["properties"][key],
                "{key}: regenerate the fixture with write_blender_schema_fixture"
            );
        }
    });
}

#[test]
fn a_save_loads_in_a_fresh_game_and_games_without_scenes_have_no_record() {
    let prefab = barrel_prefab();
    let file = TempAsset::with_contents("level.gltf", &barrel_level(prefab.name()));
    let save = TempAsset::new("save.json");
    let path = save.path();
    game().run_headless(0, |ctx| {
        reflect(ctx);
        assert!(
            !ctx.world.reflected_resources().contains(&"GltfSceneSaves"),
            "not listed before a glTF scene is spawned"
        );
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        play_barrel_level(ctx, root);
        std::fs::write(&path, ctx.world.save().to_string()).unwrap();
    });
    // The next run: nothing glTF happened yet when the save loads.
    let mut world = game().run_headless(0, |ctx| {
        reflect(ctx);
        ctx.load_scene(save.name()).unwrap();
        let root = ctx.spawn_prefab(file.name(), Transform::default()).unwrap();
        assert!(ctx.world.get::<GltfRoot>(root).unwrap().entity("BarrelA").is_some());
    });
    assert_eq!(barrels(&mut world).len(), 2);
}
