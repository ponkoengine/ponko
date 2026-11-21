//! Versioned descriptors of the 3D picture (STABILITY.md, "Versioned
//! descriptors"): `PonkoPostEx` with split toning, depth of field and the
//! sample count, `Post::lerp` and post volumes through it, and the header
//! rules for the particle and camera descriptors. What the particle and
//! camera descriptors do inside the engine is checked in
//! `src/descriptors_render.rs`, which can look at the Rust context.
mod common;

use PonkoStatus::*;
use common::{last_error, run_ctx};
use ponko_ffi::*;
use std::ptr::{null, null_mut};

/// A descriptor with its header set for `version`.
fn header<T>(version: u32) -> T {
    // SAFETY: every descriptor is plain data, all-zero valid.
    let mut d: T = unsafe { std::mem::zeroed() };
    let words = (&raw mut d).cast::<u32>();
    // SAFETY: the first two words of a descriptor are its header.
    unsafe {
        words.write(size_of::<T>() as u32);
        words.add(1).write(version);
    }
    d
}

fn post_ex() -> PonkoPostEx {
    let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION);
    // SAFETY: a descriptor with its header set.
    assert_eq!(unsafe { ponko_post_ex_defaults(&mut p) }, PONKO_OK, "{}", last_error());
    p
}

#[test]
fn the_new_descriptors_refuse_a_bad_header_and_leave_the_caller_alone() {
    unsafe {
        let mut p: PonkoPostEx = std::mem::zeroed();
        assert_eq!(ponko_post_ex_defaults(&mut p), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("version is 0"), "{}", last_error());
        assert_eq!(p.msaa, 0, "nothing written");
        let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION + 1);
        assert_eq!(ponko_post_ex_defaults(&mut p), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("newer"), "{}", last_error());
        let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        p.struct_size = size_of::<PonkoPost>() as u32;
        assert_eq!(ponko_post_ex_defaults(&mut p), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("struct_size"), "{}", last_error());
        assert_eq!(ponko_post_ex_defaults(null_mut()), PONKO_INVALID_ARGUMENT);

        let mut s: PonkoParticleSettings3DEx = std::mem::zeroed();
        assert_eq!(ponko_particle_settings3d_ex_defaults(&mut s), PONKO_INVALID_ARGUMENT);
        let mut s: PonkoParticleSettings3DEx = header(PONKO_PARTICLE_SETTINGS3D_EX_VERSION + 1);
        assert_eq!(ponko_particle_settings3d_ex_defaults(&mut s), PONKO_INVALID_ARGUMENT);
        assert_eq!(s.softness, 0.0);

        let camera = PonkoCamera3D {
            position: PonkoVec3::default(),
            rotation: ponko_quat_identity(),
            fov_y: 1.0,
            near: 0.1,
            far: 10.0,
        };
        let mut c: PonkoCamera3DEx = std::mem::zeroed();
        assert_eq!(ponko_camera3d_ex_from(&camera, &mut c), PONKO_INVALID_ARGUMENT);
        let mut c: PonkoCamera3DEx = header(PONKO_CAMERA3D_EX_VERSION);
        assert_eq!(ponko_camera3d_ex_from(null(), &mut c), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_camera3d_ex_from(&camera, null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_camera3d_ex_from(&camera, &mut c), PONKO_OK);
        assert_eq!(c.camera, camera);

        let mut t: PonkoTargetCameraEx = header(PONKO_TARGET_CAMERA_EX_VERSION + 1);
        let abi2 = PonkoTargetCamera {
            camera,
            target: PonkoTexture {
                id: 0,
                width: 1,
                height: 1,
            },
            has_clip_plane: false,
            clip_plane: [0.0; 4],
        };
        assert_eq!(ponko_target_camera_ex_from(&abi2, &mut t), PONKO_INVALID_ARGUMENT);
    }
}

#[test]
fn an_older_callers_post_descriptor_leaves_the_newer_fields_alone() {
    // A caller built against a header with only the ABI 2 post (struct_size
    // short of version 1) is refused; a caller built against a later header
    // has a longer struct: the library ignores the tail.
    #[repr(C)]
    struct Newer {
        known: PonkoPostEx,
        unknown: [u8; 32],
    }
    unsafe {
        let mut n = Newer {
            known: header(PONKO_POST_EX_VERSION),
            unknown: [7; 32],
        };
        n.known.struct_size = size_of::<Newer>() as u32;
        assert_eq!(ponko_post_ex_defaults(&mut n.known), PONKO_OK);
        assert_eq!(n.known.msaa, 4);
        assert_eq!(n.unknown, [7; 32], "a field the library doesn't know is left alone");
    }
}

#[test]
fn split_toning_depth_of_field_and_the_sample_count_reach_the_engine_and_come_back() {
    run_ctx(|ctx| unsafe {
        let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut p), PONKO_OK);
        assert_eq!(p, post_ex(), "a new game starts with the defaults");
        let teal = PonkoColor {
            r: 0.35,
            g: 0.5,
            b: 0.6,
            a: 1.0,
        };
        p.split_toning_shadows = teal;
        p.split_toning_balance = 0.25;
        p.dof_enabled = true;
        p.dof_focus_distance = 4.0;
        p.dof_focus_range = 1.0;
        p.dof_blur = 0.03;
        p.msaa = 2;
        p.post.exposure = 0.5;
        assert_eq!(ponko_ctx_set_post_ex(ctx, &p), PONKO_OK, "{}", last_error());
        let mut back: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut back), PONKO_OK);
        assert_eq!(back, p);

        // The ABI 2 calls read and change the same post, and leave what
        // PonkoPost can't hold as it is.
        let mut old = std::mem::zeroed::<PonkoPost>();
        assert_eq!(ponko_ctx_post(ctx, &mut old), PONKO_OK);
        assert_eq!(old.exposure, 0.5);
        old.bloom = 0.4;
        assert_eq!(ponko_ctx_set_post(ctx, &old), PONKO_OK);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut back), PONKO_OK);
        assert_eq!(back.post.bloom, 0.4);
        assert_eq!(
            (back.split_toning_shadows, back.dof_enabled, back.msaa),
            (teal, true, 2),
            "ponko_ctx_set_post keeps split toning, depth of field and the sample count"
        );
    });
}

/// Version 2 adds thin objects in reflections (`Post::ssr_thin_objects`):
/// they reach the engine and come back; a version 1 caller neither reads nor
/// changes them, and blends with them off, as before version 2.
#[test]
fn thin_objects_reach_the_engine_in_version_2_and_version_1_callers_keep_todays_behaviour() {
    run_ctx(|ctx| unsafe {
        let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut p), PONKO_OK);
        assert!(!p.ssr_thin_objects, "off by default");
        p.ssr_thin_objects = true;
        assert_eq!(ponko_ctx_set_post_ex(ctx, &p), PONKO_OK, "{}", last_error());
        let mut back: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut back), PONKO_OK);
        assert!(back.ssr_thin_objects);

        let mut v1: PonkoPostEx = header(1);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut v1), PONKO_OK);
        assert!(!v1.ssr_thin_objects, "a field past version 1 isn't written");
        v1.post.exposure = 0.5;
        assert_eq!(ponko_ctx_set_post_ex(ctx, &v1), PONKO_OK, "{}", last_error());
        assert_eq!(ponko_ctx_post_ex(ctx, &mut back), PONKO_OK);
        assert_eq!(back.post.exposure, 0.5);
        assert!(back.ssr_thin_objects, "nor read: it keeps its value");

        let mut blended = v1;
        blended.ssr_thin_objects = true;
        let mut on = back;
        on.version = 1;
        assert_eq!(ponko_post_ex_lerp(&v1, &on, 1.0, &mut blended), PONKO_OK);
        let mut from_v2: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(ponko_post_ex_lerp(&back, &back, 1.0, &mut from_v2), PONKO_OK);
        assert!(blended.ssr_thin_objects, "the output's field past version 1 is left alone");
        assert!(from_v2.ssr_thin_objects, "version 2 blends it");
    });
}

/// `PonkoGraphicsQuality` (ABI 2) has no field for thin objects: they follow
/// the preset the struct is, so ultra set from C is ultra, as in Rust.
#[test]
fn a_quality_preset_from_c_sets_thin_objects_as_the_rust_preset_does() {
    run_ctx(|ctx| unsafe {
        let mut q: PonkoGraphicsQuality = std::mem::zeroed();
        let mut level = 9;
        let mut p: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        for (preset, thin) in [(3, true), (2, false), (0, false), (3, true)] {
            assert_eq!(ponko_graphics_quality_preset(preset, &mut q), PONKO_OK);
            assert_eq!(ponko_graphics_quality_level(&q, &mut level), PONKO_OK);
            assert_eq!(level, preset, "the struct is the preset");
            assert_eq!(ponko_ctx_set_graphics_quality(ctx, &q), PONKO_OK, "{}", last_error());
            assert_eq!(ponko_ctx_post_ex(ctx, &mut p), PONKO_OK);
            assert_eq!(p.ssr_thin_objects, thin, "preset {preset}");
            assert_eq!(ponko_ctx_graphics_quality(ctx, &mut q), PONKO_OK);
            assert_eq!(ponko_graphics_quality_level(&q, &mut level), PONKO_OK);
            assert_eq!(level, preset, "and reads back as it");
        }
        // A custom struct leaves them as they are.
        q.shadow_distance = 99.0;
        assert_eq!(ponko_ctx_set_graphics_quality(ctx, &q), PONKO_OK);
        assert_eq!(ponko_ctx_post_ex(ctx, &mut p), PONKO_OK);
        assert!(p.ssr_thin_objects, "kept from ultra");
    });
}

#[test]
fn a_post_descriptor_out_of_range_is_refused_and_changes_nothing() {
    run_ctx(|ctx| unsafe {
        let mut good = post_ex();
        good.post.exposure = 1.0;
        assert_eq!(ponko_ctx_set_post_ex(ctx, &good), PONKO_OK);
        type Change = fn(&mut PonkoPostEx);
        let cases: [(&str, Change); 9] = [
            ("balance above 1", |p| p.split_toning_balance = 1.5),
            ("NaN tint", |p| p.split_toning_highlights.g = f32::NAN),
            ("negative focus distance", |p| p.dof_focus_distance = -1.0),
            ("negative focus range", |p| p.dof_focus_range = -0.5),
            ("blur above 0.05", |p| p.dof_blur = 0.06),
            ("infinite focus", |p| p.dof_focus_distance = f32::INFINITY),
            ("3 samples", |p| p.msaa = 3),
            ("0 samples", |p| p.msaa = 0),
            ("an ABI 2 field", |p| p.post.render_scale = 9.0),
        ];
        for (what, change) in cases {
            let mut bad = good;
            bad.post.exposure = 5.0;
            change(&mut bad);
            assert_eq!(ponko_ctx_set_post_ex(ctx, &bad), PONKO_INVALID_ARGUMENT, "{what}");
            let mut now: PonkoPostEx = header(PONKO_POST_EX_VERSION);
            assert_eq!(ponko_ctx_post_ex(ctx, &mut now), PONKO_OK);
            assert_eq!(now, good, "{what}: nothing changes");
        }
        assert_eq!(ponko_ctx_set_post_ex(ctx, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_post_ex(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn lerp_blends_the_post_settings_like_the_rust_one() {
    unsafe {
        let (mut day, mut night) = (post_ex(), post_ex());
        day.post.bloom = 0.0;
        night.post.exposure = -1.0;
        night.post.saturation = 0.6;
        night.split_toning_shadows = PonkoColor {
            r: 0.3,
            g: 0.5,
            b: 0.7,
            a: 1.0,
        };
        night.dof_enabled = true;
        night.dof_blur = 0.02;
        night.msaa = 2;
        let mut out: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        assert_eq!(
            ponko_post_ex_lerp(&day, &night, 0.25, &mut out),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(out.post.exposure, -0.25);
        assert!((out.post.saturation - 0.9).abs() < 1e-6);
        assert!((out.split_toning_shadows.r - 0.45).abs() < 1e-6, "tints blend");
        assert!((out.dof_blur - 0.005).abs() < 1e-6, "an effect off at one end fades in");
        assert!(out.dof_enabled);
        assert_eq!(out.msaa, 4, "a choice that can't mix switches halfway");
        assert_eq!(ponko_post_ex_lerp(&day, &night, 0.75, &mut out), PONKO_OK);
        assert_eq!(out.msaa, 2);
        // The ends are the inputs; a NaN counts as 0; t is clamped.
        assert_eq!(ponko_post_ex_lerp(&day, &night, 0.0, &mut out), PONKO_OK);
        assert_eq!(out, day);
        assert_eq!(ponko_post_ex_lerp(&day, &night, 7.0, &mut out), PONKO_OK);
        assert_eq!(out, night);
        assert_eq!(ponko_post_ex_lerp(&day, &night, f32::NAN, &mut out), PONKO_OK);
        assert_eq!(out, day);
        // The output may be an input.
        let mut a = day;
        assert_eq!(ponko_post_ex_lerp(&a, &night, 0.5, &mut a), PONKO_OK);
        assert_eq!(a.post.exposure, -0.5);
        // Bad inputs and outputs are refused, with nothing written.
        let mut bad = night;
        bad.dof_blur = 1.0;
        let mut out: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        out.post.exposure = 3.0;
        assert_eq!(ponko_post_ex_lerp(&day, &bad, 0.5, &mut out), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_post_ex_lerp(&bad, &day, 0.5, &mut out), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_post_ex_lerp(null(), &day, 0.5, &mut out), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_post_ex_lerp(&day, null(), 0.5, &mut out), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_post_ex_lerp(&day, &night, 0.5, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let mut unset: PonkoPostEx = std::mem::zeroed();
        assert_eq!(
            ponko_post_ex_lerp(&day, &night, 0.5, &mut unset),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(out.post.exposure, 3.0, "nothing written");
    }
}

#[test]
fn post_volumes_blend_by_where_the_camera_is_and_the_later_volume_wins() {
    unsafe {
        let outdoors = post_ex();
        let mut cave = post_ex();
        cave.post.exposure = 1.0;
        cave.split_toning_balance = 0.5;
        let mut room = post_ex();
        room.post.exposure = -1.0;
        let at = |x: f32, y: f32, z: f32| PonkoVec3 { x, y, z };
        let volumes = [
            PonkoPostVolume {
                min: at(-10.0, -1.0, -40.0),
                max: at(10.0, 5.0, -20.0),
                blend_distance: 4.0,
                post: &cave,
            },
            PonkoPostVolume {
                min: at(-1.0, 0.0, -31.0),
                max: at(1.0, 2.0, -29.0),
                blend_distance: 0.0,
                post: &room,
            },
        ];
        let blend = |camera: PonkoVec3, list: &[PonkoPostVolume]| {
            let mut out: PonkoPostEx = header(PONKO_POST_EX_VERSION);
            let status = ponko_post_blend_volumes(&outdoors, list.as_ptr(), list.len(), camera, &mut out);
            assert_eq!(status, PONKO_OK, "{}", last_error());
            out
        };
        assert_eq!(
            blend(at(5.0, 2.0, -25.0), &volumes).post.exposure,
            1.0,
            "inside the cave"
        );
        assert_eq!(blend(at(0.0, 1.0, 20.0), &volumes).post.exposure, 0.0, "far outside");
        let edge = blend(at(0.0, 2.0, -18.0), &volumes);
        assert!(
            (edge.post.exposure - 0.5).abs() < 1e-6,
            "halfway through the blend distance"
        );
        assert_eq!(
            blend(at(0.0, 1.0, -30.0), &volumes).post.exposure,
            -1.0,
            "the later volume wins"
        );
        assert_eq!(blend(at(0.0, 1.0, -30.0), &volumes[..1]).post.exposure, 1.0);
        assert_eq!(blend(at(0.0, 1.0, -30.0), &[]), outdoors, "no volumes: the base");

        // Refused: a volume with its corners the wrong way round, a negative
        // blend distance, no settings, a bad look, a null list.
        let mut out: PonkoPostEx = header(PONKO_POST_EX_VERSION);
        let (camera, count) = (at(0.0, 0.0, 0.0), 1);
        let mut wrong = volumes[0];
        std::mem::swap(&mut wrong.min, &mut wrong.max);
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, &wrong, count, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        let mut negative = volumes[0];
        negative.blend_distance = -1.0;
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, &negative, count, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        let mut bare = volumes[0];
        bare.post = null();
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, &bare, count, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        let mut broken = cave;
        broken.msaa = 8;
        let mut spoiled = volumes[0];
        spoiled.post = &broken;
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, &spoiled, count, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, null(), 1, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_post_blend_volumes(&outdoors, volumes.as_ptr(), 2, at(f32::NAN, 0.0, 0.0), &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_post_blend_volumes(null(), volumes.as_ptr(), 2, camera, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(out.post.exposure, 0.0, "nothing written");
    }
}
