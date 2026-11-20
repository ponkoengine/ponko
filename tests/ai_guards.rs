//! The `ai_guards` example, played headlessly: guards driven by the tree
//! in `assets/ai/guard.json` chase the intruder, catch it, get hurt, go
//! home and rest; far guards think less often; two runs match.
#[path = "../examples/ai_guards/game.rs"]
mod game;

use game::*;
use ponko::ai::{Blackboard, Brain};
use ponko::d2::Transform;
use ponko::prelude::*;

/// What one run ended with: the tally, each guard's position, action and
/// period, and every action seen during the run.
struct Run {
    tally: Tally,
    guards: Vec<(Vec2, Option<&'static str>, u32)>,
    seen: std::collections::BTreeSet<&'static str>,
}

fn play(ticks: u64) -> Run {
    #[derive(Default)]
    struct Seen(std::collections::BTreeSet<&'static str>);
    let mut world = game()
        .sim_system(|ctx: &mut Ctx| {
            let actions: Vec<_> = ctx.world.query::<&Brain>().iter().filter_map(Brain::action).collect();
            ctx.world
                .resource_mut::<Seen>()
                .0
                .extend(actions.iter().map(|k| k.name()));
        })
        .run_headless(ticks, |ctx| {
            ctx.world.insert_resource(Seen::default());
            setup(ctx, 12);
        });
    let guards = world
        .query::<(&Transform, &Brain, &Guard)>()
        .iter()
        .map(|(t, b, _)| (t.position, b.action().map(|k| k.name()), b.period()))
        .collect();
    Run {
        tally: world.resource::<Tally>().clone(),
        guards,
        seen: world.remove_resource::<Seen>().unwrap().0,
    }
}

#[test]
fn guards_patrol_chase_catch_and_rest_as_their_tree_says() {
    let run = play(60 * 60);
    assert_eq!(run.guards.len(), 12);
    let seen: Vec<_> = run.seen.iter().copied().collect();
    assert_eq!(seen, ["chase", "go_home", "patrol", "rest"]);
    assert!(run.tally.catches >= 1, "{:?}", run.tally);
    assert!(run.tally.rests >= 1, "{:?}", run.tally);
}

#[test]
fn far_guards_think_less_often() {
    let run = play(120);
    let periods: std::collections::BTreeSet<u32> = run.guards.iter().map(|g| g.2).collect();
    assert!(periods.contains(&1) && periods.iter().any(|&p| p > 1), "{periods:?}");
}

#[test]
fn two_runs_of_the_guards_match() {
    let (a, b) = (play(900), play(900));
    assert_eq!(a.guards, b.guards);
    assert_eq!(a.tally.catches, b.tally.catches);
}

#[test]
fn every_guard_gets_the_tree_defaults() {
    let mut world = game().run_headless(1, |ctx| setup(ctx, 4));
    world.query::<&Blackboard>().for_each(|bb| {
        assert_eq!(bb.num(HEALTH), 1.0);
        assert!(bb.contains(TIRED));
    });
}
