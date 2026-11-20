//! The lobby client from C, against a real lobby server over UDP on this
//! machine: rooms and codes, members, room text, the relay, host migration,
//! matchmaking, refusals, shutdown and argument checks.
use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use ponko::net::NetMessages;
use ponko_ffi::*;
use ponko_lobby::{LobbyLimits, LobbyServer};

use PonkoLobbyEventKind::*;
use PonkoStatus::*;

type Lobby = *mut PonkoLobby;

/// An event with its bytes and text copied out.
#[derive(Debug, Clone, PartialEq)]
struct Event {
    kind: PonkoLobbyEventKind,
    member: u32,
    text: String,
    bytes: Vec<u8>,
    lane: PonkoLane,
    error: PonkoLobbyError,
    reason: PonkoLobbyLeaveReason,
}

unsafe fn next(lobby: Lobby) -> Option<Event> {
    unsafe {
        let mut e = PonkoLobbyEvent::default();
        let mut found = false;
        assert_eq!(ponko_lobby_next_event(lobby, &mut e, &mut found), PONKO_OK);
        found.then(|| Event {
            kind: e.kind,
            member: e.member,
            text: CStr::from_ptr(e.text).to_str().unwrap().to_owned(),
            bytes: if e.len == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(e.bytes, e.len).to_vec()
            },
            lane: e.lane,
            error: e.error,
            reason: e.reason,
        })
    }
}

fn state(lobby: Lobby) -> PonkoLobbyState {
    let mut s = PonkoLobbyState::default();
    assert_eq!(unsafe { ponko_lobby_state(lobby, &mut s) }, PONKO_OK);
    s
}

fn code(s: &PonkoLobbyState) -> String {
    unsafe { CStr::from_ptr(s.code.as_ptr()) }.to_str().unwrap().to_owned()
}

struct World {
    server: LobbyServer,
    clients: Vec<Lobby>,
    events: Vec<Vec<Event>>,
}

impl World {
    fn new(n: usize) -> Self {
        let mut server = LobbyServer::new(LobbyLimits::default());
        server.listen(NetMessages::host("127.0.0.1:0", 8).unwrap());
        let addr = CString::new(server.endpoint(0).unwrap().local_addr().unwrap().to_string()).unwrap();
        let clients = (0..n)
            .map(|_| {
                let mut lobby = null_mut();
                assert_eq!(unsafe { ponko_lobby_join(addr.as_ptr(), &mut lobby) }, PONKO_OK);
                lobby
            })
            .collect();
        let mut world = Self {
            server,
            clients,
            events: vec![Vec::new(); n],
        };
        world.until(|w| w.clients.iter().all(|&c| state(c).connected));
        world
    }

    /// Polls the server and every client until `done` (at most 5 s).
    fn until(&mut self, mut done: impl FnMut(&mut Self) -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            self.server.poll();
            for (i, &c) in self.clients.iter().enumerate() {
                assert_eq!(unsafe { ponko_lobby_poll(c) }, PONKO_OK);
                while let Some(e) = unsafe { next(c) } {
                    self.events[i].push(e);
                }
            }
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("not done within 5 s: {:?}", self.events);
    }

    fn take(&mut self, i: usize) -> Vec<Event> {
        std::mem::take(&mut self.events[i])
    }
}

impl Drop for World {
    fn drop(&mut self) {
        for &c in &self.clients {
            unsafe { ponko_lobby_free(c) };
        }
    }
}

#[test]
fn a_c_game_makes_joins_and_plays_in_a_room() {
    let mut w = World::new(3);
    let (a, b, c) = (w.clients[0], w.clients[1], w.clients[2]);
    let connected = w.take(0);
    assert_eq!(connected[0].kind, PONKO_LOBBY_CONNECTED);
    assert_eq!(connected[0].member, state(a).me);

    // A makes a room; the event carries its code.
    assert_eq!(unsafe { ponko_lobby_create_room(a, 3) }, PONKO_OK);
    w.until(|w| w.events[0].iter().any(|e| e.kind == PONKO_LOBBY_JOINED));
    let joined = w.take(0).remove(0);
    assert_eq!(joined.text.len(), 6);
    assert_eq!(joined.member, state(a).me, "the creator hosts");
    let room = CString::new(joined.text.to_lowercase()).unwrap();
    for client in [b, c] {
        assert_eq!(unsafe { ponko_lobby_join_room(client, room.as_ptr()) }, PONKO_OK);
    }
    w.until(|_| state(b).in_room && state(c).in_room && state(a).members == 3);
    let s = state(b);
    assert_eq!((s.size, s.members, s.host, s.is_host), (3, 3, state(a).me, false));
    assert_eq!(code(&s), joined.text);
    assert!(state(a).is_host);
    assert!(s.max_relay_bytes > 0);

    // Members, in the order they came in.
    let mut count = 0;
    assert_eq!(unsafe { ponko_lobby_members(b, null_mut(), 0, &mut count) }, PONKO_OK);
    assert_eq!(count, 3);
    let mut small = [0u32; 2];
    assert_eq!(
        unsafe { ponko_lobby_members(b, small.as_mut_ptr(), 2, &mut count) },
        PONKO_INVALID_ARGUMENT
    );
    let mut members = [0u32; 3];
    assert_eq!(
        unsafe { ponko_lobby_members(b, members.as_mut_ptr(), 3, &mut count) },
        PONKO_OK
    );
    assert_eq!(members[0], state(a).me);

    // The host's text reaches everyone.
    assert_eq!(
        unsafe { ponko_lobby_set_meta(a, c"udp=203.0.113.5:7777".as_ptr()) },
        PONKO_OK
    );
    w.until(|w| w.events[1].iter().any(|e| e.kind == PONKO_LOBBY_META_CHANGED));
    let mut buf = [0 as c_char; 64];
    let mut len = 0;
    assert_eq!(
        unsafe { ponko_lobby_room_meta(c, buf.as_mut_ptr(), buf.len(), &mut len) },
        PONKO_OK
    );
    assert_eq!(
        unsafe { CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap(),
        "udp=203.0.113.5:7777"
    );
    assert_eq!(len, 20);
    (0..3).for_each(|i| drop(w.take(i)));

    // The relay: to everyone (0), and to one member.
    let bytes = [0u8, 1, 2, 255];
    assert_eq!(
        unsafe { ponko_lobby_relay(a, 0, PonkoLane::PONKO_LANE_RELIABLE, bytes.as_ptr(), bytes.len()) },
        PONKO_OK
    );
    assert_eq!(
        unsafe { ponko_lobby_relay(b, state(c).me, PonkoLane::PONKO_LANE_UNRELIABLE, b"hi".as_ptr(), 2) },
        PONKO_OK
    );
    w.until(|w| w.events[2].len() == 2 && w.events[1].len() == 1);
    let to_b = w.take(1).remove(0);
    assert_eq!(
        (to_b.kind, to_b.member, &to_b.bytes[..]),
        (PONKO_LOBBY_RELAYED, state(a).me, &bytes[..])
    );
    let to_c = w.take(2);
    assert!(
        to_c.iter()
            .any(|e| e.bytes == b"hi" && e.member == state(b).me && e.lane == PonkoLane::PONKO_LANE_UNRELIABLE)
    );

    // The host leaves: B, there longest, hosts.
    let old_host = state(a).me;
    assert_eq!(unsafe { ponko_lobby_leave_room(a) }, PONKO_OK);
    w.until(|w| w.events[2].iter().any(|e| e.kind == PONKO_LOBBY_HOST_CHANGED));
    let events = w.take(2);
    assert!(
        events
            .iter()
            .any(|e| e.kind == PONKO_LOBBY_MEMBER_LEFT && e.member == old_host)
    );
    assert!(
        events
            .iter()
            .any(|e| e.kind == PONKO_LOBBY_HOST_CHANGED && e.member == state(b).me)
    );
    assert!(state(b).is_host);
    let left = w.take(0);
    assert!(
        left.iter()
            .any(|e| e.kind == PONKO_LOBBY_LEFT && e.reason == PonkoLobbyLeaveReason::PONKO_LOBBY_LEFT_REQUESTED)
    );
    assert!(!state(a).in_room);

    // A refusal.
    assert_eq!(unsafe { ponko_lobby_join_room(a, c"ZZZZZZ".as_ptr()) }, PONKO_OK);
    w.until(|w| !w.events[0].is_empty());
    let refused = w.take(0).remove(0);
    assert_eq!(
        (refused.kind, refused.error),
        (PONKO_LOBBY_REFUSED, PonkoLobbyError::PONKO_LOBBY_ROOM_NOT_FOUND)
    );

    // Matchmaking: queue, then cancel.
    assert_eq!(unsafe { ponko_lobby_queue(a, c"duel".as_ptr(), 2, 1500) }, PONKO_OK);
    w.until(|_| state(a).queued);
    assert_eq!(unsafe { ponko_lobby_cancel_queue(a) }, PONKO_OK);
    w.until(|_| !state(a).queued);
    let kinds: Vec<_> = w.take(0).iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [PONKO_LOBBY_QUEUED, PONKO_LOBBY_UNQUEUED]);

    // The server stops: members hear why, then the connection closes.
    w.server.shutdown();
    w.until(|w| w.events[1].iter().any(|e| e.kind == PONKO_LOBBY_DISCONNECTED));
    let events = w.take(1);
    assert!(
        events
            .iter()
            .any(|e| e.kind == PONKO_LOBBY_LEFT && e.reason == PonkoLobbyLeaveReason::PONKO_LOBBY_LEFT_SERVER_STOPPING)
    );
    assert!(!events.last().unwrap().text.is_empty(), "a disconnect says why");
}

#[test]
fn matchmaking_and_disconnect_from_c() {
    let mut w = World::new(2);
    let (a, b) = (w.clients[0], w.clients[1]);
    for client in [a, b] {
        assert_eq!(unsafe { ponko_lobby_queue(client, c"duel".as_ptr(), 2, 0) }, PONKO_OK);
    }
    w.until(|_| state(a).in_room && state(b).in_room);
    assert_eq!(code(&state(a)), code(&state(b)));
    assert_eq!(unsafe { ponko_lobby_disconnect(a) }, PONKO_OK);
    w.until(|w| w.server.stats().connections == 1 && state(b).is_host);
    assert_eq!(
        unsafe { ponko_lobby_disconnect(a) },
        PONKO_INVALID_ARGUMENT,
        "already gone"
    );
    assert_eq!(
        unsafe { ponko_lobby_create_room(a, 2) },
        PONKO_INVALID_ARGUMENT,
        "the connection is gone"
    );
}

#[test]
fn bad_arguments_are_errors_not_crashes() {
    let mut lobby = null_mut();
    unsafe {
        assert_eq!(ponko_lobby_join(null(), &mut lobby), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_lobby_join(c"not an address".as_ptr(), &mut lobby),
            PONKO_INVALID_ARGUMENT
        );
        assert!(lobby.is_null());
        assert_eq!(
            ponko_lobby_join(c"127.0.0.1:9".as_ptr(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_lobby_join(c"127.0.0.1:9".as_ptr(), &mut lobby), PONKO_OK);
        // Not in a room: relaying is refused at once.
        assert_eq!(
            ponko_lobby_relay(lobby, 0, PonkoLane::PONKO_LANE_RELIABLE, b"x".as_ptr(), 1),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_lobby_relay(lobby, 0, PonkoLane::PONKO_LANE_RELIABLE, null(), 3),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_lobby_join_room(lobby, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_lobby_queue(lobby, null(), 2, 0), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_lobby_set_meta(lobby, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_lobby_next_event(lobby, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_lobby_state(lobby, null_mut()), PONKO_INVALID_ARGUMENT);
        let mut len = 0;
        assert_eq!(
            ponko_lobby_room_meta(lobby, null_mut(), 4, &mut len),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_lobby_room_meta(lobby, null_mut(), 0, &mut len), PONKO_OK);
        assert_eq!(len, 0);
        assert_eq!(
            ponko_lobby_members(lobby, null_mut(), 0, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        let s = state(lobby);
        assert!(!s.connected && s.me == 0 && !s.in_room);
        ponko_lobby_free(lobby);
        ponko_lobby_free(null_mut());
    }
}

/// Without the `webrtc` feature the WebRTC calls fail and name it (with the
/// feature, `tests/rtc.rs` runs the flow). The native test programs run both
/// ways: `examples/cpp/CMakeLists.txt`, option `PONKO_WEBRTC`.
#[cfg(not(feature = "webrtc"))]
#[test]
fn without_the_webrtc_feature_the_calls_name_it() {
    unsafe {
        let mut lobby = null_mut();
        assert_eq!(ponko_lobby_join(c"127.0.0.1:9".as_ptr(), &mut lobby), PONKO_OK);
        let mut endpoint = 1 as *mut PonkoNetMessages;
        assert_eq!(
            ponko_net_messages_rtc_host(lobby, null(), null(), 4, &mut endpoint),
            PONKO_INVALID_ARGUMENT
        );
        assert!(endpoint.is_null());
        let message = CStr::from_ptr(ponko_last_error()).to_string_lossy().into_owned();
        assert!(message.contains("without the `webrtc` feature"), "{message}");
        endpoint = 1 as *mut PonkoNetMessages;
        assert_eq!(
            ponko_net_messages_rtc_join(lobby, 1, null(), null(), &mut endpoint),
            PONKO_INVALID_ARGUMENT
        );
        assert!(endpoint.is_null());
        ponko_lobby_free(lobby);
    }
}
