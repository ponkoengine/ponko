//! glTF extensions drawn offscreen: a file's point light
//! (`KHR_lights_punctual`), spawned where its model stands, lights the
//! floor under it as the same light made by hand does, and nothing without
//! it (the negative control); a material's emissive strength
//! (`KHR_materials_emissive_strength`) makes it glow brighter than its
//! emissive color alone. Skips without a GPU (a skip is not a pass).
use super::regression_stage::*;
use super::render_test_support::Tolerance;
use crate::assets::MemorySource;
use crate::d3::{PointLight, Transform as Transform3D};
use crate::prelude::*;
use crate::render::{MaterialDesc, MeshData, ModelLight, ModelLightKind};

/// A file with a point light 2 above its origin and a material "glow" of
/// emissive 0.2 at a strength of `strength`.
fn lamp(strength: f32) -> Vec<u8> {
    // One triangle (its corners, base64): the file needs a mesh to load.
    let corners = "AAAAAAAAAAAAAAAAAACAPwAAAAAAAAAAAAAAAAAAgD8AAAAA";
    format!(
        r#"{{
        "asset": {{ "version": "2.0" }},
        "extensionsUsed": ["KHR_lights_punctual", "KHR_materials_emissive_strength"],
        "extensions": {{ "KHR_lights_punctual": {{ "lights": [
            {{ "type": "point", "color": [1.0, 0.8, 0.6], "intensity": {candela}, "range": 6 }}
        ] }} }},
        "scene": 0,
        "scenes": [{{ "nodes": [0, 1] }}],
        "nodes": [
            {{ "mesh": 0 }},
            {{ "translation": [0, 2, 0], "extensions": {{ "KHR_lights_punctual": {{ "light": 0 }} }} }}
        ],
        "materials": [{{ "name": "glow", "emissiveFactor": [0.2, 0.2, 0.2],
            "pbrMetallicRoughness": {{ "baseColorFactor": [0, 0, 0, 1] }},
            "extensions": {{ "KHR_materials_emissive_strength": {{ "emissiveStrength": {strength} }} }} }}],
        "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "material": 0 }}] }}],
        "buffers": [{{ "byteLength": 36, "uri": "data:application/octet-stream;base64,{corners}" }}],
        "bufferViews": [{{ "buffer": 0, "byteLength": 36 }}],
        "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                        "min": [0, 0, 0], "max": [1, 1, 0] }}]
    }}"#,
        candela = 4.0 * ModelLight::CANDELA_PER_UNIT,
    )
    .into_bytes()
}

/// The file's lights and its "glow" material, loaded by a headless
/// context of its own (the stage's renderer only knows what the stage
/// makes).
fn loaded(strength: f32) -> (Vec<ModelLight>, MaterialDesc) {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::ONE);
    ctx.assets
        .add_source(MemorySource::new("mem").with("lamp.gltf", lamp(strength)));
    let lights = ctx.gfx.load_lights("lamp.gltf");
    let glow = ctx.gfx.loaded_materials("glow")[0].1;
    (lights, glow)
}

/// A dark stage: no sun, a little ambient light, a floor.
fn dark_stage() -> Option<Stage> {
    let mut stage = Stage::new()?;
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    stage.ctx.scene3d.lighting.ambient_intensity = 0.02;
    stage.floor();
    Some(stage)
}

#[test]
fn a_files_point_light_lights_the_floor_where_its_model_stands() {
    let Some(mut stage) = dark_stage() else { return };
    let (lights, _) = loaded(1.0);
    let model = Transform3D::at(vec3(1.0, 0.0, 0.0));
    let unlit = stage.image();
    let light = lights[0].spawn(&mut stage.ctx.world, &model).unwrap();
    let lit = stage.image();
    let under = vec3(1.0, 0.0, 0.0);
    let far = vec3(-4.0, 0.0, -3.0);
    let (under_lit, under_unlit) = (luma(stage.color_at(&lit, under)), luma(stage.color_at(&unlit, under)));
    assert!(
        under_lit > under_unlit + 60.0,
        "under the light: {under_lit} lit, {under_unlit} without it"
    );
    // Beyond its range (6) nothing changes.
    let (far_lit, far_unlit) = (luma(stage.color_at(&lit, far)), luma(stage.color_at(&unlit, far)));
    assert!(
        (far_lit - far_unlit).abs() < 2.0,
        "beyond its range: {far_lit} lit, {far_unlit} without it"
    );

    // The same light made by hand, where the file puts it in the world:
    // the same picture.
    let ModelLightKind::Point(point) = lights[0].kind else {
        panic!("{:?}", lights[0].kind)
    };
    assert_eq!((point.intensity, point.range), (4.0, 6.0));
    stage.ctx.world.despawn(light);
    let by_hand = PointLight {
        color: point.color,
        intensity: 4.0,
        range: 6.0,
        ..Default::default()
    };
    stage.ctx.world.spawn((Transform3D::at(vec3(1.0, 2.0, 0.0)), by_hand));
    let by_hand = stage.image();
    let same = Tolerance {
        channel: 1,
        outliers: 0.0,
        edge: 0,
    };
    assert_similar(
        &lit,
        &by_hand,
        same,
        "a file's point light and the same light made by hand",
    );
    stage.finish();
}

#[test]
fn emissive_strength_makes_a_material_glow_brighter() {
    let Some(mut stage) = dark_stage() else { return };
    let (_, plain) = loaded(1.0);
    let (_, strong) = loaded(3.0);
    let cube = MeshData::cube(1.0);
    let plain = stage.mesh(&cube, plain);
    let strong = stage.mesh(&cube, strong);
    stage.block(plain, vec3(-2.0, 0.0, -0.5), vec3(-1.0, 1.0, 0.5));
    stage.block(strong, vec3(1.0, 0.0, -0.5), vec3(2.0, 1.0, 0.5));
    let image = stage.image();
    let glow = |x: f32| luma(stage.color_at(&image, vec3(x, 0.5, 0.5)));
    let (plain, strong) = (glow(-1.5), glow(1.5));
    // Linear 0.2 and 0.6 stored as sRGB bytes: about 124 and 203.
    assert!((plain - 124.0).abs() < 12.0, "emissive 0.2 shows as {plain}");
    assert!(
        (strong - 203.0).abs() < 12.0,
        "emissive 0.2 at strength 3 shows as {strong}"
    );
    stage.finish();
}
