//! `Game::run_server`: real-time pacing, stopping cleanly, the tick-rate
//! report and a client over UDP. Signals are in `tests/server_signals.rs`
//! (their own process, since handlers are process-wide).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use ponko::net::{Lane, MessageEvent, NetMessages, PeerId};
use ponko::prelude::*;
use ponko::server::{LogFormat, ServerConfig, ServerStatus, StopReason};

#[derive(Component)]
struct Marker;

/// A test server: no signal handlers, no log lines.
fn config() -> ServerConfig {
    ServerConfig::new().signals(false).log_format(LogFormat::Off)
}

fn status(world: &World) -> &ServerStatus {
    world.resource::<ServerStatus>()
}

#[test]
fn ticks_are_paced_to_wall_time() {
    let start = Instant::now();
    let world = Game::new("paced", 1, 1)
        .fixed_hz(100)
        .run_server(config().max_ticks(20), |_| {});
    let elapsed = start.elapsed();
    assert_eq!(status(&world).tick, 20);
    assert_eq!(status(&world).target_hz, 100);
    // 20 ticks at 100 Hz: the 20th starts 190 ms after the first. A lower
    // bound: pacing waits, and a busy machine only makes the run longer.
    assert!(elapsed >= Duration::from_millis(185), "{elapsed:?}");
}

#[test]
fn the_tick_limit_stops_with_its_reason_and_a_final_report() {
    let world = Game::new("limit", 1, 1)
        .fixed_hz(200)
        .setup(|ctx| {
            ctx.world.spawn((Marker,));
        })
        .run_server(config().max_ticks(10).report_every(Duration::from_secs(60)));
    let status = status(&world);
    assert_eq!((status.tick, status.stopping), (10, Some(StopReason::MaxTicks)));
    let report = status.report.as_ref().expect("a report for the last window");
    assert_eq!((report.ticks, report.target_hz, report.entities), (10, 200, 1));
    assert!(report.hz > 0.0 && report.tick_ms_max >= report.tick_ms_mean);
    assert_eq!(report.peers, None, "no message endpoint");
}

#[test]
fn reports_can_be_turned_off() {
    let world = Game::new("quiet", 1, 1)
        .fixed_hz(500)
        .run_server(config().max_ticks(5).report_every(Duration::ZERO), |_| {});
    assert!(status(&world).report.is_none());
}

#[test]
fn a_stop_request_gives_systems_the_grace_to_wrap_up() {
    let config = config().shutdown_grace(Duration::from_secs(5));
    let stop = config.stop_handle();
    let after_stop = Rc::new(Cell::new(0u32));
    let seen = after_stop.clone();
    let world = Game::new("graceful", 1, 1)
        .fixed_hz(200)
        .sim_system(move |ctx: &mut Ctx| {
            let tick = ctx.world.resource::<ServerStatus>().tick;
            if tick == 4 {
                stop.stop();
            }
            if ctx.world.resource::<ServerStatus>().stopping == Some(StopReason::Requested) {
                // Say goodbye for three ticks, then quit.
                seen.set(seen.get() + 1);
                if seen.get() == 3 {
                    ctx.quit();
                }
            }
        })
        .run_server(config, |_| {});
    assert_eq!(after_stop.get(), 3);
    // Ticks 0..=4 run before the request is seen, then three more: the
    // quit ended the grace early (its 5 s at 200 Hz would be ~1000 ticks).
    assert_eq!(status(&world).tick, 8);
    assert_eq!(status(&world).stopping, Some(StopReason::Requested));
}

#[test]
fn without_grace_a_stop_runs_no_further_tick() {
    let config = config();
    let stop = config.stop_handle();
    let world = Game::new("abrupt", 1, 1)
        .fixed_hz(500)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.world.resource::<ServerStatus>().tick == 2 {
                stop.stop();
            }
        })
        .run_server(config, |_| {});
    // The system saw ticks 0, 1, 2 (stopping at 2): three ticks ran.
    assert_eq!(status(&world).tick, 3);
}

#[test]
fn the_grace_ends_the_run_when_the_game_ignores_the_stop() {
    let config = config().shutdown_grace(Duration::from_millis(100));
    config.stop_handle().stop();
    let start = Instant::now();
    let world = Game::new("stubborn", 1, 1).fixed_hz(100).run_server(config, |_| {});
    let elapsed = start.elapsed();
    // At least the grace (sleeps only make it longer). The upper bound only
    // catches a grace that doesn't end the run; a busy machine stretches
    // 100 ms by far less than a minute.
    assert!(elapsed >= Duration::from_millis(100), "{elapsed:?}");
    assert!(elapsed < Duration::from_secs(60), "{elapsed:?}");
    assert!(status(&world).tick >= 5, "ticks kept running in the grace");
    assert_eq!(status(&world).stopping, Some(StopReason::Requested));
}

#[test]
fn ctx_quit_stops_the_server_with_reason_quit() {
    let world = Game::new("quits", 1, 1)
        .fixed_hz(500)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.world.resource::<ServerStatus>().tick == 1 {
                ctx.quit();
            }
        })
        .run_server(config(), |_| {});
    assert_eq!(
        (status(&world).tick, status(&world).stopping),
        (2, Some(StopReason::Quit))
    );
}

#[test]
fn a_slow_tick_is_late_and_a_long_stall_drops_the_backlog() {
    let world = Game::new("stall", 1, 1)
        .fixed_hz(100)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.world.resource::<ServerStatus>().tick == 3 {
                // 80 ms: eight ticks' worth, past the two it may catch up.
                std::thread::sleep(Duration::from_millis(80));
            }
        })
        .run_server(
            config()
                .max_ticks(10)
                .max_catch_up(2)
                .report_every(Duration::from_secs(60)),
            |_| {},
        );
    let report = status(&world).report.clone().expect("report");
    assert!(report.late_ticks >= 1, "{report:?}");
    assert!(report.skipped_ticks >= 4, "{report:?}");
    assert!(report.tick_ms_max >= 80.0, "{report:?}");
    assert_eq!(report.ticks, 10);
}

/// The client: says hello, waits for the answer, leaves.
fn client(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("messages");
    let mut answer = None;
    while let Some(event) = messages.next_event() {
        match &event {
            MessageEvent::Connected(host) => messages.send(*host, Lane::Reliable, b"hello").unwrap(),
            MessageEvent::Message { .. } => answer = event.decode::<u32>(),
            MessageEvent::Disconnected(_, reason) => panic!("{reason}"),
        }
    }
    if let Some(answer) = answer {
        messages.disconnect(PeerId::HOST).unwrap();
        ctx.world.insert_resource(answer);
        ctx.quit();
    }
}

/// The server answers hello with 42, and once its client has left it
/// stops itself.
fn host(ctx: &mut Ctx) {
    let messages = ctx.net.messages.as_mut().expect("messages");
    let mut left = false;
    while let Some(event) = messages.next_event() {
        match event {
            MessageEvent::Message { peer, .. } => messages.send_serde(peer, Lane::Reliable, &42u32).unwrap(),
            MessageEvent::Disconnected(..) => left = true,
            MessageEvent::Connected(_) => {}
        }
    }
    if left {
        ctx.quit();
    }
}

#[test]
fn a_server_accepts_a_client_and_reports_its_peers() {
    let endpoint = NetMessages::host("127.0.0.1:0", 4).unwrap();
    let address = endpoint.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        Game::new("echo server", 1, 1)
            .fixed_hz(100)
            .messages(endpoint)
            .sim_system(host)
            // A safety net: a broken client can't hang the test.
            .run_server(config().max_ticks(1000).report_every(Duration::from_millis(30)), |_| {})
    });
    let client_world = Game::new("client", 1, 1)
        .messages(NetMessages::join(address).unwrap())
        .sim_system(client)
        .sim_system(|_: &mut Ctx| std::thread::sleep(Duration::from_millis(1)))
        .run_headless(5000, |_| {});
    assert_eq!(client_world.get_resource::<u32>(), Some(&42));
    let world = server.join().unwrap();
    let status = status(&world);
    assert_eq!(status.stopping, Some(StopReason::Quit), "the server stopped itself");
    assert!(status.tick < 1000);
    assert!(status.report.as_ref().unwrap().peers.is_some());
}

#[test]
fn a_configured_game_runs_as_a_server_with_its_setups() {
    let world = Game::new("configured", 1, 1)
        .fixed_hz(500)
        .setup(|ctx| ctx.world.insert_resource(7u8))
        .run_server(config().max_ticks(1));
    assert_eq!(*world.resource::<u8>(), 7);
    assert_eq!(status(&world).target_hz, 500, "setup ran after the status was in");
}
