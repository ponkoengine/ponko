//! The server's abuse limits: one test each.

mod common;

use std::time::Duration;

use common::{Lobby, STEP, limits};
use ponko::net::lobby::{LeaveReason, LobbyError, LobbyEvent, protocol};
use ponko::net::{Lane, NetMessages, PeerId};
use ponko_lobby::{AddressKey, LobbyLimits};

fn refusals(events: &[LobbyEvent], error: LobbyError) -> usize {
    events.iter().filter(|e| **e == LobbyEvent::Refused(error)).count()
}

#[test]
fn requests_over_the_rate_are_refused_per_address() {
    let mut l = limits();
    l.address_key = AddressKey::Ip; // in memory, every client has 127.0.0.1
    l.request_burst = 4.0;
    l.requests_per_second = 2.0;
    let mut lobby = Lobby::new(2, l);
    // Six bad requests within one step from two clients of one address:
    // four are read (and refused for what they ask), the rest are over
    // the shared budget.
    for _ in 0..3 {
        lobby.clients[0].leave_room().unwrap();
        lobby.clients[1].leave_room().unwrap();
    }
    lobby.steps(5);
    let (a, b) = (lobby.take(0), lobby.take(1));
    let read = refusals(&a, LobbyError::NotInRoom) + refusals(&b, LobbyError::NotInRoom);
    assert_eq!(read, 4);
    assert_eq!(lobby.server.stats().rate_limited, 2);
    // Told once, not once per dropped request.
    assert_eq!(
        refusals(&a, LobbyError::RateLimited) + refusals(&b, LobbyError::RateLimited),
        1
    );
    // A second later the budget has two more.
    lobby.steps(100);
    for _ in 0..3 {
        lobby.clients[0].leave_room().unwrap();
    }
    lobby.steps(5);
    assert_eq!(refusals(&lobby.take(0), LobbyError::NotInRoom), 2);
}

#[test]
fn every_connection_has_its_own_budget_when_keyed_by_connection() {
    let mut l = limits();
    l.request_burst = 4.0;
    l.requests_per_second = 0.001;
    let mut lobby = Lobby::new(2, l);
    for _ in 0..4 {
        lobby.clients[0].leave_room().unwrap();
        lobby.clients[1].leave_room().unwrap();
    }
    lobby.steps(5);
    assert_eq!(refusals(&lobby.take(0), LobbyError::NotInRoom), 4);
    assert_eq!(refusals(&lobby.take(1), LobbyError::NotInRoom), 4);
    assert_eq!(lobby.server.stats().rate_limited, 0);
}

#[test]
fn relay_over_the_byte_rate_is_dropped() {
    let mut l = limits();
    l.relay_burst_bytes = 1000.0;
    l.relay_bytes_per_second = 100.0;
    let mut lobby = Lobby::new(2, l);
    let room = lobby.create(0, 2);
    lobby.join(&[1], &room.code);
    lobby.take(1);
    for _ in 0..5 {
        lobby.clients[0].relay(None, Lane::Reliable, &[7; 300]).unwrap();
    }
    lobby.steps(5);
    // 1000 bytes of budget: three 300-byte messages, then nothing.
    let got = lobby.take(1).len();
    assert_eq!(got, 3);
    let stats = lobby.server.stats();
    assert_eq!((stats.relay_dropped, stats.rate_limited), (2, 2));
    // Three seconds refill 300 bytes: one more message.
    lobby.steps(300);
    lobby.clients[0].relay(None, Lane::Reliable, &[7; 300]).unwrap();
    lobby.steps(5);
    assert_eq!(lobby.take(1).len(), 1);
}

#[test]
fn connections_over_the_per_address_cap_are_refused_and_closed() {
    let mut l = limits();
    l.address_key = AddressKey::Ip;
    l.connections_per_address = 2;
    let mut endpoints = NetMessages::loopback(4).into_iter();
    let mut server = ponko_lobby::LobbyServer::new(l);
    server.listen(endpoints.next().unwrap());
    let mut clients: Vec<_> = endpoints.map(ponko::net::lobby::LobbyClient::new).collect();
    let mut events = vec![Vec::new(); 3];
    for _ in 0..60 {
        server.poll_elapsed(STEP);
        for (c, e) in clients.iter_mut().zip(&mut events) {
            c.poll_elapsed(STEP).unwrap();
            e.extend(std::iter::from_fn(|| c.next_event()));
        }
    }
    let refused: Vec<_> = events
        .iter()
        .filter(|e| e.contains(&LobbyEvent::Refused(LobbyError::TooManyConnections)))
        .collect();
    assert_eq!(refused.len(), 1, "{events:?}");
    assert!(matches!(refused[0].last(), Some(LobbyEvent::Disconnected(_))));
    assert_eq!(server.stats().connections, 2);
    assert_eq!(server.stats().closed_connections, 1);
}

#[test]
fn oversized_requests_are_refused_and_oversized_relays_dropped() {
    let mut l = limits();
    l.max_request_bytes = 200;
    l.max_meta_bytes = 16;
    l.max_relay_bytes = 64;
    let mut lobby = Lobby::new(2, l);
    let room = lobby.create(0, 2);
    lobby.join(&[1], &room.code);
    lobby.take(0);
    lobby.take(1);
    lobby.clients[0].set_meta(&"x".repeat(17)).unwrap();
    lobby.clients[0].set_meta(&"y".repeat(300)).unwrap();
    lobby.steps(5);
    assert_eq!(refusals(&lobby.take(0), LobbyError::TooLarge), 2);
    // The client checks the cap the server announced (the server's own
    // check is the next test).
    assert_eq!(lobby.clients[0].server_limits().max_relay_bytes, 64);
    let e = lobby.clients[0].relay(None, Lane::Reliable, &[0; 65]).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::InvalidInput);
    lobby.clients[0].relay(None, Lane::Reliable, &[0; 64]).unwrap();
    lobby.steps(5);
    assert_eq!(lobby.take(1).len(), 1);
}

#[test]
fn a_raw_client_past_the_relay_cap_is_dropped_by_the_server() {
    let mut l = limits();
    l.max_relay_bytes = 64;
    let mut endpoints = NetMessages::loopback(3).into_iter();
    let mut server = ponko_lobby::LobbyServer::new(l);
    server.listen(endpoints.next().unwrap());
    let mut raw = endpoints.next().unwrap();
    let mut other = ponko::net::lobby::LobbyClient::new(endpoints.next().unwrap());
    let step =
        |server: &mut ponko_lobby::LobbyServer, raw: &mut NetMessages, other: &mut ponko::net::lobby::LobbyClient| {
            server.poll_elapsed(STEP);
            raw.poll_elapsed(STEP).unwrap();
            other.poll_elapsed(STEP).unwrap();
        };
    for _ in 0..20 {
        step(&mut server, &mut raw, &mut other);
    }
    // The raw client makes a room by hand; the other joins it.
    raw.send(
        PeerId::HOST,
        Lane::Reliable,
        &protocol::encode_control(&protocol::Request::Create {
            size: 2,
            meta: String::new(),
        }),
    )
    .unwrap();
    for _ in 0..10 {
        step(&mut server, &mut raw, &mut other);
    }
    let code = server.rooms().next().unwrap().code.clone();
    other.join_room(&code).unwrap();
    for _ in 0..10 {
        step(&mut server, &mut raw, &mut other);
    }
    while other.next_event().is_some() {}
    let mut frame = Vec::new();
    protocol::encode_relay(protocol::EVERYONE, &[1; 65], &mut frame);
    raw.send(PeerId::HOST, Lane::Reliable, &frame).unwrap();
    protocol::encode_relay(protocol::EVERYONE, &[1; 64], &mut frame);
    raw.send(PeerId::HOST, Lane::Reliable, &frame).unwrap();
    for _ in 0..10 {
        step(&mut server, &mut raw, &mut other);
    }
    let got: Vec<_> = std::iter::from_fn(|| other.next_event()).collect();
    assert!(
        matches!(&got[..], [LobbyEvent::Relayed { bytes, .. }] if bytes.len() == 64),
        "{got:?}"
    );
    assert_eq!(server.stats().relay_dropped, 1);
}

#[test]
fn garbage_is_refused_without_closing_the_connection() {
    let mut endpoints = NetMessages::loopback(2).into_iter();
    let mut server = ponko_lobby::LobbyServer::new(limits());
    server.listen(endpoints.next().unwrap());
    let mut raw = endpoints.next().unwrap();
    for _ in 0..20 {
        server.poll_elapsed(STEP);
        raw.poll_elapsed(STEP).unwrap();
    }
    for bytes in [&b"\x01{not json"[..], b"\x01{\"op\":\"fly\"}", b"\x09", b""] {
        raw.send(PeerId::HOST, Lane::Reliable, bytes).unwrap();
    }
    for _ in 0..10 {
        server.poll_elapsed(STEP);
        raw.poll_elapsed(STEP).unwrap();
    }
    let refused = std::iter::from_fn(|| raw.next_event())
        .filter_map(|e| match e {
            ponko::net::MessageEvent::Message { bytes, .. } => {
                protocol::decode::<protocol::Reply>(&bytes).and_then(|f| match f {
                    protocol::Frame::Control(protocol::Reply::Refused { error }) => Some(error),
                    _ => None,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(refused, [LobbyError::BadRequest; 4]);
    assert_eq!(server.stats().connections, 1);
}

#[test]
fn room_sizes_outside_the_cap_are_refused() {
    let mut l = limits();
    l.max_room_size = 4;
    let mut lobby = Lobby::new(1, l);
    for size in [0, 1, 5, 500] {
        lobby.clients[0].create_room(size).unwrap();
    }
    lobby.clients[0].queue("q", 9, 0).unwrap();
    lobby.steps(5);
    assert_eq!(refusals(&lobby.take(0), LobbyError::BadSize), 5);
    assert_eq!(lobby.clients[0].server_limits().max_room_size, 4);
}

#[test]
fn rooms_over_the_server_cap_are_refused() {
    let mut l = limits();
    l.max_rooms = 2;
    let mut lobby = Lobby::new(3, l);
    lobby.create(0, 2);
    lobby.create(1, 2);
    lobby.clients[2].create_room(2).unwrap();
    lobby.steps(5);
    assert_eq!(lobby.take(2), [LobbyEvent::Refused(LobbyError::ServerFull)]);
}

#[test]
fn idle_connections_are_closed() {
    let mut l = limits();
    l.idle_timeout = Duration::from_secs(2);
    let mut lobby = Lobby::new(2, l);
    // Client 1 waits in a queue: not idle.
    lobby.clients[1].queue("q", 2, 0).unwrap();
    lobby.steps(150);
    assert_eq!(lobby.server.stats().connections, 2, "1.5 s is under the timeout");
    lobby.steps(100);
    let events = lobby.take(0);
    assert_eq!(events[0], LobbyEvent::Left(LeaveReason::Idle));
    assert!(matches!(events.last(), Some(LobbyEvent::Disconnected(_))));
    assert_eq!(lobby.server.stats().connections, 1);
    assert!(lobby.clients[1].is_queued());
    assert_eq!(lobby.clients[0].server_limits().idle_timeout_ms, 2000);
}

#[test]
fn idle_rooms_are_closed_and_traffic_keeps_them_open() {
    let mut l = limits();
    l.room_idle_timeout = Duration::from_secs(3);
    let mut lobby = Lobby::new(4, l);
    let quiet = lobby.create(0, 2);
    lobby.join(&[1], &quiet.code);
    let busy = lobby.create(2, 2);
    lobby.join(&[3], &busy.code);
    (0..4).for_each(|i| drop(lobby.take(i)));
    for _ in 0..40 {
        lobby.clients[2].relay(None, Lane::Unreliable, b"tick").unwrap();
        lobby.steps(10);
    }
    assert!(lobby.server.room(&quiet.code).is_none());
    assert!(lobby.server.room(&busy.code).is_some());
    for i in [0, 1] {
        assert_eq!(lobby.take(i), [LobbyEvent::Left(LeaveReason::Idle)]);
    }
}

#[test]
fn default_limits_are_the_documented_ones() {
    let l = LobbyLimits::default();
    assert_eq!(l.address_key, AddressKey::Ip);
    assert_eq!(l.connections_per_address, 16);
    assert_eq!((l.requests_per_second, l.request_burst), (10.0, 20.0));
    assert_eq!((l.relay_bytes_per_second, l.relay_burst_bytes), (262_144.0, 524_288.0));
    assert_eq!((l.max_request_bytes, l.max_relay_bytes), (1024, 4096));
    assert_eq!((l.max_room_size, l.max_meta_bytes, l.max_rooms), (16, 256, 10_000));
    assert_eq!(l.idle_timeout, Duration::from_secs(60));
    assert_eq!(l.room_idle_timeout, Duration::from_secs(600));
}
