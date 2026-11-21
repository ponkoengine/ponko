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

/// What the game read from the pad, for comparing a live run with its replay.
#[derive(Debug, Default, Clone, PartialEq)]
struct PadSeen {
    /// The gyro's yaw rate, summed over every tick.
    yaw: f32,
    presses: u32,
    /// The pad's name and family while it was connected.
    name: String,
    family: Option<ponko::input::ControllerFamily>,
    connected_ticks: u32,
}

fn read_pad(ctx: &mut Ctx) {
    let pad = Gamepad::from_slot(0).unwrap();
    let mut seen = ctx.world.remove_resource::<PadSeen>().unwrap_or_default();
    if let Some(motion) = ctx.input.gamepad_motion(pad) {
        seen.yaw += motion.gyro.y;
    }
    if ctx.input.gamepad_pressed(pad, ponko::input::GamepadButton::South) {
        seen.presses += 1;
    }
    if let Some(info) = ctx.input.gamepad_info(pad) {
        seen.name = info.name.clone();
        seen.family = Some(info.family);
        seen.connected_ticks += 1;
    }
    ctx.world.insert_resource(seen);
}

fn dualsense() -> ponko::input::GamepadInfo {
    ponko::input::GamepadInfo::new("DualSense Wireless Controller")
        .ids(0x054c, 0x0ce6)
        .capabilities(ponko::input::GamepadCapabilities {
            rumble: true,
            trigger_rumble: false,
            motion: true,
        })
}

#[test]
fn a_pad_plugged_in_its_motion_and_its_unplugging_are_recorded_and_replay_the_same() {
    use ponko::input::{FakeGamepads, GamepadButton, GamepadMotion};
    let fake = FakeGamepads::new();
    let script = fake.clone();
    let mut live = Game::new("t", 1, 1)
        .gamepad_backend(fake)
        .sim_system(move |ctx: &mut Ctx| {
            // What a backend reports during a tick reaches the next one.
            let motion = |y: f32| GamepadMotion {
                gyro: vec3(0.0, y, 0.0),
                accel: vec3(0.0, 9.81, 0.0),
            };
            match ctx.time.sim_tick() {
                1 => {
                    script.connect(dualsense());
                }
                3 => {
                    script.set_motion(ponko::input::DeviceKey(0), motion(1.0));
                    script.set_motion(ponko::input::DeviceKey(0), motion(2.0));
                }
                4 => script.press(ponko::input::DeviceKey(0), GamepadButton::South),
                6 => {
                    script.release(ponko::input::DeviceKey(0), GamepadButton::South);
                    script.set_motion(ponko::input::DeviceKey(0), motion(-0.5));
                }
                8 => script.disconnect(ponko::input::DeviceKey(0)),
                10 => {
                    let recorded = ctx.input.stop_recording();
                    ctx.world.insert_resource(Recorded(recorded));
                }
                _ => {}
            }
        })
        .sim_system(read_pad)
        .run_headless(11, |ctx| ctx.input.start_recording());
    let scenario = live.remove_resource::<Recorded>().unwrap().0.unwrap();
    let pad = Gamepad::from_slot(0).unwrap();
    let motion = |y: f32| {
        InputEvent::GamepadMotion(
            pad,
            GamepadMotion {
                gyro: vec3(0.0, y, 0.0),
                accel: vec3(0.0, 9.81, 0.0),
            },
        )
    };
    // Two motion readings between two ticks keep only the last.
    let expected = InputScenario::new()
        .connect_gamepad(2, pad, dualsense())
        .at(4, motion(2.0))
        .at(5, InputEvent::GamepadButton(pad, GamepadButton::South, true))
        .at(7, InputEvent::GamepadButton(pad, GamepadButton::South, false))
        .at(7, motion(-0.5))
        .at(9, InputEvent::GamepadDisconnected(pad))
        .duration(11);
    assert_eq!(scenario.to_json(), expected.to_json());
    let step = &scenario.to_json()["steps"][0];
    assert_eq!(step["type"], "gamepad_connected");
    assert_eq!(step["info"]["name"], "DualSense Wireless Controller");
    assert_eq!(step["info"]["family"], "PlayStation");
    assert_eq!(step["info"]["vendor_id"], 0x054c);
    assert_eq!(step["info"]["motion"], true);
    let again = InputScenario::from_value(&scenario.to_json()).unwrap();
    assert_eq!(again.to_json(), scenario.to_json(), "the JSON round-trips");

    // Replayed with no gamepad backend at all: the same pad, readings and presses.
    let (replayed, report) = Game::new("t", 1, 1)
        .sim_system(read_pad)
        .run_headless_scenario(again, |_| {});
    report.assert_passed();
    let live_seen = live.resource::<PadSeen>().clone();
    assert_eq!(live_seen.name, "DualSense Wireless Controller");
    assert_eq!(live_seen.family, Some(ponko::input::ControllerFamily::PlayStation));
    assert_eq!((live_seen.presses, live_seen.connected_ticks), (1, 7));
    assert_eq!(*replayed.resource::<PadSeen>(), live_seen);
}

#[test]
fn a_recording_started_with_a_pad_connected_begins_with_that_pad_and_its_last_motion() {
    use ponko::input::{FakeGamepads, GamepadMotion};
    let fake = FakeGamepads::new();
    fake.connect(dualsense());
    let reading = GamepadMotion {
        gyro: vec3(0.1, 0.2, 0.3),
        accel: vec3(0.0, 9.81, 0.0),
    };
    let world = Game::new("t", 1, 1)
        .gamepad_backend(fake)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                let recorded = ctx.input.stop_recording();
                ctx.world.insert_resource(Recorded(recorded));
            }
        })
        .run_headless(3, move |ctx| {
            // The backend was polled before setup: the pad is here already.
            ctx.input
                .feed(InputEvent::GamepadMotion(Gamepad::from_slot(0).unwrap(), reading));
            ctx.input.start_recording();
        });
    let scenario = world.resource::<Recorded>().0.clone().unwrap();
    let pad = Gamepad::from_slot(0).unwrap();
    let expected = InputScenario::new()
        .connect_gamepad(0, pad, dualsense())
        .at(0, InputEvent::GamepadMotion(pad, reading))
        .duration(3);
    assert_eq!(scenario.to_json(), expected.to_json());
}

#[test]
fn a_gamepad_connected_step_without_info_connects_a_generic_pad_and_writes_no_info() {
    let pad = Gamepad::from_slot(1).unwrap();
    let scenario = InputScenario::from_json(
        r#"{"version": 1, "steps": [
            {"tick": 0, "type": "gamepad_connected", "pad": 1},
            {"tick": 0, "type": "gamepad_connected", "pad": 2,
             "info": {"name": "Pro Controller", "family": "Nintendo", "trigger_rumble": true}}
        ]}"#,
    )
    .unwrap();
    let json = scenario.to_json();
    assert_eq!(InputScenario::from_value(&json).unwrap().to_json(), json);
    assert!(json["steps"][0].get("info").is_none());
    assert_eq!(json["steps"][1]["info"]["family"], "Nintendo");
    assert!(
        json["steps"][1]["info"].get("rumble").is_none(),
        "false capabilities are left out"
    );
    let (world, report) = Game::new("t", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            let generic = ctx.input.gamepad_info(pad).unwrap();
            assert_eq!(generic.family, ponko::input::ControllerFamily::Generic);
            let pro = ctx.input.gamepad_info(Gamepad::from_slot(2).unwrap()).unwrap();
            assert_eq!(pro.name, "Pro Controller");
            assert_eq!(pro.family, ponko::input::ControllerFamily::Nintendo);
            assert!(pro.capabilities.trigger_rumble && !pro.capabilities.rumble);
            ctx.world.insert_resource(PadSeen::default());
        })
        .run_headless_scenario(InputScenario::from_value(&json).unwrap(), |_| {});
    report.assert_passed();
    assert!(world.get_resource::<PadSeen>().is_some(), "the checks ran");
    assert!(
        InputScenario::from_json(
            r#"{"version": 1, "steps": [{"tick": 0, "type": "gamepad_connected", "pad": 0, "info": {"nmae": "x"}}]}"#
        )
        .is_err(),
        "a misspelt info field is an error"
    );
}
