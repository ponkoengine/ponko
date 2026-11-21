//! On-screen controls' looks and places, and the player's layout, through
//! the C ABI: styles and places set with the controls, the edit mode that
//! moves a control, the layout's size, opacity and per-control changes,
//! its JSON, and saving it for the next run.
mod common;
use common::last_error;
use ponko_ffi::*;
use std::ffi::CString;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

fn at(x: f32, y: f32) -> PonkoVec2 {
    PonkoVec2 { x, y }
}

fn desc() -> PonkoTouchControlsDesc {
    // SAFETY: a plain C struct; zero is a valid start.
    let mut d: PonkoTouchControlsDesc = unsafe { std::mem::zeroed() };
    (d.struct_size, d.version) = (
        size_of::<PonkoTouchControlsDesc>() as u32,
        PONKO_TOUCH_CONTROLS_DESC_VERSION,
    );
    // SAFETY: header set.
    assert_eq!(unsafe { ponko_touch_controls_desc_defaults(&mut d) }, PONKO_OK);
    d
}

const CONTROLS: [PonkoTouchControl; 2] = [
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

/// Runs `body` in the setup of a 960 x 540 headless game (with `dir` as
/// its data folder).
fn run(dir: Option<&CString>, body: impl FnOnce(*mut PonkoCtx)) {
    unsafe {
        let game = ponko_game_new(c"touch".as_ptr(), 960, 540);
        assert_eq!(ponko_game_hot_reload(game, false), PONKO_OK);
        if let Some(dir) = dir {
            assert_eq!(ponko_game_data_dir(game, dir.as_ptr()), PONKO_OK);
        }
        ponko_world_free(common::run_game(game, 0, body));
    }
}

unsafe fn center(ctx: *mut PonkoCtx, action: &std::ffi::CStr) -> PonkoVec2 {
    let mut c = PonkoVec2::default();
    assert_eq!(
        unsafe { ponko_ctx_touch_control_center(ctx, action.as_ptr(), &mut c) },
        PONKO_OK,
        "{}",
        last_error()
    );
    c
}

unsafe fn shown_controls(ctx: *mut PonkoCtx, d: &mut PonkoTouchControlsDesc) {
    d.controls = CONTROLS.as_ptr();
    d.count = CONTROLS.len();
    d.show = PonkoTouchShow::PONKO_TOUCH_SHOW_ALWAYS as u32;
    assert_eq!(
        unsafe { ponko_ctx_set_touch_controls_ex(ctx, d) },
        PONKO_OK,
        "{}",
        last_error()
    );
}

#[test]
fn a_place_set_from_c_puts_the_control_there() {
    run(None, |ctx| unsafe {
        let mut d = desc();
        let jump = [PonkoTouchControlPlace {
            action: c"jump".as_ptr(),
            place: PonkoTouchPlace {
                anchor: at(0.75, 1.0),
                offset: at(0.0, -60.0),
            },
        }];
        d.places = jump.as_ptr();
        d.place_count = 1;
        shown_controls(ctx, &mut d);
        let c = center(ctx, c"jump");
        assert_eq!((c.x, c.y), (720.0, 480.0));
    });
}

#[test]
fn touch_styles_from_c_start_from_the_engines_and_are_checked() {
    let style = ponko_touch_style_default();
    assert_eq!(
        (style.size, style.label_size, style.has_image, style.has_knob_image),
        (1.0, 0.8, false, false)
    );
    assert_eq!(desc().style, style, "the desc's defaults are the engine's look");
    run(None, |ctx| unsafe {
        let mut d = desc();
        d.style.size = 1.2;
        let red = [PonkoTouchControlStyle {
            action: c"jump".as_ptr(),
            style: PonkoTouchStyle {
                button: PonkoColor {
                    r: 1.0,
                    g: 0.2,
                    b: 0.2,
                    a: 0.4,
                },
                ..style
            },
        }];
        d.styles = red.as_ptr();
        d.style_count = 1;
        shown_controls(ctx, &mut d);
        // A size of 0, a colour that isn't finite, a missing array: refused.
        let mut bad = d;
        bad.style.size = 0.0;
        assert_eq!(ponko_ctx_set_touch_controls_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        let nan = [PonkoTouchControlStyle {
            action: c"jump".as_ptr(),
            style: PonkoTouchStyle {
                held: PonkoColor {
                    r: f32::NAN,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
                ..style
            },
        }];
        let mut bad = d;
        bad.styles = nan.as_ptr();
        assert_eq!(ponko_ctx_set_touch_controls_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        let mut bad = d;
        bad.places = null();
        bad.place_count = 1;
        assert_eq!(ponko_ctx_set_touch_controls_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_set_touch_controls_ex(ctx, null()), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn edit_mode_moves_a_control_into_the_players_layout_without_pressing_it() {
    run(None, |ctx| unsafe {
        let mut d = desc();
        shown_controls(ctx, &mut d);
        let mut editing = true;
        assert_eq!(ponko_ctx_editing_touch_controls(ctx, &mut editing), PONKO_OK);
        assert!(!editing);
        assert_eq!(ponko_ctx_edit_touch_controls(ctx, true), PONKO_OK);
        assert_eq!(ponko_ctx_editing_touch_controls(ctx, &mut editing), PONKO_OK);
        assert!(editing);
        let from = center(ctx, c"jump");
        let to = at(from.x - 300.0, from.y);
        use PonkoTouchPhase::*;
        assert_eq!(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_STARTED as u32, from), PONKO_OK);
        assert_eq!(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_MOVED as u32, to), PONKO_OK);
        assert_eq!(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_ENDED as u32, to), PONKO_OK);
        assert_eq!(ponko_ctx_edit_touch_controls(ctx, false), PONKO_OK);
        let now = center(ctx, c"jump");
        assert_eq!((now.x, now.y), (to.x, to.y));
        let mut adjust = PonkoTouchAdjust::default();
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"jump".as_ptr(), &mut adjust),
            PONKO_OK
        );
        assert!(adjust.placed, "the move is in the player's layout");
        assert_eq!(adjust.size, 1.0);
        // The stick nobody moved follows the game.
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"move".as_ptr(), &mut adjust),
            PONKO_OK
        );
        assert!(!adjust.placed);
    });
}

#[test]
fn the_players_layout_is_set_read_reset_and_round_trips_as_json() {
    run(None, |ctx| unsafe {
        let mut d = desc();
        shown_controls(ctx, &mut d);
        let game_place = center(ctx, c"jump");
        assert_eq!(ponko_ctx_set_touch_layout(ctx, 1.5, 2.0), PONKO_OK);
        let (mut size, mut opacity) = (0.0, 0.0);
        assert_eq!(ponko_ctx_touch_layout(ctx, &mut size, &mut opacity), PONKO_OK);
        assert_eq!((size, opacity), (1.5, 1.0), "opacity is kept within 0..=1");
        assert_eq!(ponko_ctx_set_touch_layout(ctx, f32::NAN, 1.0), PONKO_INVALID_ARGUMENT);

        let moved = PonkoTouchAdjust {
            placed: true,
            place: PonkoTouchPlace {
                anchor: at(0.0, 1.0),
                offset: at(200.0, -200.0),
            },
            size: 2.0,
        };
        assert_eq!(
            ponko_ctx_set_touch_layout_control(ctx, c"jump".as_ptr(), &moved),
            PONKO_OK
        );
        let c = center(ctx, c"jump");
        assert_eq!((c.x, c.y), (200.0, 340.0), "the player's place, at once");
        let mut back = PonkoTouchAdjust::default();
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"jump".as_ptr(), &mut back),
            PONKO_OK
        );
        assert_eq!(back, moved);

        // As JSON and back.
        let mut len = 0;
        assert_eq!(ponko_ctx_touch_layout_json(ctx, null_mut(), 0, &mut len), PONKO_OK);
        let mut json = vec![0u8; len + 1];
        assert_eq!(
            ponko_ctx_touch_layout_json(ctx, json.as_mut_ptr().cast(), json.len(), &mut len),
            PONKO_OK
        );
        let json = CString::new(&json[..len]).unwrap();
        assert!(json.to_str().unwrap().contains("\"jump\""));
        assert_eq!(ponko_ctx_reset_touch_layout(ctx), PONKO_OK);
        let c = center(ctx, c"jump");
        assert_eq!(
            (c.x, c.y),
            (game_place.x, game_place.y),
            "reset: where the game puts it"
        );
        assert_eq!(ponko_ctx_set_touch_layout_json(ctx, json.as_ptr()), PONKO_OK);
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"jump".as_ptr(), &mut back),
            PONKO_OK
        );
        assert_eq!(back, moved);
        assert_eq!(
            ponko_ctx_set_touch_layout_json(ctx, c"{ not json".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"jump".as_ptr(), &mut back),
            PONKO_OK
        );
        assert_eq!(back, moved, "a broken layout changes nothing");

        // Null forgets the player's changes to one control.
        assert_eq!(
            ponko_ctx_set_touch_layout_control(ctx, c"jump".as_ptr(), null()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_touch_layout_control(ctx, c"jump".as_ptr(), &mut back),
            PONKO_OK
        );
        assert!(!back.placed);
        let nan = PonkoTouchAdjust {
            size: f32::NAN,
            ..moved
        };
        assert_eq!(
            ponko_ctx_set_touch_layout_control(ctx, c"jump".as_ptr(), &nan),
            PONKO_INVALID_ARGUMENT
        );
    });
}

#[test]
fn a_saved_layout_comes_back_in_the_next_run() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-touch-layout-{}", std::process::id()));
    let c_dir = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
    run(Some(&c_dir), |ctx| unsafe {
        // No file yet: nothing changes.
        assert_eq!(ponko_ctx_restore_touch_layout(ctx), PONKO_OK);
        assert_eq!(ponko_ctx_set_touch_layout(ctx, 1.0, 0.4), PONKO_OK);
        assert_eq!(ponko_ctx_save_touch_layout(ctx), PONKO_OK, "{}", last_error());
    });
    run(Some(&c_dir), |ctx| unsafe {
        assert_eq!(ponko_ctx_restore_touch_layout(ctx), PONKO_OK, "{}", last_error());
        let mut opacity = 0.0;
        assert_eq!(ponko_ctx_touch_layout(ctx, null_mut(), &mut opacity), PONKO_OK);
        assert_eq!(opacity, 0.4);
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn trackpad_gestures_from_c_feed_the_pinch_and_make_no_touch_device() {
    use PonkoGesture::*;
    use PonkoTouchPhase::*;
    run(None, |ctx| unsafe {
        assert_eq!(
            ponko_ctx_feed_trackpad(ctx, PONKO_GESTURE_PINCH as u32, PONKO_TOUCH_STARTED as u32, 0.25),
            PONKO_OK
        );
        let mut scale = 0.0;
        assert_eq!(
            ponko_ctx_gesture_deltas(ctx, null_mut(), &mut scale, null_mut()),
            PONKO_OK
        );
        assert_eq!(scale, 1.25);
        let mut touch = true;
        assert_eq!(ponko_ctx_touch_device(ctx, &mut touch), PONKO_OK);
        assert!(!touch, "a trackpad is not a touch screen");
        assert_eq!(ponko_ctx_feed_trackpad(ctx, 99, 0, 0.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_feed_trackpad(ctx, 4, 9, 0.0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_feed_trackpad(ctx, 4, 0, f32::NAN), PONKO_INVALID_ARGUMENT);
    });
    // The scenario step is the same one Rust writes.
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(
            ponko_input_scenario_trackpad(s, 3, PONKO_GESTURE_ROTATE as u32, PONKO_TOUCH_MOVED as u32, 0.5),
            PONKO_OK
        );
        let mut len = 0;
        assert_eq!(ponko_input_scenario_to_json(s, null_mut(), 0, &mut len), PONKO_OK);
        let mut json = vec![0u8; len + 1];
        assert_eq!(
            ponko_input_scenario_to_json(s, json.as_mut_ptr().cast(), json.len(), &mut len),
            PONKO_OK
        );
        let json = String::from_utf8_lossy(&json[..len]).into_owned();
        assert!(json.contains("trackpad"), "{json}");
        assert_eq!(ponko_input_scenario_trackpad(s, 3, 99, 0, 0.0), PONKO_INVALID_ARGUMENT);
        ponko_input_scenario_free(s);
    }
}
