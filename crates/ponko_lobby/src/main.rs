//! `ponko-lobby`: the reference lobby server as a dedicated server.
//!
//! It runs in [`Game::run_server`]: stops cleanly on `SIGINT`/`SIGTERM`,
//! writes JSON log lines, and passes `ponko package`'s smoke run, so it
//! goes into the same container image recipe as a game server. Settings
//! come from the environment (`ponko-lobby --help`).

use std::sync::Mutex;
use std::time::Duration;

use ponko::net::NetMessages;
use ponko::prelude::*;
use ponko::server::{LogLevel, ServerConfig, ServerStatus, log};
use ponko_lobby::{LobbyLimits, LobbyServer, StunResponder};
use serde_json::json;

const HELP: &str = "\
ponko-lobby: a reference lobby, matchmaking and relay server for Ponko games.

Settings, from the environment:
  LOBBY_UDP=0.0.0.0:7700      where clients connect over UDP
  LOBBY_WEB=0.0.0.0:7701      browser clients over WebSocket (ws://); empty: off (default)
  LOBBY_STUN=0.0.0.0:3478     answer STUN binding requests (UDP), for WebRTC
                              peers' NAT traversal; empty: off (default)
  LOBBY_MAX_CLIENTS=1024      clients the UDP endpoint takes (1..=1024)
  LOBBY_HZ=100                polls per second (each one moves every packet waiting)
  LOBBY_REPORT_SECS=10        how often to log a lobby_report line; 0: never

It stops on SIGINT or SIGTERM: members hear that the server stops, then it
exits. Logs are JSON lines on standard output. See the guide's \"Lobbies,
matchmaking and relays\" section for its limits and what it is not.
";

/// The server and its report clock, a resource of the server's world (in
/// a mutex: endpoints move between threads but aren't shared).
struct Lobby {
    server: LobbyServer,
    stun: Option<StunResponder>,
    report_every: Duration,
    last_report: Duration,
    stopping: bool,
}

fn setting(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn number<T: std::str::FromStr>(name: &str, default: &str) -> Result<T, String> {
    let value = setting(name, default);
    value.parse().map_err(|_| format!("{name}={value} is not a number"))
}

fn open() -> Result<(LobbyServer, Option<StunResponder>, u32, Duration), String> {
    let udp = setting("LOBBY_UDP", "0.0.0.0:7700");
    let web = setting("LOBBY_WEB", "");
    let stun = setting("LOBBY_STUN", "");
    let max_clients: usize = number("LOBBY_MAX_CLIENTS", "1024")?;
    let hz: u32 = number("LOBBY_HZ", "100")?;
    let report: u64 = number("LOBBY_REPORT_SECS", "10")?;
    if !(1..=1000).contains(&hz) {
        return Err(format!("LOBBY_HZ={hz} is outside 1..=1000"));
    }
    let mut endpoint = NetMessages::host(udp.as_str(), max_clients).map_err(|e| format!("LOBBY_UDP={udp}: {e}"))?;
    if !web.is_empty() {
        endpoint = endpoint
            .accept_web(web.as_str())
            .map_err(|e| format!("LOBBY_WEB={web}: {e}"))?;
    }
    let udp_addr = endpoint.local_addr();
    let web_addr = endpoint.web_addr();
    let stun = if stun.is_empty() {
        None
    } else {
        Some(StunResponder::bind(stun.as_str()).map_err(|e| format!("LOBBY_STUN={stun}: {e}"))?)
    };
    let stun_addr = stun.as_ref().and_then(|s| s.local_addr().ok());
    let mut server = LobbyServer::new(LobbyLimits::default());
    server.listen(endpoint);
    log(
        LogLevel::Info,
        "lobby_listening",
        json!({
            "udp": udp_addr.map(|a| a.to_string()),
            "web": web_addr.map(|a| a.to_string()),
            "stun": stun_addr.map(|a| a.to_string()),
            "max_clients": max_clients,
            "hz": hz,
        }),
    );
    Ok((server, stun, hz, Duration::from_secs(report)))
}

fn report(lobby: &Lobby, event: &str) {
    let s = lobby.server.stats();
    let stun = lobby.stun.as_ref().map(StunResponder::stats).unwrap_or_default();
    log(
        LogLevel::Info,
        event,
        json!({
            "connections": s.connections,
            "rooms": s.rooms,
            "queued": s.queued,
            "rooms_created": s.rooms_created,
            "matches_made": s.matches_made,
            "host_migrations": s.host_migrations,
            "rooms_closed": s.rooms_closed,
            "requests": s.requests,
            "refused": s.refused,
            "rate_limited": s.rate_limited,
            "relay_in": s.relay_in,
            "relay_out": s.relay_out,
            "relay_bytes_in": s.relay_bytes_in,
            "relay_bytes_out": s.relay_bytes_out,
            "relay_dropped": s.relay_dropped,
            "closed_connections": s.closed_connections,
            "socket_errors": s.socket_errors,
            "stun_answered": stun.answered,
            "stun_rate_limited": stun.rate_limited,
            "last_error": lobby.server.last_error(),
        }),
    );
}

/// Every tick: poll the server; on a stop request, tell the members and
/// quit once every connection is closed.
fn tick(ctx: &mut Ctx) {
    let stopping = ctx.world.resource::<ServerStatus>().stopping.is_some();
    let lobby = ctx
        .world
        .resource_mut::<Mutex<Lobby>>()
        .get_mut()
        .unwrap_or_else(|e| e.into_inner());
    if stopping && !lobby.stopping {
        lobby.stopping = true;
        lobby.server.shutdown();
    }
    lobby.server.poll();
    if let Some(stun) = &mut lobby.stun {
        stun.poll();
    }
    let now = lobby.server.uptime();
    if !lobby.report_every.is_zero() && now - lobby.last_report >= lobby.report_every {
        lobby.last_report = now;
        report(lobby, "lobby_report");
    }
    let done = lobby.stopping && lobby.server.stats().connections == 0;
    if done {
        report(lobby, "lobby_stopped");
    }
    if done {
        ctx.quit();
    }
}

fn main() {
    if std::env::args().skip(1).any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return;
    }
    let (server, stun, hz, report_every) = match open() {
        Ok(opened) => opened,
        Err(e) => {
            log(LogLevel::Error, "lobby_start_failed", json!({ "error": e }));
            std::process::exit(2);
        }
    };
    let lobby = Lobby {
        server,
        stun,
        report_every,
        last_report: Duration::ZERO,
        stopping: false,
    };
    // The server's own tick report too, and 2 s to tell members it stops.
    let config = ServerConfig::new().shutdown_grace(Duration::from_secs(2));
    Game::new("ponko-lobby", 1, 1)
        .fixed_hz(hz)
        .sim_system(tick)
        .run_server(config, move |ctx| ctx.world.insert_resource(Mutex::new(lobby)));
}
