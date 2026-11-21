//! The `safe_insets` scenario step: a headless run feeds the insets a phone
//! reports on a tick, so a layout for a notched phone is checked without a
//! device. The UI lays out with them in the frame of that tick, they stay
//! until another step changes them, and the JSON form (what the control
//! endpoint takes) says the same.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};
use ponko::window::SafeInsets;

/// Per frame: the window's safe area, the HUD root's rect and the score's.
type Frames = Rc<RefCell<Vec<(Rect, Rect, Rect)>>>;

fn run(scenario: InputScenario) -> Frames {
    let frames = Frames::default();
    let out = frames.clone();
    let (_world, report) = Game::new("notch", 844, 390)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let rect = |name: &str| ctx.ui.rect(ctx.ui.find(name).unwrap()).unwrap();
            out.borrow_mut()
                .push((ctx.window.safe_area(), rect("hud"), rect("hud.score")));
        })
        .run_headless_scenario(scenario, |ctx| {
            let hud = ctx
                .ui
                .root()
                .layout(UiLayout::fill())
                .name("hud")
                .inside_safe_area()
                .id();
            ctx.ui
                .label(hud, "Score 0")
                .name("hud.score")
                .layout(UiLayout::fixed(120.0, 30.0));
            ctx.ui.show_hud(hud);
        });
    report.assert_passed();
    frames
}

#[test]
fn a_safe_insets_step_feeds_the_window_and_the_ui_lays_out_with_it_that_tick() {
    let scenario = InputScenario::new()
        // A phone held sideways, the notch on the left...
        .safe_insets(1, SafeInsets::new(0.0, 0.0, 21.0, 47.0))
        // ...then turned the other way.
        .safe_insets(3, SafeInsets::new(0.0, 47.0, 21.0, 0.0))
        .expect(1, ScenarioCheck::ui("hud.score").exists(true))
        .duration(5);
    let frames = run(scenario);
    let frames = frames.borrow();
    let window = Rect::new(0.0, 0.0, 844.0, 390.0);
    assert_eq!(frames.len(), 5);
    for (tick, expected) in [
        (0, window),
        (1, Rect::new(47.0, 0.0, 797.0, 369.0)),
        (2, Rect::new(47.0, 0.0, 797.0, 369.0)),
        (3, Rect::new(0.0, 0.0, 797.0, 369.0)),
        (4, Rect::new(0.0, 0.0, 797.0, 369.0)),
    ] {
        let (safe, hud, score) = frames[tick];
        assert_eq!(safe, expected, "tick {tick}: the insets stay until the next step");
        assert_eq!(hud, expected, "tick {tick}: the HUD fills the safe area in that frame");
        assert_eq!(score.min, expected.min, "tick {tick}: the score keeps to its corner");
    }
}

#[test]
fn a_safe_insets_step_counts_as_a_step_and_lasts_the_scenario_to_its_tick() {
    let scenario = InputScenario::new().safe_insets(7, SafeInsets::new(47.0, 0.0, 34.0, 0.0));
    assert_eq!(scenario.step_count(), 1);
    assert_eq!(scenario.ticks(), 8);
}

#[test]
fn safe_insets_steps_read_and_write_json_with_missing_sides_as_zero() {
    let scenario = InputScenario::from_json(
        r#"{
            "version": 1,
            "steps": [
                { "tick": 0, "type": "safe_insets", "top": 47, "bottom": 34 },
                { "tick": 2, "type": "safe_insets", "top": 0, "right": 0, "bottom": 21, "left": 47 }
            ]
        }"#,
    )
    .unwrap();
    assert_eq!(scenario.step_count(), 2);
    let json = scenario.to_json();
    let again = InputScenario::from_value(&json).unwrap();
    assert_eq!(again.to_json(), json, "the same scenario comes back");
    assert_eq!(json["steps"][0]["right"], 0.0, "a side left out is 0");
    assert_eq!(json["steps"][1]["left"], 47.0);
    assert!(
        InputScenario::from_json(r#"{"version":1,"steps":[{"tick":0,"type":"safe_insets","tpo":1}]}"#).is_err(),
        "a typo is an error, not a silent zero"
    );
}

#[test]
fn a_json_safe_insets_step_runs_like_the_builder_step() {
    let scenario = InputScenario::from_json(
        r#"{ "version": 1, "duration": 3,
             "steps": [{ "tick": 1, "type": "safe_insets", "top": 0, "right": 0, "bottom": 21, "left": 47 }] }"#,
    )
    .unwrap();
    let frames = run(scenario);
    let frames = frames.borrow();
    assert_eq!(frames[0].0, Rect::new(0.0, 0.0, 844.0, 390.0));
    assert_eq!(frames[1].0, Rect::new(47.0, 0.0, 797.0, 369.0));
    assert_eq!(frames[1].1, frames[1].0, "the HUD follows it");
}
