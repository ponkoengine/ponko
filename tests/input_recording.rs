//! Recording input into an `InputScenario` (`Ctx::start_input_recording`)
//! and replaying it: each event lands on the tick that first saw it, and a
//! deterministic game replays to the same world.

use ponko::actions::InputEvent;
use ponko::input::{Gamepad, GamepadAxis, ImeEvent};
use ponko::prelude::*;
use ponko::reflect::world_hash;
use ponko::scenario::InputScenario;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Component, Serialize, Deserialize)]
struct Player {
    at: Vec2,
    jumps: u32,
    aim: Vec2,
    zoom: f32,
    lean: f32,
}

/// Where a finished recording is kept.
struct Recorded(Option<InputScenario>);

const TICKS: u64 = 40;

/// Stands in for a player at the window: events arrive before a tick's
/// game systems (it is the first system).
fn live_player(ctx: &mut Ctx) {
    let pad = Gamepad::from_slot(0).unwrap();
    let feed = |ctx: &mut Ctx, e| ctx.input.feed(e);
    match ctx.time.sim_tick() {
        2 => feed(ctx, InputEvent::Key(Key::D, true)),
        5 => {
            feed(ctx, InputEvent::Key(Key::Space, true));
            for x in [10.0, 20.0, 30.0] {
                feed(ctx, InputEvent::MouseMove(vec2(x, 5.0)));
            }
        }
        6 => feed(ctx, InputEvent::Key(Key::Space, false)),
        9 => feed(ctx, InputEvent::MouseWheel(vec2(0.0, 2.0))),
        12 => {
            feed(ctx, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.25));
            feed(ctx, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.75));
        }
        17 => feed(ctx, InputEvent::Key(Key::D, false)),
        20 => {
            feed(ctx, InputEvent::Key(Key::Space, true));
            feed(ctx, InputEvent::Key(Key::Space, false));
        }
        25 => ctx.input.feed_ime(ImeEvent::Commit("hi".into())),
        31 => feed(ctx, InputEvent::FocusLost),
        _ => {}
    }
}

/// The game: reads the input every tick.
fn play(ctx: &mut Ctx) {
    let pad = Gamepad::from_slot(0).unwrap();
    let (right, jump) = (ctx.input.key_down(Key::D), ctx.input.key_pressed(Key::Space));
    let (aim, wheel) = (ctx.input.mouse_pos(), ctx.input.mouse_wheel().y);
    let lean = ctx.input.gamepad_axis(pad, GamepadAxis::LeftStickX);
    let typed = ctx.input.typed_text().len() as f32;
    for p in ctx.world.query::<&mut Player>().iter() {
        p.at.x += if right { 1.0 } else { 0.0 } + typed;
        p.jumps += u32::from(jump);
        p.aim = aim;
        p.zoom += wheel;
        p.lean += lean;
    }
}

fn spawn_player(ctx: &mut Ctx) {
    ctx.world.reflect::<Player>();
    ctx.world.spawn((Player {
        at: Vec2::ZERO,
        jumps: 0,
        aim: Vec2::ZERO,
        zoom: 0.0,
        lean: 0.0,
    },));
}

/// Plays live, recording from the start; returns the world and the recording.
fn record_live() -> (World, InputScenario) {
    let mut world = Game::new("t", 1, 1)
        .sim_system(live_player)
        .sim_system(play)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == TICKS - 1 {
                let recorded = ctx.input.stop_recording();
                ctx.world.insert_resource(Recorded(recorded));
            }
        })
        .run_headless(TICKS, |ctx| {
            spawn_player(ctx);
            ctx.input.start_recording();
        });
    let scenario = world.remove_resource::<Recorded>().unwrap().0.unwrap();
    (world, scenario)
}

#[test]
fn a_recording_replays_to_the_same_world() {
    let (live, scenario) = record_live();
    assert_eq!(scenario.ticks(), TICKS);
    let (replayed, report) = Game::new("t", 1, 1)
        .sim_system(play)
        .run_headless_scenario(scenario, spawn_player);
    report.assert_passed();
    let player = |w: &World| *w.query_ref::<&Player>().single().unwrap();
    let p = player(&live);
    assert!(p.at.x > 10.0 && p.jumps == 2 && p.zoom == 2.0 && p.lean > 0.0, "{p:?}");
    assert_eq!(player(&replayed), p);
    assert_eq!(world_hash(&replayed), world_hash(&live));
}

#[test]
fn each_event_is_recorded_on_the_tick_that_first_sees_it() {
    let (_, scenario) = record_live();
    let pad = Gamepad::from_slot(0).unwrap();
    // Pointer moves and stick readings within a tick keep only the last.
    let expected = InputScenario::new()
        .at(2, InputEvent::Key(Key::D, true))
        .at(5, InputEvent::Key(Key::Space, true))
        .at(5, InputEvent::MouseMove(vec2(30.0, 5.0)))
        .at(6, InputEvent::Key(Key::Space, false))
        .at(9, InputEvent::MouseWheel(vec2(0.0, 2.0)))
        .at(12, InputEvent::GamepadAxis(pad, GamepadAxis::LeftStickX, 0.75))
        .at(17, InputEvent::Key(Key::D, false))
        .at(20, InputEvent::Key(Key::Space, true))
        .at(20, InputEvent::Key(Key::Space, false))
        .ime(25, ImeEvent::Commit("hi".into()))
        .at(31, InputEvent::FocusLost)
        .duration(TICKS);
    assert_eq!(scenario.to_json(), expected.to_json());
}

#[test]
fn a_recording_started_from_a_system_begins_on_the_next_tick_with_what_is_held() {
    let world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            1 => ctx.input.feed(InputEvent::Key(Key::W, true)),
            3 => {
                ctx.input.feed(InputEvent::MouseMove(vec2(4.0, 2.0)));
                ctx.input.start_recording(); // tick 0 is tick 4
                assert!(ctx.input.is_recording());
            }
            5 => ctx.input.feed(InputEvent::Key(Key::W, false)),
            6 => {
                let recorded = ctx.input.stop_recording();
                assert!(!ctx.input.is_recording());
                assert!(ctx.input.stop_recording().is_none(), "already stopped");
                ctx.world.insert_resource(Recorded(recorded));
            }
            _ => {}
        })
        .run_headless(8, |_| {});
    let scenario = world.resource::<Recorded>().0.clone().unwrap();
    let expected = InputScenario::new()
        .at(0, InputEvent::MouseMove(vec2(4.0, 2.0)))
        .at(0, InputEvent::Key(Key::W, true))
        .at(1, InputEvent::Key(Key::W, false))
        .duration(3);
    assert_eq!(scenario.to_json(), expected.to_json());
}

#[test]
fn a_scenario_played_while_recording_is_recorded_too() {
    let scenario = InputScenario::new()
        .at(1, InputEvent::Key(Key::A, true))
        .at(3, InputEvent::Key(Key::A, false))
        .duration(5);
    let world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 4 {
                let recorded = ctx.input.stop_recording();
                ctx.world.insert_resource(Recorded(recorded));
            }
        })
        .input_scenario(scenario.clone())
        .run_headless(5, |ctx| ctx.input.start_recording());
    let recorded = world.resource::<Recorded>().0.clone().unwrap();
    assert_eq!(recorded.to_json(), scenario.to_json());
}
