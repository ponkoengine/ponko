//! Message endpoints from C: in-memory loopback pairs and real UDP on the
//! local machine, with connection and message events, lanes, statistics,
//! disconnects and timeouts.
use ponko_ffi::*;
use std::ffi::CStr;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use PonkoLane::*;
use PonkoMessageEventKind::*;
use PonkoStatus::*;

const OK: PonkoStatus = PONKO_OK;
const BAD: PonkoStatus = PONKO_INVALID_ARGUMENT;

type Endpoint = *mut PonkoNetMessages;

/// An event with its bytes and reason copied out, so it outlives the next call.
#[derive(Debug, PartialEq)]
enum Event {
    Connected(u64),
    Disconnected(u64, String),
    Message(u64, PonkoLane, Vec<u8>),
}

unsafe fn next(endpoint: Endpoint) -> Option<Event> {
    unsafe {
        let mut e = PonkoMessageEvent::default();
        let mut found = false;
        assert_eq!(ponko_net_messages_next_event(endpoint, &mut e, &mut found), OK);
        found.then(|| match e.kind {
            PONKO_MESSAGE_CONNECTED => Event::Connected(e.peer),
            PONKO_MESSAGE_DISCONNECTED => {
                assert!(!e.reason.is_null());
                Event::Disconnected(e.peer, CStr::from_ptr(e.reason).to_str().unwrap().to_string())
            }
            PONKO_MESSAGE => {
                let bytes = if e.len == 0 {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(e.bytes, e.len).to_vec()
                };
                Event::Message(e.peer, e.lane, bytes)
            }
        })
    }
}

unsafe fn drain(endpoint: Endpoint) -> Vec<Event> {
    unsafe { std::iter::from_fn(|| next(endpoint)).collect() }
}

/// Polls every endpoint now and then until `done` says the events collected
/// so far are enough (or 30 s pass: then it panics). A hang detector: it
/// ends as soon as the events are in, so only a hang uses the 30 s up.
unsafe fn pump(endpoints: &[Endpoint], events: &mut [Vec<Event>], done: impl Fn(&[Vec<Event>]) -> bool) {
    unsafe {
        let start = Instant::now();
        while !done(events) {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out; events: {events:?}"
            );
            for (i, &endpoint) in endpoints.iter().enumerate() {
                assert_eq!(ponko_net_messages_poll(endpoint), OK);
                events[i].extend(drain(endpoint));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Two endpoints connected to each other: `[host, client]`, with the host's
/// id for the client.
unsafe fn connected(endpoints: [Endpoint; 2]) -> u64 {
    unsafe {
        let mut events = [Vec::new(), Vec::new()];
        pump(&endpoints, &mut events, |e| !e[0].is_empty() && !e[1].is_empty());
        assert_eq!(events[1], [Event::Connected(0)], "the host is always peer 0");
        let Event::Connected(peer) = events[0][0] else {
            panic!("the host sees a connection first: {:?}", events[0]);
        };
        peer
    }
}

fn messages_of(events: &[Event]) -> Vec<(PonkoLane, &[u8])> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Message(_, lane, bytes) => Some((*lane, bytes.as_slice())),
            _ => None,
        })
        .collect()
}

#[test]
fn loopback_endpoints_connect_exchange_messages_and_count_traffic() {
    unsafe {
        let mut e = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback(2, e.as_mut_ptr()), OK);
        let [host, client] = e;
        let peer = connected(e);

        // Reliable both ways, then the other lanes; the bytes arrive intact.
        let mut events = [Vec::new(), Vec::new()];
        assert_eq!(
            ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, b"hello".as_ptr(), 5),
            OK
        );
        assert_eq!(
            ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, b"world".as_ptr(), 5),
            OK
        );
        assert_eq!(
            ponko_net_messages_send(client, 0, PONKO_LANE_UNRELIABLE, b"aim".as_ptr(), 3),
            OK
        );
        assert_eq!(
            ponko_net_messages_send_to_all(host, PONKO_LANE_RELIABLE_UNORDERED, b"all".as_ptr(), 3),
            OK
        );
        pump(&e, &mut events, |ev| {
            messages_of(&ev[1]).len() == 3 && messages_of(&ev[0]).len() == 1
        });
        assert_eq!(messages_of(&events[0]), [(PONKO_LANE_UNRELIABLE, &b"aim"[..])]);
        let received = messages_of(&events[1]);
        assert_eq!(received[0], (PONKO_LANE_RELIABLE, &b"hello"[..]), "in the order sent");
        assert_eq!(received[1], (PONKO_LANE_RELIABLE, &b"world"[..]));
        assert_eq!(received[2], (PONKO_LANE_RELIABLE_UNORDERED, &b"all"[..]));
        assert!(
            events[1]
                .iter()
                .all(|e| !matches!(e, Event::Message(from, ..) if *from != 0))
        );

        // An empty message is allowed (null bytes with length 0); a missing
        // payload or one over 1 MiB is not.
        assert_eq!(ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, null(), 0), OK);
        assert_eq!(ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, null(), 4), BAD);
        assert_eq!(
            ponko_net_messages_send_to_all(host, PONKO_LANE_RELIABLE, null(), 4),
            BAD
        );
        let big = vec![0u8; (1 << 20) + 1];
        assert_ne!(
            ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, big.as_ptr(), big.len()),
            OK
        );

        // Traffic was counted on both sides.
        let (mut hs, mut cs) = (PonkoMessageStats::default(), PonkoMessageStats::default());
        assert_eq!(ponko_net_messages_stats(host, &mut hs), OK);
        assert_eq!(ponko_net_messages_stats(client, &mut cs), OK);
        assert!(hs.packets_sent > 0 && hs.bytes_sent > 0, "{hs:?}");
        assert!(cs.packets_received > 0 && cs.bytes_received > 0, "{cs:?}");
        assert!(hs.bytes_sent >= 10, "at least the payloads: {hs:?}");
        assert_eq!(ponko_net_messages_stats(host, null_mut()), BAD);

        // Loopback endpoints have no port; the timeout setter accepts any value.
        let mut port = 99;
        assert_eq!(ponko_net_messages_local_port(host, &mut port), OK);
        assert_eq!(port, 0);
        assert_eq!(ponko_net_messages_set_timeout(host, 30_000), OK);
        // No Steam identities here: zero, with or without the feature.
        let mut id = 7;
        assert_eq!(ponko_net_messages_steam_peer(host, peer, &mut id), OK);
        assert_eq!(id, 0);

        ponko_net_messages_free(host);
        ponko_net_messages_free(client);
    }
}

#[test]
fn an_event_stays_valid_until_the_next_one_is_taken() {
    unsafe {
        let mut e = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback(2, e.as_mut_ptr()), OK);
        let [host, client] = e;
        let peer = connected(e);
        for text in [&b"one"[..], b"two"] {
            assert_eq!(
                ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, text.as_ptr(), text.len()),
                OK
            );
        }
        // Poll without draining, so both are queued.
        let start = Instant::now();
        loop {
            assert_eq!(ponko_net_messages_poll(host), OK);
            assert_eq!(ponko_net_messages_poll(client), OK);
            let mut first = PonkoMessageEvent::default();
            let mut found = false;
            assert_eq!(ponko_net_messages_next_event(client, &mut first, &mut found), OK);
            if found {
                assert_eq!(first.kind, PONKO_MESSAGE);
                assert_eq!(std::slice::from_raw_parts(first.bytes, first.len), b"one");
                // Asking for the second one is what invalidates the first.
                assert_eq!(
                    next(client),
                    Some(Event::Message(0, PONKO_LANE_RELIABLE, b"two".to_vec()))
                );
                assert_eq!(next(client), None);
                break;
            }
            // A hang detector: the loop ends when the message is in.
            assert!(start.elapsed() < Duration::from_secs(30), "no message arrived");
            std::thread::sleep(Duration::from_millis(2));
        }
        ponko_net_messages_free(host);
        ponko_net_messages_free(client);
    }
}

#[test]
fn disconnecting_a_peer_tells_it_why() {
    unsafe {
        let mut e = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback(2, e.as_mut_ptr()), OK);
        let [host, client] = e;
        let peer = connected(e);
        assert_eq!(ponko_net_messages_disconnect(host, peer), OK);
        let mut events = [Vec::new(), Vec::new()];
        pump(&e, &mut events, |ev| {
            ev[1].iter().any(|e| matches!(e, Event::Disconnected(..)))
        });
        let gone: Vec<_> = events[1]
            .iter()
            .filter(|e| matches!(e, Event::Disconnected(..)))
            .collect();
        assert_eq!(gone.len(), 1, "reported once: {:?}", events[1]);
        assert!(matches!(gone[0], Event::Disconnected(0, _)));
        ponko_net_messages_free(host);
        ponko_net_messages_free(client);
    }
}

#[test]
fn a_silent_peer_times_out() {
    unsafe {
        let mut e = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback(2, e.as_mut_ptr()), OK);
        let [host, client] = e;
        connected(e);
        // The client stops hearing from the host for longer than its timeout.
        assert_eq!(ponko_net_messages_set_timeout(client, 50), OK);
        let start = Instant::now();
        let mut seen = Vec::new();
        while !seen.iter().any(|e| matches!(e, Event::Disconnected(..))) {
            // A hang detector: the 50 ms timeout ends the loop; a stalled
            // machine only makes it later, never earlier.
            assert!(start.elapsed() < Duration::from_secs(30), "no timeout: {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
            assert_eq!(ponko_net_messages_poll(client), OK);
            seen.extend(drain(client));
        }
        assert!(matches!(seen.last(), Some(Event::Disconnected(0, _))), "{seen:?}");
        ponko_net_messages_free(host);
        ponko_net_messages_free(client);
    }
}

#[test]
fn three_loopback_players_hear_the_host() {
    unsafe {
        let mut e = [null_mut(); 3];
        assert_eq!(ponko_net_messages_loopback(3, e.as_mut_ptr()), OK);
        let mut events = [Vec::new(), Vec::new(), Vec::new()];
        pump(&e, &mut events, |ev| {
            ev[0].len() == 2 && ev[1].len() == 1 && ev[2].len() == 1
        });
        assert_eq!(
            ponko_net_messages_send_to_all(e[0], PONKO_LANE_RELIABLE, b"hi".as_ptr(), 2),
            OK
        );
        pump(&e, &mut events, |ev| {
            ev[1..].iter().all(|events| messages_of(events).len() == 1)
        });
        for events in &events[1..] {
            assert_eq!(messages_of(events), [(PONKO_LANE_RELIABLE, &b"hi"[..])]);
        }
        for endpoint in e {
            ponko_net_messages_free(endpoint);
        }
    }
}

#[test]
fn udp_endpoints_talk_on_the_local_machine() {
    unsafe {
        let mut host = null_mut();
        assert_eq!(ponko_net_messages_host(c"127.0.0.1:0".as_ptr(), 2, &mut host), OK);
        let mut port = 0;
        assert_eq!(ponko_net_messages_local_port(host, &mut port), OK);
        assert_ne!(port, 0, "a free port was picked");
        let mut client = null_mut();
        let addr = std::ffi::CString::new(format!("127.0.0.1:{port}")).unwrap();
        assert_eq!(ponko_net_messages_join(addr.as_ptr(), &mut client), OK);

        let peer = connected([host, client]);
        assert_eq!(
            ponko_net_messages_send(host, peer, PONKO_LANE_RELIABLE, b"over udp".as_ptr(), 8),
            OK
        );
        let mut events = [Vec::new(), Vec::new()];
        pump(&[host, client], &mut events, |ev| !messages_of(&ev[1]).is_empty());
        assert_eq!(messages_of(&events[1]), [(PONKO_LANE_RELIABLE, &b"over udp"[..])]);

        ponko_net_messages_free(client);
        ponko_net_messages_free(host);
    }
}

#[test]
fn bad_arguments_are_errors_not_crashes() {
    unsafe {
        let mut e = [null_mut(); 2];
        let mut one = null_mut();
        // Creating endpoints.
        assert_eq!(ponko_net_messages_loopback(1, e.as_mut_ptr()), BAD, "needs two players");
        assert_eq!(ponko_net_messages_loopback(0, e.as_mut_ptr()), BAD);
        assert_eq!(ponko_net_messages_loopback(2, null_mut()), BAD);
        assert_eq!(ponko_net_messages_host(null(), 2, &mut one), BAD);
        assert_eq!(ponko_net_messages_host(c"127.0.0.1:0".as_ptr(), 2, null_mut()), BAD);
        assert_ne!(ponko_net_messages_host(c"not an address".as_ptr(), 2, &mut one), OK);
        assert!(one.is_null());
        assert_eq!(ponko_net_messages_join(null(), &mut one), BAD);
        assert_eq!(ponko_net_messages_join(c"127.0.0.1:9".as_ptr(), null_mut()), BAD);
        assert_ne!(ponko_net_messages_join(c"not an address".as_ptr(), &mut one), OK);

        // A null endpoint everywhere.
        let mut ev = PonkoMessageEvent::default();
        let (mut found, mut port, mut stats, mut id) = (false, 0u16, PonkoMessageStats::default(), 0u64);
        assert_eq!(ponko_net_messages_poll(null_mut()), BAD);
        assert_eq!(ponko_net_messages_next_event(null_mut(), &mut ev, &mut found), BAD);
        assert_eq!(
            ponko_net_messages_send(null_mut(), 0, PONKO_LANE_RELIABLE, null(), 0),
            BAD
        );
        assert_eq!(
            ponko_net_messages_send_to_all(null_mut(), PONKO_LANE_RELIABLE, null(), 0),
            BAD
        );
        assert_eq!(ponko_net_messages_disconnect(null_mut(), 0), BAD);
        assert_eq!(ponko_net_messages_stats(null_mut(), &mut stats), BAD);
        assert_eq!(ponko_net_messages_local_port(null_mut(), &mut port), BAD);
        assert_eq!(ponko_net_messages_set_timeout(null_mut(), 10), BAD);
        assert_eq!(ponko_net_messages_steam_peer(null_mut(), 0, &mut id), BAD);
        ponko_net_messages_free(null_mut());

        // Outputs must be writable.
        assert_eq!(ponko_net_messages_loopback(2, e.as_mut_ptr()), OK);
        assert_eq!(ponko_net_messages_next_event(e[0], null_mut(), &mut found), BAD);
        assert_eq!(ponko_net_messages_next_event(e[0], &mut ev, null_mut()), BAD);
        assert_eq!(ponko_net_messages_local_port(e[0], null_mut()), BAD);
        assert_eq!(ponko_net_messages_steam_peer(e[0], 0, null_mut()), BAD);
        // Disconnecting someone who isn't there is an error, not a crash.
        let _ = ponko_net_messages_disconnect(e[0], 12345);
        ponko_net_messages_free(e[0]);
        ponko_net_messages_free(e[1]);
    }
}

/// A version-1 loopback link with these settings.
fn link(loss: f64, delay: f64, jitter: f64, seed: u64) -> PonkoLoopbackLink {
    let mut l: PonkoLoopbackLink = unsafe { std::mem::zeroed() };
    (l.struct_size, l.version) = (size_of::<PonkoLoopbackLink>() as u32, PONKO_LOOPBACK_LINK_VERSION);
    assert_eq!(unsafe { ponko_loopback_link_defaults(&mut l) }, OK);
    (l.loss, l.delay, l.jitter, l.seed) = (loss, delay, jitter, seed);
    l
}

/// What a client heard from its host over `link`, stepped at 60 Hz on the
/// endpoints' own clocks: the host sends message `i` on the reliable and on
/// the unreliable lane at frame `i` (200 frames), then the run goes on until
/// every reliable one is in. The frame and number of each reliable arrival,
/// and the numbers of the unreliable ones.
fn heard_over(link: &PonkoLoopbackLink) -> (Vec<(u32, u32)>, Vec<u32>) {
    unsafe {
        let mut e = [null_mut(); 2];
        assert_eq!(ponko_net_messages_loopback_link(2, link, e.as_mut_ptr()), OK);
        let [host, client] = e;
        let (mut peer, mut joined) = (None, false);
        let (mut reliable, mut unreliable) = (Vec::new(), Vec::new());
        let mut sent = 0u32;
        for frame in 0..5000u32 {
            for &endpoint in &e {
                assert_eq!(ponko_net_messages_poll_elapsed(endpoint, 1.0 / 60.0), OK);
            }
            for ev in drain(host) {
                if let Event::Connected(p) = ev {
                    peer = Some(p);
                }
            }
            for ev in drain(client) {
                match ev {
                    Event::Connected(_) => joined = true,
                    Event::Message(0, PONKO_LANE_RELIABLE, b) => {
                        reliable.push((frame, u32::from_le_bytes(b[..].try_into().unwrap())))
                    }
                    Event::Message(0, PONKO_LANE_UNRELIABLE, b) => {
                        unreliable.push(u32::from_le_bytes(b[..].try_into().unwrap()))
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            if let (Some(p), true) = (peer, joined)
                && sent < 200
            {
                let bytes = sent.to_le_bytes();
                assert_eq!(
                    ponko_net_messages_send(host, p, PONKO_LANE_RELIABLE, bytes.as_ptr(), 4),
                    OK
                );
                assert_eq!(
                    ponko_net_messages_send(host, p, PONKO_LANE_UNRELIABLE, bytes.as_ptr(), 4),
                    OK
                );
                sent += 1;
            }
            if reliable.len() == 200 {
                break;
            }
        }
        ponko_net_messages_free(host);
        ponko_net_messages_free(client);
        (reliable, unreliable)
    }
}

#[test]
fn a_lossy_loopback_link_loses_and_delays_the_same_datagrams_on_every_run() {
    let lossy = link(0.2, 0.03, 0.05, 3);
    let (reliable, unreliable) = heard_over(&lossy);
    // The reliable lane gets every message through, once and in order.
    assert_eq!(
        reliable.iter().map(|r| r.1).collect::<Vec<_>>(),
        (0..200).collect::<Vec<_>>()
    );
    // The unreliable lane loses about a fifth.
    assert!(
        (100..=185).contains(&unreliable.len()),
        "{} of 200 unreliable messages arrived",
        unreliable.len()
    );
    // The same seed, the same run; another seed, another one.
    assert_eq!(heard_over(&lossy), (reliable.clone(), unreliable.clone()));
    assert_ne!(heard_over(&link(0.2, 0.03, 0.05, 4)).1, unreliable);
    // Negative control: a perfect link loses nothing and delivers sooner.
    let (fast, all) = heard_over(&link(0.0, 0.0, 0.0, 3));
    assert_eq!(all, (0..200).collect::<Vec<_>>());
    assert!(fast.last().unwrap().0 < reliable.last().unwrap().0);
}

#[test]
fn bad_loopback_links_and_clocks_are_refused() {
    unsafe {
        let mut e = [null_mut(); 2];
        for bad in [
            link(1.5, 0.0, 0.0, 0),
            link(-0.1, 0.0, 0.0, 0),
            link(0.0, -1.0, 0.0, 0),
            link(0.0, 0.0, f64::NAN, 0),
            link(0.0, f64::INFINITY, 0.0, 0),
        ] {
            assert_eq!(
                ponko_net_messages_loopback_link(2, &bad, e.as_mut_ptr()),
                BAD,
                "{bad:?}"
            );
        }
        let fine = link(0.5, 0.01, 0.01, 1);
        assert_eq!(ponko_net_messages_loopback_link(1, &fine, e.as_mut_ptr()), BAD);
        assert_eq!(ponko_net_messages_loopback_link(1026, &fine, e.as_mut_ptr()), BAD);
        assert_eq!(ponko_net_messages_loopback_link(2, &fine, null_mut()), BAD);
        assert_eq!(ponko_net_messages_loopback_link(2, null(), e.as_mut_ptr()), BAD);
        // A descriptor without its header, or from a newer header, is refused.
        let mut unset = fine;
        unset.version = 0;
        assert_eq!(ponko_net_messages_loopback_link(2, &unset, e.as_mut_ptr()), BAD);
        let mut newer = fine;
        newer.version = PONKO_LOOPBACK_LINK_VERSION + 1;
        assert_eq!(ponko_net_messages_loopback_link(2, &newer, e.as_mut_ptr()), BAD);
        assert_eq!(ponko_loopback_link_defaults(null_mut()), BAD);
        // Poll times must be finite and not negative.
        assert_eq!(ponko_net_messages_loopback_link(2, &fine, e.as_mut_ptr()), OK);
        assert_eq!(ponko_net_messages_poll_elapsed(e[0], -0.5), BAD);
        assert_eq!(ponko_net_messages_poll_elapsed(e[0], f64::NAN), BAD);
        assert_eq!(ponko_net_messages_poll_elapsed(null_mut(), 0.0), BAD);
        assert_eq!(ponko_net_messages_poll_elapsed(e[0], 0.0), OK);
        ponko_net_messages_free(e[0]);
        ponko_net_messages_free(e[1]);
    }
}
