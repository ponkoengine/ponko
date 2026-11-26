//! Game setup calls that only matter once a window opens (frame and late
//! systems, camera controls, MSAA) and Steam calls, as far
//! as they can run in a headless build without a Steam client. What opens a
//! window is on the allowlist of `scripts/check-ffi-coverage.py`.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const BAD: PonkoStatus = PONKO_INVALID_ARGUMENT;

#[derive(Default)]
struct Counts {
    sim: u32,
    frame: u32,
    late: u32,
}

unsafe extern "C" fn count_sim(_: *mut PonkoCtx, user: *mut c_void) {
    unsafe { (*user.cast::<Counts>()).sim += 1 }
}
unsafe extern "C" fn count_frame(_: *mut PonkoCtx, user: *mut c_void) {
    unsafe { (*user.cast::<Counts>()).frame += 1 }
}
unsafe extern "C" fn count_late(_: *mut PonkoCtx, user: *mut c_void) {
    unsafe { (*user.cast::<Counts>()).late += 1 }
}

#[test]
fn frame_and_late_systems_are_registered_by_name_and_do_not_run_headless() {
    let mut counts = Counts::default();
    let user = (&raw mut counts).cast::<c_void>();
    unsafe {
        let game = ponko_game_new(c"systems".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_system(game, c"hud".as_ptr(), Some(count_frame), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_late_system(game, c"look-at".as_ptr(), Some(count_late), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(game, c"sim".as_ptr(), 1, Some(count_sim), user),
            PONKO_OK
        );
        // A null name gets a default one; a null function is refused.
        assert_eq!(ponko_game_system(game, null(), Some(count_frame), user), PONKO_OK);
        assert_eq!(ponko_game_late_system(game, null(), Some(count_late), user), PONKO_OK);
        assert_eq!(ponko_game_system(game, c"x".as_ptr(), None, user), BAD);
        assert_eq!(ponko_game_late_system(game, c"x".as_ptr(), None, user), BAD);
        assert_eq!(
            ponko_game_system(null_mut(), c"x".as_ptr(), Some(count_frame), user),
            BAD
        );
        assert_eq!(
            ponko_game_late_system(null_mut(), c"x".as_ptr(), Some(count_late), user),
            BAD
        );

        // Both are found by their names, once each.
        assert_eq!(ponko_game_remove_system(game, c"look-at".as_ptr()), PONKO_OK);
        assert_eq!(ponko_game_remove_system(game, c"look-at".as_ptr()), PONKO_NOT_FOUND);
        assert_eq!(ponko_game_remove_system(game, c"hud".as_ptr()), PONKO_OK);
        assert_eq!(ponko_game_remove_system(game, c"hud".as_ptr()), PONKO_NOT_FOUND);

        // Re-add them (the unnamed pair stays) and run: only the sim system runs.
        assert_eq!(
            ponko_game_system(game, c"hud".as_ptr(), Some(count_frame), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_late_system(game, c"look-at".as_ptr(), Some(count_late), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 5, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(counts.sim, 5);
    assert_eq!((counts.frame, counts.late), (0, 0), "frames only exist in a window");
}

#[test]
fn camera_controls_is_the_built_in_system_by_its_path_and_leaves_the_camera_alone_without_input() {
    unsafe extern "C" fn camera_is_still(ctx: *mut PonkoCtx, user: *mut c_void) {
        unsafe {
            let mut camera = PonkoCamera {
                position: PonkoVec2 { x: 1.0, y: 1.0 },
                zoom: 0.0,
            };
            assert_eq!(ponko_ctx_camera(ctx, &mut camera), PONKO_OK);
            *user.cast::<(f32, f32, f32)>() = (camera.position.x, camera.position.y, camera.zoom);
        }
    }
    let mut seen: (f32, f32, f32) = (9.0, 9.0, 9.0);
    unsafe {
        let game = ponko_game_new(c"controls".as_ptr(), 1, 1);
        assert_eq!(ponko_game_camera_controls(game), PONKO_OK);
        assert_eq!(ponko_game_camera_controls(null_mut()), BAD);
        // It is the engine's own system: removable by its path, once.
        assert_eq!(
            ponko_game_remove_system(game, c"ponko::camera::camera_controls".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_remove_system(game, c"ponko::camera::camera_controls".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(ponko_game_camera_controls(game), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 2, Some(camera_is_still), (&raw mut seen).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(seen, (0.0, 0.0, 1.0));
}

#[test]
fn msaa_checks_its_argument_and_leaves_headless_runs_working() {
    unsafe {
        let game = ponko_game_new(c"settings".as_ptr(), 1, 1);
        // The 3D frame takes 2 or 4 samples per pixel.
        for samples in [0, 1, 3, 8] {
            assert_eq!(ponko_game_msaa(game, samples), BAD, "{samples} samples");
        }
        assert_eq!(ponko_game_msaa(game, 2), PONKO_OK);
        assert_eq!(ponko_game_msaa(game, 4), PONKO_OK);
        // vsync and fullscreen are read only when a window opens: no headless
        // effect to observe, so they are on the allowlist.
        assert_eq!(ponko_game_msaa(null_mut(), 4), BAD);
        // A refused value left the game intact.
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

/// Without the `steam` feature every Steam call says so; the calls that need
/// a live lobby cannot be exercised without a Steam client (see the allowlist).
#[cfg(not(feature = "steam"))]
#[test]
fn steam_calls_report_a_build_without_steam_and_start_nothing() {
    unsafe {
        let mut lobby = 7u64;
        assert_eq!(ponko_steam_wait_for_invite(480, 0, &mut lobby), BAD);
        assert_eq!(lobby, 7, "the output is untouched");
        assert_eq!(ponko_steam_wait_for_invite(480, 0, null_mut()), BAD);
        let game = ponko_game_new(c"steam".as_ptr(), 1, 1);
        assert_eq!(ponko_game_lockstep_steam_join(game, 480, 1), BAD);
        assert_eq!(ponko_game_lockstep_steam_join(null_mut(), 480, 1), BAD);
        // The game is still an ordinary one.
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
}

unsafe extern "C" fn steam_in_the_context(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let status = user.cast::<(PonkoStatus, PonkoStatus, u64)>();
        let mut lobby = 7u64;
        (*status).0 = ponko_ctx_steam_lobby(ctx, &mut lobby);
        (*status).2 = lobby;
        (*status).1 = ponko_ctx_steam_invite(ctx);
        assert_eq!(ponko_ctx_steam_lobby(ctx, null_mut()), BAD);
        assert_eq!(ponko_ctx_steam_lobby(null_mut(), &mut lobby), BAD);
    }
}

#[test]
fn a_session_that_is_not_on_steam_has_no_lobby() {
    let mut seen = (PONKO_OK, PONKO_OK, 7u64);
    unsafe {
        let game = ponko_game_new(c"lobbyless".as_ptr(), 1, 1);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 1, Some(steam_in_the_context), (&raw mut seen).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    if cfg!(feature = "steam") {
        // Lobby 0 means "none"; inviting to no lobby does nothing.
        assert_eq!(seen, (PONKO_OK, PONKO_OK, 0));
    } else {
        assert_eq!(seen, (BAD, BAD, 7), "no Steam in this build");
    }
}

#[test]
fn steam_lobby_calls_on_a_plain_message_endpoint_are_refused() {
    unsafe {
        let mut endpoints = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback(2, endpoints.as_mut_ptr()), PONKO_OK);
        let endpoint = endpoints[0];
        let (mut id, mut needed, mut members, mut text) = (7u64, 7usize, [0u64; 4], [0 as std::ffi::c_char; 16]);
        assert_eq!(ponko_net_messages_steam_lobby_id(endpoint, &mut id), BAD);
        assert_eq!(id, 7);
        assert_eq!(ponko_net_messages_steam_lobby_invite(endpoint), BAD);
        assert_eq!(
            ponko_net_messages_steam_lobby_set_metadata(endpoint, c"mode".as_ptr(), c"coop".as_ptr()),
            BAD
        );
        assert_eq!(
            ponko_net_messages_steam_lobby_metadata(endpoint, c"mode".as_ptr(), text.as_mut_ptr(), 16, &mut needed),
            BAD
        );
        assert_eq!(
            ponko_net_messages_steam_lobby_members(endpoint, members.as_mut_ptr(), 4, &mut needed),
            BAD
        );
        assert_eq!(needed, 7, "nothing was written");
        assert_eq!(ponko_net_messages_steam_lobby_id(null_mut(), &mut id), BAD);
        ponko_net_messages_free(endpoints[0]);
        ponko_net_messages_free(endpoints[1]);
    }
}
