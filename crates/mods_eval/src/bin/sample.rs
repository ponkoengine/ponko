//! The size sample: Ember Hop with a system that drives every drip faster
//! than 50 px/s (the mod's "frost drip") through the mod runtimes this build
//! has. Built, never run, for `ls -l target/release/sample`.
#[path = "../../../../examples/ember_hop/game.rs"]
#[allow(dead_code, unused_imports)]
mod game;

use ponko::prelude::*;
use std::cell::RefCell;

thread_local! {
    static RUNTIMES: RefCell<Vec<Box<dyn mods_eval::Runtime>>> = RefCell::new(mods_eval::runtimes());
}

fn frost_drips(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    ctx.world
        .query::<(&mut ponko::d2::Transform, &mut game::Drip)>()
        .for_each(|(t, d)| {
            if d.speed > 50.0 {
                RUNTIMES.with(|r| {
                    for rt in r.borrow_mut().iter_mut() {
                        (t.position.x, d.heading) = rt.update(t.position.x, d.heading, d.speed, dt);
                    }
                });
            }
        });
}

fn main() {
    game::build().sim_system(frost_drips).configured().run();
}
