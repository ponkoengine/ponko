//! glTF light units and HDR environments through the C API: a file's lights
//! placed with an EV100 exposure, one light placed the same way, and the
//! scene's environment set from a panorama, six faces or the studio.
mod common;
use common::{last_error, run_ctx};
use ponko_ffi::*;
use std::ffi::{CString, c_char};
use std::mem::size_of;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const COURTYARD_CANDELA: f32 = 2000.0;

fn courtyard() -> CString {
    CString::new(format!(
        "{}/../../assets/levels/courtyard.gltf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn origin() -> PonkoTransform3D {
    ponko_transform3d_at(PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 })
}

/// A Radiance HDR image of `width` x `height` pixels, all the same finite color.
fn hdr(width: usize, height: usize) -> Vec<u8> {
    let mut file = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n").into_bytes();
    for _ in 0..width * height {
        file.extend([128, 128, 128, 130]);
    }
    file
}

fn with_files(ctx: *mut PonkoCtx, files: &[(&str, Vec<u8>)]) {
    unsafe {
        let memory = ponko_memory_source_new(c"environments".as_ptr());
        for (name, bytes) in files {
            let name = CString::new(*name).unwrap();
            assert_eq!(
                ponko_memory_source_insert(memory, name.as_ptr(), bytes.as_ptr(), bytes.len()),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_add_memory_source(ctx, memory), PONKO_OK);
        ponko_memory_source_free(memory);
    }
}

fn world_len(ctx: *mut PonkoCtx) -> usize {
    let mut n = 0;
    assert_eq!(unsafe { ponko_world_len(ponko_ctx_world(ctx), &mut n) }, PONKO_OK);
    n
}

fn point_intensity(ctx: *mut PonkoCtx, entity: u64) -> PonkoPointLight3D {
    unsafe {
        let mut id = 0;
        assert_eq!(ponko_point_light3d_component(ponko_ctx_world(ctx), &mut id), PONKO_OK);
        let mut light = std::mem::zeroed::<PonkoPointLight3D>();
        assert_eq!(
            ponko_get(
                ponko_ctx_world(ctx),
                entity,
                id,
                (&mut light as *mut PonkoPointLight3D).cast(),
                size_of::<PonkoPointLight3D>()
            ),
            PONKO_OK
        );
        light
    }
}

#[test]
fn a_gltf_preview_places_the_files_lights_with_exposure_and_sets_the_look() {
    let path = courtyard();
    run_ctx(|ctx| unsafe {
        let mut count = 0;
        assert_eq!(
            ponko_ctx_gltf_local_light_count(ctx, path.as_ptr(), &mut count),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(count, 1, "the courtyard has one lantern");

        // A preview starts from the scene's defaults.
        let mut has = true;
        assert_eq!(ponko_ctx_has_environment(ctx, &mut has), PONKO_OK);
        assert!(!has);

        // A list too short for the lights changes nothing.
        let entities = world_len(ctx);
        let mut spawned = [0u64; 1];
        let mut written = 99;
        assert_eq!(
            ponko_ctx_light_gltf(ctx, path.as_ptr(), origin(), 8.0, spawned.as_mut_ptr(), 0, &mut written),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(world_len(ctx), entities);
        assert_eq!(ponko_ctx_has_environment(ctx, &mut has), PONKO_OK);
        assert!(!has);

        assert_eq!(
            ponko_ctx_light_gltf(ctx, path.as_ptr(), origin(), 8.0, spawned.as_mut_ptr(), 1, &mut written),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(written, 1);
        assert_eq!(world_len(ctx), entities + 1);
        // lux and candela are multiplied by 1 / (1.2 * 2^EV100); the file's range stays.
        let lantern = point_intensity(ctx, spawned[0]);
        let expected = COURTYARD_CANDELA / (1.2 * 256.0);
        assert!(
            (lantern.intensity - expected).abs() < expected * 1e-3,
            "{} vs {expected}",
            lantern.intensity
        );
        assert_eq!(lantern.range, 10.0);

        assert_eq!(ponko_ctx_has_environment(ctx, &mut has), PONKO_OK);
        assert!(has, "a preview without a map gets the studio");
        let mut post = std::mem::zeroed::<PonkoPost>();
        assert_eq!(ponko_ctx_post(ctx, &mut post), PONKO_OK);
        assert_eq!(post.tonemap, 4);
        let mut sun = std::mem::zeroed::<PonkoLighting>();
        assert_eq!(ponko_ctx_lighting(ctx, &mut sun), PONKO_OK);
        assert_eq!(sun.sun_intensity, 0.0, "the file has no sun");

        // Counting only: no list asked for, the light is still placed (one stop lower: twice as bright).
        assert_eq!(
            ponko_ctx_light_gltf(ctx, path.as_ptr(), origin(), 7.0, null_mut(), 0, &mut written),
            PONKO_OK
        );
        assert_eq!(written, 1);
        assert_eq!(world_len(ctx), entities + 2);

        // Errors name the file, and a missing one is not found.
        assert_eq!(
            ponko_ctx_light_gltf(
                ctx,
                c"levels/nowhere.gltf".as_ptr(),
                origin(),
                8.0,
                null_mut(),
                0,
                &mut written
            ),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_light_gltf(ctx, null(), origin(), 8.0, null_mut(), 0, &mut written),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_light_gltf(ctx, path.as_ptr(), origin(), 8.0, null_mut(), 0, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_light_gltf(ctx, path.as_ptr(), origin(), 8.0, null_mut(), 1, &mut written),
            PONKO_INVALID_ARGUMENT,
            "a capacity needs a list"
        );
        assert_eq!(
            ponko_ctx_gltf_local_light_count(ctx, path.as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn one_light_is_placed_with_exposure_and_the_sun_sets_the_scene_sun() {
    run_ctx(|ctx| unsafe {
        let mut light = ponko_model_light_default();
        assert_eq!(light.kind, PonkoModelLightKind::PONKO_MODEL_LIGHT_POINT);
        light.point.intensity = 2.0;
        light.point.range = 12.0;
        light.transform = ponko_transform3d_at(PonkoVec3 { x: 0.0, y: 2.0, z: 0.0 });
        let house = ponko_transform3d_at(PonkoVec3 {
            x: 10.0,
            y: 0.0,
            z: 0.0,
        });
        let (mut entity, mut spawned) = (0, false);
        assert_eq!(
            ponko_ctx_place_light_exposed(ctx, &light, house, 2.0, &mut entity, &mut spawned),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert!(spawned);
        // Point intensity is candela / 683: exposure multiplies the candela by 1 / (1.2 * 2^2).
        let placed = point_intensity(ctx, entity);
        let expected = 2.0 * 683.0 / (1.2 * 4.0);
        assert!(
            (placed.intensity - expected).abs() < expected * 1e-3,
            "{}",
            placed.intensity
        );
        assert_eq!(placed.range, 12.0, "an authored range stays");
        let mut at = std::mem::zeroed::<PonkoTransform3D>();
        let mut id = 0;
        assert_eq!(ponko_transform3d_component(ponko_ctx_world(ctx), &mut id), PONKO_OK);
        assert_eq!(
            ponko_get(
                ponko_ctx_world(ctx),
                entity,
                id,
                (&mut at as *mut PonkoTransform3D).cast(),
                size_of::<PonkoTransform3D>()
            ),
            PONKO_OK
        );
        assert_eq!((at.position.x, at.position.y), (10.0, 2.0));

        // A spot light spawns too, and the outputs are optional.
        let mut spot = ponko_model_light_default();
        spot.kind = PonkoModelLightKind::PONKO_MODEL_LIGHT_SPOT;
        spot.spot = PonkoSpotLight3D {
            color: spot.point.color,
            intensity: 1.0,
            range: 5.0,
            inner_angle: 0.2,
            outer_angle: 0.5,
            shadows: false,
            shadow_resolution: 0,
        };
        let before = world_len(ctx);
        assert_eq!(
            ponko_ctx_place_light_exposed(ctx, &spot, house, 0.0, null_mut(), null_mut()),
            PONKO_OK
        );
        assert_eq!(world_len(ctx), before + 1);

        // The sun spawns nothing and sets the scene's sun; lower EV100 is brighter, and NaN is 0.
        let mut sun = ponko_model_light_default();
        sun.kind = PonkoModelLightKind::PONKO_MODEL_LIGHT_SUN;
        sun.sun_intensity = 100_000.0;
        let before = world_len(ctx);
        entity = 77;
        assert_eq!(
            ponko_ctx_place_light_exposed(ctx, &sun, origin(), f32::NAN, &mut entity, &mut spawned),
            PONKO_OK
        );
        assert!(!spawned);
        assert_eq!(entity, 0);
        assert_eq!(world_len(ctx), before);
        let mut lighting = std::mem::zeroed::<PonkoLighting>();
        assert_eq!(ponko_ctx_lighting(ctx, &mut lighting), PONKO_OK);
        let expected = 100_000.0 / 1.2;
        assert!((lighting.sun_intensity - expected).abs() < expected * 1e-3);

        assert_eq!(
            ponko_ctx_place_light_exposed(ctx, null(), origin(), 0.0, &mut entity, &mut spawned),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_place_light_exposed(null_mut(), &sun, origin(), 0.0, &mut entity, &mut spawned),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn the_scene_environment_comes_from_a_panorama_six_faces_or_the_studio() {
    run_ctx(|ctx| unsafe {
        let faces: Vec<(String, Vec<u8>)> = (0..6).map(|i| (format!("face{i}.hdr"), hdr(2, 2))).collect();
        let mut files = vec![
            ("sky.hdr", hdr(4, 2)),
            ("square.hdr", hdr(2, 2)),
            ("broken.hdr", b"not an image".to_vec()),
        ];
        files.extend(faces.iter().map(|(name, bytes)| (name.as_str(), bytes.clone())));
        files.push(("wide.hdr", hdr(4, 2)));
        with_files(ctx, &files);
        let has = || {
            let mut has = false;
            assert_eq!(ponko_ctx_has_environment(ctx, &mut has), PONKO_OK);
            has
        };
        assert!(!has());

        assert_eq!(
            ponko_ctx_load_environment(ctx, c"sky.hdr".as_ptr()),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert!(has());
        assert_eq!(ponko_ctx_clear_environment(ctx), PONKO_OK);
        assert!(!has());
        assert_eq!(ponko_ctx_set_studio_environment(ctx), PONKO_OK);
        assert!(has());
        assert_eq!(ponko_ctx_clear_environment(ctx), PONKO_OK);

        // A failed load keeps the old environment: nothing, then the studio.
        for bad in [c"square.hdr", c"broken.hdr", c"nowhere.hdr"] {
            assert_ne!(ponko_ctx_load_environment(ctx, bad.as_ptr()), PONKO_OK);
            assert!(!has());
        }
        assert_eq!(
            ponko_ctx_load_environment(ctx, c"nowhere.hdr".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_load_environment(ctx, c"square.hdr".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_set_studio_environment(ctx), PONKO_OK);
        assert_eq!(
            ponko_ctx_load_environment(ctx, c"broken.hdr".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert!(has(), "the studio stays");
        assert_eq!(ponko_ctx_clear_environment(ctx), PONKO_OK);

        let names: Vec<CString> = faces
            .iter()
            .map(|(name, _)| CString::new(name.as_str()).unwrap())
            .collect();
        let mut paths: Vec<*const c_char> = names.iter().map(|n| n.as_ptr()).collect();
        assert_eq!(
            ponko_ctx_load_environment_cube(ctx, paths.as_ptr()),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert!(has());
        assert_eq!(ponko_ctx_clear_environment(ctx), PONKO_OK);
        // Faces must be equal-size squares.
        paths[3] = c"wide.hdr".as_ptr();
        assert_eq!(
            ponko_ctx_load_environment_cube(ctx, paths.as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert!(!has());
        paths[3] = null();
        assert_eq!(
            ponko_ctx_load_environment_cube(ctx, paths.as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_load_environment_cube(ctx, null()), PONKO_INVALID_ARGUMENT);

        assert_eq!(ponko_ctx_load_environment(ctx, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_has_environment(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_set_studio_environment(null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_clear_environment(null_mut()), PONKO_INVALID_ARGUMENT);
    });
}
