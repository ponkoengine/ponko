//! Desktops: a Mac trackpad's gestures feed the touch gestures, and
//! recordings keep fingers and trackpad gestures, so a replay plays what a
//! person did on a touch screen or a trackpad. Headless, fed as events.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::input::{GestureAxis, TouchPhase};
use ponko::prelude::*;
use ponko::scenario::InputScenario;

fn trackpad(gesture: Gesture, phase: TouchPhase, amount: f32) -> InputEvent {
    InputEvent::Trackpad { gesture, phase, amount }
}

#[test]
fn a_trackpad_pinch_and_rotation_drive_the_gestures_a_touch_screen_does() {
    Game::new("trackpad", 960, 540).run_headless(0, |ctx| {
        ctx.input.set_action(
            "zoom",
            Action::Axis(vec![AxisBinding::Gesture {
                axis: GestureAxis::Pinch,
                scale: 1.0,
            }]),
        );
        ctx.input.set_action(
            "turn",
            Action::Axis(vec![AxisBinding::Gesture {
                axis: GestureAxis::Rotate,
                scale: 1.0,
            }]),
        );
        ctx.input
            .set_action("zooming", Action::Button(vec![ButtonBinding::Gesture(Gesture::Pinch)]));
        ctx.input.set_action(
            "reset",
            Action::Button(vec![ButtonBinding::Gesture(Gesture::DoubleTap)]),
        );
        ctx.input.feed(InputEvent::MouseMove(vec2(300.0, 200.0)));
        ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Started, 0.1));
        ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Moved, 0.1));
        assert!((ctx.input.pinch_scale() - 1.21).abs() < 1e-5);
        assert!((ctx.input.action_axis("zoom") - 0.21).abs() < 1e-5);
        assert!(ctx.input.action_pressed("zooming") && ctx.input.action_down("zooming"));
        let at = ctx.input.gesture_events()[0];
        assert_eq!(
            (at.gesture, at.position),
            (Gesture::Pinch, vec2(300.0, 200.0)),
            "at the pointer"
        );
        ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Ended, 0.0));
        ctx.input.feed(trackpad(Gesture::Rotate, TouchPhase::Started, 0.3));
        assert!((ctx.input.rotation_delta() - 0.3).abs() < 1e-6);
        assert!((ctx.input.action_axis("turn") - 0.3).abs() < 1e-6);
        ctx.input.feed(trackpad(Gesture::DoubleTap, TouchPhase::Ended, 0.0));
        assert!(ctx.input.action_pressed("reset"));
        // Garbage from the system changes nothing.
        ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Moved, f32::NAN));
        assert!(ctx.input.pinch_scale().is_finite());
        assert!(!ctx.input.touch_device(), "a trackpad is no touch screen");
        assert!(ctx.input.touches().is_empty());
    });
}

#[test]
fn a_trackpad_pinch_holds_its_gesture_until_the_fingers_lift() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    Game::new("trackpad", 960, 540)
        .sim_system(move |ctx: &mut Ctx| {
            match ctx.time.sim_tick() {
                1 => ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Started, 0.1)),
                3 => ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Ended, 0.0)),
                _ => {}
            }
            log.borrow_mut().push(ctx.input.action_down("zooming"));
        })
        .run_headless(5, |ctx| {
            ctx.input
                .set_action("zooming", Action::Button(vec![ButtonBinding::Gesture(Gesture::Pinch)]));
        });
    assert_eq!(*seen.borrow(), [false, true, true, false, false]);
}

#[test]
fn a_recording_keeps_fingers_and_trackpad_gestures_and_replays_them() {
    #[derive(Default)]
    struct Recorded(Option<InputScenario>);
    let world = Game::new("record", 960, 540)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            1 => ctx.input.feed(InputEvent::touch_start(1, vec2(100.0, 100.0))),
            2 => ctx.input.feed(InputEvent::touch_end(1, vec2(100.0, 100.0))),
            3 => ctx.input.feed(trackpad(Gesture::Pinch, TouchPhase::Started, 0.5)),
            5 => {
                let scenario = ctx.input.stop_recording();
                ctx.world.insert_resource(Recorded(scenario));
            }
            _ => {}
        })
        .run_headless(6, |ctx| ctx.input.start_recording());
    let scenario = world.resource::<Recorded>().0.clone().unwrap();
    assert_eq!(scenario.step_count(), 3, "the finger down, up and the pinch");
    // Through the scenario file and back, then played.
    let scenario = InputScenario::from_json(&scenario.to_json().to_string()).unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    let (_, report) = Game::new("replay", 960, 540)
        .sim_system(move |ctx: &mut Ctx| {
            let gestures: Vec<Gesture> = ctx.input.gesture_events().iter().map(|e| e.gesture).collect();
            log.borrow_mut().extend(gestures);
        })
        .run_headless_scenario(scenario, |_| {});
    report.assert_passed();
    assert_eq!(*seen.borrow(), [Gesture::Tap, Gesture::Pinch]);
}
