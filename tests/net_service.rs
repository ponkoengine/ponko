//! `ctx.net`: one networking entry for lockstep, messages, snapshots and
//! interest, with the engine polling the message endpoint.

use ponko::net::Interest;
use ponko::net::InterestConfig;
use ponko::net::InterestEntity;
use ponko::net::InterestViewer;
use ponko::net::Lane;
use ponko::net::MessageEvent;
use ponko::net::Net;
use ponko::net::NetMessages;
use ponko::net::NetState;
use ponko::net::PeerId;
use ponko::net::Pose;
use ponko::net::SnapshotState;
use ponko::net::UpdateStamp;
use ponko::prelude::*;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// What the guest's system collected.
#[derive(Default)]
struct Got(Vec<u32>);

fn pace(_: &mut Ctx) {
    // Message timers run on the wall clock and headless ticks don't wait.
    thread::sleep(Duration::from_millis(1));
}

fn echo_host(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("the endpoint");
    let mut left = false;
    while let Some(event) = messages.next_event() {
        match event {
            MessageEvent::Message { peer, lane, bytes } => messages.send(peer, lane, &bytes).unwrap(),
            MessageEvent::Disconnected(..) => left = true,
            MessageEvent::Connected(_) => {}
        }
    }
    if left {
        ctx.quit();
    }
}

fn counting_guest(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("the endpoint");
    let mut echoed = Vec::new();
    while let Some(event) = messages.next_event() {
        match &event {
            MessageEvent::Connected(host) => {
                for n in 0u32..20 {
                    messages.send_serde(*host, Lane::Reliable, &n).unwrap();
                }
            }
            MessageEvent::Message { .. } => echoed.push(event.decode::<u32>().expect("a number")),
            MessageEvent::Disconnected(_, why) => panic!("{why}"),
        }
    }
    let got = ctx.world.resource_mut::<Got>();
    got.0.extend(echoed);
    if got.0.len() == 20 {
        ctx.net.messages.as_mut().unwrap().disconnect(PeerId::HOST).unwrap();
        ctx.quit();
    }
}

/// Two games, each with its end in `ctx.net.messages`; neither calls `poll`.
fn session(mut ends: Vec<NetMessages>) {
    let (guest, host) = (ends.pop().unwrap(), ends.pop().unwrap());
    let hosting = thread::spawn(move || {
        Game::new("host", 1, 1)
            .messages(host)
            .sim_system(echo_host)
            .sim_system(pace)
            .run_headless(10_000, |_| {});
    });
    let world = Game::new("guest", 1, 1)
        .messages(guest)
        .sim_system(counting_guest)
        .sim_system(pace)
        .run_headless(10_000, |ctx| ctx.world.insert_resource(Got::default()));
    assert_eq!(world.resource::<Got>().0, (0..20).collect::<Vec<_>>());
    hosting.join().expect("the host stopped when the guest left");
}

#[test]
fn a_message_session_runs_through_ctx_net_without_polling_by_hand() {
    session(NetMessages::loopback(2));
}

#[test]
fn a_lossy_message_session_still_delivers_in_order() {
    session(NetMessages::loopback_lossy(2, 4));
}

#[test]
fn the_endpoint_is_polled_while_lockstep_waits_for_inputs() {
    // A lockstep guest whose host never shows up runs no tick at all, but
    // its message endpoint is still polled: the session connects.
    let mut ends = NetMessages::loopback(2);
    let (guest_end, mut host_end) = (ends.pop().unwrap(), ends.pop().unwrap());
    let (connected_tx, connected_rx) = mpsc::channel();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let host_stop = stop.clone();
    let hosting = thread::spawn(move || {
        while !host_stop.load(std::sync::atomic::Ordering::Relaxed) {
            host_end.poll().unwrap();
            if host_end.events().any(|e| matches!(e, MessageEvent::Connected(_))) {
                let _ = connected_tx.send(());
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let lockstep = Net::join("127.0.0.1:9").unwrap().timeout(Duration::from_millis(600));
    let world = Game::new("guest", 1, 1)
        .lockstep(lockstep)
        .messages(guest_end)
        .sim_system(|ctx: &mut Ctx| *ctx.world.resource_mut::<u32>() += 1)
        .run_headless(100, |ctx| ctx.world.insert_resource(0u32));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    hosting.join().unwrap();
    assert_eq!(*world.resource::<u32>(), 0, "no tick ran: lockstep never connected");
    connected_rx
        .try_recv()
        .expect("the message endpoint connected meanwhile");
}

#[test]
fn ctx_net_is_offline_and_empty_by_default_and_still_reads_as_lockstep() {
    let world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            // The old spelling and the new one are the same session.
            ctx.net.set_input(&7u32);
            assert_eq!(ctx.net.state(), NetState::Offline);
            assert_eq!(ctx.net.lockstep.players(), 1);
            assert!(ctx.net.messages.is_none());
            assert!(ctx.net.interest.is_none());
            assert!(ctx.net.snapshots.is_empty());
            assert!(ctx.net.message_error().is_none());
            if ctx.time.sim_tick() > 0 {
                assert_eq!(ctx.net.lockstep.input::<u32>(0), Some(7));
                *ctx.world.resource_mut::<u32>() += 1;
            }
        })
        .run_headless(3, |ctx| ctx.world.insert_resource(0u32));
    assert_eq!(*world.resource::<u32>(), 2);
}

#[test]
fn an_endpoint_can_be_set_and_dropped_while_the_game_runs() {
    let world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| match ctx.time.sim_tick() {
            0 => ctx.net.messages = Some(NetMessages::loopback(2).remove(0)),
            1 => {
                let messages = ctx.net.messages.as_ref().expect("set last tick");
                // Polled by the engine: nothing to hear, nothing went wrong.
                assert_eq!(messages.stats().packets_received, 0);
                assert!(ctx.net.message_error().is_none());
                ctx.net.messages = None;
            }
            _ => {
                assert!(ctx.net.messages.is_none());
                *ctx.world.resource_mut::<u32>() += 1;
            }
        })
        .run_headless(4, |ctx| ctx.world.insert_resource(0u32));
    assert_eq!(*world.resource::<u32>(), 2);
}

#[test]
fn unread_events_are_reported_as_a_message_error_and_cleared_once_drained() {
    let mut ends = NetMessages::loopback(2);
    let (mut guest, host) = (ends.pop().unwrap(), ends.pop().unwrap());
    let flood = thread::spawn(move || {
        // Connect, then send more messages than an endpoint queues unread.
        let mut peer = None;
        let mut sent = 0u32;
        for _ in 0..20_000 {
            guest.poll().unwrap();
            for event in guest.events() {
                match event {
                    MessageEvent::Connected(p) => peer = Some(p),
                    MessageEvent::Disconnected(..) => return,
                    MessageEvent::Message { .. } => {}
                }
            }
            if let Some(p) = peer {
                while sent < 6000 && guest.send(p, Lane::Reliable, &sent.to_le_bytes()).is_ok() {
                    sent += 1;
                }
            }
            thread::sleep(Duration::from_micros(200));
        }
    });
    #[derive(Default)]
    struct Seen {
        blocked: bool,
        cleared: bool,
        messages: u32,
    }
    let world = Game::new("host", 1, 1)
        .messages(host)
        .sim_system(|ctx: &mut Ctx| {
            let blocked = ctx
                .net
                .message_error()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::WouldBlock);
            let seen_before = ctx.world.resource::<Seen>().blocked;
            let messages = ctx.net.messages.as_mut().unwrap();
            let mut read = 0;
            let mut peer = None;
            // Ignore the events until the engine's poll reports the backlog.
            if blocked || seen_before {
                for event in messages.events() {
                    if let MessageEvent::Message { peer: p, .. } = event {
                        read += 1;
                        peer = Some(p);
                    }
                }
            }
            let seen = ctx.world.resource_mut::<Seen>();
            seen.cleared |= seen.blocked && !blocked;
            seen.blocked |= blocked;
            seen.messages += read;
            if seen.messages == 6000 {
                let peer = peer.expect("the sender");
                ctx.net.messages.as_mut().unwrap().disconnect(peer).unwrap();
                ctx.quit();
            }
        })
        .sim_system(pace)
        .run_headless(20_000, |ctx| ctx.world.insert_resource(Seen::default()));
    flood.join().unwrap();
    let seen = world.resource::<Seen>();
    assert!(seen.blocked, "the backlog was reported");
    assert!(seen.cleared, "and the next poll that worked cleared the error");
    assert_eq!(seen.messages, 6000, "and nothing was lost");
}

#[test]
fn snapshots_and_interest_live_on_ctx_net() {
    let world = Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let now = tick as f64 * 0.05;
            // A packet per tick for entity 7, as a message handler would push it.
            let stamp = UpdateStamp {
                epoch: 0,
                seq: tick as u16,
            };
            if ctx.net.snapshots.accept(7, stamp) {
                let pose = Pose::at(vec3(tick as f32, 0.0, 0.0));
                ctx.net.snapshots.stream(7).push(100.0 + now, now, pose);
            }
            assert!(!ctx.net.snapshots.accept(7, stamp), "a duplicate");

            let interest = ctx
                .net
                .interest
                .get_or_insert_with(|| Interest::new(InterestConfig::default()));
            let viewers = [InterestViewer {
                peer: PeerId::HOST,
                position: Vec3::ZERO,
            }];
            let entities = [InterestEntity {
                id: 7,
                position: vec3(10.0, 0.0, 0.0),
            }];
            interest.update(tick, &viewers, &entities);
            assert_eq!(interest.entries(PeerId::HOST).len(), 1);

            if tick == 19 {
                let shown = ctx.net.snapshots.sample(7, now).expect("snapshots arrived");
                assert_eq!(shown.state, SnapshotState::Interpolated);
                ctx.world.insert_resource(shown.value.position.x);
            }
        })
        .run_headless(20, |_| {});
    // Shown 100 ms (two snapshots) in the past.
    assert!((world.resource::<f32>() - 17.0).abs() < 0.01);
}

#[test]
fn an_endpoint_on_the_game_clock_counts_game_time_not_wall_time() {
    // The host is never polled, so the guest hears nothing. On the game
    // clock it gives up after its timeout in game time (10 s: 600 ticks at
    // 60 Hz), though the headless run takes a tiny fraction of that.
    let mut ends = NetMessages::loopback(2);
    let guest = ends.pop().unwrap().game_clock();
    let _host = ends.pop();
    let world = Game::new("guest", 1, 1)
        .messages(guest)
        .sim_system(|ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let gone = ctx
                .net
                .messages
                .as_mut()
                .is_some_and(|m| m.events().any(|e| matches!(e, MessageEvent::Disconnected(..))));
            if gone && ctx.world.get_resource::<u64>().is_none() {
                ctx.world.insert_resource(tick);
            }
        })
        .run_headless(700, |_| {});
    let at = *world.resource::<u64>();
    assert!((595..=605).contains(&at), "timed out at tick {at}");
}
