//! The navmesh crowd's work counted from C (`ponko_ctx_navmesh_crowd_stats`,
//! Rust `NavMesh::crowd_stats`): one step a tick while an agent avoids
//! others, its agents and work added up, so a test can check the crowd's
//! cost by counting rather than timing.
mod common;
use common::last_error;
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

fn v3(x: f32, y: f32, z: f32) -> PonkoVec3 {
    PonkoVec3 { x, y, z }
}

fn stats_header(version: u32) -> PonkoNavCrowdStats {
    PonkoNavCrowdStats {
        version,
        ..Default::default()
    }
}

/// The stats the late system read on the last frame.
struct Seen {
    agents: usize,
    stats: PonkoNavCrowdStats,
}

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &*user.cast::<Seen>();
        // No navmesh yet: nothing to read.
        let mut stats = stats_header(PONKO_NAV_CROWD_STATS_VERSION);
        assert_eq!(ponko_ctx_navmesh_crowd_stats(ctx, &mut stats), PONKO_NOT_FOUND);
        let sources = ponko_nav_sources_new();
        let floor = ponko_mesh_data_plane(20.0);
        assert_eq!(
            ponko_nav_sources_add_mesh(sources, floor, ponko_transform3d_at(v3(0.0, 0.0, 0.0))),
            PONKO_OK
        );
        ponko_mesh_data_free(floor);
        assert_eq!(
            ponko_ctx_bake_navmesh(ctx, sources, null()),
            PONKO_OK,
            "{}",
            last_error()
        );
        ponko_nav_sources_free(sources);
        // Just baked: nothing done yet.
        assert_eq!(ponko_ctx_navmesh_crowd_stats(ctx, &mut stats), PONKO_OK);
        assert_eq!((stats.steps, stats.agent_steps, stats.work), (0, 0, 0));
        // A header of version 0 or a null output is refused.
        let mut bad = stats_header(0);
        assert_eq!(ponko_ctx_navmesh_crowd_stats(ctx, &mut bad), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_navmesh_crowd_stats(ctx, null_mut()), PONKO_INVALID_ARGUMENT);

        let world = ponko_ctx_world(ctx);
        let mut transform = 0;
        assert_eq!(ponko_transform3d_component(world, &mut transform), PONKO_OK);
        for i in 0..seen.agents {
            let at = ponko_transform3d_at(v3(-5.0, 0.0, i as f32 - 2.0));
            let part = PonkoValue {
                component: transform,
                data: (&raw const at).cast(),
                size: size_of::<PonkoTransform3D>(),
            };
            let mut e = 0;
            assert_eq!(ponko_spawn(world, &part, 1, &mut e), PONKO_OK);
            assert_eq!(ponko_ctx_set_nav_agent(ctx, e, 2.0, 0.25, 8.0), PONKO_OK);
            assert_eq!(ponko_ctx_set_nav_avoidance(ctx, e, true, 1.0, 3.0), PONKO_OK);
            assert_eq!(ponko_ctx_set_nav_destination(ctx, e, &v3(5.0, 0.0, 0.0)), PONKO_OK);
        }
    }
}

unsafe extern "C" fn look(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        assert_eq!(ponko_ctx_navmesh_crowd_stats(ctx, &mut seen.stats), PONKO_OK);
    }
}

fn crowd(agents: usize, ticks: u64) -> PonkoNavCrowdStats {
    let mut seen = Seen {
        agents,
        stats: stats_header(PONKO_NAV_CROWD_STATS_VERSION),
    };
    unsafe {
        let g = ponko_game_new(c"crowd".as_ptr(), 1, 1);
        assert_eq!(ponko_game_hot_reload(g, false), PONKO_OK);
        assert_eq!(ponko_game_fixed_hz(g, 60), PONKO_OK);
        assert_eq!(ponko_game_navmesh3d(g), PONKO_OK);
        let user = (&raw mut seen).cast();
        // After the navigation step, in the same tick.
        assert_eq!(
            ponko_game_sim_system(g, c"look".as_ptr(), 1, Some(look), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, ticks, Some(setup), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    seen.stats
}

#[test]
fn the_crowd_counts_one_step_a_tick_and_every_agent_it_moves() {
    let stats = crowd(4, 10);
    assert_eq!(stats.steps, 10, "one crowd step a 60 Hz tick");
    assert_eq!(stats.agent_steps, 40, "four agents a step");
    assert!(stats.work > 0, "the backend did some work");
}

#[test]
fn a_crowd_of_the_same_density_costs_about_the_same_per_agent() {
    let (small, large) = (crowd(2, 10), crowd(5, 10));
    let per = |s: PonkoNavCrowdStats| s.work as f64 / s.agent_steps as f64;
    // Agents at 1 m spacing in a line: a bounded number of neighbours each.
    assert!(
        per(large) < per(small) * 3.0,
        "work per agent step grew with the crowd: {} vs {}",
        per(large),
        per(small)
    );
}
