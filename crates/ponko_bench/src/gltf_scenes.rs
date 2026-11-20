//! glTF scenes (`GltfRoot::spawn`): a file whose nodes repeat one rock
//! mesh, some with triangle mesh, convex hull and box colliders, spawned
//! once and a hundred times. Reports the time, the meshes and collision
//! meshes held, the meshes' estimated bytes and the heap the spawns keep.
use std::time::{Duration, Instant};

use ponko::assets::MemorySource;
use ponko::d3::{GltfOptions, GltfRoot, Transform as Transform3D};
use ponko::inventory::AssetClass;
use ponko::prelude::{Game, Vec3, vec3};

use crate::Bench;
use crate::alloc::live_bytes;
use crate::report;

/// Rock nodes in the file, all of the one mesh.
const ROCKS: usize = 12;
/// Grid points along each side of the rock mesh.
const GRID: usize = 32;
/// Scenes in the big case.
const SPAWNS: usize = 100;

/// A `.glb` of `ROCKS` nodes sharing one `GRID` x `GRID` rock mesh: four
/// with a triangle mesh collider, two with a hull, two with a box, four
/// without (all at the same scale, so their shapes match).
pub fn rocks_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for z in 0..GRID {
        for x in 0..GRID {
            let (fx, fz) = (x as f32 / (GRID - 1) as f32 - 0.5, z as f32 / (GRID - 1) as f32 - 0.5);
            let y = 0.3 * ((fx * 9.0).sin() * (fz * 7.0).cos());
            for (k, v) in [fx * 4.0, y, fz * 4.0].into_iter().enumerate() {
                lo[k] = lo[k].min(v);
                hi[k] = hi[k].max(v);
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    let positions_len = bin.len();
    let mut indices = 0;
    for z in 0..GRID - 1 {
        for x in 0..GRID - 1 {
            let i = (z * GRID + x) as u16;
            let g = GRID as u16;
            for v in [i, i + g, i + 1, i + 1, i + g, i + g + 1] {
                bin.extend_from_slice(&v.to_le_bytes());
                indices += 1;
            }
        }
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let colliders = ["trimesh", "trimesh", "trimesh", "trimesh", "hull", "hull", "box", "box"];
    let nodes: Vec<serde_json::Value> = (0..ROCKS)
        .map(|i| {
            let mut node = serde_json::json!({
                "name": format!("Rock{i}"),
                "mesh": 0,
                "translation": [(i % 4) as f32 * 5.0, 0.0, (i / 4) as f32 * 5.0],
            });
            if let Some(kind) = colliders.get(i) {
                node["extras"] = serde_json::json!({ "ponko:collider": kind });
            }
            node
        })
        .collect();
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": (0..ROCKS).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }],
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": positions_len },
            { "buffer": 0, "byteOffset": positions_len, "byteLength": indices * 2 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": GRID * GRID, "type": "VEC3", "min": lo, "max": hi },
            { "bufferView": 1, "componentType": 5123, "count": indices, "type": "SCALAR" }
        ]
    });
    let mut json = json.to_string().into_bytes();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"JSON");
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    glb.extend_from_slice(b"BIN\0");
    glb.extend_from_slice(&bin);
    glb
}

/// What `spawns` scenes of the file hold, measured in one fresh game.
struct Sample {
    time: Duration,
    meshes: usize,
    mesh_bytes: u64,
    collision_meshes: usize,
    heap: i64,
}

fn sample(glb: &[u8], spawns: usize) -> Sample {
    let glb = glb.to_vec();
    let out = std::rc::Rc::new(std::cell::Cell::new(None));
    let put = out.clone();
    Game::new("bench", 1, 1)
        .physics3d(Vec3::ZERO)
        .run_headless(0, move |ctx| {
            ctx.assets.add_source(MemorySource::new("bench").with("rocks.glb", glb));
            let heap = live_bytes();
            let t = Instant::now();
            for i in 0..spawns {
                let at = Transform3D::at(vec3((i % 10) as f32 * 30.0, 0.0, (i / 10) as f32 * 30.0));
                GltfRoot::spawn(ctx, "rocks.glb", at, GltfOptions::default()).expect("the bench file spawns");
            }
            let time = t.elapsed();
            let heap = live_bytes() - heap;
            let inventory = ctx.asset_inventory();
            let mesh = inventory.get(AssetClass::Mesh);
            put.set(Some(Sample {
                time,
                meshes: mesh.live,
                mesh_bytes: mesh.bytes,
                collision_meshes: ctx.live_counts().collision_meshes,
                heap,
            }));
        });
    out.take().expect("the setup ran")
}

pub fn run(b: &mut Bench) {
    let glb = rocks_glb();
    for spawns in [1, SPAWNS] {
        let name = format!("gltf/spawn-{spawns}");
        if b.skips(&name) {
            continue;
        }
        const RUNS: usize = 7;
        let samples: Vec<Sample> = (0..RUNS + 1).map(|_| sample(&glb, spawns)).skip(1).collect();
        let mut times: Vec<Duration> = samples.iter().map(|s| s.time).collect();
        b.report(report::Record::new(&name, report::Kind::Fresh, 1, &mut times));
        let mut heaps: Vec<i64> = samples.iter().map(|s| s.heap).collect();
        heaps.sort_unstable();
        let s = &samples[0];
        println!(
            "    {spawns} scene(s) of {ROCKS} rock nodes: {} meshes ({:.1} KiB), {} collision meshes, {:.1} KiB heap kept (median)",
            s.meshes,
            s.mesh_bytes as f64 / 1024.0,
            s.collision_meshes,
            heaps[heaps.len() / 2] as f64 / 1024.0
        );
    }
}
