//! Input scenarios from C: input and checks by tick, run headless, with a
//! report in the world.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const SPACE: u32 = PonkoKey::PONKO_KEY_SPACE as u32;

unsafe fn report(world: *mut PonkoWorld) -> PonkoScenarioReport {
    let mut r = std::mem::MaybeUninit::<PonkoScenarioReport>::zeroed();
    assert_eq!(
        unsafe { ponko_world_scenario_report(world, r.as_mut_ptr()) },
        PONKO_OK,
        "{}",
        last_error()
    );
    unsafe { r.assume_init() }
}
unsafe fn report_json(world: *mut PonkoWorld) -> serde_json::Value {
    let mut len = 0;
    unsafe {
        assert_eq!(
            ponko_world_scenario_report_json(world, null_mut(), 0, &mut len),
            PONKO_OK
        );
        let mut buffer = vec![0 as std::ffi::c_char; len + 1];
        assert_eq!(
            ponko_world_scenario_report_json(world, buffer.as_mut_ptr(), buffer.len(), &mut len),
            PONKO_OK
        );
        serde_json::from_str(CStr::from_ptr(buffer.as_ptr()).to_str().unwrap()).unwrap()
    }
}
unsafe fn json(s: *const PonkoInputScenario) -> String {
    let mut len = 0;
    unsafe {
        assert_eq!(ponko_input_scenario_to_json(s, null_mut(), 0, &mut len), PONKO_OK);
        let mut buffer = vec![0 as std::ffi::c_char; len + 1];
        assert_eq!(
            ponko_input_scenario_to_json(s, buffer.as_mut_ptr(), buffer.len(), &mut len),
            PONKO_OK
        );
        CStr::from_ptr(buffer.as_ptr()).to_str().unwrap().to_owned()
    }
}
/// Holds Space on ticks 2 to 4 and checks the jump action's edges.
unsafe fn jump_scenario(wrong_tick: Option<u64>) -> *mut PonkoInputScenario {
    let s = ponko_input_scenario_new();
    unsafe {
        assert_eq!(ponko_input_scenario_named(s, c"jump".as_ptr()), PONKO_OK);
        assert_eq!(ponko_input_scenario_at(s, 2, 0, 0, SPACE, 1.0), PONKO_OK);
        assert_eq!(ponko_input_scenario_at(s, 5, 0, 0, SPACE, 0.0), PONKO_OK);
        assert_eq!(
            ponko_input_scenario_expect_action(s, 2, c"jump".as_ptr(), 1, 1, -1),
            PONKO_OK
        );
        assert_eq!(
            ponko_input_scenario_expect_action(s, 5, c"jump".as_ptr(), -1, 0, 1),
            PONKO_OK
        );
        if let Some(tick) = wrong_tick {
            assert_eq!(
                ponko_input_scenario_expect_action(s, tick, c"jump".as_ptr(), 1, -1, -1),
                PONKO_OK
            );
        }
        assert_eq!(ponko_input_scenario_duration(s, 8), PONKO_OK);
    }
    s
}
unsafe fn run(s: *const PonkoInputScenario) -> *mut PonkoWorld {
    let game = new_game();
    let mut ticks = 0;
    unsafe {
        assert_eq!(ponko_input_scenario_info(s, &mut ticks, null_mut()), PONKO_OK);
        assert_eq!(ponko_game_input_scenario(game, s), PONKO_OK);
    }
    run_game(game, ticks, |ctx| unsafe { install_actions(ctx) })
}

#[test]
fn a_scenario_feeds_input_through_the_actions_and_passes() {
    unsafe {
        let s = jump_scenario(None);
        let (mut ticks, mut steps) = (0, 0);
        assert_eq!(ponko_input_scenario_info(s, &mut ticks, &mut steps), PONKO_OK);
        assert_eq!((ticks, steps), (8, 4));
        let world = run(s);
        ponko_input_scenario_free(s);
        let r = report(world);
        assert_eq!(
            r.status,
            PonkoScenarioStatus::PONKO_SCENARIO_PASSED as u32,
            "{}",
            report_json(world)
        );
        assert!(!r.failed);
        assert_eq!((r.duration, r.ticks_run, r.steps), (8, 8, 4));
        assert_eq!((r.inputs_applied, r.checks_passed), (2, 2));
        assert_eq!(report_json(world)["id"], "jump");
        ponko_world_free(world);
    }
}

#[test]
fn a_failed_check_names_its_step_and_tick() {
    unsafe {
        let s = jump_scenario(Some(3)); // pressed on tick 2, not 3
        let world = run(s);
        ponko_input_scenario_free(s);
        let r = report(world);
        assert_eq!(r.status, PonkoScenarioStatus::PONKO_SCENARIO_FAILED as u32);
        assert!(r.failed);
        assert_eq!((r.failure_step, r.failure_tick, r.failure_sim_tick), (4, 3, 3));
        assert_eq!(r.ticks_run, 4, "the run ends at the failed check");
        let failure = &report_json(world)["failure"];
        assert_eq!(failure["step"], 4);
        assert!(failure["message"].as_str().unwrap().contains("jump"), "{failure}");
        ponko_world_free(world);
    }
}

#[test]
fn resource_checks_compare_reflected_values() {
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(
            ponko_input_scenario_expect_resource(s, 1, c"Score".as_ptr(), c"/points".as_ptr(), c"3".as_ptr()),
            PONKO_OK
        );
        assert_eq!(ponko_input_scenario_expect_count(s, 1, c"Coin".as_ptr(), 0), PONKO_OK);
        assert_eq!(ponko_input_scenario_duration(s, 2), PONKO_OK);
        let game = new_game();
        assert_eq!(ponko_game_input_scenario(game, s), PONKO_OK);
        ponko_input_scenario_free(s);
        let world = run_game(game, 2, |ctx| {
            let w = ponko_ctx_world(ctx);
            let points: u32 = 3;
            let field = PonkoField {
                name: c"points".as_ptr(),
                kind: PonkoFieldType::PONKO_FIELD_U32 as u32,
                offset: 0,
            };
            assert_eq!(ponko_resource_reflect(w, c"Score".as_ptr(), 4, 4, &field, 1), PONKO_OK);
            assert_eq!(
                ponko_resource_set(w, c"Score".as_ptr(), (&raw const points).cast(), 4, 4),
                PONKO_OK
            );
            let mut coin = 0;
            assert_eq!(ponko_component_register(w, c"Coin".as_ptr(), 4, 4, &mut coin), PONKO_OK);
            assert_eq!(ponko_component_reflect(w, coin, &field, 1), PONKO_OK);
        });
        let r = report(world);
        assert_eq!(
            r.status,
            PonkoScenarioStatus::PONKO_SCENARIO_PASSED as u32,
            "{}",
            report_json(world)
        );
        assert_eq!(r.checks_passed, 2);
        ponko_world_free(world);
    }
}

#[test]
fn json_round_trips_and_bad_json_is_a_json_error() {
    unsafe {
        let s = jump_scenario(None);
        let text = json(s);
        let mut back = null_mut();
        let c_text = std::ffi::CString::new(text.clone()).unwrap();
        assert_eq!(ponko_input_scenario_from_json(c_text.as_ptr(), &mut back), PONKO_OK);
        assert_eq!(json(back), text);
        ponko_input_scenario_free(back);
        ponko_input_scenario_free(s);

        assert_eq!(
            ponko_input_scenario_from_json(c"{".as_ptr(), &mut back),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_JSON);
        assert_eq!(
            ponko_input_scenario_from_json(null(), &mut back),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn bad_steps_leave_the_scenario_unchanged() {
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(
            ponko_input_scenario_at(s, 1, 9, 0, 0, 1.0),
            PONKO_INVALID_ARGUMENT,
            "unknown kind"
        );
        assert_eq!(
            ponko_input_scenario_at(s, 1, 0, 0, 100_000, 1.0),
            PONKO_INVALID_ARGUMENT,
            "unknown key"
        );
        assert_eq!(
            ponko_input_scenario_expect_action(s, 1, c"jump".as_ptr(), 2, -1, -1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_input_scenario_expect_resource(s, 1, c"Score".as_ptr(), null(), c"not json".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_JSON);
        let mut steps = 9;
        assert_eq!(ponko_input_scenario_info(s, null_mut(), &mut steps), PONKO_OK);
        assert_eq!(steps, 0);
        assert_eq!(
            ponko_input_scenario_at(null_mut(), 1, 0, 0, SPACE, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_game_input_scenario(null_mut(), s), PONKO_INVALID_ARGUMENT);
        let game = new_game();
        assert_eq!(ponko_game_input_scenario(game, null()), PONKO_INVALID_ARGUMENT);
        ponko_game_free(game);
        let w = ponko_world_new();
        let mut r = std::mem::MaybeUninit::zeroed();
        assert_eq!(ponko_world_scenario_report(w, r.as_mut_ptr()), PONKO_NOT_FOUND);
        ponko_world_free(w);
        ponko_input_scenario_free(s);
    }
}

/// Starts a scenario that holds Space on tick 1 and cancels it on tick 3.
unsafe extern "C" fn start_and_cancel(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Vec<(u64, bool, bool)>>() };
    unsafe {
        let tick = ponko_ctx_sim_tick(ctx);
        if tick == 1 {
            let s = ponko_input_scenario_new();
            assert_eq!(ponko_input_scenario_at(s, 0, 0, 0, SPACE, 1.0), PONKO_OK);
            assert_eq!(ponko_input_scenario_duration(s, 10), PONKO_OK);
            assert_eq!(ponko_ctx_start_scenario(ctx, s), PONKO_OK);
            ponko_input_scenario_free(s);
        }
        let mut was_running = false;
        if tick == 3 {
            assert_eq!(ponko_ctx_cancel_scenario(ctx, &mut was_running), PONKO_OK);
        }
        let mut space = false;
        assert_eq!(ponko_ctx_key_down(ctx, SPACE, &mut space), PONKO_OK);
        seen.push((tick, space, was_running));
    }
}

#[test]
fn a_system_starts_and_cancels_a_scenario() {
    unsafe {
        let mut seen: Vec<(u64, bool, bool)> = Vec::new();
        let game = new_game();
        assert_eq!(
            ponko_game_sim_system(game, c"s".as_ptr(), 1, Some(start_and_cancel), (&raw mut seen).cast()),
            PONKO_OK
        );
        let world = run_game(game, 5, |_| {});
        assert_eq!(
            seen,
            [
                (0, false, false),
                (1, false, false),
                (2, true, false),
                (3, false, true),
                (4, false, false)
            ],
            "fed from the next tick; cancelling releases Space"
        );
        assert_eq!(
            report(world).status,
            PonkoScenarioStatus::PONKO_SCENARIO_CANCELLED as u32
        );
        ponko_world_free(world);
    }
}

/// Records from tick 1 (started during tick 0), presses Space during tick 2
/// and stops during tick 4.
unsafe extern "C" fn record_input(ctx: *mut PonkoCtx, user: *mut c_void) {
    let recorded = unsafe { &mut *user.cast::<*mut PonkoInputScenario>() };
    unsafe {
        match ponko_ctx_sim_tick(ctx) {
            0 => assert_eq!(ponko_ctx_start_input_recording(ctx), PONKO_OK),
            2 => assert_eq!(ponko_ctx_feed_input(ctx, 0, 0, SPACE, 1.0), PONKO_OK),
            4 => {
                assert_eq!(ponko_ctx_stop_input_recording(ctx, recorded), PONKO_OK);
                let mut none = null_mut();
                assert_eq!(ponko_ctx_stop_input_recording(ctx, &mut none), PONKO_OK);
                assert!(none.is_null(), "no recording runs any more");
                assert_eq!(ponko_ctx_stop_input_recording(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
            }
            _ => {}
        }
    }
}

#[test]
fn a_system_records_live_input_into_a_scenario() {
    unsafe {
        let mut recorded: *mut PonkoInputScenario = null_mut();
        let game = new_game();
        assert_eq!(
            ponko_game_sim_system(game, c"r".as_ptr(), 1, Some(record_input), (&raw mut recorded).cast()),
            PONKO_OK
        );
        ponko_world_free(run_game(game, 5, |_| {}));
        assert!(!recorded.is_null());
        let (mut ticks, mut steps) = (0, 0);
        assert_eq!(ponko_input_scenario_info(recorded, &mut ticks, &mut steps), PONKO_OK);
        assert_eq!((ticks, steps), (4, 1), "ticks 1 to 4; one key");
        let text: serde_json::Value = serde_json::from_str(&json(recorded)).unwrap();
        assert_eq!(text["steps"][0]["tick"], 1, "fed during tick 2, the recording's tick 1");
        assert_eq!(ponko_ctx_start_input_recording(null_mut()), PONKO_INVALID_ARGUMENT);
        ponko_input_scenario_free(recorded);
    }
}

/// Whether Space was held in each tick, seen by a simulation system that
/// runs after the scenario's input.
unsafe extern "C" fn record_space(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Vec<bool>>() };
    let mut down = false;
    assert_eq!(unsafe { ponko_ctx_key_down(ctx, SPACE, &mut down) }, PONKO_OK);
    seen.push(down);
}

/// Presses Space on tick 0 and never lets go in a two-tick scenario, then
/// runs the game two ticks longer.
fn space_after_the_run(release_when_done: bool) -> (Vec<bool>, PonkoScenarioReport) {
    let mut seen = Vec::<bool>::new();
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(ponko_input_scenario_at(s, 0, 0, 0, SPACE, 1.0), PONKO_OK);
        assert_eq!(ponko_input_scenario_duration(s, 2), PONKO_OK);
        if release_when_done {
            assert_eq!(ponko_input_scenario_release_when_done(s), PONKO_OK);
        }
        let game = new_game();
        assert_eq!(ponko_game_input_scenario(game, s), PONKO_OK);
        ponko_input_scenario_free(s);
        assert_eq!(
            ponko_game_sim_system(game, c"space".as_ptr(), 1, Some(record_space), (&raw mut seen).cast()),
            PONKO_OK
        );
        let world = run_game(game, 4, |_| {});
        let r = report(world);
        assert_eq!(
            r.status,
            PonkoScenarioStatus::PONKO_SCENARIO_PASSED as u32,
            "{}",
            report_json(world)
        );
        ponko_world_free(world);
        (seen, r)
    }
}

#[test]
fn input_stays_held_after_a_run_unless_released_when_done() {
    let (kept, _) = space_after_the_run(false);
    assert_eq!(
        kept,
        [true, true, true, true],
        "the key the scenario pressed stays down"
    );
    let (released, report) = space_after_the_run(true);
    assert_eq!(released, [true, true, false, false], "released when the run ends");
    assert_eq!(report.still_held, 0);
}

#[test]
fn a_buffer_too_small_for_the_json_gets_an_empty_string_and_the_full_length() {
    unsafe {
        let s = jump_scenario(None);
        let full = json(s).len();
        let mut buffer = [b'x' as std::ffi::c_char; 8];
        let mut len = 0;
        assert_eq!(
            ponko_input_scenario_to_json(s, buffer.as_mut_ptr(), buffer.len(), &mut len),
            PONKO_OK
        );
        assert!(full >= buffer.len());
        assert_eq!(len, full, "the length it needs, without the NUL");
        assert_eq!(buffer[0], 0, "an empty string, not a cut-off one");
        assert_eq!(buffer[1], b'x' as std::ffi::c_char, "nothing else written");
        // Exactly the length is still one short: the NUL needs a byte too.
        let mut exact = vec![b'x' as std::ffi::c_char; full];
        assert_eq!(
            ponko_input_scenario_to_json(s, exact.as_mut_ptr(), exact.len(), &mut len),
            PONKO_OK
        );
        assert_eq!((exact[0], len), (0, full));
        ponko_input_scenario_free(s);
    }
}
