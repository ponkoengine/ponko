//! Navmesh reads inside C loops: a `PonkoNavView` taken before
//! `ponko_query` answers `ponko_nav_view_*` calls inside its callback and
//! inside `ponko_query_par`, where the ctx's own navmesh calls are refused,
//! and `ponko_nav_view_sample_positions` snaps a whole array in one call.
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use PonkoStatus::*;
use ponko_ffi::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

/// What one visit saw.
#[derive(Debug, PartialEq)]
struct Visit {
    sampled: (PonkoStatus, bool, f32),
    path: (PonkoStatus, usize),
    ctx_call: PonkoStatus,
}

struct Scene {
    marker: u32,
    view: PonkoNavView,
    ctx: usize,
    visits: Mutex<Vec<Visit>>,
}

unsafe extern "C" fn visit(chunk: *const PonkoChunk, user: *mut c_void) {
    unsafe {
        let scene = &*user.cast::<Scene>();
        for _ in 0..(*chunk).count {
            let (mut p, mut found) = (v3(0.0, 0.0, 0.0), false);
            let s = ponko_nav_view_sample_position(&scene.view, v3(1.0, 1.5, 1.0), 3.0, &mut p, &mut found);
            let mut corners = [v3(0.0, 0.0, 0.0); 8];
            let mut n = 0;
            let f = ponko_nav_view_find_path(
                &scene.view,
                v3(-5.0, 0.0, -5.0),
                v3(5.0, 0.0, 5.0),
                corners.as_mut_ptr(),
                8,
                &mut n,
            );
            let ctx_call = ponko_ctx_navmesh_sample_position(
                scene.ctx as *mut PonkoCtx,
                v3(1.0, 1.5, 1.0),
                3.0,
                &mut p,
                &mut found,
            );
            scene.visits.lock().unwrap().push(Visit {
                sampled: (s, found, p.y),
                path: (f, n),
                ctx_call,
            });
        }
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let scene = &mut *user.cast::<Scene>();
        let mut none = PonkoNavView { navmesh: null() };
        assert_eq!(ponko_ctx_nav_view(ctx, &mut none), PONKO_NOT_FOUND, "nothing baked yet");
        let sources = ponko_nav_sources_new();
        let floor = ponko_mesh_data_plane(20.0);
        assert_eq!(
            ponko_nav_sources_add_mesh(sources, floor, ponko_transform3d_at(v3(0.0, 0.0, 0.0))),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_bake_navmesh(ctx, sources, null()), PONKO_OK);
        ponko_mesh_data_free(floor);
        ponko_nav_sources_free(sources);
        let w = ponko_ctx_world(ctx);
        assert_eq!(
            ponko_component_register(w, c"walker".as_ptr(), 4, 4, &mut scene.marker),
            PONKO_OK
        );
        let one = 1u32;
        let marked = PonkoValue {
            component: scene.marker,
            data: (&raw const one).cast(),
            size: 4,
        };
        let mut e = 0;
        for _ in 0..3 {
            assert_eq!(ponko_spawn(w, &marked, 1, &mut e), PONKO_OK);
        }

        // The loops: one view, three visits each.
        assert_eq!(ponko_ctx_nav_view(ctx, &mut scene.view), PONKO_OK);
        scene.ctx = ctx as usize;
        let ids = [scene.marker];
        assert_eq!(ponko_query(w, ids.as_ptr(), 1, Some(visit), user), PONKO_OK);
        assert_eq!(ponko_query_par(w, ids.as_ptr(), 1, Some(visit), user), PONKO_OK);

        // A whole array at once, in place: two on the floor, one far off it.
        let mut points = [v3(1.0, 1.0, 1.0), v3(-3.0, 0.5, 2.0), v3(100.0, 0.0, 0.0)];
        let mut found = [false; 3];
        let p = points.as_mut_ptr();
        assert_eq!(
            ponko_nav_view_sample_positions(&scene.view, p, 3, 2.0, p, found.as_mut_ptr()),
            PONKO_OK
        );
        assert_eq!(found, [true, true, false]);
        assert!(points[0].y.abs() < 0.3 && points[1].y.abs() < 0.3, "{points:?}");
        assert_eq!(points[2].x, 100.0, "a point not found stays as it was");
        assert_eq!(
            ponko_nav_view_sample_positions(&scene.view, null(), 1, 2.0, p, found.as_mut_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_nav_view_sample_positions(null(), null(), 0, 2.0, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT,
            "no view"
        );
    }
}

#[test]
fn a_nav_view_answers_inside_query_callbacks_where_ctx_calls_are_busy() {
    let mut scene = Scene {
        marker: 0,
        view: PonkoNavView { navmesh: null() },
        ctx: 0,
        visits: Mutex::default(),
    };
    unsafe {
        let game = ponko_game_new(c"nav-view".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), (&raw mut scene).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    let visits = scene.visits.into_inner().unwrap();
    assert_eq!(visits.len(), 6, "three in each loop");
    for v in &visits {
        assert_eq!(v.sampled.0, PONKO_OK);
        assert!(v.sampled.1 && v.sampled.2.abs() < 0.3, "snapped to the floor: {v:?}");
        assert_eq!(v.path.0, PONKO_OK);
        assert!(v.path.1 >= 2, "a path with its two ends: {v:?}");
        assert_eq!(v.ctx_call, PONKO_BUSY, "the ctx's own call is refused in a loop");
    }
}
