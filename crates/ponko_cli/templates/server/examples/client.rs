//! A bot client: joins the server, walks for a few seconds, leaves, and
//! exits with an error if it never got in.
//!
//!   cargo run --example client                    # the server on this machine
//!   cargo run --example client -- 10.0.0.5:7777   # another one
//!
//! `TICKS` sets how long it stays (default 180, three seconds).

use {{crate_name}}::{ClientReport, client};
use ponko::net::NetMessages;

fn main() -> Result<(), String> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:7777".into());
    let ticks = std::env::var("TICKS")
        .ok()
        .and_then(|t| t.parse().ok())
        .unwrap_or(180);
    let endpoint = NetMessages::join(&addr).map_err(|e| format!("{addr}: {e}"))?;
    // At most 30 s: a server that never answers makes this fail, not hang.
    let world = client(endpoint, "bot", ticks).run_headless(30 * 60);
    let report = world.resource::<ClientReport>();
    println!("client: {report:?}");
    match (report.welcome, &report.error) {
        (Some(id), None) if report.states > 0 => {
            println!("client: joined {addr} as player {id}, got {} states", report.states);
            Ok(())
        }
        _ => Err(format!("client: couldn't play on {addr}: {report:?}")),
    }
}
