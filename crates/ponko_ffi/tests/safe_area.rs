//! The window's safe area from C: `ponko_ctx_window_safe_area`, `_safe_insets`
//! and `_feed_safe_insets`, UI roots laid out inside it
//! (`ponko_ctx_ui_set_inside_safe_area`), and the scenario step that feeds
//! insets (`ponko_input_scenario_safe_insets`), all as Rust's.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;

fn insets(top: f32, right: f32, bottom: f32, left: f32) -> PonkoSafeInsets {
    PonkoSafeInsets {
        top,
        right,
        bottom,
        left,
    }
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> PonkoRect {
    PonkoRect {
        min: PonkoVec2 { x: x0, y: y0 },
        max: PonkoVec2 { x: x1, y: y1 },
    }
}

#[test]
fn the_safe_area_is_the_whole_window_until_insets_are_fed_and_leaves_what_they_cover() {
    unsafe {
        let game = ponko_game_new(c"notch".as_ptr(), 844, 390);
        let world = run_game(game, 0, |ctx| {
            let mut area = PonkoRect::default();
            let mut got = PonkoSafeInsets::default();
            assert_eq!(ponko_ctx_window_safe_area(ctx, &mut area), PONKO_OK);
            assert_eq!(area, rect(0.0, 0.0, 844.0, 390.0), "nothing covered");
            assert_eq!(ponko_ctx_window_safe_insets(ctx, &mut got), PONKO_OK);
            assert_eq!(got, PonkoSafeInsets::default());
            // A phone held sideways: the notch on the left, the home bar below.
            assert_eq!(
                ponko_ctx_window_feed_safe_insets(ctx, insets(0.0, 0.0, 21.0, 47.0)),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_window_safe_area(ctx, &mut area), PONKO_OK);
            assert_eq!(area, rect(47.0, 0.0, 844.0, 369.0));
            assert_eq!(ponko_ctx_window_safe_insets(ctx, &mut got), PONKO_OK);
            assert_eq!(got, insets(0.0, 0.0, 21.0, 47.0));
            // The same as Rust's: a negative or not finite inset is 0.
            assert_eq!(
                ponko_ctx_window_feed_safe_insets(ctx, insets(-5.0, f32::NAN, 21.0, f32::INFINITY)),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_window_safe_insets(ctx, &mut got), PONKO_OK);
            assert_eq!(got, insets(0.0, 0.0, 21.0, 0.0));
            assert_eq!(
                ponko_ctx_window_feed_safe_insets(ctx, PonkoSafeInsets::default()),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_window_safe_area(ctx, &mut area), PONKO_OK);
            assert_eq!(area, rect(0.0, 0.0, 844.0, 390.0), "uncovered again");
        });
        ponko_world_free(world);
    }
}

#[test]
fn safe_area_calls_refuse_null_outputs_and_null_contexts() {
    unsafe {
        run_ctx(|ctx| {
            assert_eq!(ponko_ctx_window_safe_area(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
            assert_eq!(ponko_ctx_window_safe_insets(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
        });
        let mut area = PonkoRect::default();
        let mut got = PonkoSafeInsets::default();
        assert_eq!(
            ponko_ctx_window_safe_area(null_mut(), &mut area),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_window_safe_insets(null_mut(), &mut got),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_window_feed_safe_insets(null_mut(), PonkoSafeInsets::default()),
            PONKO_INVALID_ARGUMENT
        );
    }
}

/// What the frame system saw each frame: the safe area, a HUD root made
/// inside it and a backdrop that was not.
#[derive(Default)]
struct Seen {
    frames: Vec<(PonkoRect, PonkoRect, PonkoRect)>,
}

unsafe extern "C" fn frame(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        let (mut area, mut hud, mut backdrop) = (PonkoRect::default(), PonkoRect::default(), PonkoRect::default());
        let (mut h, mut b) = (PONKO_UI_NONE, PONKO_UI_NONE);
        assert_eq!(ponko_ctx_window_safe_area(ctx, &mut area), PONKO_OK);
        assert_eq!(
            ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, c"hud".as_ptr(), &mut h),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, c"backdrop".as_ptr(), &mut b),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, h, &mut hud), PONKO_OK);
        assert_eq!(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, b, &mut backdrop), PONKO_OK);
        seen.frames.push((area, hud, backdrop));
    }
}

unsafe fn build_hud(ctx: *mut PonkoCtx) {
    unsafe {
        let fill = ponko_ui_layout_fill();
        let (mut backdrop, mut hud) = (PONKO_UI_NONE, PONKO_UI_NONE);
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &mut backdrop),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &mut hud),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, backdrop, c"backdrop".as_ptr()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, hud, c"hud".as_ptr()),
            PONKO_OK
        );
        let mut on = true;
        assert_eq!(
            ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, &mut on),
            PONKO_OK
        );
        assert!(!on, "roots are laid out in the whole window by default");
        assert_eq!(
            ponko_ctx_ui_set_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, true),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, &mut on),
            PONKO_OK
        );
        assert!(on);
        assert_eq!(
            ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, backdrop, &mut on),
            PONKO_OK
        );
        assert!(!on, "the backdrop stays in the whole window");
        assert_eq!(ponko_ctx_ui_show_hud(ctx, PONKO_UI_SCREEN, backdrop), PONKO_OK);
        assert_eq!(ponko_ctx_ui_show_hud(ctx, PONKO_UI_SCREEN, hud), PONKO_OK);
        assert_eq!(
            ponko_ctx_ui_set_inside_safe_area(ctx, PONKO_UI_SCREEN, 9999, true),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, 9999, &mut on),
            PONKO_NOT_FOUND
        );
    }
}

#[test]
fn ui_roots_made_inside_the_safe_area_lay_out_in_it_and_follow_fed_insets() {
    unsafe {
        let mut seen = Seen::default();
        let game = ponko_game_new(c"hud".as_ptr(), 844, 390);
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        assert_eq!(
            ponko_game_system(game, c"frame".as_ptr(), Some(frame), (&raw mut seen).cast()),
            PONKO_OK
        );
        let world = run_game(game, 3, |ctx| {
            build_hud(ctx);
            assert_eq!(
                ponko_ctx_window_feed_safe_insets(ctx, insets(0.0, 0.0, 21.0, 47.0)),
                PONKO_OK
            );
        });
        ponko_world_free(world);
        assert_eq!(seen.frames.len(), 3);
        for (i, &(area, hud, backdrop)) in seen.frames.iter().enumerate() {
            assert_eq!(area, rect(47.0, 0.0, 844.0, 369.0), "frame {i}");
            assert_eq!(hud, area, "frame {i}: the HUD fills the safe area");
            assert_eq!(
                backdrop,
                rect(0.0, 0.0, 844.0, 390.0),
                "frame {i}: the backdrop reaches under the notch"
            );
        }
    }
}

#[test]
fn the_scenario_step_from_c_adds_what_rust_adds_and_feeds_the_window_on_its_tick() {
    use ponko::window::SafeInsets;
    unsafe {
        let s = ponko_input_scenario_new();
        assert_eq!(
            ponko_input_scenario_safe_insets(s, 1, insets(0.0, 0.0, 21.0, 47.0)),
            PONKO_OK
        );
        assert_eq!(
            ponko_input_scenario_safe_insets(s, 3, insets(0.0, 47.0, 21.0, 0.0)),
            PONKO_OK
        );
        let rust = ponko::scenario::InputScenario::new()
            .safe_insets(1, SafeInsets::new(0.0, 0.0, 21.0, 47.0))
            .safe_insets(3, SafeInsets::new(0.0, 47.0, 21.0, 0.0));
        let mut len = 0;
        assert_eq!(ponko_input_scenario_to_json(s, null_mut(), 0, &mut len), PONKO_OK);
        let mut buffer = vec![0u8; len + 1];
        assert_eq!(
            ponko_input_scenario_to_json(s, buffer.as_mut_ptr().cast(), buffer.len(), &mut len),
            PONKO_OK
        );
        let json: serde_json::Value = serde_json::from_slice(&buffer[..len]).unwrap();
        assert_eq!(json, rust.to_json(), "the same steps, ticks and insets");
        let (mut ticks, mut steps) = (0, 0);
        assert_eq!(ponko_input_scenario_info(s, &mut ticks, &mut steps), PONKO_OK);
        assert_eq!((ticks, steps), (4, 2));

        let mut seen = Seen::default();
        let game = ponko_game_new(c"hud".as_ptr(), 844, 390);
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        assert_eq!(
            ponko_game_system(game, c"frame".as_ptr(), Some(frame), (&raw mut seen).cast()),
            PONKO_OK
        );
        assert_eq!(ponko_game_input_scenario(game, s), PONKO_OK);
        let world = run_game(game, 5, |ctx| build_hud(ctx));
        let mut report: PonkoScenarioReport = std::mem::zeroed();
        assert_eq!(ponko_world_scenario_report(world, &mut report), PONKO_OK);
        assert_eq!(report.status, PonkoScenarioStatus::PONKO_SCENARIO_PASSED as u32);
        assert_eq!(report.inputs_applied, 2);
        ponko_world_free(world);
        ponko_input_scenario_free(s);
        let whole = rect(0.0, 0.0, 844.0, 390.0);
        let left = rect(47.0, 0.0, 844.0, 369.0);
        let right = rect(0.0, 0.0, 797.0, 369.0);
        let areas: Vec<PonkoRect> = seen.frames.iter().map(|f| f.0).collect();
        assert_eq!(areas, [whole, left, left, right, right]);
        let huds: Vec<PonkoRect> = seen.frames.iter().map(|f| f.1).collect();
        assert_eq!(huds, areas, "the HUD is laid out with them in the frame of the tick");
        assert_eq!(
            ponko_input_scenario_safe_insets(null_mut(), 1, PonkoSafeInsets::default()),
            PONKO_INVALID_ARGUMENT
        );
    }
}
