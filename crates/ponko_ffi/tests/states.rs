//! Game states from C: a title, a level and a pause menu pushed over it.
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;

use PonkoStatus::*;

const TITLE: u32 = 0;
const PLAYING: u32 = 1;
const PAUSED: u32 = 2;

#[derive(Default)]
struct Run {
    /// `(what, state, tick)` for every enter and exit.
    log: Vec<(&'static str, u32, u64)>,
    /// Ticks the level ran, and ticks it was drawn (also under the pause).
    played: u32,
    drawn: u32,
    /// What `ponko_ctx_state_in_stack` said about PLAYING on tick 12.
    under_pause: bool,
}

fn run_of(user: *mut c_void) -> &'static mut Run {
    // SAFETY: every callback gets the test's `Run`.
    unsafe { &mut *user.cast::<Run>() }
}

unsafe fn note(ctx: *mut PonkoCtx, user: *mut c_void, what: &'static str) {
    let mut state = u32::MAX;
    unsafe {
        assert_eq!(ponko_ctx_state(ctx, &mut state), PONKO_OK);
        run_of(user).log.push((what, state, ponko_ctx_sim_tick(ctx)));
    }
}

unsafe extern "C" fn entered(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe { note(ctx, user, "enter") }
}

unsafe extern "C" fn left(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe { note(ctx, user, "exit") }
}

/// The menus: play on tick 2, pause on 10, resume on 15.
unsafe extern "C" fn menus(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        match ponko_ctx_sim_tick(ctx) {
            2 => assert_eq!(ponko_ctx_set_state(ctx, PLAYING), PONKO_OK),
            10 => assert_eq!(ponko_ctx_push_state(ctx, PAUSED), PONKO_OK),
            12 => {
                let mut found = false;
                assert_eq!(ponko_ctx_state_in_stack(ctx, PLAYING, &mut found), PONKO_OK);
                run_of(user).under_pause = found;
            }
            15 => assert_eq!(ponko_ctx_pop_state(ctx), PONKO_OK),
            _ => {}
        }
    }
}

unsafe extern "C" fn play(_: *mut PonkoCtx, user: *mut c_void) {
    run_of(user).played += 1;
}

unsafe extern "C" fn draw(_: *mut PonkoCtx, user: *mut c_void) {
    run_of(user).drawn += 1;
}

#[test]
fn c_games_change_state_at_the_end_of_the_tick_that_asks() {
    let mut run = Run::default();
    let user = (&raw mut run).cast::<c_void>();
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"states".as_ptr(), 1, 1);
        assert_eq!(ponko_game_state(g, TITLE), PONKO_OK);
        for s in [TITLE, PLAYING, PAUSED] {
            assert_eq!(ponko_game_on_enter(g, s, Some(entered), user), PONKO_OK);
            assert_eq!(ponko_game_on_exit(g, s, Some(left), user), PONKO_OK);
        }
        assert_eq!(
            ponko_game_sim_system(g, c"menus".as_ptr(), 1, Some(menus), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_add_system_in_state(g, 3, c"play".as_ptr(), 1, PLAYING, Some(play), user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_add_system_in_stack(g, 4, c"draw".as_ptr(), 1, PLAYING, Some(draw), user),
            PONKO_OK
        );
        assert_eq!(ponko_game_run_headless(g, 20, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!(
        run.log,
        [
            ("enter", TITLE, 0),
            ("exit", TITLE, 2),
            ("enter", PLAYING, 2),
            ("enter", PAUSED, 10),
            ("exit", PAUSED, 15),
        ]
    );
    // Playing on ticks 3-10 and 16-19; paused over it on 11-15.
    assert_eq!(run.played, 12);
    assert_eq!(run.drawn, 17);
    assert!(run.under_pause);
}

fn last_error() -> String {
    // SAFETY: the library keeps the message until the next call fails.
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

unsafe extern "C" fn nothing(_: *mut PonkoCtx, _: *mut c_void) {}

unsafe extern "C" fn ask_without_state(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let mut state = 0;
        let status = &mut *user.cast::<Vec<PonkoStatus>>();
        status.push(ponko_ctx_state(ctx, &mut state));
        status.push(ponko_ctx_set_state(ctx, 1));
        status.push(ponko_ctx_pop_state(ctx));
    }
}

#[test]
fn state_calls_without_a_state_machine_fail_with_a_hint() {
    unsafe {
        let g = ponko_game_new(c"no states".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_on_enter(g, 0, Some(nothing), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("ponko_game_state first"), "{}", last_error());
        assert_eq!(
            ponko_game_add_system_in_state(g, 3, null_mut(), 1, 0, Some(nothing), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_game_state(g, 0), PONKO_OK);
        assert_eq!(ponko_game_state(g, 1), PONKO_INVALID_ARGUMENT, "one state machine");
        assert_eq!(ponko_game_on_exit(g, 0, None, null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_game_add_system_in_stack(g, 5, null_mut(), 2, 0, Some(nothing), null_mut()),
            PONKO_INVALID_ARGUMENT,
            "frame phases take no interval"
        );
        ponko_game_free(g);

        let mut statuses: Vec<PonkoStatus> = Vec::new();
        let mut world = null_mut();
        let g = ponko_game_new(c"no states".as_ptr(), 1, 1);
        let user = (&raw mut statuses).cast::<c_void>();
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(ask_without_state), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
        assert_eq!(statuses, [PONKO_INVALID_ARGUMENT; 3]);
        assert_eq!(ponko_ctx_push_state(null_mut(), 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_state_in_stack(null_mut(), 0, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    }
}

/// Runs on even ticks only.
unsafe extern "C" fn even_tick(ctx: *mut PonkoCtx, user: *mut c_void) -> bool {
    unsafe {
        *user.cast::<u32>() += 1;
        ponko_ctx_sim_tick(ctx).is_multiple_of(2)
    }
}

unsafe extern "C" fn count(_: *mut PonkoCtx, user: *mut c_void) {
    unsafe { *user.cast::<u32>() += 1 }
}

#[test]
fn a_system_with_a_condition_callback_runs_only_when_it_returns_true() {
    let (mut checked, mut ran) = (0u32, 0u32);
    let mut world = null_mut();
    unsafe {
        let g = ponko_game_new(c"conditions".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_add_system_if(
                g,
                3,
                c"every other tick".as_ptr(),
                1,
                Some(even_tick),
                (&raw mut checked).cast(),
                Some(count),
                (&raw mut ran).cast(),
            ),
            PONKO_OK,
            "no state machine needed"
        );
        let mut plan = vec![0u8; 1 << 16];
        let mut len = 0;
        assert_eq!(
            ponko_game_schedule_plan(g, plan.as_mut_ptr().cast(), plan.len(), &mut len),
            PONKO_OK
        );
        let plan = String::from_utf8_lossy(&plan[..len]).into_owned();
        let line = plan
            .lines()
            .find(|l| l.contains("every other tick"))
            .expect("in the plan");
        assert!(line.contains("alone"), "a C system with a condition runs alone: {line}");
        assert_eq!(ponko_game_run_headless(g, 10, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert_eq!((checked, ran), (10, 5));
}

#[test]
fn a_null_condition_callback_is_refused() {
    unsafe {
        let g = ponko_game_new(c"conditions".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_add_system_if(g, 3, null_mut(), 1, None, null_mut(), Some(nothing), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("condition is null"), "{}", last_error());
        ponko_game_free(g);
    }
}

/// The pause menu's screen: a panel with one focused button that closes it.
unsafe extern "C" fn build_pause(ctx: *mut PonkoCtx, user: *mut c_void) -> u64 {
    let (mut root, mut back) = (PONKO_UI_NONE, PONKO_UI_NONE);
    unsafe {
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, std::ptr::null(), &mut root),
            PONKO_OK
        );
        let resume = c"Resume".as_ptr();
        assert_eq!(
            ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, resume, std::ptr::null(), &mut back),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_set_closes_screen(ctx, PONKO_UI_SCREEN, back, true),
            PONKO_OK
        );
        let mut on = false;
        assert_eq!(
            ponko_ctx_ui_closes_screen(ctx, PONKO_UI_SCREEN, back, &mut on),
            PONKO_OK
        );
        assert!(on);
        assert_eq!(
            ponko_ctx_ui_set_initial_focus(ctx, PONKO_UI_SCREEN, back, true),
            PONKO_OK
        );
        *user.cast::<u32>() += 1;
    }
    root
}

/// `(tick, state, top screen is up)` every frame.
unsafe extern "C" fn watch_screen(ctx: *mut PonkoCtx, user: *mut c_void) {
    let (mut state, mut top) = (u32::MAX, 0);
    unsafe {
        assert_eq!(ponko_ctx_state(ctx, &mut state), PONKO_OK);
        assert_eq!(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &mut top), PONKO_OK);
        let seen = &mut *user.cast::<Vec<(u64, u32, bool)>>();
        seen.push((ponko_ctx_sim_tick(ctx), state, top != PONKO_UI_NONE));
    }
}

unsafe extern "C" fn pause_on_tick_two(ctx: *mut PonkoCtx, _: *mut c_void) {
    if unsafe { ponko_ctx_sim_tick(ctx) } == 2 {
        assert_eq!(unsafe { ponko_ctx_push_state(ctx, PAUSED) }, PONKO_OK);
    }
}

#[test]
fn a_state_screen_is_up_while_its_state_lasts_and_its_closing_button_pops_the_state() {
    let (mut built, mut seen) = (0u32, Vec::<(u64, u32, bool)>::new());
    unsafe {
        let s = ponko_input_scenario_new();
        let enter = PonkoKey::PONKO_KEY_ENTER as u32;
        assert_eq!(ponko_input_scenario_at(s, 6, 0, 0, enter, 1.0), PONKO_OK);
        assert_eq!(ponko_input_scenario_at(s, 7, 0, 0, enter, 0.0), PONKO_OK);
        let g = ponko_game_new(c"state screen".as_ptr(), 800, 600);
        assert_eq!(ponko_game_state(g, PLAYING), PONKO_OK);
        assert_eq!(ponko_game_headless_frames(g, true), PONKO_OK);
        let modal = PonkoUiScreenMode::PONKO_UI_SCREEN_MODE_MODAL as u32;
        let built_user = (&raw mut built).cast::<c_void>();
        assert_eq!(
            ponko_game_state_screen(g, PAUSED, modal, Some(build_pause), built_user),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(g, c"pause".as_ptr(), 1, Some(pause_on_tick_two), null_mut()),
            PONKO_OK
        );
        let seen_user = (&raw mut seen).cast::<c_void>();
        assert_eq!(
            ponko_game_system(g, c"watch".as_ptr(), Some(watch_screen), seen_user),
            PONKO_OK
        );
        assert_eq!(ponko_game_input_scenario(g, s), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 12, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
        ponko_input_scenario_free(s);
    }
    assert_eq!(built, 1, "built once, on entering Paused");
    let at = |tick: u64| *seen.iter().rev().find(|(t, ..)| *t == tick).unwrap();
    assert_eq!(at(1), (1, PLAYING, false));
    assert_eq!(at(4), (4, PAUSED, true), "pushed on tick 2, up from the next frame");
    assert_eq!(
        at(10),
        (10, PLAYING, false),
        "Enter on the focused Resume closed it and left Paused"
    );
}

#[test]
fn state_screen_refuses_a_missing_builder_an_unknown_mode_and_a_game_without_states() {
    unsafe {
        let modal = PonkoUiScreenMode::PONKO_UI_SCREEN_MODE_MODAL as u32;
        let g = ponko_game_new(c"refused".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_state_screen(g, PAUSED, modal, Some(build_pause), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("state"), "{}", last_error());
        assert_eq!(ponko_game_state(g, PLAYING), PONKO_OK);
        assert_eq!(
            ponko_game_state_screen(g, PAUSED, modal, None, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_state_screen(g, PAUSED, 99, Some(build_pause), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_state_screen(null_mut(), PAUSED, modal, Some(build_pause), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        ponko_game_free(g);
    }
}

unsafe extern "C" fn build_nothing(_: *mut PonkoCtx, _: *mut c_void) -> u64 {
    PONKO_UI_NONE
}

#[test]
fn a_builder_that_returns_no_root_gets_an_empty_screen() {
    let mut seen = Vec::<(u64, u32, bool)>::new();
    unsafe {
        let g = ponko_game_new(c"empty screen".as_ptr(), 800, 600);
        assert_eq!(ponko_game_state(g, PAUSED), PONKO_OK);
        assert_eq!(ponko_game_headless_frames(g, true), PONKO_OK);
        let modeless = PonkoUiScreenMode::PONKO_UI_SCREEN_MODE_MODELESS as u32;
        assert_eq!(
            ponko_game_state_screen(g, PAUSED, modeless, Some(build_nothing), null_mut()),
            PONKO_OK
        );
        let seen_user = (&raw mut seen).cast::<c_void>();
        assert_eq!(
            ponko_game_system(g, c"watch".as_ptr(), Some(watch_screen), seen_user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(g, 3, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    assert!(seen.iter().all(|(_, state, up)| *state == PAUSED && *up), "{seen:?}");
}
