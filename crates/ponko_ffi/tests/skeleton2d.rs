//! 2D skeletons from C: loading a file (and its warnings), putting a
//! skeleton on an entity, clips, the state machine and its checked
//! parameters, events, IK targets, bones, slots and skins. Runs headless
//! with the skeleton updated by hand.
mod common;

use common::{last_error, run_ctx};
use ponko_ffi::*;
use std::ffi::{CStr, c_char};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

const RIG: &str = r#"{ "format": "ponko-skeleton2d", "version": 1,
  "bones": [
    { "name": "root" },
    { "name": "upper", "parent": "root", "length": 10 },
    { "name": "lower", "parent": "upper", "x": 10, "length": 10 },
    { "name": "goal", "parent": "root", "x": 15, "y": 5 }
  ],
  "slots": [{ "name": "body", "bone": "root", "attachment": "body" }],
  "skins": [
    { "name": "default", "attachments": [
      { "slot": "body", "name": "body", "image": "crab.png", "size": [8, 8] },
      { "slot": "body", "name": "fist", "image": "crab.png", "size": [4, 4] } ] },
    { "name": "gold", "attachments": [{ "slot": "body", "name": "body", "image": "crab.png", "size": [16, 16] }] }
  ],
  "ik": [{ "name": "reach", "bones": ["upper", "lower"], "target": "goal", "mix": 0 }],
  "clips": [
    { "name": "idle", "duration": 1, "events": [{ "time": 0.5, "name": "blink" }] },
    { "name": "wave", "bones": [{ "bone": "root",
      "translate": [{ "time": 0, "value": [0, 0] }, { "time": 1, "value": [10, 0] }] }] }
  ],
  "graph": {
    "parameters": [{ "name": "speed", "float": 0 }, { "name": "grounded", "bool": true }, { "name": "jump", "trigger": false }],
    "states": [{ "name": "idle", "clip": "idle" }, { "name": "wave", "clip": "wave", "loop": false }],
    "transitions": [{ "from": "idle", "to": "wave", "when": [{ "trigger": "jump" }] }],
    "initial": "idle"
  }
}"#;

const NO_SCALE: &str = r#"{ "skeleton": { "spine": "4.2.11" },
  "bones": [{ "name": "root" }, { "name": "arm", "parent": "root", "inherit": "noScale" }] }"#;

const CRAB: &[u8] = include_bytes!("../../../assets/crab.png");

unsafe fn files(ctx: *mut PonkoCtx) {
    unsafe {
        let source = ponko_memory_source_new(c"rigs".as_ptr());
        for (name, bytes) in [
            (c"rig/rig.json", RIG.as_bytes()),
            (c"rig/crab.png", CRAB),
            (c"spine/no-scale.json", NO_SCALE.as_bytes()),
        ] {
            assert_eq!(
                ponko_memory_source_insert(source, name.as_ptr(), bytes.as_ptr(), bytes.len()),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
        ponko_memory_source_free(source);
    }
}

/// A new entity with the rig on it, updated by hand.
unsafe fn rigged(ctx: *mut PonkoCtx) -> u64 {
    unsafe {
        let mut e = 0u64;
        assert_eq!(ponko_spawn(ponko_ctx_world(ctx), null(), 0, &mut e), PONKO_OK);
        assert_eq!(ponko_ctx_set_skeleton2d(ctx, e, c"rig/rig.json".as_ptr()), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_set_manual(ctx, e, true), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        e
    }
}

unsafe fn index(
    f: unsafe extern "C" fn(*mut PonkoCtx, u64, *const c_char, *mut usize) -> PonkoStatus,
    ctx: *mut PonkoCtx,
    e: u64,
    name: &CStr,
) -> usize {
    let mut i = 99;
    assert_eq!(
        unsafe { f(ctx, e, name.as_ptr(), &mut i) },
        PONKO_OK,
        "{}",
        last_error()
    );
    i
}

unsafe fn state(ctx: *mut PonkoCtx, e: u64) -> (usize, f32, bool) {
    let (mut s, mut t, mut done) = (99, f32::NAN, false);
    assert_eq!(
        unsafe { ponko_ctx_skeleton2d_state(ctx, e, &mut s, &mut t, &mut done) },
        PONKO_OK
    );
    (s, t, done)
}

unsafe fn bone(ctx: *mut PonkoCtx, e: u64, bone: usize) -> (PonkoVec2, f32) {
    let (mut p, mut r) = (
        PonkoVec2 {
            x: f32::NAN,
            y: f32::NAN,
        },
        f32::NAN,
    );
    assert_eq!(
        unsafe { ponko_ctx_skeleton2d_bone(ctx, e, bone, &mut p, &mut r) },
        PONKO_OK
    );
    (p, r)
}

unsafe fn attachment(ctx: *mut PonkoCtx, e: u64, slot: usize) -> String {
    let mut buffer = [0 as c_char; 32];
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_ctx_skeleton2d_attachment(ctx, e, slot, buffer.as_mut_ptr(), buffer.len(), &mut len) },
        PONKO_OK
    );
    let text = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().unwrap().to_string();
    assert_eq!(text.len(), len);
    text
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

#[test]
fn files_load_once_with_their_warnings() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let mut warnings = 99;
        assert_eq!(
            ponko_ctx_load_skeleton2d(ctx, c"rig/rig.json".as_ptr(), &mut warnings),
            PONKO_OK
        );
        assert_eq!(warnings, 0);
        assert_eq!(
            ponko_ctx_load_skeleton2d(ctx, c"spine/no-scale.json".as_ptr(), &mut warnings),
            PONKO_OK
        );
        assert_eq!(warnings, 1);
        let mut len = 0;
        assert_eq!(
            ponko_ctx_skeleton2d_warning(ctx, c"spine/no-scale.json".as_ptr(), 0, null_mut(), 0, &mut len),
            PONKO_OK
        );
        let mut buffer = vec![0 as c_char; len + 1];
        assert_eq!(
            ponko_ctx_skeleton2d_warning(
                ctx,
                c"spine/no-scale.json".as_ptr(),
                0,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut len
            ),
            PONKO_OK
        );
        let warning = CStr::from_ptr(buffer.as_ptr()).to_str().unwrap();
        assert!(
            warning.starts_with("spine/no-scale.json: Spine inherit mode \"noScale\" (bone \"arm\")"),
            "{warning}"
        );
        assert_eq!(
            ponko_ctx_skeleton2d_warning(ctx, c"spine/no-scale.json".as_ptr(), 1, null_mut(), 0, &mut len),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_load_skeleton2d(ctx, c"nope.json".as_ptr(), null_mut()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_load_skeleton2d(ctx, null(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn clips_play_crossfade_and_finish() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        assert_eq!(state(ctx, e).0, 0, "the graph's first state");
        assert_eq!(ponko_ctx_skeleton2d_play(ctx, e, c"wave".as_ptr(), false), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.5), PONKO_OK);
        near(bone(ctx, e, 0).0.x, 5.0);
        let (s, t, done) = state(ctx, e);
        assert_eq!((s, done), (usize::MAX, false), "a clip, not a state");
        near(t, 0.5);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.6), PONKO_OK);
        assert!(state(ctx, e).2, "played once, finished");
        near(bone(ctx, e, 0).0.x, 10.0);
        assert_eq!(ponko_ctx_skeleton2d_crossfade(ctx, e, c"idle".as_ptr(), 0.5), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.25), PONKO_OK);
        near(bone(ctx, e, 0).0.x, 5.0);
        assert_eq!(
            ponko_ctx_skeleton2d_play(ctx, e, c"fly".as_ptr(), true),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_skeleton2d_crossfade(ctx, e, c"idle".as_ptr(), -1.0),
            PONKO_INVALID_ARGUMENT
        );
        // The speed scales the clock.
        let white = PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        assert_eq!(ponko_ctx_skeleton2d_set_look(ctx, e, 2.0, white, 2.0), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_play(ctx, e, c"wave".as_ptr(), false), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.25), PONKO_OK);
        near(bone(ctx, e, 0).0.x, 5.0);
        assert_eq!(
            ponko_ctx_skeleton2d_set_look(ctx, e, 0.0, white, -1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, f32::NAN), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn the_state_machine_takes_checked_parameters_and_states() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        let wave = index(ponko_ctx_skeleton2d_state_index, ctx, e, c"wave");
        let speed = index(ponko_ctx_skeleton2d_parameter_index, ctx, e, c"speed");
        let grounded = index(ponko_ctx_skeleton2d_parameter_index, ctx, e, c"grounded");
        let jump = index(ponko_ctx_skeleton2d_parameter_index, ctx, e, c"jump");
        assert_eq!((wave, speed, grounded, jump), (1, 0, 1, 2));
        let mut i = 0;
        assert_eq!(
            ponko_ctx_skeleton2d_state_index(ctx, e, c"swim".as_ptr(), &mut i),
            PONKO_NOT_FOUND
        );

        assert_eq!(ponko_ctx_skeleton2d_set_float(ctx, e, speed, 2.0), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_set_bool(ctx, e, grounded, false), PONKO_OK);
        assert_eq!(
            ponko_ctx_skeleton2d_set_float(ctx, e, grounded, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(last_error(), "parameter 1 is not a float parameter");
        assert_eq!(
            ponko_ctx_skeleton2d_set_float(ctx, e, speed, f32::NAN),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(last_error(), "value for parameter 0 is not finite");
        assert_eq!(ponko_ctx_skeleton2d_set_bool(ctx, e, 9, true), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_skeleton2d_set_trigger(ctx, e, speed, true),
            PONKO_INVALID_ARGUMENT
        );

        // The trigger fires the transition and is used up.
        assert_eq!(ponko_ctx_skeleton2d_set_trigger(ctx, e, jump, true), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        assert_eq!(state(ctx, e).0, wave);

        assert_eq!(
            ponko_ctx_skeleton2d_crossfade_state(ctx, e, 5, 0.1, 0.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(last_error(), "no such animation state 5");
        assert_eq!(
            ponko_ctx_skeleton2d_crossfade_state(ctx, e, 0, -1.0, 0.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_skeleton2d_crossfade_state(ctx, e, 0, 0.0, 0.25), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        let (s, t, _) = state(ctx, e);
        assert_eq!(s, 0);
        near(t, 0.25);
    });
}

#[test]
fn events_come_out_one_at_a_time() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.6), PONKO_OK);
        let mut buffer = [0 as c_char; 4];
        let mut found = false;
        assert_eq!(
            ponko_ctx_skeleton2d_next_event(ctx, e, buffer.as_mut_ptr(), buffer.len(), &mut found),
            PONKO_OK
        );
        assert!(found);
        assert_eq!(CStr::from_ptr(buffer.as_ptr()).to_str().unwrap(), "bli", "cut to fit");
        assert_eq!(
            ponko_ctx_skeleton2d_next_event(ctx, e, buffer.as_mut_ptr(), buffer.len(), &mut found),
            PONKO_OK
        );
        assert!(!found, "taken");
        assert_eq!(
            ponko_ctx_skeleton2d_next_event(ctx, e, null_mut(), 0, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn ik_targets_bend_the_chain_by_their_mix() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        let reach = index(ponko_ctx_skeleton2d_ik_index, ctx, e, c"reach");
        let upper = index(ponko_ctx_skeleton2d_bone_index, ctx, e, c"upper");
        let lower = index(ponko_ctx_skeleton2d_bone_index, ctx, e, c"lower");
        assert_eq!((reach, upper, lower), (0, 1, 2));
        near(bone(ctx, e, upper).1, 0.0);
        // Straight down, exactly the chain's length away: both bones point
        // down (a quarter turn clockwise on screen).
        let down = PonkoVec2 { x: 0.0, y: 20.0 };
        assert_eq!(ponko_ctx_skeleton2d_set_ik_target(ctx, e, reach, down), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, reach, 1.0), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        near(bone(ctx, e, upper).1, std::f32::consts::FRAC_PI_2);
        let (p, _) = bone(ctx, e, lower);
        near(p.x, 0.0);
        near(p.y, 10.0);
        assert_eq!(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, reach, 0.0), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        near(bone(ctx, e, upper).1, 0.0);
        assert_eq!(ponko_ctx_skeleton2d_clear_ik_target(ctx, e, reach), PONKO_OK);
        assert_eq!(
            ponko_ctx_skeleton2d_set_ik_target(ctx, e, 3, down),
            PONKO_INVALID_ARGUMENT
        );
        let nan = PonkoVec2 { x: f32::NAN, y: 0.0 };
        assert_eq!(
            ponko_ctx_skeleton2d_set_ik_target(ctx, e, reach, nan),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, 3, 1.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_skeleton2d_clear_ik_target(ctx, e, 3), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_skeleton2d_bone(ctx, e, 9, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn look_at_and_two_bone_ik_pose_the_bones_once() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        let upper = index(ponko_ctx_skeleton2d_bone_index, ctx, e, c"upper");
        let lower = index(ponko_ctx_skeleton2d_bone_index, ctx, e, c"lower");
        // Look straight down from the root: a quarter turn clockwise.
        let below = PonkoVec2 { x: 0.0, y: 10.0 };
        assert_eq!(ponko_ctx_skeleton2d_look_at(ctx, e, upper, below, 1.0), PONKO_OK);
        near(bone(ctx, e, upper).1, std::f32::consts::FRAC_PI_2);
        // Half way: an eighth turn.
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_look_at(ctx, e, upper, below, 0.5), PONKO_OK);
        near(bone(ctx, e, upper).1, std::f32::consts::FRAC_PI_4);
        // Two bones of 10 reaching (10, 10): the elbow at (10, 0) bending
        // clockwise, at (0, 10) the other way.
        let corner = PonkoVec2 { x: 10.0, y: 10.0 };
        assert_eq!(
            ponko_ctx_skeleton2d_two_bone_ik(ctx, e, upper, lower, corner, true, 1.0),
            PONKO_OK
        );
        near(bone(ctx, e, upper).1, 0.0);
        let (elbow, turn) = bone(ctx, e, lower);
        near(elbow.x, 10.0);
        near(elbow.y, 0.0);
        near(turn, std::f32::consts::FRAC_PI_2);
        assert_eq!(
            ponko_ctx_skeleton2d_two_bone_ik(ctx, e, upper, lower, corner, false, 1.0),
            PONKO_OK
        );
        let (elbow, turn) = bone(ctx, e, lower);
        near(elbow.x, 0.0);
        near(elbow.y, 10.0);
        near(turn, 0.0);
        // The next update poses the bones from their clips again.
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        near(bone(ctx, e, upper).1, 0.0);
        near(bone(ctx, e, lower).0.y, 0.0);
        // Checked: a chain whose second bone isn't the first's child, a bone
        // that isn't there, a target or mix that isn't a number.
        assert_eq!(
            ponko_ctx_skeleton2d_two_bone_ik(ctx, e, lower, upper, corner, true, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("child"));
        assert_eq!(
            ponko_ctx_skeleton2d_two_bone_ik(ctx, e, upper, 9, corner, true, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_skeleton2d_look_at(ctx, e, 9, below, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        let nan = PonkoVec2 { x: f32::NAN, y: 0.0 };
        assert_eq!(
            ponko_ctx_skeleton2d_look_at(ctx, e, upper, nan, 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_skeleton2d_two_bone_ik(ctx, e, upper, lower, corner, true, f32::NAN),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn slots_and_skins_swap_attachments() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        let body = index(ponko_ctx_skeleton2d_slot_index, ctx, e, c"body");
        assert_eq!(attachment(ctx, e, body), "body");
        assert_eq!(
            ponko_ctx_skeleton2d_set_attachment(ctx, e, body, c"fist".as_ptr()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        assert_eq!(attachment(ctx, e, body), "fist");
        assert_eq!(ponko_ctx_skeleton2d_set_attachment(ctx, e, body, null()), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        assert_eq!(attachment(ctx, e, body), "", "hidden");
        assert_eq!(ponko_ctx_skeleton2d_reset_attachment(ctx, e, body), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_OK);
        assert_eq!(attachment(ctx, e, body), "body");
        assert_eq!(ponko_ctx_skeleton2d_set_skin(ctx, e, c"gold".as_ptr()), PONKO_OK);
        assert_eq!(
            ponko_ctx_skeleton2d_set_skin(ctx, e, c"silver".as_ptr()),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_skeleton2d_set_attachment(ctx, e, 4, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_skeleton2d_reset_attachment(ctx, e, 4), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_skeleton2d_attachment(ctx, e, 4, null_mut(), 0, &mut 0),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn entities_without_a_skeleton_are_not_found() {
    run_ctx(|ctx| unsafe {
        files(ctx);
        let e = rigged(ctx);
        assert_eq!(ponko_ctx_remove_skeleton2d(ctx, e), PONKO_OK);
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, e, 0.0), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_skeleton2d_set_manual(ctx, e, false), PONKO_NOT_FOUND);
        assert_eq!(ponko_ctx_remove_skeleton2d(ctx, e), PONKO_OK, "nothing to remove");
        assert_eq!(
            ponko_ctx_set_skeleton2d(ctx, e, c"rig/missing.json".as_ptr()),
            PONKO_NOT_FOUND
        );
        let dead = 0xffff_ffff_0000_0000u64;
        assert_eq!(ponko_ctx_skeleton2d_update(ctx, dead, 0.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_set_skeleton2d(ctx, dead, c"rig/rig.json".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_remove_skeleton2d(ctx, dead), PONKO_INVALID_ARGUMENT);
    });
}
