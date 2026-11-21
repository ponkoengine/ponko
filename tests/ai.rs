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
    let mut brain = world.get_json(e, "Brain").unwrap();
    // Its memory, for saves: the running nodes, by index.
    let memory = brain.as_object_mut().unwrap().remove("memory").unwrap();
    assert_eq!(
        brain,
        serde_json::json!({"tree": "ai/guard.json", "period": 2, "action": "patrol", "status": "running"})
    );
    assert_eq!(memory["running"], 4);
    assert_eq!(memory["thinks"], 1);
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

// ---- subtrees ---------------------------------------------------------------

#[test]
fn a_subtree_runs_another_tree_with_its_blackboard_defaults() {
    let mut trees = BehaviorTrees::default();
    trees.insert(
        "patrol",
        BehaviorTree::from_json(r#"{"blackboard": {"legs": 4, "alert": true}, "root": {"do": "walk"}}"#).unwrap(),
    );
    trees.insert(
        "guard",
        BehaviorTree::from_json(
            r#"{"blackboard": {"alert": false}, "root": {"selector": [
                {"sequence": [{"check": "alert"}, {"do": "chase"}]},
                {"subtree": "patrol"}
            ]}}"#,
        )
        .unwrap(),
    );
    let guard = trees.get("guard").unwrap().clone();
    assert_eq!(guard.subtrees(), ["patrol"]);
    let (mut brain, mut bb) = (Brain::new("guard"), Blackboard::new());
    assert_eq!(think(&mut brain, &guard, &mut bb, 0), Some("walk"));
    // The subtree's defaults fill in, the outer tree's win.
    assert_eq!(bb.num(Key::new("legs")), 4.0);
    assert!(!bb.flag(ALERT));
    let labels: Vec<_> = brain.trace().iter().map(|l| (l.depth, l.label)).collect();
    assert!(
        labels.contains(&(1, "subtree patrol")) && labels.contains(&(2, "do walk")),
        "{labels:?}"
    );
}

#[test]
fn a_subtree_added_or_replaced_later_reaches_every_tree_that_runs_it() {
    let mut trees = BehaviorTrees::default();
    let t = |json: &str| BehaviorTree::from_json(json).unwrap();
    trees.insert(
        "guard",
        t(r#"{"root": {"selector": [{"subtree": "fight"}, {"do": "idle"}]}}"#),
    );
    trees.insert("boss", t(r#"{"root": {"subtree": "guard"}}"#));
    let (mut brain, mut bb) = (Brain::new("boss"), Blackboard::new());
    // Not loaded yet: the subtree node fails, and the selector goes on.
    let boss = trees.get("boss").unwrap().clone();
    assert_eq!(think(&mut brain, &boss, &mut bb, 0), Some("idle"));
    assert!(brain.trace().iter().any(|l| l.label == "subtree fight (not loaded)"));
    trees.insert("fight", t(r#"{"root": {"do": "punch"}}"#));
    let boss = trees.get("boss").unwrap().clone();
    assert_eq!(boss.subtrees(), ["guard", "fight"]);
    assert_eq!(think(&mut brain, &boss, &mut bb, 1), Some("punch"));
    trees.insert("fight", t(r#"{"root": {"do": "kick"}}"#));
    let boss = trees.get("boss").unwrap().clone();
    assert_eq!(think(&mut brain, &boss, &mut bb, 2), Some("kick"));
}

#[test]
fn a_subtree_cycle_is_an_error_with_both_names() {
    let mut trees = BehaviorTrees::default();
    let t = |json: &str| BehaviorTree::from_json(json).unwrap();
    trees.insert("ai/a.json", t(r#"{"root": {"subtree": "ai/b.json"}}"#));
    let error = trees
        .try_insert(
            "ai/b.json",
            t(r#"{"root": {"sequence": [{"do": "x"}, {"subtree": "ai/a.json"}]}}"#),
        )
        .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Invalid);
    let message = error.to_string();
    assert!(
        message.contains("ai/a.json") && message.contains("ai/b.json"),
        "{message}"
    );
    assert!(
        message.contains("subtree cycle: ai/b.json -> ai/a.json -> ai/b.json"),
        "{message}"
    );
    // Nothing changed.
    assert!(trees.get("ai/b.json").is_none());
    let error = trees
        .try_insert("ai/c.json", t(r#"{"root": {"subtree": "ai/c.json"}}"#))
        .unwrap_err();
    assert!(error.to_string().contains("ai/c.json -> ai/c.json"), "{error}");
}

#[test]
#[should_panic(expected = "subtree cycle: me -> me")]
fn inserting_a_tree_that_runs_itself_panics_with_its_name() {
    let mut trees = BehaviorTrees::default();
    trees.insert("me", BehaviorTree::from_json(r#"{"root": {"subtree": "me"}}"#).unwrap());
}

#[test]
fn loading_a_tree_file_loads_the_files_of_its_subtrees() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("m")
                .with(
                    "ai/guard.json",
                    r#"{"root": {"selector": [{"subtree": "ai/fight.json"}, {"do": "idle"}]}}"#,
                )
                .with(
                    "ai/fight.json",
                    r#"{"root": {"sequence": [{"check": "alert"}, {"subtree": "ai/punch.json"}]}}"#,
                )
                .with("ai/punch.json", r#"{"root": {"do": "punch"}}"#),
        );
        ctx.assets.load_behavior_tree(&mut ctx.world, "ai/guard.json");
        let trees = ctx.world.resource::<BehaviorTrees>();
        assert_eq!(trees.names(), ["ai/fight.json", "ai/guard.json", "ai/punch.json"]);
        let guard = trees.get("ai/guard.json").unwrap().clone();
        let (mut brain, mut bb) = (Brain::new("ai/guard.json"), Blackboard::new().with(ALERT, true));
        assert_eq!(think(&mut brain, &guard, &mut bb, 0), Some("punch"));
    });
}

#[test]
fn a_subtree_cycle_between_files_is_an_error_with_both_names_and_adds_nothing() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(
            MemorySource::new("m")
                .with("ai/a.json", r#"{"root": {"subtree": "ai/b.json"}}"#)
                .with("ai/b.json", r#"{"root": {"subtree": "ai/a.json"}}"#)
                .with("ai/lost.json", r#"{"root": {"subtree": "ai/none.json"}}"#),
        );
        let error = ctx
            .assets
            .try_load_behavior_tree(&mut ctx.world, "ai/a.json")
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("ai/a.json -> ai/b.json -> ai/a.json"), "{message}");
        assert!(ctx.world.get_resource::<BehaviorTrees>().is_none());
        // A subtree file that doesn't exist is an error too.
        let error = ctx
            .assets
            .try_load_behavior_tree(&mut ctx.world, "ai/lost.json")
            .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::NotFound, "{error}");
    });
}

#[test]
fn editing_a_subtree_file_reaches_the_trees_that_run_it() {
    let dir = std::env::temp_dir().join(format!("ponko-ai-subtree-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (guard, fight) = (dir.join("guard.json"), dir.join("fight.json"));
    let fight_name = fight.to_str().unwrap().to_owned();
    std::fs::write(&fight, r#"{"root": {"do": "punch"}}"#).unwrap();
    std::fs::write(&guard, format!(r#"{{"root": {{"subtree": {fight_name:?}}}}}"#)).unwrap();
    let name = guard.to_str().unwrap().to_owned();
    let edit = fight.clone();
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
            if ctx.time.sim_tick() == 1 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                std::fs::write(&edit, r#"{"root": {"do": "kick"}}"#).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(400));
            }
        })
        .run_headless(4, move |ctx| {
            ctx.world.insert_resource(Seen::default());
            ctx.assets.load_behavior_tree(&mut ctx.world, &name);
            ctx.world.spawn((Brain::new(&name), Blackboard::new()));
        });
    let seen = world.remove_resource::<Seen>().unwrap().0;
    assert_eq!(seen[0], Some("punch"));
    assert_eq!(seen.last().copied().flatten(), Some("kick"), "{seen:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- parallel ---------------------------------------------------------------

#[test]
fn a_parallel_runs_an_action_while_its_other_children_watch() {
    let t = tree(
        r#"{"root": {"sequence": [
            {"parallel": [{"do": "walk"}, {"wait": 0.05}], "success": "any"},
            {"do": "rest"}
        ]}}"#,
    );
    let (mut brain, mut bb) = (Brain::new("w"), Blackboard::new());
    // 0.05 s at 60 ticks a second: three ticks.
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("walk"));
    assert_eq!(think(&mut brain, &t, &mut bb, 1), Some("walk"));
    assert!(!brain.started(), "the walk carries on");
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("walk"));
    // The wait succeeded: the walk is stopped, the sequence goes on.
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("rest"));
    let lines: Vec<_> = brain.trace().iter().map(|l| (l.label, l.status)).collect();
    assert!(
        lines.contains(&("parallel (success any, failure any)", Some(Status::Success))),
        "{lines:?}"
    );
}

#[test]
fn a_parallel_stops_its_running_action_when_its_policy_decides() {
    let t = tree(r#"{"root": {"parallel": [{"do": "aim"}, {"check": "alert"}], "failure": "any"}}"#);
    let (mut brain, mut bb) = (Brain::new("w"), Blackboard::new().with(ALERT, true));
    assert_eq!(think(&mut brain, &t, &mut bb, 0), Some("aim"));
    bb.set(ALERT, false);
    // The check fails: the parallel fails, and nothing runs.
    assert_eq!(think(&mut brain, &t, &mut bb, 1), None);
    bb.set(ALERT, true);
    // Started over, the action is new again.
    assert_eq!(think(&mut brain, &t, &mut bb, 2), Some("aim"));
    assert!(brain.started());
}

#[test]
fn a_parallel_counts_successes_and_failures_by_its_policy() {
    // Two of three must succeed; it fails once two have failed.
    let json = |a: bool, b: bool, c: bool| {
        format!(
            r#"{{"root": {{"parallel": [{{"check": "a", "is": {a}}}, {{"check": "b", "is": {b}}}, {{"check": "c", "is": {c}}}],
                "success": 2, "failure": 2}}}}"#
        )
    };
    let bb = Blackboard::new()
        .with(Key::new("a"), true)
        .with(Key::new("b"), true)
        .with(Key::new("c"), true);
    let result = |a, b, c| {
        let t = tree(&json(a, b, c));
        let mut brain = Brain::new("p");
        brain.think(&t, &mut bb.clone(), 0, DT);
        brain.trace()[0].status
    };
    assert_eq!(result(true, true, false), Some(Status::Success));
    assert_eq!(result(true, false, false), Some(Status::Failure));
    assert_eq!(result(false, true, true), Some(Status::Success));
    // Finished children keep their result while others run.
    let t = tree(r#"{"root": {"parallel": [{"wait": 0.05}, {"set": "x", "to": 1}], "success": "all"}}"#);
    let (mut brain, mut bb) = (Brain::new("p"), Blackboard::new());
    brain.think(&t, &mut bb, 0, DT);
    assert_eq!(brain.trace()[0].status, Some(Status::Running));
    bb.remove(Key::new("x"));
    brain.think(&t, &mut bb, 3, DT);
    assert_eq!(brain.trace()[0].status, Some(Status::Success));
    assert!(!bb.contains(Key::new("x")), "the finished set didn't run again");
}

#[test]
fn bad_parallel_nodes_name_the_node_and_the_problem() {
    let error = |json: &str| BehaviorTree::from_json(json).unwrap_err().to_string();
    let e = error(r#"{"root": {"parallel": [{"do": "a"}, {"sequence": [{"do": "b"}]}]}}"#);
    assert!(
        e.contains("root/parallel: children [0] and [1] both have \"do\" nodes"),
        "{e}"
    );
    let e = error(r#"{"root": {"parallel": [{"do": "a"}], "success": 2}}"#);
    assert!(e.contains("\"success\" counts children: from 1 to 1"), "{e}");
    let e = error(r#"{"root": {"parallel": [{"do": "a"}], "failure": "some"}}"#);
    assert!(e.contains("\"failure\" is \"all\", \"any\" or a number"), "{e}");
    let e = error(r#"{"root": {"chance": 2}}"#);
    assert!(e.contains("root/chance: takes a probability"), "{e}");
    // A `do` node may still have parameters named like the newer kinds.
    let t = tree(r#"{"root": {"do": "attack", "chance": 0.5, "random": true}}"#);
    let (mut brain, mut bb) = (Brain::new("x"), Blackboard::new());
    brain.think(&t, &mut bb, 0, DT);
    assert_eq!(brain.param_num(Key::new("chance")), Some(0.5));
}

// ---- random and chance ------------------------------------------------------

/// What `n` brains, seeded 0..n, choose with a `random` node.
fn random_choices(seed_base: u64, n: u64) -> Vec<&'static str> {
    let t = tree(r#"{"root": {"random": [{"do": "a"}, {"do": "b"}, {"do": "c"}]}}"#);
    (0..n)
        .map(|i| {
            let mut brain = Brain::new("r").with_seed(seed_base + i);
            think(&mut brain, &t, &mut Blackboard::new(), 0).unwrap()
        })
        .collect()
}

#[test]
fn random_and_chance_draw_from_the_seed() {
    let first = random_choices(0, 60);
    assert_eq!(first, random_choices(0, 60), "same seeds, same choices");
    assert_ne!(first, random_choices(1000, 60), "other seeds, other choices");
    for action in ["a", "b", "c"] {
        assert!(first.contains(&action), "every child gets picked: {first:?}");
    }
    // A running child carries on; it isn't drawn again.
    let t = tree(r#"{"root": {"random": [{"do": "a"}, {"do": "b"}, {"do": "c"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("r"), Blackboard::new());
    let picked = think(&mut brain, &t, &mut bb, 0);
    for tick in 1..20 {
        assert_eq!(think(&mut brain, &t, &mut bb, tick), picked);
    }
    // A failed child: the others are tried in the same think.
    let t = tree(r#"{"root": {"random": [{"check": "nope"}, {"check": "nope"}, {"do": "c"}]}}"#);
    for seed in 0..10 {
        let mut brain = Brain::new("r").with_seed(seed);
        assert_eq!(think(&mut brain, &t, &mut Blackboard::new(), 0), Some("c"));
    }
    let t = tree(r#"{"root": {"selector": [{"sequence": [{"chance": 0.25}, {"do": "taunt"}]}, {"do": "wait"}]}}"#);
    let taunts = (0..400)
        .filter(|&seed| {
            let mut brain = Brain::new("c").with_seed(seed);
            think(&mut brain, &t, &mut Blackboard::new(), 0) == Some("taunt")
        })
        .count();
    assert!((60..140).contains(&taunts), "about a quarter: {taunts} of 400");
}

// ---- signals ----------------------------------------------------------------

#[test]
fn wait_for_ends_when_the_brain_hears_its_signal() {
    let t = tree(
        r#"{"root": {"sequence": [
            {"wait_for": "noise", "store": "heard_at"},
            {"do": "look"}
        ]}}"#,
    );
    assert_eq!(t.signals(), [Key::new("noise")]);
    let (mut brain, mut bb) = (Brain::new("g"), Blackboard::new());
    for tick in 0..5 {
        assert_eq!(think(&mut brain, &t, &mut bb, tick), None);
        assert_eq!(brain.trace()[1].status, Some(Status::Running));
    }
    // Another signal doesn't end it.
    brain.signal(Key::new("other"));
    assert_eq!(think(&mut brain, &t, &mut bb, 5), None);
    brain.signal_with(Key::new("noise"), vec2(3.0, 4.0));
    assert_eq!(think(&mut brain, &t, &mut bb, 6), Some("look"));
    assert_eq!(bb.vec2(Key::new("heard_at")), Some(vec2(3.0, 4.0)));
    // Heard once: forgotten after that think.
    brain.succeed();
    assert_eq!(think(&mut brain, &t, &mut bb, 7), None);
}

#[test]
fn wait_for_with_a_timeout_fails_when_nothing_comes() {
    let t = tree(
        r#"{"root": {"selector": [
            {"sequence": [{"wait_for": "call", "timeout": 0.05}, {"do": "answer"}]},
            {"do": "give_up"}
        ]}}"#,
    );
    let (mut brain, mut bb) = (Brain::new("g"), Blackboard::new());
    assert_eq!(think(&mut brain, &t, &mut bb, 0), None);
    assert_eq!(think(&mut brain, &t, &mut bb, 2), None);
    assert_eq!(think(&mut brain, &t, &mut bb, 3), Some("give_up"));
}

/// Brains of period 1,000 waiting for `alarm`; returns each one's ticks of
/// thought and whether it rang its bell.
fn signal_run(send: fn(&mut Ctx, &[Entity])) -> Vec<(u32, bool)> {
    #[derive(Default)]
    struct Guards(Vec<Entity>);
    let world = Game::new("t", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 5 {
                let guards = ctx.world.resource::<Guards>().0.clone();
                send(ctx, &guards);
            }
        })
        .sim_system(ai::think)
        .run_headless(10, |ctx| {
            load(
                ctx,
                "ai/bell.json",
                r#"{"root": {"selector": [
                    {"sequence": [{"wait_for": "alarm"}, {"set": "rang", "to": true}]},
                    {"do": "sleep"}
                ]}}"#,
            );
            let guards = (0..3)
                .map(|_| {
                    ctx.world
                        .spawn((Brain::new("ai/bell.json").with_period(1000), Blackboard::new()))
                })
                .collect();
            ctx.world.insert_resource(Guards(guards));
        });
    let guards = world.resource::<Guards>().0.clone();
    guards
        .iter()
        .map(|&e| {
            let thinks = world.get::<Brain>(e).unwrap().thinks();
            (thinks, world.get::<Blackboard>(e).unwrap().flag(Key::new("rang")))
        })
        .collect()
}

#[test]
fn a_signal_wakes_its_brain_at_once_and_reaches_only_its_target() {
    // Bound at tick 0, then asleep for 1,000 ticks: only the signal wakes it.
    let to_one = signal_run(|ctx, guards| ctx.events.send(ai::Signal::to(guards[1], Key::new("alarm"))));
    assert_eq!(to_one, [(1, false), (2, true), (1, false)]);
    let to_all = signal_run(|ctx, _| ctx.events.send(ai::Signal::broadcast(Key::new("alarm"))));
    assert_eq!(to_all, [(2, true), (2, true), (2, true)]);
    // A signal no tree waits for wakes no one.
    let other = signal_run(|ctx, _| ctx.events.send(ai::Signal::broadcast(Key::new("lunch"))));
    assert_eq!(other, [(1, false), (1, false), (1, false)]);
}

#[test]
fn forward_turns_engine_events_into_signals() {
    struct Hit {
        target: Entity,
        by: Entity,
    }
    let mut world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                let (target, by) = {
                    let mut q = ctx.world.query::<Entity>().filter::<With<Brain>>();
                    let target = q.iter().next().unwrap();
                    (target, target)
                };
                ctx.events.send(Hit { target, by });
            }
        })
        .sim_system(ai::forward(|hit: &Hit| {
            Some(ai::Signal::to(hit.target, Key::new("hit")).with(hit.by))
        }))
        .sim_system(ai::think)
        .run_headless(4, |ctx| {
            load(
                ctx,
                "ai/hurt.json",
                r#"{"root": {"sequence": [{"wait_for": "hit", "store": "attacker"}, {"do": "fight_back"}]}}"#,
            );
            ctx.world
                .spawn((Brain::new("ai/hurt.json").with_period(100), Blackboard::new()));
        });
    let (e, brain) = world
        .query::<(Entity, &Brain)>()
        .single()
        .map(|(e, b)| (e, b.clone()))
        .unwrap();
    assert!(brain.doing(Key::new("fight_back")));
    assert_eq!(
        world.get::<Blackboard>(e).unwrap().entity(Key::new("attacker")),
        Some(e)
    );
}

// ---- saves ------------------------------------------------------------------

/// A tree with something to remember in every kind of node: a sequence
/// part done, a wait part waited, a cooldown, a parallel with a finished
/// child, a random choice, a wait for a signal.
const MEMORY_TREE: &str = r#"{"root": {"selector": [
    {"cooldown": 0.2, "child": {"sequence": [{"check": "rang"}, {"set": "rang", "to": false}, {"do": "celebrate"}]}},
    {"sequence": [
        {"do": "wake_up"},
        {"wait": 0.1},
        {"parallel": [
            {"random": [{"do": "dig"}, {"do": "sweep"}, {"do": "sing"}]},
            {"sequence": [{"wait_for": "bell"}, {"set": "rang", "to": true}]}
        ], "success": "any"},
        {"chance": 0.5},
        {"do": "rest"}
    ]}
]}}"#;

type Log = Vec<Vec<(Option<&'static str>, Status)>>;

/// Runs `brains` brains (or the world in `save`) for `ticks` ticks; logs
/// every brain's action each tick, and saves the world after tick `save_at`.
fn memory_run(load_from: Option<&serde_json::Value>, ticks: u64, save_at: u64) -> (Log, Option<serde_json::Value>) {
    #[derive(Default)]
    struct Run(Log, Option<serde_json::Value>, u64);
    let load_from = load_from.cloned();
    let mut world = Game::new("t", 1, 1)
        .seed(11)
        .sim_system(|ctx: &mut Ctx| {
            // The bell rings for every brain whose blackboard counts 25 ticks
            // of work (state that is saved, unlike the sim tick).
            let mut ring = Vec::new();
            ctx.world.query::<(Entity, &mut Blackboard)>().for_each(|(e, bb)| {
                let n = bb.num(Key::new("work")) + 1.0;
                bb.set(Key::new("work"), n);
                if n as u32 % 25 == 0 {
                    ring.push(e);
                }
            });
            for e in ring {
                ctx.events.send(ai::Signal::to(e, Key::new("bell")));
            }
        })
        .sim_system(ai::think)
        .sim_system(|ctx: &mut Ctx| {
            // Actions take three ticks of their own counting.
            ctx.world
                .query::<(&mut Brain, &mut Blackboard)>()
                .for_each(|(brain, bb)| {
                    if brain.action().is_some() {
                        let steps = bb.num(Key::new("steps")) + 1.0;
                        bb.set(Key::new("steps"), steps);
                        if steps as u32 % 3 == 0 {
                            brain.succeed();
                        }
                    }
                });
            let row = ctx
                .world
                .query::<&Brain>()
                .iter()
                .map(|b| (b.action().map(Key::name), b.status()))
                .collect();
            let save = (ctx.time.sim_tick() == ctx.world.resource::<Run>().2).then(|| ctx.world.save());
            let run = ctx.world.resource_mut::<Run>();
            run.0.push(row);
            if save.is_some() {
                run.1 = save;
            }
        })
        .run_headless(ticks, move |ctx| {
            ctx.world.insert_resource(Run(Vec::new(), None, save_at));
            load(ctx, "ai/memory.json", MEMORY_TREE);
            match &load_from {
                Some(save) => ctx.world.load(save).unwrap(),
                None => {
                    for i in 0..12u32 {
                        ctx.world.spawn((
                            Brain::new("ai/memory.json").with_period(1 + i % 3),
                            Blackboard::new().with(Key::new("work"), i as f32),
                        ));
                    }
                }
            }
        });
    let run = world.remove_resource::<Run>().unwrap();
    (run.0, run.1)
}

#[test]
fn a_saved_brain_carries_on_at_the_same_running_node() {
    let (whole, save) = memory_run(None, 120, 59);
    let save = save.unwrap();
    // Some brains were in the middle of something when saved.
    let saved_brains: Vec<_> = save["entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["components"]["Brain"].get("memory"))
        .collect();
    assert_eq!(saved_brains.len(), 12);
    assert!(
        whole[59].iter().filter(|(a, _)| a.is_some()).count() >= 3,
        "{:?}",
        whole[59]
    );
    let (rest, _) = memory_run(Some(&save), 60, u64::MAX);
    for (tick, row) in rest.iter().enumerate() {
        assert_eq!(row, &whole[60 + tick], "tick {} after the save", tick);
    }
    let kinds: std::collections::HashSet<_> = whole.iter().flatten().filter_map(|(a, _)| *a).collect();
    assert!(kinds.len() >= 5, "the run visits most of the tree: {kinds:?}");
}

#[test]
fn a_saved_brain_starts_over_when_its_tree_changed() {
    let t = tree(r#"{"root": {"sequence": [{"do": "a"}, {"do": "b"}]}}"#);
    let (mut brain, mut bb) = (Brain::new("t"), Blackboard::new());
    think(&mut brain, &t, &mut bb, 0);
    brain.succeed();
    think(&mut brain, &t, &mut bb, 1);
    assert_eq!(brain.action(), Some(Key::new("b")));
    let json = serde_json::to_value(&brain).unwrap();
    assert_eq!(json["memory"]["running"], 2);
    // Same tree: carries on with b, not started again.
    let mut same: Brain = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(think(&mut same, &t, &mut bb, 0), Some("b"));
    assert!(!same.started());
    // Another tree: starts over.
    let other = tree(r#"{"root": {"sequence": [{"do": "a"}, {"do": "c"}]}}"#);
    let mut changed: Brain = serde_json::from_value(json).unwrap();
    assert_eq!(think(&mut changed, &other, &mut bb, 0), Some("a"));
    assert!(changed.started());
}

// ---- determinism ------------------------------------------------------------

/// 3,000 brains with random, chance, parallel and wait_for nodes, senses
/// from the entity and the tick and signals to some; each tick's choices.
fn decisions(seed: u64) -> Vec<u64> {
    const TREE: &str = r#"{"root": {"selector": [
        {"sequence": [{"wait_for": "shout", "timeout": 0}, {"do": "turn"}]},
        {"parallel": [
            {"random": [{"do": "dig"}, {"sequence": [{"chance": 0.3}, {"do": "sing"}]}, {"do": "walk"}]},
            {"check": "hunger", "below": 70}
        ]},
        {"utility": [
            {"score": [{"key": "hunger", "from": 0, "to": 100}], "do": "eat"},
            {"score": 0.3, "do": "nap"}
        ]}
    ]}}"#;
    #[derive(Default)]
    struct Hashes(Vec<u64>);
    let mut world = Game::new("crowd", 1, 1)
        .seed(seed)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let mut shout = Vec::new();
            ctx.world.query::<(Entity, &mut Blackboard)>().for_each(|(e, bb)| {
                let x = (e.index() as u64 * 2_654_435_761 + tick * 40_503) % 1000;
                bb.set(HUNGER, (x % 100) as f32);
                if x % 97 == 0 {
                    shout.push(e);
                }
            });
            for e in shout {
                ctx.events.send(ai::Signal::to(e, Key::new("shout")));
            }
        })
        .sim_system(ai::think)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            ctx.world.query::<(Entity, &mut Brain)>().for_each(|(e, b)| {
                for byte in b.action().map_or("", Key::name).bytes() {
                    h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
                h = (h ^ 0xff).wrapping_mul(0x0100_0000_01b3);
                if (e.index() as u64 + tick).is_multiple_of(4) {
                    b.succeed();
                }
            });
            ctx.world.resource_mut::<Hashes>().0.push(h);
        })
        .run_headless(90, |ctx| {
            ctx.world.insert_resource(Hashes::default());
            ai::add_tree(&mut ctx.world, "crowd", BehaviorTree::from_json(TREE).unwrap());
            for i in 0..3000u32 {
                ctx.world
                    .spawn((Brain::new("crowd").with_period(1 + i % 3), Blackboard::new()));
            }
        });
    world.remove_resource::<Hashes>().unwrap().0
}

#[test]
fn the_same_seed_and_inputs_give_the_same_decisions() {
    let first = decisions(3);
    assert_eq!(first.len(), 90);
    for _ in 0..2 {
        assert_eq!(decisions(3), first, "same seed and inputs, on all cores");
    }
    let other = decisions(4);
    assert_ne!(other, first, "another seed draws other random choices");
}
