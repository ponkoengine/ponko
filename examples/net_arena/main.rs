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
//! join a server too: it also accepts WebSocket connections on port 7778,
//! and a web build joins with `NetMessages::join_web("ws://<host>:7778")`.
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
    let words: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--bot").collect();
    match words.as_slice() {
        [] => {
            let net = host(&format!("0.0.0.0:{PORT}"));
            play(build("Orb Arena (host)").messages(net), bot, true);
        }
        ["server"] => serve(&format!("0.0.0.0:{PORT}")),
        ["server", addr] => serve(addr),
        ["join", addr] => {
            let net = NetMessages::join(*addr).unwrap_or_else(|e| fail(&format!("can't join {addr}: {e}")));
            play(build("Orb Arena").messages(net), bot, false);
        }
        _ => fail("usage: net_arena [server [addr] | join <addr>] [--bot]"),
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    // A browser can't host: it joins the server of the page's machine.
    let net = NetMessages::join_web(&format!("ws://127.0.0.1:{WEB_PORT}")).expect("a ws:// address");
    play(build("Orb Arena").messages(net), false, false);
}

#[cfg(not(target_arch = "wasm32"))]
fn fail(message: &str) -> ! {
    eprintln!("net_arena: {message}");
    std::process::exit(2);
}

/// A UDP endpoint for up to 16 clients, browsers included.
#[cfg(not(target_arch = "wasm32"))]
fn host(addr: &str) -> NetMessages {
    let net = NetMessages::host(addr, 16).unwrap_or_else(|e| fail(&format!("can't host on {addr}: {e}")));
    let web = std::net::SocketAddr::new([0, 0, 0, 0].into(), WEB_PORT);
    net.accept_web(web)
        .unwrap_or_else(|e| fail(&format!("can't accept browsers on {web}: {e}")))
}

// ANCHOR: dedicated
/// A dedicated server: the same game, no window, paced to wall time, with
/// JSON log lines and a clean stop on Ctrl-C.
#[cfg(not(target_arch = "wasm32"))]
fn serve(addr: &str) {
    let config = ponko::server::ServerConfig::new();
    build("Orb Arena server")
        .messages(host(addr))
        .run_server(config, |_| {});
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
