//! The lobby as a WebRTC signaling and STUN server: two clients in a room
//! connect peer to peer with the engine's WebRTC endpoints (feature
//! `webrtc`), their offers, answers and addresses relayed by the server,
//! their public addresses asked from its STUN responder.
//!
//! The lobby runs in memory on the simulated clock; the WebRTC connections
//! and STUN use real UDP sockets on loopback. Run with
//! `cargo test -p ponko_lobby --features webrtc --test webrtc`.

mod common;

use ponko::net::lobby::LobbyEvent;
use ponko::net::rtc::RtcConfig;
use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};
use ponko_lobby::StunResponder;

use common::{Lobby, STEP, limits};

#[test]
fn room_members_connect_peer_to_peer_with_the_lobby_as_signaling_and_stun_server() {
    let mut lobby = Lobby::new(2, limits());
    let mut stun = StunResponder::bind("127.0.0.1:0").unwrap();
    let config = RtcConfig::new()
        .bind("127.0.0.1:0".parse().unwrap())
        .stun(&format!("stun:{}", stun.local_addr().unwrap()));

    let room = lobby.create(0, 2);
    lobby.join(&[1], &room.code);
    let host_member = lobby.clients[0].me().unwrap();
    assert_eq!(room.host, host_member);

    let mut host = NetMessages::host_rtc(lobby.clients[0].signaling(), config.clone(), 4).unwrap();
    let mut client = NetMessages::join_rtc(lobby.clients[1].signaling(), host_member.to_bits().into(), config).unwrap();

    let mut connected = (false, false);
    let mut got = Vec::new();
    for step in 0..2000 {
        lobby.step();
        stun.poll();
        host.poll_elapsed(STEP).unwrap();
        client.poll_elapsed(STEP).unwrap();
        while let Some(event) = host.next_event() {
            match event {
                MessageEvent::Connected(peer) => {
                    connected.0 = true;
                    // The host knows its client by lobby member id.
                    assert_eq!(
                        host.rtc_signal_peer(peer),
                        lobby.clients[1].me().map(|m| m.to_bits().into())
                    );
                    host.send(peer, Lane::Reliable, b"welcome").unwrap();
                }
                MessageEvent::Message { bytes, .. } => got.push(bytes),
                MessageEvent::Disconnected(..) => panic!("host lost its client at step {step}"),
            }
        }
        while let Some(event) = client.next_event() {
            match event {
                MessageEvent::Connected(PeerId::HOST) => {
                    connected.1 = true;
                    client.send(PeerId::HOST, Lane::Unreliable, b"hello").unwrap();
                }
                MessageEvent::Message { bytes, .. } => got.push(bytes),
                event => panic!("client: {event:?} at step {step}"),
            }
        }
        if got.len() == 2 {
            break;
        }
    }
    assert_eq!(connected, (true, true));
    got.sort();
    assert_eq!(got, [b"hello".to_vec(), b"welcome".to_vec()]);
    assert!(
        stun.stats().answered >= 2,
        "both peers asked the lobby's STUN responder"
    );
    // Signaling went to the endpoints, never to the game as relay events.
    for i in 0..2 {
        assert!(lobby.take(i).iter().all(|e| !matches!(e, LobbyEvent::Relayed { .. })));
    }
    // On loopback STUN reports the socket's own address: no extra candidate.
    assert!(
        lobby.server.stats().relay_out >= 2,
        "the offer and the answer were relayed"
    );
}
