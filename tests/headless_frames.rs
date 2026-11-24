//! Headless runs with frames (`Game::headless_frames`): frame systems and the
//! retained UI run without a window or GPU, and input scenarios click, type
//! and check the UI by node name.
use ponko::actions::InputEvent;
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck, ScenarioStatus};
use ponko::ui::{UiEvent, UiLayout};
use std::cell::RefCell;
use std::rc::Rc;

/// What ran, in order, as `(tick at the time, label)`.
type Log = Rc<RefCell<Vec<(u64, &'static str)>>>;

fn logger(log: &Log, label: &'static str) -> impl FnMut(&mut Ctx) + 'static {
    let log = log.clone();
    move |ctx: &mut Ctx| log.borrow_mut().push((ctx.time.sim_tick(), label))
}

fn phases(log: &Log) -> Game {
    Game::new("frames", 640, 480)
        .sim_system(logger(log, "sim"))
        .system(logger(log, "frame"))
        .add(Phase::Late, logger(log, "late"))
        .add(Phase::Draw, logger(log, "draw"))
}

#[test]
fn headless_frames_run_frame_late_and_draw_systems_once_per_tick_after_the_tick() {
    let log = Log::default();
    phases(&log).headless_frames(true).run_headless(2, |_| {});
    let expected = [
        (0, "sim"),
        (1, "frame"),
        (1, "late"),
        (1, "draw"),
        (1, "sim"),
        (2, "frame"),
        (2, "late"),
        (2, "draw"),
    ];
    assert_eq!(log.borrow().as_slice(), &expected);
}

#[test]
fn sim_only_headless_runs_still_skip_frame_systems() {
    let log = Log::default();
    phases(&log).run_headless(3, |_| {});
    assert_eq!(log.borrow().as_slice(), &[(0, "sim"), (1, "sim"), (2, "sim")]);
}

#[test]
fn the_closure_setup_returns_runs_every_headless_frame() {
    let frames = Rc::new(RefCell::new(0));
    let seen = frames.clone();
    Game::new("closure", 1, 1)
        .headless_frames(true)
        .run_headless(4, move |_| move |_: &mut Ctx| *seen.borrow_mut() += 1);
    assert_eq!(*frames.borrow(), 4);
}

#[test]
fn headless_frames_lay_the_ui_out_in_the_virtual_window() {
    let rect = Rc::new(RefCell::new(None));
    let out = rect.clone();
    Game::new("virtual window", 640, 480)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            assert_eq!(ctx.window_size(), vec2(640.0, 480.0));
            let root = ctx.ui.find("root").unwrap();
            *out.borrow_mut() = ctx.ui.rect(root);
        })
        .run_headless(1, |ctx| {
            let root = ctx.ui.root().layout(UiLayout::fill()).name("root").id();
            ctx.ui.push_modeless(root);
        });
    let rect = rect.borrow().unwrap();
    assert_eq!((rect.min, rect.max), (Vec2::ZERO, vec2(640.0, 480.0)));
}

/// A small form: a name field, an OK button and a modal confirmation that
/// covers both. `form_events` logs the UI's events with the tick.
fn form(ctx: &mut Ctx) {
    let root = ctx.ui.root().layout(UiLayout::fill().gap(8.0)).name("form").id();
    ctx.ui
        .text_input(root)
        .layout(UiLayout::fixed(200.0, 30.0))
        .name("form.name");
    ctx.ui
        .button(root, "OK")
        .layout(UiLayout::fixed(100.0, 30.0))
        .name("form.ok");
    ctx.ui
        .button(root, "Off")
        .layout(UiLayout::fixed(100.0, 30.0))
        .enabled(false)
        .name("form.off");
    ctx.ui.push_modeless(root);
    let modal = ctx
        .ui
        .root()
        .layout(UiLayout::centered(300.0, 100.0))
        .name("confirm")
        .id();
    ctx.ui
        .button(modal, "Yes")
        .layout(UiLayout::fixed(100.0, 30.0))
        .name("confirm.yes");
}

type Events = Rc<RefCell<Vec<(u64, UiEvent)>>>;

fn form_game(events: &Events) -> Game {
    let events = events.clone();
    Game::new("form", 640, 480)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick() - 1; // the frame's tick has run
            let confirm = ctx.ui.find("confirm").unwrap();
            while let Some(event) = ctx.ui.next_event() {
                events.borrow_mut().push((tick, event));
                // OK opens the modal confirmation; Yes closes it.
                if event.clicked() == ctx.ui.find("form.ok") {
                    ctx.ui.push_modal(confirm);
                } else if event.clicked() == ctx.ui.find("confirm.yes") {
                    ctx.ui.pop_screen();
                }
            }
        })
}

#[test]
fn scenarios_click_and_type_into_the_ui_by_name_and_check_the_tree() {
    let events = Events::default();
    let scenario = InputScenario::new()
        .expect(
            0,
            ScenarioCheck::ui("form.name")
                .exists(true)
                .shown(true)
                .focused(false)
                .text(""),
        )
        .expect(0, ScenarioCheck::ui("confirm.yes").exists(true).shown(false))
        .expect(0, ScenarioCheck::ui("nothing").exists(false))
        .type_text(2, "form.name", "Ada")
        .expect(2, ScenarioCheck::ui("form.name").focused(true).text("Ada"))
        .type_text(3, "form.name", " L.")
        .expect(3, ScenarioCheck::ui("form.name").text("Ada L."))
        .click(5, "form.ok")
        // The modal it opened has taken the focus...
        .expect(5, ScenarioCheck::ui("form.ok").focused(false))
        .expect(5, ScenarioCheck::ui("confirm.yes").shown(true))
        .click(7, "confirm.yes")
        .expect(7, ScenarioCheck::ui("confirm.yes").shown(false))
        // ...and gives it back when it closes.
        .expect(7, ScenarioCheck::ui("form.ok").focused(true))
        .duration(9);
    let (_world, report) = form_game(&events).run_headless_scenario(scenario, form);
    report.assert_passed();
    assert_eq!(report.inputs_applied, 4, "four UI steps");
    let seen: Vec<(u64, &str)> = events
        .borrow()
        .iter()
        .map(|&(tick, e)| {
            let kind = match e {
                UiEvent::Clicked(_) => "clicked",
                UiEvent::Changed(_) => "changed",
                UiEvent::Submitted(_) => "submitted",
                UiEvent::ScreenPopped(_) => "popped",
                _ => "other",
            };
            (tick, kind)
        })
        .collect();
    assert_eq!(
        seen,
        [(2, "changed"), (3, "changed"), (5, "clicked"), (7, "clicked")],
        "each step's events are there in the frame of its tick"
    );
}

#[test]
fn key_steps_reach_the_ui_in_the_frame_of_their_tick() {
    let events = Events::default();
    let scenario = InputScenario::new()
        .click(1, "form.ok")
        .expect(1, ScenarioCheck::ui("confirm").shown(true))
        // Escape pops the modal: the key is fed before the UI's frame.
        .at(3, InputEvent::Key(Key::Escape, true))
        .at(4, InputEvent::Key(Key::Escape, false))
        .expect(3, ScenarioCheck::ui("confirm").shown(false))
        // Tab moves the focus from OK to the name field and back (the
        // disabled button is skipped).
        .at(6, InputEvent::Key(Key::Tab, true))
        .at(7, InputEvent::Key(Key::Tab, false))
        .expect(6, ScenarioCheck::ui("form.name").focused(true))
        .duration(8);
    let (_world, report) = form_game(&events).run_headless_scenario(scenario, form);
    report.assert_passed();
    let popped: Vec<u64> = (events.borrow().iter())
        .filter(|(_, e)| matches!(e, UiEvent::ScreenPopped(_)))
        .map(|&(tick, _)| tick)
        .collect();
    assert_eq!(popped, [3]);
}

fn failure(scenario: InputScenario) -> (usize, u64, String) {
    let (_world, report) = form_game(&Events::default()).run_headless_scenario(scenario, form);
    assert_eq!(report.status, ScenarioStatus::Failed);
    let f = report.failure.expect("a failure");
    (f.step, f.tick, f.message)
}

#[test]
fn a_click_on_a_missing_hidden_covered_or_disabled_node_fails_the_run_at_its_tick() {
    let (step, tick, message) = failure(InputScenario::new().click(2, "form.cancel").duration(5));
    assert_eq!((step, tick), (0, 2));
    assert!(message.contains("no UI node is named `form.cancel`"), "{message}");

    let (_, _, message) = failure(InputScenario::new().click(2, "confirm.yes").duration(5));
    assert!(message.contains("`confirm.yes` is not on a shown screen"), "{message}");

    // The modal covers the form: its OK button can't be clicked any more.
    let scenario = InputScenario::new().click(1, "form.ok").click(3, "form.ok").duration(5);
    let (step, tick, message) = failure(scenario);
    assert_eq!((step, tick), (1, 3));
    assert!(message.contains("`form.ok` can't be clicked"), "{message}");

    let (_, _, message) = failure(InputScenario::new().click(1, "form.off").duration(5));
    assert!(message.contains("`form.off` can't be clicked"), "{message}");
}

#[test]
fn typing_into_a_node_that_is_not_a_text_input_fails_the_run() {
    let (_, tick, message) = failure(InputScenario::new().type_text(2, "form.ok", "x").duration(5));
    assert_eq!(tick, 2);
    assert!(message.contains("`form.ok` is not a text input"), "{message}");
}

#[test]
fn a_ui_check_that_does_not_hold_names_what_it_saw() {
    let scenario = InputScenario::new()
        .expect(1, ScenarioCheck::ui("form.name").text("Ada").focused(true))
        .duration(3);
    let (_world, report) = form_game(&Events::default()).run_headless_scenario(scenario, form);
    let f = report.failure.expect("a failure");
    assert_eq!((f.step, f.tick, f.check.as_str()), (0, 1, "ui form.name"));
    assert_eq!(f.expected, serde_json::json!({"text": "Ada", "focused": true}));
    assert_eq!(f.actual, serde_json::json!({"text": "", "focused": false}));

    let scenario = InputScenario::new()
        .expect(1, ScenarioCheck::ui("form.gone").shown(true))
        .duration(3);
    let (_world, report) = form_game(&Events::default()).run_headless_scenario(scenario, form);
    assert!(
        report
            .failure
            .unwrap()
            .message
            .contains("no UI node is named `form.gone`")
    );
}

#[test]
fn ui_steps_on_one_tick_take_one_frame_each() {
    let events = Events::default();
    let scenario = InputScenario::new()
        .type_text(2, "form.name", "a")
        .type_text(2, "form.name", "b")
        .expect(2, ScenarioCheck::ui("form.name").text("a"))
        .expect(3, ScenarioCheck::ui("form.name").text("ab"))
        .duration(5);
    let (_world, report) = form_game(&events).run_headless_scenario(scenario, form);
    report.assert_passed();
}

#[test]
fn ui_steps_and_checks_round_trip_through_json() {
    let scenario = InputScenario::new()
        .click(1, "menu.play")
        .type_text(2, "chat", "hi")
        .expect(3, ScenarioCheck::ui("menu").shown(false).text("x"));
    let json = scenario.to_json();
    let steps = json["steps"].as_array().unwrap();
    assert_eq!(
        steps[0],
        serde_json::json!({"type": "ui_click", "tick": 1, "node": "menu.play"})
    );
    assert_eq!(
        steps[1],
        serde_json::json!({"type": "ui_type", "tick": 2, "node": "chat", "text": "hi"})
    );
    assert_eq!(
        steps[2],
        serde_json::json!({"type": "expect_ui", "tick": 3, "node": "menu", "shown": false, "text": "x"})
    );
    let again = InputScenario::from_value(&json).unwrap();
    assert_eq!(again.to_json(), json);
    assert_eq!(again.ticks(), 4);

    let empty = r#"{ "version": 1, "steps": [{ "tick": 0, "type": "expect_ui", "node": "menu" }] }"#;
    let error = InputScenario::from_json(empty).unwrap_err().to_string();
    assert!(error.contains("`expect_ui` needs at least one of"), "{error}");
}

#[test]
fn headless_frames_advance_elapsed_time_at_the_fixed_rate_by_default() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let frames = seen.clone();
    Game::new("headless time", 1, 1)
        .fixed_hz(50)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            frames
                .borrow_mut()
                .push((ctx.time.frame(), ctx.time.frame_dt(), ctx.time.elapsed()));
        })
        .run_headless(2, |_| {});
    assert_eq!(*seen.borrow(), [(1, 0.02, 0.02), (2, 0.02, 0.04)]);
}

#[test]
fn headless_frame_duration_advances_the_frame_clock_without_changing_simulation_dt() {
    use std::time::Duration;

    let seen = Rc::new(RefCell::new(Vec::new()));
    let frames = seen.clone();
    Game::new("custom headless time", 1, 1)
        .fixed_hz(50)
        .headless_frame_duration(Duration::from_millis(125))
        .system(move |ctx: &mut Ctx| {
            frames.borrow_mut().push((
                ctx.time.frame_dt(),
                ctx.time.elapsed(),
                ctx.time.sim_dt(),
                ctx.time.sim_tick(),
            ));
        })
        .run_headless(2, |_| {});
    assert_eq!(*seen.borrow(), [(0.125, 0.125, 0.02, 1), (0.125, 0.25, 0.02, 2)]);
}
