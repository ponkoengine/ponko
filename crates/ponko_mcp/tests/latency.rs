//! How long a tool call to a running game takes. The game's endpoint
//! accepts connections without polling, and the server keeps one connection
//! per game and reuses it, reconnecting when the game restarts. A coarse
//! bound, so a fixed delay per call (an accept poll, a connection per call)
//! can't come back unnoticed.

use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use ponko::prelude::*;
use ponko_mcp::{Config, Server};
use serde_json::{Value, json};

/// Starts a paused headless game named `name`; returns its port and thread.
fn start_game(name: &'static str) -> (u16, thread::JoinHandle<World>) {
    let (tx, rx) = mpsc::channel();
    let h = thread::spawn(move || {
        Game::new(name, 1, 1)
            .control_endpoint(true)
            .run_headless(u64::MAX, move |ctx| {
                ctx.time.pause();
                tx.send(ctx.control_addr().unwrap().port()).unwrap();
            })
    });
    (rx.recv_timeout(Duration::from_secs(10)).unwrap(), h)
}

fn tool(server: &mut Server, name: &str, args: Value) -> Result<Value, String> {
    let reply = server
        .handle(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } }))
        .unwrap();
    let r = &reply["result"];
    let text = r["content"][0]["text"].as_str().unwrap().to_string();
    if r["isError"] == true {
        Err(text)
    } else {
        Ok(serde_json::from_str(&text).unwrap())
    }
}

/// Wall time of `calls` sequential `instance_info` calls.
fn time_calls(server: &mut Server, calls: u32) -> Duration {
    let start = Instant::now();
    for _ in 0..calls {
        tool(server, "instance_info", json!({})).unwrap();
    }
    start.elapsed()
}

// One test function: it sets the environment variable that tells the games
// where to register, and tests in a file run in parallel.
#[test]
fn tool_calls_to_a_running_game_take_well_under_a_frame() {
    let dir = std::env::temp_dir().join(format!("ponko-mcp-latency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // SAFETY: set before any other thread of this test binary starts.
    unsafe { std::env::set_var("PONKO_INSTANCES_DIR", &dir) };
    let mut server = Server::new(Config {
        instances_dir: dir.clone(),
        logs_dir: dir.join("logs"),
        read_only: false,
    });

    let (_, game) = start_game("latency");
    tool(&mut server, "instance_info", json!({})).unwrap(); // warm up
    let took = time_calls(&mut server, 200);
    println!(
        "200 calls: {took:?}, {:.2} ms per call",
        took.as_secs_f64() * 1000.0 / 200.0
    );
    // 2 s is 10 ms a call: a debug build on a busy machine stays far below,
    // while one accept poll per call (10 ms on average) or a frame's wait
    // would not.
    assert!(took < Duration::from_secs(2), "200 calls took {took:?}");

    // The game restarts: it registers again, with another port and token.
    // The next call finds it and connects to it, with no error in between.
    tool(&mut server, "run_ticks", json!({ "ticks": 5 })).unwrap();
    assert_eq!(tool(&mut server, "instance_info", json!({})).unwrap()["tick"], 5);
    tool(&mut server, "quit_instance", json!({})).unwrap();
    game.join().unwrap();
    let (new_port, game) = start_game("latency");
    let info = tool(&mut server, "instance_info", json!({})).unwrap();
    assert_eq!(info["tick"], 0, "the new game answered: {info}");
    let listed = tool(&mut server, "list_instances", json!({})).unwrap();
    assert_eq!(listed[0]["port"], new_port, "{listed}");
    let took = time_calls(&mut server, 50);
    assert!(
        took < Duration::from_secs(1),
        "50 calls after the restart took {took:?}"
    );
    tool(&mut server, "quit_instance", json!({})).unwrap();
    game.join().unwrap();
    assert_eq!(tool(&mut server, "list_instances", json!({})).unwrap(), json!([]));
    let _ = std::fs::remove_dir_all(Path::new(&dir));
}
