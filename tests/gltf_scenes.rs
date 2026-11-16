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
/// cube around the node's origin), every node without a parent at the top
/// of the scene, and `lights` as `KHR_lights_punctual` lights.
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
        "meshes": [{ "name": "cube", "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] }],
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
    assert!(matches!(ground.shape, Shape::Mesh(_)));
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
                ctx.spawn_prefab(&name, Transform::default()).unwrap();
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
