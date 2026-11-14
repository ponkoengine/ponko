//! Holding only the 3D physics step from C while a sim system keeps running.
#![cfg(feature = "physics3d")]
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;

#[derive(Default)]
struct Run {
    transform: u32,
    ball: u64,
    /// The ball's height after each tick, and whether physics was held then.
    heights: Vec<(f32, bool)>,
}

fn value<T>(component: u32, v: &T) -> PonkoValue {
    PonkoValue {
        component,
        data: (v as *const T).cast(),
        size: size_of::<T>(),
    }
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &mut *user.cast::<Run>();
        let w = ponko_ctx_world(ctx);
        let (mut body, mut collider) = (0, 0);
        assert_eq!(ponko_transform3d_component(w, &mut run.transform), PONKO_OK);
        assert_eq!(ponko_rigid_body3d_component(w, &mut body), PONKO_OK);
        assert_eq!(ponko_collider3d_component(w, &mut collider), PONKO_OK);
        let (at, dynamic, shape) = (
            ponko_transform3d_at(PonkoVec3 {
                x: 0.0,
                y: 50.0,
                z: 0.0,
            }),
            ponko_rigid_body3d(PonkoBodyKind::PONKO_BODY_DYNAMIC as u32),
            ponko_collider3d_sphere(0.5),
        );
        let ball = [
            value(run.transform, &at),
            value(body, &dynamic),
            value(collider, &shape),
        ];
        assert_eq!(ponko_spawn(w, ball.as_ptr(), 3, &mut run.ball), PONKO_OK);
    }
}

/// A pause menu as a sim system: holds 3D physics on tick 10, lets it go on tick 30.
unsafe extern "C" fn menu(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let run = &mut *user.cast::<Run>();
        match ponko_ctx_sim_tick(ctx) {
            10 => assert_eq!(ponko_ctx_set_physics3d_paused(ctx, true), PONKO_OK),
            30 => assert_eq!(ponko_ctx_set_physics3d_paused(ctx, false), PONKO_OK),
            _ => {}
        }
        let mut held = false;
        assert_eq!(ponko_ctx_is_physics3d_paused(ctx, &mut held), PONKO_OK);
        let mut t = ponko_transform3d_at(PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
        let w = ponko_ctx_world(ctx);
        let size = size_of::<PonkoTransform3D>();
        assert_eq!(
            ponko_get(w, run.ball, run.transform, (&raw mut t).cast(), size),
            PONKO_OK
        );
        run.heights.push((t.position.y, held));
    }
}

#[test]
fn c_games_hold_only_the_3d_physics_step() {
    let mut run = Run::default();
    let user = (&raw mut run).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"pause3d".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_physics3d(
                g,
                PonkoVec3 {
                    x: 0.0,
                    y: -9.81,
                    z: 0.0
                }
            ),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(g, c"menu".as_ptr(), 1, Some(menu), user),
            PONKO_OK
        );
        assert_eq!(ponko_game_run_headless(g, 60, Some(setup), user, &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(run.heights.len(), 60, "the menu ran every tick");
    for (tick, &(_, held)) in run.heights.iter().enumerate() {
        assert_eq!(held, (10..30).contains(&tick), "held on tick {tick}");
    }
    // The menu runs after the step: ticks 11 to 30 have no step.
    let y = |tick: usize| run.heights[tick].0;
    assert!(y(10) < y(0));
    assert!((11..=30).all(|tick| y(tick) == y(10)), "{:?}", run.heights);
    assert!(y(31) < y(30));
}

#[test]
fn holding_3d_physics_needs_a_live_ctx_and_an_output() {
    unsafe {
        assert_eq!(ponko_ctx_set_physics3d_paused(null_mut(), true), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_is_physics3d_paused(null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    }
}
