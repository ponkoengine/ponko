//! Rooms, codes, host migration, matchmaking and the relay.

mod common;

use std::time::Duration;

use common::{Lobby, limits};
use ponko::net::Lane;
use ponko::net::lobby::{LeaveReason, LobbyError, LobbyEvent, MemberId, RoomInfo};
use ponko_lobby::{LobbyServer, Matchmaker, Waiting};

#[test]
fn a_created_room_has_a_code_others_join_in_any_case() {
    let mut l = Lobby::new(3, limits());
    let room = l.create(0, 3);
    assert_eq!(room.code.len(), 6);
    assert_eq!(room.host, l.clients[0].me().unwrap());
    assert!(l.clients[0].is_host());
    l.join(&[1], &room.code);
    l.join(&[2], &room.code.to_lowercase());
    let server_room = l.server.room(&room.code).unwrap();
    let ids: Vec<_> = l.clients.iter().map(|c| c.me().unwrap()).collect();
    assert_eq!(server_room.members, ids, "members in the order they came in");
    for client in &l.clients {
        assert_eq!(client.room(), Some(server_room), "every member sees the same room");
    }
    assert!(!l.clients[1].is_host());
}

#[test]
fn unknown_codes_and_full_rooms_are_refused() {
    let mut l = Lobby::new(3, limits());
    l.clients[1].join_room("ZZZZZZ").unwrap();
    l.steps(10);
    assert_eq!(l.take(1), [LobbyEvent::Refused(LobbyError::RoomNotFound)]);

    let room = l.create(0, 2);
    l.join(&[1], &room.code);
    l.clients[2].join_room(&room.code).unwrap();
    l.steps(10);
    assert_eq!(l.take(2), [LobbyEvent::Refused(LobbyError::RoomFull)]);
    assert!(l.clients[2].room().is_none());
}

#[test]
fn a_member_in_a_room_or_queue_cannot_enter_another() {
    let mut l = Lobby::new(2, limits());
    let room = l.create(0, 2);
    l.clients[0].create_room(2).unwrap();
    l.clients[0].queue("duel", 2, 0).unwrap();
    l.clients[1].queue("duel", 3, 0).unwrap();
    l.steps(10);
    l.clients[1].join_room(&room.code).unwrap();
    l.steps(10);
    assert_eq!(
        l.take(0),
        [
            LobbyEvent::Refused(LobbyError::AlreadyInRoom),
            LobbyEvent::Refused(LobbyError::AlreadyInRoom)
        ]
    );
    assert_eq!(
        l.take(1),
        [LobbyEvent::Queued, LobbyEvent::Refused(LobbyError::AlreadyQueued)]
    );
}

#[test]
fn members_hear_joins_and_leaves() {
    let mut l = Lobby::new(3, limits());
    let room = l.create(0, 3);
    l.join(&[1], &room.code);
    let b = l.clients[1].me().unwrap();
    assert_eq!(l.take(0), [LobbyEvent::MemberJoined(b)]);
    l.take(1);
    l.join(&[2], &room.code);
    let c = l.clients[2].me().unwrap();
    l.take(2);
    assert_eq!(l.take(0), [LobbyEvent::MemberJoined(c)]);
    assert_eq!(l.take(1), [LobbyEvent::MemberJoined(c)]);

    l.clients[1].leave_room().unwrap();
    l.steps(10);
    assert_eq!(l.take(1), [LobbyEvent::Left(LeaveReason::Requested)]);
    assert_eq!(l.take(0), [LobbyEvent::MemberLeft(b)]);
    assert_eq!(l.take(2), [LobbyEvent::MemberLeft(b)]);
    assert_eq!(l.clients[0].room().unwrap().members.len(), 2);
    assert!(l.clients[1].room().is_none());
    l.clients[1].leave_room().unwrap();
    l.steps(10);
    assert_eq!(l.take(1), [LobbyEvent::Refused(LobbyError::NotInRoom)]);
}

#[test]
fn when_the_host_leaves_the_longest_present_member_hosts() {
    let mut l = Lobby::new(3, limits());
    let room = l.create(0, 3);
    l.join(&[1], &room.code);
    l.join(&[2], &room.code);
    (0..3).for_each(|i| drop(l.take(i)));
    let (a, b) = (l.clients[0].me().unwrap(), l.clients[1].me().unwrap());
    l.clients[0].leave_room().unwrap();
    l.steps(10);
    for i in [1, 2] {
        assert_eq!(l.take(i), [LobbyEvent::MemberLeft(a), LobbyEvent::HostChanged(b)]);
    }
    assert!(l.clients[1].is_host());
    assert_eq!(l.server.room(&room.code).unwrap().host, b);
    assert_eq!(l.server.stats().host_migrations, 1);
}

#[test]
fn a_host_that_loses_its_connection_is_replaced_too() {
    let mut l = Lobby::new(2, limits());
    let room = l.create(0, 2);
    l.join(&[1], &room.code);
    l.take(1);
    l.clients[0].disconnect().unwrap();
    l.steps(10);
    let b = l.clients[1].me().unwrap();
    assert!(l.take(1).contains(&LobbyEvent::HostChanged(b)));
    assert!(l.clients[1].is_host());
    assert_eq!(l.server.stats().connections, 1);
}

#[test]
fn the_host_migration_hook_picks_the_next_host() {
    // The newest member hosts next.
    let server = LobbyServer::new(limits())
        .seed(1)
        .on_host_left(|room: &RoomInfo| room.members.last().copied());
    let mut l = Lobby::with(3, server);
    let room = l.create(0, 3);
    l.join(&[1], &room.code);
    l.join(&[2], &room.code);
    l.clients[0].leave_room().unwrap();
    l.steps(10);
    let c = l.clients[2].me().unwrap();
    assert!(l.clients[2].is_host());
    assert!(l.take(1).contains(&LobbyEvent::HostChanged(c)));
}

#[test]
fn a_hook_without_a_next_host_closes_the_room() {
    let server = LobbyServer::new(limits()).on_host_left(|_: &RoomInfo| None);
    let mut l = Lobby::with(3, server);
    let room = l.create(0, 3);
    l.join(&[1, 2], &room.code);
    (0..3).for_each(|i| drop(l.take(i)));
    l.clients[0].leave_room().unwrap();
    l.steps(10);
    for i in [1, 2] {
        assert!(l.take(i).ends_with(&[LobbyEvent::Left(LeaveReason::RoomClosed)]));
        assert!(l.clients[i].room().is_none());
    }
    assert!(l.server.room(&room.code).is_none());
    assert_eq!(l.server.stats().rooms_closed, 1);
}

#[test]
fn a_room_closes_when_its_last_member_leaves() {
    let mut l = Lobby::new(1, limits());
    let room = l.create(0, 2);
    l.clients[0].leave_room().unwrap();
    l.steps(10);
    assert!(l.server.room(&room.code).is_none());
    assert_eq!(l.server.stats().rooms, 0);
}

#[test]
fn only_the_host_sets_the_room_text_and_every_member_sees_it() {
    let mut l = Lobby::new(2, limits());
    let room = l.create(0, 2);
    l.join(&[1], &room.code);
    l.take(0);
    l.take(1);
    l.clients[1].set_meta("mine").unwrap();
    l.clients[0].set_meta("udp=203.0.113.5:7777").unwrap();
    l.steps(10);
    assert_eq!(
        l.take(1),
        [
            LobbyEvent::Refused(LobbyError::NotHost),
            LobbyEvent::MetaChanged("udp=203.0.113.5:7777".into())
        ]
    );
    assert_eq!(l.clients[1].room().unwrap().meta, "udp=203.0.113.5:7777");
    assert_eq!(l.server.room(&room.code).unwrap().meta, "udp=203.0.113.5:7777");
}

#[test]
fn relay_bytes_reach_one_member_or_the_whole_room_unchanged() {
    let mut l = Lobby::new(3, limits());
    let room = l.create(0, 3);
    l.join(&[1, 2], &room.code);
    (0..3).for_each(|i| drop(l.take(i)));
    let ids: Vec<MemberId> = l.clients.iter().map(|c| c.me().unwrap()).collect();
    // Bytes the server can't read as anything, sent unchanged.
    let opaque = [0u8, 1, 2, 255, 254, b'{', 0];
    l.clients[0].relay(None, Lane::Reliable, &opaque).unwrap();
    l.clients[1].relay(Some(ids[2]), Lane::Unreliable, b"only you").unwrap();
    l.steps(10);
    let from_a = LobbyEvent::Relayed {
        from: ids[0],
        lane: Lane::Reliable,
        bytes: opaque.to_vec(),
    };
    assert_eq!(l.take(0), []);
    assert_eq!(l.take(1), std::slice::from_ref(&from_a));
    let c = l.take(2);
    assert_eq!(c.len(), 2);
    assert!(c.contains(&from_a));
    assert!(c.contains(&LobbyEvent::Relayed {
        from: ids[1],
        lane: Lane::Unreliable,
        bytes: b"only you".to_vec(),
    }));
    let stats = l.server.stats();
    assert_eq!((stats.relay_in, stats.relay_out), (2, 3));
    assert_eq!(stats.relay_bytes_out, 2 * opaque.len() as u64 + 8);
}

#[test]
fn relay_never_crosses_rooms() {
    let mut l = Lobby::new(4, limits());
    let first = l.create(0, 2);
    l.join(&[1], &first.code);
    let second = l.create(2, 2);
    l.join(&[3], &second.code);
    (0..4).for_each(|i| drop(l.take(i)));
    let other = l.clients[3].me().unwrap();
    l.clients[0].relay(None, Lane::Reliable, b"room one").unwrap();
    l.clients[0].relay(Some(other), Lane::Reliable, b"not yours").unwrap();
    l.steps(10);
    assert_eq!(l.take(1).len(), 1);
    assert_eq!(l.take(2), []);
    assert_eq!(l.take(3), []);
    assert_eq!(l.server.stats().relay_dropped, 1);
}

#[test]
fn relay_needs_a_room() {
    let mut l = Lobby::new(1, limits());
    let e = l.clients[0].relay(None, Lane::Reliable, b"x").unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::NotConnected);
}

#[test]
fn matchmaking_fills_rooms_by_size_first_come_first_served() {
    let mut l = Lobby::new(5, limits());
    for i in 0..5 {
        l.clients[i].queue("duel", 2, 0).unwrap();
        l.step();
    }
    l.steps(10);
    assert!(l.clients[4].is_queued(), "the fifth waits for a partner");
    for pair in [[0, 1], [2, 3]] {
        let room = l.clients[pair[0]].room().unwrap().clone();
        assert_eq!(
            room.members,
            [l.clients[pair[0]].me().unwrap(), l.clients[pair[1]].me().unwrap()]
        );
        assert!(l.clients[pair[0]].is_host());
        assert_eq!(l.clients[pair[1]].room(), Some(&room));
        assert!(matches!(
            l.take(pair[1])[..],
            [LobbyEvent::Queued, LobbyEvent::Joined(_)]
        ));
    }
    let stats = l.server.stats();
    assert_eq!((stats.matches_made, stats.queued), (2, 1));
}

#[test]
fn queues_with_another_mode_or_size_never_meet() {
    let mut l = Lobby::new(3, limits());
    l.clients[0].queue("duel", 2, 0).unwrap();
    l.clients[1].queue("race", 2, 0).unwrap();
    l.clients[2].queue("duel", 3, 0).unwrap();
    l.steps(20);
    assert!(l.clients.iter().all(|c| c.is_queued() && c.room().is_none()));
}

#[test]
fn a_cancelled_player_leaves_the_queue() {
    let mut l = Lobby::new(2, limits());
    l.clients[0].queue("duel", 2, 0).unwrap();
    l.steps(5);
    l.clients[0].cancel_queue().unwrap();
    l.steps(5);
    assert_eq!(l.take(0), [LobbyEvent::Queued, LobbyEvent::Unqueued]);
    l.clients[1].queue("duel", 2, 0).unwrap();
    l.steps(20);
    assert!(l.clients[1].is_queued(), "no match with a player who left");
    assert!(l.clients[0].room().is_none());
}

/// Pairs players whose ratings are within 100.
struct CloseRatings;
impl Matchmaker for CloseRatings {
    fn form(&mut self, _mode: &str, size: u16, waiting: &[Waiting], _now: Duration, groups: &mut Vec<Vec<usize>>) {
        let mut used = vec![false; waiting.len()];
        for i in 0..waiting.len() {
            if used[i] {
                continue;
            }
            let group: Vec<usize> = (i..waiting.len())
                .filter(|&j| !used[j] && (waiting[j].rating - waiting[i].rating).abs() <= 100)
                .take(usize::from(size))
                .collect();
            if group.len() == usize::from(size) {
                group.iter().for_each(|&k| used[k] = true);
                groups.push(group);
            }
        }
    }
}

#[test]
fn a_game_matchmaker_groups_players_by_its_own_rule() {
    let server = LobbyServer::new(limits()).matchmaker(CloseRatings);
    let mut l = Lobby::with(4, server);
    for (i, rating) in [1000, 2000, 1050, 2080].into_iter().enumerate() {
        l.clients[i].queue("ranked", 2, rating).unwrap();
        l.step();
    }
    l.steps(20);
    let code = |l: &Lobby, i: usize| l.clients[i].room().unwrap().code.clone();
    assert_eq!(code(&l, 0), code(&l, 2));
    assert_eq!(code(&l, 1), code(&l, 3));
    assert_ne!(code(&l, 0), code(&l, 1));
}

/// A rule that returns nonsense: the server skips it.
struct Broken;
impl Matchmaker for Broken {
    fn form(&mut self, _: &str, size: u16, waiting: &[Waiting], _: Duration, groups: &mut Vec<Vec<usize>>) {
        groups.push(vec![0, 0]);
        groups.push(vec![0, waiting.len() + 5]);
        groups.push(vec![0]);
        groups.push((0..=usize::from(size)).collect()); // one too many
    }
}

#[test]
fn invalid_groups_from_a_matchmaker_are_skipped() {
    let mut l = Lobby::with(3, LobbyServer::new(limits()).matchmaker(Broken));
    for client in &mut l.clients {
        client.queue("x", 2, 0).unwrap();
    }
    l.steps(20);
    assert!(l.clients.iter().all(|c| c.is_queued()));
    assert_eq!(l.server.stats().rooms, 0);
}

#[test]
fn shutdown_tells_members_and_closes_every_connection() {
    let mut l = Lobby::new(3, limits());
    let room = l.create(0, 2);
    l.join(&[1], &room.code);
    l.take(0);
    l.take(1);
    l.server.shutdown();
    l.steps(40);
    assert_eq!(l.server.stats().connections, 0);
    for i in [0, 1] {
        let events = l.take(i);
        assert_eq!(events[0], LobbyEvent::Left(LeaveReason::ServerStopping));
        assert!(matches!(events.last(), Some(LobbyEvent::Disconnected(_))));
    }
    assert!(matches!(l.take(2)[..], [LobbyEvent::Disconnected(_)]));
}

#[test]
fn the_load_test_fills_every_room_and_delivers_every_relay_message() {
    let report = ponko_lobby::load::run(&ponko_lobby::load::LoadConfig {
        rooms: 40,
        room_size: 3,
        matched_rooms: 10,
        relay_rounds: 5,
        payload_bytes: 48,
    })
    .unwrap();
    assert_eq!(report.clients, 150);
    assert_eq!(report.rooms_created, 50);
    assert_eq!(report.server.matches_made, 10);
    assert_eq!(report.relay_delivered, 150 * 2 * 5);
    assert_eq!(report.relay_bytes, 150 * 2 * 5 * 48);
    assert_eq!(report.migrations_seen, 50);
    assert_eq!(report.server.relay_dropped, 0);
}
