//! A load test of the whole server in one process: thousands of
//! [`LobbyClient`]s over in-memory endpoints, on a simulated clock, through
//! the same protocol, framing and reliability layer as UDP (only the
//! sockets are left out). `examples/load_test.rs` runs it with 1000 rooms.
//!
//! ```
//! let report = ponko_lobby::load::run(&ponko_lobby::load::LoadConfig {
//!     rooms: 8,
//!     room_size: 3,
//!     matched_rooms: 2,
//!     relay_rounds: 4,
//!     payload_bytes: 32,
//! }).unwrap();
//! assert_eq!(report.rooms_created, 10);
//! assert_eq!(report.relay_delivered, report.relay_expected);
//! ```

use std::time::{Duration, Instant};

use ponko::net::lobby::{LobbyClient, LobbyEvent};
use ponko::net::{Lane, NetMessages};

use crate::{AddressKey, LobbyLimits, LobbyServer, LobbyStats};

/// What [`run`] does.
/// ```
/// let config = ponko_lobby::load::LoadConfig::default();
/// assert_eq!(config.rooms, 1000);
/// ```
#[derive(Clone, Debug)]
pub struct LoadConfig {
    /// Rooms made by code: a host creates each, the others join it.
    pub rooms: usize,
    /// Members of every room.
    pub room_size: u16,
    /// Rooms filled by the matchmaking queue instead.
    pub matched_rooms: usize,
    /// Relay messages every member sends to the rest of its room.
    pub relay_rounds: usize,
    /// Bytes in each relay message.
    pub payload_bytes: usize,
}

impl Default for LoadConfig {
    fn default() -> Self {
        Self {
            rooms: 1000,
            room_size: 3,
            matched_rooms: 100,
            relay_rounds: 20,
            payload_bytes: 64,
        }
    }
}

/// What [`run`] measured. Wall times are of this process, which runs the
/// clients too: the server alone is faster.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct LoadReport {
    /// Clients connected.
    pub clients: usize,
    /// Wall time until every client was connected.
    pub connect: Duration,
    /// Rooms made (by code and by matchmaking).
    pub rooms_created: u64,
    /// Wall time from the first create request until every room by code
    /// was full.
    pub fill_rooms: Duration,
    /// Rooms by code per second of `fill_rooms`.
    pub rooms_per_second: f64,
    /// Wall time until every queued player was in a room.
    pub matchmaking: Duration,
    /// Relay messages that should arrive.
    pub relay_expected: u64,
    /// Relay messages that arrived.
    pub relay_delivered: u64,
    /// Payload bytes that arrived.
    pub relay_bytes: u64,
    /// Wall time of the relay phase.
    pub relay: Duration,
    /// Messages delivered per second of `relay`.
    pub relay_messages_per_second: f64,
    /// Payload MiB delivered per second of `relay`.
    pub relay_mib_per_second: f64,
    /// Rooms whose host left and that every member saw get a new one.
    pub migrations_seen: u64,
    /// Wall time of the whole run.
    pub total: Duration,
    /// Simulated time of the whole run.
    pub simulated: Duration,
    /// The server's counters at the end.
    pub server: LobbyStats,
}

/// The simulated time per step.
const STEP: Duration = Duration::from_millis(10);
/// Steps a phase may take before the run fails (20 simulated seconds).
const MAX_STEPS: usize = 2000;
/// Clients per in-memory endpoint (the most one host takes).
const PER_ENDPOINT: usize = 1024;

struct Bench {
    server: LobbyServer,
    clients: Vec<LobbyClient>,
    steps: usize,
}

impl Bench {
    fn step(&mut self) -> Result<(), String> {
        self.steps += 1;
        self.server.poll_elapsed(STEP);
        for client in &mut self.clients {
            client.poll_elapsed(STEP).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Steps until `done`, reading every client's events into `on_event`.
    fn until(
        &mut self,
        phase: &str,
        mut on_event: impl FnMut(usize, LobbyEvent, &mut Vec<LobbyClient>) -> Result<(), String>,
        mut done: impl FnMut(&[LobbyClient]) -> bool,
    ) -> Result<(), String> {
        for _ in 0..MAX_STEPS {
            self.step()?;
            for i in 0..self.clients.len() {
                while let Some(event) = self.clients[i].next_event() {
                    if let LobbyEvent::Disconnected(why) = &event {
                        return Err(format!("{phase}: client {i} disconnected: {why}"));
                    }
                    if let LobbyEvent::Refused(error) = &event {
                        return Err(format!("{phase}: client {i} refused: {error}"));
                    }
                    on_event(i, event, &mut self.clients)?;
                }
            }
            if done(&self.clients) {
                return Ok(());
            }
        }
        Err(format!("{phase}: not done after {MAX_STEPS} steps"))
    }
}

/// Runs the load test: connect every client, fill `rooms` rooms by code
/// and `matched_rooms` by matchmaking, relay `relay_rounds` messages from
/// every member to its room on the reliable lane, then make every room's
/// host leave and wait until its members saw the new host.
///
/// # Errors
///
/// A phase that doesn't finish within 20 simulated seconds, a refused
/// request or a lost connection, described.
pub fn run(config: &LoadConfig) -> Result<LoadReport, String> {
    let size = usize::from(config.room_size);
    if size < 2 {
        return Err("room_size must be at least 2".into());
    }
    let start = Instant::now();
    let by_code = config.rooms * size;
    let total = by_code + config.matched_rooms * size;
    let limits = LobbyLimits {
        // Every in-memory client has the same address: limit per connection.
        address_key: AddressKey::Connection,
        max_rooms: config.rooms + config.matched_rooms,
        ..LobbyLimits::default()
    };
    let mut server = LobbyServer::new(limits).seed(1);
    let mut clients = Vec::with_capacity(total);
    let mut left = total;
    while left > 0 {
        let n = left.min(PER_ENDPOINT);
        let mut endpoints = NetMessages::loopback(n + 1).into_iter();
        server.listen(endpoints.next().ok_or("no host endpoint")?);
        clients.extend(endpoints.map(LobbyClient::new));
        left -= n;
    }
    let mut bench = Bench {
        server,
        clients,
        steps: 0,
    };
    let mut report = LoadReport {
        clients: total,
        ..LoadReport::default()
    };

    // Connect.
    bench.until("connect", |_, _, _| Ok(()), |c| c.iter().all(LobbyClient::is_connected))?;
    report.connect = start.elapsed();

    // Rooms by code: hosts create, the others join once the code is known.
    let fill = Instant::now();
    for room in 0..config.rooms {
        bench.clients[room * size]
            .create_room(config.room_size)
            .map_err(|e| e.to_string())?;
    }
    bench.until(
        "fill rooms",
        |i, event, clients| {
            if let LobbyEvent::Joined(room) = event
                && i < by_code
                && i % size == 0
            {
                for member in &mut clients[i + 1..i + size] {
                    member.join_room(&room.code).map_err(|e| e.to_string())?;
                }
            }
            Ok(())
        },
        |c| c[..by_code].iter().all(|c| c.room().is_some_and(|r| r.is_full())),
    )?;
    report.fill_rooms = fill.elapsed();
    report.rooms_per_second = config.rooms as f64 / report.fill_rooms.as_secs_f64();

    // Matchmaking.
    let matching = Instant::now();
    for client in &mut bench.clients[by_code..] {
        client.queue("load", config.room_size, 0).map_err(|e| e.to_string())?;
    }
    bench.until(
        "matchmaking",
        |_, _, _| Ok(()),
        |c| c[by_code..].iter().all(|c| c.room().is_some_and(|r| r.is_full())),
    )?;
    report.matchmaking = matching.elapsed();

    // Relay: every member sends one message a step to the rest of its room.
    let payload = vec![0xA5; config.payload_bytes];
    report.relay_expected = (total * (size - 1) * config.relay_rounds) as u64;
    let relay = Instant::now();
    let (mut delivered, mut bytes) = (0u64, 0u64);
    for _ in 0..config.relay_rounds {
        for client in &mut bench.clients {
            client
                .relay(None, Lane::Reliable, &payload)
                .map_err(|e| format!("relay: {e}"))?;
        }
        bench.step()?;
        for client in &mut bench.clients {
            while let Some(event) = client.next_event() {
                if let LobbyEvent::Relayed { bytes: b, .. } = event {
                    delivered += 1;
                    bytes += b.len() as u64;
                }
            }
        }
    }
    let counted = std::cell::Cell::new((delivered, bytes));
    let expected = report.relay_expected;
    bench.until(
        "relay",
        |_, event, _| {
            if let LobbyEvent::Relayed { bytes: b, .. } = event {
                let (n, total) = counted.get();
                counted.set((n + 1, total + b.len() as u64));
            }
            Ok(())
        },
        |_| counted.get().0 >= expected,
    )?;
    (report.relay_delivered, report.relay_bytes) = counted.get();
    report.relay = relay.elapsed();
    if report.relay_delivered != report.relay_expected {
        return Err(format!(
            "relay: {} of {} messages arrived",
            report.relay_delivered, report.relay_expected
        ));
    }
    report.relay_messages_per_second = report.relay_delivered as f64 / report.relay.as_secs_f64();
    report.relay_mib_per_second = report.relay_bytes as f64 / (1024.0 * 1024.0) / report.relay.as_secs_f64();

    // Host migration: every host leaves.
    let hosts: Vec<usize> = (0..bench.clients.len())
        .filter(|&i| bench.clients[i].is_host())
        .collect();
    for &i in &hosts {
        bench.clients[i].leave_room().map_err(|e| e.to_string())?;
    }
    let seen = std::cell::Cell::new(0u64);
    let migrations = (hosts.len() * (size - 1)) as u64;
    bench.until(
        "host migration",
        |_, event, _| {
            if let LobbyEvent::HostChanged(_) = event {
                seen.set(seen.get() + 1);
            }
            Ok(())
        },
        |_| seen.get() == migrations,
    )?;
    report.migrations_seen = hosts.len() as u64;
    report.rooms_created = bench.server.stats().rooms_created;
    report.server = bench.server.stats();
    report.total = start.elapsed();
    report.simulated = STEP * bench.steps as u32;
    Ok(report)
}
