//! `ActionId`: names resolved once, queried by index.
#[allow(unused_imports)]
use ponko::actions::InputEvent;
use ponko::prelude::*;

actions! {
    /// Test actions: names declared once.
    struct Act { jump = "jump", throttle = "throttle", steer = "steer" }
}

fn bindings() -> ActionMap {
    let mut map = ActionMap::default();
    map.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
    map.set(
        "throttle",
        Action::Axis(vec![AxisBinding::Keys {
            negative: Key::S,
            positive: Key::W,
        }]),
    );
    map.set(
        "steer",
        Action::Axis2(vec![Axis2Binding::Keys {
            left: Key::A,
            right: Key::D,
            up: Key::Q,
            down: Key::E,
        }]),
    );
    map
}

fn headless(f: impl FnOnce(&mut Ctx)) {
    Game::new("action ids", 1, 1).run_headless(0, |ctx| {
        ctx.input.set_actions(bindings());
        f(ctx)
    });
}

#[test]
fn id_queries_match_the_name_queries() {
    headless(|ctx| {
        let [jump, throttle, steer] = ctx.input.action_ids(&["jump", "throttle", "steer"]);
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        ctx.input.feed(InputEvent::Key(Key::W, true));
        ctx.input.feed(InputEvent::Key(Key::D, true));
        assert!(ctx.input.action_down(jump) && ctx.input.action_down("jump"));
        assert!(ctx.input.action_pressed(jump) && !ctx.input.action_released(jump));
        assert_eq!(ctx.input.action_axis(throttle), 1.0);
        assert_eq!(ctx.input.action_axis2(steer), Vec2::X);
        ctx.input.feed(InputEvent::Key(Key::Space, false));
        assert!(ctx.input.action_released(jump) && ctx.input.action_released("jump"));
        assert!(!ctx.input.action_down(jump));
    });
}

#[test]
fn ids_are_copy_and_distinct_per_action() {
    headless(|ctx| {
        let a = ctx.input.action_id("jump");
        let b = a; // Copy
        assert_eq!(a, b);
        assert_ne!(a, ctx.input.action_id("throttle"));
        assert!(!a.is_none());
    });
}

#[test]
fn unknown_names_fail_at_resolution_with_a_helpful_message() {
    headless(|ctx| {
        let error = ctx.input.try_action_id("jmup").unwrap_err();
        assert_eq!(error.name(), "jmup");
        assert_eq!(
            error.to_string(),
            "unknown action \"jmup\" (known: jump, steer, throttle)"
        );
        // The logging form gives the same error to stderr and an id that reads false/zero.
        let none = ctx.input.action_id("jmup");
        assert!(none.is_none());
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(
            !ctx.input.action_down(none)
                && ctx.input.action_axis(none) == 0.0
                && ctx.input.action_axis2(none) == Vec2::ZERO
        );
        assert_eq!(ActionId::default(), ActionId::NONE);
    });
}

#[test]
fn resolving_with_no_actions_installed_says_so() {
    let error = ActionMap::default().resolve("jump").unwrap_err();
    assert_eq!(
        error.to_string(),
        "unknown action \"jump\" (no actions are installed; see Input::set_actions)"
    );
}

#[test]
fn ids_survive_rebinding_and_loading() {
    headless(|ctx| {
        let jump = ctx.input.action_id("jump");
        let throttle = ctx.input.action_id("throttle");
        // Rebind jump to Enter, add a new action, drop `steer`.
        let mut changed = ctx.input.actions().clone();
        changed.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Enter)]));
        changed.set("fire", Action::Button(vec![ButtonBinding::Key(Key::F)]));
        changed.remove("steer");
        ctx.input.set_actions(changed);
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(!ctx.input.action_down(jump));
        ctx.input.feed(InputEvent::Key(Key::Enter, true));
        assert!(ctx.input.action_down(jump), "the id still names `jump`");
        ctx.input.feed(InputEvent::Key(Key::W, true));
        assert_eq!(ctx.input.action_axis(throttle), 1.0);
        // A map loaded from JSON keeps the slots too.
        let json = ctx.input.actions().to_json().unwrap();
        ctx.input.set_actions(ActionMap::from_json(&json).unwrap());
        assert!(ctx.input.action_down(jump));
        assert_eq!(ctx.input.action_axis(throttle), 1.0);
        // New names resolve, removed ones don't.
        let fire = ctx.input.action_id("fire");
        ctx.input.feed(InputEvent::Key(Key::F, true));
        assert!(ctx.input.action_down(fire));
        assert!(ctx.input.try_action_id("steer").is_err());
    });
}

#[test]
fn a_removed_actions_id_reads_zero_and_rebinding_the_name_reuses_its_slot() {
    headless(|ctx| {
        let steer = ctx.input.action_id("steer");
        ctx.input.feed(InputEvent::Key(Key::D, true));
        assert_eq!(ctx.input.action_axis2(steer), Vec2::X);
        let mut changed = ctx.input.actions().clone();
        changed.remove("steer");
        ctx.input.set_actions(changed);
        assert_eq!(ctx.input.action_axis2(steer), Vec2::ZERO);
        // Binding the name again reuses its slot.
        let mut again = ctx.input.actions().clone();
        again.set(
            "steer",
            Action::Axis2(vec![Axis2Binding::Keys {
                left: Key::A,
                right: Key::D,
                up: Key::Q,
                down: Key::E,
            }]),
        );
        ctx.input.set_actions(again);
        assert_eq!(ctx.input.action_id("steer"), steer);
        assert_eq!(ctx.input.action_axis2(steer), Vec2::X);
    });
}

#[test]
fn the_actions_macro_resolves_every_name_once() {
    headless(|ctx| {
        assert_eq!(Act::NAMES, ["jump", "throttle", "steer"]);
        let act = Act::resolve(ctx);
        let copy = act; // Copy
        assert_eq!(copy.jump, ctx.input.action_id("jump"));
        assert_eq!(act.throttle, ctx.input.action_id("throttle"));
        assert_eq!(act.steer, ctx.input.action_id("steer"));
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(ctx.input.action_pressed(act.jump));
    });
}

#[test]
fn string_queries_still_work_for_owned_names() {
    headless(|ctx| {
        let name = String::from("jump");
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(ctx.input.action_down(&name));
        assert!(!ctx.input.action_down("absent"));
    });
}

#[test]
fn names_held_in_any_string_type_query_like_str() {
    headless(|ctx| {
        let held: std::sync::Arc<str> = "jump".into();
        let boxed: Box<str> = "jump".into();
        let cow: std::borrow::Cow<str> = "jump".into();
        ctx.input.feed(InputEvent::Key(Key::Space, true));
        assert!(ctx.input.action_down(&held) && ctx.input.action_down(&boxed) && ctx.input.action_down(&cow));
        assert!(ctx.input.action_down(&&*held), "a reference to a &str");
    });
}
