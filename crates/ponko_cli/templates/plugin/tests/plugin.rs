//! Games built with the plugin, run headless. `cargo test` runs them.

use ponko::prelude::*;
use ponko::reflect::JsonValue;

use {{crate_name}}::{Pulse, PulsePlugin, SET_RATE, TOGGLE};

fn game() -> ConfiguredGame {
    Game::new("pulses", 1, 1).plugin(PulsePlugin).setup(|ctx| {
        ctx.world.spawn((Pulse::new(1.0),));
        ctx.world.spawn((Pulse::new(2.0),));
    })
}

#[test]
fn pulses_advance_every_tick_and_start_again() {
    // 60 ticks a second: half a second.
    let mut world = game().run_headless(30);
    let mut phases: Vec<f32> = world.query::<&Pulse>().iter().map(|p| p.phase).collect();
    phases.sort_by(f32::total_cmp);
    assert!(
        phases[0] < 1e-3,
        "the faster one went round once: {phases:?}"
    );
    assert!(
        (phases[1] - 0.5).abs() < 1e-3,
        "the slower one is half way: {phases:?}"
    );
}

#[test]
fn the_plugin_is_added_once() {
    let started = Game::new("twice", 1, 1)
        .plugin(PulsePlugin)
        .plugin(PulsePlugin)
        .try_run_headless(0, |_| {});
    let error = started.err().expect("adding it twice is an error");
    assert_eq!(error.kind(), ErrorKind::Invalid);
}

#[test]
fn tools_see_the_component_the_toggle_and_the_method() {
    Game::new("tools", 1, 1)
        .debug_draw(true)
        .plugin(PulsePlugin)
        .run_headless(0, |ctx| {
            let report: JsonValue =
                ponko::reflect::json::from_str(&ctx.capabilities_json()).unwrap();
            let named = |list: &JsonValue, name: &str| {
                list.as_array()
                    .unwrap()
                    .iter()
                    .any(|item| item["name"] == name)
            };
            assert!(named(&report["reflection"]["components"], "Pulse"));
            assert!(named(&report["methods"], SET_RATE));
            assert!(!ctx.debug.toggled(TOGGLE), "the toggle starts off");
            ctx.debug.set_toggle(TOGGLE, true).unwrap();
            assert!(ctx.debug.toggled(TOGGLE));
        });
}
