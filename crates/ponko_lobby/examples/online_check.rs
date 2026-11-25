//! Checks a lobby setup from two terminals: the guide's "Putting a game
//! online" runs it against `ponko-lobby` and a TURN server on one machine,
//! then against the real server.
//!
//! ```sh
//! cargo run -p ponko_lobby --features webrtc --example online_check -- host relay
//! # It prints a room code; in a second terminal:
//! cargo run -p ponko_lobby --features webrtc --example online_check -- join CODE relay
//! ```
//!
//! `relay` sends the messages through the lobby server's relay
//! (`NetMessages::host_relay`); `turn` connects over WebRTC through the
//! TURN server only (`RtcConfig::relay_only`), so it fails unless TURN
//! works. Settings, from the environment: `LOBBY` (default
//! `127.0.0.1:7700`), `TURN` (`turn:127.0.0.1:3478`), `TURN_USER` (`game`)
//! and `TURN_PASSWORD` (`secret`).

use std::io;
use std::thread::sleep;
use std::time::{Duration, Instant};

use ponko::net::lobby::{LobbyClient, LobbyEvent};
use ponko::net::rtc::RtcConfig;
use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};

const USAGE: &str = "usage: online_check host <relay|turn> | join <CODE> <relay|turn>";

fn setting(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn fail(why: impl Into<String>) -> io::Error {
    io::Error::other(why.into())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("online_check: {e}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (code, path) = match args.as_slice() {
        [mode, path] if mode == "host" => (None, path.as_str()),
        [mode, code, path] if mode == "join" => (Some(code.as_str()), path.as_str()),
        _ => return Err(fail(USAGE)),
    };
    if path != "relay" && path != "turn" {
        return Err(fail(USAGE));
    }
    let mut lobby = LobbyClient::join(setting("LOBBY", "127.0.0.1:7700"))?;
    // Into a room for two: the host makes it, the other player joins its code.
    match code {
        None => lobby.create_room(2)?,
        Some(code) => lobby.join_room(code)?,
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    while !lobby.room().is_some_and(|room| room.is_full()) {
        lobby.poll()?;
        while let Some(event) = lobby.next_event() {
            match event {
                LobbyEvent::Joined(room) if code.is_none() => {
                    println!(
                        "room {}: in another terminal, run this example with: join {} {path}",
                        room.code, room.code
                    );
                }
                LobbyEvent::Refused(error) => return Err(fail(format!("the lobby refused: {error:?}"))),
                _ => {}
            }
        }
        if Instant::now() > deadline {
            return Err(fail("nobody joined the room within two minutes"));
        }
        sleep(Duration::from_millis(5));
    }
    let host = lobby.room().map(|room| room.host).expect("in a room");
    let hosting = lobby.is_host();
    let (mut endpoint, mut signaling_lobby) = if path == "relay" {
        // The session takes the lobby client and polls it itself.
        let endpoint = if hosting {
            NetMessages::host_relay(lobby, 1)?
        } else {
            NetMessages::join_relay(lobby)?
        };
        (endpoint, None)
    } else {
        // WebRTC through the TURN server only; the lobby carries the
        // signaling, so the game keeps polling it.
        let config = RtcConfig::new()
            .turn(
                &setting("TURN", "turn:127.0.0.1:3478"),
                &setting("TURN_USER", "game"),
                &setting("TURN_PASSWORD", "secret"),
            )
            .relay_only();
        let endpoint = if hosting {
            NetMessages::host_rtc(lobby.signaling(), config, 1)?
        } else {
            NetMessages::join_rtc(lobby.signaling(), host.to_bits().into(), config)?
        };
        (endpoint, Some(lobby))
    };
    // The other player says hello, the host answers, and both are done.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(lobby) = &mut signaling_lobby {
            lobby.poll()?;
            while lobby.next_event().is_some() {}
        }
        endpoint.poll()?;
        while let Some(event) = endpoint.next_event() {
            match event {
                MessageEvent::Connected(peer) if !hosting => {
                    println!("connected to the host through the {path}");
                    endpoint.send(peer, Lane::Reliable, b"hello")?;
                }
                MessageEvent::Connected(_) => println!("the other player connected through the {path}"),
                MessageEvent::Message { peer, bytes, .. } if hosting => {
                    println!("received {:?}", String::from_utf8_lossy(&bytes));
                    endpoint.send(peer, Lane::Reliable, b"hello back")?;
                }
                MessageEvent::Message { bytes, .. } => {
                    println!("received {:?}: it works", String::from_utf8_lossy(&bytes));
                    endpoint.disconnect(PeerId::HOST)?;
                    endpoint.poll()?;
                    return Ok(());
                }
                MessageEvent::Disconnected(_, reason) if hosting => {
                    println!("the other player left ({reason}): it works");
                    return Ok(());
                }
                MessageEvent::Disconnected(_, reason) => return Err(fail(format!("the host is gone: {reason}"))),
            }
        }
        if Instant::now() > deadline {
            return Err(fail(format!("no answer through the {path} within 30 seconds")));
        }
        sleep(Duration::from_millis(5));
    }
}
