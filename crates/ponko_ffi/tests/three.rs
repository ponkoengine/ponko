//! 3D from C: transforms and models as plain components and query columns,
//! meshes and materials, the camera and the light.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

#[derive(Default)]
struct Ids {
    transform: u32,
    model: u32,
    cubes: Vec<u64>,
    mesh: PonkoMesh,
    errors: Vec<PonkoStatus>,
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}
fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}
const WHITE: PonkoColor = PonkoColor {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let ids = &mut *user.cast::<Ids>();
        let w = ponko_ctx_world(ctx);
        assert_eq!(ponko_transform3d_component(w, &mut ids.transform), PONKO_OK);
        assert_eq!(ponko_model_component(w, &mut ids.model), PONKO_OK);

        let mut stone = PonkoMaterial::default();
        let desc = ponko_material_desc(WHITE);
        assert_eq!(ponko_ctx_create_material(ctx, &desc, &mut stone), PONKO_OK);
        let cube = ponko_mesh_data_cube(1.0);
        assert_eq!(ponko_ctx_create_mesh(ctx, cube, stone, &mut ids.mesh), PONKO_OK);
        let mut batch = PonkoInstanceBatch::default();
        let matrices = [ponko::prelude::Mat4::IDENTITY.to_cols_array()];
        let options = PonkoInstanceBatchOptions {
            cell_size: 32.0,
            max_distance: 100.0,
            cast_shadows: true,
            tint: WHITE,
        };
        assert_eq!(
            ponko_ctx_create_instance_batch(ctx, ids.mesh, matrices.as_ptr(), 1, options, &mut batch),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_update_instance_batch(ctx, batch, matrices.as_ptr(), 1),
            PONKO_OK
        );
        for mode in 0..=2 {
            assert_eq!(ponko_ctx_set_instance_batch_culling(ctx, batch, mode), PONKO_OK);
        }
        assert_eq!(
            ponko_ctx_set_instance_batch_culling(ctx, batch, 3),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_instance_batch_culling(null_mut(), batch, 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_free_instance_batch(ctx, batch), PONKO_OK);
        assert_eq!(
            ponko_ctx_set_instance_batch_culling(ctx, batch, 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_update_instance_batch(ctx, batch, matrices.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_create_instance_batch(ctx, ids.mesh, null(), 1, options, &mut batch),
            PONKO_INVALID_ARGUMENT
        );
        let mut bad = matrices;
        bad[0][0] = f32::NAN;
        assert_eq!(
            ponko_ctx_create_instance_batch(ctx, ids.mesh, bad.as_ptr(), 1, options, &mut batch),
            PONKO_INVALID_ARGUMENT
        );
        let levels = [
            PonkoInstanceLevel {
                mesh: ids.mesh,
                min_distance: 0.0,
                max_distance: 10.0,
                camera: true,
                shadows: false,
            },
            PonkoInstanceLevel {
                mesh: ids.mesh,
                min_distance: 10.0,
                max_distance: f32::INFINITY,
                camera: false,
                shadows: true,
            },
        ];
        let colors = [WHITE];
        assert_eq!(
            ponko_ctx_create_instance_batch_levels(
                ctx,
                levels.as_ptr(),
                2,
                matrices.as_ptr(),
                1,
                colors.as_ptr(),
                1,
                options,
                &mut batch
            ),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, matrices.as_ptr(), 1, null(), 0),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, matrices.as_ptr(), 1, colors.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, matrices.as_ptr(), 1, colors.as_ptr(), 2),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, matrices.as_ptr(), 1, null(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, bad.as_ptr(), 1, null(), 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_free_instance_batch(ctx, batch), PONKO_OK);
        assert_eq!(
            ponko_ctx_update_instance_batch_colors(ctx, batch, null(), 0, null(), 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_create_instance_batch_levels(ctx, null(), 0, null(), 0, null(), 0, options, &mut batch),
            PONKO_INVALID_ARGUMENT
        );
        let mut invalid_levels = levels;
        invalid_levels[0].max_distance = 0.0;
        assert_eq!(
            ponko_ctx_create_instance_batch_levels(
                ctx,
                invalid_levels.as_ptr(),
                2,
                null(),
                0,
                null(),
                0,
                options,
                &mut batch
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_create_instance_batch_levels(ctx, levels.as_ptr(), 2, null(), 0, null(), 0, options, &mut batch),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_free_instance_batch(ctx, batch), PONKO_OK);
        let mut b = PonkoGpuBuffer::default();
        assert_eq!(
            ponko_ctx_create_gpu_buffer(ctx, 2, std::ptr::null(), 0, &mut b),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_set_mesh_gpu_buffer(ctx, ids.mesh, b, 2), PONKO_OK);
        assert_eq!(
            ponko_ctx_set_mesh_gpu_bounds(ctx, ids.mesh, v3(0.0, 5.0, 0.0), 3.0),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_set_mesh_gpu_bounds(ctx, ids.mesh, v3(0.0, 5.0, 0.0), -1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_mesh_gpu_bounds(ctx, PonkoMesh { id: u32::MAX }, v3(0.0, 0.0, 0.0), 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_mesh_gpu_buffer(ctx, ids.mesh, b, 3),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_mesh_gpu_buffer(ctx, PonkoMesh { id: u32::MAX }, b, 1),
            PONKO_INVALID_ARGUMENT
        );
        // A material this game didn't make.
        let mut other = PonkoMesh::default();
        ids.errors
            .push(ponko_ctx_create_mesh(ctx, cube, PonkoMaterial { id: 99 }, &mut other));
        ponko_mesh_data_free(cube);
        let shiny = PonkoMaterialDesc { metallic: 2.0, ..desc };
        ids.errors.push(ponko_ctx_create_material(ctx, &shiny, &mut stone));

        for i in 0..3 {
            let t = ponko_transform3d_at(v3(i as f32 * 2.0, 0.0, 0.0));
            let m = ponko_model(ids.mesh);
            let parts = [value(ids.transform, &t), value(ids.model, &m)];
            let mut e = 0;
            assert_eq!(ponko_spawn(w, parts.as_ptr(), 2, &mut e), PONKO_OK);
            ids.cubes.push(e);
        }

        let mut camera = std::mem::zeroed::<PonkoCamera3D>();
        assert_eq!(ponko_ctx_camera3d(ctx, &mut camera), PONKO_OK);
        camera.position = v3(0.0, 5.0, 10.0);
        assert_eq!(ponko_ctx_set_camera3d(ctx, &camera), PONKO_OK);
        assert_eq!(ponko_ctx_camera3d_look_at(ctx, v3(0.0, 0.0, 0.0)), PONKO_OK);
        let mut culling = true;
        assert_eq!(ponko_ctx_occlusion_culling(ctx, &mut culling), PONKO_OK);
        assert!(!culling);
        assert_eq!(ponko_ctx_set_occlusion_culling(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_occlusion_culling(ctx, &mut culling), PONKO_OK);
        assert!(culling);
        assert_eq!(ponko_ctx_set_occlusion_culling(ctx, false), PONKO_OK);
        let bad = PonkoCamera3D { near: 0.0, ..camera };
        ids.errors.push(ponko_ctx_set_camera3d(ctx, &bad));

        let mut light = std::mem::zeroed::<PonkoLighting>();
        assert_eq!(ponko_ctx_lighting(ctx, &mut light), PONKO_OK);
        assert!(light.shadows);
        light.sun_intensity = 5.0;
        assert_eq!(ponko_ctx_set_lighting(ctx, &light), PONKO_OK);
        let dark = PonkoLighting {
            sun_intensity: -1.0,
            ..light
        };
        ids.errors.push(ponko_ctx_set_lighting(ctx, &dark));
        assert_eq!((light.shadow_cascades, light.shadow_refresh), (4, 2));
        assert_eq!(light.shadow_filter, 3);
        assert!(!light.has_equator);
        let trilight = PonkoLighting {
            has_equator: true,
            equator_color: PonkoColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
                a: 1.0,
            },
            ..light
        };
        assert_eq!(ponko_ctx_set_lighting(ctx, &trilight), PONKO_OK);
        let mut back = std::mem::zeroed::<PonkoLighting>();
        assert_eq!(ponko_ctx_lighting(ctx, &mut back), PONKO_OK);
        assert!(back.has_equator && back.equator_color.g == 1.0);
        // The ambient colors are the sky's in Rust: setting the sky keeps them.
        let mut sky_now = std::mem::zeroed::<PonkoSky>();
        assert_eq!(ponko_ctx_sky(ctx, &mut sky_now), PONKO_OK);
        assert_eq!(ponko_ctx_set_sky(ctx, &sky_now), PONKO_OK);
        assert_eq!(ponko_ctx_lighting(ctx, &mut back), PONKO_OK);
        assert!(back.has_equator && back.equator_color.g == 1.0);
        assert_eq!(
            (back.sky_color, back.ground_color),
            (light.sky_color, light.ground_color)
        );
        assert_eq!(ponko_ctx_set_lighting(ctx, &light), PONKO_OK);
        let many = PonkoLighting {
            shadow_cascades: 5,
            ..light
        };
        ids.errors.push(ponko_ctx_set_lighting(ctx, &many));
        // Global illumination: off by default; its settings are checked
        // only when it's on.
        assert!(!light.gi_enabled);
        assert_eq!(
            (
                light.gi_extent.y,
                light.gi_spacing,
                light.gi_probes_per_frame,
                light.gi_intensity
            ),
            (16.0, 2.0, 4, 1.0)
        );
        let bounced = PonkoLighting {
            gi_enabled: true,
            gi_spacing: 1.5,
            gi_probes_per_frame: 8,
            ..light
        };
        assert_eq!(ponko_ctx_set_lighting(ctx, &bounced), PONKO_OK);
        assert_eq!(ponko_ctx_lighting(ctx, &mut back), PONKO_OK);
        assert!(back.gi_enabled && back.gi_spacing == 1.5 && back.gi_probes_per_frame == 8);
        for bad in [
            PonkoLighting {
                gi_spacing: 0.0,
                ..bounced
            },
            PonkoLighting {
                gi_probes_per_frame: 17,
                ..bounced
            },
            PonkoLighting {
                gi_intensity: f32::NAN,
                ..bounced
            },
            PonkoLighting {
                gi_extent: v3(0.0, 1.0, 1.0),
                ..bounced
            },
        ] {
            assert_eq!(ponko_ctx_set_lighting(ctx, &bad), PONKO_INVALID_ARGUMENT);
            let off = PonkoLighting {
                gi_enabled: false,
                ..bad
            };
            assert_eq!(ponko_ctx_set_lighting(ctx, &off), PONKO_OK);
        }
        assert_eq!(ponko_ctx_set_lighting(ctx, &light), PONKO_OK);

        let mut sky = std::mem::zeroed::<PonkoSky>();
        assert_eq!(ponko_ctx_sky(ctx, &mut sky), PONKO_OK);
        assert!(sky.atmosphere && sky.fog_density == 0.0);
        sky.fog_density = 0.01;
        assert_eq!(ponko_ctx_set_sky(ctx, &sky), PONKO_OK);
        let flat = PonkoSky {
            fog_falloff: 0.0,
            ..sky
        };
        ids.errors.push(ponko_ctx_set_sky(ctx, &flat));
        assert!(sky.fog_end == 0.0 && sky.fog_sun_focus == 0.7);
        let far = PonkoSky {
            fog_start: 500.0,
            fog_end: 1200.0,
            fog_sun_glow: 0.06,
            ..sky
        };
        assert_eq!(ponko_ctx_set_sky(ctx, &far), PONKO_OK);
        let mut back = std::mem::zeroed::<PonkoSky>();
        assert_eq!(ponko_ctx_sky(ctx, &mut back), PONKO_OK);
        assert_eq!(back, far);
        let ring = PonkoSky {
            fog_sun_focus: 1.0,
            ..sky
        };
        ids.errors.push(ponko_ctx_set_sky(ctx, &ring));
        let bright = PonkoSky {
            light_shafts: 1.5,
            ..sky
        };
        ids.errors.push(ponko_ctx_set_sky(ctx, &bright));
        let behind = PonkoSky { fog_start: -1.0, ..sky };
        ids.errors.push(ponko_ctx_set_sky(ctx, &behind));
        assert!(ponko_sun_direction_at(12.0).y < -0.8);

        let mut post = std::mem::zeroed::<PonkoPost>();
        assert_eq!(ponko_ctx_post(ctx, &mut post), PONKO_OK);
        assert_eq!((post.tonemap, post.contrast, post.has_lut), (0, 1.0, false));
        post.tonemap = 1;
        post.bloom = 0.3;
        assert_eq!(ponko_ctx_set_post(ctx, &post), PONKO_OK);
        assert!(!post.ao_enabled);
        assert_eq!(post.anti_aliasing, 0);
        post.ao_enabled = true;
        post.ao_half_resolution = true;
        post.ao_temporal = true;
        post.ao_radius = 2.0;
        post.ao_slices = 4;
        post.ao_steps = 8;
        post.anti_aliasing = 2;
        assert_eq!(ponko_ctx_set_post(ctx, &post), PONKO_OK);
        assert_ne!(
            ponko_ctx_set_post(
                ctx,
                &PonkoPost {
                    anti_aliasing: 4,
                    ..post
                }
            ),
            PONKO_OK
        );
        assert_ne!(
            ponko_ctx_set_post(
                ctx,
                &PonkoPost {
                    taa_sharpness: 2.0,
                    ..post
                }
            ),
            PONKO_OK
        );
        assert_ne!(
            ponko_ctx_set_post(
                ctx,
                &PonkoPost {
                    contact_shadows: -1.0,
                    ..post
                }
            ),
            PONKO_OK
        );
        let contact = PonkoPost {
            contact_shadows: 0.3,
            ..post
        };
        assert_eq!(ponko_ctx_set_post(ctx, &contact), PONKO_OK);
        let mut back = std::mem::zeroed::<PonkoPost>();
        assert_eq!(ponko_ctx_post(ctx, &mut back), PONKO_OK);
        assert_eq!(back.contact_shadows, 0.3);
        // Screen-space reflections: off by default, 0 to 1, a positive reach.
        assert_eq!((post.ssr, post.ssr_max_distance), (0.0, 30.0));
        let reflections = PonkoPost {
            ssr: 0.8,
            ssr_max_distance: 12.0,
            ..post
        };
        assert_eq!(ponko_ctx_set_post(ctx, &reflections), PONKO_OK);
        assert_eq!(ponko_ctx_post(ctx, &mut back), PONKO_OK);
        assert_eq!((back.ssr, back.ssr_max_distance), (0.8, 12.0));
        for bad in [
            PonkoPost { ssr: 1.5, ..post },
            PonkoPost { ssr: -0.1, ..post },
            PonkoPost { ssr: f32::NAN, ..post },
            PonkoPost {
                ssr_max_distance: 0.0,
                ..post
            },
            PonkoPost {
                ssr_max_distance: f32::INFINITY,
                ..post
            },
        ] {
            assert_eq!(ponko_ctx_set_post(ctx, &bad), PONKO_INVALID_ARGUMENT);
        }
        // Temporal antialiasing, sharpened less, and a camera cut.
        let taa = PonkoPost {
            anti_aliasing: 3,
            taa_sharpness: 0.5,
            ao_enabled: true,
            ao_temporal: true,
            ..post
        };
        assert_eq!(ponko_ctx_set_post(ctx, &taa), PONKO_OK);
        assert_eq!(ponko_ctx_reset_temporal_history(ctx), PONKO_OK);
        assert_eq!(ponko_ctx_set_post(ctx, &post), PONKO_OK);
        assert_ne!(
            ponko_ctx_set_post(
                ctx,
                &PonkoPost {
                    ao_radius: f32::NAN,
                    ..post
                }
            ),
            PONKO_OK
        );
        assert_ne!(ponko_ctx_set_post(ctx, &PonkoPost { ao_steps: 0, ..post }), PONKO_OK);
        let mut back = std::mem::zeroed::<PonkoPost>();
        assert_eq!(ponko_ctx_post(ctx, &mut back), PONKO_OK);
        assert_eq!(back, post);
        assert_eq!(post.render_scale, 1.0);
        assert_eq!(post.sharpen, 0.0);
        for bad in [
            PonkoPost {
                render_scale: 0.1,
                ..post
            },
            PonkoPost {
                render_scale: 2.5,
                ..post
            },
            PonkoPost {
                render_scale: f32::NAN,
                ..post
            },
            PonkoPost { sharpen: 1.5, ..post },
        ] {
            assert_eq!(ponko_ctx_set_post(ctx, &bad), PONKO_INVALID_ARGUMENT);
        }
        let scaled = PonkoPost {
            render_scale: 0.5,
            sharpen: 0.4,
            ..post
        };
        assert_eq!(ponko_ctx_set_post(ctx, &scaled), PONKO_OK);
        assert_eq!(ponko_ctx_post(ctx, &mut back), PONKO_OK);
        assert_eq!(back, scaled);
        assert_eq!(ponko_ctx_set_post(ctx, &post), PONKO_OK);

        // Graphics quality: presets, reading back, tweaking, validation.
        let mut q = std::mem::zeroed::<PonkoGraphicsQuality>();
        assert_eq!(ponko_graphics_quality_preset(4, &mut q), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_graphics_quality_preset(0, std::ptr::null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let mut level = 99;
        for expected in 0..4u32 {
            assert_eq!(ponko_graphics_quality_preset(expected, &mut q), PONKO_OK);
            assert_eq!(ponko_graphics_quality_level(&q, &mut level), PONKO_OK);
            assert_eq!(level, expected);
            assert_eq!(ponko_ctx_set_graphics_quality(ctx, &q), PONKO_OK);
            let mut read = std::mem::zeroed::<PonkoGraphicsQuality>();
            assert_eq!(ponko_ctx_graphics_quality(ctx, &mut read), PONKO_OK);
            assert_eq!(read, q);
        }
        // Ultra is applied: the lighting and post structs show it.
        assert_eq!(ponko_ctx_lighting(ctx, &mut light), PONKO_OK);
        assert_eq!(
            (
                light.shadow_resolution,
                light.light_shadow_views,
                light.light_shadow_atlas,
                light.shadow_cascades
            ),
            (4096, 24, 8192, 4)
        );
        assert_eq!(light.shadow_filter, 4, "ultra has contact-hardening sun shadows");
        assert_eq!(ponko_ctx_set_lighting(ctx, &light), PONKO_OK, "and C can set them back");
        assert_eq!(ponko_ctx_post(ctx, &mut back), PONKO_OK);
        assert!(
            back.ao_half_resolution
                && back.ao_temporal
                && back.ao_enabled
                && back.anti_aliasing == 2
                && back.ao_steps == 8
                && back.ssr == 1.0
        );
        assert!(q.gi, "ultra has global illumination");
        let mut lit = std::mem::zeroed::<PonkoLighting>();
        assert_eq!(ponko_ctx_lighting(ctx, &mut lit), PONKO_OK);
        assert!(lit.gi_enabled);
        // A tweak makes it custom.
        q.shadow_cascades = 2;
        assert_eq!(ponko_ctx_set_graphics_quality(ctx, &q), PONKO_OK);
        assert_eq!(ponko_graphics_quality_level(&q, &mut level), PONKO_OK);
        assert_eq!(level, 4);
        // Invalid values are refused, and change nothing.
        let before = q;
        for bad in [
            PonkoGraphicsQuality { render_scale: 0.0, ..q },
            PonkoGraphicsQuality { sharpen: -1.0, ..q },
            PonkoGraphicsQuality {
                shadow_cascades: 5,
                ..q
            },
            PonkoGraphicsQuality {
                shadow_resolution: 100,
                ..q
            },
            PonkoGraphicsQuality {
                shadow_resolution: 8192,
                ..q
            },
            PonkoGraphicsQuality {
                light_shadow_views: 25,
                ..q
            },
            PonkoGraphicsQuality {
                light_shadow_atlas: 3000,
                ..q
            },
            PonkoGraphicsQuality {
                light_shadow_atlas: 16384,
                ..q
            },
            PonkoGraphicsQuality {
                light_shadow_resolution: 4096,
                ..q
            },
            PonkoGraphicsQuality { ao_slices: 0, ..q },
            PonkoGraphicsQuality { ao_steps: 17, ..q },
            PonkoGraphicsQuality { anti_aliasing: 4, ..q },
            PonkoGraphicsQuality { shadow_refresh: 0, ..q },
            PonkoGraphicsQuality { shadow_filter: 0, ..q },
            PonkoGraphicsQuality { shadow_filter: 5, ..q },
            PonkoGraphicsQuality { ssr: 2.0, ..q },
            PonkoGraphicsQuality {
                shadow_distance: f32::NAN,
                ..q
            },
        ] {
            assert_eq!(ponko_ctx_set_graphics_quality(ctx, &bad), PONKO_INVALID_ARGUMENT);
            assert_eq!(ponko_graphics_quality_level(&bad, &mut level), PONKO_INVALID_ARGUMENT);
        }
        let mut read = std::mem::zeroed::<PonkoGraphicsQuality>();
        assert_eq!(ponko_ctx_graphics_quality(ctx, &mut read), PONKO_OK);
        assert_eq!(read, before);
        assert_eq!(
            ponko_ctx_set_graphics_quality(ctx, std::ptr::null()),
            PONKO_INVALID_ARGUMENT
        );
        // Lighting validates the new fields as it does the others.
        assert_eq!(ponko_ctx_lighting(ctx, &mut light), PONKO_OK);
        for bad in [
            PonkoLighting {
                shadow_resolution: 128,
                ..light
            },
            PonkoLighting {
                light_shadow_views: 25,
                ..light
            },
            PonkoLighting {
                light_shadow_atlas: 512,
                ..light
            },
            PonkoLighting {
                light_shadow_resolution: 64,
                ..light
            },
            PonkoLighting {
                shadow_refresh: 0,
                ..light
            },
            PonkoLighting {
                shadow_refresh: 9,
                ..light
            },
            PonkoLighting {
                shadow_filter: 0,
                ..light
            },
            PonkoLighting {
                shadow_filter: 5,
                ..light
            },
        ] {
            assert_eq!(ponko_ctx_set_lighting(ctx, &bad), PONKO_INVALID_ARGUMENT);
        }
        // Back to the defaults for the rest of the test.
        assert_eq!(ponko_ctx_set_post(ctx, &post), PONKO_OK);
        ids.errors
            .push(ponko_ctx_set_post(ctx, &PonkoPost { tonemap: 7, ..post }));

        let mut wind = PonkoShader::default();
        let source = c"fn vertex(v: Vertex) -> Vertex { var o = v; o.position.x += material.custom[0].x; return o; }";
        assert_eq!(
            ponko_ctx_create_shader(ctx, c"wind".as_ptr(), source.as_ptr(), &mut wind),
            PONKO_OK
        );
        let mut leaves = PonkoMaterial::default();
        let mut desc = ponko_material_desc(WHITE);
        desc.has_shader = true;
        desc.shader = wind;
        desc.double_sided = true;
        desc.custom[0] = 0.2;
        assert_eq!(ponko_ctx_create_material(ctx, &desc, &mut leaves), PONKO_OK);
        desc.blend = 1;
        assert_eq!(ponko_ctx_update_material(ctx, leaves, &desc), PONKO_OK);
        desc.blend = 2; // cut out
        assert_eq!(ponko_ctx_update_material(ctx, leaves, &desc), PONKO_OK);
        let broken = c"fn surface(s: Surface) -> Surface { return 1; }";
        ids.errors.push(ponko_ctx_create_shader(
            ctx,
            c"broken".as_ptr(),
            broken.as_ptr(),
            &mut wind,
        ));
        desc.shader = PonkoShader { id: 42 };
        ids.errors.push(ponko_ctx_create_material(ctx, &desc, &mut leaves));
    }
}

/// Spins every model a little around Y, through a query column.
unsafe extern "C" fn spin(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe extern "C" fn each(chunk: *const PonkoChunk, _: *mut c_void) {
        unsafe {
            let chunk = &*chunk;
            let transforms = std::slice::from_raw_parts_mut((*chunk.columns).cast::<PonkoTransform3D>(), chunk.count);
            let turn = ponko_quat_from_axis_angle(v3(0.0, 1.0, 0.0), 0.1);
            for t in transforms {
                t.rotation = ponko_quat_mul(turn, t.rotation);
            }
        }
    }
    unsafe {
        let ids = &*user.cast::<Ids>();
        let columns = [ids.transform, ids.model];
        assert_eq!(
            ponko_query(ponko_ctx_world(ctx), columns.as_ptr(), 2, Some(each), null_mut()),
            PONKO_OK
        );
    }
}

#[test]
fn games_build_and_move_3d_worlds_from_c() {
    let mut ids = Ids::default();
    let user = (&raw mut ids).cast();
    unsafe {
        let g = ponko_game_new(c"3d".as_ptr(), 640, 480);
        assert_eq!(
            ponko_game_sim_system(g, c"spin".as_ptr(), 1, Some(spin), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 5, Some(setup), user, &mut world), PONKO_OK);
        assert_eq!(
            ids.errors, [PONKO_INVALID_ARGUMENT; 12],
            "bad material, bad metallic, bad camera, negative light, bad fog, bad tone map, broken shader, unknown shader"
        );

        // Five ticks of 0.1 radians, as Rust sees it through the same bytes.
        let mut t = std::mem::zeroed::<PonkoTransform3D>();
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(world, ids.cubes[1], ids.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        assert_eq!(t.position.x, 2.0);
        let expected = ponko_quat_from_axis_angle(v3(0.0, 1.0, 0.0), 0.5);
        assert!((t.rotation.y - expected.y).abs() < 1e-5, "{:?}", t.rotation);
        let mut m = ponko_model(PonkoMesh::default());
        let size = size_of::<PonkoModel>();
        assert_eq!(
            ponko_get(world, ids.cubes[2], ids.model, (&raw mut m).cast(), size),
            PONKO_OK
        );
        assert_eq!(m.mesh, ids.mesh);
        ponko_world_free(world);
    }
}

#[test]
fn mesh_data_round_trips_and_is_checked() {
    let positions = [v3(0.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)];
    unsafe {
        let d = ponko_mesh_data_new(positions.as_ptr(), null(), null(), 3, [0, 1, 2].as_ptr(), 3);
        assert!(!d.is_null());
        let mut view = std::mem::zeroed::<PonkoMeshView>();
        assert_eq!(ponko_mesh_data_view(d, &mut view), PONKO_OK);
        assert_eq!((view.vertex_count, view.index_count), (3, 3));
        assert!(view.normals.is_null() && view.uvs.is_null());
        assert_eq!(*view.positions.add(1), positions[1]);
        ponko_mesh_data_free(d);

        // Two indices: not a whole triangle.
        assert!(ponko_mesh_data_new(positions.as_ptr(), null(), null(), 3, [0, 1].as_ptr(), 2).is_null());
        let error = std::ffi::CStr::from_ptr(ponko_last_error()).to_str().unwrap();
        assert!(error.contains("3 per triangle"), "{error}");
    }
}

unsafe extern "C" fn terrain(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut tile_id) = (0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_terrain_tile_component(w, &mut tile_id), PONKO_OK);
        let mut ground = PonkoMaterial::default();
        assert_eq!(
            ponko_ctx_create_material(ctx, &ponko_material_desc(WHITE), &mut ground),
            PONKO_OK
        );
        let heights: Vec<f32> = (0..81).map(|i| (i % 9) as f32 * 0.5).collect();
        let mut tile = std::mem::zeroed::<PonkoTerrainTile>();
        assert_eq!(
            ponko_ctx_create_terrain_tile(ctx, heights.as_ptr(), heights.len(), 16.0, ground, &mut tile),
            PONKO_OK
        );
        assert_eq!(tile.lod_distance, 16.0);
        let ids: Vec<u32> = tile.lods.iter().map(|m| m.id).collect();
        assert!(ids.windows(2).all(|p| p[0] != p[1]), "one mesh per level: {ids:?}");
        let t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let parts = [value(transform, &t), value(tile_id, &tile)];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, parts.as_ptr(), 2, &mut e), PONKO_OK);
        // A terrain material from two layers and a weight map.
        let pixels = [200u8; 2 * 2 * 4 * 2];
        let mut layers = PonkoTextureArray::default();
        assert_eq!(
            ponko_ctx_create_texture_array(
                ctx,
                2,
                2,
                pixels.as_ptr(),
                pixels.len(),
                PONKO_TEXTURE_COLOR,
                &mut layers
            ),
            PONKO_OK
        );
        assert_eq!((layers.width, layers.layers), (2, 2));
        let mut weights = PonkoTextureArray::default();
        assert_eq!(
            ponko_ctx_create_texture_array(ctx, 2, 2, pixels.as_ptr(), 16, PONKO_TEXTURE_DATA, &mut weights),
            PONKO_OK
        );
        let size = PonkoVec2 { x: 16.0, y: 16.0 };
        let desc = ponko_terrain_material_desc(layers, weights, PonkoVec2::default(), size);
        let mut terrain = PonkoMaterial::default();
        assert_eq!(ponko_ctx_create_terrain_material(ctx, &desc, &mut terrain), PONKO_OK);
        // A made-up layer count, half an image, a bad kind, no weight area.
        let forged = PonkoTerrainMaterialDesc {
            layers: PonkoTextureArray { layers: 3, ..layers },
            ..desc
        };
        errors.push(ponko_ctx_create_terrain_material(ctx, &forged, &mut terrain));
        errors.push(ponko_ctx_create_texture_array(
            ctx,
            2,
            2,
            pixels.as_ptr(),
            8,
            PONKO_TEXTURE_COLOR,
            &mut layers,
        ));
        errors.push(ponko_ctx_create_texture_array(
            ctx,
            2,
            2,
            pixels.as_ptr(),
            16,
            7,
            &mut layers,
        ));
        let flat = PonkoTerrainMaterialDesc {
            weights_size: PonkoVec2::default(),
            ..desc
        };
        errors.push(ponko_ctx_create_terrain_material(ctx, &flat, &mut terrain));
        // Not a square grid, no size, and another game's material.
        errors.push(ponko_ctx_create_terrain_tile(
            ctx,
            heights.as_ptr(),
            80,
            16.0,
            ground,
            &mut tile,
        ));
        errors.push(ponko_ctx_create_terrain_tile(
            ctx,
            heights.as_ptr(),
            81,
            0.0,
            ground,
            &mut tile,
        ));
        errors.push(ponko_ctx_create_terrain_tile(
            ctx,
            heights.as_ptr(),
            81,
            16.0,
            PonkoMaterial { id: 99 },
            &mut tile,
        ));
    }
}

#[test]
fn terrain_tiles_are_made_and_spawned_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"terrain".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(ponko_game_run_headless(g, 1, Some(terrain), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 7]);
}

unsafe extern "C" fn effects(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let mut grey = PonkoEffect::default();
        let source = c"fn effect(p: Pixel) -> vec3<f32> { return vec3<f32>(dot(p.color, vec3<f32>(0.3, 0.6, 0.1))); }";
        assert_eq!(
            ponko_ctx_create_effect(ctx, c"grey".as_ptr(), source.as_ptr(), &mut grey),
            PONKO_OK
        );
        let mut values = PonkoMaterial::default();
        assert_eq!(
            ponko_ctx_create_material(ctx, &ponko_material_desc(WHITE), &mut values),
            PONKO_OK
        );
        let list = [
            PonkoScreenEffect {
                effect: grey,
                has_material: false,
                material: PonkoMaterial::default(),
            },
            PonkoScreenEffect {
                effect: grey,
                has_material: true,
                material: values,
            },
        ];
        assert_eq!(ponko_ctx_set_effects(ctx, list.as_ptr(), 2), PONKO_OK);
        let mut back = [PonkoScreenEffect::default(); 4];
        let mut n = 0;
        assert_eq!(ponko_ctx_effects(ctx, back.as_mut_ptr(), 4, &mut n), PONKO_OK);
        assert_eq!((n, back[1]), (2, list[1]));
        // A broken shader, an unknown effect, and another game's material.
        let mut other = PonkoEffect::default();
        errors.push(ponko_ctx_create_effect(
            ctx,
            c"bad".as_ptr(),
            c"fn effect(p: Pixel) -> f32 { return 1.0; }".as_ptr(),
            &mut other,
        ));
        let unknown = [PonkoScreenEffect {
            effect: PonkoEffect { id: 9 },
            ..list[0]
        }];
        errors.push(ponko_ctx_set_effects(ctx, unknown.as_ptr(), 1));
        let foreign = [PonkoScreenEffect {
            material: PonkoMaterial { id: 99 },
            ..list[1]
        }];
        errors.push(ponko_ctx_set_effects(ctx, foreign.as_ptr(), 1));
        // The bad lists changed nothing.
        assert_eq!(ponko_ctx_effects(ctx, back.as_mut_ptr(), 4, &mut n), PONKO_OK);
        assert_eq!(n, 2);
    }
}

unsafe extern "C" fn target_cameras(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let mut mirror = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(ponko_ctx_create_render_target(ctx, 64, 32, &mut mirror), PONKO_OK);
        assert_eq!((mirror.width, mirror.height), (64, 32));
        let mut eye = std::mem::zeroed::<PonkoCamera3D>();
        assert_eq!(ponko_ctx_camera3d(ctx, &mut eye), PONKO_OK);
        eye.position = PonkoVec3 { x: 0.0, y: 4.0, z: 0.0 };
        let mut below = std::mem::zeroed::<PonkoTargetCamera>();
        assert_eq!(
            ponko_ctx_reflection_camera(ctx, &eye, mirror, 1.0, &mut below),
            PONKO_OK
        );
        assert_eq!(below.camera.position.y, -2.0);
        assert!(below.has_clip_plane && below.clip_plane == [0.0, 1.0, 0.0, -1.0]);
        assert_eq!(ponko_ctx_set_target_cameras(ctx, &below, 1), PONKO_OK);
        let mut back = [below; 2];
        let mut n = 0;
        assert_eq!(ponko_ctx_target_cameras(ctx, back.as_mut_ptr(), 2, &mut n), PONKO_OK);
        assert_eq!((n, back[0].target, back[0].clip_plane), (1, mirror, below.clip_plane));
        assert_eq!(back[0].camera.position, below.camera.position);
        // A plain texture, a zero normal, and a zero size.
        let pixel = [255u8; 4];
        let mut plain = mirror;
        assert_eq!(
            ponko_ctx_create_texture(ctx, 1, 1, pixel.as_ptr(), 4, &mut plain),
            PONKO_OK
        );
        let wrong = PonkoTargetCamera { target: plain, ..below };
        errors.push(ponko_ctx_set_target_cameras(ctx, &wrong, 1));
        let flat = PonkoTargetCamera {
            clip_plane: [0.0; 4],
            ..below
        };
        errors.push(ponko_ctx_set_target_cameras(ctx, &flat, 1));
        errors.push(ponko_ctx_create_render_target(ctx, 0, 8, &mut plain));
        assert_eq!(ponko_ctx_target_cameras(ctx, back.as_mut_ptr(), 2, &mut n), PONKO_OK);
        assert_eq!(n, 1, "failed calls change nothing");
        let mut buffer = PonkoGpuBuffer::default();
        assert_eq!(
            ponko_ctx_create_gpu_buffer(ctx, 65, std::ptr::null(), 0, &mut buffer),
            PONKO_OK
        );
        let data = [[1.0, 2.0, 3.0, 4.0]];
        assert_eq!(ponko_ctx_write_gpu_buffer(ctx, buffer, 64, data.as_ptr(), 1), PONKO_OK);
        assert_eq!(
            ponko_ctx_write_gpu_buffer(ctx, buffer, 65, data.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_write_gpu_buffer(ctx, buffer, 0, std::ptr::null(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_create_gpu_buffer(ctx, 0, std::ptr::null(), 0, &mut buffer),
            PONKO_INVALID_ARGUMENT
        );
        // Freeing: once; later calls see an invalid buffer; the slot comes
        // back under a new id, so the freed handle stays invalid.
        let freed = buffer;
        assert_eq!(ponko_ctx_free_gpu_buffer(ctx, freed), PONKO_OK);
        assert_eq!(ponko_ctx_free_gpu_buffer(ctx, freed), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_write_gpu_buffer(ctx, freed, 0, data.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_create_gpu_buffer(ctx, 4, std::ptr::null(), 0, &mut buffer),
            PONKO_OK
        );
        let slot = ponko::__ffi::SLOT_MASK;
        assert_eq!(buffer.id & slot, freed.id & slot);
        assert_ne!(buffer.id, freed.id);
        assert_eq!(
            ponko_ctx_write_gpu_buffer(ctx, freed, 0, data.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        // A compute pass on the render target.
        let mut fade = PonkoCompute::default();
        let source = c"fn compute(c: Cell) -> vec4<f32> { return c.value * 0.9; }";
        assert_eq!(
            ponko_ctx_create_compute(ctx, c"fade".as_ptr(), source.as_ptr(), &mut fade),
            PONKO_OK
        );
        let pass = PonkoComputePass {
            has_buffer: false,
            buffer: Default::default(),
            compute: fade,
            target: mirror,
            has_material: false,
            material: PonkoMaterial::default(),
        };
        assert_eq!(ponko_ctx_set_compute_passes(ctx, &pass, 1), PONKO_OK);
        let mut update = PonkoCompute::default();
        assert_eq!(
            ponko_ctx_create_compute(
                ctx,
                c"update".as_ptr(),
                c"fn update(i: u32) { items[i].x += frame.time.y; }".as_ptr(),
                &mut update
            ),
            PONKO_OK
        );
        let bp = PonkoComputePass {
            has_buffer: true,
            buffer,
            compute: update,
            ..pass
        };
        assert_eq!(ponko_ctx_set_compute_passes(ctx, &bp, 1), PONKO_OK);
        let mut read = bp;
        let mut n = 0;
        assert_eq!(ponko_ctx_compute_passes(ctx, &mut read, 1, &mut n), PONKO_OK);
        assert!(read.has_buffer && read.buffer == buffer && n == 1);
        assert_eq!(ponko_ctx_set_compute_passes(ctx, &pass, 1), PONKO_OK);
        let mut passes = [pass; 2];
        assert_eq!(ponko_ctx_compute_passes(ctx, passes.as_mut_ptr(), 2, &mut n), PONKO_OK);
        assert_eq!((n, passes[0]), (1, pass));
        // A broken shader, a plain texture, an unknown compute pass.
        errors.push(ponko_ctx_create_compute(
            ctx,
            c"bad".as_ptr(),
            c"fn compute(c: Cell) -> f32 { return 1.0; }".as_ptr(),
            &mut fade,
        ));
        errors.push(ponko_ctx_set_compute_passes(
            ctx,
            &PonkoComputePass { target: plain, ..pass },
            1,
        ));
        let unknown = PonkoComputePass {
            compute: PonkoCompute { id: 9 },
            ..pass
        };
        errors.push(ponko_ctx_set_compute_passes(ctx, &unknown, 1));
    }
}

#[test]
fn target_cameras_and_compute_passes_are_set_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"targets".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(target_cameras), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 6]);
}

#[test]
fn screen_effects_are_made_and_listed_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"effects".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(ponko_game_run_headless(g, 1, Some(effects), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 3]);
}

unsafe extern "C" fn materials(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let found = &mut *user.cast::<Vec<(usize, bool)>>();
        let mut house = PonkoMesh::default();
        assert_eq!(
            ponko_ctx_load_mesh(ctx, c"test-ffi-house.glb".as_ptr(), &mut house),
            PONKO_OK
        );
        let mut count = 0;
        assert_eq!(
            ponko_ctx_loaded_materials(ctx, c"walls".as_ptr(), null_mut(), null_mut(), 0, &mut count),
            PONKO_OK
        );
        let mut ms = vec![PonkoMaterial::default(); count];
        let mut ds = vec![ponko_material_desc(WHITE); count];
        assert_eq!(
            ponko_ctx_loaded_materials(
                ctx,
                c"walls".as_ptr(),
                ms.as_mut_ptr(),
                ds.as_mut_ptr(),
                count,
                &mut count
            ),
            PONKO_OK
        );
        // A new shader on the walls, keeping the rest of what they were made from.
        let mut tint = PonkoShader::default();
        let source = c"fn surface(s: Surface) -> Surface { var o = s; o.albedo *= 0.5; return o; }";
        assert_eq!(
            ponko_ctx_create_shader(ctx, c"tint".as_ptr(), source.as_ptr(), &mut tint),
            PONKO_OK
        );
        ds[0].has_shader = true;
        ds[0].shader = tint;
        let updated = ponko_ctx_update_material(ctx, ms[0], &ds[0]) == PONKO_OK;
        found.push((count, updated));
        assert_eq!(
            ponko_ctx_loaded_materials(ctx, c"nothing".as_ptr(), null_mut(), null_mut(), 0, &mut count),
            PONKO_OK
        );
        found.push((count, true));
    }
}

#[test]
fn loaded_materials_are_found_and_changed_from_c() {
    // The engine's test house, copied under this crate's assets/ for the run.
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    let copy = assets.join("test-ffi-house.glb");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/house.glb"),
        &copy,
    )
    .unwrap();
    let mut found: Vec<(usize, bool)> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"materials".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut found).cast();
        assert_eq!(
            ponko_game_run_headless(g, 1, Some(materials), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    let _ = std::fs::remove_file(&copy);
    let _ = std::fs::remove_dir(&assets); // only if empty
    assert_eq!(found, vec![(1, true), (0, true)]);
}

#[test]
fn punctual_light_components_have_rust_layout_and_roundtrip() {
    unsafe {
        let w = ponko_world_new();
        let mut point_id = 0;
        let mut spot_id = 0;
        assert_eq!(ponko_point_light3d_component(w, &mut point_id), PONKO_OK);
        assert_eq!(ponko_spot_light3d_component(w, &mut spot_id), PONKO_OK);
        assert_ne!(point_id, spot_id);
        let point = PonkoPointLight3D {
            color: WHITE,
            intensity: 42.0,
            range: 17.0,
            shadows: false,
            shadow_resolution: 0,
        };
        let spot = PonkoSpotLight3D {
            color: WHITE,
            intensity: 20.0,
            range: 11.0,
            inner_angle: 0.2,
            outer_angle: 0.6,
            shadows: true,
            shadow_resolution: 256,
        };
        let parts = [value(point_id, &point), value(spot_id, &spot)];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, parts.as_ptr(), parts.len(), &mut e), PONKO_OK);
        let mut got = point;
        got.intensity = 0.0;
        assert_eq!(
            ponko_get(
                w,
                e,
                point_id,
                (&mut got as *mut PonkoPointLight3D).cast(),
                size_of_val(&got)
            ),
            PONKO_OK
        );
        assert_eq!(got, point);
        let mut beam = spot;
        beam.range = 0.0;
        assert_eq!(
            ponko_get(
                w,
                e,
                spot_id,
                (&mut beam as *mut PonkoSpotLight3D).cast(),
                size_of_val(&beam)
            ),
            PONKO_OK
        );
        assert_eq!(beam, spot);
        assert_eq!(
            ponko_point_light3d_component(null_mut(), &mut point_id),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_spot_light3d_component(w, null_mut()), PONKO_INVALID_ARGUMENT);
        ponko_world_free(w);
    }
}

unsafe extern "C" fn lod(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let w = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        let mut white = PonkoMaterial::default();
        assert_eq!(ponko_ctx_default_material(ctx, &mut white), PONKO_OK);
        let cube = ponko_mesh_data_cube(1.0);
        let mut mesh = PonkoMesh::default();
        assert_eq!(ponko_ctx_create_mesh(ctx, cube, white, &mut mesh), PONKO_OK);
        ponko_mesh_data_free(cube);
        let t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let mut e = 0;
        assert_eq!(ponko_spawn(w, &value(transform, &t), 1, &mut e), PONKO_OK);
        let mut bare = 0;
        assert_eq!(ponko_spawn(w, null(), 0, &mut bare), PONKO_OK);
        let renderers = [ponko_lod_renderer(mesh)];
        let levels = [
            PonkoLodLevel {
                screen_height: 0.5,
                fade_width: 0.0,
                renderers: renderers.as_ptr(),
                renderer_count: 1,
                custom_shadow: false,
                shadow_casters: null(),
                shadow_caster_count: 0,
            },
            PonkoLodLevel {
                screen_height: 0.1,
                fade_width: 0.5,
                renderers: renderers.as_ptr(),
                renderer_count: 1,
                custom_shadow: true,
                shadow_casters: null(),
                shadow_caster_count: 0,
            },
        ];
        assert_eq!(
            ponko_ctx_set_lod_group(ctx, e, 2.0, v3(0.0, 0.0, 0.0), levels.as_ptr(), 2),
            PONKO_OK
        );
        // A group with no transform has no pick.
        assert_eq!(
            ponko_ctx_set_lod_group(ctx, bare, 2.0, v3(0.0, 0.0, 0.0), levels.as_ptr(), 2),
            PONKO_OK
        );
        let mut pick = PonkoLodPick::default();
        errors.push(ponko_ctx_lod_pick(ctx, bare, &mut pick));
        let mut camera = std::mem::zeroed();
        assert_eq!(ponko_ctx_camera3d(ctx, &mut camera), PONKO_OK);
        let mut at = |d: f32| {
            camera.position = v3(0.0, 0.0, d);
            assert_eq!(ponko_ctx_set_camera3d(ctx, &camera), PONKO_OK);
            assert_eq!(ponko_ctx_camera3d_look_at(ctx, v3(0.0, 0.0, 0.0)), PONKO_OK);
            let mut pick = PonkoLodPick::default();
            assert_eq!(ponko_ctx_lod_pick(ctx, e, &mut pick), PONKO_OK);
            pick
        };
        assert_eq!((at(1.0).kind, at(1.0).level), (1, 0));
        assert_eq!((at(5.0).kind, at(5.0).level), (1, 1));
        assert_eq!(at(100.0).kind, 0);
        let mut settings = PonkoLodSettings {
            bias: 0.0,
            crossfade: false,
        };
        assert_eq!(ponko_ctx_lod_settings(ctx, &mut settings), PONKO_OK);
        assert_eq!((settings.bias, settings.crossfade), (1.0, true));
        settings.bias = -1.0;
        errors.push(ponko_ctx_set_lod_settings(ctx, settings));
        // Stale entities and null lists.
        errors.push(ponko_ctx_set_lod_group(
            ctx,
            u64::MAX,
            1.0,
            v3(0.0, 0.0, 0.0),
            null(),
            0,
        ));
        errors.push(ponko_ctx_set_lod_group(ctx, e, 1.0, v3(0.0, 0.0, 0.0), null(), 1));
        let bad = [PonkoLodLevel {
            renderers: null(),
            renderer_count: 2,
            ..levels[0]
        }];
        errors.push(ponko_ctx_set_lod_group(ctx, e, 1.0, v3(0.0, 0.0, 0.0), bad.as_ptr(), 1));
    }
}

#[test]
fn lod_groups_are_set_and_picked_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"lod".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(ponko_game_run_headless(g, 0, Some(lod), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 5]);
}

unsafe extern "C" fn overrides(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let errors = &mut *user.cast::<Vec<PonkoStatus>>();
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut model) = (0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut transform), PONKO_OK);
        assert_eq!(ponko_model_component(w, &mut model), PONKO_OK);
        let desc = ponko_material_desc(WHITE);
        let (mut gold, mut wood) = (PonkoMaterial::default(), PonkoMaterial::default());
        assert_eq!(ponko_ctx_create_material(ctx, &desc, &mut gold), PONKO_OK);
        assert_eq!(ponko_ctx_create_material(ctx, &desc, &mut wood), PONKO_OK);

        // A mesh with no material of its own.
        let cube = ponko_mesh_data_cube(1.0);
        let mut mesh = PonkoMesh::default();
        assert_eq!(ponko_ctx_create_plain_mesh(ctx, cube, &mut mesh), PONKO_OK);
        let mut live = false;
        assert_eq!(ponko_ctx_is_mesh_live(ctx, mesh, &mut live), PONKO_OK);
        assert!(live);
        errors.push(ponko_ctx_create_plain_mesh(ctx, null(), &mut mesh));
        errors.push(ponko_ctx_create_plain_mesh(ctx, cube, null_mut()));
        ponko_mesh_data_free(cube);

        let t = ponko_transform3d_at(v3(0.0, 0.0, 0.0));
        let m = ponko_model(mesh);
        let parts = [value(transform, &t), value(model, &m)];
        let mut e = 0;
        assert_eq!(ponko_spawn(w, parts.as_ptr(), 2, &mut e), PONKO_OK);

        // The whole-model material: set, read, taken off.
        let (mut found, mut has) = (PonkoMaterial::default(), true);
        assert_eq!(ponko_ctx_model_material(ctx, e, &mut found, &mut has), PONKO_OK);
        assert!(!has);
        assert_eq!(ponko_ctx_set_model_material(ctx, e, gold, true), PONKO_OK);
        assert_eq!(ponko_ctx_model_material(ctx, e, &mut found, &mut has), PONKO_OK);
        assert!(has && found.id == gold.id);
        assert_eq!(ponko_ctx_model_material(ctx, e, null_mut(), &mut has), PONKO_OK);
        assert!(has);
        assert_eq!(ponko_ctx_set_model_material(ctx, e, gold, false), PONKO_OK);
        assert_eq!(ponko_ctx_model_material(ctx, e, &mut found, &mut has), PONKO_OK);
        assert!(!has);
        // Taking it off needs no live material.
        assert_eq!(
            ponko_ctx_set_model_material(ctx, e, PonkoMaterial { id: 99 }, false),
            PONKO_OK
        );
        errors.push(ponko_ctx_set_model_material(ctx, e, PonkoMaterial { id: 99 }, true));
        errors.push(ponko_ctx_set_model_material(ctx, u64::MAX, gold, true));
        errors.push(ponko_ctx_model_material(ctx, u64::MAX, &mut found, &mut has));
        errors.push(ponko_ctx_model_material(ctx, e, &mut found, null_mut()));

        // The per-part list: the last of a part listed twice, sorted by part.
        let numbers = [3u32, 0, 3];
        let materials = [gold, wood, wood];
        assert_eq!(
            ponko_ctx_set_model_part_materials(ctx, e, numbers.as_ptr(), materials.as_ptr(), 3),
            PONKO_OK
        );
        let mut n = 0;
        assert_eq!(
            ponko_ctx_model_part_materials(ctx, e, null_mut(), null_mut(), 0, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 2);
        let (mut got_parts, mut got) = ([9u32; 2], [PonkoMaterial::default(); 2]);
        assert_eq!(
            ponko_ctx_model_part_materials(ctx, e, got_parts.as_mut_ptr(), got.as_mut_ptr(), 2, &mut n),
            PONKO_OK
        );
        assert_eq!((n, got_parts, got[0].id, got[1].id), (2, [0, 3], wood.id, wood.id));
        // A short buffer gets the first ones and the whole count.
        let (mut one_part, mut one) = ([9u32; 1], [PonkoMaterial::default(); 1]);
        assert_eq!(
            ponko_ctx_model_part_materials(ctx, e, one_part.as_mut_ptr(), one.as_mut_ptr(), 1, &mut n),
            PONKO_OK
        );
        assert_eq!((n, one_part), (2, [0]));
        let stale = [gold, PonkoMaterial { id: 99 }];
        errors.push(ponko_ctx_set_model_part_materials(
            ctx,
            e,
            numbers.as_ptr(),
            stale.as_ptr(),
            2,
        ));
        errors.push(ponko_ctx_set_model_part_materials(
            ctx,
            e,
            null(),
            materials.as_ptr(),
            2,
        ));
        errors.push(ponko_ctx_set_model_part_materials(ctx, e, numbers.as_ptr(), null(), 2));
        errors.push(ponko_ctx_set_model_part_materials(
            ctx,
            u64::MAX,
            numbers.as_ptr(),
            materials.as_ptr(),
            2,
        ));
        errors.push(ponko_ctx_model_part_materials(
            ctx,
            u64::MAX,
            null_mut(),
            null_mut(),
            0,
            &mut n,
        ));
        errors.push(ponko_ctx_model_part_materials(
            ctx,
            e,
            null_mut(),
            null_mut(),
            0,
            null_mut(),
        ));
        // The failed calls changed nothing; an empty list takes it off.
        assert_eq!(
            ponko_ctx_model_part_materials(ctx, e, null_mut(), null_mut(), 0, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 2);
        assert_eq!(ponko_ctx_set_model_part_materials(ctx, e, null(), null(), 0), PONKO_OK);
        assert_eq!(
            ponko_ctx_model_part_materials(ctx, e, null_mut(), null_mut(), 0, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 0);
        // Left on for the frames after setup: both kinds draw without error.
        assert_eq!(ponko_ctx_set_model_material(ctx, e, gold, true), PONKO_OK);
        assert_eq!(
            ponko_ctx_set_model_part_materials(ctx, e, numbers.as_ptr(), materials.as_ptr(), 1),
            PONKO_OK
        );
    }
}

#[test]
fn model_material_overrides_are_set_read_and_cleared_from_c() {
    let mut errors: Vec<PonkoStatus> = Vec::new();
    unsafe {
        let g = ponko_game_new(c"overrides".as_ptr(), 64, 64);
        let mut world = null_mut();
        let user = (&raw mut errors).cast();
        assert_eq!(
            ponko_game_run_headless(g, 2, Some(overrides), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(errors, vec![PONKO_INVALID_ARGUMENT; 12]);
}
