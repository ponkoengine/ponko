//! The agent acceptance suite's runner: game-development tasks done through
//! the MCP tools, each scored by a checker (see `tests/suite/`).
//!
//!   agent_suite list                    the tasks
//!   agent_suite goal TASK               what an agent is told
//!   agent_suite replay [TASK...]        the scripted solutions and negative
//!                                       controls, scored; no model involved
//!   agent_suite mcp TASK [--dir DIR] [--out FILE] [--agent NAME]
//!                                       one task for a real agent: an MCP
//!                                       server over stdio, with every
//!                                       ponko-mcp tool and `submit_task`
//!   agent_suite scores FILE...          a Markdown table of `mcp` result
//!                                       files, for a pull request
//!
//! `replay` exits 1 when a solution doesn't score full marks or a control
//! isn't caught; `cargo test -p ponko_mcp --test agent_suite` runs the same
//! in CI. `mcp` is for people trying an agent on the suite: it never runs in
//! CI, needs no keys and contacts nothing; the agent is whatever MCP client
//! starts it. It prepares the task's folder (`--dir`, default a new temp
//! folder), starts the task's game headless and paused in a child process
//! (`agent_suite host TASK`, its output in `game.log`), proxies the tools,
//! and writes the result (checks, tool calls, errors, submissions) to
//! `--out` (default: next to the folder, `<folder>.result.json`). The guide's
//! "Working with AI agents" chapter has the setup.

#[path = "../tests/suite/mod.rs"]
mod suite;

use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!(
        "usage: agent_suite list | goal TASK | replay [TASK...] | mcp TASK [--dir DIR] [--out FILE] [--agent NAME] | scores FILE..."
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["list"] => {
            for t in suite::TASKS {
                println!("{:<16} {:<12} {}", t.id, t.kind, t.title);
            }
            ExitCode::SUCCESS
        }
        ["goal", id] => match suite::task(id) {
            Some(t) => {
                print!("{}", t.goal);
                ExitCode::SUCCESS
            }
            None => unknown(id),
        },
        ["replay", ids @ ..] => replay(ids),
        ["scores", files @ ..] if !files.is_empty() => scores(files),
        ["host", id] => match suite::task(id) {
            Some(t) => {
                suite::run_game(t, None);
                ExitCode::SUCCESS
            }
            None => unknown(id),
        },
        ["mcp", id, rest @ ..] => {
            let Some(task) = suite::task(id) else {
                return unknown(id);
            };
            let mut options = McpOptions::default();
            let mut rest = rest.iter();
            while let Some(flag) = rest.next() {
                let Some(value) = rest.next() else { return usage() };
                match *flag {
                    "--dir" => options.dir = Some(PathBuf::from(value)),
                    "--out" => options.out = Some(PathBuf::from(value)),
                    "--agent" => options.agent = Some((*value).to_string()),
                    _ => return usage(),
                }
            }
            match mcp(task, options) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("agent_suite: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => usage(),
    }
}

fn unknown(id: &str) -> ExitCode {
    eprintln!("agent_suite: no task `{id}` (try `agent_suite list`)");
    ExitCode::from(2)
}

/// A scratch folder of this run.
fn scratch(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ponko-agent-suite-{}-{name}", std::process::id()))
}

fn replay(ids: &[&str]) -> ExitCode {
    let mut tasks = Vec::new();
    for id in ids {
        match suite::task(id) {
            Some(t) => tasks.push(t),
            None => return unknown(id),
        }
    }
    if tasks.is_empty() {
        tasks = suite::TASKS.iter().collect();
    }
    let root = scratch("replay");
    let instances = root.join("instances");
    if let Err(e) = std::fs::create_dir_all(&instances) {
        eprintln!("agent_suite: can't create {}: {e}", instances.display());
        return ExitCode::FAILURE;
    }
    // SAFETY: before any thread starts.
    unsafe { suite::prepare_process(&instances) };
    let reports = suite::replay_all(&tasks, &root, &instances);
    let _ = std::env::set_current_dir(std::env::temp_dir());
    let _ = std::fs::remove_dir_all(&root);
    println!("\n{}", suite::table(&reports));
    let ok = reports
        .iter()
        .all(|r| r.solution.solved() && r.controls_caught() == r.controls.len());
    if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}

/// The result files of real-agent runs as one Markdown table.
fn scores(files: &[&str]) -> ExitCode {
    println!("| Task | Agent | Solved | Checks | Tool calls | Tool errors | Submissions | Seconds |");
    println!("|---|---|---|---|---|---|---|---|");
    let (mut solved, mut runs) = (0, 0);
    for file in files {
        let result: Value = match std::fs::read_to_string(file)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("agent_suite: {file}: {e}");
                return ExitCode::FAILURE;
            }
        };
        let total = result["checks"].as_array().map_or(0, Vec::len);
        runs += 1;
        solved += usize::from(result["solved"] == true);
        let seconds = result["seconds"]
            .as_f64()
            .map_or("-".to_string(), |s| format!("{s:.0}"));
        println!(
            "| `{}` | {} | {} | {}/{} | {} | {} | {} | {} |",
            result["task"].as_str().unwrap_or("?"),
            result["agent"].as_str().unwrap_or("-"),
            if result["solved"] == true { "yes" } else { "no" },
            result["passed"],
            total,
            result["tool_calls"],
            result["tool_errors"],
            result["submissions"],
            seconds
        );
    }
    println!("\n{solved}/{runs} runs solved");
    ExitCode::SUCCESS
}

#[derive(Default)]
struct McpOptions {
    dir: Option<PathBuf>,
    out: Option<PathBuf>,
    agent: Option<String>,
}

fn mcp(task: &'static suite::Task, options: McpOptions) -> Result<(), String> {
    let dir = options.dir.unwrap_or_else(|| scratch(task.id));
    let dir = std::path::absolute(&dir).map_err(|e| e.to_string())?;
    let out = options.out.unwrap_or_else(|| dir.with_extension("result.json"));
    let instances = scratch("instances");
    let program = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut session = suite::Session::in_child(task, &dir, &instances, &program)?;
    eprintln!(
        "agent_suite: task {} in {}; result goes to {}",
        task.id,
        dir.display(),
        out.display()
    );

    let mut record = suite::proxy::Record::new(task, options.agent, &dir);
    let write = |record: &suite::proxy::Record| {
        let text = serde_json::to_string_pretty(&record.json()).unwrap_or_default();
        if let Err(e) = std::fs::write(&out, text + "\n") {
            eprintln!("agent_suite: can't write {}: {e}", out.display());
        }
    };
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => suite::proxy::answer(&mut session, &mut record, &message),
            Err(e) => Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } }),
            ),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}").map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
            write(&record);
        }
    }
    // The client went away. Score what is there if the agent never submitted.
    record.submit_if_never(&mut session);
    write(&record);
    eprintln!("agent_suite: {}", record.summary());
    session.finish();
    Ok(())
}
