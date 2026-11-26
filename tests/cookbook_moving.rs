//! Cookbook, "Moving things": recipes for walking, jumping, dashing, clicking,
//! dragging and rebinding. Every recipe between `ANCHOR` lines is what
//! `book/src/cookbook/moving.md` shows, and each test runs it headless for a
//! few ticks and checks that it did its job.
//!
//! A headless run has no mouse, so tests play it: they move the pointer with
//! `InputEvent::MouseMove` (`point_at`) and press buttons with an input
//! scenario. The camera stays where it is.
#![allow(clippy::type_complexity)]

use ponko::actions::InputEvent;
use ponko::d2::*;
use ponko::prelude::*;
use ponko::scenario::InputScenario;

/// Holds `key` from tick `from` until tick `to`.
fn hold(scenario: InputScenario, key: Key, from: u64, to: u64) -> InputScenario {
    scenario
        .at(from, InputEvent::Key(key, true))
        .at(to, InputEvent::Key(key, false))
}

/// A tap of `key` on one tick.
fn tap(scenario: InputScenario, key: Key, tick: u64) -> InputScenario {
    hold(scenario, key, tick, tick + 1)
}

/// Where the first entity with `T` is.
fn position_of<T: Component>(world: &mut World) -> Vec2 {
    let mut query = world.query::<(&T, &Transform)>();
    let (_, t) = query.iter().next().expect("the entity");
    t.position
}

/// Moves the headless pointer onto a world point: to the window pixel the
/// camera shows it at.
fn point_at(ctx: &mut Ctx, world_point: Vec2) {
    let pixel = ctx.camera.world_to_screen(world_point);
    ctx.input.feed(InputEvent::MouseMove(pixel));
}

// ANCHOR: wasd
#[derive(Component)]
struct Player;

/// The `move` action, resolved once when it is bound: systems read it by
/// id, without looking its name up every tick.
#[derive(Clone, Copy)]
struct Move(ActionId);

/// One name for the controls; keyboard, and other devices, are bound to it once.
fn bind_move(ctx: &mut Ctx) {
    ctx.input.set_action(
        "move",
        Action::Axis2(vec![
            Axis2Binding::Keys {
                left: Key::A,
                right: Key::D,
                up: Key::W,
                down: Key::S,
            },
            Axis2Binding::Keys {
                left: Key::Left,
                right: Key::Right,
                up: Key::Up,
                down: Key::Down,
            },
        ]),
    );
    let movement = ctx.input.action_id("move");
    ctx.world.insert_resource(Move(movement));
}

fn walk(ctx: &mut Ctx) {
    let Move(movement) = *ctx.world.resource::<Move>();
    // Up and right together is longer than 1: cap it so diagonals aren't faster.
    let direction = ctx.input.action_axis2(movement).clamp_length_max(1.0);
    let step = direction * 200.0 * ctx.time.dt(); // 200 pixels per second
    ctx.world
        .query::<(&Player, &mut Transform)>()
        .for_each(|(_, t)| t.position += step);
}
// ANCHOR_END: wasd

fn walker(scenario: InputScenario, ticks: u64) -> World {
    Game::new("walk", 640, 480)
        .setup(|ctx| {
            bind_move(ctx);
            ctx.world.spawn((Player, Transform::at(vec2(100.0, 100.0))));
        })
        .sim_system(walk)
        .input_scenario(scenario)
        .run_headless(ticks)
}

#[test]
fn holding_d_walks_right_at_200_pixels_per_second() {
    let mut world = walker(hold(InputScenario::new(), Key::D, 0, 30), 40);
    let at = position_of::<Player>(&mut world);
    assert!((at.x - 200.0).abs() < 0.5, "30 ticks, half a second: {at}");
    assert_eq!(at.y, 100.0);
}

#[test]
fn walking_diagonally_is_no_faster() {
    let scenario = hold(hold(InputScenario::new(), Key::D, 0, 60), Key::S, 0, 60);
    let mut world = walker(scenario, 60);
    let moved = position_of::<Player>(&mut world) - vec2(100.0, 100.0);
    assert!((moved.length() - 200.0).abs() < 1.0, "one second of speed 200: {moved}");
    assert!(moved.x > 100.0 && moved.y > 100.0, "right and down: {moved}");
}

// ANCHOR: gamepad
fn bind_move_for_everyone(ctx: &mut Ctx) {
    let pad = Gamepad::from_slot(0).unwrap(); // player one's controller
    ctx.input.set_action(
        "move",
        Action::Axis2(vec![
            Axis2Binding::Keys {
                left: Key::A,
                right: Key::D,
                up: Key::W,
                down: Key::S,
            },
            // Small wobbles around the centre of the stick read as zero.
            Axis2Binding::Gamepad {
                pad,
                stick: GamepadStick::Left,
                deadzone: 0.2,
            },
        ]),
    );
    let movement = ctx.input.action_id("move");
    ctx.world.insert_resource(Move(movement));
    // `walk` from the last recipe doesn't change: it reads "move", not a device.
}
// ANCHOR_END: gamepad

#[test]
fn the_same_move_action_reads_a_gamepad_stick_and_ignores_a_wobble() {
    let pad = Gamepad::from_slot(0).unwrap();
    let scenario = InputScenario::new()
        .at(0, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.1)) // a wobble
        .at(10, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 1.0))
        .at(40, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.0));
    let mut world = Game::new("pad", 640, 480)
        .setup(|ctx| {
            bind_move_for_everyone(ctx);
            ctx.world.spawn((Player, Transform::at(vec2(0.0, 0.0))));
        })
        .sim_system(walk)
        .input_scenario(scenario)
        .run_headless(60);
    let at = position_of::<Player>(&mut world);
    assert!(
        (at.x - 100.0).abs() < 1.0,
        "30 ticks at full tilt, none for the wobble: {at}"
    );
}

// ANCHOR: jump
#[derive(Component, Default)]
struct Body {
    vy: f32,
    on_ground: bool,
}

const GRAVITY: f32 = 1200.0; // pixels per second squared; y points down
const FLOOR: f32 = 400.0;

fn jump_and_fall(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    let jump = ctx.input.key_pressed(Key::Space);
    ctx.world.query::<(&mut Body, &mut Transform)>().for_each(|(body, t)| {
        if jump && body.on_ground {
            body.vy = -500.0; // up
        }
        body.vy += GRAVITY * dt;
        t.position.y += body.vy * dt;
        body.on_ground = t.position.y >= FLOOR;
        if body.on_ground {
            t.position.y = FLOOR;
            body.vy = 0.0;
        }
    });
}
// ANCHOR_END: jump

#[test]
fn space_jumps_once_and_lands_on_the_floor() {
    #[derive(Default)]
    struct Highest(f32);
    let mut world = Game::new("jump", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Highest(FLOOR));
            ctx.world.spawn((Body::default(), Transform::at(vec2(50.0, FLOOR))));
        })
        .sim_system(jump_and_fall)
        .sim_system(|ctx: &mut Ctx| {
            let y = ctx
                .world
                .query::<(&Body, &Transform)>()
                .iter()
                .next()
                .unwrap()
                .1
                .position
                .y;
            let high = ctx.world.resource_mut::<Highest>();
            high.0 = high.0.min(y);
        })
        .input_scenario(tap(InputScenario::new(), Key::Space, 5))
        .run_headless(120);
    // v² / 2g = 500² / 2400, about 104 pixels.
    let apex = FLOOR - world.resource::<Highest>().0;
    assert!((90.0..110.0).contains(&apex), "jumped {apex} pixels high");
    assert_eq!(position_of::<Body>(&mut world).y, FLOOR, "and came back down");
}

#[test]
fn a_jump_in_the_air_does_nothing() {
    let mut world = Game::new("jump", 640, 480)
        .setup(|ctx| {
            ctx.world
                .spawn((Body::default(), Transform::at(vec2(50.0, FLOOR - 200.0))));
        })
        .sim_system(jump_and_fall)
        .input_scenario(tap(InputScenario::new(), Key::Space, 1))
        .run_headless(10);
    // Falling since the start: 200 pixels takes 0.58 s, so it is still in the air.
    let vy = world.query::<&Body>().iter().next().unwrap().vy;
    assert!(vy > 0.0, "still falling, not pushed up: {vy}");
}

// ANCHOR: dash
#[derive(Component)]
struct Dash {
    ready_at: f64, // simulation time, in seconds
}

fn dash(ctx: &mut Ctx) {
    if !ctx.input.key_pressed(Key::LeftShift) {
        return;
    }
    let now = ctx.time.sim_time();
    let Move(movement) = *ctx.world.resource::<Move>(); // from the first recipe
    let aim = ctx.input.action_axis2(movement).normalize_or_zero();
    ctx.world.query::<(&mut Dash, &mut Transform)>().for_each(|(dash, t)| {
        if now >= dash.ready_at {
            t.position += aim * 120.0;
            dash.ready_at = now + 1.5; // a cooldown of a second and a half
        }
    });
}
// ANCHOR_END: dash

#[test]
fn dash_jumps_120_pixels_and_waits_out_its_cooldown() {
    // D is held all the time; shift is tapped at ticks 2, 20 and 120.
    let mut scenario = hold(InputScenario::new(), Key::D, 0, 200);
    for tick in [2, 20, 120] {
        scenario = tap(scenario, Key::LeftShift, tick);
    }
    let mut world = Game::new("dash", 640, 480)
        .setup(|ctx| {
            bind_move(ctx);
            ctx.world
                .spawn((Player, Dash { ready_at: 0.0 }, Transform::at(Vec2::ZERO)));
        })
        .sim_system(dash)
        .input_scenario(scenario)
        .run_headless(130);
    // Tick 20 is only 0.3 s after the first dash: refused. Tick 120 is 2 s after it.
    assert_eq!(
        position_of::<Player>(&mut world).x,
        240.0,
        "two dashes of 120, not three"
    );
}

// ANCHOR: click_to_move
#[derive(Component, Default)]
struct Goal(Option<Vec2>);

fn click_to_move(ctx: &mut Ctx) {
    // A click on a button isn't a click on the ground.
    if ctx.input.mouse_pressed(MouseButton::Left) && !ctx.ui.wants_pointer() {
        let spot = ctx.mouse_world(); // through the camera, in world units
        ctx.world.query::<&mut Goal>().for_each(|goal| goal.0 = Some(spot));
    }
    let step = 150.0 * ctx.time.dt();
    ctx.world.query::<(&mut Goal, &mut Transform)>().for_each(|(goal, t)| {
        let Some(to) = goal.0 else { return };
        let left = to - t.position;
        if left.length() <= step {
            t.position = to; // arrived
            goal.0 = None;
        } else {
            t.position += left.normalize() * step;
        }
    });
}
// ANCHOR_END: click_to_move

#[test]
fn a_click_sends_the_player_there_and_stops_it() {
    let mut world = Game::new("click", 640, 480)
        .setup(|ctx| {
            point_at(ctx, vec2(120.0, 90.0));
            ctx.world.spawn((Player, Goal::default(), Transform::at(Vec2::ZERO)));
        })
        .sim_system(click_to_move)
        .input_scenario(
            InputScenario::new()
                .at(2, InputEvent::Mouse(MouseButton::Left, true))
                .at(3, InputEvent::Mouse(MouseButton::Left, false)),
        )
        .run_headless(120);
    assert_eq!(position_of::<Player>(&mut world), vec2(120.0, 90.0));
    assert_eq!(world.query::<&Goal>().iter().next().unwrap().0, None, "and stays put");
}

// ANCHOR: drag
#[derive(Component)]
struct Draggable {
    half_size: Vec2,
}

/// The sprite being dragged and where, relative to its centre, it was grabbed.
#[derive(Default)]
struct Held(Option<(Entity, Vec2)>);

fn drag(ctx: &mut Ctx) {
    let mouse = ctx.mouse_world();
    let mut held = ctx.world.resource::<Held>().0;
    if ctx.input.mouse_pressed(MouseButton::Left) {
        ctx.world
            .query::<(Entity, &Draggable, &Transform)>()
            .for_each(|(e, d, t)| {
                if (mouse - t.position).abs().cmple(d.half_size).all() {
                    held = Some((e, t.position - mouse));
                }
            });
    }
    if !ctx.input.mouse_down(MouseButton::Left) {
        held = None; // let go
    }
    if let Some((e, grab)) = held {
        ctx.world.get_mut::<Transform>(e).unwrap().position = mouse + grab;
    }
    ctx.world.resource_mut::<Held>().0 = held;
}
// ANCHOR_END: drag

#[test]
fn dragging_carries_a_sprite_by_the_spot_that_was_grabbed() {
    let mut world = Game::new("drag", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Held::default());
            ctx.world.spawn((
                Draggable {
                    half_size: vec2(20.0, 20.0),
                },
                Transform::at(vec2(100.0, 100.0)),
            ));
            ctx.world.spawn((
                Draggable {
                    half_size: vec2(20.0, 20.0),
                },
                Transform::at(vec2(300.0, 300.0)),
            ));
        })
        // The test plays the cursor: it is at the sprite's corner at tick 2,
        // then moves 100 right and 50 down, and lets go at tick 12.
        .sim_system(|ctx: &mut Ctx| {
            let t = ctx.time.sim_tick();
            let cursor = match t {
                0..=2 => vec2(110.0, 90.0),
                _ => vec2(110.0, 90.0) + vec2(100.0, 50.0) * ((t - 2).min(8) as f32 / 8.0),
            };
            point_at(ctx, cursor);
        })
        .sim_system(drag)
        .input_scenario(
            InputScenario::new()
                .at(2, InputEvent::Mouse(MouseButton::Left, true))
                .at(12, InputEvent::Mouse(MouseButton::Left, false)),
        )
        .run_headless(20);
    let mut at: Vec<Vec2> = world
        .query::<(&Draggable, &Transform)>()
        .iter()
        .map(|(_, t)| t.position)
        .collect();
    at.sort_by(|a, b| a.x.total_cmp(&b.x));
    assert_eq!(
        at,
        [vec2(200.0, 150.0), vec2(300.0, 300.0)],
        "one moved, the other didn't"
    );
    assert!(world.resource::<Held>().0.is_none(), "and it was let go");
}

// ANCHOR: face_mouse
fn face_the_mouse(ctx: &mut Ctx) {
    let mouse = ctx.mouse_world();
    ctx.world.query::<(&Player, &mut Transform)>().for_each(|(_, t)| {
        let to = mouse - t.position;
        t.rotation = to.y.atan2(to.x); // radians; 0 faces right, y points down
    });
}
// ANCHOR_END: face_mouse

#[test]
fn the_player_turns_to_face_the_cursor() {
    let mut world = Game::new("aim", 640, 480)
        .setup(|ctx| {
            point_at(ctx, vec2(50.0, 150.0)); // straight below the player
            ctx.world.spawn((Player, Transform::at(vec2(50.0, 50.0))));
        })
        .sim_system(face_the_mouse)
        .run_headless(2);
    let rotation = world.query::<(&Player, &Transform)>().iter().next().unwrap().1.rotation;
    assert!(
        (rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
        "facing down: {rotation}"
    );
}

// ANCHOR: stay_on_screen
fn stay_on_screen(ctx: &mut Ctx) {
    let margin = Vec2::splat(16.0); // half the sprite
    let (min, max) = (margin, ctx.window_size() - margin);
    ctx.world
        .query::<(&Player, &mut Transform)>()
        .for_each(|(_, t)| t.position = t.position.clamp(min, max));
}
// ANCHOR_END: stay_on_screen

#[test]
fn walking_into_the_edge_stops_at_the_margin() {
    let mut world = Game::new("edge", 640, 480)
        .setup(|ctx| {
            bind_move(ctx);
            ctx.world.spawn((Player, Transform::at(vec2(600.0, 240.0))));
        })
        .sim_system(walk)
        .sim_system(stay_on_screen)
        .input_scenario(hold(InputScenario::new(), Key::D, 0, 120))
        .run_headless(120);
    assert_eq!(position_of::<Player>(&mut world), vec2(624.0, 240.0));
}

// ANCHOR: rebind
/// True while the "press a key for jump" prompt is open.
#[derive(Default)]
struct Rebinding(bool);

fn rebind_jump(ctx: &mut Ctx) {
    if !ctx.world.resource::<Rebinding>().0 {
        return;
    }
    // The first key, mouse button or pad button that went down this frame.
    // (Collect it first: the iterator borrows `ctx.input`.)
    let pressed = ctx.input.pressed_this_frame().find_map(|p| p.button().cloned());
    if let Some(binding) = pressed {
        ctx.input.set_action("jump", Action::Button(vec![binding]));
        ctx.world.resource_mut::<Rebinding>().0 = false;
    }
}
// ANCHOR_END: rebind

#[test]
fn the_next_key_pressed_becomes_the_jump_key() {
    let world = Game::new("rebind", 640, 480)
        .setup(|ctx| {
            ctx.world.insert_resource(Rebinding(true));
            ctx.input
                .set_action("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
        })
        .sim_system(rebind_jump)
        .sim_system(|ctx: &mut Ctx| {
            // What a game would do with the new binding: jump is down on K only.
            if ctx.time.sim_tick() == 4 {
                assert!(ctx.input.action_down("jump"), "K is jump now");
            }
            if ctx.time.sim_tick() == 8 {
                assert!(!ctx.input.action_down("jump"), "space no longer is");
            }
        })
        .input_scenario(hold(hold(InputScenario::new(), Key::K, 2, 6), Key::Space, 7, 9))
        .run_headless(10);
    assert!(!world.resource::<Rebinding>().0, "the prompt closed itself");
}
