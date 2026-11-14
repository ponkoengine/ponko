//! glTF extensions through the public API, headless: the lights and
//! cameras a file places, and clear errors for extensions the engine
//! doesn't read. The loader's own tests (`src/three/gltf_ext.rs`) cover
//! each extension's data in detail.
#![cfg(feature = "3d")]

use ponko::assets::MemorySource;
use ponko::d3::{PointLight, Transform};
use ponko::prelude::*;
use ponko::render::{ModelLightKind, ModelProjection};

/// One triangle, a point light on a node above it and a camera, as a
/// `.gltf` with its buffer inline. `extra` goes into the top-level object.
fn lamp_post(extra: &str) -> Vec<u8> {
    let corners: Vec<u8> = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .iter()
        .flat_map(|f| f.to_le_bytes())
        .collect();
    // Thirty-six bytes, base64 without padding.
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut b64 = String::new();
    for chunk in corners.chunks(3) {
        let n = u32::from(chunk[0]) << 16 | u32::from(chunk[1]) << 8 | u32::from(chunk[2]);
        for i in 0..4 {
            b64.push(alphabet[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        {extra}
        "extensionsUsed": ["KHR_lights_punctual"],
        "extensions": {{ "KHR_lights_punctual": {{ "lights": [
            {{ "type": "point", "name": "lamp", "intensity": 6830, "range": 8 }}
        ] }} }},
        "scene": 0,
        "scenes": [{{ "nodes": [0, 1, 2] }}],
        "nodes": [
            {{ "mesh": 0 }},
            {{ "translation": [0, 4, 0], "extensions": {{ "KHR_lights_punctual": {{ "light": 0 }} }} }},
            {{ "name": "overview", "camera": 0, "translation": [0, 2, 6] }}
        ],
        "cameras": [{{ "type": "perspective", "perspective": {{ "yfov": 0.9, "znear": 0.2 }} }}],
        "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}],
        "buffers": [{{ "byteLength": 36, "uri": "data:application/octet-stream;base64,{b64}" }}],
        "bufferViews": [{{ "buffer": 0, "byteLength": 36 }}],
        "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                        "min": [0, 0, 0], "max": [1, 1, 0] }}]
    }}"#
    )
    .into_bytes()
}

#[test]
fn a_files_lights_and_cameras_load_with_it_and_spawn_where_the_model_is() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .add_source(MemorySource::new("mem").with("lamp.gltf", lamp_post("")));
        let mesh = ctx.gfx.load_mesh("lamp.gltf");
        let lights = ctx.gfx.load_lights("lamp.gltf");
        assert_eq!(lights.len(), 1);
        let ModelLightKind::Point(point) = lights[0].kind else {
            panic!("{:?}", lights[0].kind)
        };
        assert!((point.intensity - 10.0).abs() < 1e-4 && point.range == 8.0);
        let at = Transform::at(vec3(5.0, 0.0, 0.0));
        ctx.world.spawn((at, ponko::render::Model::new(mesh)));
        let light = lights[0].spawn(&mut ctx.world, &at).unwrap();
        assert_eq!(ctx.world.get::<Transform>(light).unwrap().position, vec3(5.0, 4.0, 0.0));
        assert!(ctx.world.get::<PointLight>(light).is_some());

        let cameras = ctx.gfx.load_cameras("lamp.gltf");
        assert_eq!(cameras[0].name, "overview");
        assert!(matches!(
            cameras[0].projection,
            ModelProjection::Perspective { fov_y, far: None, .. } if fov_y == 0.9
        ));
        cameras[0].apply(&mut ctx.scene3d.camera, &at);
        assert_eq!(ctx.scene3d.camera.position, vec3(5.0, 2.0, 6.0));
        assert_eq!(ctx.scene3d.camera.near, 0.2);

        // Freeing the mesh forgets them with it; loading again brings them back.
        ctx.gfx.free(mesh);
        assert_eq!(ctx.gfx.load_lights("lamp.gltf").len(), 1);
    });
}

#[test]
fn a_file_needing_an_extension_the_engine_lacks_fails_naming_it() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let draco = r#""extensionsRequired": ["KHR_draco_mesh_compression"],"#;
        ctx.assets
            .add_source(MemorySource::new("mem").with("squeezed.gltf", lamp_post(draco)));
        let error = ctx.gfx.try_load_mesh("squeezed.gltf").unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Decode);
        assert!(error.message().contains("KHR_draco_mesh_compression"), "{error}");
        assert!(ctx.gfx.try_load_lights("squeezed.gltf").is_err());
    });
}
