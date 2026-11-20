//! The AI kit (`ponko::ai`): behaviour trees from JSON, utility scoring,
//! brains thinking at their period, hot reload, reflection and determinism.
use std::sync::Arc;

use ponko::ai::{self, BehaviorTree, BehaviorTrees, Blackboard, Brain, Key, Status, Value};
use ponko::assets::MemorySource;
use ponko::d2::Transform;
use ponko::error::ErrorKind;
use ponko::prelude::*;
use ponko::reflect::WorldReflect;

const DT: f32 = 1.0 / 60.0;
const ALERT: Key = Key::new("alert");
const HEALTH: Key = Key::new("health");
const HUNGER: Key = Key::new("hunger");

fn tree(json: &str) -> Arc<BehaviorTree> {
    Arc::new(BehaviorTree::from_json(json).unwrap_or_else(|e| panic!("{e}")))
}

/// Thinks once at tick `tick` and returns the action's name.
fn think(brain: &mut Brain, tree: &Arc<BehaviorTree>, bb: &mut Blackboard, tick: u64) -> Option<&'static str> {
    brain.think(tree, bb, tick, DT);
    brain.action().map(Key::name)
}

#[test]
fn a_sequence_runs_its_actions_in_order_as_the_game_reports_them() {
    let t = tree(r#"{"root": {"sequence": [{"do": "aim"}, {"do": "fire"}, {"do": "reload"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("gun"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("aim"));
    assert!(brain.started());
    // Not reported: it carries on, and isn't new.
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("aim"));
    assert!(!brain.started());
    assert_eq!(brain.status(), Status::Running);
    brain.succeed();
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("fire"));
    assert!(brain.started());
    brain.fail();
    // The sequence fails, and nothing runs; the next think starts it over.
    assert_eq!(think(&mut brain, &t, &mut bb, 3), None);
    assert_eq!(think(&mut brain, &t, &mut bb, 4), Some("aim"));
}

#[test]
fn a_higher_branch_takes_over_as_soon_as_its_check_passes() {
    let t = tree(
        r#"{"root": {"selector": [
            {"sequence": [{"check": "alert"}, {"do": "chase"}]},
            {"do": "patrol"}
        ]}}"#,
    );
    let (mut brain, mut bb) = (Brain::new("guard"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("patrol"));
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("patrol"));
    bb.set(ALERT, true);
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("chase"));
    assert!(brain.started());
    bb.set(ALERT, false);
    // Patrol was dropped while chasing, so it starts over.
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("patrol"));
    assert!(brain.started());
}

#[test]
fn a_running_sequence_resumes_its_step_but_asks_its_checks_again() {
    let t = tree(
        r#"{"root": {"selector": [
            {"sequence": [{"check": "alert"}, {"do": "shout"}, {"do": "chase"}]},
            {"do": "patrol"}
        ]}}"#,
    );
    let (mut brain, mut bb) = (Brain::new("guard"), Blackboard::new().with(ALERT, true));
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("shout"));
    brain.succeed();
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("chase"));
    // Shout is done: the sequence resumes at chase, it doesn't shout again.
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("chase"));
    assert!(!brain.started());
    // Its check is asked again: no longer alert, it stops.
    bb.set(ALERT, false);
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("patrol"));
    // Alert again: the sequence starts from the top.
    bb.set(ALERT, true);
    assert_eq!(think(&mut brain, &t, &mut bb, 4), Some("shout"));
}

#[test]
fn actions_read_their_parameters() {
    let t = tree(r#"{"root": {"do": "chase", "speed": 4.5, "goal": [1, 2], "loud": true}}"#);
    let (mut brain, mut bb) = (Brain::new("guard"), Blackboard::new());
    brain.think(&t, &mut bb, 0, DT);
    assert_eq!(brain.param_num(Key::new("speed")), Some(4.5));
    assert_eq!(brain.param(Key::new("goal")), Some(Value::Vec2(vec2(1.0, 2.0))));
    assert_eq!(brain.param(Key::new("loud")), Some(Value::Bool(true)));
    assert_eq!(brain.param(Key::new("missing")), None);
    assert_eq!(t.actions(), [Key::new("chase")]);
}

#[test]
fn checks_compare_with_numbers_and_other_entries_and_set_writes_the_blackboard() {
    let t = tree(
        r#"{"root": {"selector": [
            {"sequence": [{"check": "health", "below": 0.3}, {"set": "fled", "to": true}, {"do": "flee"}]},
            {"sequence": [{"check": "enemies", "above": "friends"}, {"do": "regroup"}]},
            {"sequence": [{"check": "stance", "is": 2}, {"do": "guard"}]},
            {"do": "idle"}
        ]}}"#,
    );
    let mut brain = Brain::new("unit");
    let mut bb = Blackboard::new().with(HEALTH, 1.0);
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("idle"));
    bb.set(Key::new("stance"), 2.0);
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("guard"));
    bb.set(Key::new("enemies"), 5.0);
    bb.set(Key::new("friends"), 3.0);
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("regroup"));
    bb.set(HEALTH, 0.2);
    assert!(!bb.flag(Key::new("fled")));
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("flee"));
    assert!(bb.flag(Key::new("fled")), "set wrote the blackboard");
    // A check on a missing entry, or one of another type, fails.
    let t = tree(r#"{"root": {"selector": [{"check": "nothing", "below": 1}, {"check": "health", "is": true}]}}"#);
    brain.think(&t, &mut bb, 4, DT);
    assert_eq!(brain.trace()[0].status, Some(Status::Failure));
}

#[test]
fn utility_picks_the_highest_score_and_falls_back_when_it_fails() {
    let t = tree(
        r#"{"root": {"utility": [
            {"score": [{"key": "hunger", "from": 0, "to": 100}], "do": "eat"},
            {"score": [{"key": "tired", "from": 0, "to": 100, "curve": "quadratic"}], "do": "sleep"},
            {"score": 0.6, "sequence": [{"check": "has_work"}, {"do": "work"}]},
            {"score": 0.05, "do": "idle"}
        ]}}"#,
    );
    let mut brain = Brain::new("villager");
    let mut bb = Blackboard::new().with(HUNGER, 50.0).with(Key::new("tired"), 90.0);
    // eat 0.5, sleep 0.81, work 0.6 (but no work: fails), idle 0.05.
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("sleep"));
    let scores: Vec<_> = brain.trace().iter().filter_map(|l| l.score).collect();
    assert_eq!(scores.len(), 4);
    assert!((scores[1] - 0.81).abs() < 1e-5, "{scores:?}");
    bb.set(Key::new("tired"), 0.0);
    // work scores 0.6 > eat 0.5, but its check fails: eat runs.
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("eat"));
    bb.set(HUNGER, 0.0);
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("idle"));
    bb.set(Key::new("has_work"), true);
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("work"));
    // Nothing above 0: the utility node fails.
    let t = tree(r#"{"root": {"utility": [{"score": 0, "do": "never"}]}}"#);
    assert_eq!(think(&mut brain, &t, &mut bb, 4), None);
}

#[test]
fn utility_ties_go_to_the_earlier_child() {
    let t = tree(r#"{"root": {"utility": [{"score": 0.5, "do": "a"}, {"score": 0.5, "do": "b"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("t"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("a"));
}

#[test]
fn stick_keeps_the_running_choice_against_a_slightly_better_one() {
    let json = |stick: f32| {
        format!(
            r#"{{"root": {{"utility": [
                {{"score": [{{"key": "hunger", "from": 0, "to": 100}}], "do": "eat"}},
                {{"score": 0.5, "do": "wander"}}
            ], "stick": {stick}}}}}"#
        )
    };
    for (stick, expected) in [(0.0, "eat"), (0.1, "wander")] {
        let t = tree(&json(stick));
        let mut brain = Brain::new("t");
        let mut bb = Blackboard::new().with(HUNGER, 40.0);
        assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("wander"));
        bb.set(HUNGER, 55.0); // eat 0.55 against wander 0.5 (+ stick)
        assert_eq!(think(&mut brain, &t, &mut bb, 1), Some(expected), "stick {stick}");
    }
}

#[test]
fn wait_and_cooldown_count_sim_ticks() {
    // 0.1 s at 60 ticks a second: 6 ticks.
    let t = tree(r#"{"root": {"sequence": [{"wait": 0.1}, {"do": "go"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("t"), Blackboard::new());
    for tick in 0..6 {
        assert_eq!(think(&mut brain, &t, &mut bb, tick), None, "tick {tick}");
    }
    assert_eq!(think(&mut brain, &t, &mut bb, 6), Some("go"));

    let t = tree(r#"{"root": {"selector": [{"cooldown": 0.05, "child": {"do": "shoot"}}, {"do": "reload"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("t"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("shoot"));
    brain.succeed();
    // Shot at tick 1 (the selector succeeds): cooling down for 3 ticks.
    assert_eq!(think(&mut brain, &t, &mut bb, 1), None);
    for tick in 2..4 {
        assert_eq!(think(&mut brain, &t, &mut bb, tick), Some("reload"), "tick {tick}");
    }
    assert_eq!(think(&mut brain, &t, &mut bb, 4), Some("shoot"));
}

#[test]
fn invert_and_succeed_change_results() {
    let t = tree(
        r#"{"root": {"sequence": [
            {"succeed": {"check": "missing"}},
            {"invert": {"check": "alert"}},
            {"do": "rest"}
        ]}}"#,
    );
    let (mut brain, mut bb) = (Brain::new("t"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("rest"));
    bb.set(ALERT, true);
    assert_eq!(think(&mut brain, &t, &mut bb, 1), None);
}

#[test]
fn tree_defaults_fill_only_missing_entries() {
    let t = tree(r#"{"blackboard": {"health": 1.0, "home": [3, 4, 5]}, "root": {"do": "idle"}}"#);
    let mut brain = Brain::new("t");
    let mut bb = Blackboard::new().with(HEALTH, 0.5);
    brain.think(&t, &mut bb, 0, DT);
    assert_eq!(bb.num(HEALTH), 0.5);
    assert_eq!(bb.vec3(Key::new("home")), Some(vec3(3.0, 4.0, 5.0)));
}

#[test]
fn bad_trees_name_the_node_and_the_problem() {
    let cases = [
        (r#"{"root": {"sequense": []}}"#, "root: no node kind"),
        (r#"{"rot": {"do": "x"}}"#, "unknown field \"rot\""),
        (
            r#"{"root": {"sequence": [{"do": "a"}, {"wait": -1}]}}"#,
            "root/sequence[1]/wait: takes seconds",
        ),
        (
            r#"{"root": {"selector": []}}"#,
            "root/selector: \"selector\" needs at least one node",
        ),
        (
            r#"{"root": {"utility": [{"do": "a"}]}}"#,
            "root/utility[0]/do: a child of \"utility\" needs a \"score\"",
        ),
        (
            r#"{"root": {"do": "a", "score": 1}}"#,
            "\"score\" only counts on a child of \"utility\"",
        ),
        (r#"{"root": {"check": "a", "below": 1, "above": 2}}"#, "takes one of"),
        (r#"{"root": {"do": "a", "wait": 1}}"#, "has two node kinds"),
        (
            r#"{"root": {"sequence": [{"check": "a", "beneath": 1}]}}"#,
            "root/sequence[0]/check: unknown field \"beneath\"",
        ),
        (
            r#"{"root": {"utility": [{"score": [{"key": "a", "curve": "wiggly"}], "do": "a"}]}}"#,
            "root/utility[0]/do/score[0]/curve",
        ),
        (r#"{"root": {"set": "a"}}"#, "\"set\" needs a \"to\" value"),
        (r#"{"root": "patrol"}"#, "root: a node must be an object"),
        ("not json", "expected"),
    ];
    for (json, expected) in cases {
        let error = BehaviorTree::from_json(json).expect_err(json);
        assert_eq!(error.kind(), ErrorKind::Json, "{json}");
        assert!(error.to_string().contains(expected), "{json}: {error}");
    }
    let wide: Vec<String> = (0..65).map(|i| format!(r#"{{"score": 1, "do": "a{i}"}}"#)).collect();
    let error = BehaviorTree::from_json(&format!(r#"{{"root": {{"utility": [{}]}}}}"#, wide.join(","))).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Limit);
}

// ---- in a game ---------------------------------------------------------------

const GUARD: &str = r#"{"root": {"selector": [
    {"sequence": [{"check": "alert"}, {"do": "chase"}]},
    {"do": "patrol"}
]}}"#;

fn load(ctx: &mut Ctx, name: &str, json: &str) {
    ctx.assets.add_source(MemorySource::new(name).with(name, json));
    ctx.assets.load_behavior_tree(&mut ctx.world, name);
}

#[test]
fn think_runs_every_brain_in_the_world_on_its_blackboard() {
    let mut world = Game::new("t", 1, 1).sim_system(ai::think).run_headless(1, |ctx| {
        load(ctx, "ai/guard.json", GUARD);
        ctx.world
            .spawn((Brain::new("ai/guard.json"), Blackboard::new().with(ALERT, true)));
        ctx.world.spawn((Brain::new("ai/guard.json"), Blackboard::new()));
        // No blackboard: it gets one.
        ctx.world.spawn((Brain::new("ai/guard.json"),));
        // A tree that isn't loaded: nothing happens.
        ctx.world.spawn((Brain::new("ai/other.json"), Blackboard::new()));
    });
    let mut actions: Vec<Option<&str>> = world
        .query::<&Brain>()
        .iter()
        .map(|b| b.action().map(Key::name))
        .collect();
    actions.sort();
    assert_eq!(actions, [None, Some("chase"), Some("patrol"), Some("patrol")]);
    assert_eq!(world.query::<&Blackboard>().count(), 4);
    assert!(world.query::<&Brain>().iter().any(|b| !b.has_tree()));
}

#[test]
fn think_does_nothing_without_trees() {
    let mut world = Game::new("t", 1, 1).sim_system(ai::think).run_headless(3, |ctx| {
        ctx.world.spawn((Brain::new("ai/guard.json"),));
    });
    let brain = world.query::<&Brain>().single().unwrap();
    assert_eq!(brain.thinks(), 0);
    assert_eq!(world.query::<&Blackboard>().count(), 0);
}

/// The ticks at which each of `n` brains of period `period` thought.
fn think_ticks(period: u32) -> Vec<Vec<u64>> {
    #[derive(Default)]
    struct Log(Vec<Vec<u64>>, Vec<u32>);
    let mut world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 6 {
                ctx.world.query::<&mut Brain>().for_each(|b| b.wake());
            }
        })
        .sim_system(ai::think)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let thinks: Vec<u32> = ctx.world.query::<&Brain>().iter().map(Brain::thinks).collect();
            let log = ctx.world.resource_mut::<Log>();
            log.0.resize(thinks.len(), Vec::new());
            log.1.resize(thinks.len(), 0);
            for (i, n) in thinks.into_iter().enumerate() {
                if n != log.1[i] {
                    log.0[i].push(tick);
                    log.1[i] = n;
                }
            }
        })
        .run_headless(12, move |ctx| {
            ctx.world.insert_resource(Log::default());
            load(ctx, "ai/guard.json", GUARD);
            for _ in 0..3 {
                ctx.world
                    .spawn((Brain::new("ai/guard.json").with_period(period), Blackboard::new()));
            }
        });
    world.remove_resource::<Log>().unwrap().0
}

#[test]
fn brains_with_a_period_think_on_staggered_ticks_and_when_woken() {
    let every = think_ticks(1);
    assert!(every.iter().all(|ticks| ticks.len() == 12), "{every:?}");
    let ticks = think_ticks(4);
    // The first think binds the tree, at once; then every 4th tick, each
    // brain on its own tick, and all at tick 6 (woken).
    let firsts: Vec<u64> = ticks.iter().map(|t| t[0]).collect();
    assert_eq!(firsts, [0, 0, 0]);
    let mut phases: Vec<u64> = ticks.iter().map(|t| t[1] % 4).collect();
    phases.sort();
    phases.dedup();
    assert_eq!(phases.len(), 3, "spread over different ticks: {ticks:?}");
    for t in &ticks {
        assert!(t.contains(&6), "woken: {ticks:?}");
        assert!(t[1..].iter().all(|&k| k == 6 || (k - t[1]) % 4 == 0), "{ticks:?}");
        assert!((t[1]..12).step_by(4).all(|k| t.contains(&k)), "{ticks:?}");
    }
}

#[test]
fn a_finished_action_wakes_a_slow_brain_at_the_next_tick() {
    let mut world = Game::new("t", 1, 1)
        .sim_system(ai::think)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                ctx.world.query::<&mut Brain>().for_each(Brain::succeed);
            }
        })
        .run_headless(4, move |ctx| {
            let tree = BehaviorTree::from_json(r#"{"root": {"sequence": [{"do": "a"}, {"do": "b"}]}}"#).unwrap();
            ai::add_tree(&mut ctx.world, "t", tree);
            ctx.world.spawn((Brain::new("t").with_period(1000), Blackboard::new()));
        });
    let brain = world.query::<&Brain>().single().unwrap();
    assert_eq!(brain.action(), Some(Key::new("b")));
    assert_eq!(brain.thinks(), 2, "bound at tick 0, woken at tick 3");
}

/// Runs 2,000 agents for 120 ticks with blackboards that change with the
/// tick and the entity; returns each agent's action and thinks.
fn crowd_run() -> Vec<(Option<&'static str>, u32)> {
    const TREE: &str = r#"{"root": {"selector": [
        {"sequence": [{"check": "health", "below": 0.25}, {"do": "flee"}]},
        {"utility": [
            {"score": [{"key": "hunger", "from": 0, "to": 100, "curve": "smooth"}], "do": "eat"},
            {"score": [{"key": "danger", "from": 0, "to": 1}, {"key": "health", "from": 0, "to": 1}], "do": "fight"},
            {"score": 0.2, "sequence": [{"do": "wander"}, {"wait": 0.25}]}
        ], "stick": 0.05}
    ]}}"#;
    let mut world = Game::new("crowd", 1, 1)
        .seed(7)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Blackboard)>().for_each(|(e, bb)| {
                let x = (e.index() as u64 * 2_654_435_761 + tick * 40_503) % 1000;
                bb.set(HUNGER, (x % 100) as f32);
                bb.set(Key::new("danger"), (x % 7) as f32 / 7.0);
                bb.set(HEALTH, (x % 13) as f32 / 12.0);
            });
        })
        .sim_system(ai::think)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            ctx.world.query::<(Entity, &mut Brain)>().for_each(|(e, b)| {
                if (e.index() as u64 + tick).is_multiple_of(5) {
                    b.succeed();
                }
            });
        })
        .run_headless(120, |ctx| {
            ai::add_tree(&mut ctx.world, "crowd", BehaviorTree::from_json(TREE).unwrap());
            for i in 0..2000u32 {
                ctx.world
                    .spawn((Brain::new("crowd").with_period(1 + i % 4), Blackboard::new()));
            }
        });
    world
        .query::<&Brain>()
        .iter()
        .map(|b| (b.action().map(Key::name), b.thinks()))
        .collect()
}

#[test]
fn the_same_blackboards_give_the_same_choices_on_every_run() {
    let first = crowd_run();
    assert_eq!(first.len(), 2000);
    let kinds: std::collections::HashSet<_> = first.iter().map(|(a, _)| *a).collect();
    assert!(kinds.len() >= 3, "a varied crowd: {kinds:?}");
    for _ in 0..2 {
        assert_eq!(crowd_run(), first, "thinking on all cores is deterministic");
    }
}

#[test]
fn blackboard_and_brain_read_and_write_as_json() {
    let mut world = Game::new("t", 1, 1).sim_system(ai::think).run_headless(1, |ctx| {
        load(ctx, "ai/guard.json", GUARD);
        let target = ctx.world.spawn((Transform::at(Vec2::ZERO),));
        ctx.world.spawn((
            Brain::new("ai/guard.json").with_period(2),
            Blackboard::new()
                .with(ALERT, false)
                .with(HEALTH, 0.5)
                .with(Key::new("home"), vec2(1.0, 2.0))
                .with(Key::new("target"), target),
        ));
    });
    let e = world.query::<Entity>().filter::<With<Brain>>().single().unwrap();
    let target = world.query::<Entity>().filter::<Without<Brain>>().single().unwrap();
    let bb = world.get_json(e, "Blackboard").unwrap();
    assert_eq!(
        bb,
        serde_json::json!({"alert": false, "health": 0.5, "home": [1.0, 2.0], "target": {"entity": target.to_bits()}})
    );
    assert_eq!(
        world.get_json(e, "Brain").unwrap(),
        serde_json::json!({"tree": "ai/guard.json", "period": 2, "action": "patrol", "status": "running"})
    );
    world
        .set_json(e, "Blackboard", serde_json::json!({"alert": true, "health": 0.25}))
        .unwrap();
    let bb = world.get::<Blackboard>(e).unwrap();
    assert!(bb.flag(ALERT));
    assert_eq!(bb.num(HEALTH), 0.25);
    world
        .set_json(e, "Brain", serde_json::json!({"tree": "ai/guard.json", "period": 8}))
        .unwrap();
    assert_eq!(world.get::<Brain>(e).unwrap().period(), 8);
    assert!(world.set_json(e, "Blackboard", serde_json::json!({"x": [1]})).is_err());
}

#[test]
fn editing_a_tree_file_swaps_it_in_the_running_game() {
    let dir = std::env::temp_dir().join(format!("ponko-ai-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("guard.json");
    std::fs::write(&file, r#"{"root": {"do": "patrol"}}"#).unwrap();
    let path = file.to_str().unwrap().to_owned();
    let (edit, name) = (file.clone(), path.clone());
    #[derive(Default)]
    struct Seen(Vec<Option<&'static str>>);
    let mut world = Game::new("hot", 1, 1)
        .hot_reload(true)
        .sim_system(ai::think)
        .sim_system(move |ctx: &mut Ctx| {
            let action = ctx
                .world
                .query::<&Brain>()
                .iter()
                .next()
                .and_then(|b| b.action().map(Key::name));
            ctx.world.resource_mut::<Seen>().0.push(action);
            match ctx.time.sim_tick() {
                1 => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::fs::write(&edit, r#"{"root": {"do": "dance"}}"#).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
                3 => {
                    // A broken file keeps the tree that works.
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    std::fs::write(&edit, r#"{"root": {"dance": 1}}"#).unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
                _ => {}
            }
        })
        .run_headless(6, move |ctx| {
            ctx.world.insert_resource(Seen::default());
            ctx.assets.load_behavior_tree(&mut ctx.world, &name);
            ctx.world.spawn((Brain::new(&name), Blackboard::new()));
        });
    let seen = world.remove_resource::<Seen>().unwrap().0;
    assert_eq!(seen[0], Some("patrol"));
    assert_eq!(seen.last().copied().flatten(), Some("dance"), "{seen:?}");
    assert!(world.resource::<BehaviorTrees>().get(&path).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_tree_file_is_an_error_and_adds_nothing() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let error = ctx
            .assets
            .try_load_behavior_tree(&mut ctx.world, "ai/none.json")
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(ctx.world.get_resource::<BehaviorTrees>().is_none());
    });
}
