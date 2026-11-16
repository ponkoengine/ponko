//! Tests of model cooking, on GLB files built here.

use super::*;

/// A GLB of `json` with `bin` as its binary chunk.
pub fn glb(json: &Value, bin: &[u8]) -> Vec<u8> {
    let mut text = serde_json::to_vec(json).unwrap();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + text.len() + 8 + bin.len();
    let mut out = Vec::new();
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// A wavy `n` x `n` grid of quads as glTF JSON and its buffer: positions,
/// normals and texture coordinates, triangles in a scrambled order, a node
/// with extras and an animation whose accessors aren't vertex data.
pub fn grid(n: u32) -> (Value, Vec<u8>) {
    let side = n + 1;
    let (mut pos, mut nrm, mut uv) = (Vec::new(), Vec::new(), Vec::new());
    for y in 0..side {
        for x in 0..side {
            let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
            let h = (fx * 6.0).sin() * 0.2;
            pos.extend([fx * 4.0, h, fy * 4.0]);
            let normal = [-(fx * 6.0).cos() * 0.3, 1.0, 0.0f32];
            let len = normal.iter().map(|c| c * c).sum::<f32>().sqrt();
            nrm.extend(normal.map(|c| c / len));
            uv.extend([fx, fy]);
        }
    }
    let mut tris: Vec<[u32; 3]> = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let i = y * side + x;
            tris.push([i, i + side, i + 1]);
            tris.push([i + 1, i + side, i + side + 1]);
        }
    }
    // A fixed scramble, so the order is bad for the cache.
    let len = tris.len();
    let mut scrambled = Vec::with_capacity(len);
    for k in 0..len {
        scrambled.push(tris[(k * 7919) % len]);
    }
    let indices: Vec<u32> = scrambled.into_iter().flatten().collect();
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut push = |bytes: Vec<u8>, target: u32| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target}));
        bin.extend(bytes);
    };
    let f32s = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
    push(f32s(&pos), 34962);
    push(f32s(&nrm), 34962);
    push(f32s(&uv), 34962);
    push(indices.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect(), 34963);
    push(f32s(&[0.0, 1.0]), 0);
    push(f32s(&[0.0, 0.0, 0.0, 1.0, 0.0, 0.0]), 0);
    for v in &mut views[4..] {
        v.as_object_mut().unwrap().remove("target");
    }
    let count = side * side;
    let json = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "ground", "extras": {"collider": "mesh", "spawn": [1, 2, 3]}}],
        "meshes": [{"primitives": [{
            "attributes": {"POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2},
            "indices": 3
        }]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": count, "type": "VEC3",
             "min": [0.0, -0.2, 0.0], "max": [4.0, 0.2, 4.0]},
            {"bufferView": 1, "componentType": 5126, "count": count, "type": "VEC3"},
            {"bufferView": 2, "componentType": 5126, "count": count, "type": "VEC2"},
            {"bufferView": 3, "componentType": 5123, "count": indices.len(), "type": "SCALAR"},
            {"bufferView": 4, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0.0], "max": [1.0]},
            {"bufferView": 5, "componentType": 5126, "count": 2, "type": "VEC3"}
        ],
        "animations": [{"channels": [{"sampler": 0, "target": {"node": 0, "path": "translation"}}],
                        "samplers": [{"input": 4, "output": 5}]}],
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    (json, bin)
}

/// Every triangle of the file's first primitive as its corners' (position,
/// normal, uv), rotated to start at its smallest corner, sorted: the same
/// list for the same surface whatever the order of triangles and vertices.
fn surface(file: &[u8]) -> Vec<[[f32; 8]; 3]> {
    let (json, bin) = split_glb(file).unwrap();
    let doc = Doc {
        json,
        buffers: vec![bin.unwrap()],
    };
    let prim = &doc.json["meshes"][0]["primitives"][0];
    let read = |semantic: &str, comps: usize| -> Vec<f32> {
        let a = prim["attributes"][semantic].as_u64().unwrap() as usize;
        let l = doc.layout(a).unwrap().unwrap();
        let bytes = doc.elements(&l).unwrap();
        let normalized = doc.json["accessors"][a]["normalized"].as_bool() == Some(true);
        match l.component {
            5126 => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| f32::from_le_bytes(*c))
                .collect(),
            5122 if normalized => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| f32::from(i16::from_le_bytes(*c)) / 32767.0)
                .collect(),
            5123 if normalized => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| f32::from(u16::from_le_bytes(*c)) / 65535.0)
                .collect(),
            other => panic!("component type {other} with {comps} components"),
        }
    };
    let (p, n, t) = (read("POSITION", 3), read("NORMAL", 3), read("TEXCOORD_0", 2));
    let indices = doc
        .indices(prim["indices"].as_u64().unwrap() as usize)
        .unwrap()
        .unwrap();
    let corner = |i: u32| {
        let i = i as usize;
        [
            p[i * 3],
            p[i * 3 + 1],
            p[i * 3 + 2],
            n[i * 3],
            n[i * 3 + 1],
            n[i * 3 + 2],
            t[i * 2],
            t[i * 2 + 1],
        ]
    };
    let mut tris: Vec<[[f32; 8]; 3]> = indices
        .chunks(3)
        .map(|c| {
            let mut tri = [corner(c[0]), corner(c[1]), corner(c[2])];
            let first = (0..3)
                .min_by(|&a, &b| tri[a][..3].partial_cmp(&tri[b][..3]).unwrap())
                .unwrap();
            tri.rotate_left(first);
            tri
        })
        .collect();
    tris.sort_by(|a, b| {
        a[0][..3]
            .partial_cmp(&b[0][..3])
            .unwrap()
            .then(a[1][..3].partial_cmp(&b[1][..3]).unwrap())
    });
    tris
}

const ALL: Options = Options {
    optimize: true,
    quantize: true,
};

fn cooked(json: &Value, bin: &[u8], options: Options) -> (Vec<u8>, String) {
    cook("grid.glb", &glb(json, bin), &[], options).unwrap().unwrap()
}

#[test]
fn a_cooked_model_draws_the_same_triangles() {
    let (json, bin) = grid(12);
    let source = glb(&json, &bin);
    let (out, _) = cooked(&json, &bin, ALL);
    let (before, after) = (surface(&source), surface(&out));
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(&after) {
        for (ca, cb) in a.iter().zip(b) {
            for k in 0..3 {
                assert_eq!(ca[k], cb[k], "positions are exact");
            }
            for k in 3..8 {
                assert!((ca[k] - cb[k]).abs() < 4e-5, "{} vs {}", ca[k], cb[k]);
            }
        }
    }
}

#[test]
fn cooking_improves_vertex_cache_use() {
    let (json, bin) = grid(16);
    let (_, note) = cooked(&json, &bin, ALL);
    // "vertex cache misses per triangle a -> b"
    let numbers: Vec<f64> = note
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter_map(|w| w.parse().ok())
        .collect();
    let (before, after) = (numbers[1], numbers[2]);
    assert!(after < before * 0.8, "{note}");
}

#[test]
fn nodes_animations_and_extras_are_written_back_as_they_were() {
    let (json, bin) = grid(4);
    let (out, _) = cooked(&json, &bin, ALL);
    let (cooked, _) = split_glb(&out).unwrap();
    for key in ["nodes", "scenes", "scene", "animations", "asset"] {
        assert_eq!(cooked[key], json[key], "{key}");
    }
    for a in [4, 5] {
        assert_eq!(cooked["accessors"][a]["count"], json["accessors"][a]["count"]);
        assert_eq!(cooked["accessors"][a]["componentType"], 5126);
    }
    assert_eq!(cooked["extensionsRequired"], json!(["KHR_mesh_quantization"]));
    assert_eq!(cooked["extensionsUsed"], json!(["KHR_mesh_quantization"]));
    assert_eq!(cooked["buffers"].as_array().unwrap().len(), 1);
    for (key, value) in cooked.as_object().unwrap() {
        assert!(!value.is_null(), "{key} is null");
    }
    assert_eq!(
        cooked
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !k.starts_with("extensions"))
            .count(),
        json.as_object().unwrap().len(),
        "no keys added but the extensions"
    );
    assert!(cooked["buffers"][0].get("uri").is_none());
}

#[test]
fn cooking_is_deterministic() {
    let (json, bin) = grid(10);
    assert_eq!(cooked(&json, &bin, ALL).0, cooked(&json, &bin, ALL).0);
}

#[test]
fn quantized_attributes_are_smaller_and_aligned() {
    let (json, bin) = grid(10);
    let source = glb(&json, &bin);
    let (out, note) = cooked(&json, &bin, ALL);
    assert!(out.len() < source.len(), "{} vs {}", out.len(), source.len());
    assert!(note.contains("2 attributes quantized"), "{note}");
    let (cooked, _) = split_glb(&out).unwrap();
    for view in cooked["bufferViews"].as_array().unwrap() {
        assert_eq!(view["byteOffset"].as_u64().unwrap() % 4, 0);
        if let Some(stride) = view["byteStride"].as_u64() {
            assert_eq!(stride % 4, 0);
        }
    }
    let (plain, note) = cooked_plain(&json, &bin);
    assert!(!note.contains("quantized"), "{note}");
    assert!(split_glb(&plain).unwrap().0.get("extensionsUsed").is_none());
}

fn cooked_plain(json: &Value, bin: &[u8]) -> (Vec<u8>, String) {
    cooked(
        json,
        bin,
        Options {
            optimize: true,
            quantize: false,
        },
    )
}

#[test]
fn a_tiled_uv_stays_a_float() {
    let (mut json, mut bin) = grid(2);
    // Scale the uvs past 1.
    let view = &json["bufferViews"][2];
    let (start, len) = (
        view["byteOffset"].as_u64().unwrap() as usize,
        view["byteLength"].as_u64().unwrap() as usize,
    );
    for c in bin[start..start + len].as_chunks_mut::<4>().0 {
        *c = (f32::from_le_bytes(*c) * 3.0).to_le_bytes();
    }
    json["extras"] = json!("kept");
    let (out, note) = cooked(&json, &bin, ALL);
    assert!(note.contains("1 attributes quantized"), "{note}");
    let (cooked, _) = split_glb(&out).unwrap();
    assert_eq!(cooked["accessors"][2]["componentType"], 5126);
    assert_eq!(cooked["extras"], "kept");
}

#[test]
fn shared_vertices_keep_their_order() {
    let (mut json, bin) = grid(3);
    // A second primitive drawing with the same vertices: their order must
    // stay, for both.
    let first = json["meshes"][0]["primitives"][0].clone();
    json["meshes"][0]["primitives"].as_array_mut().unwrap().push(first);
    let (out, _) = cooked(&json, &bin, ALL);
    let (cooked, blob) = split_glb(&out).unwrap();
    let doc = Doc {
        json: cooked,
        buffers: vec![blob.unwrap()],
    };
    let l = doc.layout(0).unwrap().unwrap();
    let original = Doc {
        json: json.clone(),
        buffers: vec![bin.clone()],
    };
    let ol = original.layout(0).unwrap().unwrap();
    assert_eq!(doc.elements(&l).unwrap(), original.elements(&ol).unwrap());
    // Each primitive got its own index list.
    let a = &doc.json["meshes"][0]["primitives"];
    assert_ne!(a[0]["indices"], a[1]["indices"]);
}

#[test]
fn unknown_extensions_and_compressed_geometry_are_kept() {
    let (mut json, bin) = grid(2);
    json["extensionsUsed"] = json!(["EXT_meshopt_compression"]);
    let kept = cook("a.glb", &glb(&json, &bin), &[], ALL).unwrap().unwrap_err();
    assert!(kept.contains("EXT_meshopt_compression"), "{kept}");
}

#[test]
fn a_gltf_with_an_external_buffer_becomes_one_glb() {
    let (mut json, bin) = grid(3);
    json["buffers"][0]["uri"] = json!("grid%20data.bin");
    let text = serde_json::to_vec(&json).unwrap();
    let mut all = BTreeMap::new();
    all.insert("models/grid.gltf".to_string(), text.clone());
    all.insert("models/grid data.bin".to_string(), bin.clone());
    let deps = dependencies("models/grid.gltf", &text, &all).unwrap();
    assert_eq!(deps.len(), 1);
    let (out, _) = cook("models/grid.gltf", &text, &deps, ALL).unwrap().unwrap();
    assert!(out.starts_with(b"glTF"));
    assert_eq!(
        surface(&out).len(),
        surface(&glb(
            &{
                let mut j = json.clone();
                j["buffers"][0].as_object_mut().unwrap().remove("uri");
                j
            },
            &bin
        ))
        .len()
    );
    let missing = cook("models/grid.gltf", &text, &[], ALL).unwrap_err();
    assert!(missing.contains("not among the assets"), "{missing}");
}

#[test]
fn indices_past_the_vertices_are_an_error() {
    let (mut json, bin) = grid(2);
    json["accessors"][0]["count"] = json!(3);
    json["accessors"][1]["count"] = json!(3);
    json["accessors"][2]["count"] = json!(3);
    let err = cook("bad.glb", &glb(&json, &bin), &[], ALL).unwrap_err();
    assert!(
        err.starts_with("bad.glb: ") && err.contains("past its vertices"),
        "{err}"
    );
}
