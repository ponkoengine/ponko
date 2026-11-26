//! The MCP server end to end: real headless games with the control endpoint
//! on, driven through `Server::handle` the way an agent's client would.

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[allow(unused_imports)]
use ponko::input::Input;
use ponko::prelude::*;
use ponko_mcp::{Config, Server};
use serde_json::{Value, json};

#[derive(serde::Serialize, serde::Deserialize, Component)]
struct Health {
    current: f32,
    max: f32,
}

#[derive(serde::Serialize)]
struct Regen {
    tick: u64,
}

/// Game-wide state, reflected so agents can read and change it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Rules {
    regen: f32,
    round: u32,
}

fn regen(ctx: &mut Ctx) {
    ctx.world
        .query::<&mut Health>()
        .for_each(|h| h.current = (h.current + 1.0).min(h.max));
    let tick = ctx.time.sim_tick();
    ctx.events.send(Regen { tick });
    ctx.world.resource_mut::<Rules>().round += 1;
}

/// Starts a paused headless game; returns its port and the thread.
fn start_game(name: &'static str) -> (u16, thread::JoinHandle<World>) {
    start_game_with(name, false)
}

/// Like [`start_game`], and with a "jump" action on Space; `injection` lets
/// tools start input scenarios.
fn start_game_with(name: &'static str, injection: bool) -> (u16, thread::JoinHandle<World>) {
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        Game::new(name, 1, 1)
            .control_endpoint(true)
            .input_injection(injection)
            .developer_console(true)
            .console_variable(
                "round_scale",
                "Scale the round",
                ponko::console::ConsoleValue::Float(1.0),
            )
            .console_command(
                "bump_round",
                "Advance the round",
                &[ponko::console::ConsoleArg::new(
                    "by",
                    ponko::console::ConsoleType::Integer,
                )],
                |ctx, args| {
                    let [ponko::console::ConsoleValue::Integer(by)] = args else {
                        unreachable!("arguments are validated before dispatch");
                    };
                    let rules = ctx.world.resource_mut::<Rules>();
                    rules.round += *by as u32;
                    Ok(rules.round.to_string())
                },
            )
            .sim_system(regen)
            // What a plugin adds for tools: a debug toggle and a method.
            .debug_toggle("rules.regen", false)
            .control_method(
                "rules.set_round",
                "sets the round: params {\"round\": number}",
                |ctx: &mut Ctx, params: &Value| {
                    let round = params["round"]
                        .as_u64()
                        .ok_or_else(|| Error::new(ErrorKind::Invalid, "`round` must be a whole number"))?;
                    ctx.world.resource_mut::<Rules>().round = round as u32;
                    Ok(json!({ "round": round }))
                },
            )
            .run_headless(u64::MAX, move |ctx| {
                ctx.world.reflect::<Health>();
                ctx.events.reflect::<Regen>();
                ctx.world.reflect_resource::<Rules>();
                ctx.world.insert_resource(Rules { regen: 1.0, round: 0 });
                let mut actions = ActionMap::default();
                actions.set("jump", Action::Button(vec![ButtonBinding::Key(Key::Space)]));
                ctx.input.set_actions(actions);
                ctx.time.pause();
                tx.send(ctx.control_addr().unwrap().port()).unwrap();
            })
    });
    (rx.recv_timeout(Duration::from_secs(10)).unwrap(), h)
}

struct Agent {
    server: Server,
    next: u64,
}

impl Agent {
    fn new(dir: &Path, read_only: bool) -> Self {
        let server = Server::new(Config {
            instances_dir: dir.to_path_buf(),
            logs_dir: dir.join("logs"),
            read_only,
        });
        Self { server, next: 0 }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let reply = self
            .server
            .handle(&json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }))
            .unwrap();
        assert_eq!(reply["id"], self.next);
        reply
    }

    /// Calls a tool; `Ok(json)` or `Err(message)`.
    fn tool(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let r = self.rpc("tools/call", json!({ "name": name, "arguments": args }))["result"].clone();
        let text = r["content"][0]["text"].as_str().unwrap().to_string();
        if r["isError"] == true {
            Err(text)
        } else {
            Ok(serde_json::from_str(&text).unwrap())
        }
    }

    fn tool_names(&mut self) -> Vec<String> {
        self.rpc("tools/list", json!({}))["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }
}

// One test function, because it sets an environment variable that the games
// read (where to register), and tests in a file run in parallel.
#[test]
fn agents_drive_several_games_through_mcp() {
    let dir = std::env::temp_dir().join(format!("ponko-mcp-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: set before any other thread of this test binary starts.
    unsafe { std::env::set_var("PONKO_INSTANCES_DIR", &dir) };
    // The games run in this process, and save files relative to its working
    // folder: make that the test's own directory.
    std::fs::create_dir_all(&dir).unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let mut agent = Agent::new(&dir, false);

    // Handshake.
    let init = agent.rpc("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "ponko-mcp");
    assert!(
        agent
            .server
            .handle(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .is_none()
    );
    assert_eq!(agent.rpc("nope", json!({}))["error"]["code"], -32601);

    // No games yet.
    assert_eq!(agent.tool("list_instances", json!({})).unwrap(), json!([]));
    assert!(
        agent
            .tool("instance_info", json!({}))
            .unwrap_err()
            .contains("no game is running")
    );

    let (port_a, game_a) = start_game("alpha");
    let (port_b, game_b) = start_game("beta");

    let list = agent.tool("list_instances", json!({})).unwrap();
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert!(
        list.iter().all(|i| i.get("token").is_none()),
        "tokens are never shown to agents"
    );
    let err = agent.tool("world_summary", json!({})).unwrap_err();
    assert!(err.contains("2 games are running"), "{err}");

    let cam = agent
        .tool(
            "set_camera",
            json!({ "instance": "alpha", "zoom": 4.0, "position": [10.0, 20.0] }),
        )
        .unwrap();
    assert_eq!(cam["position"], json!([10.0, 20.0]));
    assert_eq!(cam["zoom"], 4.0);

    let err = agent.tool("screenshot", json!({ "instance": "alpha" })).unwrap_err();
    assert!(err.contains("no window"), "headless games say why: {err}");

    // A method the game added, and its debug toggle.
    let answer = agent
        .tool(
            "call_game_method",
            json!({ "instance": "beta", "method": "rules.set_round", "params": { "round": 7 } }),
        )
        .unwrap();
    assert_eq!(answer, json!({ "round": 7 }));
    let rules = agent
        .tool("get_resource", json!({ "instance": "beta", "resource": "Rules" }))
        .unwrap();
    assert_eq!(rules["round"], 7, "{rules}");
    let refused = agent
        .tool(
            "call_game_method",
            json!({ "instance": "beta", "method": "rules.set_round" }),
        )
        .unwrap_err();
    assert!(refused.contains("`round` must be a whole number"), "{refused}");
    let refused = agent
        .tool("call_game_method", json!({ "instance": "beta", "method": "game.quit" }))
        .unwrap_err();
    assert!(refused.contains("not a method this game added"), "{refused}");
    let drawn = agent
        .tool(
            "debug_draw",
            json!({ "instance": "beta", "toggles": { "rules.regen": true } }),
        )
        .unwrap();
    assert_eq!(drawn["toggles"], json!({ "rules.regen": true }), "{drawn}");
    let methods = agent
        .tool("instance_capabilities", json!({ "instance": "beta" }))
        .unwrap();
    assert!(
        methods.to_string().contains("rules.set_round"),
        "the added method is reported: {methods}"
    );

    // Capabilities: what alpha can do, learned without changing it.
    let tick_before = agent.tool("instance_info", json!({ "instance": "alpha" })).unwrap()["tick"].clone();
    let caps = agent
        .tool("instance_capabilities", json!({ "instance": "alpha" }))
        .unwrap();
    assert_eq!(caps["format"], "ponko-capabilities");
    assert_eq!(
        (caps["mode"].clone(), caps["name"].clone()),
        (json!("headless"), json!("alpha"))
    );
    assert_eq!(caps["subsystems"]["renderer"]["active"], false);
    let tool = |caps: &Value, name: &str| {
        caps["mcp"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing"))
            .clone()
    };
    let shot = tool(&caps, "screenshot");
    assert_eq!(shot["available"], false);
    assert!(shot["reason"].as_str().unwrap().contains("no window"), "{shot}");
    assert_eq!(tool(&caps, "spawn_entity")["available"], true);
    let schemas = caps["reflection"]["components"].as_array().unwrap();
    let health = schemas.iter().find(|c| c["name"] == "Health").unwrap();
    assert_eq!(health["schema"]["status"], "opaque", "Health isn't described: {health}");
    assert!(
        caps["reflection"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["name"] == "Rules")
    );
    assert_eq!(
        agent.tool("instance_info", json!({ "instance": "alpha" })).unwrap()["tick"],
        tick_before,
        "discovery doesn't step the game"
    );

    // Drive alpha by name, beta by port.
    let e = agent
        .tool(
            "spawn_entity",
            json!({ "instance": "alpha", "components": { "Health": { "current": 1, "max": 10 } } }),
        )
        .unwrap()["entity"]
        .as_u64()
        .unwrap();
    let ran = agent
        .tool("run_ticks", json!({ "instance": "alpha", "ticks": 4 }))
        .unwrap();
    assert_eq!(ran["ticks_run"], 4);
    assert_eq!(
        agent
            .tool("get_entity", json!({ "instance": "alpha", "entity": e }))
            .unwrap(),
        json!({ "Health": { "current": 5.0, "max": 10.0 } })
    );
    let set = agent
        .tool(
            "set_component",
            json!({ "instance": "alpha", "entity": e, "component": "Health", "value": { "max": 6 } }),
        )
        .unwrap();
    assert_eq!(set, json!({ "current": 5.0, "max": 6.0 }));
    agent
        .tool("run_ticks", json!({ "instance": "alpha", "ticks": 3 }))
        .unwrap();
    assert_eq!(
        agent
            .tool("get_entity", json!({ "instance": "alpha", "entity": e }))
            .unwrap()["Health"]["current"],
        6.0
    );

    // Questions about many entities: filtered lists and summaries.
    agent
        .tool(
            "spawn_entity",
            json!({ "instance": "alpha", "components": { "Health": { "current": 2, "max": 2 } } }),
        )
        .unwrap();
    let low = agent
        .tool(
            "list_entities",
            json!({ "instance": "alpha", "where": ["Health.current < 4"] }),
        )
        .unwrap();
    assert_eq!(low["total"], 1);
    let stats = agent
        .tool(
            "entity_stats",
            json!({ "instance": "alpha", "fields": ["Health.current", "Health.max"] }),
        )
        .unwrap();
    assert_eq!(stats["entities"], 2);
    assert_eq!(
        stats["fields"]["Health.current"],
        json!({ "count": 2, "min": 2.0, "max": 6.0, "mean": 4.0 })
    );
    // Without fields: only how many match.
    let counted = agent
        .tool(
            "entity_stats",
            json!({ "instance": "alpha", "with": ["Health"], "where": ["Health.current < 4"] }),
        )
        .unwrap();
    assert_eq!(counted, json!({ "entities": 1, "fields": {} }));
    let err = agent
        .tool(
            "entity_stats",
            json!({ "instance": "alpha", "fields": ["Mana.current"] }),
        )
        .unwrap_err();
    assert!(err.contains("not a reflected component"), "{err}");
    agent
        .tool(
            "despawn_entity",
            json!({ "instance": "alpha", "entity": low["entities"][0]["entity"] }),
        )
        .unwrap();

    // Resources: listed with their values, changed field by field.
    assert_eq!(
        agent.tool("list_resources", json!({ "instance": "alpha" })).unwrap(),
        json!({ "Rules": { "regen": 1.0, "round": 7 } })
    );
    let rules = agent
        .tool(
            "set_resource",
            json!({ "instance": "alpha", "resource": "Rules", "value": { "regen": 2.5 } }),
        )
        .unwrap();
    assert_eq!(rules, json!({ "regen": 2.5, "round": 7 }));
    assert_eq!(
        agent
            .tool("get_resource", json!({ "instance": "alpha", "resource": "Rules" }))
            .unwrap()["regen"],
        2.5
    );
    let err = agent
        .tool("get_resource", json!({ "instance": "alpha", "resource": "Gold" }))
        .unwrap_err();
    assert!(err.contains("no reflected resource"), "{err}");

    // Watch: run 10 ticks, sampled every 5, in one call.
    let watched = agent
        .tool(
            "watch",
            json!({ "instance": "alpha", "ticks": 10, "every": 5, "resources": ["Rules"], "count": ["Health", "Nothing"] }),
        )
        .unwrap();
    let samples = watched["samples"].as_array().unwrap();
    let ticks: Vec<u64> = samples.iter().map(|s| s["tick"].as_u64().unwrap()).collect();
    assert_eq!(ticks, [7, 12, 17], "a sample before, then one every 5 ticks");
    assert_eq!(samples[2]["resources"]["Rules"]["round"], 17);
    assert_eq!(samples[2]["counts"], json!({ "Health": 1, "Nothing": 0 }));
    assert!(samples[0]["wall_ms_per_tick"].is_null() && samples[1]["wall_ms_per_tick"].is_number());
    let err = agent
        .tool("watch", json!({ "instance": "alpha", "ticks": 1_000_000, "every": 1 }))
        .unwrap_err();
    assert!(err.contains("too many"), "{err}");

    // Beta is untouched.
    let b = json!(port_b.to_string());
    assert_eq!(
        agent.tool("instance_info", json!({ "instance": b })).unwrap()["tick"],
        0
    );
    assert_eq!(
        agent.tool("world_summary", json!({ "instance": b })).unwrap()["entities"],
        0
    );

    // run_ticks refuses a running sim, so "exactly N" holds.
    agent.tool("resume", json!({ "instance": b })).unwrap();
    assert!(
        agent
            .tool("run_ticks", json!({ "instance": b, "ticks": 1 }))
            .unwrap_err()
            .contains("pause")
    );
    agent.tool("pause", json!({ "instance": b })).unwrap();

    // A second agent, read-only: sees the games, can't change them.
    let mut viewer = Agent::new(&dir, true);
    let names = viewer.tool_names();
    assert!(names.contains(&"list_entities".to_string()));
    assert!(names.contains(&"instance_capabilities".to_string()));
    let caps = viewer
        .tool("instance_capabilities", json!({ "instance": port_a }))
        .unwrap();
    assert_eq!(caps["mcp"]["read_only"], true);
    let spawn = caps["mcp"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "spawn_entity")
        .unwrap();
    assert_eq!(spawn["available"], false);
    assert!(spawn["reason"].as_str().unwrap().contains("read-only"));
    assert!(!names.iter().any(|n| n == "spawn_entity"
        || n == "launch"
        || n == "run_ticks"
        || n == "watch"
        || n == "run_input_scenario"));
    let listed = viewer
        .tool("list_entities", json!({ "instance": port_a, "with": ["Health"] }))
        .unwrap();
    assert_eq!(listed["total"], 1);
    let err = viewer
        .tool("despawn_entity", json!({ "instance": port_a, "entity": e }))
        .unwrap_err();
    assert!(err.contains("read-only"), "{err}");
    assert!(viewer.tool("list_resources", json!({ "instance": port_a })).is_ok());
    let err = viewer
        .tool(
            "set_resource",
            json!({ "instance": port_a, "resource": "Rules", "value": { "round": 0 } }),
        )
        .unwrap_err();
    assert!(err.contains("read-only"), "{err}");

    // Graphics quality: the tool follows what the game reports (this crate
    // can't see the engine's features), and a read-only server hides it.
    let tools = agent
        .tool("instance_capabilities", json!({ "instance": port_a }))
        .unwrap();
    let quality = tools["mcp"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "set_graphics_quality")
        .cloned()
        .unwrap();
    let set = agent.tool("set_graphics_quality", json!({ "instance": port_a, "level": "medium" }));
    if quality["available"] == true {
        let set = set.unwrap();
        assert_eq!(set["quality"]["level"], "medium", "{set}");
        let perf = agent
            .tool("perf_report", json!({ "instance": port_a, "history": 4 }))
            .unwrap();
        assert_eq!(perf["render_inspector"]["quality"], set["quality"]);
        assert_eq!(
            perf["render_inspector"]["history"],
            json!([]),
            "headless: nothing drawn"
        );
    } else {
        let reason = quality["reason"].as_str().unwrap();
        assert!(reason.contains("3d"), "{quality}");
        assert!(set.unwrap_err().contains(reason));
    }
    // perf_report passes gpu_allocations on: a headless game says it has
    // no GPU to list instead of "not asked for".
    let perf = agent
        .tool("perf_report", json!({ "instance": port_a, "gpu_allocations": true }))
        .unwrap();
    let why = perf["render_inspector"]["unavailable"]["gpu_allocations"]
        .as_str()
        .unwrap();
    assert!(why.contains("headless"), "{why}");
    assert!(!names.iter().any(|n| n == "set_graphics_quality"));
    let err = viewer
        .tool("set_graphics_quality", json!({ "instance": port_a, "level": "low" }))
        .unwrap_err();
    assert!(err.contains("read-only"), "{err}");

    // Errors from the game come through as tool errors.
    let err = agent
        .tool("get_entity", json!({ "instance": "alpha", "entity": 123_456 }))
        .unwrap_err();
    assert!(err.contains("does not exist"), "{err}");
    assert!(
        agent
            .tool("instance_info", json!({ "instance": "gamma" }))
            .unwrap_err()
            .contains("no running instance")
    );
    // Not started with `launch`, and its output is still there.
    let logs = agent.tool("read_logs", json!({ "instance": "alpha", "lines": 5000 }));
    if !cfg!(any(unix, windows)) {
        assert!(logs.unwrap_err().contains("captured"), "says why there are no logs");
    } else {
        let logs = logs.unwrap();
        // (The test harness keeps `eprintln!` itself, so the text may be empty.)
        assert!(logs["text"].is_string(), "{logs}");
        let next = logs["next"].as_u64().unwrap();
        let none = agent
            .tool("read_logs", json!({ "instance": "alpha", "since": next }))
            .unwrap();
        assert!(none["next"].as_u64().unwrap() >= next);
    }

    // Checkpoint, experiment, roll back.
    let checkpoint = dir.join("checkpoint.json");
    let saved = agent
        .tool("save_world", json!({ "instance": "alpha", "path": "checkpoint.json" }))
        .unwrap();
    assert_eq!(saved["entities"], 1);
    assert_eq!(saved["resources"], 1);
    // A second save to the same file is refused unless it may overwrite.
    let err = agent
        .tool("save_world", json!({ "instance": "alpha", "path": "checkpoint.json" }))
        .unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    agent
        .tool(
            "save_world",
            json!({ "instance": "alpha", "path": "checkpoint.json", "overwrite": true }),
        )
        .unwrap();
    agent
        .tool("despawn_entity", json!({ "instance": "alpha", "entity": e }))
        .unwrap();
    assert_eq!(
        agent.tool("world_summary", json!({ "instance": "alpha" })).unwrap()["entities"],
        0
    );
    // The checkpoint sits in the registry directory on purpose: discovery
    // must never delete files that aren't registry entries.
    agent.tool("list_instances", json!({})).unwrap();
    assert!(checkpoint.exists(), "discovery deleted a file it doesn't own");
    // Only paths below the game's folder are accepted.
    let err = agent
        .tool("load_world", json!({ "instance": "alpha", "path": checkpoint }))
        .unwrap_err();
    assert!(err.contains("relative"), "{err}");
    agent
        .tool("load_world", json!({ "instance": "alpha", "path": "checkpoint.json" }))
        .unwrap();
    assert_eq!(
        agent
            .tool("get_entity", json!({ "instance": "alpha", "entity": e }))
            .unwrap()["Health"]["current"],
        6.0,
        "the same entity id is back with its saved values"
    );
    // The read-only agent may save a new file (it changes nothing), not
    // replace one, and not load.
    assert!(
        viewer
            .tool(
                "save_world",
                json!({ "instance": "alpha", "path": "checkpoint.json", "overwrite": true })
            )
            .unwrap_err()
            .contains("read-only")
    );
    assert!(
        viewer
            .tool("save_world", json!({ "instance": "alpha", "path": "viewer.json" }))
            .is_ok()
    );
    assert!(
        viewer
            .tool("load_world", json!({ "instance": "alpha", "path": "checkpoint.json" }))
            .unwrap_err()
            .contains("read-only")
    );

    // Events: what happened while ticks ran.
    let kinds = agent.tool("list_events", json!({ "instance": "alpha" })).unwrap();
    assert_eq!(kinds[0]["name"], "Regen");
    viewer
        .tool("subscribe_events", json!({ "instance": "alpha", "types": ["Regen"] }))
        .unwrap();
    let start = agent.tool("instance_info", json!({ "instance": "alpha" })).unwrap()["tick"]
        .as_u64()
        .unwrap();
    agent
        .tool("run_ticks", json!({ "instance": "alpha", "ticks": 3 }))
        .unwrap();
    let got = viewer.tool("take_events", json!({ "instance": "alpha" })).unwrap();
    let ticks: Vec<u64> = (got["events"].as_array().unwrap().iter())
        .map(|e| e["data"]["tick"].as_u64().unwrap())
        .collect();
    assert_eq!(ticks, [start, start + 1, start + 2]);

    // Input scenarios: refused unless the game allows injection, then run to
    // a report; a failed check names its step and tick and releases the input.
    let (_, plain) = start_game_with("plain", false);
    let scenario = json!({ "version": 1, "steps": [{ "tick": 0, "type": "key", "key": "Space", "down": true }] });
    let refused = agent
        .tool(
            "run_input_scenario",
            json!({ "instance": "plain", "scenario": scenario }),
        )
        .unwrap_err();
    assert!(refused.contains("input injection is off"), "{refused}");
    agent.tool("quit_instance", json!({ "instance": "plain" })).unwrap();
    plain.join().unwrap();

    let (_, pad) = start_game_with("pad", true);
    let passing = json!({
        "version": 1,
        "id": "hold-then-release",
        "steps": [
            { "tick": 1, "type": "key", "key": "Space", "down": true },
            { "tick": 1, "type": "expect_resource", "resource": "Rules", "pointer": "/round", "equals": 2 },
            { "tick": 3, "type": "key", "key": "Space", "down": false },
            { "tick": 3, "type": "expect_action", "action": "jump", "released": true }
        ]
    });
    let run = agent
        .tool("run_input_scenario", json!({ "instance": "pad", "scenario": passing }))
        .unwrap();
    assert_eq!(run["passed"], true, "{run}");
    assert_eq!(
        (
            run["report"]["ticks_run"].clone(),
            run["report"]["checks_passed"].clone()
        ),
        (json!(4), json!(2))
    );
    let failing = json!({
        "version": 1,
        "duration": 500,
        "steps": [
            { "tick": 0, "type": "key", "key": "Space", "down": true },
            { "tick": 2, "type": "expect_resource", "resource": "Rules", "pointer": "/round", "equals": 99 }
        ]
    });
    let run = agent
        .tool("run_input_scenario", json!({ "instance": "pad", "scenario": failing }))
        .unwrap();
    assert_eq!(run["passed"], false);
    let failure = &run["report"]["failure"];
    assert_eq!((failure["step"].clone(), failure["tick"].clone()), (json!(1), json!(2)));
    assert_eq!(
        (failure["expected"].clone(), failure["actual"].clone()),
        (json!(99), json!(7))
    );
    assert_eq!(run["report"]["released"], 1, "the failed run let go of Space");
    let info = agent.tool("instance_info", json!({ "instance": "pad" })).unwrap();
    assert_eq!(
        info["tick"],
        4 + 3,
        "the run stopped at the failing tick, not at its 500 ticks"
    );
    let too_long = agent
        .tool(
            "run_input_scenario",
            json!({ "instance": "pad", "scenario": failing, "max_ticks": 100 }),
        )
        .unwrap_err();
    assert!(too_long.contains("more than `max_ticks`"), "{too_long}");
    // The refused (too long) scenario never touched the running state, and a
    // huge timeout is clamped instead of crashing the server.
    let hold =
        json!({ "version": 1, "duration": 2, "steps": [{ "tick": 0, "type": "key", "key": "Space", "down": true }] });
    let run = agent
        .tool(
            "run_input_scenario",
            json!({ "instance": "pad", "scenario": hold, "timeout_secs": u64::MAX }),
        )
        .unwrap();
    assert_eq!(run["passed"], true);
    assert_eq!(
        (run["report"]["released"].clone(), run["report"]["still_held"].clone()),
        (json!(1), json!(0)),
        "a finished run lets go of its keys"
    );
    agent
        .tool(
            "run_ticks",
            json!({ "instance": "pad", "ticks": 1, "timeout_secs": u64::MAX }),
        )
        .unwrap();
    // Opting out keeps the key held and owned: cancelling releases it.
    let kept = agent
        .tool(
            "run_input_scenario",
            json!({ "instance": "pad", "scenario": hold, "keep_input_when_done": true }),
        )
        .unwrap();
    assert_eq!(
        (kept["report"]["released"].clone(), kept["report"]["still_held"].clone()),
        (json!(0), json!(1))
    );
    let cancelled = agent
        .tool("cancel_input_scenario", json!({ "instance": "pad" }))
        .unwrap();
    assert_eq!(cancelled["cancelled"], true, "it releases what the finished run kept");
    assert_eq!(cancelled["report"]["released"], 1);
    let cancelled = agent
        .tool("cancel_input_scenario", json!({ "instance": "pad" }))
        .unwrap();
    assert_eq!(cancelled["cancelled"], false, "nothing left");
    assert!(
        viewer
            .tool("run_input_scenario", json!({ "instance": "pad", "scenario": passing }))
            .unwrap_err()
            .contains("read-only")
    );
    agent.tool("quit_instance", json!({ "instance": "pad" })).unwrap();
    pad.join().unwrap();

    console_tools_share_headless_game_state_and_enforce_read_only(&mut agent, &dir);
    agent.tool("quit_instance", json!({ "instance": "alpha" })).unwrap();
    agent.tool("quit_instance", json!({ "instance": b })).unwrap();
    game_a.join().unwrap();
    game_b.join().unwrap();
    assert_eq!(agent.tool("list_instances", json!({})).unwrap(), json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}

fn console_tools_share_headless_game_state_and_enforce_read_only(agent: &mut Agent, dir: &Path) {
    let mut readonly = Agent::new(dir, true);
    let instance = "alpha";
    let list = readonly.tool("console_help", json!({ "instance": instance })).unwrap();
    assert!(list["help"].as_str().unwrap().contains("bump_round <by:integer>"));
    assert!(list["help"].as_str().unwrap().contains("round_scale [value:float]"));
    assert!(!readonly.tool_names().contains(&"run_console".to_string()));
    let refused = readonly
        .tool("run_console", json!({ "instance": instance, "line": "bump_round 2" }))
        .unwrap_err();
    assert!(refused.contains("read-only"));
    let bad = agent
        .tool(
            "run_console",
            json!({ "instance": instance, "line": "bump_round several" }),
        )
        .unwrap_err();
    assert!(bad.contains("by") && bad.contains("integer"));
    let before = agent
        .tool("get_resource", json!({"instance": instance, "resource": "Rules"}))
        .unwrap()["round"]
        .as_u64()
        .unwrap();
    let result = agent
        .tool("run_console", json!({ "instance": instance, "line": "bump_round 3" }))
        .unwrap();
    assert_eq!(result["output"], (before + 3).to_string());
    let after = agent
        .tool("get_resource", json!({"instance": instance, "resource": "Rules"}))
        .unwrap();
    assert_eq!(after["round"], before + 3);
    let result = agent
        .tool(
            "run_console",
            json!({ "instance": instance, "line": "round_scale 1.5" }),
        )
        .unwrap();
    assert_eq!(result["output"], "1.5");
    let result = agent
        .tool("run_console", json!({ "instance": instance, "line": "round_scale" }))
        .unwrap();
    assert_eq!(result["output"], "1.5");
}
