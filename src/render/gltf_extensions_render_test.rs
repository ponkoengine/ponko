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
fn lamp(strength: f32, [r, g, b]: [f32; 3]) -> Vec<u8> {
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
        "materials": [{{ "name": "glow", "emissiveFactor": [{r}, {g}, {b}],
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
    loaded_color(strength, [0.2; 3])
}

fn loaded_color(strength: f32, color: [f32; 3]) -> (Vec<ModelLight>, MaterialDesc) {
    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::ONE);
    ctx.assets
        .add_source(MemorySource::new("mem").with("lamp.gltf", lamp(strength, color)));
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

#[test]
fn exposed_lux_and_candela_agree_at_half_a_metre() {
    let Some(mut stage) = dark_stage() else { return };
    stage.ctx.scene3d.lighting.ambient_intensity = 0.0;
    let at = Transform3D::default();
    let mut light = ModelLight {
        name: String::new(),
        transform: Transform3D::at(vec3(0.0, 0.5, 0.0)),
        kind: ModelLightKind::Sun {
            color: Color::WHITE,
            intensity: 4.0,
        },
        joint: None,
    };
    light.transform.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let pixel = |stage: &mut Stage| {
        let image = stage.image();
        luma(stage.color_at(&image, Vec3::ZERO))
    };
    let unlit = pixel(&mut stage);
    let ev100 = 2.0;
    light.place_exposed(&mut stage.ctx.world, &mut stage.ctx.scene3d.lighting, &at, ev100);
    assert!((stage.ctx.scene3d.lighting.sun_intensity - 1.0 / 1.2).abs() < 1e-6);
    let sun = pixel(&mut stage);
    stage.ctx.scene3d.lighting.sun_intensity = 0.0;
    for kind in [
        ModelLightKind::Point(PointLight {
            intensity: 1.0 / ModelLight::CANDELA_PER_UNIT,
            range: 8.0,
            ..Default::default()
        }),
        ModelLightKind::Spot(crate::d3::SpotLight {
            intensity: 1.0 / ModelLight::CANDELA_PER_UNIT,
            range: 8.0,
            ..Default::default()
        }),
    ] {
        light.kind = kind;
        let entity = light
            .place_exposed(&mut stage.ctx.world, &mut stage.ctx.scene3d.lighting, &at, ev100)
            .unwrap();
        let local = pixel(&mut stage);
        assert!((local - sun).abs() < 6.0, "{kind:?}: local {local}, sun {sun}");
        stage.ctx.world.despawn(entity);
    }
    assert!(sun > unlit + 35.0, "sun {sun}, unlit {unlit}");
    stage.finish();
}

#[test]
fn the_highlight_shoulder_retains_glowing_color() {
    let Some(mut stage) = dark_stage() else { return };
    let (_, glow) = loaded_color(16.0, [0.03, 0.1, 0.2]);
    let mesh = stage.mesh(&MeshData::cube(1.0), glow);
    stage.block(mesh, vec3(-0.5, 0.0, -0.5), vec3(0.5, 1.0, 0.5));
    let read = |stage: &mut Stage| {
        let image = stage.image();
        stage.color_at(&image, vec3(0.0, 0.5, 0.5))
    };
    let clipped = read(&mut stage);
    stage.ctx.scene3d.post.tonemap = crate::render::Tonemap::HuePreserving;
    let [r, g, b] = read(&mut stage);
    assert!(clipped[2] - clipped[1] < 5.0, "clipped: {clipped:?}");
    assert!(b > 215.0 && b - g > 35.0 && g - r > 30.0, "mapped: {r} {g} {b}");
    stage.finish();
}

#[test]
fn hdr_environment_lights_diffuse_and_metal_and_can_be_removed() {
    use super::render_test_support::check_reference;
    use crate::render::EnvironmentMap;

    let Some(mut stage) = dark_stage() else { return };
    stage.ctx.scene3d.lighting.ambient_intensity = 1.0;
    stage.ctx.scene3d.sky.sky_color = Color::BLACK;
    stage.ctx.scene3d.sky.ground_color = Color::BLACK;
    stage.ctx.scene3d.post.tonemap = crate::render::Tonemap::HuePreserving;
    let cube = MeshData::cube(1.0);
    for (x, metallic) in [(-1.0, 0.0), (1.0, 1.0)] {
        let mesh = stage.mesh(
            &cube,
            MaterialDesc {
                metallic,
                roughness: 0.48,
                ..Default::default()
            },
        );
        stage.block(mesh, vec3(x - 0.5, 0.0, -0.5), vec3(x + 0.5, 1.0, 0.5));
    }
    let mut bytes = Vec::new();
    let pixels: Vec<_> = (0..32)
        .map(|i| {
            let blue = if i % 8 < 4 { 0.4 } else { 2.5 };
            image::Rgb([0.06, 0.24, blue])
        })
        .collect();
    image::codecs::hdr::HdrEncoder::new(&mut bytes)
        .encode(&pixels, 8, 4)
        .unwrap();
    stage
        .ctx
        .assets
        .add_source(MemorySource::new("hdr").with("blue.hdr", bytes));
    let map: EnvironmentMap = stage.ctx.gfx.try_load_environment("blue.hdr").unwrap();
    let before = stage.image();
    stage.ctx.scene3d.environment = Some(map);
    let after = stage.image();
    for x in [-1.0, 1.0] {
        let [r, _, b] = stage.color_at(&after, vec3(x, 0.5, 0.5));
        assert!(b > r + 55.0 && b > 100.0, "at {x}: r {r}, b {b}");
    }
    check_reference(
        "gltf_hdr_environment",
        &after,
        Tolerance {
            channel: 12,
            outliers: 0.0075,
            edge: 1,
        },
        "Matte and rough metal cubes lit only by a blue HDR panorama, with a black sky and no sun.",
        &stage.info,
    );
    stage.ctx.scene3d.environment = None;
    let restored = stage.image();
    assert_similar(
        &before,
        &restored,
        Tolerance {
            channel: 1,
            outliers: 0.0,
            edge: 0,
        },
        "environment removed",
    );
    stage.finish();
}

#[test]
fn gltf_preview_keeps_a_supplied_cube_and_falls_back_to_studio() {
    use crate::render::{EnvironmentMap, Tonemap};
    use std::sync::Arc;

    let mut ctx = Ctx::new(1.0 / 60.0, Vec2::ONE);
    let mut face = Vec::new();
    image::codecs::hdr::HdrEncoder::new(&mut face)
        .encode(&[image::Rgb([0.5, 1.0, 2.0]); 4], 2, 2)
        .unwrap();
    ctx.assets.add_source(
        MemorySource::new("preview")
            .with("lamp.gltf", lamp(1.0, [0.2; 3]))
            .with("face.hdr", face),
    );
    assert_eq!(
        ctx.gfx.try_load_environment("face.hdr").unwrap_err().kind(),
        ErrorKind::Invalid
    );
    let cube = ctx.gfx.try_load_environment_cube(["face.hdr"; 6]).unwrap();
    ctx.scene3d.environment = Some(cube.clone());
    let at = Transform3D::default();
    let loaded = ctx.gfx.try_load_lights("lamp.gltf").unwrap();
    let lights = ctx.scene3d.light_gltf(&mut ctx.world, &loaded, &at, 8.0);
    assert_eq!(lights.len(), 1);
    assert!(ctx.world.get::<PointLight>(lights[0]).is_some());
    assert!(Arc::ptr_eq(&ctx.scene3d.environment.as_ref().unwrap().0, &cube.0));
    assert_eq!(ctx.scene3d.lighting.sun_intensity, 0.0);
    assert_eq!(ctx.scene3d.post.tonemap, Tonemap::HuePreserving);
    ctx.world.despawn(lights[0]);
    ctx.scene3d.environment = None;
    ctx.scene3d.light_gltf(&mut ctx.world, &loaded, &at, 8.0);
    assert!(Arc::ptr_eq(
        &ctx.scene3d.environment.as_ref().unwrap().0,
        &EnvironmentMap::studio().0
    ));
}
