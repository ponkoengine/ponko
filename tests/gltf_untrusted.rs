//! glTF files the game doesn't control (mods, downloads): a malformed file
//! is an error, never a crash. Cases found by the `gltf` fuzz target
//! (`fuzz/`).
#![cfg(feature = "3d")]

use ponko::assets::MemorySource;
use ponko::prelude::*;

/// A triangle on node 0 with a two-key animation channel whose target is
/// `target` (JSON), its data in `anim.bin`.
fn animated(target: &str) -> MemorySource {
    let floats: [f32; 17] = [
        // 0: the triangle's corners
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // 36: key times
        0.0, 1.0, // 44: translations
        0.0, 0.0, 0.0, 0.0, 2.0, 0.0,
    ];
    let bin: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    let gltf = format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "scene": 0,
        "scenes": [{{ "nodes": [0] }}],
        "nodes": [{{ "name": "tri", "mesh": 0 }}],
        "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}],
        "animations": [{{
            "name": "move",
            "samplers": [{{ "input": 1, "output": 2 }}],
            "channels": [{{ "sampler": 0, "target": {target} }}]
        }}],
        "buffers": [{{ "byteLength": 68, "uri": "anim.bin" }}],
        "bufferViews": [
            {{ "buffer": 0, "byteLength": 36 }},
            {{ "buffer": 0, "byteOffset": 36, "byteLength": 8 }},
            {{ "buffer": 0, "byteOffset": 44, "byteLength": 24 }}
        ],
        "accessors": [
            {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 0] }},
            {{ "bufferView": 1, "componentType": 5126, "count": 2, "type": "SCALAR", "min": [0], "max": [1] }},
            {{ "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" }}
        ]
    }}"#
    );
    MemorySource::new("untrusted")
        .with("anim.gltf", gltf)
        .with("anim.bin", bin)
}

#[test]
fn animation_channels_with_an_unknown_path_are_skipped() {
    Game::new("gltf", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(animated(r#"{ "node": 0, "path": "pointer" }"#));
        let clips = ctx.gfx.try_load_clips("anim.gltf").expect("the file loads");
        assert_eq!(clips.len(), 1);
        assert!(clips[0].tracks().is_empty(), "the unknown channel is left out");
    });
}

#[test]
fn an_animation_channel_targeting_a_missing_node_is_a_decode_error() {
    Game::new("gltf", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .add_source(animated(r#"{ "node": 7, "path": "translation" }"#));
        let err = ctx.gfx.try_load_mesh_data("anim.gltf").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(err.message().contains("targets node 7"), "{err}");
    });
}

#[test]
fn a_valid_animation_channel_still_loads() {
    Game::new("gltf", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .add_source(animated(r#"{ "node": 0, "path": "translation" }"#));
        let clips = ctx.gfx.try_load_clips("anim.gltf").expect("the file loads");
        assert_eq!(clips[0].tracks().len(), 1);
    });
}

#[test]
fn a_binary_gltf_whose_length_is_shorter_than_its_header_is_a_decode_error() {
    let mut glb = b"glTF".to_vec();
    glb.extend(2u32.to_le_bytes());
    glb.extend(8u32.to_le_bytes()); // under the 12-byte header
    glb.extend(b"JSON{}");
    Game::new("gltf", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("untrusted").with("short.glb", glb));
        let err = ctx.gfx.try_load_mesh_data("short.glb").unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Decode);
        assert!(err.message().contains("length under 12 bytes"), "{err}");
    });
}

/// The error loading `animated` (with a valid channel) gives once `edit`
/// changed its glTF text.
fn error_after(edit: impl Fn(&str) -> String) -> ponko::error::Error {
    let valid = animated(r#"{ "node": 0, "path": "translation" }"#);
    let text = String::from_utf8(valid.read("anim.gltf").unwrap()).unwrap();
    let files = valid.with("anim.gltf", edit(&text));
    let mut err = None;
    Game::new("gltf", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(files);
        err = ctx.gfx.try_load_mesh_data("anim.gltf").err();
    });
    let err = err.expect("the edited file is refused");
    assert_eq!(err.kind(), ErrorKind::Decode, "{err}");
    err
}

/// `text` with accessor 3 added: `accessor` (JSON).
fn with_accessor(text: &str, accessor: &str) -> String {
    text.replace(
        r#"{ "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" }"#,
        &format!(r#"{{ "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC3" }}, {accessor}"#),
    )
}

#[test]
fn accessors_of_a_type_gltf_does_not_allow_for_their_use_are_a_decode_error() {
    let err = error_after(|t| {
        with_accessor(
            t,
            r#"{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC2" }"#,
        )
        .replace(r#""POSITION": 0"#, r#""POSITION": 0, "NORMAL": 3"#)
    });
    assert!(
        err.message()
            .contains("accessor 3 (NORMAL) is Vec2 F32, which glTF doesn't allow"),
        "{err}"
    );
    // Accessor 1 holds floats: not indices.
    let err = error_after(|t| t.replace(r#""POSITION": 0 }"#, r#""POSITION": 0 }, "indices": 1"#));
    assert!(err.message().contains("accessor 1 (indices) is Scalar F32"), "{err}");
    // Colors of signed bytes: glTF allows unsigned bytes, shorts and floats.
    let err = error_after(|t| {
        with_accessor(
            t,
            r#"{ "bufferView": 0, "componentType": 5120, "count": 3, "type": "VEC3" }"#,
        )
        .replace(r#""POSITION": 0"#, r#""POSITION": 0, "COLOR_0": 3"#)
    });
    assert!(err.message().contains("accessor 3 (COLOR) is Vec3 I8"), "{err}");
}

#[test]
fn attributes_with_another_count_than_position_are_a_decode_error() {
    // Accessor 2: two VEC3 floats, for three corners.
    let err = error_after(|t| t.replace(r#""POSITION": 0"#, r#""POSITION": 0, "NORMAL": 2"#));
    assert!(
        err.message()
            .contains("accessor 2 (NORMAL) has 2 elements, but POSITION has 3"),
        "{err}"
    );
}

#[test]
fn positions_past_their_view_or_a_huge_count_of_zeros_are_a_decode_error() {
    // A billion corners claimed in a 36-byte view: refused before any room
    // is set aside for them.
    let err = error_after(|t| {
        t.replace(
            r#""componentType": 5126, "count": 3, "type": "VEC3""#,
            r#""componentType": 5126, "count": 1000000000, "type": "VEC3""#,
        )
    });
    assert!(
        err.message()
            .contains("accessor 0 (POSITION) runs past the end of its buffer view"),
        "{err}"
    );
    // Without a view (only a sparse change), every element but the changed
    // one is a zero: a billion of them is refused too.
    let err = error_after(|t| {
        t.replace(
            r#"{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3""#,
            r#"{ "componentType": 5126, "count": 1000000000, "type": "VEC3",
                "sparse": { "count": 1, "indices": { "bufferView": 1, "componentType": 5125 },
                            "values": { "bufferView": 0 } }"#,
        )
    });
    assert!(err.message().contains("has 1000000000 elements and no data"), "{err}");
}

#[test]
fn an_accessor_whose_stride_is_shorter_than_an_element_is_a_decode_error() {
    let err = error_after(|t| {
        t.replace(
            r#"{ "buffer": 0, "byteLength": 36 }"#,
            r#"{ "buffer": 0, "byteLength": 36, "byteStride": 4 }"#,
        )
    });
    assert!(err.message().contains("accessor 0 (POSITION)"), "{err}");
}

#[test]
fn a_position_naming_a_missing_accessor_is_a_decode_error() {
    let err = error_after(|t| t.replace(r#""POSITION": 0"#, r#""POSITION": 9"#));
    assert!(err.message().contains("POSITION names accessor 9"), "{err}");
}

#[test]
fn nodes_that_are_not_trees_are_a_decode_error() {
    // Node 1 is its own child: walking the scene never ended.
    let err = error_after(|t| {
        t.replace(
            r#""nodes": [{ "name": "tri", "mesh": 0 }]"#,
            r#""nodes": [{ "name": "tri", "mesh": 0, "children": [1] }, { "children": [1] }]"#,
        )
    });
    assert!(err.message().contains("is a child of both node 0 and node 1"), "{err}");
    let err = error_after(|t| {
        t.replace(
            r#""nodes": [{ "name": "tri", "mesh": 0 }]"#,
            r#""nodes": [{ "name": "tri", "mesh": 0 }, { "children": [1] }]"#,
        )
    });
    assert!(err.message().contains("node 1 is its own ancestor"), "{err}");
    // Two parents, or a root listed twice: walked once per path to it.
    let err = error_after(|t| t.replace(r#""nodes": [0] }"#, r#""nodes": [0, 0] }"#));
    assert!(err.message().contains("scene 0 lists node 0 twice"), "{err}");
    let err = error_after(|t| {
        t.replace(r#""nodes": [0] }"#, r#""nodes": [0, 1] }"#).replace(
            r#""nodes": [{ "name": "tri", "mesh": 0 }]"#,
            r#""nodes": [{ "name": "tri", "mesh": 0, "children": [1] }, { "mesh": 0 }]"#,
        )
    });
    assert!(
        err.message().contains("scene 0 lists node 1, a child of node 0"),
        "{err}"
    );
}
