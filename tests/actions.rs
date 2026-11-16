#[allow(unused_imports)]
use ponko::actions::InputEvent;
#[allow(unused_imports)]
use ponko::input::{Gamepad, GamepadAxis, GamepadButton, GamepadStick};
use ponko::prelude::*;

fn bindings() -> ActionMap {
    let mut map = ActionMap::default();
    let pad = Gamepad::from_slot(0).unwrap();
    map.set(
        "jump",
        Action::Button(vec![
            ButtonBinding::Key(Key::Space),
            ButtonBinding::Mouse(MouseButton::Left),
            ButtonBinding::Gamepad {
                pad,
                button: GamepadButton::South,
            },
        ]),
    );
    map.set(
        "throttle",
        Action::Axis(vec![
            AxisBinding::Keys {
                negative: Key::S,
                positive: Key::W,
            },
            AxisBinding::Gamepad {
                pad,
                axis: GamepadAxis::RightTrigger,
                deadzone: 0.2,
            },
        ]),
    );
    map.set(
        "move",
        Action::Axis2(vec![
            Axis2Binding::Keys {
                left: Key::A,
                right: Key::D,
                up: Key::W,
                down: Key::S,
            },
            Axis2Binding::Gamepad {
                pad,
                stick: GamepadStick::Left,
                deadzone: 0.2,
            },
        ]),
    );
    map
}

#[test]
fn aggregate_edges_survive_taps_and_clear_between_headless_frames() {
    let mut frame = 0;
    Game::new("actions", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            match frame {
                0 => {
                    ctx.input.feed(InputEvent::Key(Key::Space, true));
                    assert!(ctx.input.action_pressed("jump"));
                }
                1 => {
                    assert!(ctx.input.action_down("jump"));
                    assert!(!ctx.input.action_pressed("jump"));
                    ctx.input.feed(InputEvent::Mouse(MouseButton::Left, true));
                    ctx.input.feed(InputEvent::Key(Key::Space, false));
                    assert!(ctx.input.action_down("jump"));
                    assert!(!ctx.input.action_pressed("jump"));
                    assert!(!ctx.input.action_released("jump"));
                }
                2 => {
                    ctx.input.feed(InputEvent::Mouse(MouseButton::Left, false));
                    assert!(ctx.input.action_released("jump"));
                    ctx.input.feed(InputEvent::Key(Key::Space, true));
                    ctx.input.feed(InputEvent::Key(Key::Space, false));
                    assert!(ctx.input.action_pressed("jump"));
                    assert!(ctx.input.action_released("jump"));
                    assert!(!ctx.input.action_down("jump"));
                }
                3 => {
                    assert!(!ctx.input.action_pressed("jump"));
                    assert!(!ctx.input.action_released("jump"));
                    let pad = Gamepad::from_slot(0).unwrap();
                    ctx.input
                        .feed(InputEvent::GamepadButton(pad, GamepadButton::South, true));
                    assert!(ctx.input.action_pressed("jump"));
                    ctx.input.feed(InputEvent::GamepadDisconnected(pad));
                    assert!(ctx.input.action_released("jump"));
                    assert!(!ctx.input.action_down("jump"));
                }
                _ => unreachable!(),
            }
            frame += 1;
        })
        .run_headless(4, |ctx| ctx.input.set_actions(bindings()));
}

#[test]
fn axes_cancel_normalize_select_strongest_and_apply_deadzones() {
    Game::new("axes", 1, 1).run_headless(0, |ctx| {
        ctx.input.set_actions(bindings());
        ctx.input.feed(InputEvent::Key(Key::W, true));
        ctx.input.feed(InputEvent::Key(Key::D, true));
        let movement = ctx.input.action_axis2("move");
        assert!((movement.length() - 1.0).abs() < 1e-6);
        assert!(movement.x > 0.0 && movement.y < 0.0);
        assert_eq!(ctx.input.action_axis("throttle"), 1.0);
        ctx.input.feed(InputEvent::Key(Key::S, true));
        assert_eq!(ctx.input.action_axis("throttle"), 0.0);
        ctx.input.feed(InputEvent::FocusLost);
        assert_eq!(ctx.input.action_axis2("move"), Vec2::ZERO);
        let pad = Gamepad::from_slot(0).unwrap();
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::RightTrigger, 0.1));
        assert_eq!(ctx.input.action_axis("throttle"), 0.0);
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::RightTrigger, 0.6));
        assert!((ctx.input.action_axis("throttle") - 0.5).abs() < 1e-6);
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.6));
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickY, 0.8));
        assert!((ctx.input.action_axis2("move") - Vec2::new(0.6, 0.8)).length() < 1e-6);
        ctx.input
            .feed(InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, f32::NAN));
        assert_eq!(ctx.input.action_axis2("move"), Vec2::ZERO);
        ctx.input.feed(InputEvent::GamepadDisconnected(pad));
        assert_eq!(ctx.input.action_axis("throttle"), 0.0);
        assert_eq!(ctx.input.action_axis2("move"), Vec2::ZERO);
        assert!(!ctx.input.action_down("absent"));
        assert_eq!(ctx.input.action_axis("move"), 0.0);
        assert_eq!(ctx.input.action_axis2("jump"), Vec2::ZERO);
    });
}

#[test]
fn json_round_trip_and_rebinding_reset_edges_without_releasing_held_sources() {
    let json = bindings().to_json().unwrap();
    let map = ActionMap::from_json(&json).unwrap();
    assert_eq!(map.to_json().unwrap(), json);
    assert!(ActionMap::from_json("bad").is_err());
    assert!(
        ActionMap::from_json(r#"{"bindings":{"bad":{"Button":[{"Gamepad":{"pad":8,"button":"South"}}]}}}"#).is_err()
    );
    Game::new("rebind", 1, 1).run_headless(0, |ctx| {
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        ctx.input.set_actions(map);
        assert!(ctx.input.action_down("jump"));
        assert!(!ctx.input.action_pressed("jump"));
        let mut changed = ctx.input.actions().clone();
        changed.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Enter)]));
        changed.remove("move");
        ctx.input.set_actions(changed);
        assert!(!ctx.input.action_down("jump"));
        assert!(!ctx.input.action_released("jump"));
        ctx.input.feed(InputEvent::Key(Key::Enter, true));
        assert!(ctx.input.action_pressed("jump"));
        ctx.input.feed(InputEvent::FocusLost);
        assert!(ctx.input.action_released("jump"));
        assert_eq!(ctx.input.action_axis2("move"), Vec2::ZERO);
    });
}

#[test]
fn a_map_built_in_code_and_one_loaded_from_json_read_back_the_same_bindings() {
    let built = bindings();
    let loaded = ActionMap::from_json(&built.to_json().unwrap()).unwrap();
    assert_eq!(built.len(), 3);
    assert!(!built.is_empty() && ActionMap::default().is_empty());
    // Ordered by name, whatever order they were set in.
    let names: Vec<&str> = built.iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["jump", "move", "throttle"]);
    assert!(built.iter().eq(loaded.iter()));
    for (name, action) in built.iter() {
        assert_eq!(loaded.get(name), Some(action), "{name}");
    }
    assert_eq!(built.get("nope"), None);

    let jump = loaded.get("jump").unwrap();
    assert_eq!(jump.buttons()[0], ButtonBinding::Key(Key::Space));
    assert_eq!(jump.buttons()[1], ButtonBinding::Mouse(MouseButton::Left));
    assert!(jump.axes().is_empty() && jump.axes2().is_empty());
    let pad = Gamepad::from_slot(0).unwrap();
    assert_eq!(
        loaded.get("throttle").unwrap().axes()[1],
        AxisBinding::Gamepad {
            pad,
            axis: GamepadAxis::RightTrigger,
            deadzone: 0.2
        }
    );
    assert!(matches!(
        loaded.get("move").unwrap().axes2()[0],
        Axis2Binding::Keys { left: Key::A, .. }
    ));
    assert!(loaded.get("move").unwrap().buttons().is_empty());
}

#[test]
fn set_action_changes_one_action_in_place_keeping_ids_and_the_others_state() {
    Game::new("set_action", 1, 1).run_headless(0, |ctx| {
        ctx.input.set_actions(bindings());
        let [jump, throttle, mv] = ctx.input.action_ids(&["jump", "throttle", "move"]);
        ctx.input.feed(InputEvent::Key(Key::W, true));
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(ctx.input.action_pressed(jump));

        // Rebind one source of "move": read it back, change it, put it back.
        let mut walk = ctx.input.actions().get("move").unwrap().clone();
        let Action::Axis2(sources) = &mut walk else {
            unreachable!()
        };
        sources[0] = Axis2Binding::Keys {
            left: Key::J,
            right: Key::L,
            up: Key::I,
            down: Key::K,
        };
        ctx.input.set_action("move", walk.clone());
        assert_eq!(ctx.input.actions().get("move"), Some(&walk));
        assert_eq!(ctx.input.action_id("move"), mv, "the name keeps its slot");
        assert_eq!(ctx.input.action_axis2(mv), Vec2::ZERO, "W no longer moves");
        ctx.input.feed(InputEvent::Key(Key::I, true));
        assert_eq!(ctx.input.action_axis2(mv), Vec2::new(0.0, -1.0), "I does");

        // The others are untouched: still held, this frame's edge still there.
        assert!(ctx.input.action_down(jump) && ctx.input.action_pressed(jump));
        assert_eq!(ctx.input.action_axis(throttle), 1.0);

        // A held source becomes the baseline: no edge from rebinding itself.
        ctx.input
            .set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
        assert!(ctx.input.action_down(jump));
        assert!(!ctx.input.action_pressed(jump) && !ctx.input.action_released(jump));
        ctx.input
            .set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Enter)]));
        assert!(!ctx.input.action_down(jump));
        assert!(!ctx.input.action_released(jump), "rebinding releases nothing");

        // A new name gets a new slot; ids resolved before stay valid.
        ctx.input
            .set_action("fire", Action::Button(vec![ButtonBinding::Key(Key::F)]));
        let fire = ctx.input.action_id("fire");
        assert!(fire != jump && fire != throttle && fire != mv);
        ctx.input.feed(InputEvent::Key(Key::F, true));
        assert!(ctx.input.action_pressed(fire));
        assert_eq!(ctx.input.action_axis(throttle), 1.0);
    });
}

#[test]
fn named_button_constructors_preserve_legacy_events_and_float_equality() {
    use ponko::actions::ButtonState::{Held, Released};
    let pad = Gamepad::from_slot(0).unwrap();
    for (state, down) in [(Held, true), (Released, false)] {
        assert_eq!(InputEvent::key(Key::Space, state), InputEvent::Key(Key::Space, down));
        assert_eq!(
            InputEvent::mouse(MouseButton::Left, state),
            InputEvent::Mouse(MouseButton::Left, down)
        );
        assert_eq!(
            InputEvent::gamepad_button(pad, GamepadButton::South, state),
            InputEvent::GamepadButton(pad, GamepadButton::South, down)
        );
    }
    let axis = InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.5);
    assert_eq!(axis, axis);
    let nan = InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, f32::NAN);
    assert_ne!(nan, nan);
    assert_ne!(InputEvent::FocusLost, InputEvent::GamepadDisconnected(pad));
}

#[test]
fn repeated_held_events_do_not_repeat_edges_for_any_button_source() {
    use ponko::actions::ButtonState::{Held, Released};
    let pad = Gamepad::from_slot(0).unwrap();
    let events = [
        (InputEvent::key(Key::Space, Held), InputEvent::key(Key::Space, Released)),
        (
            InputEvent::mouse(MouseButton::Left, Held),
            InputEvent::mouse(MouseButton::Left, Released),
        ),
        (
            InputEvent::gamepad_button(pad, GamepadButton::South, Held),
            InputEvent::gamepad_button(pad, GamepadButton::South, Released),
        ),
    ];
    for (held, released) in events {
        let mut frame = 0;
        Game::new("named states", 1, 1)
            .sim_system(move |ctx: &mut Ctx| {
                ctx.input.feed(held);
                assert!(ctx.input.action_down("jump"));
                assert_eq!(ctx.input.action_pressed("jump"), frame == 0);
                if frame == 1 {
                    ctx.input.feed(released);
                    assert!(ctx.input.action_released("jump"));
                    assert!(!ctx.input.action_down("jump"));
                }
                frame += 1;
            })
            .run_headless(2, |ctx| ctx.input.set_actions(bindings()));
    }
}

#[test]
fn json_errors_use_engine_kind_and_keep_decode_location_and_context() {
    fn round_trip(map: &ActionMap) -> Result<ActionMap, Error> {
        ActionMap::from_json(&map.to_json()?)
    }
    assert_eq!(round_trip(&bindings()).unwrap(), bindings());
    let err = ActionMap::from_json("{\n  invalid\n}").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Json);
    assert!(err.message().contains("decoding action map"));
    assert!(err.message().contains("line 2 column"));
}

#[test]
fn map_equality_compares_bindings_ignoring_runtime_edges_and_slot_history() {
    let map = bindings();
    let mut changed = map.clone();
    changed.remove("jump");
    assert_ne!(map, changed);
    changed.set("temporary", Action::Button(vec![]));
    changed.remove("temporary");
    changed.set("jump", map.get("jump").unwrap().clone());
    assert_eq!(map, changed);
    Game::new("equality", 1, 1).run_headless(0, |ctx| {
        ctx.input.set_actions(map.clone());
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert_eq!(*ctx.input.actions(), map);
        assert_eq!(ActionMap::from_json(&map.to_json().unwrap()).unwrap(), map);
    });
}
