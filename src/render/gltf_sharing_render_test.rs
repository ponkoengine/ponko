//! glTF scenes on the GPU, drawn offscreen by the production renderer: a
//! file whose eight nodes use one glTF mesh holds one mesh's vertex and
//! index buffers on the GPU and draws it eight times, where the same eight
//! nodes with a glTF mesh each (the negative control, and what every file
//! cost before nodes shared meshes) hold eight. Despawning the scene frees
//! the GPU buffers. Skips without a GPU (a skip is not a pass).
use crate::assets::MemorySource;
use crate::d3::{GltfOptions, GltfRoot, Transform as Transform3D};
use crate::prelude::*;
use crate::render::FrameStatus;

const SIZE: u32 = 128;
const NODES: usize = 8;
/// Grid points along each side of the test mesh.
const GRID: usize = 24;

/// A `.glb` of `NODES` nodes in a row in front of the camera, each a
/// `GRID` x `GRID` plate: of one glTF mesh when `shared`, else of a glTF
/// mesh each (with the same data).
fn plates(shared: bool) -> Vec<u8> {
    let mut bin = Vec::new();
    for z in 0..GRID {
        for x in 0..GRID {
            let (fx, fz) = (x as f32 / (GRID - 1) as f32 - 0.5, z as f32 / (GRID - 1) as f32 - 0.5);
            for v in [fx, fz, 0.0f32] {
                bin.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    let positions = bin.len();
    let mut count = 0;
    for z in 0..GRID - 1 {
        for x in 0..GRID - 1 {
            let (i, g) = ((z * GRID + x) as u16, GRID as u16);
            for v in [i, i + 1, i + g, i + 1, i + g + 1, i + g] {
                bin.extend_from_slice(&v.to_le_bytes());
                count += 1;
            }
        }
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let meshes = if shared { 1 } else { NODES };
    let nodes: Vec<serde_json::Value> = (0..NODES)
        .map(|i| {
            serde_json::json!({
                "name": format!("Plate{i}"),
                "mesh": if shared { 0 } else { i },
                "translation": [i as f32 * 1.1 - 3.85, 0.0, 0.0],
            })
        })
        .collect();
    let mesh = serde_json::json!({ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }] });
    let json = serde_json::json!({
        "asset": { "version": "2.0" },
        "scene": 0,
        "scenes": [{ "nodes": (0..NODES).collect::<Vec<_>>() }],
        "nodes": nodes,
        "meshes": vec![mesh; meshes],
        "materials": [{ "emissiveFactor": [1.0, 1.0, 1.0], "doubleSided": true }],
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": positions },
            { "buffer": 0, "byteOffset": positions, "byteLength": count * 2 }
        ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": GRID * GRID, "type": "VEC3",
              "min": [-0.5, -0.5, 0.0], "max": [0.5, 0.5, 0.0] },
            { "bufferView": 1, "componentType": 5123, "count": count, "type": "SCALAR" }
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

/// What one spawn of `file` holds on the GPU and draws: (GPU meshes and
/// their bytes above an empty frame's, models drawn per mesh, sorted,
/// and the GPU meshes left once the scene is despawned). `None` without
/// a GPU.
fn spawned(file: &str) -> Option<(usize, u64, Vec<usize>, usize)> {
    let mut ctx = super::render_test_support::offscreen_ctx((SIZE, SIZE))?;
    ctx.assets.add_source(
        MemorySource::new("mem")
            .with("shared.glb", plates(true))
            .with("apart.glb", plates(false)),
    );
    ctx.scene3d.sky.atmosphere = false;
    ctx.scene3d.camera.position = vec3(0.0, 0.0, 9.0);
    ctx.scene3d.camera.look_at(Vec3::ZERO);
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let gpu = |ctx: &Ctx| {
        ctx.asset_inventory()
            .gpu
            .expect("an offscreen renderer reports its GPU memory")
    };
    let empty = gpu(&ctx);
    let root = GltfRoot::spawn(&mut ctx, file, Transform3D::default(), GltfOptions::default()).unwrap();
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let held = gpu(&ctx);
    let mut costs = Vec::new();
    ctx.gfx.mesh_costs_into(&mut costs);
    let mut models: Vec<usize> = costs.iter().map(|c| c.models).collect();
    models.sort_unstable();
    ctx.despawn_prefab(root);
    assert_eq!(ctx.render_frame(), FrameStatus::Offscreen);
    let left = gpu(&ctx).meshes - empty.meshes;
    Some((
        held.meshes - empty.meshes,
        held.mesh_bytes - empty.mesh_bytes,
        models,
        left,
    ))
}

#[test]
fn nodes_of_one_gltf_mesh_hold_one_mesh_on_the_gpu_and_draw_it_for_each_node() {
    let Some((meshes, bytes, models, left)) = spawned("shared.glb") else {
        return;
    };
    // Negative control: a glTF mesh per node, a GPU mesh per node.
    let (apart_meshes, apart_bytes, apart_models, apart_left) = spawned("apart.glb").expect("the GPU is still there");
    eprintln!(
        "{NODES} nodes of one glTF mesh: {meshes} GPU mesh(es), {bytes} bytes; \
         of a glTF mesh each: {apart_meshes} GPU meshes, {apart_bytes} bytes"
    );
    assert_eq!(apart_meshes, NODES);
    assert_eq!(apart_models, vec![1; NODES], "each mesh drawn once");
    assert_eq!(meshes, 1, "one GPU mesh for the nodes of one glTF mesh");
    assert_eq!(models, [NODES], "the one mesh drawn for every node");
    // The arena's buffers keep a quarter spare, rounded for alignment.
    assert!(
        (bytes * NODES as u64).abs_diff(apart_bytes) * 100 < apart_bytes,
        "an eighth of the GPU memory: {bytes} bytes, {apart_bytes} apart"
    );
    assert_eq!((left, apart_left), (0, 0), "freed with the scene");
}
