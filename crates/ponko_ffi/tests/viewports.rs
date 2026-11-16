//! Several cameras through the C ABI: split screens, the views' lists,
//! picking through the camera under a point, and render layers, in
//! headless games.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

type Body = Box<dyn FnOnce(*mut PonkoCtx)>;
struct Slot {
    body: Option<Body>,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

/// Runs `body` once in the setup of an 800 x 400 headless game (a failed
/// assertion inside is rethrown after the run).
fn with_ctx(body: impl FnOnce(*mut PonkoCtx)) {
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
        let game = ponko_game_new(c"viewports".as_ptr(), 800, 400);
        let mut world = null_mut();
        let status = ponko_game_run_headless(game, 0, Some(trampoline), (&raw mut slot).cast(), &mut world);
        if let Some(p) = slot.panic.take() {
            std::panic::resume_unwind(p);
        }
        assert_eq!(status, PONKO_OK);
        ponko_world_free(world);
    }
}

/// A zeroed descriptor with its header set.
macro_rules! desc {
    ($t:ty, $v:expr) => {{
        #[allow(unused_unsafe)]
        let mut d: $t = unsafe { std::mem::zeroed() };
        d.struct_size = size_of::<$t>() as u32;
        d.version = $v;
        d
    }};
}

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

#[test]
fn split_screens_tile_the_window() {
    let halves = [ponko_viewport_split(2, 0), ponko_viewport_split(2, 1)];
    assert_eq!((halves[0].x, halves[0].width), (0.0, 0.5));
    assert_eq!((halves[1].x, halves[1].width), (0.5, 0.5));
    let quarter = ponko_viewport_split(4, 3);
    assert_eq!(
        (quarter.x, quarter.y, quarter.width, quarter.height),
        (0.5, 0.5, 0.5, 0.5)
    );
}

#[test]
fn two_2d_views_split_the_window_and_pick_their_own_world_points() {
    with_ctx(|ctx| unsafe {
        let mut defaults = desc!(PonkoView2D, PONKO_VIEW2D_VERSION);
        assert_eq!(ponko_view2d_defaults(&mut defaults), PONKO_OK);
        assert_eq!(defaults.layers, PONKO_RENDER_LAYERS_ALL);
        assert!(!defaults.has_clear);

        let mut p1 = desc!(PonkoView2D, PONKO_VIEW2D_VERSION);
        assert_eq!(ponko_ctx_camera_view(ctx, &mut p1), PONKO_OK);
        assert_eq!((p1.rect.width, p1.camera.zoom), (1.0, 1.0));
        p1.rect = ponko_viewport_split(2, 0);
        p1.layers = PONKO_RENDER_LAYERS_ALL & !2;
        assert_eq!(ponko_ctx_set_camera_view(ctx, &p1), PONKO_OK);
        let mut p2 = p1;
        p2.rect = ponko_viewport_split(2, 1);
        p2.camera.position = v2(1000.0, 0.0);
        p2.layers = PONKO_RENDER_LAYERS_ALL;
        p2.has_clear = true;
        p2.clear = PonkoColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        };
        assert_eq!(ponko_ctx_set_view2d(ctx, 0, &p2), PONKO_OK, "index == count adds");
        assert_eq!(ponko_ctx_set_view2d(ctx, 5, &p2), PONKO_NOT_FOUND);
        let mut count = 0;
        assert_eq!(ponko_ctx_view2d_count(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 1);
        let mut back = desc!(PonkoView2D, PONKO_VIEW2D_VERSION);
        assert_eq!(ponko_ctx_view2d(ctx, 0, &mut back), PONKO_OK);
        assert_eq!(back, p2);
        assert_eq!(ponko_ctx_camera_view(ctx, &mut back), PONKO_OK);
        assert_eq!((back.rect, back.layers), (p1.rect, p1.layers));

        // At once, the conversions follow each camera's half.
        let mut at = 0;
        assert_eq!(ponko_ctx_camera_at(ctx, v2(410.0, 20.0), &mut at), PONKO_OK);
        assert_eq!(at, 0, "the right half is view 0");
        assert_eq!(ponko_ctx_camera_at(ctx, v2(10.0, 20.0), &mut at), PONKO_OK);
        assert_eq!(at, -1, "the left half is the main camera");
        let mut world = v2(0.0, 0.0);
        assert_eq!(ponko_ctx_screen_to_world_at(ctx, v2(410.0, 20.0), &mut world), PONKO_OK);
        assert_eq!((world.x, world.y), (1010.0, 20.0));
        assert_eq!(ponko_ctx_screen_to_world_at(ctx, v2(10.0, 20.0), &mut world), PONKO_OK);
        assert_eq!((world.x, world.y), (10.0, 20.0));

        assert_eq!(ponko_ctx_set_draw_layers(ctx, 2), PONKO_OK);
        assert_eq!(ponko_ctx_remove_view2d(ctx, 1), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_remove_view2d(ctx, 0), PONKO_OK);
        assert_eq!(ponko_ctx_view2d(ctx, 0, &mut back), PONKO_NOT_FOUND);
    });
}

#[test]
fn view_descriptors_are_checked() {
    with_ctx(|ctx| unsafe {
        let unset: PonkoView2D = std::mem::zeroed();
        assert_eq!(
            ponko_ctx_set_view2d(ctx, 0, &unset),
            PONKO_INVALID_ARGUMENT,
            "version 0"
        );
        let mut newer = desc!(PonkoView2D, PONKO_VIEW2D_VERSION + 1);
        assert_eq!(ponko_ctx_camera_view(ctx, &mut newer), PONKO_INVALID_ARGUMENT);
        let mut v = desc!(PonkoView2D, PONKO_VIEW2D_VERSION);
        ponko_view2d_defaults(&mut v);
        v.camera.zoom = 0.0;
        assert_eq!(ponko_ctx_set_view2d(ctx, 0, &v), PONKO_INVALID_ARGUMENT, "zoom 0");
        v.camera.zoom = 1.0;
        v.rect.width = f32::NAN;
        assert_eq!(
            ponko_ctx_set_camera_view(ctx, &v),
            PONKO_INVALID_ARGUMENT,
            "a NaN rectangle"
        );
        assert_eq!(ponko_ctx_set_view2d(ctx, 0, null()), PONKO_INVALID_ARGUMENT);
        let mut count = 9;
        assert_eq!(ponko_ctx_view2d_count(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 0, "nothing changed");
        let mut v3 = desc!(PonkoView3D, PONKO_VIEW3D_VERSION);
        ponko_view3d_defaults(&mut v3);
        v3.has_post = true;
        v3.post.anti_aliasing = 9;
        assert_eq!(
            ponko_ctx_set_view3d(ctx, 0, &v3),
            PONKO_INVALID_ARGUMENT,
            "a bad post setting"
        );
        v3.has_post = false;
        v3.camera.near = 0.0;
        assert_eq!(ponko_ctx_set_view3d(ctx, 0, &v3), PONKO_INVALID_ARGUMENT, "near 0");
        assert_eq!(ponko_ctx_view3d_count(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 0);
    });
}

#[test]
fn a_3d_view_keeps_its_own_post_and_picks_the_ground_under_its_camera() {
    with_ctx(|ctx| unsafe {
        let mut main = desc!(PonkoView3D, PONKO_VIEW3D_VERSION);
        assert_eq!(ponko_ctx_camera3d_view(ctx, &mut main), PONKO_OK);
        assert!(!main.has_post && !main.has_clear);
        main.rect = ponko_viewport_split(2, 0);
        main.camera.position = PonkoVec3 {
            x: 0.0,
            y: 10.0,
            z: 10.0,
        };
        main.camera.rotation =
            ponko_quat_from_axis_angle(PonkoVec3 { x: 1.0, y: 0.0, z: 0.0 }, -std::f32::consts::FRAC_PI_4);
        assert_eq!(ponko_ctx_set_camera3d_view(ctx, &main), PONKO_OK);

        let mut p2 = desc!(PonkoView3D, PONKO_VIEW3D_VERSION);
        assert_eq!(ponko_view3d_defaults(&mut p2), PONKO_OK);
        p2.camera = main.camera;
        p2.camera.position.x = 50.0;
        p2.rect = ponko_viewport_split(2, 1);
        p2.layers = 2;
        p2.has_post = true;
        assert_eq!(ponko_ctx_post(ctx, &mut p2.post), PONKO_OK);
        p2.post.bloom = 0.0;
        p2.post.anti_aliasing = 1;
        assert_eq!(ponko_ctx_set_view3d(ctx, 0, &p2), PONKO_OK);
        let mut back = desc!(PonkoView3D, PONKO_VIEW3D_VERSION);
        assert_eq!(ponko_ctx_view3d(ctx, 0, &mut back), PONKO_OK);
        assert!(back.has_post && back.post.bloom == 0.0 && back.post.anti_aliasing == 1);
        assert_eq!((back.layers, back.rect), (2, p2.rect));
        let mut count = 0;
        assert_eq!(ponko_ctx_view3d_count(ctx, &mut count), PONKO_OK);
        assert_eq!(count, 1);

        let mut at = 7;
        assert_eq!(ponko_ctx_camera3d_at(ctx, v2(600.0, 200.0), &mut at), PONKO_OK);
        assert_eq!(at, 0);
        let (mut found, mut ground) = (false, PonkoVec3::default());
        assert_eq!(
            ponko_ctx_screen_to_ground_at(ctx, v2(600.0, 200.0), 0.0, &mut found, &mut ground),
            PONKO_OK
        );
        assert!(found);
        assert!((ground.x - 50.0).abs() < 1e-2 && ground.z.abs() < 1e-2, "{ground:?}");
        assert_eq!(
            ponko_ctx_screen_to_ground_at(ctx, v2(200.0, 200.0), 0.0, &mut found, &mut ground),
            PONKO_OK
        );
        assert!(found && ground.x.abs() < 1e-2, "{ground:?}");

        assert_eq!(ponko_ctx_remove_view3d(ctx, 3), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_remove_view3d(ctx, 0), PONKO_OK);
        assert_eq!(ponko_ctx_view3d(ctx, 0, &mut back), PONKO_NOT_FOUND);
    });
}

#[test]
fn entities_go_on_render_layers_and_back_to_the_default() {
    with_ctx(|ctx| unsafe {
        let w = ponko_ctx_world(ctx);
        let mut e = 0;
        assert_eq!(ponko_spawn(w, null(), 0, &mut e), PONKO_OK);
        let mut layers = 0;
        assert_eq!(ponko_world_render_layers(w, e, &mut layers), PONKO_OK);
        assert_eq!(layers, PONKO_RENDER_LAYERS_DEFAULT);
        assert_eq!(ponko_world_set_render_layers(w, e, 0b110), PONKO_OK);
        assert_eq!(ponko_world_render_layers(w, e, &mut layers), PONKO_OK);
        assert_eq!(layers, 0b110);
        assert_eq!(
            ponko_world_set_render_layers(w, e, PONKO_RENDER_LAYERS_DEFAULT),
            PONKO_OK
        );
        assert_eq!(ponko_world_render_layers(w, e, &mut layers), PONKO_OK);
        assert_eq!(layers, PONKO_RENDER_LAYERS_DEFAULT);
        assert_eq!(ponko_world_render_layers(w, 42, &mut layers), PONKO_NOT_FOUND);
    });
}
