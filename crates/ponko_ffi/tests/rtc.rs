//! WebRTC message endpoints from C, signaled through lobby clients of a
//! real lobby server on this machine: a room's host and member connect
//! peer to peer and exchange messages. Loopback only; run with
//! `cargo test -p ponko_ffi --features webrtc --test rtc`.
// Empty without the feature, so a `--test '*'` glob still matches it
// (a target with `required-features` makes the glob an error).
#![cfg(feature = "webrtc")]

use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use ponko::net::NetMessages;
use ponko_ffi::*;
use ponko_lobby::{LobbyLimits, LobbyServer};

use PonkoMessageEventKind::*;
use PonkoStatus::*;

fn state(lobby: *mut PonkoLobby) -> PonkoLobbyState {
    let mut s = PonkoLobbyState::default();
    assert_eq!(unsafe { ponko_lobby_state(lobby, &mut s) }, PONKO_OK);
    s
}

/// Drains a lobby client's events (signaling is taken before they're made).
fn drain_lobby(lobby: *mut PonkoLobby) -> Vec<PonkoLobbyEventKind> {
    let mut kinds = Vec::new();
    loop {
        let mut e = PonkoLobbyEvent::default();
        let mut found = false;
        assert_eq!(unsafe { ponko_lobby_next_event(lobby, &mut e, &mut found) }, PONKO_OK);
        if !found {
            return kinds;
        }
        kinds.push(e.kind);
    }
}

/// The endpoint's next event: kind, peer and bytes.
fn next(endpoint: *mut PonkoNetMessages) -> Option<(PonkoMessageEventKind, u64, Vec<u8>)> {
    let mut e = PonkoMessageEvent::default();
    let mut found = false;
    assert_eq!(
        unsafe { ponko_net_messages_next_event(endpoint, &mut e, &mut found) },
        PONKO_OK
    );
    found.then(|| {
        let bytes = if e.len == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(e.bytes, e.len) }.to_vec()
        };
        (e.kind, e.peer, bytes)
    })
}

#[test]
fn a_c_game_hosts_a_room_member_over_webrtc() {
    let mut server = LobbyServer::new(LobbyLimits::default());
    server.listen(NetMessages::host("127.0.0.1:0", 8).unwrap());
    let addr = std::ffi::CString::new(server.endpoint(0).unwrap().local_addr().unwrap().to_string()).unwrap();
    let (mut a, mut b) = (null_mut(), null_mut());
    assert_eq!(unsafe { ponko_lobby_join(addr.as_ptr(), &mut a) }, PONKO_OK);
    assert_eq!(unsafe { ponko_lobby_join(addr.as_ptr(), &mut b) }, PONKO_OK);
    let mut endpoints: [*mut PonkoNetMessages; 2] = [null_mut(); 2];
    let mut stage = 0;
    let mut got = Vec::new();
    let start = Instant::now();
    while got.len() < 2 {
        // A hang detector: the loop ends when both are connected.
        assert!(start.elapsed() < Duration::from_secs(60), "stage {stage}, got {got:?}");
        server.poll();
        for lobby in [a, b] {
            assert_eq!(unsafe { ponko_lobby_poll(lobby) }, PONKO_OK);
            drain_lobby(lobby);
        }
        match stage {
            0 if state(a).connected && state(b).connected => {
                assert_eq!(unsafe { ponko_lobby_create_room(a, 2) }, PONKO_OK);
                stage = 1;
            }
            1 if state(a).in_room => {
                let code = state(a).code;
                assert_eq!(unsafe { ponko_lobby_join_room(b, code.as_ptr()) }, PONKO_OK);
                stage = 2;
            }
            2 if state(b).in_room => {
                let bind = c"127.0.0.1:0";
                let host = unsafe { ponko_net_messages_rtc_host(a, null(), bind.as_ptr(), 4, &mut endpoints[0]) };
                assert_eq!(host, PONKO_OK);
                let join = unsafe {
                    ponko_net_messages_rtc_join(b, state(b).host, c"".as_ptr(), bind.as_ptr(), &mut endpoints[1])
                };
                assert_eq!(join, PONKO_OK);
                stage = 3;
            }
            _ => {}
        }
        if stage == 3 {
            for (i, &endpoint) in endpoints.iter().enumerate() {
                assert_eq!(unsafe { ponko_net_messages_poll(endpoint) }, PONKO_OK);
                while let Some((kind, peer, bytes)) = next(endpoint) {
                    match kind {
                        PONKO_MESSAGE_CONNECTED => {
                            let mut member = 0;
                            assert_eq!(
                                unsafe { ponko_net_messages_rtc_peer(endpoint, peer, &mut member) },
                                PONKO_OK
                            );
                            // Each side knows the other by lobby member id.
                            assert_eq!(member, state([b, a][i]).me);
                            let text = if i == 0 {
                                &b"from the host"[..]
                            } else {
                                &b"from the member"[..]
                            };
                            let sent = unsafe {
                                ponko_net_messages_send(
                                    endpoint,
                                    peer,
                                    PonkoLane::PONKO_LANE_RELIABLE,
                                    text.as_ptr(),
                                    text.len(),
                                )
                            };
                            assert_eq!(sent, PONKO_OK);
                        }
                        PONKO_MESSAGE => got.push(bytes),
                        PONKO_MESSAGE_DISCONNECTED => panic!("endpoint {i} lost its peer"),
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    got.sort();
    assert_eq!(got, [b"from the host".to_vec(), b"from the member".to_vec()]);
    unsafe {
        endpoints.iter().for_each(|&e| ponko_net_messages_free(e));
        ponko_lobby_free(a);
        ponko_lobby_free(b);
    }
}

#[test]
fn bad_webrtc_arguments_are_refused() {
    let mut out = null_mut();
    assert_eq!(
        unsafe { ponko_net_messages_rtc_host(null_mut(), null(), null(), 4, &mut out) },
        PONKO_INVALID_ARGUMENT
    );
    assert!(out.is_null());
    let mut lobby = null_mut();
    assert_eq!(
        unsafe { ponko_lobby_join(c"127.0.0.1:9".as_ptr(), &mut lobby) },
        PONKO_OK
    );
    for (stun, bind) in [(c"http://x".as_ptr(), null()), (null(), c"not an address".as_ptr())] {
        assert_eq!(
            unsafe { ponko_net_messages_rtc_host(lobby, stun, bind, 4, &mut out) },
            PONKO_INVALID_ARGUMENT
        );
    }
    assert_eq!(
        unsafe { ponko_net_messages_rtc_join(lobby, 0, null(), null(), &mut out) },
        PONKO_INVALID_ARGUMENT,
        "member ids start at 1"
    );
    let mut member = 7;
    let mut loopback = [null_mut(); 2];
    assert_eq!(
        unsafe { ponko_net_messages_loopback(2, loopback.as_mut_ptr()) },
        PONKO_OK
    );
    assert_eq!(
        unsafe { ponko_net_messages_rtc_peer(loopback[0], 1, &mut member) },
        PONKO_OK
    );
    assert_eq!(member, 0, "not a WebRTC endpoint");
    unsafe {
        loopback.iter().for_each(|&e| ponko_net_messages_free(e));
        ponko_lobby_free(lobby);
    }
}
