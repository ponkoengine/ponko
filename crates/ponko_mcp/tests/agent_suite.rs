//! The agent acceptance suite in its deterministic mode: every task's
//! scripted solution is replayed through the MCP tools against a fresh copy
//! of its starting project and must score full marks, and every negative
//! control (a plausible wrong fix) must fail exactly the checks it names.
//! No model is involved and nothing leaves this machine.
//!
//! The scores must match `tests/suite/baseline.json`, the published
//! baseline. After a deliberate change to a task, its checks or the tools,
//! rewrite it with `PONKO_UPDATE_BASELINE=1 cargo test -p ponko_mcp --test
//! agent_suite` and explain the change in the pull request.

mod suite;

use serde_json::{Value, json};
use std::path::Path;

const BASELINE: &str = include_str!("suite/baseline.json");

// One test function: the tasks' games run in this process one after
// another, each in the process's working folder, and the environment is
// set before any of them starts.
#[test]
fn every_task_is_solvable_and_its_checker_catches_wrong_fixes() {
    let root = std::env::temp_dir().join(format!("ponko-agent-suite-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let instances = root.join("instances");
    std::fs::create_dir_all(&instances).unwrap();
    // SAFETY: the first thing this test binary does; no thread runs yet.
    unsafe { suite::prepare_process(&instances) };

    tasks_are_well_formed();
    let tasks: Vec<_> = suite::TASKS.iter().collect();
    let reports = suite::replay_all(&tasks, &root, &instances);
    println!("{}", suite::table(&reports));

    for r in &reports {
        let s = &r.solution;
        assert!(
            s.solved() && s.stuck.is_none(),
            "{}: the scripted solution should solve the task; stuck: {:?}; checks: {:#?}",
            r.task.id,
            s.stuck,
            s.checks
        );
        assert!(!r.controls.is_empty(), "{}: needs a negative control", r.task.id);
        assert!(
            r.controls.iter().any(|(script, _)| script.steps.is_empty()),
            "{}: needs a do-nothing control, so the starting state fails",
            r.task.id
        );
        for (script, outcome) in &r.controls {
            assert!(
                !outcome.solved(),
                "{}: control `{}` passed every check",
                r.task.id,
                script.name
            );
            assert_eq!(
                outcome.failed(),
                script.fails,
                "{}: control `{}` ({}) should fail exactly these checks; checks: {:#?}",
                r.task.id,
                script.name,
                script.why,
                outcome.checks
            );
        }
    }

    let now = suite::baseline(&reports);
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/suite/baseline.json");
    if std::env::var_os("PONKO_UPDATE_BASELINE").is_some() {
        std::fs::write(&file, serde_json::to_string_pretty(&now).unwrap() + "\n").unwrap();
    } else {
        let published: serde_json::Value = serde_json::from_str(BASELINE).unwrap();
        assert_eq!(
            now, published,
            "the scores differ from the published baseline (tests/suite/baseline.json); \
             if the change is deliberate, rerun with PONKO_UPDATE_BASELINE=1"
        );
    }
    the_mcp_proxy_serves_a_task_and_scores_submissions(&root, &instances);
    a_folder_that_is_not_the_suites_is_never_cleared(&root);
    std::env::set_current_dir(std::env::temp_dir()).unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

/// One JSON-RPC request to the proxy a real agent talks to; its reply.
fn rpc(session: &mut suite::Session, record: &mut suite::proxy::Record, method: &str, params: Value) -> Value {
    let message = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params });
    let reply = suite::proxy::answer(session, record, &message).expect("a request gets a reply");
    assert_eq!(reply["id"], 7, "{reply}");
    reply["result"].clone()
}

/// What `agent_suite mcp` serves a real agent: the goal as instructions,
/// ponko-mcp's tools without `launch`/`quit_instance`, plus `submit_task`,
/// which scores the task as it is when called; every call is recorded.
fn the_mcp_proxy_serves_a_task_and_scores_submissions(root: &Path, instances: &Path) {
    let task = suite::task("tune-speed").unwrap();
    let dir = root.join("proxy");
    let mut session = suite::Session::in_process(task, &dir, instances).unwrap();
    let mut record = suite::proxy::Record::new(task, Some("test".into()), &dir);
    let s = &mut session;
    let r = &mut record;

    let init = rpc(s, r, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    let instructions = init["instructions"].as_str().unwrap();
    assert!(instructions.contains(task.goal) && instructions.contains(&dir.display().to_string()));
    let tools = rpc(s, r, "tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"submit_task") && names.contains(&"get_resource"),
        "{names:?}"
    );
    assert!(
        !names.contains(&"launch") && !names.contains(&"quit_instance"),
        "{names:?}"
    );

    let refused = rpc(s, r, "tools/call", json!({ "name": "quit_instance", "arguments": {} }));
    assert_eq!(refused["isError"], true);
    let speed = rpc(
        s,
        r,
        "tools/call",
        json!({ "name": "get_resource", "arguments": { "resource": "Settings" } }),
    );
    assert_eq!(suite::tool_result(&speed).unwrap()["speed"], 120.0);

    let submit = |s: &mut suite::Session, r: &mut suite::proxy::Record| {
        let result = rpc(s, r, "tools/call", json!({ "name": "submit_task", "arguments": {} }));
        suite::tool_result(&result).unwrap()
    };
    let first = submit(s, r);
    assert_eq!(
        (first["solved"].clone(), first["total"].clone()),
        (json!(false), json!(3)),
        "{first}"
    );
    std::fs::write(dir.join("assets/settings.json"), "{ \"speed\": 350 }\n").unwrap();
    let second = submit(s, r);
    assert_eq!(second["solved"], true, "{second}");

    // Every tool a task leans on is one of the server's.
    for t in suite::TASKS {
        for tool in t.leans_on {
            assert!(
                names.contains(tool) || suite::proxy::HIDDEN.contains(tool),
                "{}: leans on `{tool}`, which isn't a tool",
                t.id
            );
        }
    }

    // `--hide`: a poorer tool surface, for comparing runs.
    let mut poorer = suite::proxy::Record::new(task, None, &dir);
    poorer.hide(&["watch".to_string()]);
    let p = &mut poorer;
    let init = rpc(s, p, "initialize", json!({ "protocolVersion": "2025-06-18" }));
    assert!(init["instructions"].as_str().unwrap().contains("`watch`"), "{init}");
    let tools = rpc(s, p, "tools/list", json!({}));
    assert!(
        !tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == "watch"),
        "{tools}"
    );
    let refused = rpc(
        s,
        p,
        "tools/call",
        json!({ "name": "watch", "arguments": { "ticks": 1 } }),
    );
    assert_eq!(refused["isError"], true, "{refused}");
    assert_eq!(
        poorer.json()["hidden_tools"],
        json!(["launch", "quit_instance", "watch"])
    );

    let result = record.json();
    assert_eq!(
        (
            &result["solved"],
            &result["tool_calls"],
            &result["tool_errors"],
            &result["submissions"]
        ),
        (&json!(true), &json!(2), &json!(1), &json!(2)),
        "{result}"
    );
    assert_eq!(result["calls"][0]["tool"], "quit_instance");
    // Wall time: when each call came, and when the last submission did.
    assert!(result["calls"][0]["at"].is_number(), "{result}");
    assert!(result["seconds"].as_f64().is_some_and(|s| s >= 0.0), "{result}");
    session.finish();
}

/// Every task has a goal, starting files, uniquely named checks, and
/// controls that name only its checks.
fn tasks_are_well_formed() {
    let mut ids = std::collections::HashSet::new();
    for task in suite::TASKS {
        assert!(ids.insert(task.id), "{}: two tasks with this id", task.id);
        assert!(
            task.goal.contains("Goal:"),
            "{}: the goal says what to do after `Goal:`",
            task.id
        );
        assert!(!task.files.is_empty(), "{}: starts from no files", task.id);
        assert!(!task.leans_on.is_empty(), "{}: says which tools it leans on", task.id);
        let names: std::collections::HashSet<_> = task.checks.iter().collect();
        assert_eq!(names.len(), task.checks.len(), "{}: two checks share a name", task.id);
        for control in task.controls() {
            for name in &control.fails {
                assert!(
                    task.checks.contains(&name.as_str()),
                    "{}: control `{}` names an unknown check `{name}`",
                    task.id,
                    control.name
                );
            }
        }
    }
}

/// `--dir` may name any folder: the suite clears only its own.
fn a_folder_that_is_not_the_suites_is_never_cleared(root: &Path) {
    let task = &suite::TASKS[0];
    let theirs = root.join("someone-elses");
    std::fs::create_dir_all(&theirs).unwrap();
    std::fs::write(theirs.join("notes.txt"), "keep me").unwrap();
    let err = suite::prepare(task, &theirs).unwrap_err();
    assert!(err.contains("not a folder of this suite"), "{err}");
    assert_eq!(std::fs::read_to_string(theirs.join("notes.txt")).unwrap(), "keep me");
    // An empty one, and then the suite's own, are fine.
    let ours = root.join("ours");
    std::fs::create_dir_all(&ours).unwrap();
    suite::prepare(task, &ours).unwrap();
    suite::prepare(task, &ours).unwrap();
}
