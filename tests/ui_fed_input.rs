//! Input a system feeds (a replay, a test, a bot) reaches the retained UI
//! in the frame it was fed: the UI takes it before the next system runs.
//! Headless frames, so the UI runs as in a window.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

/// A full-window screen with a "play" button in its middle (240..720,
/// 135..405) and a text field at the top left (10..210, 10..50).
fn menu(ctx: &mut Ctx) {
    let root = ctx.ui.root().layout(UiLayout::fill()).id();
    ctx.ui
        .button(root, "Play")
        .name("play")
        .layout(UiLayout::anchored(0.25, 0.25, 0.5, 0.5));
    ctx.ui.text_input(root).name("name").layout(
        UiLayout::anchored(0.0, 0.0, 0.0, 0.0)
            .left(10.0)
            .top(10.0)
            .width(200.0)
            .height(40.0),
    );
    ctx.ui.push_modeless(root);
}

const PLAY: Vec2 = Vec2::new(480.0, 270.0);

/// The frames in which a system read a click on "play".
fn clicks_of(ctx: &mut Ctx, log: &RefCell<Vec<u64>>) {
    let play = ctx.ui.find("play").unwrap();
    let frame = ctx.time.frame();
    for event in ctx.ui.events() {
        if event == UiEvent::Clicked(play) {
            log.borrow_mut().push(frame);
        }
    }
}

#[test]
fn a_tap_a_system_feeds_clicks_a_ui_button_in_the_same_frame() {
    let fed = Rc::new(RefCell::new(Vec::new()));
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let (fed_log, click_log) = (fed.clone(), clicks.clone());
    Game::new("menu", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, move |ctx: &mut Ctx| {
            let frame = ctx.time.frame();
            let finger = match frame {
                3 => 1,
                6 => 2,
                _ => return,
            };
            // Down and up within one system: a tap.
            ctx.input.feed(InputEvent::touch_start(finger, PLAY));
            ctx.input.feed(InputEvent::touch_end(finger, PLAY));
            fed_log.borrow_mut().push(frame);
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| clicks_of(ctx, &click_log))
        .run_headless(8, menu);
    assert_eq!(fed.borrow().len(), 2);
    assert_eq!(
        *clicks.borrow(),
        *fed.borrow(),
        "each tap clicked once, in the frame it was fed"
    );
}

#[test]
fn a_click_and_typing_a_system_feeds_reach_the_ui_before_the_next_system() {
    let seen = Rc::new(RefCell::new(None));
    let log = seen.clone();
    Game::new("menu", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, |ctx: &mut Ctx| {
            if ctx.time.frame() == 2 {
                // A bot clicks the name field with the mouse and types.
                ctx.input.feed(InputEvent::MouseMove(vec2(60.0, 30.0)));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
            }
        })
        .add(Phase::Input, |ctx: &mut Ctx| {
            if ctx.time.frame() == 2 {
                let field = ctx.ui.find("name").unwrap();
                assert_eq!(ctx.ui.focus(), Some(field), "focused by the click of the system before");
                ctx.input.insert_text("Ada");
            }
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            if ctx.time.frame() == 2 {
                let field = ctx.ui.find("name").unwrap();
                *log.borrow_mut() = ctx.ui.text(field).map(str::to_string);
            }
        })
        .run_headless(3, menu);
    assert_eq!(
        seen.borrow().as_deref(),
        Some("Ada"),
        "typed into the field in the same frame"
    );
}

#[test]
fn a_click_the_last_system_feeds_is_read_by_the_next_frames_systems_once() {
    let fed = Rc::new(RefCell::new(Vec::new()));
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let (fed_log, click_log) = (fed.clone(), clicks.clone());
    Game::new("menu", 960, 540)
        .headless_frames(true)
        // The first system reads; the last one feeds.
        .add(Phase::Input, move |ctx: &mut Ctx| clicks_of(ctx, &click_log))
        .add(Phase::Draw, move |ctx: &mut Ctx| {
            if ctx.time.frame() == 3 {
                ctx.input.feed(InputEvent::MouseMove(PLAY));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
                fed_log.borrow_mut().push(ctx.time.frame());
            }
        })
        .run_headless(7, menu);
    let play_pressed_in = fed.borrow()[0];
    assert_eq!(
        *clicks.borrow(),
        [play_pressed_in + 1],
        "the UI clicked in the frame it was fed; the first system after it reads it, once"
    );
}

#[test]
fn input_the_ui_took_at_the_frame_start_is_not_taken_again() {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let log = clicks.clone();
    // The scenario's tap comes before the frame, as a window's does.
    let scenario = InputScenario::new().tap(3, PLAY).duration(6);
    let (_, report) = Game::new("menu", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, |ctx: &mut Ctx| {
            // Every frame a bot also presses a key the UI doesn't use.
            ctx.input
                .feed(InputEvent::Key(Key::F, ctx.time.frame().is_multiple_of(2)));
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| clicks_of(ctx, &log))
        .run_headless_scenario(scenario, menu);
    report.assert_passed();
    assert_eq!(clicks.borrow().len(), 1, "one tap, one click: {:?}", clicks.borrow());
}

#[test]
fn fed_input_that_changes_nothing_lays_nothing_out() {
    let passes = Rc::new(RefCell::new(Vec::new()));
    let log = passes.clone();
    Game::new("menu", 960, 540)
        .headless_frames(true)
        .add(Phase::Input, |ctx: &mut Ctx| {
            // A bot taps beside every widget each frame.
            ctx.input.feed(InputEvent::touch_start(1, vec2(900.0, 500.0)));
            ctx.input.feed(InputEvent::touch_end(1, vec2(900.0, 500.0)));
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            log.borrow_mut().push(ctx.ui.layout_passes())
        })
        .run_headless(8, menu);
    let passes = passes.borrow();
    assert_eq!(passes[2], passes[7], "still frames lay nothing out: {passes:?}");
}
