//! Camera rigs through the C ABI: the smoothing helpers, the shake, the 2D
//! follow camera and pixel-perfect 2D, the 3D orbit, first- and
//! third-person cameras, and screen/world conversion, in headless games.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

type Body = Box<dyn FnOnce(*mut PonkoCtx)>;
struct Slot {
    body: Option<Body>,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// Runs `body` once in the setup of an 800 x 600 headless game with
/// `configure` applied (a failed assertion inside is rethrown after the run).
fn with_ctx(configure: impl FnOnce(*mut PonkoGame), body: impl FnOnce(*mut PonkoCtx)) {
    unsafe extern "C" fn trampoline(ctx: *mut PonkoCtx, user: *mut c_void) {
        // SAFETY: `user` is the `Slot` below, alive for the run.
        let slot = unsafe { &mut *user.cast::<Slot>() };
        let body = slot.body.take().expect("setup runs once");
        if let Err(p) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| body(ctx))) {
            slot.panic = Some(p);
        }
    }
    let body: Box<dyn FnOnce(*mut PonkoCtx) + '_> = Box::new(body);
    // SAFETY: the box is consumed inside the headless run, before this returns.
    let body: Body = unsafe { std::mem::transmute(body) };
    let mut slot = Slot {
        body: Some(body),
        panic: None,
    };
    unsafe {
        let game = ponko_game_new(c"camera-rigs".as_ptr(), 800, 600);
        configure(game);
        let mut world = null_mut();
        let status = ponko_game_run_headless(game, 1, Some(trampoline), (&raw mut slot).cast(), &mut world);
        if let Some(p) = slot.panic.take() {
            std::panic::resume_unwind(p);
        }
        assert_eq!(status, PONKO_OK);
        ponko_world_free(world);
    }
}

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}
fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}
fn close(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() < eps
}
fn dist3(a: PonkoVec3, b: PonkoVec3) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// A zeroed descriptor with its header set.
macro_rules! desc {
    ($t:ty, $v:expr) => {{
        #[allow(unused_unsafe)] // also used inside unsafe blocks
        let mut d: $t = unsafe { std::mem::zeroed() };
        d.struct_size = size_of::<$t>() as u32;
        d.version = $v;
        d
    }};
}

/// The 2D camera's center.
unsafe fn center(ctx: *mut PonkoCtx) -> PonkoVec2 {
    let mut cam = PonkoCamera {
        position: v2(0.0, 0.0),
        zoom: 1.0,
    };
    assert_eq!(unsafe { ponko_ctx_camera(ctx, &mut cam) }, PONKO_OK);
    v2(cam.position.x + 400.0 / cam.zoom, cam.position.y + 300.0 / cam.zoom)
}

unsafe fn camera3d(ctx: *mut PonkoCtx) -> PonkoCamera3D {
    let mut cam: PonkoCamera3D = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { ponko_ctx_camera3d(ctx, &mut cam) }, PONKO_OK);
    cam
}

#[test]
fn smoothing_helpers_match_their_rust_twins() {
    let mut v = 0.0f32;
    let mut at = 0.0;
    for _ in 0..120 {
        at = unsafe { ponko_smooth_damp(at, 10.0, &mut v, 0.2, 1.0 / 60.0) };
    }
    assert!(close(at, 10.0, 1e-2) && at <= 10.0);
    assert_eq!(unsafe { ponko_smooth_damp(3.0, 9.0, null_mut(), 0.0, 0.1) }, 9.0);
    let mut v2_ = v2(0.0, 0.0);
    let p = unsafe { ponko_smooth_damp_vec2(v2(0.0, 0.0), v2(4.0, -2.0), &mut v2_, 0.1, 5.0) };
    assert!(close(p.x, 4.0, 1e-3) && close(p.y, -2.0, 1e-3));
    let mut v3_ = v3(0.0, 0.0, 0.0);
    let p = unsafe { ponko_smooth_damp_vec3(v3(0.0, 0.0, 0.0), v3(1.0, 2.0, 3.0), &mut v3_, 0.1, 0.05) };
    assert!(p.z > 0.0 && p.z < 3.0 && v3_.z > 0.0, "on its way, with speed");
    assert!(close(ponko_damp(0.0, 10.0, 0.5, 0.5), 5.0, 1e-5));
    assert!(close(
        ponko_damp_vec2(v2(0.0, 0.0), v2(2.0, 4.0), 1.0, 1.0).y,
        2.0,
        1e-5
    ));
    assert!(close(
        ponko_damp_vec3(v3(0.0, 0.0, 0.0), v3(8.0, 0.0, 0.0), 1.0, 2.0).x,
        6.0,
        1e-4
    ));
    let a = ponko_damp_angle(3.0, -3.0, 0.1, 10.0);
    assert!(close(a, -3.0 + std::f32::consts::TAU, 1e-3), "the short way: {a}");
}

#[test]
fn descriptors_with_a_bad_header_are_refused_and_left_alone() {
    let mut s = desc!(PonkoCameraShake, 0);
    s.trauma = 0.5;
    assert_eq!(unsafe { ponko_camera_shake_defaults(&mut s) }, PONKO_INVALID_ARGUMENT);
    assert_eq!(s.trauma, 0.5);
    let mut f = desc!(PonkoFollowCamera2D, PONKO_FOLLOW_CAMERA2D_VERSION + 1);
    assert_eq!(
        unsafe { ponko_follow_camera2d_defaults(&mut f) },
        PONKO_INVALID_ARGUMENT
    );
    let mut o = desc!(PonkoOrbitCamera, PONKO_ORBIT_CAMERA_VERSION);
    o.struct_size = 8;
    assert_eq!(unsafe { ponko_orbit_camera_defaults(&mut o) }, PONKO_INVALID_ARGUMENT);
    assert_eq!(
        unsafe { ponko_pixel_perfect_defaults(null_mut()) },
        PONKO_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { ponko_first_person_camera_look(null_mut(), v2(1.0, 0.0)) },
        PONKO_INVALID_ARGUMENT
    );
}

#[test]
fn trauma_shakes_the_cameras_and_wears_off() {
    let mut s = desc!(PonkoCameraShake, PONKO_CAMERA_SHAKE_VERSION);
    unsafe {
        assert_eq!(ponko_camera_shake_defaults(&mut s), PONKO_OK);
        assert_eq!((s.trauma, s.decay, s.exponent), (0.0, 1.0, 2.0));
        assert_eq!(ponko_camera_shake_add_trauma(&mut s, 0.7), PONKO_OK);
        assert_eq!(ponko_camera_shake_add_trauma(&mut s, 0.7), PONKO_OK);
        assert_eq!(s.trauma, 1.0, "kept within 0 to 1");
        assert_eq!(ponko_camera_shake_update(&mut s, 0.1), PONKO_OK);
        assert!(close(s.trauma, 0.9, 1e-6) && close(s.time, 0.1, 1e-6));
    }
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let before = center(ctx);
            let rotation = camera3d(ctx).rotation;
            assert_eq!(ponko_camera_shake_apply2d(&s, ctx), PONKO_OK);
            assert_eq!(ponko_camera_shake_apply3d(&s, ctx), PONKO_OK);
            let after = center(ctx);
            let moved = ((after.x - before.x).powi(2) + (after.y - before.y).powi(2)).sqrt();
            assert!(moved > 0.0 && moved <= 10.0 * 2f32.sqrt(), "{moved}");
            assert_ne!(camera3d(ctx).rotation, rotation);
        },
    );
}

#[test]
fn the_follow_camera_follows_inside_its_bounds_and_snaps() {
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let mut f = desc!(PonkoFollowCamera2D, PONKO_FOLLOW_CAMERA2D_VERSION);
            assert_eq!(ponko_follow_camera2d_defaults(&mut f), PONKO_OK);
            f.has_bounds = true;
            f.bounds = PonkoRect {
                min: v2(0.0, 0.0),
                max: v2(2000.0, 1000.0),
            };
            assert_eq!(
                ponko_follow_camera2d_update(&mut f, ctx, v2(900.0, 500.0), 1.0 / 60.0),
                PONKO_OK
            );
            assert!(f.started, "the state came back");
            let c = center(ctx);
            assert!(
                close(c.x, 900.0, 1e-3) && close(c.y, 500.0, 1e-3),
                "the first update snaps: {c:?}"
            );
            for _ in 0..120 {
                assert_eq!(
                    ponko_follow_camera2d_update(&mut f, ctx, v2(100.0, 500.0), 1.0 / 60.0),
                    PONKO_OK
                );
            }
            let c = center(ctx);
            assert!(close(c.x, 400.0, 1e-2), "stopped at the left edge: {c:?}");
            assert_eq!(ponko_follow_camera2d_snap(&mut f, ctx, v2(1000.0, 500.0)), PONKO_OK);
            assert!(close(center(ctx).x, 1000.0, 1e-3));
        },
    );
}

#[test]
fn pixel_perfect_sets_a_whole_zoom_and_the_letterbox() {
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let mut p = desc!(PonkoPixelPerfect, PONKO_PIXEL_PERFECT_VERSION);
            assert_eq!(ponko_pixel_perfect_defaults(&mut p), PONKO_OK);
            p.resolution = v2(320.0, 180.0);
            assert_eq!(ponko_pixel_perfect_apply(&p, ctx), PONKO_OK);
            let mut cam = PonkoCamera {
                position: v2(0.0, 0.0),
                zoom: 0.0,
            };
            assert_eq!(ponko_ctx_camera(ctx, &mut cam), PONKO_OK);
            assert_eq!(cam.zoom, 2.0, "800 x 600 fits 320 x 180 twice");
            assert_eq!(cam.position.x, cam.position.x.round());
            let mut r = PonkoRect::default();
            assert_eq!(ponko_pixel_perfect_screen_rect(&p, ctx, &mut r), PONKO_OK);
            assert_eq!((r.min.x, r.min.y, r.max.x, r.max.y), (80.0, 120.0, 720.0, 480.0));
        },
    );
}

#[test]
fn the_orbit_camera_circles_its_target() {
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let mut o = desc!(PonkoOrbitCamera, PONKO_ORBIT_CAMERA_VERSION);
            assert_eq!(ponko_orbit_camera_defaults(&mut o), PONKO_OK);
            o.target = v3(1.0, 0.0, -2.0);
            o.distance = 6.0;
            assert_eq!(ponko_orbit_camera_mouse_controls(&mut o, ctx), PONKO_OK);
            assert_eq!(o.distance, 6.0, "no input headless: nothing changed");
            assert_eq!(ponko_orbit_camera_update(&mut o, ctx, 1.0 / 60.0), PONKO_OK);
            let cam = camera3d(ctx);
            assert!(close(dist3(cam.position, o.target), 6.0, 1e-3));
            assert!(cam.position.y > 0.0, "looking down on it");
            o.distance = 1e9;
            assert_eq!(ponko_orbit_camera_update(&mut o, ctx, 1.0 / 60.0), PONKO_OK);
            assert_eq!(o.distance, o.max_distance, "kept within its limits");
        },
    );
}

#[test]
fn the_first_person_camera_looks_from_the_eye() {
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let mut f = desc!(PonkoFirstPersonCamera, PONKO_FIRST_PERSON_CAMERA_VERSION);
            assert_eq!(ponko_first_person_camera_defaults(&mut f), PONKO_OK);
            assert_eq!(ponko_first_person_camera_look(&mut f, v2(0.0, -200.0)), PONKO_OK);
            assert!(f.pitch > 0.0, "the mouse moved up: look up");
            assert_eq!(
                ponko_first_person_camera_update(&mut f, ctx, v3(0.0, 1.7, 0.0), 0.016),
                PONKO_OK
            );
            let cam = camera3d(ctx);
            assert_eq!((cam.position.x, cam.position.y, cam.position.z), (0.0, 1.7, 0.0));
            let forward = ponko_quat_rotate(cam.rotation, v3(0.0, 0.0, -1.0));
            assert!(forward.y > 0.0);
        },
    );
}

#[test]
fn flat_forward_and_right_follow_the_rigs_yaw_and_match_rust() {
    unsafe {
        let mut f = desc!(PonkoFirstPersonCamera, PONKO_FIRST_PERSON_CAMERA_VERSION);
        assert_eq!(ponko_first_person_camera_defaults(&mut f), PONKO_OK);
        let mut t = desc!(PonkoThirdPersonCamera, PONKO_THIRD_PERSON_CAMERA_VERSION);
        assert_eq!(ponko_third_person_camera_defaults(&mut t), PONKO_OK);
        for yaw in [0.0f32, 0.7, -2.0, 3.1] {
            f.yaw = yaw;
            t.yaw = yaw;
            let mut rust = ponko::d3::FirstPersonCamera::default();
            rust.yaw = yaw;
            let (mut forward, mut right) = (v3(9.0, 9.0, 9.0), v3(9.0, 9.0, 9.0));
            assert_eq!(ponko_first_person_camera_flat_forward(&f, &mut forward), PONKO_OK);
            assert_eq!(ponko_first_person_camera_flat_right(&f, &mut right), PONKO_OK);
            let want = (rust.flat_forward(), rust.flat_right());
            assert!(dist3(forward, v3(want.0.x, want.0.y, want.0.z)) < 1e-6, "{forward:?}");
            assert!(dist3(right, v3(want.1.x, want.1.y, want.1.z)) < 1e-6, "{right:?}");
            assert_eq!(forward.y, 0.0, "level");
            let (mut chase_forward, mut chase_right) = (v3(9.0, 9.0, 9.0), v3(9.0, 9.0, 9.0));
            assert_eq!(ponko_third_person_camera_flat_forward(&t, &mut chase_forward), PONKO_OK);
            assert_eq!(ponko_third_person_camera_flat_right(&t, &mut chase_right), PONKO_OK);
            assert!(dist3(chase_forward, forward) < 1e-6 && dist3(chase_right, right) < 1e-6);
        }
        // A bad descriptor or no output: refused, nothing written.
        let mut out = v3(9.0, 9.0, 9.0);
        assert_eq!(
            ponko_first_person_camera_flat_forward(null(), &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_third_person_camera_flat_right(&t, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let mut bad = t;
        bad.version = 0;
        assert_eq!(
            ponko_third_person_camera_flat_forward(&bad, &mut out),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(out.x, 9.0);
    }
}

#[test]
fn the_third_person_camera_stays_behind_the_target_with_or_without_physics() {
    with_ctx(
        |game| unsafe {
            assert_eq!(ponko_game_physics3d(game, v3(0.0, -9.81, 0.0)), PONKO_OK);
        },
        |ctx| unsafe {
            let mut t = desc!(PonkoThirdPersonCamera, PONKO_THIRD_PERSON_CAMERA_VERSION);
            assert_eq!(ponko_third_person_camera_defaults(&mut t), PONKO_OK);
            assert_eq!(ponko_third_person_camera_look(&mut t, v2(0.0, 1e6)), PONKO_OK);
            assert_eq!(t.pitch, t.min_pitch);
            t.pitch = 0.0;
            assert_eq!(
                ponko_third_person_camera_update(&mut t, ctx, v3(0.0, 0.0, 0.0), 0.016),
                PONKO_OK
            );
            let cam = camera3d(ctx);
            assert!(
                close(cam.position.z, 5.0, 1e-3) && close(cam.position.y, 1.5, 1e-3),
                "{cam:?}"
            );
            // Nothing in the way: the same place through the physics.
            let mut filter = desc!(PonkoQueryFilterEx, PONKO_QUERY_FILTER_EX_VERSION);
            assert_eq!(ponko_query_filter_ex_defaults(&mut filter), PONKO_OK);
            assert_eq!(
                ponko_third_person_camera_update_physics(&mut t, ctx, v3(0.0, 0.0, 0.0), &filter, 0.016),
                PONKO_OK
            );
            assert!(close(camera3d(ctx).position.z, 5.0, 1e-3));
            assert_eq!(
                ponko_third_person_camera_update_physics(&mut t, ctx, v3(0.0, 0.0, 0.0), null(), 0.016),
                PONKO_OK,
                "a null filter is the defaults"
            );
            let mut bad = filter;
            bad.version = 0;
            assert_eq!(
                ponko_third_person_camera_update_physics(&mut t, ctx, v3(0.0, 0.0, 0.0), &bad, 0.016),
                PONKO_INVALID_ARGUMENT
            );
        },
    );
}

#[test]
fn screen_and_world_points_convert_both_ways_in_2d_and_3d() {
    with_ctx(
        |_| {},
        |ctx| unsafe {
            let cam = PonkoCamera {
                position: v2(100.0, 50.0),
                zoom: 2.0,
            };
            assert_eq!(ponko_ctx_set_camera(ctx, &cam), PONKO_OK);
            let mut s = v2(0.0, 0.0);
            assert_eq!(ponko_ctx_world_to_screen(ctx, v2(110.0, 60.0), &mut s), PONKO_OK);
            assert_eq!((s.x, s.y), (20.0, 20.0));
            let mut w = v2(0.0, 0.0);
            assert_eq!(ponko_ctx_screen_to_world(ctx, s, &mut w), PONKO_OK);
            assert_eq!((w.x, w.y), (110.0, 60.0));

            assert_eq!(ponko_ctx_camera3d_look_at(ctx, v3(0.0, 0.0, 0.0)), PONKO_OK);
            let mut ground = v3(9.0, 9.0, 9.0);
            let mut found = false;
            assert_eq!(
                ponko_ctx_screen_to_ground(ctx, v2(400.0, 300.0), 0.0, &mut found, &mut ground),
                PONKO_OK
            );
            assert!(found && dist3(ground, v3(0.0, 0.0, 0.0)) < 1e-3, "{ground:?}");
            let mut visible = false;
            assert_eq!(ponko_ctx_world_to_screen3d(ctx, ground, &mut visible, &mut s), PONKO_OK);
            assert!(visible && close(s.x, 400.0, 1e-2) && close(s.y, 300.0, 1e-2));
            let mut p = v3(0.0, 0.0, 0.0);
            assert_eq!(
                ponko_ctx_screen_to_world3d(ctx, v2(100.0, 500.0), 4.0, &mut p),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_world_to_screen3d(ctx, p, &mut visible, &mut s), PONKO_OK);
            assert!(visible && close(s.x, 100.0, 1e-2) && close(s.y, 500.0, 1e-2));
            // Behind the camera, and a plane the ray never meets.
            let behind = camera3d(ctx).position;
            assert_eq!(
                ponko_ctx_world_to_screen3d(ctx, v3(behind.x, behind.y + 1.0, behind.z + 10.0), &mut visible, &mut s),
                PONKO_OK
            );
            assert!(!visible);
            assert_eq!(
                ponko_ctx_screen_to_ground(ctx, v2(400.0, 300.0), 100.0, &mut found, &mut ground),
                PONKO_OK
            );
            assert!(!found);
            assert_eq!(
                ponko_ctx_screen_to_ground(ctx, v2(400.0, 300.0), 0.0, null_mut(), &mut ground),
                PONKO_INVALID_ARGUMENT
            );
        },
    );
}
