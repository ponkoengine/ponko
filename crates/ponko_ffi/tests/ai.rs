//! Behaviour trees and brains through the C API.
use ponko_ffi::PonkoAiCurve::*;
use ponko_ffi::PonkoAiStatus::*;
use ponko_ffi::*;
use std::ffi::{CStr, CString};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

fn key(name: &str) -> u64 {
    let name = CString::new(name).unwrap();
    let mut id = 0;
    assert_eq!(unsafe { ponko_ai_key(name.as_ptr(), &mut id) }, PONKO_OK);
    id
}

fn tree(json: &str) -> *mut PonkoBehaviorTree {
    let json = CString::new(json).unwrap();
    let tree = unsafe { ponko_behavior_tree_new(json.as_ptr()) };
    assert!(!tree.is_null(), "{}", last_error());
    tree
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

fn action(brain: *const PonkoBrain) -> u64 {
    let mut k = u64::MAX;
    assert_eq!(unsafe { ponko_brain_action(brain, &mut k) }, PONKO_OK);
    k
}

fn text(f: impl Fn(*mut std::ffi::c_char, usize, *mut usize) -> PonkoStatus) -> String {
    let mut len = 0;
    assert_eq!(f(null_mut(), 0, &mut len), PONKO_OK);
    let mut buf = vec![0u8; len + 1];
    assert_eq!(f(buf.as_mut_ptr().cast(), buf.len(), &mut len), PONKO_OK);
    String::from_utf8(buf[..len].to_vec()).unwrap()
}

#[test]
fn a_native_brain_thinks_acts_and_reports_through_the_c_api() {
    let guard = tree(
        r#"{"root": {"selector": [
            {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4.5}]},
            {"do": "patrol"}
        ]}}"#,
    );
    let (alert, chase, patrol, speed) = (key("alert"), key("chase"), key("patrol"), key("speed"));
    let brain = ponko_brain_new(1);
    unsafe {
        assert_eq!(action(brain), 0, "nothing before the first think");
        assert_eq!(ponko_brain_think(brain, guard, 0, 1.0 / 60.0), PONKO_OK);
        assert_eq!(action(brain), patrol);
        let mut started = false;
        assert_eq!(ponko_brain_started(brain, &mut started), PONKO_OK);
        assert!(started);

        assert_eq!(ponko_brain_set_flag(brain, alert, true), PONKO_OK);
        assert_eq!(ponko_brain_think(brain, guard, 1, 1.0 / 60.0), PONKO_OK);
        assert_eq!(action(brain), chase);
        let mut v = 0.0;
        assert_eq!(ponko_brain_param_num(brain, speed, &mut v), PONKO_OK);
        assert_eq!(v, 4.5);
        assert_eq!(ponko_brain_param_num(brain, alert, &mut v), PONKO_NOT_FOUND);

        assert_eq!(ponko_brain_finish(brain, PONKO_AI_SUCCESS), PONKO_OK);
        let mut status = PONKO_AI_RUNNING;
        assert_eq!(ponko_brain_status(brain, &mut status), PONKO_OK);
        assert_eq!(status, PONKO_AI_SUCCESS);
        assert_eq!(ponko_brain_finish(brain, PONKO_AI_RUNNING), PONKO_INVALID_ARGUMENT);

        let trace = text(|b, c, l| ponko_brain_trace(brain, b, c, l));
        assert_eq!(
            trace,
            "> selector\n  > sequence\n    + check alert\n    > do chase (speed: 4.5)\n  . do patrol\n"
        );

        let mut len = 0;
        assert_eq!(ponko_behavior_tree_len(guard, &mut len), PONKO_OK);
        assert_eq!(len, 5);
        ponko_brain_free(brain);
        ponko_behavior_tree_free(guard);
    }
}

#[test]
fn the_blackboard_reads_back_what_the_game_wrote() {
    let brain = ponko_brain_new(1);
    let (health, alert, home, aim, target) = (key("health"), key("alert"), key("home"), key("aim"), key("target"));
    unsafe {
        assert_eq!(ponko_brain_set_num(brain, health, 0.5), PONKO_OK);
        assert_eq!(ponko_brain_set_flag(brain, alert, true), PONKO_OK);
        assert_eq!(
            ponko_brain_set_vec2(brain, home, PonkoVec2 { x: 1.0, y: 2.0 }),
            PONKO_OK
        );
        assert_eq!(
            ponko_brain_set_vec3(brain, aim, PonkoVec3 { x: 1.0, y: 2.0, z: 3.0 }),
            PONKO_OK
        );
        assert_eq!(ponko_brain_set_entity(brain, target, 7), PONKO_OK);
        let (mut n, mut f) = (0.0, false);
        assert_eq!(ponko_brain_num(brain, health, &mut n), PONKO_OK);
        assert_eq!(n, 0.5);
        assert_eq!(ponko_brain_flag(brain, alert, &mut f), PONKO_OK);
        assert!(f);
        assert_eq!(
            ponko_brain_num(brain, alert, &mut n),
            PONKO_NOT_FOUND,
            "a flag isn't a number"
        );
        assert_eq!(ponko_brain_flag(brain, key("missing"), &mut f), PONKO_NOT_FOUND);
        // An id no name was made for.
        assert_eq!(ponko_brain_set_num(brain, 12345, 1.0), PONKO_NOT_FOUND);
        let json = text(|b, c, l| ponko_brain_blackboard_json(brain, b, c, l));
        assert_eq!(
            json,
            r#"{"health":0.5,"alert":true,"home":[1.0,2.0],"aim":[1.0,2.0,3.0],"target":{"entity":7}}"#
        );
        let name = text(|b, c, l| ponko_ai_key_name(home, b, c, l));
        assert_eq!(name, "home");
        ponko_brain_free(brain);
    }
}

#[test]
fn a_bad_tree_returns_null_and_names_the_node() {
    let json = CString::new(r#"{"root": {"sequence": [{"do": "a"}, {"wiat": 1}]}}"#).unwrap();
    assert!(unsafe { ponko_behavior_tree_new(json.as_ptr()) }.is_null());
    let error = last_error();
    assert!(error.contains("root/sequence[1]: no node kind"), "{error}");
    assert!(unsafe { ponko_behavior_tree_new(null()) }.is_null());
}

#[test]
fn periods_spread_thinking_and_finishing_wakes_the_brain() {
    let t = tree(r#"{"root": {"sequence": [{"do": "a"}, {"do": "b"}]}}"#);
    let brain = ponko_brain_new(4);
    let due = |index: u32, tick: u64| {
        let mut d = false;
        assert_eq!(unsafe { ponko_brain_due(brain, index, tick, &mut d) }, PONKO_OK);
        d
    };
    unsafe {
        assert!(due(1, 1), "due before its first think");
        assert_eq!(ponko_brain_think(brain, t, 0, 0.1), PONKO_OK);
        assert!(!due(1, 1) && due(1, 3) && due(1, 7));
        assert_eq!(ponko_brain_finish(brain, PONKO_AI_SUCCESS), PONKO_OK);
        assert!(due(1, 1), "woken when its action ends");
        assert_eq!(ponko_brain_think(brain, t, 1, 0.1), PONKO_OK);
        assert_eq!(action(brain), key("b"));
        assert_eq!(ponko_brain_set_period(brain, 1), PONKO_OK);
        assert!(due(1, 2));
        assert_eq!(ponko_brain_wake(brain), PONKO_OK);
        ponko_brain_free(brain);
        ponko_behavior_tree_free(t);
    }
}

#[test]
fn utility_helpers_match_rust() {
    for (curve, param, x, expected) in [
        (PONKO_AI_CURVE_LINEAR, 0.0, 0.3, 0.3),
        (PONKO_AI_CURVE_CUBIC, 0.0, 0.5, 0.125),
        (PONKO_AI_CURVE_SQRT, 0.0, 0.25, 0.5),
        (PONKO_AI_CURVE_SMOOTH, 0.0, 0.5, 0.5),
        (PONKO_AI_CURVE_POWER, 3.0, 0.5, 0.125),
        (PONKO_AI_CURVE_LOGISTIC, 10.0, 0.5, 0.5),
        (PONKO_AI_CURVE_STEP, 0.5, 0.5, 1.0),
    ] {
        assert!((ponko_ai_curve(curve, param, x) - expected).abs() < 1e-6, "{curve:?}");
    }
    let scores = [1.0f32, 0.0, 1.0];
    assert_eq!(unsafe { ponko_ai_combine(scores.as_ptr(), 3) }, 0.0);
}

#[test]
fn a_linked_tree_runs_its_subtrees_and_a_cycle_names_both_trees() {
    let fight = tree(r#"{"root": {"sequence": [{"check": "alert"}, {"subtree": "punch"}]}}"#);
    let punch = tree(r#"{"root": {"do": "punch"}}"#);
    let names = [c"fight".as_ptr(), c"punch".as_ptr()];
    let subtrees = [fight.cast_const(), punch.cast_const()];
    let json = cr#"{"root": {"selector": [{"subtree": "fight"}, {"do": "idle"}]}}"#;
    let guard = unsafe { ponko_behavior_tree_new_linked(json.as_ptr(), names.as_ptr(), subtrees.as_ptr(), 2) };
    assert!(!guard.is_null(), "{}", last_error());
    unsafe {
        // The subtrees may go: the linked tree keeps what it needs.
        ponko_behavior_tree_free(fight);
        ponko_behavior_tree_free(punch);
        let brain = ponko_brain_new(1);
        assert_eq!(ponko_brain_set_flag(brain, key("alert"), true), PONKO_OK);
        assert_eq!(ponko_brain_think(brain, guard, 0, 0.1), PONKO_OK);
        assert_eq!(action(brain), key("punch"));
        ponko_brain_free(brain);
        ponko_behavior_tree_free(guard);

        let a = tree(r#"{"root": {"subtree": "b"}}"#);
        let b = tree(r#"{"root": {"subtree": "a"}}"#);
        let names = [c"a".as_ptr(), c"b".as_ptr()];
        let subtrees = [a.cast_const(), b.cast_const()];
        let json = cr#"{"root": {"subtree": "a"}}"#;
        assert!(ponko_behavior_tree_new_linked(json.as_ptr(), names.as_ptr(), subtrees.as_ptr(), 2).is_null());
        let error = last_error();
        assert!(error.contains("subtree cycle: b -> a -> b"), "{error}");
        assert!(ponko_behavior_tree_new_linked(json.as_ptr(), null(), null(), 1).is_null());
        ponko_behavior_tree_free(a);
        ponko_behavior_tree_free(b);
    }
}

#[test]
fn a_signal_ends_a_wait_and_a_saved_brain_carries_on_where_it_was() {
    let t = tree(
        r#"{"root": {"sequence": [
            {"wait_for": "door"}, {"do": "enter"}, {"wait": 1}, {"do": "sit"}
        ]}}"#,
    );
    let brain = ponko_brain_new(1);
    unsafe {
        assert_eq!(ponko_brain_set_seed(brain, 99), PONKO_OK);
        assert_eq!(ponko_brain_think(brain, t, 0, 0.1), PONKO_OK);
        assert_eq!(action(brain), 0, "waiting for the door");
        assert_eq!(ponko_brain_signal(brain, key("door")), PONKO_OK);
        let mut due = false;
        assert_eq!(ponko_brain_due(brain, 0, 1, &mut due), PONKO_OK);
        assert!(due);
        assert_eq!(ponko_brain_think(brain, t, 1, 0.1), PONKO_OK);
        assert_eq!(action(brain), key("enter"));
        assert_eq!(ponko_brain_finish(brain, PONKO_AI_SUCCESS), PONKO_OK);
        // Three of the ten ticks of the wait go by, then the game saves.
        for tick in 2..5 {
            assert_eq!(ponko_brain_think(brain, t, tick, 0.1), PONKO_OK);
        }
        assert_eq!(ponko_brain_set_num(brain, key("health"), 0.5), PONKO_OK);
        let saved = text(|b, c, l| ponko_brain_save_json(brain, b, c, l));
        ponko_brain_free(brain);

        let json = CString::new(saved.clone()).unwrap();
        let back = ponko_brain_load_json(json.as_ptr());
        assert!(!back.is_null(), "{}", last_error());
        assert_eq!(
            text(|b, c, l| ponko_brain_save_json(back, b, c, l)),
            saved,
            "reads back as it was"
        );
        let mut health = 0.0;
        assert_eq!(ponko_brain_num(back, key("health"), &mut health), PONKO_OK);
        assert_eq!(health, 0.5);
        // Its own ticks go on from the save (tick 4), whatever the game's
        // are now: the wait, begun at tick 2, ends at its tick 12.
        for tick in 1000..1007 {
            assert_eq!(ponko_brain_think(back, t, tick, 0.1), PONKO_OK);
            assert_eq!(action(back), 0, "still waiting at {tick}");
        }
        assert_eq!(ponko_brain_think(back, t, 1007, 0.1), PONKO_OK);
        assert_eq!(action(back), key("sit"));
        ponko_brain_free(back);
        assert!(ponko_brain_load_json(c"{\"brain\": 3}".as_ptr()).is_null());
        assert!(last_error().contains("not a saved brain"), "{}", last_error());
        ponko_behavior_tree_free(t);
    }
}
