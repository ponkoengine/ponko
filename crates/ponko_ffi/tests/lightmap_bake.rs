//! Baked lighting from C: lightmap coordinates for a mesh, a scene of a
//! floor, a glowing block and a lamp, a small bake, its result kept as
//! bytes and read back, set for drawing, and the `PonkoLightmapped`
//! component, all through the C functions in a headless setup.
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;
use ponko_ffi::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

fn with_ctx(f: fn(*mut PonkoCtx)) {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
        let f: fn(*mut PonkoCtx) = unsafe { std::mem::transmute(user) };
        f(ctx);
    }
    unsafe {
        let game = ponko_game_new(c"bake".as_ptr(), 64, 64);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), f as *mut c_void, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

fn settings() -> PonkoLightBakeSettings {
    let mut s: PonkoLightBakeSettings = unsafe { std::mem::zeroed() };
    s.struct_size = size_of::<PonkoLightBakeSettings>() as u32;
    s.version = PONKO_LIGHT_BAKE_SETTINGS_VERSION;
    assert_eq!(unsafe { ponko_light_bake_settings_defaults(&mut s) }, PONKO_OK);
    s
}

fn model(mesh: *const PonkoMeshData, at: PonkoVec3) -> PonkoBakeModel {
    let mut m: PonkoBakeModel = unsafe { std::mem::zeroed() };
    m.struct_size = size_of::<PonkoBakeModel>() as u32;
    m.version = PONKO_BAKE_MODEL_VERSION;
    assert_eq!(unsafe { ponko_bake_model_defaults(&mut m) }, PONKO_OK);
    m.mesh = mesh;
    m.transform = ponko_transform3d_at(at);
    m
}

fn info(b: *const PonkoBakedLighting) -> PonkoBakedLightingInfo {
    let mut i: PonkoBakedLightingInfo = unsafe { std::mem::zeroed() };
    i.struct_size = size_of::<PonkoBakedLightingInfo>() as u32;
    i.version = PONKO_BAKED_LIGHTING_INFO_VERSION;
    assert_eq!(unsafe { ponko_baked_lighting_info(b, &mut i) }, PONKO_OK);
    i
}

/// The floor's lightmap texels, as light values.
fn floor_light(b: *const PonkoBakedLighting) -> Vec<[f32; 3]> {
    let mut rect = PonkoLightmapRect::default();
    assert_eq!(
        unsafe { ponko_baked_lighting_lightmap(b, 0, &mut rect, null_mut()) },
        PONKO_OK
    );
    let mut light = Vec::new();
    for y in rect.y..rect.y + rect.height {
        for x in rect.x..rect.x + rect.width {
            let (mut l, mut baked) = (v3(0.0, 0.0, 0.0), false);
            assert_eq!(
                unsafe { ponko_baked_lighting_texel(b, rect.page, x, y, &mut l, &mut baked) },
                PONKO_OK
            );
            if baked {
                light.push([l.x, l.y, l.z]);
            }
        }
    }
    light
}

/// A floor (lightmapped), a glowing block beside it (not lightmapped)
/// and a lamp above, baked small.
unsafe fn bake(ctx: *mut PonkoCtx) -> (*mut PonkoBakedLighting, usize, usize) {
    unsafe {
        let floor = ponko_mesh_data_plane(8.0);
        let mut side = 0;
        assert_eq!(ponko_mesh_data_generate_lightmap_uvs(floor, 2.0, &mut side), PONKO_OK);
        assert!(side >= 16, "an 8 x 8 floor at 2 texels a unit: {side}");
        let block = ponko_mesh_data_cube(1.0);

        let mut scene = null_mut();
        assert_eq!(ponko_ctx_light_bake_scene_new(ctx, &mut scene), PONKO_OK);
        let (mut a, mut b) = (99, 99);
        assert_eq!(
            ponko_light_bake_scene_add(scene, &model(floor, v3(0.0, 0.0, 0.0)), &mut a),
            PONKO_OK
        );
        let mut glow = model(block, v3(2.0, 0.5, 0.0));
        glow.lightmapped = false;
        glow.emissive = PonkoColor {
            r: 1.0,
            g: 0.5,
            b: 0.2,
            a: 1.0,
        };
        assert_eq!(ponko_light_bake_scene_add(scene, &glow, &mut b), PONKO_OK);
        let lamp = PonkoPointLight3D {
            color: PonkoColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            },
            intensity: 20.0,
            range: 10.0,
            shadows: false,
            shadow_resolution: 0,
        };
        assert_eq!(
            ponko_light_bake_scene_add_point_light(scene, v3(0.0, 3.0, 0.0), &lamp),
            PONKO_OK
        );
        let spot = PonkoSpotLight3D {
            color: PonkoColor {
                r: 1.0,
                g: 0.9,
                b: 0.8,
                a: 1.0,
            },
            intensity: 10.0,
            range: 8.0,
            inner_angle: 0.3,
            outer_angle: 0.5,
            shadows: false,
            shadow_resolution: 0,
        };
        assert_eq!(
            ponko_light_bake_scene_add_spot_light(scene, v3(-3.0, 3.0, 0.0), v3(0.0, -1.0, 0.0), &spot),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_light_bake_scene_add_lights(ctx, scene), PONKO_OK);

        let mut s = settings();
        s.page_size = 64;
        s.samples = 8;
        s.bounces = 1;
        s.texels_per_unit = 2.0;
        s.probe_spacing = 2.0;
        let mut baked = null_mut();
        assert_eq!(ponko_bake_lighting(scene, &s, &mut baked), PONKO_OK);

        ponko_light_bake_scene_free(scene);
        ponko_mesh_data_free(floor);
        ponko_mesh_data_free(block);
        (baked, a, b)
    }
}

#[test]
fn a_c_scene_bakes_lightmaps_and_probes() {
    with_ctx(|ctx| unsafe {
        let (baked, floor, block) = bake(ctx);
        assert_eq!((floor, block), (0, 1));
        let i = info(baked);
        assert_eq!((i.page_size, i.pages), (64, 1));
        assert!(i.directional && i.gpu_bytes > 0);
        assert!(i.texels > 0 && i.probes > 0 && i.rays > 0 && i.seconds >= 0.0, "{i:?}");

        // The floor has a lightmap, lit; the block, not lightmapped, has none.
        let (mut rect, mut component) = (PonkoLightmapRect::default(), PonkoLightmapped { index: 9 });
        assert_eq!(
            ponko_baked_lighting_lightmap(baked, 0, &mut rect, &mut component),
            PONKO_OK
        );
        assert!(rect.width > 0 && rect.height > 0 && rect.x + rect.width <= 64);
        assert_eq!(component.index, 0);
        assert_eq!(
            ponko_baked_lighting_lightmap(baked, 1, null_mut(), null_mut()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_baked_lighting_lightmap(baked, 7, null_mut(), null_mut()),
            PONKO_NOT_FOUND
        );
        let light = floor_light(baked);
        assert!(!light.is_empty());
        assert!(light.iter().any(|l| l[0] > 0.0), "the lamp lights the floor");
        let mut l = v3(0.0, 0.0, 0.0);
        assert_eq!(
            ponko_baked_lighting_texel(baked, 0, 64, 0, &mut l, null_mut()),
            PONKO_NOT_FOUND
        );

        // Probes light what moves above the floor; far away there are none.
        assert_eq!(
            ponko_baked_lighting_probe_light(baked, v3(0.0, 1.0, 0.0), v3(0.0, 1.0, 0.0), &mut l),
            PONKO_OK
        );
        assert!(l.x > 0.0 && l.y > 0.0, "{l:?}");
        assert_eq!(
            ponko_baked_lighting_probe_light(baked, v3(500.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), &mut l),
            PONKO_NOT_FOUND
        );

        // Drawn with it, and turned off again.
        assert_eq!(ponko_ctx_set_baked_lighting(ctx, baked), PONKO_OK);
        assert_eq!(ponko_ctx_set_baked_lighting(ctx, null()), PONKO_OK);
        ponko_baked_lighting_free(baked);
    });
}

#[test]
fn the_same_c_scene_bakes_the_same_light() {
    with_ctx(|ctx| unsafe {
        let (a, ..) = bake(ctx);
        let (b, ..) = bake(ctx);
        assert_eq!(floor_light(a), floor_light(b));
        ponko_baked_lighting_free(a);
        ponko_baked_lighting_free(b);
    });
}

#[test]
fn c_baked_lighting_survives_bytes() {
    with_ctx(|ctx| unsafe {
        let (baked, ..) = bake(ctx);
        let mut len = 0usize;
        assert_eq!(ponko_baked_lighting_to_bytes(baked, null_mut(), 0, &mut len), PONKO_OK);
        assert!(len > 64 * 64 * 8, "{len}");
        // Too small: only the length.
        let mut small = vec![0u8; 16];
        assert_eq!(
            ponko_baked_lighting_to_bytes(baked, small.as_mut_ptr(), 16, &mut len),
            PONKO_OK
        );
        assert!(small.iter().all(|&b| b == 0));
        let mut bytes = vec![0u8; len];
        assert_eq!(
            ponko_baked_lighting_to_bytes(baked, bytes.as_mut_ptr(), len, &mut len),
            PONKO_OK
        );

        let mut back = null_mut();
        assert_eq!(
            ponko_baked_lighting_from_bytes(bytes.as_ptr().cast(), len, &mut back),
            PONKO_OK
        );
        let (x, y) = (info(baked), info(back));
        // The light and the layout come back; how the bake went is not kept.
        assert_eq!(
            (x.page_size, x.pages, x.directional, x.gpu_bytes),
            (y.page_size, y.pages, y.directional, y.gpu_bytes)
        );
        assert_eq!(floor_light(baked), floor_light(back));
        let mut l = (v3(0.0, 0.0, 0.0), v3(0.0, 0.0, 0.0));
        let at = (v3(0.5, 1.0, 0.5), v3(0.0, 1.0, 0.0));
        assert_eq!(ponko_baked_lighting_probe_light(baked, at.0, at.1, &mut l.0), PONKO_OK);
        assert_eq!(ponko_baked_lighting_probe_light(back, at.0, at.1, &mut l.1), PONKO_OK);
        assert_eq!(l.0, l.1);

        // The same bytes as a file of an asset source.
        let source = ponko_memory_source_new(c"bakes".as_ptr());
        assert_eq!(
            ponko_memory_source_insert(source, c"room.lit".as_ptr(), bytes.as_ptr(), len),
            PONKO_OK
        );
        assert_eq!(
            ponko_memory_source_insert(source, c"bad.lit".as_ptr(), bytes.as_ptr(), 40),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
        let mut loaded = null_mut();
        assert_eq!(
            ponko_ctx_load_baked_lighting(ctx, c"room.lit".as_ptr(), &mut loaded),
            PONKO_OK
        );
        assert_eq!(floor_light(loaded), floor_light(baked));
        ponko_baked_lighting_free(loaded);

        let mut none = null_mut();
        assert_eq!(
            ponko_ctx_load_baked_lighting(ctx, c"missing.lit".as_ptr(), &mut none),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_load_baked_lighting(ctx, c"bad.lit".as_ptr(), &mut none),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_baked_lighting_from_bytes(bytes.as_ptr().cast(), 40, &mut none),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_baked_lighting_from_bytes(c"PONKO".as_ptr().cast(), 5, &mut none),
            PONKO_INVALID_ARGUMENT
        );
        assert!(none.is_null());
        ponko_baked_lighting_free(baked);
        ponko_baked_lighting_free(back);
    });
}

#[test]
fn bad_c_bakes_are_refused_with_a_reason() {
    with_ctx(|ctx| unsafe {
        let bare = ponko_mesh_data_plane(2.0);
        let mut scene = null_mut();
        assert_eq!(ponko_ctx_light_bake_scene_new(ctx, &mut scene), PONKO_OK);
        // A lightmapped mesh without lightmap coordinates; no mesh at all.
        assert_eq!(
            ponko_light_bake_scene_add(scene, &model(bare, v3(0.0, 0.0, 0.0)), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_light_bake_scene_add(scene, &model(null(), v3(0.0, 0.0, 0.0)), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        // Not lightmapped, it may lack them.
        let mut shade = model(bare, v3(0.0, 0.0, 0.0));
        shade.lightmapped = false;
        assert_eq!(ponko_light_bake_scene_add(scene, &shade, null_mut()), PONKO_OK);
        // A page size out of range.
        let mut s = settings();
        s.page_size = 10;
        let mut baked = null_mut();
        assert_eq!(ponko_bake_lighting(scene, &s, &mut baked), PONKO_INVALID_ARGUMENT);
        assert!(baked.is_null());
        // A newer settings version than this library knows.
        let mut future = settings();
        future.version = 99;
        assert_eq!(ponko_bake_lighting(scene, &future, &mut baked), PONKO_INVALID_ARGUMENT);
        ponko_light_bake_scene_free(scene);
        ponko_mesh_data_free(bare);
    });
}

#[test]
fn lightmapped_is_a_plain_c_component() {
    unsafe {
        let w = ponko_world_new();
        let mut id = 0;
        assert_eq!(ponko_lightmapped_component(w, &mut id), PONKO_OK);
        let value = PonkoLightmapped { index: 3 };
        let v = PonkoValue {
            component: id,
            data: (&raw const value).cast(),
            size: 4,
        };
        let mut e = 0;
        assert_eq!(ponko_spawn(w, &v, 1, &mut e), PONKO_OK);
        let mut back = PonkoLightmapped::default();
        assert_eq!(ponko_get(w, e, id, (&raw mut back).cast(), 4), PONKO_OK);
        assert_eq!(back, value);
        ponko_world_free(w);
    }
}
