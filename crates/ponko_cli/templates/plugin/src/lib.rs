//! A plugin for the [Ponko](https://ponko.dev/) game engine: things that
//! pulse.
//!
//! Add [`PulsePlugin`] to a game, then give entities a [`Pulse`]:
//!
//! ```
//! use ponko::prelude::*;
//! use {{crate_name}}::{Pulse, PulsePlugin};
//!
//! let mut world = Game::new("demo", 1, 1)
//!     .plugin(PulsePlugin)
//!     .setup(|ctx| {
//!         ctx.world.spawn((Pulse::new(1.0),));
//!     })
//!     .run_headless(30);
//! let phase = world.query::<&Pulse>().iter().next().unwrap().phase;
//! assert!(phase > 0.0);
//! ```
//!
//! Everything it adds to a game is named after the crate: the debug toggle
//! [`TOGGLE`] and the control method [`SET_RATE`].

use ponko::prelude::*;
use ponko::reflect::{JsonValue, WorldReflect};

/// The debug toggle that draws each pulse as a circle around its
/// `ponko::d2::Transform`: `ctx.debug.set_toggle`, or the control
/// endpoint's `debug.draw` method.
pub const TOGGLE: &str = concat!(env!("CARGO_CRATE_NAME"), ".pulses");

/// The control method that sets every pulse's rate: params
/// `{"rate": number}`, answers how many pulses changed.
pub const SET_RATE: &str = concat!(env!("CARGO_CRATE_NAME"), ".set_rate");

/// Something that pulses: `phase` goes from 0 to 1, `rate` times a second,
/// and starts again.
#[derive(Component, serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    /// Pulses per second.
    pub rate: f32,
    /// Where in the pulse it is, from 0 to 1.
    pub phase: f32,
}

impl Pulse {
    /// A pulse at the start of its cycle, `rate` times a second.
    #[must_use]
    pub fn new(rate: f32) -> Self {
        Self { rate, phase: 0.0 }
    }
}

/// The plugin: advances every [`Pulse`] each simulation tick, reflects it
/// for tools and saves, and adds the debug toggle [`TOGGLE`] and the
/// control method [`SET_RATE`]. Add it once.
pub struct PulsePlugin;

impl Plugin for PulsePlugin {
    fn build(&self, game: Game) -> Game {
        game.add(Phase::AfterPhysics, advance_pulses)
            .add(Phase::Draw, draw_pulses)
            .debug_toggle(TOGGLE, false)
            .control_method(
                SET_RATE,
                "sets every pulse's rate: params {\"rate\": number}",
                set_rate,
            )
    }

    fn setup(&self, ctx: &mut Ctx) {
        ctx.world.reflect::<Pulse>();
    }
}

/// Moves every pulse along by one tick.
fn advance_pulses(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world.query::<&mut Pulse>().for_each(|p| {
        p.phase = (p.phase + p.rate * dt).fract();
    });
}

/// With [`TOGGLE`] on, draws each pulse that has a 2D position.
fn draw_pulses(ctx: &mut Ctx) {
    if !ctx.debug.toggled(TOGGLE) {
        return;
    }
    let debug = &mut ctx.debug;
    ctx.world
        .query::<(&Pulse, &ponko::d2::Transform)>()
        .for_each(|(p, t)| {
            debug.circle(t.position, 8.0 + 8.0 * p.phase, Color::YELLOW);
        });
}

/// The control method [`SET_RATE`].
fn set_rate(ctx: &mut Ctx, params: &JsonValue) -> Result<JsonValue, Error> {
    let rate = params["rate"]
        .as_f64()
        .ok_or_else(|| Error::new(ErrorKind::Invalid, "`rate` must be a number"))?
        as f32;
    let mut changed = 0;
    ctx.world.query::<&mut Pulse>().for_each(|p| {
        p.rate = rate;
        changed += 1;
    });
    Ok(json!({ "changed": changed }))
}
