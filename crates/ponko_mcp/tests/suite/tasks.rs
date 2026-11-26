//! The tasks: each has a starting project (files in `tasks/<id>/files`), a
//! goal for the agent (`goal.md`), a checker, a scripted solution
//! (`solution.json`) and negative controls (`controls.json`).

use super::games::{self, START};
use super::{Check, Probe};
use ponko::prelude::*;
use serde_json::{Value, json};

/// A task's game, and the setup it runs before the first tick.
pub type GameAndSetup = (Game, fn(&mut Ctx));

/// A task's start hook (see [`Task::start`]).
pub type StartHook = fn(&mut Probe) -> Result<(), String>;

/// How a task's game is run.
pub enum Run {
    /// A game of this suite: a thread of the process in replays, a child
    /// process (`agent_suite host`) for a real agent.
    Game(fn() -> GameAndSetup),
    /// A Rust crate in the task's folder (`Cargo.toml`, `src/`), built with
    /// cargo and started as the binary `bin`. Code tasks: the agent changes
    /// the code, and the checker builds and starts the game from it.
    Crate {
        /// The binary's name.
        bin: &'static str,
    },
}

/// One task of the suite.
pub struct Task {
    /// Short name, used on the command line and in the baseline.
    pub id: &'static str,
    /// One line about it.
    pub title: &'static str,
    /// What kind of work it is: tuning, content, bug, performance, shader,
    /// ui, measure, experiment, code.
    pub kind: &'static str,
    /// What the agent is told.
    pub goal: &'static str,
    /// The starting files, relative to the task's folder.
    pub files: &'static [(&'static str, &'static str)],
    /// The game.
    pub game: Run,
    /// The tools a good run leans on: without them the task gets much
    /// harder (an agent would have to guess, or rebuild what they show).
    pub leans_on: &'static [&'static str],
    /// Runs once, right after the game starts and before the agent does
    /// anything (with the checker's tools): a task that needs the starting
    /// state again saves it here.
    pub start: Option<StartHook>,
    /// The checker's checks, in order.
    pub checks: &'static [&'static str],
    /// Scores the result.
    pub check: fn(&mut Probe) -> Vec<Check>,
    /// The scripted solution (JSON).
    pub solution: &'static str,
    /// The negative controls (JSON).
    pub controls: &'static str,
}

macro_rules! task_files {
    ($id:literal: $($path:literal),*) => {
        &[$(($path, include_str!(concat!("tasks/", $id, "/files/", $path)))),*]
    };
}

/// Every task, in the order they are scored.
pub static TASKS: &[Task] = &[
    Task {
        id: "tune-speed",
        title: "Tune the player's speed in a data file so a run takes the right time",
        kind: "tuning",
        goal: include_str!("tasks/tune-speed/goal.md"),
        files: task_files!("tune-speed": "assets/settings.json"),
        game: Run::Game(games::arena_game),
        leans_on: &["get_resource", "run_input_scenario"],
        start: None,
        checks: &[
            "settings file is valid",
            "game uses the file's speed",
            "flag reached in time",
        ],
        check: check_tune_speed,
        solution: include_str!("tasks/tune-speed/solution.json"),
        controls: include_str!("tasks/tune-speed/controls.json"),
    },
    Task {
        id: "coin-level",
        title: "Build a level with the tools and save it as a scene",
        kind: "content",
        goal: include_str!("tasks/coin-level/goal.md"),
        files: task_files!("coin-level": "assets/settings.json"),
        game: Run::Game(games::arena_game),
        leans_on: &["spawn_entity", "save_scene", "run_input_scenario"],
        start: None,
        checks: &[
            "level file saved",
            "level loads",
            "player and five coins",
            "holding right collects them all",
        ],
        check: check_coin_level,
        solution: include_str!("tasks/coin-level/solution.json"),
        controls: include_str!("tasks/coin-level/controls.json"),
    },
    Task {
        id: "immortal-enemy",
        title: "Reproduce and fix a seeded bug: an enemy that never dies",
        kind: "bug",
        goal: include_str!("tasks/immortal-enemy/goal.md"),
        files: task_files!("immortal-enemy": "assets/enemies.json"),
        game: Run::Game(games::enemies_game),
        leans_on: &["list_entities", "subscribe_events", "run_ticks"],
        start: None,
        checks: &[
            "roster file is fixed",
            "no live enemy heals",
            "a fresh wave dies in 5 seconds",
        ],
        check: check_immortal_enemy,
        solution: include_str!("tasks/immortal-enemy/solution.json"),
        controls: include_str!("tasks/immortal-enemy/controls.json"),
    },
    Task {
        id: "slow-crowd",
        title: "Find and cut a tick-time regression without changing the result",
        kind: "performance",
        goal: include_str!("tasks/slow-crowd/goal.md"),
        files: task_files!("slow-crowd": "assets/tuning.json"),
        game: Run::Game(games::crowd_game),
        leans_on: &["system_timings", "get_resource"],
        start: None,
        checks: &[
            "radius unchanged",
            "game uses the file",
            "work cut 10x",
            "same pairs found",
        ],
        check: check_slow_crowd,
        solution: include_str!("tasks/slow-crowd/solution.json"),
        controls: include_str!("tasks/slow-crowd/controls.json"),
    },
    Task {
        id: "broken-shader",
        title: "Fix a sprite shader that doesn't compile, while the game runs",
        kind: "shader",
        goal: include_str!("tasks/broken-shader/goal.md"),
        files: task_files!("broken-shader": "assets/settings.json", "assets/shaders/glow.wgsl"),
        game: Run::Game(games::glow_game),
        leans_on: &["shader_status"],
        start: None,
        checks: &["shader compiles in the game", "shader still glows"],
        check: check_broken_shader,
        solution: include_str!("tasks/broken-shader/solution.json"),
        controls: include_str!("tasks/broken-shader/controls.json"),
    },
    Task {
        id: "hidden-score",
        title: "Make the HUD show the score: what the player sees, checked headless",
        kind: "ui",
        goal: include_str!("tasks/hidden-score/goal.md"),
        files: task_files!("hidden-score": "assets/settings.json", "assets/hud.json"),
        game: Run::Game(games::hud_game),
        leans_on: &["run_input_scenario"],
        start: None,
        checks: &["score label shown", "label reads the score"],
        check: check_hidden_score,
        solution: include_str!("tasks/hidden-score/solution.json"),
        controls: include_str!("tasks/hidden-score/controls.json"),
    },
    Task {
        id: "lost-points",
        title: "A bug with two causes: fixing the visible one leaves the symptom as it was",
        kind: "bug",
        goal: include_str!("tasks/lost-points/goal.md"),
        files: task_files!("lost-points": "assets/dash.json", "assets/levels/run.json"),
        game: Run::Game(games::dash_game),
        leans_on: &[
            "watch",
            "subscribe_events",
            "run_input_scenario",
            "list_entities",
            "save_scene",
        ],
        start: None,
        checks: &[
            "settings file is valid",
            "level keeps its coins",
            "game uses the file",
            "a run scores 5",
        ],
        check: check_lost_points,
        solution: include_str!("tasks/lost-points/solution.json"),
        controls: include_str!("tasks/lost-points/controls.json"),
    },
    Task {
        id: "cannon-range",
        title: "Tune a value that can't be read off the file: measure the shot in the game",
        kind: "measure",
        goal: include_str!("tasks/cannon-range/goal.md"),
        files: task_files!("cannon-range": "assets/cannon.json"),
        game: Run::Game(games::cannon_game),
        leans_on: &["subscribe_events", "take_events", "set_resource", "run_ticks"],
        start: None,
        checks: &[
            "cannon file is valid",
            "game uses the file",
            "a shot lands on the target",
        ],
        check: check_cannon_range,
        solution: include_str!("tasks/cannon-range/solution.json"),
        controls: include_str!("tasks/cannon-range/controls.json"),
    },
    Task {
        id: "tough-bats",
        title: "One change across a scene, a prefab and a data file",
        kind: "content",
        goal: include_str!("tasks/tough-bats/goal.md"),
        files: task_files!("tough-bats": "assets/spawner.json", "assets/prefabs/bat.json", "assets/levels/cave.json"),
        game: Run::Game(games::cave_game),
        leans_on: &[
            "entity_stats",
            "watch",
            "load_scene",
            "save_scene",
            "spawn_prefab",
            "save_prefab",
        ],
        start: None,
        checks: &[
            "spawner file is right",
            "level bats have 30 health",
            "spawned bats have 30 health",
        ],
        check: check_tough_bats,
        solution: include_str!("tasks/tough-bats/solution.json"),
        controls: include_str!("tasks/tough-bats/controls.json"),
    },
    Task {
        id: "hold-the-gate",
        title: "Try alternatives from one saved state: place two towers that hold a wave",
        kind: "experiment",
        goal: include_str!("tasks/hold-the-gate/goal.md"),
        files: task_files!("hold-the-gate": "assets/defense.json"),
        game: Run::Game(games::defense_game),
        leans_on: &["save_world", "load_world", "watch", "list_entities"],
        start: Some(save_wave_start),
        checks: &["defense file is valid", "the file's towers stand", "the wave is held"],
        check: check_hold_the_gate,
        solution: include_str!("tasks/hold-the-gate/solution.json"),
        controls: include_str!("tasks/hold-the-gate/controls.json"),
    },
    Task {
        id: "stuck-guard",
        title: "Fix a bug in a Rust game's code, rebuilding it once per attempt",
        kind: "code",
        goal: include_str!("tasks/stuck-guard/goal.md"),
        files: task_files!("stuck-guard": "Cargo.toml", ".cargo/config.toml", "src/main.rs", "assets/patrol.json"),
        game: Run::Crate { bin: "patrol" },
        leans_on: &["launch", "quit_instance", "get_entity", "run_ticks", "subscribe_events"],
        start: None,
        checks: &[
            "game builds",
            "route file unchanged",
            "guard walks laps",
            "turns at the waypoints",
            "never faster than its speed",
        ],
        check: check_stuck_guard,
        solution: include_str!("tasks/stuck-guard/solution.json"),
        controls: include_str!("tasks/stuck-guard/controls.json"),
    },
];

/// The player entity's id.
fn player(p: &mut Probe) -> Result<Value, String> {
    let r = p.call("list_entities", json!({ "with": ["Player"] }))?;
    match r["entities"].as_array().map(Vec::as_slice) {
        Some([one]) => Ok(one["entity"].clone()),
        _ => Err(format!("expected one Player, found {}", r["total"])),
    }
}

/// Puts the player back at the start, then holds Right for `ticks` ticks.
fn hold_right(p: &mut Probe, ticks: u64) -> Result<Value, String> {
    let id = player(p)?;
    p.call(
        "set_component",
        json!({ "entity": id, "component": "Transform2D", "value": { "position": [START.x, START.y] } }),
    )?;
    p.scenario(json!({
        "version": 1,
        "duration": ticks,
        "steps": [{ "tick": 0, "type": "key", "key": "Right", "down": true }]
    }))
}

fn number(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("{what} is not a number: {v}"))
}

// ------------------------------------------------------------------ tune-speed

fn check_tune_speed(p: &mut Probe) -> Vec<Check> {
    let file = p
        .json_file("assets/settings.json")
        .and_then(|s| number(&s["speed"], "speed"));
    let mut checks = vec![Check::from(
        "settings file is valid",
        file.clone().map(|speed| format!("speed {speed}")),
    )];
    checks.push(Check::from(
        "game uses the file's speed",
        (|| {
            let file = file.clone()?;
            let live = number(&p.resource("Settings")?["speed"], "Settings.speed")?;
            if (live - file).abs() < 1e-3 {
                Ok(format!("{live}"))
            } else {
                Err(format!("the game runs at {live}, the file says {file}"))
            }
        })(),
    ));
    checks.push(Check::from(
        "flag reached in time",
        (|| {
            p.call(
                "set_resource",
                json!({ "resource": "Flag", "value": { "reached_tick": null } }),
            )?;
            let start = p.call("instance_info", json!({}))?["tick"]
                .as_u64()
                .ok_or("instance_info has no tick")?;
            hold_right(p, 130)?;
            let reached = p.resource("Flag")?["reached_tick"].as_u64();
            match reached.map(|t| t - start) {
                Some(ticks) if (91..=121).contains(&ticks) => Ok(format!("in {ticks} ticks")),
                Some(ticks) => Err(format!("in {ticks} ticks, wanted 91 to 121")),
                None => Err("not within 130 ticks".into()),
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ coin-level

const LEVEL: &str = "levels/coins.json";

fn check_coin_level(p: &mut Probe) -> Vec<Check> {
    let saved = p.json_file(&format!("assets/{LEVEL}"));
    let mut checks = vec![Check::from(
        "level file saved",
        saved.map(|_| format!("assets/{LEVEL}")),
    )];
    checks.push(Check::from(
        "level loads",
        p.call("load_scene", json!({ "path": LEVEL }))
            .map(|r| format!("{} entities", r["entities"])),
    ));
    checks.push(Check::from(
        "player and five coins",
        (|| {
            let coins = p.call("entity_stats", json!({ "fields": ["Coin.value"], "with": ["Coin"] }))?;
            let stats = &coins["fields"]["Coin.value"];
            let (count, min, max) = (coins["entities"].as_u64(), stats["min"].as_f64(), stats["max"].as_f64());
            if (count, min, max) != (Some(5), Some(1.0), Some(1.0)) {
                return Err(format!("wanted five coins worth 1, found {coins}"));
            }
            let id = player(p)?;
            let at = p.call("get_entity", json!({ "entity": id }))?["Transform2D"]["position"].clone();
            let at = (number(&at[0], "x")?, number(&at[1], "y")?);
            if (at.0 - f64::from(START.x)).abs() > 0.5 || (at.1 - f64::from(START.y)).abs() > 0.5 {
                return Err(format!("the player starts at {at:?}, not {START}"));
            }
            Ok("one player at the start, five coins".into())
        })(),
    ));
    checks.push(Check::from(
        "holding right collects them all",
        (|| {
            p.call("set_resource", json!({ "resource": "Score", "value": { "points": 0 } }))?;
            hold_right(p, 120)?;
            let points = p.resource("Score")?["points"].as_u64();
            let left = p.count(&["Coin"])?;
            match (points, left) {
                (Some(5), 0) => Ok("5 points in 2 seconds".into()),
                _ => Err(format!("{points:?} points, {left} coins left")),
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ immortal-enemy

/// The roster the task starts with: names and health must stay.
const ROSTER: &str = include_str!("tasks/immortal-enemy/files/assets/enemies.json");

fn check_immortal_enemy(p: &mut Probe) -> Vec<Check> {
    let mut checks = vec![Check::from(
        "roster file is fixed",
        (|| {
            let now = p.json_file("assets/enemies.json")?;
            let was: Value = serde_json::from_str(ROSTER).unwrap();
            let (now, was) = (
                now["enemies"].as_array().cloned().unwrap_or_default(),
                was["enemies"].as_array().unwrap(),
            );
            if now.len() != was.len() {
                return Err(format!("{} enemies, the roster had {}", now.len(), was.len()));
            }
            for (n, w) in now.iter().zip(was) {
                if n["name"] != w["name"] || n["health"] != w["health"] {
                    return Err(format!("{} changed more than its armor: {n}", w["name"]));
                }
                let armor = number(&n["armor"], "armor")?;
                if !(0.0..1.0).contains(&armor) {
                    return Err(format!("{} still has armor {armor}", n["name"]));
                }
            }
            Ok("every armor in [0, 1)".into())
        })(),
    )];
    checks.push(Check::from(
        "no live enemy heals",
        (|| {
            let r = p.call(
                "list_entities",
                json!({ "with": ["Enemy"], "where": ["Enemy.armor >= 1"] }),
            )?;
            match r["total"].as_u64() {
                Some(0) => Ok("none".into()),
                _ => Err(format!("{} enemies have armor >= 1", r["total"])),
            }
        })(),
    ));
    checks.push(Check::from(
        "a fresh wave dies in 5 seconds",
        (|| {
            let size = serde_json::from_str::<Value>(ROSTER).unwrap()["enemies"]
                .as_array()
                .unwrap()
                .len();
            p.call(
                "set_resource",
                json!({ "resource": "Waves", "value": { "wave": true } }),
            )?;
            p.call("subscribe_events", json!({ "types": ["Died"] }))?;
            p.call("run_ticks", json!({ "ticks": 300 }))?;
            let died = p.call("take_events", json!({}))?["events"]
                .as_array()
                .map_or(0, Vec::len);
            let left = p.count(&["Enemy"])?;
            if died == size && left == 0 {
                Ok(format!("{died} died"))
            } else {
                Err(format!("{died} of {size} died, {left} left"))
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ slow-crowd

/// The radius the crowd is measured with.
const RADIUS: f64 = 24.0;

fn check_slow_crowd(p: &mut Probe) -> Vec<Check> {
    let file = p.json_file("assets/tuning.json");
    let mut checks = vec![Check::from(
        "radius unchanged",
        (|| {
            let radius = number(&file.clone()?["radius"], "radius")?;
            if radius == RADIUS {
                Ok(format!("{radius}"))
            } else {
                Err(format!("radius is {radius}, was {RADIUS}"))
            }
        })(),
    )];
    checks.push(Check::from(
        "game uses the file",
        (|| {
            let file = file.clone()?;
            let live = p.resource("Tuning")?;
            if super::same(&live["radius"], &file["radius"]) && super::same(&live["cell_size"], &file["cell_size"]) {
                Ok(format!("{live}"))
            } else {
                Err(format!("the game uses {live}, the file says {file}"))
            }
        })(),
    ));
    let crowding = p
        .call("run_ticks", json!({ "ticks": 1 }))
        .and_then(|_| p.resource("Crowding"));
    let n = games::VILLAGERS as u64;
    let everything = n * (n - 1) / 2;
    checks.push(Check::from(
        "work cut 10x",
        (|| {
            let checks = crowding.clone()?["checks"].as_u64().ok_or("no Crowding.checks")?;
            if checks * 10 <= everything {
                Ok(format!("{checks} distance checks a tick (all pairs: {everything})"))
            } else {
                Err(format!("{checks} distance checks a tick; all pairs are {everything}"))
            }
        })(),
    ));
    checks.push(Check::from(
        "same pairs found",
        (|| {
            let pairs = crowding.clone()?["pairs"].as_u64().ok_or("no Crowding.pairs")?;
            // Counted here from the positions the game reports, all pairs.
            let r = p.call("list_entities", json!({ "with": ["Villager"], "limit": 10000 }))?;
            let at: Vec<(f64, f64)> = r["entities"]
                .as_array()
                .ok_or("no entities")?
                .iter()
                .map(|e| {
                    let pos = &e["components"]["Transform2D"]["position"];
                    (pos[0].as_f64().unwrap_or(f64::NAN), pos[1].as_f64().unwrap_or(f64::NAN))
                })
                .collect();
            let mut want = 0;
            for (i, a) in at.iter().enumerate() {
                for b in &at[i + 1..] {
                    if ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() < RADIUS {
                        want += 1;
                    }
                }
            }
            if pairs == want {
                Ok(format!("{pairs} pairs"))
            } else {
                Err(format!("the game found {pairs} pairs, there are {want}"))
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ broken-shader

fn check_broken_shader(p: &mut Probe) -> Vec<Check> {
    let mut checks = vec![Check::from(
        "shader compiles in the game",
        (|| {
            let status = p.call("shader_status", json!({}))?;
            let glow = status["shaders"]
                .as_array()
                .and_then(|s| s.iter().find(|s| s["shader"] == games::GLOW_SHADER))
                .ok_or_else(|| format!("{} is not in shader_status: {status}", games::GLOW_SHADER))?;
            if glow["ok"] == true {
                Ok(format!("{}", glow["outcome"]))
            } else {
                Err(format!("{}: {}", glow["outcome"], glow["diagnostics"][0]["text"]))
            }
        })(),
    )];
    checks.push(Check::from(
        "shader still glows",
        (|| {
            let source = p.text_file(&format!("assets/{}", games::GLOW_SHADER))?;
            let code: String = source
                .lines()
                .map(|l| l.split("//").next().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n");
            if code.contains("material.params[0]") && code.contains("fn fragment") {
                Ok("reads the glow parameter".into())
            } else {
                Err("the shader no longer reads material.params[0], the glow amount".into())
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ hidden-score

fn check_hidden_score(p: &mut Probe) -> Vec<Check> {
    let ui = |tick: u64, check: Value| {
        let mut step = json!({ "tick": tick, "type": "expect_ui", "node": "hud.score" });
        step.as_object_mut().unwrap().extend(check.as_object().unwrap().clone());
        json!({ "version": 1, "duration": tick + 1, "steps": [step] })
    };
    let mut checks = vec![Check::from(
        "score label shown",
        p.scenario(ui(1, json!({ "shown": true }))).map(|_| "shown".into()),
    )];
    checks.push(Check::from(
        "label reads the score",
        (|| {
            p.call("set_resource", json!({ "resource": "Score", "value": { "points": 7 } }))?;
            p.scenario(ui(1, json!({ "text": "Score: 7" })))?;
            Ok("\"Score: 7\" at 7 points".into())
        })(),
    ));
    checks
}

/// The entities of a scene or prefab file with component `name`: their
/// components.
fn file_entities(file: &Value, name: &str) -> Vec<Value> {
    file["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| e["components"].get(name).is_some())
        .map(|e| e["components"].clone())
        .collect()
}

/// `[x, y]` of a `Transform2D` value.
fn position(transform: &Value) -> Result<(f64, f64), String> {
    let p = &transform["position"];
    Ok((number(&p[0], "x")?, number(&p[1], "y")?))
}

/// The positions of `components`' transforms, sorted.
fn positions(components: &[Value]) -> Result<Vec<(f64, f64)>, String> {
    let mut at = components
        .iter()
        .map(|c| position(&c["Transform2D"]))
        .collect::<Result<Vec<_>, _>>()?;
    at.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(at)
}

// ------------------------------------------------------------------ lost-points

/// The level the task starts with: its coins must stay.
const RUN_LEVEL: &str = include_str!("tasks/lost-points/files/assets/levels/run.json");

fn check_lost_points(p: &mut Probe) -> Vec<Check> {
    let file = p.json_file("assets/dash.json");
    let mut checks = vec![Check::from(
        "settings file is valid",
        (|| {
            let file = file.clone()?;
            let speed = number(&file["speed"], "speed")?;
            let radius = number(&file["pickup_radius"], "pickup_radius")?;
            if speed != 300.0 {
                return Err(format!("speed is {speed}, was 300"));
            }
            if !(radius > 0.0 && radius <= 32.0) {
                return Err(format!("pickup_radius is {radius}; it must stay at most 32"));
            }
            Ok(format!("pickup radius {radius}"))
        })(),
    )];
    checks.push(Check::from(
        "level keeps its coins",
        (|| {
            let level = p.json_file(&format!("assets/{}", games::DASH_LEVEL))?;
            let was: Value = serde_json::from_str(RUN_LEVEL).unwrap();
            let (now_coins, was_coins) = (file_entities(&level, "Coin"), file_entities(&was, "Coin"));
            if now_coins.iter().any(|c| c["Coin"]["value"] != 1) {
                return Err("a coin is no longer worth 1".into());
            }
            if positions(&now_coins)? != positions(&was_coins)? {
                return Err(format!(
                    "the coins moved: {:?}, were {:?}",
                    positions(&now_coins)?,
                    positions(&was_coins)?
                ));
            }
            let players = positions(&file_entities(&level, "Player"))?;
            if players != [(f64::from(START.x), f64::from(START.y))] {
                return Err(format!("wanted one player at {START}, found {players:?}"));
            }
            Ok("five coins where they were, the player at the start".into())
        })(),
    ));
    let loaded = p.call("load_scene", json!({ "path": games::DASH_LEVEL }));
    checks.push(Check::from(
        "game uses the file",
        (|| {
            loaded.clone()?;
            let file = file.clone()?;
            let live = p.resource("DashSettings")?;
            if super::same(&live["pickup_radius"], &file["pickup_radius"])
                && super::same(&live["speed"], &file["speed"])
            {
                Ok(format!("{live} after loading the level"))
            } else {
                Err(format!(
                    "after loading the level the game uses {live}; the file says {file}"
                ))
            }
        })(),
    ));
    checks.push(Check::from(
        "a run scores 5",
        (|| {
            loaded.clone()?;
            p.call("set_resource", json!({ "resource": "Score", "value": { "points": 0 } }))?;
            hold_right(p, 150)?;
            let points = p.resource("Score")?["points"].as_u64();
            let left = p.count(&["Coin"])?;
            match (points, left) {
                (Some(5), 0) => Ok("5 points at the end of the run".into()),
                _ => Err(format!("{points:?} points at the end of the run, {left} coins left")),
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ cannon-range

fn check_cannon_range(p: &mut Probe) -> Vec<Check> {
    let file = p.json_file("assets/cannon.json");
    let mut checks = vec![Check::from(
        "cannon file is valid",
        (|| {
            let file = file.clone()?;
            let speed = number(&file["muzzle_speed"], "muzzle_speed")?;
            let angle = number(&file["angle_degrees"], "angle_degrees")?;
            if angle != 45.0 {
                return Err(format!("angle_degrees is {angle}, was 45"));
            }
            if speed <= 0.0 {
                return Err(format!("muzzle_speed is {speed}"));
            }
            Ok(format!("muzzle speed {speed}"))
        })(),
    )];
    checks.push(Check::from(
        "game uses the file",
        (|| {
            let file = file.clone()?;
            let live = p.resource("CannonSettings")?;
            if super::same(&live["muzzle_speed"], &file["muzzle_speed"])
                && super::same(&live["angle_degrees"], &file["angle_degrees"])
            {
                Ok(format!("{live}"))
            } else {
                Err(format!("the game uses {live}, the file says {file}"))
            }
        })(),
    ));
    checks.push(Check::from(
        "a shot lands on the target",
        (|| {
            // A ball still in the air from the agent's tries lands first.
            p.call("run_ticks", json!({ "ticks": 600 }))?;
            p.call("subscribe_events", json!({ "types": ["Landed"] }))?;
            p.call(
                "set_resource",
                json!({ "resource": "Trigger", "value": { "fire": true } }),
            )?;
            p.call("run_ticks", json!({ "ticks": 600 }))?;
            let events = p.call("take_events", json!({}))?["events"].clone();
            match events.as_array().map(Vec::as_slice) {
                Some([one]) => {
                    let x = number(&one["data"]["x"], "x")?;
                    if (600.0..=640.0).contains(&x) {
                        Ok(format!("landed at x = {x:.1}"))
                    } else {
                        Err(format!("landed at x = {x:.1}, the target is 600 to 640"))
                    }
                }
                _ => Err(format!("wanted one landing, saw {events}")),
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ tough-bats

/// The files the task starts with.
const CAVE_LEVEL: &str = include_str!("tasks/tough-bats/files/assets/levels/cave.json");
const SPAWNER: &str = include_str!("tasks/tough-bats/files/assets/spawner.json");

fn check_tough_bats(p: &mut Probe) -> Vec<Check> {
    let mut checks = vec![Check::from(
        "spawner file is right",
        (|| {
            let now = p.json_file("assets/spawner.json")?;
            let was: Value = serde_json::from_str(SPAWNER).unwrap();
            for key in ["prefab", "max_bats"] {
                if !super::same(&now[key], &was[key]) {
                    return Err(format!("{key} is {}, was {}", now[key], was[key]));
                }
            }
            let every = number(&now["every_secs"], "every_secs")?;
            let before = number(&was["every_secs"], "every_secs")?;
            if (every - 2.0 * before).abs() > 1e-3 {
                return Err(format!(
                    "every_secs is {every}; half as often as {before} is {}",
                    2.0 * before
                ));
            }
            Ok(format!("a bat every {every} s"))
        })(),
    )];
    checks.push(Check::from(
        "level bats have 30 health",
        (|| {
            let level = p.json_file(&format!("assets/{}", games::CAVE_LEVEL))?;
            let was: Value = serde_json::from_str(CAVE_LEVEL).unwrap();
            let (now, before) = (file_entities(&level, "Bat"), file_entities(&was, "Bat"));
            if positions(&now)? != positions(&before)? {
                return Err(format!(
                    "the level's bats moved or changed in number: {:?}",
                    positions(&now)?
                ));
            }
            if let Some(bat) = now.iter().find(|b| !super::same(&b["Health"]["current"], &json!(30.0))) {
                return Err(format!("a bat in the level has {}", bat["Health"]));
            }
            if file_entities(&level, "Player").len() != 1 {
                return Err("the level must keep its one player".into());
            }
            Ok(format!("{} bats with 30", now.len()))
        })(),
    ));
    checks.push(Check::from(
        "spawned bats have 30 health",
        (|| {
            let before = file_entities(&serde_json::from_str(CAVE_LEVEL).unwrap(), "Bat").len() as u64;
            p.call("load_scene", json!({ "path": games::CAVE_LEVEL }))?;
            p.call(
                "set_resource",
                json!({ "resource": "Spawning", "value": { "secs": 0.0 } }),
            )?;
            // A bat every 6 seconds: none in the first 5.9, one by 6.2.
            p.call("run_ticks", json!({ "ticks": 354 }))?;
            let early = p.count(&["Bat"])?;
            if early != before {
                return Err(format!(
                    "{early} bats after 5.9 s; the spawner adds them faster than every 6 s"
                ));
            }
            p.call("run_ticks", json!({ "ticks": 18 }))?;
            let stats = p.call("entity_stats", json!({ "fields": ["Health.current"], "with": ["Bat"] }))?;
            let health = &stats["fields"]["Health.current"];
            if stats["entities"].as_u64() != Some(before + 1) {
                return Err(format!("{} bats after 6.2 s, wanted {}", stats["entities"], before + 1));
            }
            if super::same(&health["min"], &json!(30.0)) && super::same(&health["max"], &json!(30.0)) {
                Ok("the new bat and the level's have 30".into())
            } else {
                Err(format!("bats' health after a spawn: {health}"))
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ hold-the-gate

/// Where the session keeps the wave's start, for the checker.
pub const WAVE_START: &str = "checker/wave-start.json";

fn save_wave_start(p: &mut Probe) -> Result<(), String> {
    p.call("save_world", json!({ "path": WAVE_START, "overwrite": true }))
        .map(|_| ())
}

fn check_hold_the_gate(p: &mut Probe) -> Vec<Check> {
    let file = p.json_file("assets/defense.json");
    let towers = (|| {
        let file = file.clone()?;
        let towers: Vec<String> = serde_json::from_value(file["towers"].clone())
            .map_err(|_| format!("`towers` must be a list of pad names: {}", file["towers"]))?;
        Ok::<_, String>(towers)
    })();
    let mut checks = vec![Check::from(
        "defense file is valid",
        (|| {
            let towers = towers.clone()?;
            if towers.len() > 2 {
                return Err(format!("{} towers; the budget is 2", towers.len()));
            }
            for (i, t) in towers.iter().enumerate() {
                if !games::PADS.iter().any(|(n, _)| n == t) {
                    return Err(format!("no pad `{t}`"));
                }
                if towers[..i].contains(t) {
                    return Err(format!("two towers on `{t}`"));
                }
            }
            Ok(format!("{towers:?}"))
        })(),
    )];
    let restarted = p
        .call("load_world", json!({ "path": WAVE_START }))
        .and_then(|_| p.call("run_ticks", json!({ "ticks": 1 })));
    checks.push(Check::from(
        "the file's towers stand",
        (|| {
            restarted.clone()?;
            let mut want = towers.clone()?;
            let r = p.call("list_entities", json!({ "with": ["Tower"] }))?;
            let mut stand: Vec<String> = r["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["components"]["Tower"]["pad"].as_str().map(String::from))
                .collect();
            want.sort();
            stand.sort();
            if stand == want {
                Ok(format!("{stand:?}"))
            } else {
                Err(format!("towers on {stand:?}, the file asks for {want:?}"))
            }
        })(),
    ));
    checks.push(Check::from(
        "the wave is held",
        (|| {
            restarted.clone()?;
            p.call("run_ticks", json!({ "ticks": 3600 }))?;
            let gate = p.resource("Gate")?;
            let (breaches, stopped) = (gate["breaches"].as_u64(), gate["stopped"].as_u64());
            if breaches == Some(0) && stopped == Some(games::WAVE.len() as u64) {
                Ok(format!("all {} walkers stopped", games::WAVE.len()))
            } else {
                Err(format!("{breaches:?} reached the gate, {stopped:?} stopped"))
            }
        })(),
    ));
    checks
}

// ------------------------------------------------------------------ stuck-guard

/// The route the task starts with: it must stay.
const PATROL: &str = include_str!("tasks/stuck-guard/files/assets/patrol.json");

fn check_stuck_guard(p: &mut Probe) -> Vec<Check> {
    let built = p.build.clone().unwrap_or_else(|| Err("not a code task".into()));
    let mut checks = vec![Check::from("game builds", built.clone())];
    let route: Value = serde_json::from_str(PATROL).unwrap();
    checks.push(Check::from(
        "route file unchanged",
        p.json_file("assets/patrol.json").and_then(|now| {
            if now == route {
                Ok("as it was".into())
            } else {
                Err(format!("the route file changed: {now}"))
            }
        }),
    ));
    let waypoints: Vec<(f64, f64)> = route["waypoints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| (w[0].as_f64().unwrap(), w[1].as_f64().unwrap()))
        .collect();
    let step = route["speed"].as_f64().unwrap() / 60.0;
    // A lap is four sides of 200 pixels, a corner about every 93 ticks.
    // First the whole run, with the `Reached` events the game sends; then,
    // from the same start (a world save), a position every tick around
    // the first four corners (each read is a round trip to the game).
    let run = (|| {
        built.clone()?;
        let r = p.call("list_entities", json!({ "with": ["Guard"] }))?;
        let id = match r["entities"].as_array().map(Vec::as_slice) {
            Some([one]) => one["entity"].clone(),
            _ => return Err(format!("expected one Guard, found {}", r["total"])),
        };
        let start = p.call("instance_info", json!({}))?["tick"].as_u64().ok_or("no tick")?;
        p.call(
            "save_world",
            json!({ "path": "checker/guard-start.json", "overwrite": true }),
        )?;
        p.call("subscribe_events", json!({ "types": ["Reached"] }))?;
        p.call("run_ticks", json!({ "ticks": 920 - start }))?;
        let laps = p.call("get_entity", json!({ "entity": id }))?["Guard"]["laps"].as_u64();
        let reached: Vec<u64> = p.call("take_events", json!({}))?["events"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| e["data"]["tick"].as_u64())
            .collect();
        // Where the corners are: from the events, else every 93 ticks.
        let corners: Vec<u64> = (0..4)
            .map(|k| reached.get(k).map_or(93 * (k as u64 + 1), |t| t.saturating_sub(start)))
            .collect();
        p.call("load_world", json!({ "path": "checker/guard-start.json" }))?;
        let mut tick = 0;
        let mut windows = Vec::new();
        for corner in corners {
            let from = corner.saturating_sub(7).max(tick);
            if from > tick {
                p.call("run_ticks", json!({ "ticks": from - tick }))?;
                tick = from;
            }
            let mut at = Vec::new();
            while tick <= corner + 7 {
                at.push(position(
                    &p.call("get_entity", json!({ "entity": id }))?["Transform2D"],
                )?);
                p.call("run_ticks", json!({ "ticks": 1 }))?;
                tick += 1;
            }
            windows.push(at);
        }
        Ok((laps, windows))
    })();
    checks.push(Check::from(
        "guard walks laps",
        (|| match run.clone()?.0 {
            Some(n) if n >= 2 => Ok(format!("{n} laps in 920 ticks")),
            laps => Err(format!("{laps:?} laps in 920 ticks; a lap takes about 380")),
        })(),
    ));
    checks.push(Check::from(
        "turns at the waypoints",
        (|| {
            let (_, windows) = run.clone()?;
            for (i, at) in windows.iter().enumerate() {
                let want = waypoints[(i + 1) % waypoints.len()];
                let corner = turn(at).map_err(|e| format!("near waypoint {want:?}: {e}"))?;
                let off = ((corner.0 - want.0).powi(2) + (corner.1 - want.1).powi(2)).sqrt();
                if off > 0.5 {
                    return Err(format!("turns at {corner:?}, {off:.2} px from waypoint {want:?}"));
                }
            }
            Ok("each of the first four turns on its waypoint".into())
        })(),
    ));
    checks.push(Check::from(
        "never faster than its speed",
        (|| {
            let (_, windows) = run.clone()?;
            let fastest = windows
                .iter()
                .flat_map(|at| at.windows(2))
                .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
                .fold(0.0, f64::max);
            if fastest <= step + 1e-3 {
                Ok(format!(
                    "at most {fastest:.3} px a tick at the corners (speed: {step:.3})"
                ))
            } else {
                Err(format!("moved {fastest:.3} px in a tick; its speed is {step:.3}"))
            }
        })(),
    ));
    checks
}

/// Where a walk of positions (one a tick) turns: the meeting point of the
/// straight runs before and after the turn, so a walker that carries the
/// rest of a step round a corner turns on it too. One step between them
/// may cut the corner.
fn turn(at: &[(f64, f64)]) -> Result<(f64, f64), String> {
    let mut at = at.to_vec();
    // A tick it stood still is no direction.
    at.dedup();
    let dir = |a: (f64, f64), b: (f64, f64)| {
        let (x, y) = (b.0 - a.0, b.1 - a.1);
        let l = (x * x + y * y).sqrt();
        (x / l, y / l)
    };
    let same = |a: (f64, f64), b: (f64, f64)| a.0 * b.0 + a.1 * b.1 > 0.9999;
    // Runs of steps in one direction: (first point, direction, steps).
    type Point = (f64, f64);
    let mut runs: Vec<(Point, Point, usize)> = Vec::new();
    for w in at.windows(2) {
        let d = dir(w[0], w[1]);
        match runs.last_mut() {
            Some(run) if same(run.1, d) => run.2 += 1,
            _ => runs.push((w[0], d, 1)),
        }
    }
    let straight: Vec<_> = runs.iter().filter(|r| r.2 >= 2).collect();
    let (a, c) = match straight.as_slice() {
        [a, c, ..] => (a, c),
        _ => return Err(format!("no turn between two straight runs: {at:?}")),
    };
    let cross = a.1.0 * c.1.1 - a.1.1 * c.1.0;
    if cross.abs() < 1e-6 {
        return Err(format!("turns back on itself near {:?}", c.0));
    }
    // a.0 + s * a.1 = c.0 + t * c.1
    let s = ((c.0.0 - a.0.0) * c.1.1 - (c.0.1 - a.0.1) * c.1.0) / cross;
    Ok((a.0.0 + s * a.1.0, a.0.1 + s * a.1.1))
}
