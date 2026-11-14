//! The game server and its protocol, shared by the server program
//! (`src/main.rs`), the example client (`examples/client.rs`) and the tests
//! (`tests/server.rs`). No window, GPU or sound: the server runs on any
//! machine, in a container too.
//!
//! The server owns the world: clients send what they want to do
//! ([`ClientMsg`]), the server simulates and sends everyone the result
//! ([`ServerMsg::State`]) a few times a second.

use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};
use ponko::prelude::*;
use ponko::server::{LogLevel, ServerStatus, log};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Numbers to tune without recompiling, loaded from `assets/settings.json`.
#[derive(Serialize, Deserialize)]
pub struct Settings {
    /// The side of the square world.
    pub size: f32,
    /// How fast players move, in units per second.
    pub speed: f32,
    /// How many times a second clients get the world's state.
    pub state_hz: u32,
}

/// What a client sends.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ClientMsg {
    /// The first message: who is playing.
    Hello { name: String },
    /// Where to head: a direction, at most one unit long.
    Move { x: f32, y: f32 },
}

/// What the server sends.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ServerMsg {
    /// The answer to `Hello`: the client's player id and the tick rate.
    Welcome { id: u64, tick_hz: u32 },
    /// Every player's position, `state_hz` times a second.
    State { tick: u64, players: Vec<(u64, [f32; 2])> },
    /// The server is shutting down: leave now.
    Goodbye { reason: String },
}

/// A connected player.
#[derive(Component, Serialize, Deserialize)]
pub struct Player {
    /// The message endpoint's id for its client.
    pub peer: u64,
    pub position: Vec2,
    /// Where it heads, from its last `Move`.
    pub heading: Vec2,
}

/// How many clients joined and left so far, and whether the goodbyes went
/// out.
#[derive(Default, Serialize, Deserialize)]
pub struct Lobby {
    pub joined: u64,
    pub left: u64,
    /// The tick the shutdown goodbyes were sent, if they were.
    pub goodbye_tick: Option<u64>,
}

/// The name in the logs and for agents.
pub const TITLE: &str = "{{project-name}}";
/// How many ticks a second the server runs.
pub const TICK_HZ: u32 = 30;
/// After the goodbyes, how long the server waits for clients to leave before
/// it quits anyway, in ticks.
pub const GOODBYE_TICKS: u64 = TICK_HZ as u64;

/// The server around a message endpoint, ready for
/// `run_server(ServerConfig::new())` (`src/main.rs`) or a test.
pub fn server(endpoint: NetMessages) -> ConfiguredGame {
    Game::new(TITLE, 1, 1)
        .fixed_hz(TICK_HZ)
        .messages(endpoint)
        .sim_system(receive)
        .sim_system(simulate)
        .sim_system(broadcast)
        .sim_system(shut_down)
        .setup(setup)
}

fn setup(ctx: &mut Ctx) {
    ctx.assets.load_resource::<Settings>(&mut ctx.world, "settings.json");
    ctx.world.insert_resource(Lobby::default());
    // Reflected types are what agents can read and change through ponko-mcp.
    ctx.world.reflect::<Player>();
    ctx.world.reflect_resource::<Settings>();
    ctx.world.reflect_resource::<Lobby>();
}

/// Sim system: joins, moves and leaves, from the message endpoint's events.
pub fn receive(ctx: &mut Ctx) {
    let size = ctx.world.resource::<Settings>().size;
    let messages = ctx.net.messages.as_mut().expect("Game::messages");
    let mut events = Vec::new();
    while let Some(event) = messages.next_event() {
        events.push(event);
    }
    for event in events {
        match &event {
            MessageEvent::Connected(peer) => {
                log(LogLevel::Info, "client_connected", json!({ "peer": peer.to_bits() }));
            }
            &MessageEvent::Message { peer, .. } => match event.decode::<ClientMsg>() {
                Some(ClientMsg::Hello { name }) => {
                    let welcome = ServerMsg::Welcome { id: peer.to_bits(), tick_hz: TICK_HZ };
                    send(ctx, peer, Lane::Reliable, &welcome);
                    ctx.world.spawn((Player {
                        peer: peer.to_bits(),
                        position: Vec2::splat(size / 2.0),
                        heading: Vec2::ZERO,
                    },));
                    ctx.world.resource_mut::<Lobby>().joined += 1;
                    log(LogLevel::Info, "player_joined", json!({ "peer": peer.to_bits(), "name": name }));
                }
                Some(ClientMsg::Move { x, y }) => {
                    let heading = vec2(x, y).clamp_length_max(1.0);
                    let heading = if heading.is_finite() { heading } else { Vec2::ZERO };
                    ctx.world.query::<&mut Player>().for_each(|p| {
                        if p.peer == peer.to_bits() {
                            p.heading = heading;
                        }
                    });
                }
                None => log(LogLevel::Warn, "bad_message", json!({ "peer": peer.to_bits() })),
            },
            MessageEvent::Disconnected(peer, reason) => {
                let peer = *peer;
                let mut gone = Vec::new();
                ctx.world.query::<(Entity, &Player)>().for_each(|(e, p)| {
                    if p.peer == peer.to_bits() {
                        gone.push(e);
                    }
                });
                for e in gone {
                    ctx.world.despawn(e);
                }
                ctx.world.resource_mut::<Lobby>().left += 1;
                log(LogLevel::Info, "client_left", json!({ "peer": peer.to_bits(), "reason": reason }));
            }
        }
    }
}

fn send(ctx: &mut Ctx, peer: PeerId, lane: Lane, msg: &ServerMsg) {
    let messages = ctx.net.messages.as_mut().expect("Game::messages");
    if let Err(e) = messages.send_serde(peer, lane, msg) {
        log(LogLevel::Warn, "send_failed", json!({ "peer": peer.to_bits(), "error": e.to_string() }));
    }
}

/// Sim system: moves every player, kept inside the world.
pub fn simulate(ctx: &mut Ctx) {
    let settings = ctx.world.resource::<Settings>();
    let (size, step) = (settings.size, settings.speed * ctx.time.dt());
    ctx.world.query::<&mut Player>().for_each(|p| {
        p.position = (p.position + p.heading * step).clamp(Vec2::ZERO, Vec2::splat(size));
    });
}

/// Sim system: everyone's position to every client, `state_hz` times a
/// second, on the unreliable lane (a lost state is replaced by the next).
pub fn broadcast(ctx: &mut Ctx) {
    let tick = ctx.world.resource::<ServerStatus>().tick;
    let every = u64::from((TICK_HZ / ctx.world.resource::<Settings>().state_hz.max(1)).max(1));
    if tick % every != 0 {
        return;
    }
    let mut players = Vec::new();
    ctx.world
        .query::<&Player>()
        .for_each(|p| players.push((p.peer, p.position.to_array())));
    let state = serde_json::to_vec(&ServerMsg::State { tick, players }).expect("state");
    let messages = ctx.net.messages.as_mut().expect("Game::messages");
    if let Err(e) = messages.send_to_all(Lane::Unreliable, &state) {
        log(LogLevel::Warn, "send_failed", json!({ "error": e.to_string() }));
    }
}

/// Sim system: once the server is asked to stop (`SIGTERM` from `docker
/// stop`, Ctrl+C), tells every client goodbye, waits for them to leave (at
/// most [`GOODBYE_TICKS`]), then quits.
pub fn shut_down(ctx: &mut Ctx) {
    let status = ctx.world.resource::<ServerStatus>();
    let (Some(reason), tick) = (status.stopping, status.tick) else {
        return;
    };
    let goodbye_tick = ctx.world.resource::<Lobby>().goodbye_tick;
    let Some(sent) = goodbye_tick else {
        let bye = serde_json::to_vec(&ServerMsg::Goodbye {
            reason: format!("{reason:?}"),
        })
        .expect("goodbye");
        let messages = ctx.net.messages.as_mut().expect("Game::messages");
        let _ = messages.send_to_all(Lane::Reliable, &bye);
        ctx.world.resource_mut::<Lobby>().goodbye_tick = Some(tick);
        return;
    };
    let players = ctx.world.query::<&Player>().iter().count();
    if players == 0 || tick >= sent + GOODBYE_TICKS {
        log(LogLevel::Info, "goodbyes_done", json!({ "players_left_behind": players }));
        ctx.quit();
    }
}

/// What the example client saw: [`client`].
#[derive(Default, Debug)]
pub struct ClientReport {
    pub welcome: Option<u64>,
    pub states: u32,
    pub goodbye: Option<String>,
    pub error: Option<String>,
}

/// A bot client around a joining endpoint: says hello, walks in a circle,
/// and leaves after `ticks` ticks (60 a second) with at least one state, or
/// when the server says goodbye. Its [`ClientReport`] is in the returned
/// world. `examples/client.rs` runs it against a server.
pub fn client(endpoint: NetMessages, name: &str, ticks: u64) -> ConfiguredGame {
    let name = name.to_string();
    Game::new("client", 1, 1)
        .messages(endpoint)
        .setup(|ctx| ctx.world.insert_resource(ClientReport::default()))
        .sim_system(move |ctx: &mut Ctx| {
            let messages = ctx.net.messages.as_mut().expect("Game::messages");
            let mut report = std::mem::take(ctx.world.resource_mut::<ClientReport>());
            while let Some(event) = messages.next_event() {
                match &event {
                    MessageEvent::Connected(host) => {
                        let hello = ClientMsg::Hello { name: name.clone() };
                        let _ = messages.send_serde(*host, Lane::Reliable, &hello);
                    }
                    MessageEvent::Message { .. } => match event.decode::<ServerMsg>() {
                        Some(ServerMsg::Welcome { id, .. }) => report.welcome = Some(id),
                        Some(ServerMsg::State { .. }) => report.states += 1,
                        Some(ServerMsg::Goodbye { reason }) => report.goodbye = Some(reason),
                        None => {}
                    },
                    MessageEvent::Disconnected(_, reason) => report.error = Some(reason.clone()),
                }
            }
            let tick = ctx.time.sim_tick();
            if report.welcome.is_some() && report.goodbye.is_none() && report.error.is_none() {
                let angle = tick as f32 * 0.05;
                let walk = ClientMsg::Move { x: angle.cos(), y: angle.sin() };
                let _ = messages.send_serde(PeerId::HOST, Lane::Unreliable, &walk);
            }
            let done = (tick >= ticks && report.states > 0) || report.goodbye.is_some();
            if done && report.error.is_none() {
                let _ = messages.disconnect(PeerId::HOST);
            }
            let stop = done || report.error.is_some();
            *ctx.world.resource_mut::<ClientReport>() = report;
            if stop {
                ctx.quit();
            }
        })
        // A headless run doesn't wait for a display: don't spin.
        .sim_system(|_: &mut Ctx| std::thread::sleep(std::time::Duration::from_millis(16)))
}
