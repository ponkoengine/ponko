//! A guard walks a patrol route, lap after lap.

use ponko::prelude::*;
use serde::{Deserialize, Serialize};

/// `assets/patrol.json`: the route and the guard's speed.
#[derive(Serialize, Deserialize)]
struct Route {
    /// Pixels per second.
    speed: f32,
    /// The corners, in order; after the last the guard walks back to the first.
    waypoints: Vec<Vec2>,
}

/// The guard.
#[derive(Component, Serialize, Deserialize)]
struct Guard {
    /// The waypoint it walks to.
    next: usize,
    /// Laps finished.
    laps: u32,
}

/// Sent when the guard reaches a waypoint.
#[derive(Serialize)]
struct Reached {
    waypoint: usize,
    at: Vec2,
    tick: u64,
}

fn main() {
    Game::new("patrol", 800, 600)
        .control_endpoint(true)
        .hot_reload(true)
        .input_injection(true)
        .sim_system(patrol)
        .run_headless(u64::MAX, |ctx| {
            ctx.assets.load_resource::<Route>(&mut ctx.world, "patrol.json");
            ctx.world.reflect::<Guard>();
            ctx.world.reflect::<d2::Transform>();
            ctx.world.reflect_resource::<Route>();
            ctx.events.reflect::<Reached>();
            let start = ctx.world.resource::<Route>().waypoints[0];
            ctx.world.spawn((Guard { next: 1, laps: 0 }, d2::Transform::at(start)));
        });
}

/// Sim system: the guard walks toward its next waypoint, and turns to the
/// one after when it gets there.
fn patrol(ctx: &mut Ctx) {
    let (speed, route) = {
        let r = ctx.world.resource::<Route>();
        (r.speed, r.waypoints.clone())
    };
    let step = speed * ctx.time.dt();
    let tick = ctx.time.sim_tick();
    let mut reached = Vec::new();
    ctx.world
        .query::<(&mut Guard, &mut d2::Transform)>()
        .for_each(|(guard, t)| {
            let target = route[guard.next];
            let to = target - t.position;
            if to.length() == 0.0 {
                reached.push(Reached {
                    waypoint: guard.next,
                    at: t.position,
                    tick,
                });
                guard.next = (guard.next + 1) % route.len();
                if guard.next == 1 {
                    guard.laps += 1;
                }
            } else {
                t.position += to.normalize() * step;
            }
        });
    for r in reached {
        ctx.events.send(r);
    }
}
