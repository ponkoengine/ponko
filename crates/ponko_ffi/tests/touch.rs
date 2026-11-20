//! Touch through the C API (gl#61): fingers fed by hand and by a scenario,
//! gestures bound to actions and read back, gesture thresholds, and
//! on-screen controls, in headless runs.
mod common;
use common::*;
use ponko_ffi::*;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

fn at(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

fn source(kind: PonkoActionSource, first: u32, scale: f32) -> PonkoActionBinding {
    PonkoActionBinding {
        kind: kind as u32,
        pad: 0,
        first,
        second: 0,
        third: 0,
        fourth: 0,
        deadzone: scale,
    }
}

unsafe fn feed(ctx: *mut PonkoCtx, id: u64, phase: PonkoTouchPhase, x: f32, y: f32) {
    assert_eq!(
        unsafe { ponko_ctx_feed_touch(ctx, id, phase as u32, at(x, y)) },
        PONKO_OK,
        "{}",
        last_error()
    );
}

unsafe fn flag(
    call: unsafe extern "C" fn(*mut PonkoCtx, *const std::ffi::c_char, *mut bool) -> PonkoStatus,
    ctx: *mut PonkoCtx,
    name: &std::ffi::CStr,
) -> bool {
    let mut value = false;
    assert_eq!(
        unsafe { call(ctx, name.as_ptr(), &mut value) },
        PONKO_OK,
        "{}",
        last_error()
    );
    value
}

#[test]
fn gestures_bind_to_actions_from_c_and_read_back_the_same() {
    use PonkoActionSource::*;
    run_ctx(|ctx| unsafe {
        let map = ponko_action_map_new();
        let fire = [source(
            PONKO_ACTION_GESTURE,
            PonkoGesture::PONKO_GESTURE_TAP as u32,
            0.0,
        )];
        let zoom = [source(
            PONKO_ACTION_GESTURE_AXIS,
            PonkoGestureAxis::PONKO_GESTURE_AXIS_PINCH as u32,
            0.0,
        )];
        let pan = [source(PONKO_ACTION_DRAG, 0, 2.0)];
        assert_eq!(
            ponko_action_map_set(map, c"fire".as_ptr(), 0, fire.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(
            ponko_action_map_set(map, c"zoom".as_ptr(), 1, zoom.as_ptr(), 1),
            PONKO_OK
        );
        assert_eq!(ponko_action_map_set(map, c"pan".as_ptr(), 2, pan.as_ptr(), 1), PONKO_OK);
        // A gesture source on the wrong kind of action, or an unknown gesture.
        assert_eq!(
            ponko_action_map_set(map, c"bad".as_ptr(), 1, fire.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        let unknown = [source(PONKO_ACTION_GESTURE, 99, 0.0)];
        assert_eq!(
            ponko_action_map_set(map, c"bad".as_ptr(), 0, unknown.as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );

        // Read back: the zero scale reads as 1, the one given as itself.
        let read = |name: &std::ffi::CStr| {
            let (mut kind, mut count) = (0, 0);
            let mut out = [source(PONKO_ACTION_KEY, 0, 0.0); 2];
            assert_eq!(
                ponko_action_map_get(map, name.as_ptr(), &mut kind, out.as_mut_ptr(), 2, &mut count),
                PONKO_OK,
                "{}",
                last_error()
            );
            (kind, out[0].kind, out[0].first, out[0].deadzone, count)
        };
        assert_eq!(read(c"fire"), (0, 7, 0, 0.0, 1));
        assert_eq!(read(c"zoom"), (1, 8, 0, 1.0, 1));
        assert_eq!(read(c"pan"), (2, 9, 0, 2.0, 1));

        assert_eq!(ponko_ctx_set_actions(ctx, map), PONKO_OK);
        ponko_action_map_free(map);
        // A tap presses "fire", is listed as a gesture event, and a
        // rebinding screen sees it as a gesture binding.
        feed(ctx, 1, PonkoTouchPhase::PONKO_TOUCH_STARTED, 100.0, 80.0);
        feed(ctx, 1, PonkoTouchPhase::PONKO_TOUCH_ENDED, 101.0, 80.0);
        assert!(flag(ponko_ctx_action_pressed, ctx, c"fire"));
        let mut events = [PonkoGestureEvent::default(); 4];
        let mut count = 0;
        assert_eq!(
            ponko_ctx_gesture_events(ctx, events.as_mut_ptr(), 4, &mut count),
            PONKO_OK
        );
        assert_eq!(count, 1);
        assert_eq!(events[0].gesture, PonkoGesture::PONKO_GESTURE_TAP as u32);
        assert_eq!((events[0].position.x, events[0].position.y), (101.0, 80.0));
        let mut pressed = [source(PONKO_ACTION_KEY, 0, 0.0); 4];
        assert_eq!(
            ponko_ctx_pressed_this_frame(ctx, 0.0, pressed.as_mut_ptr(), 4, &mut count),
            PONKO_OK
        );
        assert_eq!(count, 1);
        assert_eq!(
            (pressed[0].kind, pressed[0].first),
            (7, PonkoGesture::PONKO_GESTURE_TAP as u32)
        );
        // A finger moving is a drag: listed while down, its movement on "pan".
        feed(ctx, 2, PonkoTouchPhase::PONKO_TOUCH_STARTED, 300.0, 300.0);
        feed(ctx, 2, PonkoTouchPhase::PONKO_TOUCH_MOVED, 340.0, 290.0);
        let mut touches = [PonkoTouch::default(); 2];
        assert_eq!(ponko_ctx_touches(ctx, touches.as_mut_ptr(), 2, &mut count), PONKO_OK);
        assert_eq!(count, 1);
        assert_eq!(touches[0].id, 2);
        assert_eq!((touches[0].start.x, touches[0].position.x), (300.0, 340.0));
        assert!(!touches[0].claimed);
        let (mut drag, mut pinch, mut turn) = (PonkoVec2::default(), 0.0, 9.0);
        assert_eq!(
            ponko_ctx_gesture_deltas(ctx, &mut drag, &mut pinch, &mut turn),
            PONKO_OK
        );
        assert_eq!((drag.x, drag.y, pinch, turn), (40.0, -10.0, 1.0, 0.0));
        let mut pan = PonkoVec2::default();
        assert_eq!(ponko_ctx_action_axis2(ctx, c"pan".as_ptr(), &mut pan), PONKO_OK);
        assert_eq!((pan.x, pan.y), (80.0, -20.0), "the drag, scaled by 2");
        // Bad input changes nothing.
        assert_eq!(ponko_ctx_feed_touch(ctx, 3, 9, at(0.0, 0.0)), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_feed_touch(ctx, 3, 0, at(f32::NAN, 0.0)),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_touches(ctx, null_mut(), 0, &mut count), PONKO_OK);
        assert_eq!(count, 1, "only the dragging finger");
    });
}

#[test]
fn gesture_settings_read_and_change_from_c() {
    run_ctx(|ctx| unsafe {
        let mut settings = PonkoGestureSettings::default();
        assert_eq!(ponko_ctx_gesture_settings(ctx, &mut settings), PONKO_OK);
        assert_eq!(settings.long_press_time, 0.5, "the default");
        settings.long_press_time = 0.8;
        assert_eq!(ponko_ctx_set_gesture_settings(ctx, &settings), PONKO_OK);
        let mut back = PonkoGestureSettings::default();
        assert_eq!(ponko_ctx_gesture_settings(ctx, &mut back), PONKO_OK);
        assert_eq!(back, settings);
        settings.tap_time = -1.0;
        assert_eq!(ponko_ctx_set_gesture_settings(ctx, &settings), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_set_gesture_settings(ctx, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_gesture_settings(ctx, &mut back), PONKO_OK);
        assert_eq!(back.tap_time, 0.3, "a rejected change leaves them as they were");
    });
}

#[test]
fn onscreen_controls_from_c_drive_their_actions() {
    let game = unsafe { ponko_game_new(c"touch".as_ptr(), 960, 540) };
    let world = run_game(game, 0, |ctx| unsafe {
        let map = action_map();
        assert_eq!(ponko_ctx_set_actions(ctx, map), PONKO_OK);
        ponko_action_map_free(map);
        let controls = [
            PonkoTouchControl {
                kind: PonkoTouchControlKind::PONKO_TOUCH_STICK as u32,
                action: c"move".as_ptr(),
                label: null(),
            },
            PonkoTouchControl {
                kind: PonkoTouchControlKind::PONKO_TOUCH_BUTTON as u32,
                action: c"jump".as_ptr(),
                label: c"A".as_ptr(),
            },
        ];
        let auto = PonkoTouchShow::PONKO_TOUCH_SHOW_AUTO as u32;
        assert_eq!(
            ponko_ctx_set_touch_controls(ctx, controls.as_ptr(), 2, auto, false),
            PONKO_OK
        );
        let mut touch = true;
        assert_eq!(ponko_ctx_touch_device(ctx, &mut touch), PONKO_OK);
        assert!(!touch, "a headless run is not a touch device until a finger is seen");
        assert_eq!(ponko_ctx_set_touch_device(ctx, true), PONKO_OK);

        let (mut stick, mut jump) = (PonkoVec2::default(), PonkoVec2::default());
        assert_eq!(
            ponko_ctx_touch_control_center(ctx, c"move".as_ptr(), &mut stick),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_touch_control_center(ctx, c"jump".as_ptr(), &mut jump),
            PONKO_OK
        );
        assert!(
            stick.x < 480.0 && jump.x > 480.0,
            "the stick at the left, the button at the right"
        );
        let mut nowhere = PonkoVec2::default();
        assert_eq!(
            ponko_ctx_touch_control_center(ctx, c"nope".as_ptr(), &mut nowhere),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_set_touch_control_shown(ctx, c"nope".as_ptr(), false),
            PONKO_NOT_FOUND
        );

        // A finger on the button holds "jump" and is claimed; one on the
        // stick pushed right walks right.
        feed(ctx, 1, PonkoTouchPhase::PONKO_TOUCH_STARTED, jump.x, jump.y);
        feed(ctx, 2, PonkoTouchPhase::PONKO_TOUCH_STARTED, stick.x, stick.y);
        feed(ctx, 2, PonkoTouchPhase::PONKO_TOUCH_MOVED, stick.x + 300.0, stick.y);
        assert!(flag(ponko_ctx_action_down, ctx, c"jump"));
        let mut walk = PonkoVec2::default();
        assert_eq!(ponko_ctx_action_axis2(ctx, c"move".as_ptr(), &mut walk), PONKO_OK);
        assert!(walk.x > 0.99 && walk.y.abs() < 0.01, "{walk:?}");
        let mut touches = [PonkoTouch::default(); 2];
        let mut count = 0;
        assert_eq!(ponko_ctx_touches(ctx, touches.as_mut_ptr(), 2, &mut count), PONKO_OK);
        assert!(
            touches[..count].iter().all(|t| t.claimed),
            "fingers on controls are claimed"
        );

        // Hidden, a control takes no new finger.
        feed(ctx, 1, PonkoTouchPhase::PONKO_TOUCH_ENDED, jump.x, jump.y);
        assert_eq!(
            ponko_ctx_set_touch_control_shown(ctx, c"jump".as_ptr(), false),
            PONKO_OK
        );
        feed(ctx, 3, PonkoTouchPhase::PONKO_TOUCH_STARTED, jump.x, jump.y);
        assert!(!flag(ponko_ctx_action_down, ctx, c"jump"));

        // Bad descriptions change nothing; removing takes them away.
        let bad = [PonkoTouchControl {
            kind: 9,
            action: c"jump".as_ptr(),
            label: null(),
        }];
        assert_eq!(
            ponko_ctx_set_touch_controls(ctx, bad.as_ptr(), 1, auto, false),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_set_touch_controls(ctx, controls.as_ptr(), 2, 7, false),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_touch_control_center(ctx, c"move".as_ptr(), &mut nowhere),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_set_touch_controls_show(ctx, PonkoTouchShow::PONKO_TOUCH_SHOW_NEVER as u32),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_remove_touch_controls(ctx), PONKO_OK);
        assert_eq!(
            ponko_ctx_touch_control_center(ctx, c"move".as_ptr(), &mut nowhere),
            PONKO_NOT_FOUND
        );
    });
    unsafe { ponko_world_free(world) };
}

#[test]
fn a_scenario_touch_step_is_a_touch_in_its_json() {
    unsafe {
        let s = ponko_input_scenario_new();
        let start = PonkoTouchPhase::PONKO_TOUCH_STARTED as u32;
        assert_eq!(ponko_input_scenario_touch(s, 2, 1, start, at(40.0, 50.0)), PONKO_OK);
        assert_eq!(
            ponko_input_scenario_touch(s, 2, 1, 7, at(40.0, 50.0)),
            PONKO_INVALID_ARGUMENT
        );
        let mut len = 0;
        assert_eq!(ponko_input_scenario_to_json(s, null_mut(), 0, &mut len), PONKO_OK);
        let mut buffer = vec![0u8; len + 1];
        assert_eq!(
            ponko_input_scenario_to_json(s, buffer.as_mut_ptr().cast(), buffer.len(), &mut len),
            PONKO_OK
        );
        let json = String::from_utf8_lossy(&buffer[..len]).into_owned();
        assert!(
            json.contains(r#""touch""#) && json.contains(r#""started""#) && json.contains("40"),
            "{json}"
        );
        ponko_input_scenario_free(s);
    }
}
