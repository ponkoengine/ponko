//! The agent acceptance suite: game-development tasks done through the MCP
//! tools, each with a starting project, a goal and a checker that scores
//! the result from the game's state and files.
//!
//! - `tests/agent_suite.rs` replays every task's scripted solution and its
//!   negative controls (scripts that must fail), with no model involved,
//!   and compares the scores with `baseline.json`.
//! - `examples/agent_suite.rs` prints the same scores (`replay`), and hosts
//!   one task for a real agent as an MCP server (`mcp`), which adds a
//!   `submit_task` tool that runs the checker.
//!
//! The book's "Working with AI agents" chapter has the commands.

#![allow(dead_code)] // the test and the example each use part of it

pub mod games;
pub mod proxy;
pub mod tasks;

use ponko_mcp::{Config, Server};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub use tasks::{TASKS, Task};

/// Bumped when tasks or scoring change in a way that makes older results
/// incomparable.
pub const SUITE_VERSION: u32 = 1;

// ------------------------------------------------------------------ tools

/// An MCP client: one `ponko_mcp::Server`, called the way an agent's client
/// calls it.
pub struct Tools {
    server: Server,
    next: u64,
}

impl Tools {
    /// A client for the games registered in `instances`.
    pub fn new(instances: &Path) -> Self {
        let mut tools = Self {
            server: Server::new(Config {
                instances_dir: instances.to_path_buf(),
                logs_dir: instances.join("logs"),
                read_only: false,
            }),
            next: 0,
        };
        tools.rpc("initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "agent-suite", "version": "1" } }));
        tools
    }

    /// One JSON-RPC message; the reply, if it gets one.
    pub fn handle(&mut self, message: &Value) -> Option<Value> {
        self.server.handle(message)
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        self.server
            .handle(&json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }))
            .unwrap_or(Value::Null)
    }

    /// Calls a tool: its JSON answer, or the error text it returned.
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        let reply = self.rpc("tools/call", json!({ "name": name, "arguments": args }));
        if let Some(error) = reply.get("error") {
            return Err(error["message"].as_str().unwrap_or("JSON-RPC error").to_string());
        }
        tool_result(&reply["result"])
    }
}

/// A `tools/call` result as JSON, or its error text.
pub fn tool_result(result: &Value) -> Result<Value, String> {
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    if result["isError"] == true {
        Err(text.to_string())
    } else {
        Ok(serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string())))
    }
}

// ------------------------------------------------------------------ checks

/// One condition a task's result must meet.
#[derive(Clone, Debug)]
pub struct Check {
    /// Its name, as listed in the task and the baseline.
    pub name: &'static str,
    /// Whether it held.
    pub passed: bool,
    /// What was seen.
    pub detail: String,
}

impl Check {
    /// A check that held when `result` is `Ok`; its error explains the failure.
    pub fn from(name: &'static str, result: Result<String, String>) -> Self {
        match result {
            Ok(detail) => Check {
                name,
                passed: true,
                detail,
            },
            Err(detail) => Check {
                name,
                passed: false,
                detail,
            },
        }
    }
}

/// What a checker sees: the game, through the tools, and the task's folder.
pub struct Probe<'a> {
    /// The checker's own MCP client.
    pub tools: &'a mut Tools,
    /// The task's folder (the game's working folder).
    pub dir: &'a Path,
}

impl Probe<'_> {
    /// Calls a tool; an error becomes the check's failure text.
    pub fn call(&mut self, name: &str, args: Value) -> Result<Value, String> {
        self.tools.call(name, args).map_err(|e| format!("{name} failed: {e}"))
    }

    /// A JSON file in the task's folder.
    pub fn json_file(&self, path: &str) -> Result<Value, String> {
        let text = std::fs::read_to_string(self.dir.join(path)).map_err(|e| format!("can't read {path}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{path} is not valid JSON: {e}"))
    }

    /// A text file in the task's folder.
    pub fn text_file(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.dir.join(path)).map_err(|e| format!("can't read {path}: {e}"))
    }

    /// A reflected resource of the running game.
    pub fn resource(&mut self, name: &str) -> Result<Value, String> {
        self.call("get_resource", json!({ "resource": name }))
    }

    /// How many entities have every component in `with`.
    pub fn count(&mut self, with: &[&str]) -> Result<u64, String> {
        let r = self.call("list_entities", json!({ "with": with, "limit": 0 }))?;
        r["total"]
            .as_u64()
            .ok_or_else(|| format!("list_entities gave no total: {r}"))
    }

    /// Runs an input scenario; `Ok` when its checks passed, else what failed.
    pub fn scenario(&mut self, scenario: Value) -> Result<Value, String> {
        let r = self.call("run_input_scenario", json!({ "scenario": scenario }))?;
        if r["passed"] == true {
            Ok(r)
        } else {
            Err(format!("scenario failed: {}", r["report"]["failure"]))
        }
    }
}

// ------------------------------------------------------------------ host

/// Where a task's game runs.
enum Host {
    /// A thread of this process (replays: the game shares its working folder).
    Thread(JoinHandle<()>),
    /// A child process (`agent_suite host`), so its output can't reach an
    /// MCP client reading this process's stdout.
    Process(Child),
}

/// A running task: its folder, its game and the tools that reach it.
pub struct Session {
    /// The task.
    pub task: &'static Task,
    /// Its folder: the game's working folder, with the starting files.
    pub dir: PathBuf,
    host: Option<Host>,
    /// The agent's client.
    pub agent: Tools,
    /// The checker's client.
    checker: Tools,
}

/// The file that marks a folder as the suite's own, so a later run may
/// clear it.
const MARKER: &str = ".ponko-agent-suite";

/// Writes `task`'s starting files into `dir`. A folder that is already there
/// is cleared first only when it is empty or an earlier run's (it has the
/// suite's marker file); any other folder is refused, so a mistyped `--dir`
/// can't delete someone's files.
pub fn prepare(task: &Task, dir: &Path) -> Result<(), String> {
    if dir.exists() {
        let empty = std::fs::read_dir(dir).map_err(|e| e.to_string())?.next().is_none();
        if !empty && !dir.join(MARKER).is_file() {
            return Err(format!(
                "{} is not empty and not a folder of this suite; name a new folder",
                dir.display()
            ));
        }
        std::fs::remove_dir_all(dir).map_err(|e| format!("can't clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir.join("assets")).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(MARKER), task.id).map_err(|e| e.to_string())?;
    for (path, text) in task.files {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&file, text).map_err(|e| format!("can't write {}: {e}", file.display()))?;
    }
    Ok(())
}

/// Runs `task`'s game on this thread until a tool asks it to quit. The
/// working folder must be the task's folder; `ready` gets its port.
pub fn run_game(task: &Task, ready: Option<mpsc::Sender<u16>>) {
    let (game, setup) = (task.game)();
    game.control_endpoint(true)
        .hot_reload(true)
        .input_injection(true)
        .run_headless(u64::MAX, move |ctx| {
            setup(ctx);
            ctx.time.pause();
            if let Some(ready) = ready {
                let _ = ready.send(ctx.control_addr().map_or(0, |a| a.port()));
            }
        });
}

impl Session {
    /// Prepares `dir` and starts the game on a thread of this process. The
    /// process's working folder becomes `dir`, and `PONKO_INSTANCES_DIR`
    /// must already be `instances`: run one session at a time.
    pub fn in_process(task: &'static Task, dir: &Path, instances: &Path) -> Result<Self, String> {
        // Out of the folder first: Windows can't delete the working folder.
        std::env::set_current_dir(std::env::temp_dir()).map_err(|e| e.to_string())?;
        prepare(task, dir)?;
        std::env::set_current_dir(dir).map_err(|e| e.to_string())?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || run_game(task, Some(tx)));
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| format!("{}: the game didn't start", task.id))?;
        Ok(Self::with_host(task, dir, instances, Host::Thread(thread)))
    }

    /// Prepares `dir` and starts the game as `program host <task>` in it,
    /// registering in `instances`; its output goes to `dir/game.log`.
    pub fn in_child(task: &'static Task, dir: &Path, instances: &Path, program: &Path) -> Result<Self, String> {
        prepare(task, dir)?;
        std::fs::create_dir_all(instances).map_err(|e| e.to_string())?;
        let log = std::fs::File::create(dir.join("game.log")).map_err(|e| e.to_string())?;
        let child = std::process::Command::new(program)
            .args(["host", task.id])
            .current_dir(dir)
            .env("PONKO_INSTANCES_DIR", instances)
            .env_remove("CARGO_MANIFEST_DIR")
            .stdin(std::process::Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .spawn()
            .map_err(|e| format!("can't start {}: {e}", program.display()))?;
        let mut session = Self::with_host(task, dir, instances, Host::Process(child));
        let deadline = Instant::now() + Duration::from_secs(60);
        while session.checker.call("instance_info", json!({})).is_err() {
            if Instant::now() > deadline {
                return Err(format!(
                    "{}: the game didn't start; see {}",
                    task.id,
                    dir.join("game.log").display()
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(session)
    }

    fn with_host(task: &'static Task, dir: &Path, instances: &Path, host: Host) -> Self {
        Session {
            task,
            dir: dir.to_path_buf(),
            host: Some(host),
            agent: Tools::new(instances),
            checker: Tools::new(instances),
        }
    }

    /// Scores the game as it is now. First lets a file the agent saved last
    /// reach the game (hot reload checks files a few times a second) with
    /// one tick, then runs the task's checker.
    pub fn check(&mut self) -> Vec<Check> {
        std::thread::sleep(Duration::from_millis(350));
        if let Err(e) = self.checker.call("run_ticks", json!({ "ticks": 1 })) {
            return self
                .task
                .checks
                .iter()
                .map(|name| Check {
                    name,
                    passed: false,
                    detail: format!("the game doesn't answer: {e}"),
                })
                .collect();
        }
        let mut probe = Probe {
            tools: &mut self.checker,
            dir: &self.dir,
        };
        let checks = (self.task.check)(&mut probe);
        debug_assert_eq!(
            checks.iter().map(|c| c.name).collect::<Vec<_>>(),
            self.task.checks,
            "{}: the checker's checks differ from the task's list",
            self.task.id
        );
        checks
    }

    /// Asks the game to quit and waits for it.
    pub fn finish(mut self) {
        let _ = self.checker.call("quit_instance", json!({}));
        match self.host.take() {
            Some(Host::Thread(thread)) => {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !thread.is_finished() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if thread.is_finished() {
                    let _ = thread.join();
                }
            }
            Some(Host::Process(mut child)) => {
                let deadline = Instant::now() + Duration::from_secs(10);
                while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            None => {}
        }
    }
}

// ------------------------------------------------------------------ scripts

/// A scripted run of a task: what an agent would do, step by step.
#[derive(Clone, Debug)]
pub struct Script {
    /// `solution`, or the negative control's name.
    pub name: String,
    /// Why a control is wrong (empty for the solution).
    pub why: String,
    /// The steps (see [`run_step`]).
    pub steps: Vec<Value>,
    /// The checks a negative control must fail, and only those.
    pub fails: Vec<String>,
}

impl Script {
    fn parse(name: &str, value: &Value) -> Result<Self, String> {
        let steps = value["steps"]
            .as_array()
            .ok_or(format!("{name}: no `steps` array"))?
            .clone();
        let fails = value["fails"]
            .as_array()
            .map(|f| f.iter().filter_map(|s| s.as_str().map(String::from)).collect())
            .unwrap_or_default();
        Ok(Script {
            name: value["name"].as_str().unwrap_or(name).to_string(),
            why: value["why"].as_str().unwrap_or_default().to_string(),
            steps,
            fails,
        })
    }
}

impl Task {
    /// The scripted solution.
    pub fn solution(&self) -> Script {
        let value: Value = serde_json::from_str(self.solution).expect("solution.json is JSON");
        Script::parse("solution", &value).unwrap()
    }

    /// The negative controls: runs that look like an attempt and must fail.
    pub fn controls(&self) -> Vec<Script> {
        let value: Value = serde_json::from_str(self.controls).expect("controls.json is JSON");
        value
            .as_array()
            .expect("controls.json is an array")
            .iter()
            .map(|c| Script::parse("control", c).unwrap())
            .collect()
    }
}

/// Replaces `"$name"` strings in `value` with saved values.
fn substitute(value: &Value, vars: &serde_json::Map<String, Value>) -> Value {
    match value {
        Value::String(s) if s.starts_with('$') => vars.get(&s[1..]).cloned().unwrap_or_else(|| value.clone()),
        Value::Array(a) => Value::Array(a.iter().map(|v| substitute(v, vars)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), substitute(v, vars))).collect()),
        other => other.clone(),
    }
}

/// Whether two JSON values are equal, numbers within 1e-4.
fn same(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => (x - y).abs() <= 1e-4,
        _ => a == b,
    }
}

/// What running a script did.
#[derive(Default)]
struct Ran {
    tool_calls: usize,
    tool_errors: usize,
    /// The step that went wrong, and why.
    stuck: Option<String>,
}

/// One step of a script:
/// - `{"call": tool, "args": {...}, "save": {"var": "/json/pointer"}}`: a
///   tool call; `"$var"` in later arguments is the saved value. A tool
///   error stops the script, unless the step says `"error": true`.
/// - `{"write": "assets/file", "text": "..."}`: the agent edits a file in
///   the task's folder (an agent's own file tools, not MCP).
/// - `{"until": {"call", "args", "pointer", "equals"}, "each": {"call",
///   "args"}, "timeout_secs": 10}`: repeats `each` then `until`'s call
///   until the value at `pointer` equals `equals` (waiting for a hot reload).
fn run_step(
    step: &Value,
    dir: &Path,
    tools: &mut Tools,
    vars: &mut serde_json::Map<String, Value>,
    ran: &mut Ran,
) -> Result<(), String> {
    let mut call = |c: &Value, ran: &mut Ran| -> Result<Value, String> {
        let name = c["call"].as_str().ok_or(format!("no `call` in {c}"))?;
        let args = substitute(c.get("args").unwrap_or(&json!({})), vars);
        ran.tool_calls += 1;
        let r = tools.call(name, args);
        if r.is_err() {
            ran.tool_errors += 1;
        }
        r.map_err(|e| format!("{name}: {e}"))
    };
    if step.get("call").is_some() {
        match call(step, ran) {
            Ok(r) => {
                if let Some(save) = step["save"].as_object() {
                    for (var, pointer) in save {
                        let at = r
                            .pointer(pointer.as_str().unwrap_or_default())
                            .ok_or(format!("nothing at {pointer} in {r}"))?;
                        vars.insert(var.clone(), at.clone());
                    }
                }
                Ok(())
            }
            Err(_) if step["error"] == true => Ok(()),
            Err(e) => Err(e),
        }
    } else if let Some(path) = step["write"].as_str() {
        let text = step["text"].as_str().ok_or("`write` needs `text`")?;
        let file = dir.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&file, text).map_err(|e| format!("can't write {path}: {e}"))
    } else if let Some(until) = step.get("until") {
        // A hang detector: the step ends as soon as its condition holds.
        let deadline = Instant::now() + Duration::from_secs(step["timeout_secs"].as_u64().unwrap_or(10));
        loop {
            if let Some(each) = step.get("each") {
                call(each, ran)?;
            }
            let r = call(until, ran)?;
            let pointer = until["pointer"].as_str().unwrap_or_default();
            let seen = r.pointer(pointer).cloned().unwrap_or(Value::Null);
            if same(&seen, &until["equals"]) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("{pointer} stayed {seen}, wanted {}", until["equals"]));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    } else {
        Err(format!("unknown step {step}"))
    }
}

/// The result of one scripted run of a task.
#[derive(Clone, Debug)]
pub struct Outcome {
    /// The script's name.
    pub run: String,
    /// The checker's verdict.
    pub checks: Vec<Check>,
    /// Steps in the script (a wait counts as one).
    pub steps: usize,
    /// Tool calls made (waits make as many as they need).
    pub tool_calls: usize,
    /// Tool calls that returned an error.
    pub tool_errors: usize,
    /// Where the script got stuck, if it did.
    pub stuck: Option<String>,
}

impl Outcome {
    /// Checks that held.
    pub fn passed(&self) -> usize {
        self.checks.iter().filter(|c| c.passed).count()
    }

    /// Every check held.
    pub fn solved(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }

    /// The names of the checks that failed.
    pub fn failed(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.to_string())
            .collect()
    }
}

/// Plays `script` against a fresh copy of `task` in `dir`, then scores it.
pub fn replay(task: &'static Task, script: &Script, dir: &Path, instances: &Path) -> Outcome {
    let mut session = match Session::in_process(task, dir, instances) {
        Ok(s) => s,
        Err(e) => panic!("{e}"),
    };
    let mut ran = Ran::default();
    let mut vars = serde_json::Map::new();
    for (i, step) in script.steps.iter().enumerate() {
        if let Err(e) = run_step(step, dir, &mut session.agent, &mut vars, &mut ran) {
            ran.stuck = Some(format!("step {}: {e}", i + 1));
            break;
        }
    }
    let checks = session.check();
    session.finish();
    Outcome {
        run: script.name.clone(),
        checks,
        steps: script.steps.len(),
        tool_calls: ran.tool_calls,
        tool_errors: ran.tool_errors,
        stuck: ran.stuck,
    }
}

// ------------------------------------------------------------------ report

/// One task's replay: its solution and its negative controls.
pub struct TaskReport {
    /// The task.
    pub task: &'static Task,
    /// The scripted solution's outcome.
    pub solution: Outcome,
    /// Each control's outcome, with the checks it was meant to fail.
    pub controls: Vec<(Script, Outcome)>,
}

impl TaskReport {
    /// Controls whose failing checks are exactly the ones they name.
    pub fn controls_caught(&self) -> usize {
        self.controls.iter().filter(|(s, o)| o.failed() == s.fails).count()
    }
}

/// Replays `tasks` (solutions and controls) in folders under `root`.
pub fn replay_all(tasks: &[&'static Task], root: &Path, instances: &Path) -> Vec<TaskReport> {
    tasks
        .iter()
        .map(|&task| {
            let dir = root.join(task.id);
            let solution = replay(task, &task.solution(), &dir, instances);
            let controls = task
                .controls()
                .into_iter()
                .map(|control| {
                    let outcome = replay(task, &control, &dir, instances);
                    (control, outcome)
                })
                .collect();
            TaskReport {
                task,
                solution,
                controls,
            }
        })
        .collect()
}

/// The score table for people.
pub fn table(reports: &[TaskReport]) -> String {
    let mut out = format!(
        "agent acceptance suite v{SUITE_VERSION}, ponko-mcp {}: scripted replay\n\n{:<16} {:<12} {:>8} {:>6} {:>6}  {:>8}\n",
        env!("CARGO_PKG_VERSION"),
        "task",
        "kind",
        "checks",
        "steps",
        "calls",
        "controls"
    );
    let (mut solved, mut passed, mut total, mut caught, mut controls) = (0, 0, 0, 0, 0);
    for r in reports {
        let s = &r.solution;
        out += &format!(
            "{:<16} {:<12} {:>8} {:>6} {:>6}  {:>8}{}\n",
            r.task.id,
            r.task.kind,
            format!("{}/{}", s.passed(), s.checks.len()),
            s.steps,
            s.tool_calls,
            format!("{}/{}", r.controls_caught(), r.controls.len()),
            if s.solved() { "" } else { "  NOT SOLVED" }
        );
        for c in s.checks.iter().filter(|c| !c.passed) {
            out += &format!("    failed `{}`: {}\n", c.name, c.detail);
        }
        if let Some(stuck) = &s.stuck {
            out += &format!("    stuck at {stuck}\n");
        }
        for (script, o) in &r.controls {
            if o.failed() != script.fails {
                out += &format!(
                    "    control `{}` should fail {:?}, failed {:?}\n",
                    script.name,
                    script.fails,
                    o.failed()
                );
            }
        }
        solved += usize::from(s.solved());
        passed += s.passed();
        total += s.checks.len();
        caught += r.controls_caught();
        controls += r.controls.len();
    }
    out += &format!(
        "\ntotal: {solved}/{} tasks solved, {passed}/{total} checks; {caught}/{controls} negative controls caught\n",
        reports.len()
    );
    out
}

/// The scores as JSON, in the form `baseline.json` keeps: what is the same
/// on every machine (no timings, no tool-call counts, which waits change).
pub fn baseline(reports: &[TaskReport]) -> Value {
    json!({
        "suite": SUITE_VERSION,
        "tasks": reports.iter().map(|r| json!({
            "id": r.task.id,
            "kind": r.task.kind,
            "checks": r.task.checks,
            "solution": { "passed": r.solution.passed(), "steps": r.solution.steps },
            "controls": r.controls.iter().map(|(s, o)| json!({ "name": s.name, "failed": o.failed() })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// A task by id.
pub fn task(id: &str) -> Option<&'static Task> {
    TASKS.iter().find(|t| t.id == id)
}

/// Clears what an in-process replay needs cleared before any thread starts:
/// asset folders resolve to the task's folder, not the crate's, and games
/// register in `instances`.
///
/// # Safety
///
/// Changes the environment: call it before this process starts threads.
pub unsafe fn prepare_process(instances: &Path) {
    // SAFETY: the caller promises no other thread runs yet.
    unsafe {
        std::env::remove_var("CARGO_MANIFEST_DIR");
        std::env::set_var("PONKO_INSTANCES_DIR", instances);
        // Hot reload is on in every task; an outside setting must not turn it off.
        std::env::remove_var("PONKO_HOT_RELOAD");
    }
}
