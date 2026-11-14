//! Lockstep multiplayer: every player moves a crab with the arrow keys,
//! and every window shows the same game, because all run the same
//! simulation from the same inputs. Run it twice:
//!
//!   `cargo run --example lockstep -- host 7777`
//!   `cargo run --example lockstep -- join 127.0.0.1:7777`
//!
//! (or on two machines on one network). Space shoves the other crabs away.
//!
//! In the browser: the host also takes browser players over WebSocket, and
//! the web build joins the server its page names (`?server=ws://host:7778`;
//! without it, port 7778 of the page's own host):
//!
//!   `cargo run --example lockstep -- host 7777 --players 3 --web 7778`
//!   `web/build.sh lockstep && python3 -m http.server -d target/web/lockstep 8000`
//!
//! then open `http://localhost:8000` in two tabs. Desktop players can join
//! the same game with `join` (UDP) or `join-web ws://127.0.0.1:7778`.
//! `--headless <ticks>` plays that many ticks without a window, with a bot
//! at the keys, and prints how it went (the browser smoke test's server).
//!
//! Over Steam (feature `steam`; Steam running; Valve's test app 480):
//!
//!   `cargo run --example lockstep --features steam -- steam`       host, press I to invite
//!   `cargo run --example lockstep --features steam -- steam-join`  then accept the invite

#[allow(unused_imports)]
use ponko::d2::*;
#[allow(unused_imports)]
use ponko::net::{Net, NetState};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

// ANCHOR: input
/// What one player does in one tick: all that crosses the network.
#[derive(Serialize, Deserialize, Default)]
struct Pad {
    dir: Vec2,
    shove: bool,
}

/// Reads this player's keys every frame; the session sends them.
fn read_keys(ctx: &mut Ctx) {
    let mut pad = Pad::default();
    for (key, d) in [
        (Key::Left, -Vec2::X),
        (Key::Right, Vec2::X),
        (Key::Up, -Vec2::Y),
        (Key::Down, Vec2::Y),
    ] {
        if ctx.input.key_down(key) {
            pad.dir += d;
        }
    }
    pad.shove = ctx.input.key_down(Key::Space);
    ctx.net.set_input(&pad);
}
// ANCHOR_END: input

/// A crab, and whose it is.
#[derive(Serialize, Deserialize, Clone, Copy, Component)]
struct Crab {
    player: u32,
    push: Vec2,
}

/// The crabs, once the game starts and everyone knows how many players
/// there are. Plain arithmetic only: every peer, desktop or browser, must
/// get the same bits.
fn spawn_crabs(ctx: &mut Ctx) {
    if ctx.time.sim_tick() > 0 {
        return;
    }
    let players = ctx.net.players().max(1);
    for player in 0..players {
        let x = 100.0 + 600.0 * player as f32 / (players.max(2) - 1) as f32;
        ctx.world.spawn((
            d2::Transform::at(vec2(x, 300.0)),
            Crab {
                player: player as u32,
                push: Vec2::ZERO,
            },
        ));
    }
}

// ANCHOR: simulate
/// The simulation: every peer runs it on every player's input, in the
/// same order, so every peer ends up with the same world.
fn move_crabs(ctx: &mut Ctx) {
    let pads: Vec<Pad> = (0..ctx.net.players())
        .map(|p| ctx.net.input::<Pad>(p).unwrap_or_default())
        .collect();
    let dt = ctx.time.dt();
    let crabs: Vec<(Entity, Vec2, u32)> = ctx
        .world
        .query::<(Entity, &d2::Transform, &Crab)>()
        .iter()
        .map(|(e, t, c)| (e, t.position, c.player))
        .collect();
    for &(e, at, player) in &crabs {
        let pad = &pads[player as usize];
        if pad.shove {
            for &(other, other_at, _) in &crabs {
                if other != e && at.distance(other_at) < 120.0 {
                    ctx.world.get_mut::<Crab>(other).unwrap().push += (other_at - at).normalize_or_zero() * 900.0;
                }
            }
        }
        let crab = *ctx.world.get::<Crab>(e).unwrap();
        let t = ctx.world.get_mut::<d2::Transform>(e).unwrap();
        t.position += (pad.dir.normalize_or_zero() * 250.0 + crab.push) * dt;
        t.position = t.position.clamp(vec2(40.0, 40.0), vec2(760.0, 560.0));
        ctx.world.get_mut::<Crab>(e).unwrap().push = crab.push * 0.9;
    }
}
// ANCHOR_END: simulate

/// One color per player (up to 8).
const COLORS: [u32; 8] = [
    0xf56565, 0x63b3ed, 0x68d391, 0xf6e05e, 0xb794f4, 0xf6ad55, 0x4fd1c5, 0xfbb6ce,
];

fn draw(ctx: &mut Ctx) {
    ctx.gfx.clear(Color::hex(0x20304a));
    let crabs: Vec<(Vec2, u32)> = ctx
        .world
        .query::<(&d2::Transform, &Crab)>()
        .iter()
        .map(|(t, c)| (t.position, c.player))
        .collect();
    for (at, player) in crabs {
        ctx.gfx.draw_circle(at, 30.0, Color::hex(COLORS[player as usize % 8]));
    }
    let net = &ctx.net;
    let status = match net.state() {
        NetState::Connecting => "waiting for the other players...".to_string(),
        NetState::Running => format!(
            "you are player {} of {}, tick {}{}",
            net.player() + 1,
            net.players(),
            ctx.time.sim_tick(),
            net.desync().map_or(String::new(), |d| format!("  DESYNC: {d}"))
        ),
        NetState::Lost => format!("connection lost: {}", net.error().unwrap_or("?")),
        NetState::Offline => "offline".into(),
        _ => String::new(),
    };
    ctx.gfx.draw_text(&status, vec2(20.0, 20.0), 20.0, Color::WHITE);
    #[cfg(feature = "steam")]
    if ctx.net.state() == NetState::Connecting
        && let Some(lobby) = ctx.net.steam_lobby()
    {
        let text = format!("Steam lobby {}: press I to invite a friend", lobby.raw());
        ctx.gfx.draw_text(&text, vec2(20.0, 50.0), 20.0, Color::WHITE);
        if ctx.input.key_pressed(Key::I) {
            ctx.net.steam_invite();
        }
    }
    if ctx.input.key_pressed(Key::Escape) {
        ctx.quit();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(feature = "steam")]
    if let Some(net) = steam_session(&args) {
        return play(net);
    }
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
    // "7777" listens on every interface; "127.0.0.1:7777" on that one only.
    let at = |addr: &str| {
        if addr.contains(':') {
            addr.to_string()
        } else {
            format!("0.0.0.0:{addr}")
        }
    };
    let players = flag("--players").map_or(2, |n| n.parse().expect("--players takes a number"));
    let address = args.get(1).filter(|a| !a.starts_with("--"));
    // ANCHOR: session
    let net = match args.first().map(String::as_str) {
        Some("host") => {
            let host = Net::host(at(address.map_or("7777", |a| a)), players);
            match flag("--web") {
                // Browser players join over WebSocket on this TCP port.
                Some(web) => host.and_then(|host| host.accept_web(at(web))),
                None => host,
            }
        }
        Some("join") => Net::join(address.map_or("127.0.0.1:7777", |a| a)),
        Some("join-web") => Net::join_web(address.map_or("ws://127.0.0.1:7778", |a| a)),
        _ => {
            eprintln!(
                "usage: lockstep host [port] [--players n] [--web port] | join <host:port> \
                 | join-web <ws://host:port> | steam | steam-join   (any of them: --headless <ticks>)"
            );
            return;
        }
    }
    .expect("could not open a socket");
    // ANCHOR_END: session
    if let Some(addr) = net.local_addr().filter(|_| args[0] == "host") {
        let web = net
            .web_addr()
            .map_or(String::new(), |a| format!(", browsers on ws://{a}"));
        println!("hosting {players} players on {addr}{web}");
    }
    match flag("--headless") {
        Some(ticks) => headless(net, ticks.parse().expect("--headless takes a tick count")),
        None => play(net),
    }
}

/// The web build joins the server its page names: `?server=ws://host:port`,
/// or port 7778 of the page's own host.
#[cfg(target_arch = "wasm32")]
fn main() {
    let location = web_sys::window().expect("a page").location();
    let search = location.search().unwrap_or_default();
    let named = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("server="))
        .map(str::to_string);
    let server = named.unwrap_or_else(|| format!("ws://{}:7778", location.hostname().unwrap_or_default()));
    play(Net::join_web(&server).expect("a ws:// or wss:// server URL"));
}

// ANCHOR: play
fn play(net: Net) {
    game(net)
        .system(read_keys) // frame system: input out
        .system(draw)
        .system(report)
        .setup(setup)
        .run();
}

/// What every peer runs: the session and the simulation.
fn game(net: Net) -> Game {
    Game::new("Lockstep", 800, 600)
        .lockstep(net)
        .sim_system(spawn_crabs) // sim systems: everyone's input in
        .sim_system(move_crabs)
        .sim_system(fingerprint)
}

fn setup(ctx: &mut Ctx) {
    ctx.world.reflect::<Crab>(); // compared between peers
    ctx.world.insert_resource(Fingerprint(None, [0; 8]));
}
// ANCHOR_END: play

/// The tick whose crab positions every peer reports, so a test can
/// compare peers that run at different speeds.
const FINGERPRINT_TICK: u64 = 300;

/// A hash of every crab's position and push at [`FINGERPRINT_TICK`], and
/// how many ticks each player steered by then: what the peers must agree on.
struct Fingerprint(Option<u64>, [u32; 8]);

fn fingerprint(ctx: &mut Ctx) {
    for player in 0..ctx.net.players() {
        if ctx.time.sim_tick() < FINGERPRINT_TICK && ctx.net.input::<Pad>(player).is_some_and(|p| p.dir != Vec2::ZERO) {
            ctx.world.resource_mut::<Fingerprint>().1[player] += 1;
        }
    }
    if ctx.time.sim_tick() != FINGERPRINT_TICK {
        return;
    }
    let mut crabs: Vec<(u32, [f32; 4])> = ctx
        .world
        .query::<(&d2::Transform, &Crab)>()
        .iter()
        .map(|(t, c)| (c.player, [t.position.x, t.position.y, c.push.x, c.push.y]))
        .collect();
    crabs.sort_by_key(|c| c.0);
    // FNV-1a over the bits: equal only if every peer computed the same floats.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for bits in crabs.iter().flat_map(|(_, v)| v.map(f32::to_bits)) {
        hash = (hash ^ u64::from(bits)).wrapping_mul(0x100_0000_01b3);
    }
    ctx.world.resource_mut::<Fingerprint>().0 = Some(hash);
}

/// The state, as JSON: for the browser smoke test (`web/smoke/multiplayer.mjs`)
/// and the headless summary.
fn state(ctx: &Ctx) -> String {
    let net = &ctx.net;
    serde_json::json!({
        "fixture": "lockstep",
        "state": format!("{:?}", net.state()),
        "player": net.player(),
        "players": net.players(),
        "ticks": ctx.time.sim_tick(),
        "fingerprint": ctx.world.resource::<Fingerprint>().0.map(|h| format!("{h:016x}")),
        "steered": &ctx.world.resource::<Fingerprint>().1[..net.players()],
        "desync": net.desync().map(ToString::to_string),
        "error": net.error(),
    })
    .to_string()
}

/// Publishes the state on the page (`<html data-smoke>`) for the browser
/// smoke test; nothing on the desktop.
fn report(ctx: &mut Ctx) {
    #[cfg(target_arch = "wasm32")]
    if let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    {
        let _ = root.set_attribute("data-smoke", &state(ctx));
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ctx;
}

/// `--headless <ticks>`: plays without a window, a bot at the keys, and
/// prints the final state. Waits up to a minute for everyone to join.
#[cfg(not(target_arch = "wasm32"))]
fn headless(net: Net, ticks: u64) {
    let summary = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let out = summary.clone();
    game(net.timeout(std::time::Duration::from_secs(60)))
        .sim_system(bot)
        .sim_system(move |ctx: &mut Ctx| *out.borrow_mut() = state(ctx))
        .run_headless(ticks, setup);
    println!("{}", summary.borrow());
}

/// The headless player's keys: a different walk for every player.
#[cfg(not(target_arch = "wasm32"))]
fn bot(ctx: &mut Ctx) {
    let step = (ctx.time.sim_tick() / 40 + ctx.net.player() as u64) % 4;
    let dir = [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y][step as usize];
    let shove = ctx.time.sim_tick() % 120 == 60;
    ctx.net.set_input(&Pad { dir, shove });
}

// ANCHOR: steam
/// Steam: the host creates a friends-only lobby and invites from the
/// overlay; the friend accepts in Steam. No addresses, no port forwarding.
#[cfg(feature = "steam")]
fn steam_session(args: &[String]) -> Option<Net> {
    let mode = args.first().map(String::as_str);
    let invited = ponko::net::steam_lobby_from_args(); // launched by accepting an invite
    if !matches!(mode, Some("steam" | "steam-join")) && invited.is_none() {
        return None;
    }
    // 480 is Valve's test app; a real game uses its own app id.
    let client = steamworks::Client::init_app(480).expect("Steam must be running and logged in");
    if let Some(lobby) = invited {
        return Some(Net::steam_join(&client, lobby));
    }
    if mode == Some("steam") {
        return Some(Net::steam_host(&client, 2));
    }
    println!("waiting for you to accept a Steam invite...");
    let lobby = ponko::net::steam_wait_for_invite(&client, std::time::Duration::from_secs(300))?;
    Some(Net::steam_join(&client, lobby))
}
// ANCHOR_END: steam
