//! Byte messages between two games, peer to peer over WebRTC: the same two
//! games as `net_messages`, on a WebRTC host and client instead of UDP. Run
//! with `cargo run --example net_webrtc --no-default-features --features webrtc`.
//!
//! The two endpoints exchange their offer, answer and addresses through a
//! signaling channel; here a thread carries them between the two handles.
//! In a game the lobby does it (`LobbyClient::signaling`).
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ponko::net::rtc::{RtcConfig, Signaling};
use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};
use ponko::prelude::*;

/// The host answers every message.
fn host(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("Game::messages");
    let mut done = false;
    while let Some(event) = messages.next_event() {
        match event {
            MessageEvent::Connected(peer) => println!("host: {peer:?} joined over WebRTC"),
            MessageEvent::Message { peer, bytes, .. } => {
                println!("host: received {}", String::from_utf8_lossy(&bytes));
                messages.send_serde(peer, Lane::Reliable, &42u32).unwrap();
            }
            MessageEvent::Disconnected(peer, reason) => {
                println!("host: {peer:?} left ({reason})");
                done = true;
            }
        }
    }
    if done {
        ctx.quit();
    }
}

/// The client says hello once connected and leaves when the answer is in.
fn client(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("Game::messages");
    let mut answer = None;
    while let Some(event) = messages.next_event() {
        match &event {
            MessageEvent::Connected(host) => messages.send(*host, Lane::Reliable, b"hello").unwrap(),
            MessageEvent::Message { .. } => answer = event.decode::<u32>(),
            MessageEvent::Disconnected(_, reason) => panic!("{reason}"),
        }
    }
    if let Some(answer) = answer {
        println!("client: received {answer}");
        messages.disconnect(PeerId::HOST).unwrap();
        ctx.world.insert_resource(answer);
        ctx.quit();
    }
}

fn main() -> std::io::Result<()> {
    // ANCHOR: webrtc
    let (host_side, client_side) = (Signaling::new(), Signaling::new());
    // On one machine; across the internet add `.stun("stun:lobby.example.com:3478")`.
    let config = RtcConfig::new().bind("127.0.0.1:0".parse().unwrap());
    let hosting = NetMessages::host_rtc(host_side.clone(), config.clone(), 8)?;
    // The client knows the host as signaling peer 1 (with the lobby: the
    // room host's member id).
    let joining = NetMessages::join_rtc(client_side.clone(), 1, config)?;
    // ANCHOR_END: webrtc

    // The signaling channel: the host is peer 1, the client peer 2.
    let stop = Arc::new(AtomicBool::new(false));
    let carrier = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                while let Some((_, bytes)) = client_side.next_outgoing() {
                    host_side.deliver(2, &bytes);
                }
                while let Some((_, bytes)) = host_side.next_outgoing() {
                    client_side.deliver(1, &bytes);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        })
    };
    let hosting = std::thread::spawn(move || play(hosting, host));
    let world = play(joining, client);
    hosting.join().expect("the host");
    stop.store(true, Ordering::Relaxed);
    carrier.join().expect("the signaling thread");
    match world.get_resource::<u32>() {
        Some(42) => Ok(()),
        _ => Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "no answer")),
    }
}

/// One headless game with a message endpoint.
fn play(endpoint: NetMessages, system: fn(&mut Ctx)) -> World {
    Game::new("webrtc", 1, 1)
        .messages(endpoint)
        .sim_system(system)
        .sim_system(|_: &mut Ctx| std::thread::sleep(Duration::from_millis(1)))
        .run_headless(5000, |_| {})
}
