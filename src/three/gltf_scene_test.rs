//! glTF scenes parse a file once while it is unchanged: later spawns use
//! what the first one read, as long as the file and its buffers are as
//! they were and the scenes' meshes are live. Counted with
//! `Assets3D::gltf_scene_parses`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use glam::{Vec3, vec3};
use serde_json::{Value, json};

use super::Transform3D;
use crate::ctx::Ctx;
use crate::d3::GltfRoot;
use crate::game::Game;

/// Files in the engine's `assets/` folder, deleted when dropped.
struct Files {
    names: Vec<String>,
}

impl Files {
    /// `scene.gltf` and its buffer `scene.bin` under unique names: the
    /// scene's name and the buffer's.
    fn new() -> (Self, String, String) {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let stem = format!("test-{}-{n}-parse", std::process::id());
        let (gltf, bin) = (format!("{stem}.gltf"), format!("{stem}.bin"));
        let files = Self {
            names: vec![gltf.clone(), bin.clone()],
        };
        (files, gltf, bin)
    }

    fn path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join(name)
    }

    fn write(name: &str, bytes: &[u8]) {
        std::fs::write(Self::path(name), bytes).expect("the test writes its asset");
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        for name in &self.names {
            let _ = std::fs::remove_file(Self::path(name));
        }
    }
}

/// A box from `lo` to `hi`: its corners and triangles, as a glTF buffer.
fn box_buffer(lo: Vec3, hi: Vec3) -> Vec<u8> {
    let corners = [
        [lo.x, lo.y, lo.z],
        [hi.x, lo.y, lo.z],
        [hi.x, hi.y, lo.z],
        [lo.x, hi.y, lo.z],
        [lo.x, lo.y, hi.z],
        [hi.x, lo.y, hi.z],
        [hi.x, hi.y, hi.z],
        [lo.x, hi.y, hi.z],
    ];
    let indices: [u16; 36] = [
        0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5,
    ];
    let mut bytes = Vec::new();
    for v in corners.into_iter().flatten() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for i in indices {
        bytes.extend_from_slice(&i.to_le_bytes());
    }
    bytes
}

/// A `.gltf` of `nodes` (all at the top of the scene; `"mesh": 0` is the
/// box in buffer file `bin`).
fn gltf(nodes: Value, bin: &str, lo: Vec3, hi: Vec3) -> String {
    json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": (0..nodes.as_array().unwrap().len()).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }],
        "buffers": [{ "byteLength": 168, "uri": bin }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": 96 },
            { "buffer": 0, "byteOffset": 96, "byteLength": 72 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": lo.to_array(), "max": hi.to_array() },
            { "bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR" }
        ]
    })
    .to_string()
}

/// Two nodes of the box, the first `x` along.
fn two_rocks(bin: &str, x: f32) -> String {
    let half = Vec3::splat(0.5);
    gltf(
        json!([
            { "name": "A", "mesh": 0, "translation": [x, 0.0, 0.0] },
            { "name": "B", "mesh": 0, "translation": [0.0, 0.0, 3.0] }
        ]),
        bin,
        -half,
        half,
    )
}

fn parses(ctx: &Ctx) -> u64 {
    ctx.gfx.assets3d.gltf_scene_parses
}

fn spawn(ctx: &mut Ctx, name: &str, x: f32) -> ponko_ecs::Entity {
    GltfRoot::spawn(ctx, name, Transform3D::at(vec3(x, 0.0, 0.0)), Default::default()).expect("the scene spawns")
}

/// Where node `path` of the scene of `root` is in the file (its root at x).
fn node_x(ctx: &mut Ctx, root: ponko_ecs::Entity, path: &str, at: f32) -> f32 {
    let e = ctx.world.get::<GltfRoot>(root).unwrap().entity(path).unwrap();
    ctx.world.get::<Transform3D>(e).unwrap().position.x - at
}

#[test]
fn spawning_an_unchanged_file_again_does_not_parse_it() {
    let (_files, name, bin) = Files::new();
    Files::write(&bin, &box_buffer(Vec3::splat(-0.5), Vec3::splat(0.5)));
    Files::write(&name, two_rocks(&bin, 0.0).as_bytes());
    Game::new("parse", 1, 1).run_headless(0, |ctx| {
        let roots: Vec<_> = (0..3).map(|i| spawn(ctx, &name, i as f32 * 10.0)).collect();
        assert_eq!(parses(ctx), 1, "three spawns, one parse");
        for (i, &root) in roots.iter().enumerate() {
            assert_eq!(
                node_x(ctx, root, "B", i as f32 * 10.0),
                0.0,
                "each scene has every node"
            );
            assert!(ctx.world.get::<GltfRoot>(root).unwrap().entity("A").is_some());
        }

        // What was read goes with the last scene; a spawn after it reads
        // the file again.
        for root in roots {
            ctx.despawn_prefab(root);
        }
        spawn(ctx, &name, 0.0);
        assert_eq!(parses(ctx), 2);
        spawn(ctx, &name, 10.0);
        assert_eq!(parses(ctx), 2);
    });
}

#[test]
fn a_file_or_buffer_changed_on_disk_is_parsed_again_by_the_next_spawn() {
    let (_files, name, bin) = Files::new();
    let half = Vec3::splat(0.5);
    Files::write(&bin, &box_buffer(-half, half));
    Files::write(&name, two_rocks(&bin, 0.0).as_bytes());
    Game::new("parse", 1, 1).run_headless(0, |ctx| {
        let first = spawn(ctx, &name, 0.0);
        // The file changes without hot reload: a node moved.
        Files::write(&name, two_rocks(&bin, 2.0).as_bytes());
        let second = spawn(ctx, &name, 10.0);
        assert_eq!(parses(ctx), 2, "the changed file is read again");
        assert_eq!(
            node_x(ctx, second, "A", 10.0),
            2.0,
            "the new spawn has the file as it is now"
        );
        assert_eq!(node_x(ctx, first, "A", 0.0), 0.0, "the first scene stays as it was");
        spawn(ctx, &name, 20.0);
        assert_eq!(parses(ctx), 2, "unchanged again: not read again");

        // Only the buffer changes: a bigger box, same size of file.
        Files::write(&bin, &box_buffer(-half * 3.0, half * 3.0));
        spawn(ctx, &name, 30.0);
        assert_eq!(parses(ctx), 3, "a changed buffer is read again");
        spawn(ctx, &name, 40.0);
        assert_eq!(parses(ctx), 3);
    });
}

#[test]
fn a_mesh_the_game_freed_makes_the_next_spawn_read_the_file() {
    let (_files, name, bin) = Files::new();
    Files::write(&bin, &box_buffer(Vec3::splat(-0.5), Vec3::splat(0.5)));
    Files::write(&name, two_rocks(&bin, 0.0).as_bytes());
    Game::new("parse", 1, 1).run_headless(0, |ctx| {
        let first = spawn(ctx, &name, 0.0);
        let a = ctx.world.get::<GltfRoot>(first).unwrap().entity("A").unwrap();
        let mesh = ctx.world.get::<crate::render::Model>(a).unwrap().mesh;
        ctx.free(mesh);
        let second = spawn(ctx, &name, 10.0);
        assert_eq!(parses(ctx), 2, "a mesh to make again: the file is read whole");
        let a = ctx.world.get::<GltfRoot>(second).unwrap().entity("A").unwrap();
        let made = ctx.world.get::<crate::render::Model>(a).unwrap().mesh;
        assert!(ctx.is_live(made) && made != mesh);
    });
}

#[test]
fn reexporting_a_file_spawned_three_times_parses_it_once() {
    use std::sync::{Arc, Mutex};
    let (_files, name, bin) = Files::new();
    Files::write(&bin, &box_buffer(Vec3::splat(-0.5), Vec3::splat(0.5)));
    Files::write(&name, two_rocks(&bin, 0.0).as_bytes());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let (gltf_path, after) = (Files::path(&name), two_rocks(&bin, 2.0));
    let scene = name.clone();
    Game::new("parse", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.assets.reloaded_files().contains(&scene) {
                let roots: Vec<_> = ctx
                    .world
                    .query::<(ponko_ecs::Entity, &GltfRoot)>()
                    .iter()
                    .map(|(e, _)| e)
                    .collect();
                let moved: Vec<f32> = roots
                    .iter()
                    .map(|&r| {
                        let at = ctx.world.get::<Transform3D>(r).unwrap().position.x;
                        node_x(ctx, r, "A", at)
                    })
                    .collect();
                log.lock().unwrap().push((parses(ctx), moved));
            }
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                std::fs::write(&gltf_path, &after).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        })
        .run_headless(5, move |ctx| {
            for i in 0..3 {
                spawn(ctx, &name, i as f32 * 10.0);
            }
            assert_eq!(parses(ctx), 1);
        });
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1, "reloaded once: {seen:?}");
    let (count, moved) = &seen[0];
    assert_eq!(*count, 2, "the export is read once for its three scenes");
    assert_eq!(moved, &[2.0, 2.0, 2.0], "every scene has the new export");
}
