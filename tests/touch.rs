//! Touch input through the public API: gestures bound in the action map,
//! the on-screen controls, the retained UI under a finger, and input
//! scenarios that play by touch. Everything headless: the fingers are
//! `InputEvent::Touch` events, the path the window's touches take.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::actions::InputEvent;
use ponko::input::{GestureAxis, TouchPhase};
use ponko::prelude::*;
use ponko::scenario::{InputScenario, ScenarioCheck};
use ponko::ui::touch::{TouchControls, TouchShow};

fn map() -> ActionMap {
    let mut map = ActionMap::default();
    let gesture = |g| ButtonBinding::Gesture(g);
    map.set(
        "jump",
        Action::Button(vec![ButtonBinding::Key(Key::Space), gesture(Gesture::Tap)]),
    );
    map.set("dash", Action::Button(vec![gesture(Gesture::DoubleTap)]));
    map.set("menu", Action::Button(vec![gesture(Gesture::LongPress)]));
    map.set("back", Action::Button(vec![gesture(Gesture::SwipeLeft)]));
    map.set(
        "zoom",
        Action::Axis(vec![AxisBinding::Gesture {
            axis: GestureAxis::Pinch,
            scale: 1.0,
        }]),
    );
    map.set("look", Action::Axis2(vec![Axis2Binding::Drag { scale: 1.0 }]));
    map.set(
        "move",
        Action::Axis2(vec![Axis2Binding::Keys {
            left: Key::A,
            right: Key::D,
            up: Key::W,
            down: Key::S,
        }]),
    );
    map.set("fire", Action::Button(vec![ButtonBinding::Key(Key::F)]));
    map
}

/// One tick's reading of the actions.
#[derive(Debug, Clone, Default)]
struct Seen {
    tick: u64,
    pressed: Vec<&'static str>,
    down: Vec<&'static str>,
    zoom: f32,
    look: Vec2,
    movement: Vec2,
}

type Log = Rc<RefCell<Vec<Seen>>>;

/// Installs [`map`].
struct Bindings;
impl Plugin for Bindings {
    fn build(&self, game: Game) -> Game {
        game
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.input.set_actions(map());
    }
}

/// Installs [`controls`].
struct Controls;
impl Plugin for Controls {
    fn build(&self, game: Game) -> Game {
        game
    }
    fn setup(&self, ctx: &mut Ctx) {
        ctx.input.set_touch_controls(controls());
    }
}

/// A game with [`map`] whose simulation records what the actions read.
fn game(log: &Log) -> Game {
    let log = log.clone();
    Game::new("touch", 960, 540)
        .plugin(Bindings)
        .sim_system(move |ctx: &mut Ctx| {
            let names = ["jump", "dash", "menu", "back", "fire"];
            log.borrow_mut().push(Seen {
                tick: ctx.time.sim_tick(),
                pressed: names.into_iter().filter(|n| ctx.input.action_pressed(*n)).collect(),
                down: names.into_iter().filter(|n| ctx.input.action_down(*n)).collect(),
                zoom: ctx.input.action_axis("zoom"),
                look: ctx.input.action_axis2("look"),
                movement: ctx.input.action_axis2("move"),
            });
        })
}

fn pressed_at(log: &Log, action: &str) -> Vec<u64> {
    log.borrow()
        .iter()
        .filter(|s| s.pressed.contains(&action))
        .map(|s| s.tick)
        .collect()
}

#[test]
fn gestures_press_actions_bound_to_them_like_keys() {
    let log = Log::default();
    let at = vec2(480.0, 270.0);
    let scenario = InputScenario::new()
        .tap(2, at)
        .expect(2, ScenarioCheck::action("jump").pressed(true).down(true))
        .expect(3, ScenarioCheck::action("jump").down(false).released(true))
        // Two taps close together: a tap, then a double tap.
        .tap(40, at)
        .tap(44, at + vec2(8.0, 0.0))
        .touch_hold(60, 110, vec2(100.0, 100.0))
        .touch_drag(120, 124, vec2(400.0, 300.0), vec2(250.0, 300.0))
        // The key still works next to the gesture.
        .at(130, InputEvent::Key(Key::Space, true))
        .at(131, InputEvent::Key(Key::Space, false))
        .duration(135);
    let (_, report) = game(&log).run_headless_scenario(scenario, |_| {});
    report.assert_passed();
    assert_eq!(pressed_at(&log, "jump"), [2, 40, 130], "taps and Space");
    assert_eq!(pressed_at(&log, "dash"), [44], "the second tap is a double tap");
    let menu = pressed_at(&log, "menu");
    assert!(
        menu.len() == 1 && (89..=91).contains(&menu[0]),
        "a long press half a second (30 ticks) in: {menu:?}"
    );
    let held: Vec<u64> = log
        .borrow()
        .iter()
        .filter(|s| s.down.contains(&"menu"))
        .map(|s| s.tick)
        .collect();
    assert_eq!(held.last(), Some(&109), "held until the finger lifts on tick 110");
    assert_eq!(pressed_at(&log, "back"), [124], "a quick drag left is a swipe left");
}

#[test]
fn a_pinch_and_a_drag_bound_to_axes_read_their_change_each_frame() {
    let log = Log::default();
    let scenario = InputScenario::new()
        // Two fingers 200 apart spread to 300: zoom in.
        .at(1, InputEvent::touch_start(1, vec2(380.0, 270.0)))
        .at(1, InputEvent::touch_start(2, vec2(580.0, 270.0)))
        .at(2, InputEvent::touch_move(2, vec2(630.0, 270.0)))
        .at(3, InputEvent::touch_move(1, vec2(330.0, 270.0)))
        .at(5, InputEvent::touch_end(1, vec2(330.0, 270.0)))
        .at(5, InputEvent::touch_end(2, vec2(630.0, 270.0)))
        // One finger drags 100 down over 10 ticks.
        .touch_drag(20, 30, vec2(100.0, 100.0), vec2(100.0, 200.0))
        .duration(32);
    let (_, report) = game(&log).run_headless_scenario(scenario, |_| {});
    report.assert_passed();
    let log = log.borrow();
    let zoom = |tick: u64| log.iter().find(|s| s.tick == tick).unwrap().zoom;
    assert!((zoom(2) - 0.25).abs() < 1e-5, "200 to 250 apart: {}", zoom(2));
    assert!((zoom(3) - 0.2).abs() < 1e-5, "250 to 300 apart: {}", zoom(3));
    assert_eq!(zoom(4), 0.0, "no movement, no change");
    let look: Vec2 = log.iter().filter(|s| (20..=30).contains(&s.tick)).map(|s| s.look).sum();
    assert!(
        (look - vec2(0.0, 100.0)).length() < 1e-3,
        "the drag's frames add up to the finger's travel: {look}"
    );
    assert!(log.iter().all(|s| s.look.x == 0.0));
}

/// The on-screen controls of the tests: a stick for "move", a button for
/// "fire", a corner button for "menu".
fn controls() -> TouchControls {
    TouchControls::new()
        .stick("move")
        .button("fire", "F")
        .corner_button("menu", "II")
}

#[test]
fn the_onscreen_stick_and_button_drive_their_actions_at_the_same_time() {
    let log = Log::default();
    let shown = Rc::new(RefCell::new(Vec::new()));
    let seen = shown.clone();
    let game = game(&log)
        .headless_frames(true)
        .plugin(Controls)
        // Before the recording system: its tick sees what the fingers did.
        .add(Phase::Input, |ctx: &mut Ctx| {
            let stick = ctx.input.touch_control_center("move").unwrap();
            let fire = ctx.input.touch_control_center("fire").unwrap();
            match ctx.time.sim_tick() {
                // The first touch: this is a touch device, and the stick has it.
                2 => ctx.input.feed(InputEvent::touch_start(1, stick)),
                3 => ctx.input.feed(InputEvent::touch_move(1, stick + vec2(300.0, 0.0))),
                4 => ctx.input.feed(InputEvent::touch_start(2, fire)),
                // Both fingers wander off their controls: they keep them.
                6 => {
                    ctx.input.feed(InputEvent::touch_move(1, stick + vec2(0.0, -300.0)));
                    ctx.input.feed(InputEvent::touch_move(2, fire + vec2(-200.0, -100.0)));
                }
                8 => {
                    ctx.input.feed(InputEvent::touch_end(1, stick + vec2(0.0, -300.0)));
                    ctx.input.feed(InputEvent::touch_end(2, fire + vec2(-200.0, -100.0)));
                }
                _ => {}
            }
            assert!(
                ctx.input.gesture_events().is_empty(),
                "fingers on controls make no gestures"
            );
        })
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let node = ctx.ui.find("touch.fire").unwrap();
            seen.borrow_mut().push(ctx.ui.is_shown(node));
        });
    game.run_headless(10, |_| {});
    let log = log.borrow();
    let at = |tick: u64| log.iter().find(|s| s.tick == tick).unwrap().clone();
    assert_eq!(at(1).movement, Vec2::ZERO);
    assert_eq!(at(2).movement, Vec2::ZERO, "the finger is on the stick's centre");
    assert!((at(3).movement - vec2(1.0, 0.0)).length() < 1e-5, "{}", at(3).movement);
    assert!(at(4).pressed.contains(&"fire") && (at(4).movement - vec2(1.0, 0.0)).length() < 1e-5);
    assert!(
        (at(6).movement - vec2(0.0, -1.0)).length() < 1e-5,
        "the stick follows its finger"
    );
    assert!(at(7).down.contains(&"fire"), "the button keeps its finger");
    assert_eq!(at(8).movement, Vec2::ZERO, "a lifted stick centres");
    assert!(!at(8).down.contains(&"fire"));
    assert!(log.iter().all(|s| s.zoom == 0.0 && s.look == Vec2::ZERO));
    let shown = shown.borrow();
    assert!(!shown[0], "not a touch device until the first touch");
    assert!(shown[3], "shown from the frame after the first touch");
}

#[test]
fn onscreen_controls_show_only_on_touch_devices_by_default_and_the_mouse_never_presses_them() {
    let frames = Rc::new(RefCell::new(Vec::new()));
    let log = frames.clone();
    Game::new("touch", 960, 540)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            // Drawn from the frame after they were set; an empty set takes
            // them away by the next one.
            let up = ctx.ui.find("touch.controls").is_some_and(|root| ctx.ui.is_hud(root));
            log.borrow_mut().push((up, ctx.ui.find("touch.fire").is_some()));
            ctx.input.set_touch_controls(TouchControls::new());
            assert!(ctx.input.touch_control_center("fire").is_none());
            assert!(!ctx.input.touch_controls_shown());
        })
        .run_headless(2, |ctx| {
            ctx.input.set_actions(map());
            ctx.input.set_touch_controls(controls());
            assert!(!ctx.input.touch_controls_shown());
            let fire = ctx.input.touch_control_center("fire").unwrap();
            ctx.input.feed(InputEvent::MouseMove(fire));
            ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
            assert!(!ctx.input.action_down("fire"), "the mouse is not a finger");
            ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));

            // Told it is a touch device (a settings toggle): shown.
            ctx.input.set_touch_device(true);
            assert!(ctx.input.touch_controls_shown());
            ctx.input.set_touch_device(false);

            // Never: a finger on the button is just a tap.
            ctx.input.set_touch_controls_show(TouchShow::Never);
            ctx.input.feed(InputEvent::touch_start(1, fire));
            assert!(!ctx.input.action_down("fire"));
            assert!(!ctx.input.touches()[0].claimed());
            ctx.input.feed(InputEvent::touch_end(1, fire));
            assert!(ctx.input.action_pressed("jump"), "a tap");

            // Always: shown without a touch screen.
            ctx.input.set_touch_controls_show(TouchShow::Always);
            assert!(ctx.input.touch_controls_shown());
        });
    assert_eq!(*frames.borrow(), [(true, true), (false, false)]);
}

#[test]
fn onscreen_buttons_are_buttons_to_assistive_technology() {
    let checked = Rc::new(RefCell::new(false));
    let done = checked.clone();
    Game::new("touch", 960, 540)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let fire = ctx.ui.find("touch.fire").unwrap();
            let stick = ctx.ui.find("touch.move").unwrap();
            let tree = ctx.ui.accessibility_tree();
            let node = |id| tree.get(id).cloned();
            let fire = node(fire).expect("the button is in the accessibility tree");
            assert_eq!(fire.role, ponko::ui::UiRole::Button);
            assert_eq!(fire.name, "F");
            let stick = node(stick).expect("the stick is in the accessibility tree");
            assert_eq!(stick.role, ponko::ui::UiRole::Group);
            assert_eq!(stick.name, "move stick");
            *done.borrow_mut() = true;
        })
        .run_headless(1, |ctx| {
            ctx.input.set_touch_controls(controls().show(TouchShow::Always));
        });
    assert!(*checked.borrow());
}

#[test]
fn a_tap_on_a_ui_button_clicks_it() {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let log = clicks.clone();
    let scenario = InputScenario::new()
        // The button fills the middle of the window: (240..720, 135..405).
        .tap(3, vec2(480.0, 270.0))
        .tap(6, vec2(20.0, 20.0))
        .duration(8);
    let (_, report) = Game::new("menu", 960, 540)
        .headless_frames(true)
        .add(Phase::Frame, move |ctx: &mut Ctx| {
            let play = ctx.ui.find("play").unwrap();
            for event in ctx.ui.events() {
                if event == UiEvent::Clicked(play) {
                    log.borrow_mut().push(ctx.time.sim_tick());
                }
            }
        })
        .run_headless_scenario(scenario, |ctx| {
            let root = ctx.ui.root().layout(UiLayout::fill()).id();
            ctx.ui
                .button(root, "Play")
                .name("play")
                .layout(UiLayout::anchored(0.25, 0.25, 0.5, 0.5));
            ctx.ui.push_modeless(root);
        });
    report.assert_passed();
    // The frame system runs after the tick advanced.
    assert_eq!(
        *clicks.borrow(),
        [4],
        "the tap on it clicked it once; the one beside it did nothing"
    );
}

#[test]
fn touch_steps_round_trip_through_the_scenario_file() {
    let scenario = InputScenario::new()
        .touch_drag(1, 3, vec2(10.0, 20.0), vec2(30.0, 20.0))
        .at(
            5,
            InputEvent::Touch {
                id: 9,
                phase: TouchPhase::Cancelled,
                position: vec2(1.0, 2.0),
            },
        );
    let json = scenario.to_json();
    let steps = json["steps"].as_array().unwrap();
    assert_eq!(steps[0]["type"], "touch");
    assert_eq!(steps[0]["phase"], "started");
    assert_eq!(steps[3]["phase"], "cancelled");
    let back = InputScenario::from_json(&json.to_string()).unwrap();
    assert_eq!(back.to_json(), json);
    let file = r#"{ "version": 1, "steps": [
        { "tick": 2, "type": "touch", "id": 1, "phase": "started", "position": [100, 100] },
        { "tick": 2, "type": "touch", "id": 1, "phase": "ended", "position": [100, 100] },
        { "tick": 2, "type": "expect_action", "action": "jump", "pressed": true }
    ] }"#;
    let (_, report) = game(&Log::default()).run_headless_scenario(InputScenario::from_json(file).unwrap(), |_| {});
    report.assert_passed();
    assert!(InputScenario::from_json(r#"{ "version": 1, "steps": [{ "tick": 0, "type": "touch", "id": 1, "phase": "lifted", "position": [0, 0] }] }"#).is_err());
}

#[test]
fn a_failed_scenario_lifts_its_fingers_without_a_gesture() {
    let fingers = Rc::new(RefCell::new(Vec::new()));
    let log = fingers.clone();
    let scenario = InputScenario::new()
        .at(1, InputEvent::touch_start(1, vec2(100.0, 100.0)))
        .expect(3, ScenarioCheck::count("NotAComponent", 1))
        .duration(6);
    let world = game(&Log::default())
        .input_scenario(scenario)
        .sim_system(move |ctx: &mut Ctx| {
            log.borrow_mut()
                .push((ctx.input.touches().len(), ctx.input.action_pressed("jump")));
        })
        .run_headless(6, |_| {});
    let report = world.resource::<ponko::scenario::ScenarioReport>();
    assert!(!report.passed());
    assert_eq!(report.released, 1, "the finger was lifted");
    let fingers = fingers.borrow();
    assert_eq!(fingers[2], (1, false));
    assert_eq!(fingers[4], (0, false), "gone, and no tap");
}

#[test]
fn a_rebinding_screen_sees_a_gesture_as_what_was_pressed() {
    Game::new("rebind", 960, 540).run_headless(0, |ctx| {
        ctx.input.feed(InputEvent::touch_start(1, vec2(100.0, 100.0)));
        ctx.input.feed(InputEvent::touch_end(1, vec2(100.0, 100.0)));
        let pressed: Vec<PressedInput> = ctx.input.pressed_this_frame().collect();
        assert_eq!(pressed, [PressedInput::Button(ButtonBinding::Gesture(Gesture::Tap))]);
    });
}

#[test]
fn gesture_bindings_survive_the_action_map_file() {
    let json = map().to_json().unwrap();
    assert!(json.contains("\"Gesture\": \"DoubleTap\""), "{json}");
    assert_eq!(ActionMap::from_json(&json).unwrap(), map());
}

#[test]
fn onscreen_controls_step_aside_while_a_menu_screen_is_open() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = seen.clone();
    Game::new("menu over controls", 960, 540)
        .headless_frames(true)
        .plugin(Bindings)
        .add(Phase::Input, |ctx: &mut Ctx| {
            let fire = ctx.input.touch_control_center("fire").unwrap();
            match ctx.time.sim_tick() {
                1 => {
                    // A pause menu opens (it shows from the next frame).
                    let root = ctx.ui.root().layout(UiLayout::fill()).id();
                    ctx.ui.push_modal(root);
                }
                3 => ctx.input.feed(InputEvent::touch_start(1, fire)),
                4 => {
                    ctx.input.feed(InputEvent::touch_end(1, fire));
                    ctx.ui.pop_screen();
                }
                6 => ctx.input.feed(InputEvent::touch_start(2, fire)),
                _ => {}
            }
        })
        .sim_system(move |ctx: &mut Ctx| {
            log.borrow_mut()
                .push((ctx.input.touch_controls_shown(), ctx.input.action_down("fire")));
        })
        .run_headless(8, |ctx| {
            ctx.input.set_touch_controls(controls().show(TouchShow::Always));
        });
    let seen = seen.borrow();
    assert_eq!(seen[1], (true, false));
    assert_eq!(seen[3], (false, false), "under the menu the finger is the menu's");
    assert_eq!(seen[6], (true, true), "the menu closed: the button is back");
}

#[test]
fn onscreen_controls_take_action_ids_also_set_before_the_actions() {
    Game::new("touch ids", 960, 540).run_headless(0, |ctx| {
        // The controls come first; the action map installs after them.
        ctx.input
            .set_touch_controls(TouchControls::new().corner_button("menu", "II").show(TouchShow::Always));
        ctx.input.set_actions(map());
        let menu = ctx.input.action_id("menu");
        let at = ctx.input.touch_control_center(menu).expect("the control by id");
        assert_eq!(Some(at), ctx.input.touch_control_center("menu"));
        assert_eq!(
            ctx.input.touch_control_center(ctx.input.action_id("jump")),
            None,
            "no control drives jump"
        );
        assert_eq!(ctx.input.touch_control_center(ponko::actions::ActionId::NONE), None);
        ctx.input.set_touch_control_shown(menu, false);
        ctx.input.feed(InputEvent::touch_start(1, at));
        assert!(!ctx.input.action_down(menu), "hidden by id: no touch");
        ctx.input.feed(InputEvent::touch_end(1, at));
        ctx.input.set_touch_control_shown(menu, true);
        ctx.input.feed(InputEvent::touch_start(2, at));
        assert!(ctx.input.action_down(menu));
    });
}
