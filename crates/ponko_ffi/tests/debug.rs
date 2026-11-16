//! Debug drawing from C: every shape lands in the buffer as 2D or 3D lines
//! or labels, drawing turned off records nothing, the built-in views
//! round-trip, and bad input is refused without drawing.
mod common;

use common::run_ctx;
use ponko_ffi::PonkoStatus::*;
use ponko_ffi::*;
use std::ptr::{null, null_mut};

const RED: PonkoColor = PonkoColor {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn v2(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

unsafe fn stats(ctx: *mut PonkoCtx) -> PonkoDebugStats {
    let mut s = PonkoDebugStats::default();
    assert_eq!(unsafe { ponko_ctx_debug_stats(ctx, &mut s) }, PONKO_OK);
    s
}

#[test]
fn every_shape_lands_in_the_buffer_as_2d_or_3d_lines_or_labels() {
    run_ctx(|ctx| unsafe {
        assert_eq!(ponko_ctx_debug_set_enabled(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_debug_clear(ctx), PONKO_OK);
        let style = PonkoDebugStyle {
            width: 3.0,
            on_top: true,
            ..ponko_debug_style_default()
        };
        assert_eq!(
            ponko_ctx_debug_line2d(ctx, v2(0.0, 0.0), v2(1.0, 0.0), RED, &style),
            PONKO_OK
        );
        assert_eq!(stats(ctx).lines_2d, 1);
        assert_eq!(
            ponko_ctx_debug_box2d(ctx, v2(0.0, 0.0), v2(2.0, 2.0), RED, null()),
            PONKO_OK
        );
        assert_eq!(stats(ctx).lines_2d, 5);
        let o = (v2(0.0, 0.0), v2(5.0, 5.0));
        assert_eq!(ponko_ctx_debug_ray2d(ctx, o.0, o.1, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_arrow2d(ctx, o.0, o.1, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_circle2d(ctx, o.0, 3.0, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_capsule2d(ctx, o.0, o.1, 1.0, RED, null()), PONKO_OK);
        let square = [v2(0.0, 0.0), v2(1.0, 0.0), v2(1.0, 1.0)];
        assert_eq!(
            ponko_ctx_debug_polyline2d(ctx, square.as_ptr(), 3, true, RED, null()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_debug_grid2d(ctx, o.0, 1.0, 4, RED, null()), PONKO_OK);
        let screen = PonkoDebugStyle {
            screen: true,
            ..ponko_debug_style_default()
        };
        assert_eq!(
            ponko_ctx_debug_text2d(ctx, v2(8.0, 8.0), c"fps".as_ptr(), RED, &screen),
            PONKO_OK
        );
        let flat = stats(ctx);
        assert!(flat.lines_2d > 5 + 32, "{flat:?}");
        assert_eq!((flat.lines_3d, flat.labels), (0, 1));

        let (a, b) = (v3(0.0, 0.0, 0.0), v3(0.0, 1.0, 0.0));
        let turn = ponko_quat_identity();
        assert_eq!(ponko_ctx_debug_line3d(ctx, a, b, RED, null()), PONKO_OK);
        assert_eq!(
            ponko_ctx_debug_box3d(ctx, a, v3(1.0, 1.0, 1.0), turn, RED, null()),
            PONKO_OK
        );
        assert_eq!(stats(ctx).lines_3d, 13, "a line and a box's 12 edges");
        assert_eq!(ponko_ctx_debug_ray3d(ctx, a, b, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_arrow3d(ctx, a, b, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_sphere3d(ctx, a, 1.0, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_capsule3d(ctx, a, b, 0.5, RED, null()), PONKO_OK);
        let path = [a, b, v3(1.0, 1.0, 0.0)];
        assert_eq!(
            ponko_ctx_debug_polyline3d(ctx, path.as_ptr(), 3, false, RED, null()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_debug_grid3d(ctx, a, 1.0, 10, RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_frustum3d(ctx, null(), RED, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_axes3d(ctx, a, turn, 1.0, null()), PONKO_OK);
        assert_eq!(ponko_ctx_debug_text3d(ctx, b, c"here".as_ptr(), RED, null()), PONKO_OK);
        let all = stats(ctx);
        assert_eq!(all.lines_2d, flat.lines_2d, "3D shapes draw no 2D lines");
        assert!(all.lines_3d > 13 + 3 * 32, "{all:?}");
        assert_eq!(all.labels, 2);

        assert_eq!(ponko_ctx_debug_clear(ctx), PONKO_OK);
        assert_eq!(stats(ctx), PonkoDebugStats::default());
    });
}

#[test]
fn turned_off_drawing_records_nothing_and_the_views_stay_chosen() {
    run_ctx(|ctx| unsafe {
        let view = PonkoDebugView::PONKO_DEBUG_VIEW_COLLIDERS as u32;
        assert_eq!(ponko_ctx_debug_set_enabled(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_debug_show(ctx, view, true), PONKO_OK);
        assert_eq!(
            ponko_ctx_debug_line2d(ctx, v2(0.0, 0.0), v2(1.0, 0.0), RED, null()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_debug_set_enabled(ctx, false), PONKO_OK);
        let mut on = true;
        assert_eq!(ponko_ctx_debug_enabled(ctx, &mut on), PONKO_OK);
        assert!(!on);
        assert_eq!(stats(ctx), PonkoDebugStats::default(), "turning off forgets");
        assert_eq!(
            ponko_ctx_debug_sphere3d(ctx, v3(0.0, 0.0, 0.0), 1.0, RED, null()),
            PONKO_OK
        );
        assert_eq!(stats(ctx), PonkoDebugStats::default());
        let mut showing = false;
        assert_eq!(ponko_ctx_debug_showing(ctx, view, &mut showing), PONKO_OK);
        assert!(showing);
        let lights = PonkoDebugView::PONKO_DEBUG_VIEW_LIGHTS as u32;
        assert_eq!(ponko_ctx_debug_showing(ctx, lights, &mut showing), PONKO_OK);
        assert!(!showing);
    });
}

#[test]
fn bad_arguments_are_refused_and_draw_nothing() {
    unsafe {
        assert_eq!(ponko_ctx_debug_set_enabled(null_mut(), true), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_debug_circle2d(null_mut(), v2(0.0, 0.0), 1.0, RED, null()),
            PONKO_INVALID_ARGUMENT
        );
    }
    run_ctx(|ctx| unsafe {
        assert_eq!(ponko_ctx_debug_set_enabled(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_debug_clear(ctx), PONKO_OK);
        assert_eq!(ponko_ctx_debug_show(ctx, 99, true), PONKO_INVALID_ARGUMENT);
        let mut on = false;
        assert_eq!(ponko_ctx_debug_showing(ctx, 5, &mut on), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_debug_polyline2d(ctx, null(), 2, false, RED, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_debug_polyline3d(ctx, null(), 0, false, RED, null()), PONKO_OK);
        assert_eq!(
            ponko_ctx_debug_text2d(ctx, v2(0.0, 0.0), null(), RED, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_debug_text3d(ctx, v3(0.0, 0.0, 0.0), null(), RED, null()),
            PONKO_INVALID_ARGUMENT
        );
        let no_fov = PonkoCamera3D {
            position: v3(0.0, 0.0, 0.0),
            rotation: ponko_quat_identity(),
            fov_y: 0.0,
            near: 0.1,
            far: 100.0,
        };
        assert_eq!(
            ponko_ctx_debug_frustum3d(ctx, &no_fov, RED, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(stats(ctx), PonkoDebugStats::default());
        let ok = PonkoCamera3D { fov_y: 1.0, ..no_fov };
        assert_eq!(ponko_ctx_debug_frustum3d(ctx, &ok, RED, null()), PONKO_OK);
        assert_eq!(stats(ctx).lines_3d, 12);
    });
}
