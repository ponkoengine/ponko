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
            // Quantized positions: through the node that shows them.
            5123 => {
                let node = doc.json["nodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|n| n["mesh"] == 0)
                    .unwrap();
                assert!(node["extensions"].get(POSITIONS_EXTENSION).is_some(), "{node}");
                let f = |v: &Value| v.as_f64().unwrap() as f32;
                let (t, scale) = (&node["translation"], f(&node["scale"][0]));
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, c)| f32::from(u16::from_le_bytes(*c)) * scale + f(&t[i % 3]))
                    .collect()
            }
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
    images: Some(Images { basis: false }),
};

fn cooked(json: &Value, bin: &[u8], options: Options) -> (Vec<u8>, String) {
    let (out, note, _) = cook("grid.glb", &glb(json, bin), &[], options).unwrap().unwrap();
    (out, note)
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
                assert!((ca[k] - cb[k]).abs() <= POSITION_ERROR, "{} vs {}", ca[k], cb[k]);
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
    for key in ["scenes", "scene", "animations", "asset"] {
        assert_eq!(cooked[key], json[key], "{key}");
    }
    // The node keeps its name, extras and place; its mesh hangs from a
    // child that undoes the position quantization.
    let mut node = cooked["nodes"][0].clone();
    assert_eq!(node["children"], json!([1]));
    node.as_object_mut().unwrap().remove("children");
    let mut source = json["nodes"][0].clone();
    source.as_object_mut().unwrap().remove("mesh");
    assert_eq!(node, source);
    for a in [4, 5] {
        assert_eq!(cooked["accessors"][a]["count"], json["accessors"][a]["count"]);
        assert_eq!(cooked["accessors"][a]["componentType"], 5126);
    }
    assert_eq!(cooked["extensionsRequired"], json!(["KHR_mesh_quantization"]));
    assert_eq!(
        cooked["extensionsUsed"],
        json!(["KHR_mesh_quantization", POSITIONS_EXTENSION])
    );
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
            images: None,
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
    let (out, _, _) = cook("models/grid.gltf", &text, &deps, ALL).unwrap().unwrap();
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

/// A PNG of `size` x `size` whose pixels are `f(x, y)`.
#[cfg(feature = "textures")]
pub fn png(size: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(size, size, |x, y| image::Rgba(f(x, y)));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// [`grid`] with a textured material: base color, normal map, and separate
/// metallic-roughness and occlusion images (which the engine merges), all
/// PNGs of `size` x `size` in the binary chunk.
#[cfg(feature = "textures")]
pub fn textured(n: u32, size: u32) -> (Value, Vec<u8>) {
    let (mut json, mut bin) = grid(n);
    let images = [
        png(size, |x, y| [(x * 255 / size) as u8, (y * 255 / size) as u8, 160, 255]),
        png(size, |x, _| {
            let tilt = ((x as f32 / size as f32 * 6.0).sin() * 0.4 * 127.0) as i32;
            [(128 + tilt) as u8, 128, 240, 255]
        }),
        png(size, |x, y| {
            [0, (64 + y * 128 / size) as u8, (x * 255 / size) as u8, 255]
        }),
        png(size, |x, y| {
            let v = (128 + ((x + y) % 16) * 8) as u8;
            [v, v, v, 255]
        }),
    ];
    let views = json["bufferViews"].as_array_mut().unwrap();
    let mut list = Vec::new();
    for (i, bytes) in images.iter().enumerate() {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len()}));
        bin.extend_from_slice(bytes);
        list.push(json!({"bufferView": views.len() - 1, "mimeType": "image/png", "name": format!("image {i}")}));
    }
    json["buffers"][0]["byteLength"] = json!(bin.len());
    json["images"] = Value::Array(list);
    json["samplers"] = json!([{}]);
    json["textures"] = json!([
        {"source": 0, "sampler": 0}, {"source": 1, "sampler": 0},
        {"source": 2, "sampler": 0}, {"source": 3, "sampler": 0}
    ]);
    json["materials"] = json!([{
        "name": "panel",
        "pbrMetallicRoughness": {
            "baseColorTexture": {"index": 0},
            "metallicRoughnessTexture": {"index": 2}
        },
        "normalTexture": {"index": 1, "scale": 0.8},
        "occlusionTexture": {"index": 3, "strength": 0.9}
    }]);
    json["meshes"][0]["primitives"][0]["material"] = json!(0);
    json["nodes"][0]["name"] = json!("panel");
    json["nodes"][0]["extras"] = json!({"ponko:collider": "trimesh"});
    (json, bin)
}

/// The KTX2 image of texture `t` of a cooked file, and its `vkFormat`.
#[cfg(feature = "textures")]
fn ktx2_of(out: &[u8], t: usize) -> (Vec<u8>, u32) {
    let (json, bin) = split_glb(out).unwrap();
    let image = json["textures"][t]["extensions"]["KHR_texture_basisu"]["source"]
        .as_u64()
        .unwrap_or_else(|| panic!("texture {t} has a KTX2 image: {}", json["textures"]));
    let image = &json["images"][image as usize];
    assert_eq!(image["mimeType"], "image/ktx2");
    let view = &json["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
    let at = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let bytes = bin.unwrap()[at..at + view["byteLength"].as_u64().unwrap() as usize].to_vec();
    let format = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    (bytes, format)
}

#[cfg(feature = "textures")]
#[test]
fn a_models_own_images_become_ktx2_for_their_use() {
    let (json, bin) = textured(2, 32);
    let (out, note, gpu) = cook("panel.glb", &glb(&json, &bin), &[], ALL).unwrap().unwrap();
    assert!(note.contains("3 images compressed"), "{note}");
    let (cooked, _) = split_glb(&out).unwrap();
    assert_eq!(
        cooked["extensionsRequired"],
        json!(["KHR_mesh_quantization", "KHR_texture_basisu"])
    );
    // BC7 sRGB (146) for the color, BC5 (141) for the normal map, BC7
    // linear (145) for the merged ORM image.
    assert_eq!(ktx2_of(&out, 0).1, 146);
    assert_eq!(ktx2_of(&out, 1).1, 141);
    let material = &cooked["materials"][0];
    let orm = material["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"]
        .as_u64()
        .unwrap() as usize;
    assert_eq!(ktx2_of(&out, orm).1, 145);
    assert_eq!(
        material["occlusionTexture"]["index"],
        json!(orm),
        "both on the merged image"
    );
    assert_eq!(material["occlusionTexture"]["strength"], json!(0.9));
    assert_eq!(material["normalTexture"]["scale"], json!(0.8));
    // The occlusion image and its texture are gone; nothing else changed.
    assert_eq!(cooked["textures"].as_array().unwrap().len(), 3);
    assert_eq!(cooked["images"].as_array().unwrap().len(), 3);
    for texture in cooked["textures"].as_array().unwrap() {
        assert!(texture.get("source").is_none(), "{texture}");
        assert_eq!(texture["sampler"], 0);
    }
    // Video memory: three textures (the engine merges the ORM image), 32 x
    // 32 RGBA8 with mipmaps (5460 bytes) each before; BC7, BC5 and BC7 with
    // their mipmaps (16 bytes a 4 x 4 block) after.
    let (before, after) = gpu.unwrap();
    assert_eq!(before, 3 * 5460);
    assert_eq!(after, 3 * 16 * (64 + 16 + 4 + 1 + 1 + 1));
}

#[cfg(feature = "textures")]
#[test]
fn images_with_two_uses_or_of_their_own_stay_as_they_are() {
    // The base color is the normal map too.
    let (mut json, bin) = textured(2, 32);
    json["textures"][1]["source"] = json!(0);
    let (out, _, _) = cook("panel.glb", &glb(&json, &bin), &[], ALL).unwrap().unwrap();
    let (cooked, _) = split_glb(&out).unwrap();
    assert_eq!(cooked["textures"][0]["source"], 0, "kept: {}", cooked["textures"]);
    assert_eq!(cooked["images"][0]["mimeType"], "image/png");

    // An image file next to the model (other models may share it).
    let (mut json, bin) = textured(2, 32);
    json["images"][0] = json!({"uri": "shared.png"});
    let (out, _, _) = cook("panel.glb", &glb(&json, &bin), &[], ALL).unwrap().unwrap();
    let (cooked, _) = split_glb(&out).unwrap();
    assert_eq!(cooked["images"][0], json!({"uri": "shared.png"}));
    assert_eq!(cooked["textures"][0]["source"], 0);
    assert_eq!(ktx2_of(&out, 1).1, 141, "the others are compressed");

    // A side that isn't a multiple of 4: the GPU samples it as RGBA8.
    let (mut json, mut bin) = textured(2, 32);
    let odd = png(30, |_, _| [1, 2, 3, 255]);
    let at = bin.len().next_multiple_of(4);
    bin.resize(at, 0);
    bin.extend_from_slice(&odd);
    json["bufferViews"]
        .as_array_mut()
        .unwrap()
        .push(json!({"buffer": 0, "byteOffset": at, "byteLength": odd.len()}));
    json["images"][0]["bufferView"] = json!(json["bufferViews"].as_array().unwrap().len() - 1);
    json["buffers"][0]["byteLength"] = json!(bin.len());
    let (out, _, _) = cook("panel.glb", &glb(&json, &bin), &[], ALL).unwrap().unwrap();
    let (cooked, _) = split_glb(&out).unwrap();
    assert_eq!(cooked["textures"][0]["source"], 0);

    // Without image settings (a game that reads no KTX2) nothing changes.
    let (json, bin) = textured(2, 32);
    let options = Options { images: None, ..ALL };
    let (out, _, gpu) = cook("panel.glb", &glb(&json, &bin), &[], options).unwrap().unwrap();
    let (cooked, _) = split_glb(&out).unwrap();
    for image in cooked["images"].as_array().unwrap() {
        assert_eq!(image["mimeType"], "image/png");
    }
    assert!(gpu.is_none());
    assert!(
        cooked
            .get("extensionsRequired")
            .is_some_and(|e| !e.to_string().contains("basisu"))
    );
}

#[cfg(feature = "textures")]
#[test]
fn mobile_models_get_uastc_images() {
    let (json, bin) = textured(2, 32);
    let options = Options {
        images: Some(Images { basis: true }),
        ..ALL
    };
    let (out, _, _) = cook("panel.glb", &glb(&json, &bin), &[], options).unwrap().unwrap();
    assert_eq!(ktx2_of(&out, 0).1, 0, "Basis Universal: vkFormat 0");
}

#[test]
fn positions_are_quantized_within_the_bound_through_a_child_node() {
    let (json, bin) = grid(12);
    let (out, note) = cooked(&json, &bin, ALL);
    assert!(note.contains("positions of 1 meshes 16-bit"), "{note}");
    let (cooked, _) = split_glb(&out).unwrap();
    let position = &cooked["accessors"][0];
    assert_eq!(
        (position["componentType"].clone(), position.get("normalized")),
        (json!(5123), None)
    );
    assert_eq!(position["min"], json!([0, 0, 0]));
    // 4 m across: steps of 4 / 65535 m on every axis, from the least corner.
    let child = &cooked["nodes"][1];
    assert_eq!(child["mesh"], 0);
    let step = child["scale"][0].as_f64().unwrap();
    assert!((step - 4.0 / 65535.0).abs() < 1e-9, "{child}");
    assert_eq!(child["scale"][0], child["scale"][1]);
    assert_eq!(child["scale"][0], child["scale"][2]);
    assert_eq!(child["translation"][0], json!(0.0));
    assert!(
        (child["translation"][1].as_f64().unwrap() + 0.2).abs() < 0.01,
        "the least height"
    );
    // Every position within the bound (checked corner by corner in
    // `a_cooked_model_draws_the_same_triangles`); the view is 8 bytes a
    // position, 4-byte aligned.
    let view = &cooked["bufferViews"][position["bufferView"].as_u64().unwrap() as usize];
    assert_eq!(view["byteStride"], 8);
    let size = |file: &[u8]| file.len();
    let (floats, _) = cooked_plain(&json, &bin);
    assert!(size(&out) < size(&floats), "{} vs {}", size(&out), size(&floats));
}

#[test]
fn positions_past_the_bound_or_that_cant_move_stay_floats() {
    let is_float = |out: &[u8]| split_glb(out).unwrap().0["accessors"][0]["componentType"] == 5126;
    // 40 m across: steps of 0.6 mm, past the bound.
    let (json, bin) = grid(4);
    let mut big = bin.clone();
    for c in big[..25 * 12].as_chunks_mut::<4>().0 {
        *c = (f32::from_le_bytes(*c) * 10.0).to_le_bytes();
    }
    let mut json_big = json.clone();
    json_big["accessors"][0]["max"] = json!([40.0, 2.0, 40.0]);
    json_big["accessors"][0]["min"] = json!([0.0, -2.0, 0.0]);
    assert!(is_float(&cooked(&json_big, &big, ALL).0));
    // A skinned node's transform doesn't apply to its mesh.
    let mut skinned = json.clone();
    skinned["nodes"][0]["skin"] = json!(0);
    skinned["skins"] = json!([{"joints": [0]}]);
    assert!(is_float(&cooked(&skinned, &bin, ALL).0));
    // Nor does `quantize: false` quantize anything.
    let (out, _) = cooked(&json, &bin, Options { quantize: false, ..ALL });
    assert!(is_float(&out));
    assert!(split_glb(&out).unwrap().0["nodes"][0].get("children").is_none());
}

/// A level: one mesh of `tiles` x `tiles` primitives, each a hilly
/// `tile` m square of `quads` x `quads` quads with positions, normals and texture
/// coordinates, two materials in a checkerboard, shown by a node with
/// extras under a root node: `tiles * tile` meters across, a size one
/// 16-bit range can't hold within the bound past 13.1 m.
pub fn level(tiles: u32, tile: f32, quads: u32) -> (Value, Vec<u8>) {
    let n = quads;
    let side = n + 1;
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut primitives = Vec::new();
    let mut push = |bin: &mut Vec<u8>, bytes: Vec<u8>, target: u32| {
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        views.push(json!({"buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target}));
        bin.extend(bytes);
        views.len() - 1
    };
    let f32s = |v: &[f32]| v.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>();
    for ty in 0..tiles {
        for tx in 0..tiles {
            let (mut pos, mut nrm, mut uv) = (Vec::new(), Vec::new(), Vec::new());
            let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
            for y in 0..side {
                for x in 0..side {
                    let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
                    let (wx, wz) = ((tx as f32 + fx) * tile, (ty as f32 + fy) * tile);
                    let h = (wx * 0.37).sin() * 1.5 + (wz * 0.23).cos() * 0.75 + 0.123_456;
                    let p = [wx + 0.000_37 * fy, h, wz - 0.000_21 * fx];
                    for k in 0..3 {
                        min[k] = min[k].min(p[k]);
                        max[k] = max[k].max(p[k]);
                    }
                    pos.extend(p);
                    let normal = [-(wx * 0.37).cos() * 0.555, 1.0, (wz * 0.23).sin() * 0.1725f32];
                    let len = normal.iter().map(|c| c * c).sum::<f32>().sqrt();
                    nrm.extend(normal.map(|c| c / len));
                    uv.extend([fx, fy]);
                }
            }
            let mut indices = Vec::new();
            for y in 0..n {
                for x in 0..n {
                    let i = y * side + x;
                    indices.extend([i, i + side, i + 1, i + 1, i + side, i + side + 1]);
                }
            }
            let count = side * side;
            let first = accessors.len();
            let views = [
                push(&mut bin, f32s(&pos), 34962),
                push(&mut bin, f32s(&nrm), 34962),
                push(&mut bin, f32s(&uv), 34962),
                push(
                    &mut bin,
                    indices.iter().flat_map(|&i| (i as u16).to_le_bytes()).collect(),
                    34963,
                ),
            ];
            accessors.push(json!({"bufferView": views[0], "componentType": 5126, "count": count,
                                  "type": "VEC3", "min": min, "max": max}));
            accessors.push(json!({"bufferView": views[1], "componentType": 5126, "count": count, "type": "VEC3"}));
            accessors.push(json!({"bufferView": views[2], "componentType": 5126, "count": count, "type": "VEC2"}));
            accessors
                .push(json!({"bufferView": views[3], "componentType": 5123, "count": indices.len(), "type": "SCALAR"}));
            primitives.push(json!({
                "attributes": {"POSITION": first, "NORMAL": first + 1, "TEXCOORD_0": first + 2},
                "indices": first + 3,
                "material": (tx + ty) % 2,
            }));
        }
    }
    let json = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "world", "children": [1], "translation": [-5.0, 0.0, 2.0]},
            {"mesh": 0, "name": "level", "extras": {"collider": "mesh", "area": "valley"},
             "translation": [1.0, 0.5, -1.0], "scale": [1.0, 1.0, 1.0]}
        ],
        "materials": [{"name": "grass"}, {"name": "rock"}],
        "meshes": [{"name": "level", "primitives": primitives, "extras": {"biome": "temperate"}}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    (json, bin)
}

/// The positions of primitive `p` of mesh 0 as the file would draw them
/// in the space of the node that showed mesh 0 in the source: through the
/// quantized child that holds the primitive now, if any.
fn level_positions(file: &[u8], p: usize) -> Vec<[f32; 3]> {
    let (json, bin) = split_glb(file).unwrap();
    let doc = Doc {
        json,
        buffers: vec![bin.unwrap()],
    };
    // The node showing the primitive, its mesh and its index there.
    let (mut mesh, mut at, mut transform) = (0, p, None);
    for node in doc.json["nodes"].as_array().unwrap() {
        let Some(ext) = node["extensions"].get(POSITIONS_EXTENSION) else {
            continue;
        };
        let list: Vec<u64> = match ext["primitives"].as_array() {
            Some(list) => list.iter().map(|v| v.as_u64().unwrap()).collect(),
            None => (0..doc.json["meshes"][0]["primitives"].as_array().unwrap().len() as u64).collect(),
        };
        if let Some(k) = list.iter().position(|&q| q == p as u64) {
            mesh = node["mesh"].as_u64().unwrap() as usize;
            at = k;
            transform = node.get("scale").map(|s| {
                let f = |v: &Value| v.as_f64().unwrap() as f32;
                (f(&s[0]), [0, 1, 2].map(|k| f(&node["translation"][k])))
            });
        }
    }
    let a = doc.json["meshes"][mesh]["primitives"][at]["attributes"]["POSITION"]
        .as_u64()
        .unwrap() as usize;
    let l = doc.layout(a).unwrap().unwrap();
    let bytes = doc.elements(&l).unwrap();
    match (l.component, transform) {
        (5126, None) => bytes
            .as_chunks::<12>()
            .0
            .iter()
            .map(|c| [0, 1, 2].map(|k| f32::from_le_bytes(c[k * 4..k * 4 + 4].try_into().unwrap())))
            .collect(),
        (5123, Some((step, corner))) => bytes
            .as_chunks::<6>()
            .0
            .iter()
            .map(|c| [0, 1, 2].map(|k| f32::from(u16::from_le_bytes([c[k * 2], c[k * 2 + 1]])) * step + corner[k]))
            .collect(),
        other => panic!("positions of type {} with transform {:?}", other.0, other.1),
    }
}

const QUANTIZE_ONLY: Options = Options {
    optimize: false,
    quantize: true,
    images: None,
};

#[test]
fn a_level_too_big_for_one_range_gets_a_range_per_primitive() {
    // 48 m across: one range would step 0.73 mm, past the bound.
    let (json, bin) = level(4, 12.0, 4);
    let source = glb(&json, &bin);
    let (out, note) = cooked(&json, &bin, QUANTIZE_ONLY);
    assert!(
        note.contains("positions of 1 meshes 16-bit") && note.contains("a range per primitive (16 primitives)"),
        "{note}"
    );
    let (cooked_json, _) = split_glb(&out).unwrap();
    for p in 0..16 {
        let position = &cooked_json["accessors"][p * 4];
        assert_eq!(position["componentType"], 5123, "primitive {p}");
        let (before, after) = (level_positions(&source, p), level_positions(&out, p));
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() <= POSITION_ERROR, "primitive {p}: {a:?} vs {b:?}");
            }
        }
    }
    // The level node shows nothing itself now: 16 marked children, each a
    // mesh of one primitive that names where it came from; the first keeps
    // the mesh's index, name and extras.
    let node = &cooked_json["nodes"][1];
    assert!(node.get("mesh").is_none());
    assert_eq!(node["extras"], json["nodes"][1]["extras"], "the node's extras stay");
    assert_eq!(node["translation"], json["nodes"][1]["translation"]);
    let children = node["children"].as_array().unwrap();
    assert_eq!(children.len(), 16);
    for (k, c) in children.iter().enumerate() {
        let child = &cooked_json["nodes"][c.as_u64().unwrap() as usize];
        assert_eq!(
            child["extensions"][POSITIONS_EXTENSION],
            json!({"mesh": 0, "primitives": [k]})
        );
        let mesh = &cooked_json["meshes"][child["mesh"].as_u64().unwrap() as usize];
        assert_eq!(mesh["primitives"].as_array().unwrap().len(), 1);
        assert_eq!(mesh["name"], "level");
        assert_eq!(mesh["primitives"][0]["material"], json!((k % 4 + k / 4) % 2));
        let step = child["scale"][0].as_f64().unwrap();
        assert!(step <= 12.1 / 65535.0, "each range is a tile's: {step}");
    }
    assert_eq!(cooked_json["meshes"][0]["extras"], json["meshes"][0]["extras"]);
    // Smaller than the floats.
    let (floats, _) = cooked(
        &json,
        &bin,
        Options {
            quantize: false,
            ..QUANTIZE_ONLY
        },
    );
    assert!(out.len() < floats.len(), "{} vs {}", out.len(), floats.len());
    // The same level with 32 x 32 quads a tile (17,424 corners), against
    // what one range for the mesh gave it: float positions (its normals
    // and texture coordinates quantized either way; an instanced node keeps
    // float positions).
    let (json, bin) = level(4, 12.0, 32);
    let (out, _) = cooked(&json, &bin, QUANTIZE_ONLY);
    let mut one_range = json.clone();
    one_range["extensionsUsed"] = json!(["EXT_mesh_gpu_instancing"]);
    one_range["nodes"][1]["extensions"] = json!({"EXT_mesh_gpu_instancing": {"attributes": {}}});
    let (floats, note) = cooked(&one_range, &bin, QUANTIZE_ONLY);
    assert!(!note.contains("positions of"), "{note}");
    assert!(
        out.len() + 17_424 * 4 - 4096 < floats.len(),
        "{} vs {}",
        out.len(),
        floats.len()
    );
    eprintln!(
        "48 m level, 16 primitives of 1089 corners: {} bytes with float positions, {} with a range per primitive",
        floats.len(),
        out.len()
    );
}

#[test]
fn a_primitive_too_big_for_the_bound_stays_float_beside_quantized_ones() {
    // Tile 0 stretched to 30 m: it keeps floats, under a child with no
    // transform; the other three get ranges of their own.
    let (mut json, mut bin) = level(2, 10.0, 4);
    let view = &json["bufferViews"][0];
    let (start, len) = (
        view["byteOffset"].as_u64().unwrap() as usize,
        view["byteLength"].as_u64().unwrap() as usize,
    );
    for c in bin[start..start + len].as_chunks_mut::<4>().0 {
        *c = (f32::from_le_bytes(*c) * 3.0).to_le_bytes();
    }
    for key in ["min", "max"] {
        let v: Vec<f64> = json["accessors"][0][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() * 3.0)
            .collect();
        json["accessors"][0][key] = json!(v);
    }
    let source = glb(&json, &bin);
    let (out, note) = cooked(&json, &bin, QUANTIZE_ONLY);
    assert!(note.contains("(3 primitives)"), "{note}");
    let (cooked_json, _) = split_glb(&out).unwrap();
    assert_eq!(cooked_json["accessors"][0]["componentType"], 5126);
    for p in 1..4 {
        assert_eq!(cooked_json["accessors"][p * 4]["componentType"], 5123);
    }
    let children = cooked_json["nodes"][1]["children"].as_array().unwrap();
    let float_child = &cooked_json["nodes"][children[0].as_u64().unwrap() as usize];
    assert!(float_child.get("scale").is_none() && float_child.get("translation").is_none());
    assert_eq!(float_child["extensions"][POSITIONS_EXTENSION]["primitives"], json!([0]));
    for p in 0..4 {
        let (before, after) = (level_positions(&source, p), level_positions(&out, p));
        for (a, b) in before.iter().zip(&after) {
            for k in 0..3 {
                assert!((a[k] - b[k]).abs() <= POSITION_ERROR, "primitive {p}: {a:?} vs {b:?}");
            }
        }
    }
}

#[test]
fn a_mesh_that_fits_one_range_keeps_one_child() {
    let (json, bin) = level(2, 5.0, 4);
    let (out, note) = cooked(&json, &bin, QUANTIZE_ONLY);
    assert!(!note.contains("per primitive"), "{note}");
    let (cooked_json, _) = split_glb(&out).unwrap();
    assert_eq!(cooked_json["meshes"].as_array().unwrap().len(), 1);
    let children = cooked_json["nodes"][1]["children"].as_array().unwrap();
    assert_eq!(children.len(), 1);
    let child = &cooked_json["nodes"][children[0].as_u64().unwrap() as usize];
    assert_eq!(child["extensions"][POSITIONS_EXTENSION], json!({}));
}

#[cfg(feature = "textures")]
#[test]
fn images_are_compressed_without_relocating_meshopt_or_draco_geometry() {
    for ext in [
        "EXT_meshopt_compression",
        "KHR_meshopt_compression",
        "KHR_draco_mesh_compression",
    ] {
        let (mut json, bin) = textured(2, 16);
        json["extensionsUsed"] = json!([ext]);
        json["extensionsRequired"] = json!([ext]);
        if ext == "KHR_draco_mesh_compression" {
            json["meshes"][0]["primitives"][0]["extensions"] =
                json!({(ext): {"bufferView": 0, "attributes": {"POSITION": 0}}});
        } else {
            json["bufferViews"][0]["extensions"] = json!({(ext): {"buffer": 0, "byteOffset": 0, "byteLength": 12}});
            // A declared fallback placeholder without a URI has no data.
            json["buffers"]
                .as_array_mut()
                .unwrap()
                .push(json!({"byteLength": 1024, "extensions": {(ext): {"fallback": true}}}));
        }
        let options = Options {
            images: Some(Images { basis: false }),
            ..ALL
        };
        let (out, note, gpu) = cook("packed.glb", &glb(&json, &bin), &[], options).unwrap().unwrap();
        let (cooked, blob) = split_glb(&out).unwrap();
        assert_eq!(cooked["accessors"], json["accessors"]);
        assert_eq!(cooked["meshes"], json["meshes"]);
        let views = json["bufferViews"].as_array().unwrap();
        assert_eq!(&cooked["bufferViews"].as_array().unwrap()[..views.len()], views);
        assert_eq!(&blob.unwrap()[..24], &bin[..24]);
        assert!(cooked["extensionsRequired"].as_array().unwrap().contains(&json!(ext)));
        assert_eq!(ktx2_of(&out, 0).1, 146);
        assert!(note.contains("3 images compressed"));
        assert!(gpu.is_some());
    }
}
