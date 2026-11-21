//! Orb Arena: the multiplayer reference game. Players collect drifting
//! orbs in a walled square; the server owns everything, clients send their
//! controls and predict their own player. It is built only on declared
//! replication ([`ponko::net::Replication`]): no packet code anywhere.
//!
//!   `net_arena`                     host and play (a listen server, UDP 7777)
//!   `net_arena server [addr]`       a dedicated server: no window, logs only
//!   `net_arena join <addr>`         join one, such as `127.0.0.1:7777`
//!
//! Add `--bot` to let a bot play instead of the keyboard. Browser clients can
//! join a server too: it also accepts WebSocket connections on port 7778
//! (`server [addr] --web <addr>` picks another), and the web build joins the
//! server its page names (`?server=ws://host:port`; without it, port 7778 of
//! the page's own host). The browser smoke test (`web/smoke/replication.mjs`)
//! runs a dedicated server on free ports and checks that a headless browser
//! tab joins it and sees replicated orbs move.
//!
//! Move with the arrow keys or WASD, dash with Shift, wave with Space.
//!
//! The rules live in `game.rs`, which `tests/net_arena.rs` also plays with
//! no window: a dedicated server and two bots over an in-memory network
//! that loses, delays and reorders packets (`cargo test --test net_arena`).

mod game;

use game::*;
use ponko::net::{NetEvent, NetMessages, Replica, Replicated};
use ponko::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
const PORT: u16 = 7777;
const WEB_PORT: u16 = 7778;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bot = args.iter().any(|a| a == "--bot");
    let mut words: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--bot").collect();
    let default_web = format!("0.0.0.0:{WEB_PORT}");
    let web = match words.iter().position(|a| *a == "--web") {
        Some(i) if i + 1 < words.len() => {
            let addr = words[i + 1].to_string();
            words.drain(i..i + 2);
            addr
        }
        Some(_) => fail("--web needs an address, such as 127.0.0.1:7778"),
        None => default_web,
    };
    match words.as_slice() {
        [] => {
            let net = host(&format!("0.0.0.0:{PORT}"), &web);
            play(build("Orb Arena (host)").messages(net), bot, true);
        }
        ["server"] => serve(&format!("0.0.0.0:{PORT}"), &web),
        ["server", addr] => serve(addr, &web),
        ["join", addr] => {
            let net = NetMessages::join(*addr).unwrap_or_else(|e| fail(&format!("can't join {addr}: {e}")));
            play(build("Orb Arena").messages(net), bot, false);
        }
        _ => fail("usage: net_arena [server [addr] [--web <addr>] | join <addr>] [--bot]"),
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // A browser can't host: it joins the server its page names
    // (`?server=ws://host:port`), or port 7778 of the page's own host.
    let location = web_sys::window().expect("a page").location();
    let search = location.search().unwrap_or_default();
    let named = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|pair| pair.strip_prefix("server="))
        .map(str::to_string);
    let server = named.unwrap_or_else(|| format!("ws://{}:{WEB_PORT}", location.hostname().unwrap_or_default()));
    let net = NetMessages::join_web(&server).expect("a ws:// or wss:// server URL");
    play(build("Orb Arena").messages(net).system(smoke::publish), false, false);
}

#[cfg(not(target_arch = "wasm32"))]
fn fail(message: &str) -> ! {
    eprintln!("net_arena: {message}");
    std::process::exit(2);
}

/// A UDP endpoint for up to 16 clients, browsers included (over WebSocket
/// on `web`).
#[cfg(not(target_arch = "wasm32"))]
fn host(addr: &str, web: &str) -> NetMessages {
    let net = NetMessages::host(addr, 16).unwrap_or_else(|e| fail(&format!("can't host on {addr}: {e}")));
    net.accept_web(web)
        .unwrap_or_else(|e| fail(&format!("can't accept browsers on {web}: {e}")))
}

// ANCHOR: dedicated
/// A dedicated server: the same game, no window, paced to wall time, with
/// JSON log lines and a clean stop on Ctrl-C.
#[cfg(not(target_arch = "wasm32"))]
fn serve(addr: &str, web: &str) {
    let net = host(addr, web);
    // Where it listens (port 0 picks a free one): the smoke test reads it.
    if let (Some(udp), Some(web)) = (net.local_addr(), net.web_addr()) {
        println!("serving on {udp}, browsers on ws://{web}");
    }
    let config = ponko::server::ServerConfig::new();
    build("Orb Arena server").messages(net).run_server(config, |_| {});
}
// ANCHOR_END: dedicated

/// A window: the keyboard (or a bot) plays, everything is drawn.
fn play(game: Game, bot: bool, host_plays: bool) {
    let game = if bot {
        game.sim_system(game::bot)
    } else {
        game.sim_system(keyboard)
    };
    game.system(draw).run(move |ctx: &mut Ctx| {
        if host_plays {
            ctx.world.insert_resource(HostPlays);
        }
        ctx.camera.look_at(Vec2::ZERO);
    });
}

// ANCHOR: controls
/// This tick's controls: replication sends them to the server and predicts
/// them here.
fn keyboard(ctx: &mut Ctx) {
    let key = |k: Key| if ctx.input.key_down(k) { 1.0_f32 } else { 0.0 };
    let stick = vec2(
        key(Key::Right).max(key(Key::D)) - key(Key::Left).max(key(Key::A)),
        key(Key::Up).max(key(Key::W)) - key(Key::Down).max(key(Key::S)),
    );
    let dash = ctx.input.key_down(Key::LeftShift) || ctx.input.key_down(Key::RightShift);
    ctx.net.replication.set_input(Controls { stick, dash });
    if ctx.input.key_pressed(Key::Space) {
        ctx.net.replication.to_server(&Wave);
    }
}
// ANCHOR_END: controls

/// Who waved last, and until when to show it.
#[derive(Default)]
struct Shout {
    by: Option<ponko::net::NetId>,
    until: f64,
}

fn draw(ctx: &mut Ctx) {
    let now = ctx.time.elapsed();
    let me = ctx.net.replication.me();
    let mut shout = ctx.world.remove_resource::<Shout>().unwrap_or_default();
    for w in ctx.events.read::<NetEvent<Waved>>() {
        shout = Shout {
            by: Some(w.event.by),
            until: now + 2.0,
        };
    }
    ctx.gfx.clear(Color::PONKO_DARK);
    let wall = Color::rgb(0.3, 0.32, 0.4);
    let h = HALF;
    for (a, b) in [
        ((-h, -h), (h, -h)),
        ((h, -h), (h, h)),
        ((h, h), (-h, h)),
        ((-h, h), (-h, -h)),
    ] {
        ctx.gfx.draw_line(Vec2::from(a), Vec2::from(b), 3.0, wall);
    }
    let mut orbs = Vec::new();
    ctx.world.query::<&Orb>().for_each(|o| orbs.push(o.at));
    for at in orbs {
        ctx.gfx.draw_circle(at, 5.0, Color::YELLOW);
    }
    let mut players = Vec::new();
    ctx.world
        .query::<(&Player, Option<&Replica>, Option<&Replicated>)>()
        .for_each(|(p, replica, original)| {
            let id = replica
                .map(Replica::id)
                .or_else(|| original.and_then(Replicated::net_id));
            let mine = replica.is_some_and(Replica::is_mine)
                || original.is_some_and(|r| r.owner().is_some() && r.owner() == me);
            players.push((p.at, id, mine));
        });
    for (at, id, mine) in players {
        let color = if mine {
            Color::rgb(0.3, 0.8, 1.0)
        } else {
            Color::rgb(1.0, 0.45, 0.4)
        };
        ctx.gfx.draw_circle(at, 9.0, color);
        if shout.by.is_some() && shout.by == id && now < shout.until {
            ctx.gfx.draw_text("hi!", at + vec2(-10.0, 16.0), 16.0, Color::WHITE);
        }
    }
    let rows = scoreboard(ctx);
    for (n, (mine, score)) in rows.iter().enumerate() {
        let label = if *mine {
            format!("you {score}")
        } else {
            format!("{score}")
        };
        let at = vec2(-h, h + 12.0 + 18.0 * n as f32);
        ctx.gfx.draw_text(&label, at, 16.0, Color::WHITE);
    }
    if let Some(e) = ctx.net.replication.error() {
        ctx.gfx.draw_text(e, vec2(-h, -h - 24.0), 16.0, Color::RED);
    }
    ctx.world.insert_resource(shout);
}

/// What the web build shows the browser smoke test (`web/smoke/replication.mjs`):
/// `<html data-smoke>` with whether it joined, the replicas it holds, and
/// how far the replicated orbs moved since it first saw each.
#[cfg(target_arch = "wasm32")]
mod smoke {
    use std::collections::BTreeMap;

    use super::game::{Orb, Player};
    use ponko::net::Replica;
    use ponko::prelude::*;

    /// Where each orb (by net id) was when it was first seen.
    #[derive(Default)]
    struct FirstSeen(BTreeMap<u32, Vec2>);

    pub fn publish(ctx: &mut Ctx) {
        let mut first = ctx.world.remove_resource::<FirstSeen>().unwrap_or_default();
        let (mut orbs, mut moved, mut farthest) = (0usize, 0usize, 0.0f32);
        ctx.world.query::<(&Orb, &Replica)>().for_each(|(orb, replica)| {
            let from = *first.0.entry(replica.id().index()).or_insert(orb.at);
            let distance = from.distance(orb.at);
            orbs += 1;
            moved += usize::from(distance > 1.0);
            farthest = farthest.max(distance);
        });
        let mut replicas = 0usize;
        ctx.world.query::<&Replica>().for_each(|_| replicas += 1);
        let mut mine = false;
        ctx.world
            .query::<(&Player, &Replica)>()
            .for_each(|(_, r)| mine |= r.is_mine());
        ctx.world.insert_resource(first);
        let state = serde_json::json!({
            "fixture": "net_arena",
            "ticks": ctx.time.sim_tick(),
            "joined": ctx.net.replication.is_joined(),
            "error": ctx.net.replication.error(),
            "replicas": replicas,
            "orbs": orbs,
            "moved": moved,
            "farthest": farthest,
            "mine": mine,
        });
        if let Some(root) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        {
            let _ = root.set_attribute("data-smoke", &state.to_string());
        }
    }
}
