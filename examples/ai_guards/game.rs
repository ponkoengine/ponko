//! The guards of `ai_guards`, without drawing: shared by the example and
//! its headless test (`tests/ai_guards.rs`).
//!
//! Guards walk their rounds, chase an intruder they see, go home to rest
//! when tired or hurt. What they do is decided by the behaviour tree in
//! `assets/ai/guard.json`; these systems only write what each guard sees
//! (its blackboard) and carry out the action its brain picked.
use ponko::ai::{self, Blackboard, Brain, Key};
use ponko::d2::Transform;
use ponko::prelude::*;

// ANCHOR: keys
/// The blackboard entries and actions the tree names.
pub const ALERT: Key = Key::new("alert");
pub const HEALTH: Key = Key::new("health");
pub const TIRED: Key = Key::new("tired");
pub const CHASE: Key = Key::new("chase");
pub const GO_HOME: Key = Key::new("go_home");
pub const REST: Key = Key::new("rest");
pub const PATROL: Key = Key::new("patrol");
pub const SPEED: Key = Key::new("speed");
// ANCHOR_END: keys

/// The tree file, under `assets/`.
pub const TREE: &str = "ai/guard.json";
/// How far a guard sees, and how far it keeps chasing.
pub const SIGHT: f32 = 160.0;
pub const GIVE_UP: f32 = 240.0;
/// How close a guard must get to catch the intruder.
pub const CATCH: f32 = 14.0;
/// The size of the yard.
pub const YARD: Vec2 = Vec2::new(1200.0, 700.0);

/// A guard: where it rests, and the two ends of its round.
#[derive(Component, Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Guard {
    pub home: Vec2,
    pub round: [Vec2; 2],
    pub leg: usize,
}

/// The intruder: walks a loop around the yard; caught, it starts over.
#[derive(Component, Debug, Clone, Default)]
pub struct Intruder {
    pub caught: u32,
}

/// How the run went, for the test and the window's title line.
#[derive(Debug, Default, Clone)]
pub struct Tally {
    pub catches: u32,
    pub rests: u32,
}

/// The intruder's position at sim time `t` seconds: a figure of eight
/// across the yard.
pub fn intruder_path(t: f32) -> Vec2 {
    let s = t * 0.35;
    YARD / 2.0 + vec2(s.sin() * 480.0, (2.0 * s).sin() * 220.0)
}

// ANCHOR: senses
/// Senses: what each guard knows, written to its blackboard. Also sets how
/// often it thinks: every tick near the intruder, less often far away.
pub fn sense(ctx: &mut Ctx) {
    let intruder = ctx
        .world
        .query::<(&Transform, &Intruder)>()
        .iter()
        .next()
        .map(|(t, _)| t.position);
    let Some(intruder) = intruder else { return };
    ctx.world
        .query::<(&Transform, &mut Blackboard, &mut Brain)>()
        .for_each(|(t, bb, brain)| {
            let d = t.position.distance(intruder);
            // Sees it within SIGHT; once alert, keeps chasing to GIVE_UP.
            let alert = d < SIGHT || (bb.flag(ALERT) && d < GIVE_UP);
            bb.set(ALERT, alert);
            brain.set_period(ai::lod_period(d, &[(300.0, 1), (600.0, 4)], 12));
        });
}
// ANCHOR_END: senses

/// Moves `from` toward `to` by `step`; `true` once there.
fn walk(from: &mut Vec2, to: Vec2, step: f32) -> bool {
    let left = to - *from;
    if left.length() <= step {
        *from = to;
        true
    } else {
        *from += left.normalize() * step;
        false
    }
}

// ANCHOR: act
/// Actions: each guard does what its brain picked, and says when it's done.
pub fn act(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    let intruder = ctx
        .world
        .query::<(&Transform, &Intruder)>()
        .iter()
        .next()
        .map(|(t, _)| t.position);
    let Some(intruder) = intruder else { return };
    let (mut catches, mut rests) = (0, 0);
    ctx.world
        .query::<(&mut Transform, &mut Guard, &mut Brain, &mut Blackboard)>()
        .for_each(|(t, guard, brain, bb)| {
            let step = brain.param_num(SPEED).unwrap_or(50.0) * dt;
            if brain.doing(CHASE) {
                bb.set(TIRED, (bb.num(TIRED) + 0.08 * dt).min(1.0));
                if walk(&mut t.position, intruder, step) || t.position.distance(intruder) < CATCH {
                    // A scuffle: the guard is hurt, the intruder starts over.
                    bb.set(HEALTH, bb.num(HEALTH) - 0.45);
                    bb.set(ALERT, false);
                    catches += 1;
                    brain.succeed();
                }
            } else if brain.doing(GO_HOME) {
                if walk(&mut t.position, guard.home, step) {
                    brain.succeed();
                }
            } else if brain.doing(REST) {
                if brain.started() {
                    rests += 1;
                }
                bb.set(HEALTH, (bb.num(HEALTH) + 0.25 * dt).min(1.0));
                bb.set(TIRED, (bb.num(TIRED) - 0.3 * dt).max(0.0));
                if bb.num(HEALTH) >= 1.0 && bb.num(TIRED) <= 0.05 {
                    brain.succeed();
                }
            } else if brain.doing(PATROL) {
                bb.set(TIRED, (bb.num(TIRED) + 0.02 * dt).min(1.0));
                if walk(&mut t.position, guard.round[guard.leg], step) {
                    guard.leg ^= 1;
                    brain.succeed();
                }
            }
        });
    let tally = ctx.world.resource_mut::<Tally>();
    tally.catches += catches;
    tally.rests += rests;
    if catches > 0 {
        ctx.world.query::<&mut Intruder>().for_each(|i| i.caught += 1);
    }
}
// ANCHOR_END: act

/// The intruder walks its path; each catch sets it back by ten seconds.
pub fn move_intruder(ctx: &mut Ctx) {
    let t = ctx.time.sim_tick() as f32 * ctx.time.dt();
    ctx.world.query::<(&mut Transform, &Intruder)>().for_each(|(tr, i)| {
        tr.position = intruder_path(t - i.caught as f32 * 10.0);
    });
}

// ANCHOR: game
/// The game's sim systems, in order: the intruder moves, guards sense,
/// think, then act.
pub fn game() -> Game {
    Game::new("Ponko: guards", 1280, 760)
        .sim_system(move_intruder)
        .sim_system(sense)
        .sim_system(ai::think)
        .sim_system(act)
}

/// Loads the tree and places `guards` guards around the yard.
pub fn setup(ctx: &mut Ctx, guards: usize) {
    ctx.assets.load_behavior_tree(&mut ctx.world, TREE);
    ctx.world.insert_resource(Tally::default());
    ctx.world.reflect::<Guard>();
    ctx.world
        .spawn((Transform::at(intruder_path(0.0)), Intruder::default()));
    for i in 0..guards {
        let col = (i % 4) as f32;
        let row = (i / 4) as f32;
        let home = vec2(150.0 + col * 300.0, 120.0 + row * 230.0);
        let round = [home + vec2(-90.0, 60.0), home + vec2(90.0, 60.0)];
        ctx.world.spawn((
            Transform::at(home),
            Guard { home, round, leg: 0 },
            Brain::new(TREE),
            Blackboard::new(),
        ));
    }
}
// ANCHOR_END: game
