//! The retained UI's animation through the C API, in headless runs: state
//! transitions from the theme, screen transitions, tweens and timelines,
//! the drawn state, the UI clock and reduced motion.
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

type Ctx = *mut PonkoCtx;

const SCREEN: PonkoVec2 = PonkoVec2 { x: 800.0, y: 600.0 };
const S: u64 = PONKO_UI_SCREEN;

struct Scenario {
    body: unsafe fn(Ctx),
    failure: Option<String>,
}

unsafe extern "C" fn trampoline(ctx: Ctx, user: *mut c_void) {
    let scenario = unsafe { &mut *user.cast::<Scenario>() };
    let body = scenario.body;
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| unsafe { body(ctx) })) {
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()));
        scenario.failure = Some(message.unwrap_or_else(|| "panicked".into()));
    }
}

/// Runs `body` once as a sim system of a headless game and reports its failure.
fn run(body: unsafe fn(Ctx)) {
    let mut scenario = Scenario { body, failure: None };
    unsafe {
        let game = ponko_game_new(c"ui".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_sim_system(
                game,
                c"s".as_ptr(),
                1,
                Some(trampoline),
                (&mut scenario as *mut Scenario).cast()
            ),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 1, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    if let Some(failure) = scenario.failure {
        panic!("{failure}");
    }
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

#[track_caller]
fn ok(status: PonkoStatus) {
    assert_eq!(status, PONKO_OK, "{}", last_error());
}

unsafe fn id_of(status: impl FnOnce(*mut u64) -> PonkoStatus) -> u64 {
    let mut id = PONKO_UI_NONE;
    ok(status(&mut id));
    id
}

/// One frame `dt` seconds after the last, with the pointer at `pointer`.
unsafe fn frame(ctx: Ctx, dt: f32, pointer: PonkoVec2) {
    let mut i: PonkoUiInputEx = unsafe { std::mem::zeroed() };
    (i.struct_size, i.version) = (size_of::<PonkoUiInputEx>() as u32, PONKO_UI_INPUT_EX_VERSION);
    ok(unsafe { ponko_ui_input_ex_default(&mut i) });
    i.dt = dt;
    i.input.pointer = pointer;
    ok(unsafe { ponko_ctx_ui_frame_ex(ctx, S, SCREEN, &i) });
}

unsafe fn drawn(ctx: Ctx, id: u64) -> PonkoUiDrawn {
    let mut d = PonkoUiDrawn::default();
    ok(unsafe { ponko_ctx_ui_drawn(ctx, S, id, &mut d) });
    d
}

const AWAY: PonkoVec2 = PonkoVec2 { x: 790.0, y: 590.0 };

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn easing_curves_and_transitions_match_the_rust_ones() {
    let linear = ponko_ui_ease(PonkoUiEaseName::PONKO_UI_EASE_LINEAR as u32);
    let t = PonkoUiTransition {
        seconds: 0.4,
        ease: linear,
    };
    let mut k = 0.0;
    ok(unsafe { ponko_ui_transition_progress(&t, 0.1, &mut k) });
    assert!(close(k, 0.25));
    let back = ponko_ui_ease(PonkoUiEaseName::PONKO_UI_EASE_BACK_OUT as u32);
    assert!(back.y1 > 1.0, "overshoots");
    assert_eq!(ponko_ui_ease(99), linear, "an unknown name is linear");
    let bad = PonkoUiTransition {
        seconds: -1.0,
        ease: linear,
    };
    assert_eq!(
        unsafe { ponko_ui_transition_progress(&bad, 0.1, &mut k) },
        PONKO_INVALID_ARGUMENT
    );
    assert_eq!(
        unsafe { ponko_ui_transition_progress(null(), 0.1, &mut k) },
        PONKO_INVALID_ARGUMENT
    );
    assert_eq!(ponko_ui_transition(0.3).seconds, 0.3);
    let zoom = ponko_ui_screen_zoom(0.8, 0.2);
    assert_eq!((zoom.scale, zoom.opacity), (0.8, 1.0));
    let slide = ponko_ui_screen_slide(PonkoVec2 { x: 5.0, y: 0.0 }, 0.2);
    assert_eq!(slide.offset.x, 5.0);
}

#[test]
fn the_theme_carries_state_transitions_and_the_press_scale() {
    run(|ctx| unsafe {
        let mut theme: PonkoUiThemeEx = std::mem::zeroed();
        (theme.struct_size, theme.version) = (size_of::<PonkoUiThemeEx>() as u32, PONKO_UI_THEME_EX_VERSION);
        ok(ponko_ui_theme_ex_dark(&mut theme));
        assert_eq!((theme.transition.seconds, theme.press_scale), (0.0, 1.0));
        theme.transition = PonkoUiTransition {
            seconds: 0.2,
            ease: ponko_ui_ease(PonkoUiEaseName::PONKO_UI_EASE_LINEAR as u32),
        };
        theme.press_scale = 0.95;
        ok(ponko_ctx_ui_set_theme_ex(ctx, S, &theme));
        let mut back: PonkoUiThemeEx = std::mem::zeroed();
        (back.struct_size, back.version) = (size_of::<PonkoUiThemeEx>() as u32, PONKO_UI_THEME_EX_VERSION);
        ok(ponko_ctx_ui_theme_ex(ctx, S, &mut back));
        assert_eq!((back.transition, back.press_scale), (theme.transition, 0.95));

        let layout = ponko_ui_layout_fixed(200.0, 40.0);
        let root = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, PONKO_UI_NONE, &ponko_ui_layout_fill(), o));
        let button = id_of(|o| ponko_ctx_ui_add_button(ctx, S, root, c"Go".as_ptr(), &layout, o));
        ok(ponko_ctx_ui_push_screen_with_mode(ctx, S, root, 0));
        frame(ctx, 0.0, AWAY);
        let idle = drawn(ctx, button).fill;
        let mut r = PonkoRect::default();
        ok(ponko_ctx_ui_rect(ctx, S, button, &mut r));
        let over = PonkoVec2 {
            x: (r.min.x + r.max.x) / 2.0,
            y: (r.min.y + r.max.y) / 2.0,
        };
        frame(ctx, 0.0, over);
        frame(ctx, 0.1, over);
        let half = drawn(ctx, button).fill;
        frame(ctx, 0.2, over);
        let hovered = drawn(ctx, button).fill;
        assert_ne!(idle, hovered);
        assert!(close(half.r, (idle.r + hovered.r) / 2.0), "{half:?}");

        // A version 1 caller's theme: no transition, no press scale.
        let mut v1 = theme;
        v1.version = 1;
        ok(ponko_ctx_ui_set_theme_ex(ctx, S, &v1));
        ok(ponko_ctx_ui_theme_ex(ctx, S, &mut back));
        assert_eq!((back.transition.seconds, back.press_scale), (0.0, 1.0));
        theme.press_scale = f32::NAN;
        assert_eq!(ponko_ctx_ui_set_theme_ex(ctx, S, &theme), PONKO_INVALID_ARGUMENT);
    });
}

#[test]
fn screens_fade_on_and_off_and_take_no_input_while_they_go() {
    run(|ctx| unsafe {
        let layout = ponko_ui_layout_centered(300.0, 200.0);
        let menu = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, PONKO_UI_NONE, &layout, o));
        let mut fade = ponko_ui_screen_fade(0.2);
        fade.transition.ease = ponko_ui_ease(PonkoUiEaseName::PONKO_UI_EASE_LINEAR as u32);
        ok(ponko_ctx_ui_set_screen_transition(ctx, S, menu, &fade, &fade));
        let (mut enter, mut exit) = (PonkoUiScreenTransition::default(), PonkoUiScreenTransition::default());
        ok(ponko_ctx_ui_screen_transition(ctx, S, menu, &mut enter, &mut exit));
        assert_eq!((enter, exit), (fade, fade));
        ok(ponko_ctx_ui_push_screen_with_mode(ctx, S, menu, 1));
        frame(ctx, 0.1, AWAY);
        assert!(close(drawn(ctx, menu).opacity, 0.5));
        let mut animating = false;
        ok(ponko_ctx_ui_is_animating(ctx, S, menu, &mut animating));
        assert!(animating);
        frame(ctx, 0.1, AWAY);
        assert_eq!(drawn(ctx, menu).opacity, 1.0);
        let mut popped = PONKO_UI_NONE;
        ok(ponko_ctx_ui_pop_screen(ctx, S, &mut popped));
        frame(ctx, 0.05, AWAY);
        assert!(close(drawn(ctx, menu).opacity, 0.75));
        let mut shown = true;
        ok(ponko_ctx_ui_is_shown(ctx, S, menu, &mut shown));
        assert!(!shown, "popped at once, drawn while it goes");
        ok(ponko_ctx_ui_set_screen_transition(ctx, S, menu, null(), null()));
        ok(ponko_ctx_ui_screen_transition(ctx, S, menu, &mut enter, null_mut()));
        assert_eq!(enter.transition.seconds, 0.0);
        let mut bad = fade;
        bad.transition.ease.x1 = 2.0;
        assert_eq!(
            ponko_ctx_ui_set_screen_transition(ctx, S, menu, &bad, null()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_ui_set_screen_transition(ctx, S, 12345, null(), null()),
            PONKO_NOT_FOUND
        );
    });
}

#[test]
fn tweens_and_timelines_play_report_their_end_and_stop() {
    run(|ctx| unsafe {
        let root = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, PONKO_UI_NONE, &ponko_ui_layout_fill(), o));
        let card = id_of(|o| ponko_ctx_ui_add_panel(ctx, S, root, &ponko_ui_layout_fixed(100.0, 50.0), o));
        ok(ponko_ctx_ui_push_screen_with_mode(ctx, S, root, 0));
        let linear = ponko_ui_ease(PonkoUiEaseName::PONKO_UI_EASE_LINEAR as u32);

        // One tween, after a delay.
        let mut slide = ponko_ui_tween_defaults(PonkoUiProperty::PONKO_UI_PROPERTY_OFFSET as u32);
        (slide.start, slide.seconds, slide.ease) = (0.1, 0.2, linear);
        slide.to.point = PonkoVec2 { x: 40.0, y: 0.0 };
        ok(ponko_ctx_ui_animate(ctx, S, card, &slide));
        frame(ctx, 0.2, AWAY);
        let mut place = PonkoRect::default();
        ok(ponko_ctx_ui_rect(ctx, S, card, &mut place));
        assert!(close(drawn(ctx, card).rect.min.x - place.min.x, 20.0));
        frame(ctx, 0.2, AWAY);
        let mut e: PonkoUiEventEx = std::mem::zeroed();
        (e.struct_size, e.version) = (size_of::<PonkoUiEventEx>() as u32, PONKO_UI_EVENT_EX_VERSION);
        ok(ponko_ctx_ui_next_event_ex(ctx, S, &mut e));
        assert_eq!((e.kind, e.id), (PonkoUiEventKind::PONKO_UI_EVENT_ANIMATION_DONE, card));

        // A timeline: grow, then fade, twice.
        let mut grow = ponko_ui_tween_defaults(PonkoUiProperty::PONKO_UI_PROPERTY_WIDTH as u32);
        (grow.seconds, grow.ease, grow.has_from) = (0.5, linear, true);
        (grow.from.number, grow.to.number) = (100.0, 200.0);
        let mut tint = ponko_ui_tween_defaults(PonkoUiProperty::PONKO_UI_PROPERTY_BACKGROUND as u32);
        (tint.start, tint.seconds, tint.ease) = (0.5, 0.5, linear);
        tint.to.color = PonkoColor {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let steps = [grow, tint];
        let repeat = PonkoUiRepeat::PONKO_UI_REPEAT_TIMES as u32;
        ok(ponko_ctx_ui_play(ctx, S, card, steps.as_ptr(), 2, repeat, 2));
        frame(ctx, 0.25, AWAY);
        ok(ponko_ctx_ui_rect(ctx, S, card, &mut place));
        assert!(close(place.max.x - place.min.x, 150.0), "width half way");
        frame(ctx, 1.0, AWAY);
        assert_eq!(
            ponko_ctx_ui_next_event_ex(ctx, S, &mut e),
            PONKO_NOT_FOUND,
            "played once of twice"
        );
        frame(ctx, 1.0, AWAY);
        ok(ponko_ctx_ui_next_event_ex(ctx, S, &mut e));
        assert_eq!(e.kind, PonkoUiEventKind::PONKO_UI_EVENT_ANIMATION_DONE);

        // A loop, stopped where it is.
        let mut pulse = ponko_ui_tween_defaults(PonkoUiProperty::PONKO_UI_PROPERTY_SCALE as u32);
        (pulse.seconds, pulse.to.number) = (0.5, 1.2);
        let looped = PonkoUiRepeat::PONKO_UI_REPEAT_PING_PONG as u32;
        ok(ponko_ctx_ui_play(ctx, S, card, &pulse, 1, looped, 0));
        frame(ctx, 0.3, AWAY);
        ok(ponko_ctx_ui_stop_animations(ctx, S, card));
        let held = drawn(ctx, card).scale;
        frame(ctx, 0.3, AWAY);
        assert_eq!(drawn(ctx, card).scale, held);
        let mut animating = true;
        ok(ponko_ctx_ui_is_animating(ctx, S, card, &mut animating));
        assert!(!animating);

        // Bad input changes nothing.
        let mut bad = pulse;
        bad.property = 99;
        assert_eq!(ponko_ctx_ui_animate(ctx, S, card, &bad), PONKO_INVALID_ARGUMENT);
        bad = pulse;
        bad.seconds = -1.0;
        assert_eq!(ponko_ctx_ui_play(ctx, S, card, &bad, 1, 0, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_ui_play(ctx, S, card, &pulse, 1, 9, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_ui_play(ctx, S, card, null(), 1, 0, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_ui_animate(ctx, S, card, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_ui_animate(ctx, S, 4242, &pulse), PONKO_NOT_FOUND);
        ok(ponko_ctx_ui_play(ctx, S, card, null(), 0, 0, 0));
    });
}

#[test]
fn the_clock_runs_with_frames_and_reduced_motion_is_kept() {
    run(|ctx| unsafe {
        let mut seconds = -1.0;
        ok(ponko_ctx_ui_clock(ctx, S, &mut seconds));
        let start = seconds;
        frame(ctx, 0.25, AWAY);
        ok(ponko_ctx_ui_clock(ctx, S, &mut seconds));
        assert!((seconds - start - 0.25).abs() < 1e-9);
        let mut on = true;
        ok(ponko_ctx_ui_reduced_motion(ctx, S, &mut on));
        assert!(!on);
        ok(ponko_ctx_ui_set_reduced_motion(ctx, S, true));
        ok(ponko_ctx_ui_reduced_motion(ctx, S, &mut on));
        assert!(on);
    });
}
