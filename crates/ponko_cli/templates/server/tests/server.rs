//! Tests of the server, with a real client over UDP on this machine.
//! `cargo test` runs them; no window, GPU or container needed.

use std::time::Duration;

use ponko::net::NetMessages;
use ponko::prelude::*;
use ponko::server::{LogFormat, ServerConfig, ServerStatus, StopReason};

use {{crate_name}}::{ClientReport, Lobby, client, server};

/// A server on a free port of this machine, in a thread; at most `ticks`
/// ticks, so a broken test can't hang.
fn start(config: ServerConfig) -> (std::net::SocketAddr, std::thread::JoinHandle<World>) {
    let endpoint = NetMessages::host("127.0.0.1:0", 8).unwrap();
    let addr = endpoint.local_addr().unwrap();
    let config = config.signals(false).log_format(LogFormat::Off);
    (addr, std::thread::spawn(move || server(endpoint).run_server(config)))
}

#[test]
fn a_client_joins_gets_the_world_and_leaves() {
    let config = ServerConfig::new().max_ticks(300);
    let stop = config.stop_handle();
    let (addr, running) = start(config);
    let world = client(NetMessages::join(addr).unwrap(), "test", 60).run_headless(1800);
    let report = world.resource::<ClientReport>();
    assert!(report.welcome.is_some(), "{report:?}");
    assert!(report.states > 0, "{report:?}");
    assert_eq!(report.error, None);
    stop.stop();
    let world = running.join().unwrap();
    assert_eq!(world.resource::<Lobby>().joined, 1);
}

#[test]
fn stopping_the_server_says_goodbye_to_its_clients() {
    let config = ServerConfig::new().shutdown_grace(Duration::from_secs(5)).max_ticks(3000);
    let stop = config.stop_handle();
    let (addr, running) = start(config);
    let stopper = std::thread::spawn(move || {
        // Long enough for the client to join and get a state or two.
        std::thread::sleep(Duration::from_millis(800));
        stop.stop();
    });
    // The client would stay a minute: only the goodbye makes it leave.
    let world = client(NetMessages::join(addr).unwrap(), "test", 3600).run_headless(1800);
    stopper.join().unwrap();
    let report = world.resource::<ClientReport>();
    assert!(report.goodbye.is_some(), "{report:?}");
    let world = running.join().unwrap();
    assert_eq!(world.resource::<ServerStatus>().stopping, Some(StopReason::Requested));
    assert!(world.resource::<Lobby>().goodbye_tick.is_some());
}
