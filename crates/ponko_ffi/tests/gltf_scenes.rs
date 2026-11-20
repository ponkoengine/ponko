//! glTF files as scenes through the C API: the guide's courtyard spawned,
//! its nodes found by path, their custom properties and spawn points read,
//! and the scene despawned.
mod common;
use common::{last_error, run_game};
use ponko_ffi::*;
use std::ffi::{CString, c_char};
use std::ptr::null_mut;

use PonkoStatus::*;

fn courtyard() -> CString {
    CString::new(format!(
        "{}/../../assets/levels/courtyard.gltf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A text the C API writes as `(buffer, capacity, out_len)`.
unsafe fn read_text(f: impl Fn(*mut c_char, usize, *mut usize) -> PonkoStatus) -> Result<String, PonkoStatus> {
    let mut len = 0;
    let status = f(null_mut(), 0, &mut len);
    if status != PONKO_OK {
        return Err(status);
    }
    let mut buf = vec![0u8; len + 1];
    assert_eq!(f(buf.as_mut_ptr().cast(), buf.len(), &mut len), PONKO_OK);
    buf.truncate(len);
    Ok(String::from_utf8(buf).unwrap())
}

#[test]
fn a_gltf_scene_spawns_with_nodes_found_by_path_and_despawns() {
    let path = courtyard();
    let mut seen = None;
    unsafe {
        let game = ponko_game_new(c"gltf".as_ptr(), 64, 64);
        run_game(game, 0, |ctx| {
            let at = ponko_transform3d_at(PonkoVec3 {
                x: 10.0,
                y: 0.0,
                z: 0.0,
            });
            let mut root = 0;
            assert_eq!(
                ponko_ctx_spawn_gltf(ctx, path.as_ptr(), at, false, &mut root),
                PONKO_OK,
                "{}",
                last_error()
            );
            let w = ponko_ctx_world(ctx);
            let mut door = 0;
            assert_eq!(
                ponko_gltf_node_entity(w, root, c"Gate/Door".as_ptr(), &mut door),
                PONKO_OK
            );
            let extras = read_text(|b, n, len| ponko_gltf_node_extras(w, door, b, n, len)).unwrap();
            let mut start = 0;
            assert_eq!(ponko_gltf_node_entity(w, root, c"Start".as_ptr(), &mut start), PONKO_OK);
            let name = read_text(|b, n, len| ponko_spawn_point_name(w, start, b, n, len)).unwrap();
            let not_a_point = read_text(|b, n, len| ponko_spawn_point_name(w, door, b, n, len)).unwrap_err();
            let mut none = 0;
            let missing = ponko_gltf_node_entity(w, root, c"Nowhere".as_ptr(), &mut none);
            assert_eq!(ponko_ctx_despawn_gltf(ctx, root), PONKO_OK);
            let again = ponko_ctx_despawn_gltf(ctx, root);
            let mut bad = 0;
            let nowhere = ponko_ctx_spawn_gltf(ctx, c"levels/nowhere.gltf".as_ptr(), at, true, &mut bad);
            seen = Some((extras, name, not_a_point, missing, again, nowhere));
        });
    }
    let (extras, name, not_a_point, missing, again, nowhere) = seen.unwrap();
    assert_eq!(extras, r#"{"door":"north"}"#);
    assert_eq!(name, "player");
    assert_eq!(
        (not_a_point, missing, again),
        (PONKO_NOT_FOUND, PONKO_NOT_FOUND, PONKO_NOT_FOUND)
    );
    assert_eq!(nowhere, PONKO_NOT_FOUND);
}

#[test]
fn a_gltf_file_is_read_without_spawning_and_the_schema_lists_the_properties() {
    let path = courtyard();
    let mut seen = None;
    unsafe {
        let game = ponko_game_new(c"gltf".as_ptr(), 64, 64);
        run_game(game, 0, |ctx| {
            let len = || {
                let mut n = 0;
                assert_eq!(ponko_world_len(ponko_ctx_world(ctx), &mut n), PONKO_OK);
                n
            };
            let entities_before = len();
            let scene = read_text(|b, n, l| ponko_gltf_scene_load(ctx, path.as_ptr(), b, n, l)).unwrap();
            let spawned = len() - entities_before;
            let missing = read_text(|b, n, len| ponko_gltf_scene_load(ctx, c"levels/nowhere.gltf".as_ptr(), b, n, len))
                .unwrap_err();
            let schema = read_text(|b, n, len| ponko_gltf_properties_schema(ponko_ctx_world(ctx), b, n, len)).unwrap();
            seen = Some((scene, spawned, missing, schema));
        });
    }
    let (scene, spawned, missing, schema) = seen.unwrap();
    assert_eq!(spawned, 0, "nothing spawned");
    assert_eq!(missing, PONKO_NOT_FOUND);
    let scene: serde_json::Value = serde_json::from_str(&scene).unwrap();
    let door = scene["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "Gate/Door")
        .unwrap();
    assert_eq!(door["extras"]["door"], "north");
    assert_eq!(door["world"]["position"], serde_json::json!([0.0, 1.0, -6.0]));
    assert_eq!(door["mesh"]["triangles"], 12);
    assert_eq!(scene["spawn_points"][0]["name"], "player");
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();
    assert!(schema["properties"]["ponko:collider"]["enum"].is_array());
}
