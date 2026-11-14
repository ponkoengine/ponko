//! The dedicated server. It listens on UDP `SERVER_ADDR` (default
//! `0.0.0.0:7777`), runs at 30 ticks a second, logs JSON lines to standard
//! output and stops cleanly on `SIGTERM` or Ctrl+C: it tells every client
//! goodbye first (`shut_down` in `src/lib.rs`).
//!
//!   cargo run                              # the server
//!   cargo run --example client             # a bot client, in another terminal

use std::time::Duration;

use ponko::net::NetMessages;
use ponko::server::ServerConfig;

fn main() -> std::io::Result<()> {
    let addr = std::env::var("SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:7777".into());
    let max_clients = std::env::var("MAX_CLIENTS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(32);
    let endpoint = NetMessages::host(&addr, max_clients)?;
    let config = ServerConfig::new()
        // Time to say goodbye; well inside `docker stop`'s 10 seconds.
        .shutdown_grace(Duration::from_secs(3))
        .report_every(Duration::from_secs(10));
    {{crate_name}}::server(endpoint).run_server(config);
    Ok(())
}
