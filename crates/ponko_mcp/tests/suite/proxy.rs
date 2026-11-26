//! The suite as an MCP server for a real agent: ponko-mcp's tools, minus
//! the two that would replace the task's game, plus `submit_task`, which
//! runs the checker. `examples/agent_suite.rs mcp` serves it over stdio.

use super::{Check, Session, Task};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What a real agent did, for the result file.
pub struct Record {
    task: &'static Task,
    agent: Option<String>,
    dir: PathBuf,
    calls: Vec<Value>,
    errors: usize,
    submissions: Vec<Vec<Check>>,
    /// When the record began (the agent's client connected).
    started: Instant,
    /// Seconds from `started` to the last submission.
    submitted_at: Option<f64>,
}

impl Record {
    /// A record of `agent` (a name for the result file) working on `task` in `dir`.
    pub fn new(task: &'static Task, agent: Option<String>, dir: &Path) -> Self {
        Record {
            task,
            agent,
            dir: dir.to_path_buf(),
            calls: Vec::new(),
            errors: 0,
            submissions: Vec::new(),
            started: Instant::now(),
            submitted_at: None,
        }
    }

    /// Seconds since the record began, to a tenth.
    fn elapsed(&self) -> f64 {
        (self.started.elapsed().as_secs_f64() * 10.0).round() / 10.0
    }

    fn submitted(&mut self, checks: Vec<Check>) {
        self.submissions.push(checks);
        self.submitted_at = Some(self.elapsed());
    }

    /// The agent never submitted: score what is there.
    pub fn submit_if_never(&mut self, session: &mut Session) {
        if self.submissions.is_empty() {
            self.submitted(session.check());
        }
    }

    /// One line: checks passed, tool calls, errors, submissions.
    pub fn summary(&self) -> String {
        let last = self.last().unwrap_or_default();
        format!(
            "{} {}/{} checks, {} tool calls ({} errors), {} submissions",
            self.task.id,
            last.iter().filter(|c| c.passed).count(),
            last.len(),
            self.calls.len(),
            self.errors,
            self.submissions.len()
        )
    }

    /// The last submission's checks.
    pub fn last(&self) -> Option<&[Check]> {
        self.submissions.last().map(Vec::as_slice)
    }

    pub fn json(&self) -> Value {
        let last = self.submissions.last();
        let checks = |c: &[Check]| -> Vec<Value> {
            c.iter()
                .map(|c| json!({ "name": c.name, "passed": c.passed, "detail": c.detail }))
                .collect()
        };
        json!({
            "suite": super::SUITE_VERSION,
            "ponko_mcp": env!("CARGO_PKG_VERSION"),
            "task": self.task.id,
            "kind": self.task.kind,
            "agent": self.agent,
            "folder": self.dir,
            "solved": last.is_some_and(|c| c.iter().all(|c| c.passed)),
            "passed": last.map_or(0, |c| c.iter().filter(|c| c.passed).count()),
            "checks": last.map(|c| checks(c)),
            "tool_calls": self.calls.len(),
            "tool_errors": self.errors,
            "submissions": self.submissions.len(),
            "seconds": self.submitted_at,
            "calls": self.calls,
        })
    }
}

/// ponko-mcp tools the suite hides: they would start a second game or stop
/// the task's.
pub const HIDDEN: &[&str] = &["launch", "quit_instance"];

/// The `submit_task` tool the suite adds to ponko-mcp's.
pub fn submit_tool() -> Value {
    json!({
        "name": "submit_task",
        "description": "Say you are done with the task. Runs the task's checker on the game and its files as they are now and returns the score with each check's result. You may submit again after more work; the last submission counts.",
        "inputSchema": { "type": "object", "properties": {}, "required": [] },
        "annotations": { "readOnlyHint": false, "destructiveHint": false },
    })
}

pub fn instructions(task: &Task, dir: &Path) -> String {
    format!(
        "Task `{}` of the Ponko agent acceptance suite.\n\n{}\nThe game's folder is {} (its files are under assets/). \
         The game is already running; every tool reaches it without `instance` \
         (`launch` and `quit_instance` are off). When you are done, call `submit_task`.\n",
        task.id,
        task.goal,
        dir.display()
    )
}

/// One message from the agent's client: ponko-mcp answers it, except
/// `submit_task`, and the suite adds its instructions and its tool.
pub fn answer(session: &mut Session, record: &mut Record, message: &Value) -> Option<Value> {
    let method = message["method"].as_str().unwrap_or_default();
    let params = &message["params"];
    if method == "tools/call" && params["name"] == "submit_task" {
        let checks = session.check();
        let result = json!({
            "solved": checks.iter().all(|c| c.passed),
            "passed": checks.iter().filter(|c| c.passed).count(),
            "total": checks.len(),
            "checks": checks.iter().map(|c| json!({ "name": c.name, "passed": c.passed, "detail": c.detail })).collect::<Vec<_>>(),
        });
        record.submitted(checks);
        return Some(json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": { "content": [{ "type": "text", "text": result.to_string() }], "isError": false },
        }));
    }
    if method == "tools/call" && HIDDEN.contains(&params["name"].as_str().unwrap_or_default()) {
        let text = format!(
            "`{}` is off in the suite: the task's game is already running and must stay up",
            params["name"].as_str().unwrap_or_default()
        );
        record.errors += 1;
        let at = record.elapsed();
        record
            .calls
            .push(json!({ "tool": params["name"], "arguments": params["arguments"], "error": text, "at": at }));
        return Some(json!({
            "jsonrpc": "2.0",
            "id": message["id"],
            "result": { "content": [{ "type": "text", "text": text }], "isError": true },
        }));
    }
    let mut reply = session.agent.handle(message)?;
    match method {
        "initialize" => {
            reply["result"]["instructions"] = json!(instructions(session.task, &session.dir));
        }
        "tools/list" => {
            if let Some(tools) = reply["result"]["tools"].as_array_mut() {
                tools.retain(|t| !HIDDEN.contains(&t["name"].as_str().unwrap_or_default()));
                tools.push(submit_tool());
            }
        }
        "tools/call" => {
            let error = match reply.get("error") {
                Some(e) => Some(e["message"].as_str().unwrap_or("JSON-RPC error").to_string()),
                None => super::tool_result(&reply["result"]).err(),
            };
            record.errors += usize::from(error.is_some());
            let at = record.elapsed();
            record.calls.push(json!({
                "tool": params["name"],
                "arguments": params["arguments"],
                "error": error,
                "at": at,
            }));
        }
        _ => {}
    }
    Some(reply)
}
