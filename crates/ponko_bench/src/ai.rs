//! The AI kit at crowd scale (`ai/...`): 10,000 agents deciding what to
//! do every tick, timed over the decision only (senses and actions run
//! untimed around it, so blackboards change and actions end).
//!
//! The behaviour, both ways: flee when health is below 0.3; chase while
//! alert; otherwise a utility choice, with a little stickiness, between
//! eating (hunger, quadratic), going home to rest (tiredness, smooth) and
//! a patrol leg followed by a one-second wait.
//!
//! - `10k-plain`: a hand-written `match` over a mood enum in a parallel
//!   query, the plain-systems way.
//! - `10k-tree`: the same behaviour as a JSON tree, `ai::think`, every
//!   brain every tick.
//! - `10k-tree-lod`: the tree with think periods for a crowd: 10% of the
//!   agents every tick, 30% every 4th, 60% every 16th.
//! - `10k-plain-1t`, `10k-tree-1t`: the first two on one thread (the cost
//!   per agent, steadier than the parallel runs on a busy machine).
use std::hint::black_box;
use std::time::{Duration, Instant};

use ponko::ai::{self, BehaviorTree, Blackboard, Brain, Curve, Key};
use ponko::prelude::*;

use crate::{Bench, alloc, report};

const AGENTS: u32 = 10_000;
const WARMUP: usize = 30;
const TICKS: usize = 300;

const HEALTH: Key = Key::new("health");
const ALERT: Key = Key::new("alert");
const HUNGER: Key = Key::new("hunger");
const TIRED: Key = Key::new("tired");

const TREE: &str = r#"{
  "blackboard": {"health": 1.0, "alert": false, "hunger": 0.0, "tired": 0.0},
  "root": {"selector": [
    {"sequence": [{"check": "health", "below": 0.3}, {"do": "flee"}]},
    {"sequence": [{"check": "alert"}, {"do": "chase", "speed": 4}]},
    {"utility": [
      {"score": [{"key": "hunger", "from": 0, "to": 100, "curve": "quadratic"}], "do": "eat"},
      {"score": [{"key": "tired", "from": 0, "to": 1, "curve": "smooth"}],
       "sequence": [{"do": "go_home"}, {"do": "rest"}]},
      {"score": 0.3, "sequence": [{"do": "patrol"}, {"wait": 1.0}]}
    ], "stick": 0.05}
  ]}
}"#;

/// The senses, the same for both: a hash of the agent and the tick, so
/// every agent's inputs drift and its choices change now and then.
fn senses(index: u32, tick: u64) -> (f32, bool, f32, f32) {
    let x = (u64::from(index) * 2_654_435_761 + (tick / 30) * 40_503) % 10_007;
    let health = if x.is_multiple_of(23) { 0.2 } else { 1.0 };
    let alert = x.is_multiple_of(11);
    let hunger = (x % 100) as f32;
    let tired = (x % 37) as f32 / 36.0;
    (health, alert, hunger, tired)
}

/// Actions end on their own after a while (untimed, both ways).
fn ends(index: u32, tick: u64) -> bool {
    (u64::from(index) + tick).is_multiple_of(45)
}

// ---- plain ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mood {
    Flee,
    Chase,
    Eat,
    GoHome,
    Rest,
    Patrol,
    Wait,
}

#[derive(Component)]
struct Agent {
    health: f32,
    alert: bool,
    hunger: f32,
    tired: f32,
    mood: Mood,
    done: bool,
    waited: u32,
}

/// One tick of the plain version: what the tree does, written by hand.
fn decide(a: &mut Agent) {
    if a.health < 0.3 {
        a.mood = Mood::Flee;
        return;
    }
    if a.alert {
        a.mood = Mood::Chase;
        return;
    }
    // Multi-step moods carry on.
    match (a.mood, a.done) {
        (Mood::GoHome, true) => {
            a.mood = Mood::Rest;
            a.done = false;
            return;
        }
        (Mood::Patrol, true) => {
            a.mood = Mood::Wait;
            a.waited = 0;
            a.done = false;
            return;
        }
        (Mood::Wait, _) => {
            a.waited += 1;
            if a.waited < 60 {
                return;
            }
        }
        _ => {}
    }
    let eat = Curve::Quadratic.eval(ai::normalize(a.hunger, 0.0, 100.0));
    let rest = Curve::Smooth.eval(a.tired);
    let patrol = 0.3;
    let stick = |m: &[Mood]| if m.contains(&a.mood) && !a.done { 0.05 } else { 0.0 };
    let options = [
        (Mood::Eat, eat + if eat > 0.0 { stick(&[Mood::Eat]) } else { 0.0 }),
        (
            Mood::GoHome,
            rest + if rest > 0.0 {
                stick(&[Mood::GoHome, Mood::Rest])
            } else {
                0.0
            },
        ),
        (Mood::Patrol, patrol + stick(&[Mood::Patrol, Mood::Wait])),
    ];
    if let Some((mood, _)) = ai::best(options) {
        let keep = matches!((a.mood, mood), (Mood::Rest, Mood::GoHome) | (Mood::Wait, Mood::Patrol));
        if mood != a.mood && !keep {
            a.mood = mood;
            a.done = false;
        }
    }
}

// ---- runs ----------------------------------------------------------------------

/// Every timed tick, the allocations in them, and how many agents were
/// doing something at the end.
struct Times(Vec<Duration>, alloc::Counts, usize);

fn timed(ctx: &mut Ctx, f: impl FnOnce(&mut Ctx)) {
    let before = alloc::Counts::now();
    let start = Instant::now();
    f(ctx);
    let took = start.elapsed();
    let counted = alloc::Counts::now().since(before);
    let tick = ctx.time.sim_tick() as usize;
    let times = ctx.world.resource_mut::<Times>();
    if tick >= WARMUP {
        times.0.push(took);
        times.1.count += counted.count;
        times.1.bytes += counted.bytes;
    }
}

fn run_plain(parallel: bool) -> Times {
    let mut world = Game::new("ai bench", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Agent)>().for_each(|(e, a)| {
                (a.health, a.alert, a.hunger, a.tired) = senses(e.index(), tick);
            });
        })
        .sim_system(move |ctx: &mut Ctx| {
            timed(ctx, |ctx| {
                if parallel {
                    ctx.world.query::<&mut Agent>().par_for_each(decide);
                } else {
                    ctx.world.query::<&mut Agent>().for_each(decide);
                }
            });
        })
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Agent)>().for_each(|(e, a)| {
                if ends(e.index(), tick) {
                    a.done = true;
                }
            });
        })
        .run_headless((WARMUP + TICKS) as u64, |ctx| {
            ctx.world
                .insert_resource(Times(Vec::new(), alloc::Counts::default(), 0));
            for _ in 0..AGENTS {
                ctx.world.spawn((Agent {
                    health: 1.0,
                    alert: false,
                    hunger: 0.0,
                    tired: 0.0,
                    mood: Mood::Patrol,
                    done: false,
                    waited: 0,
                },));
            }
        });
    let busy = world.query::<&Agent>().iter().filter(|a| a.mood != Mood::Wait).count();
    let mut times = world.remove_resource::<Times>().expect("the times");
    times.2 = busy;
    black_box(&world);
    times
}

/// `ai::think` on one thread: the same work, brain by brain.
fn think_serially(ctx: &mut Ctx) {
    let (tick, dt) = (ctx.time.sim_tick(), ctx.time.dt());
    let tree = ctx
        .world
        .resource::<ai::BehaviorTrees>()
        .get("crowd")
        .expect("the tree")
        .clone();
    ctx.world
        .query::<(Entity, &mut Brain, &mut Blackboard)>()
        .for_each(|(e, brain, bb)| {
            if brain.due(e.index(), tick) {
                brain.think(&tree, bb, tick, dt);
            }
        });
}

fn run_tree(lod: bool, parallel: bool) -> Times {
    let mut world = Game::new("ai bench", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Blackboard)>().for_each(|(e, bb)| {
                let (health, alert, hunger, tired) = senses(e.index(), tick);
                bb.set(HEALTH, health);
                bb.set(ALERT, alert);
                bb.set(HUNGER, hunger);
                bb.set(TIRED, tired);
            });
        })
        .sim_system(move |ctx: &mut Ctx| timed(ctx, if parallel { ai::think } else { think_serially }))
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Brain)>().for_each(|(e, b)| {
                if ends(e.index(), tick) {
                    b.succeed();
                }
            });
        })
        .run_headless((WARMUP + TICKS) as u64, move |ctx| {
            ctx.world
                .insert_resource(Times(Vec::new(), alloc::Counts::default(), 0));
            ai::add_tree(
                &mut ctx.world,
                "crowd",
                BehaviorTree::from_json(TREE).expect("a valid tree"),
            );
            for i in 0..AGENTS {
                let period = match (lod, i % 10) {
                    (false, _) | (true, 0) => 1,
                    (true, 1..=3) => 4,
                    _ => 16,
                };
                ctx.world
                    .spawn((Brain::new("crowd").with_period(period), Blackboard::new()));
            }
        });
    let busy = world.query::<&Brain>().iter().filter(|b| b.action().is_some()).count();
    let mut times = world.remove_resource::<Times>().expect("the times");
    times.2 = busy;
    times
}

type Run = (&'static str, fn() -> Times);

pub fn run(b: &mut Bench) {
    let runs: [Run; 5] = [
        ("ai/10k-plain", || run_plain(true)),
        ("ai/10k-tree", || run_tree(false, true)),
        ("ai/10k-tree-lod", || run_tree(true, true)),
        ("ai/10k-plain-1t", || run_plain(false)),
        ("ai/10k-tree-1t", || run_tree(false, false)),
    ];
    for (name, run) in runs {
        if b.skips(name) {
            continue;
        }
        let Times(mut times, allocs, busy) = run();
        eprintln!("{name}: {busy} of {AGENTS} agents busy at the end");
        let mut record = report::Record::new(name, report::Kind::Steady, WARMUP, &mut times);
        record.set_allocs(allocs, TICKS);
        b.report(record);
    }
    if !b.skips("ai/10k-tree") {
        let tree = BehaviorTree::from_json(TREE).expect("a valid tree");
        eprintln!(
            "ai: tree of {} nodes; Brain {} B + 24 B a node, Blackboard {} B + {} B an entry",
            tree.len(),
            std::mem::size_of::<Brain>(),
            std::mem::size_of::<Blackboard>(),
            std::mem::size_of::<(Key, ponko::ai::Value)>(),
        );
    }
}
