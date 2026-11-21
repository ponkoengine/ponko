//! Touch scenario builders from C: `ponko_input_scenario_tap`,
//! `_touch_hold` and `_touch_drag` add the same steps as Rust's
//! `InputScenario::tap`, `touch_hold` and `touch_drag`, and a tap on an
//! on-screen button presses its action in a headless run.
mod common;
use common::*;
use ponko_ffi::*;
use std::ptr::null_mut;

use PonkoStatus::*;

fn at(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

unsafe fn json(s: *const PonkoInputScenario) -> serde_json::Value {
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_input_scenario_to_json(s, null_mut(), 0, &mut len) },
        PONKO_OK
    );
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(
        unsafe { ponko_input_scenario_to_json(s, buffer.as_mut_ptr().cast(), buffer.len(), &mut len) },
        PONKO_OK
    );
    serde_json::from_slice(&buffer[..len]).unwrap()
}

#[test]
fn tap_hold_and_drag_from_c_add_the_steps_rust_adds() {
    use ponko::prelude::vec2;
    let rust = ponko::scenario::InputScenario::new()
        .tap(3, vec2(100.0, 200.0))
        .touch_hold(10, 50, vec2(700.0, 500.0))
        .touch_drag(5, 25, vec2(100.0, 400.0), vec2(160.0, 400.0));
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(ponko_input_scenario_tap(s, 3, at(100.0, 200.0)), PONKO_OK);
        assert_eq!(ponko_input_scenario_touch_hold(s, 10, 50, at(700.0, 500.0)), PONKO_OK);
        assert_eq!(
            ponko_input_scenario_touch_drag(s, 5, 25, at(100.0, 400.0), at(160.0, 400.0)),
            PONKO_OK
        );
        let (mut ticks, mut steps) = (0, 0);
        assert_eq!(ponko_input_scenario_info(s, &mut ticks, &mut steps), PONKO_OK);
        assert_eq!((ticks, steps), (rust.ticks(), rust.step_count()));
        assert_eq!(json(s), rust.to_json(), "the same fingers, ids and ticks");
        ponko_input_scenario_free(s);
    }
}

#[test]
fn touch_builders_refuse_bad_positions_and_null_scenarios() {
    unsafe {
        let s = ponko_input_scenario_new();
        let nan = at(f32::NAN, 0.0);
        assert_eq!(ponko_input_scenario_tap(s, 1, nan), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_input_scenario_touch_hold(s, 1, 2, nan), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_input_scenario_touch_drag(s, 1, 2, at(0.0, 0.0), nan),
            PONKO_INVALID_ARGUMENT
        );
        let mut steps = 1;
        ponko_input_scenario_info(s, null_mut(), &mut steps);
        assert_eq!(steps, 0, "a refused step adds nothing");
        assert_eq!(
            ponko_input_scenario_tap(null_mut(), 1, at(0.0, 0.0)),
            PONKO_INVALID_ARGUMENT
        );
        ponko_input_scenario_free(s);
    }
}

#[test]
fn a_tapped_scenario_presses_the_action_bound_to_taps() {
    use PonkoActionSource::*;
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut std::ffi::c_void) {
        unsafe {
            let map = ponko_action_map_new();
            let tap = [PonkoActionBinding {
                kind: PONKO_ACTION_GESTURE as u32,
                pad: 0,
                first: PonkoGesture::PONKO_GESTURE_TAP as u32,
                second: 0,
                third: 0,
                fourth: 0,
                deadzone: 0.0,
            }];
            assert_eq!(
                ponko_action_map_set(map, c"fire".as_ptr(), 0, tap.as_ptr(), 1),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_set_actions(ctx, map), PONKO_OK);
            ponko_action_map_free(map);
        }
    }
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(ponko_input_scenario_tap(s, 2, at(40.0, 50.0)), PONKO_OK);
        assert_eq!(
            ponko_input_scenario_expect_action(s, 2, c"fire".as_ptr(), 1, -1, -1),
            PONKO_OK,
            "{}",
            last_error()
        );
        let game = ponko_game_new(c"taps".as_ptr(), 64, 64);
        assert_eq!(ponko_game_input_scenario(game, s), PONKO_OK);
        let mut ticks = 0;
        ponko_input_scenario_info(s, &mut ticks, null_mut());
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, ticks, Some(setup), null_mut(), &mut world),
            PONKO_OK
        );
        let mut report: PonkoScenarioReport = std::mem::zeroed();
        assert_eq!(ponko_world_scenario_report(world, &mut report), PONKO_OK);
        assert!(!report.failed, "the tap pressed fire");
        assert_eq!(report.checks_passed, 1);
        ponko_world_free(world);
        ponko_input_scenario_free(s);
    }
}
