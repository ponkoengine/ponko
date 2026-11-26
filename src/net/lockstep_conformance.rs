//! Transport conformance tests for the lockstep protocol ([`Net`]).
//!
//! Lockstep sits on the private `Transport` trait in `net.rs`: UDP, the
//! in-memory loopback, the Steam relay or the lobby server's relay
//! (feature `lobby`; the tests marked with `EVERY_VIA` run through an
//! in-memory relay server too). These tests drive that seam with
//! the scripted network in `net/scripted.rs` and a simulated clock
//! (`Net::ready_at`), so they need no sockets, no threads, no Steam
//! installation and no renderer, and they pin what the protocol does when
//! the transport delays, loses, duplicates, reorders, refuses or fails
//! packets. `net/message_conformance.rs` is the same for [`NetMessages`](crate::net::NetMessages).
//!
//! # Transport contract (lockstep)
//!
//! What a lockstep transport must do, and what each existing one does.
//! "Scripted" is the test transport; it does each of these on request.
//!
//! | Contract | UDP | Loopback (`loopback_lossy`) | Steam relay | Lobby relay | Scripted |
//! |---|---|---|---|---|---|
//! | `recv` never blocks; `None` means no packet for this poll | `WouldBlock` → `None` | yes | drains Steam's queue into an inbox | polls the lobby once per drain, then reads its inbox | yes |
//! | Receive errors are not reported; the protocol only sees silence | skipped, at most 64 in a row per poll | never errors | messages without a Steam id are skipped | lobby failures come from `error()` instead | skipped (one-shot injection) |
//! | `send` is fire-and-forget: no result, a packet that can't go out is lost | result ignored (a full buffer is a lost packet) | unknown address or closed peer: dropped | result ignored; dropped until the lobby owner is known | out of the room or a full lane: dropped | refusal and failure both drop |
//! | A failure the transport finds on its own comes from `error()`, once, and ends the session | never | never | lobby could not be created or joined | the lobby connection failed or closed, or the client left the room | on request |
//! | Deliver whole packets; the protocol sends at most 1400 bytes | OS-dependent | truncates to the 64 KiB receive buffer | truncates to the 64 KiB receive buffer | whole relay frames; refused at creation if the server relays less than 1504 bytes | drops > 1500 |
//! | May reorder / lose / duplicate | yes / yes / yes | no / about one in n / no | per Steam | per the lobby's links; its unreliable lane drops stale and duplicate frames | each on request |
//! | Address identity: one stable address per peer for the session | socket address | `127.0.0.1:(i + 1)` | synthetic IPv6 per Steam id; the lobby owner is one fixed address | synthetic IPv6 per member id; a client only hears its host | synthetic |
//! | Clock | none: every timer is measured against the `now` passed to the session | same | same | the lobby follows the session's `now` (`flush`) | same |
//! | Disconnect | none; protocol `BYE` and the silence timeout | same | same (leaving the lobby is separate) | same (leaving the room ends the session) | link cut on request |
//! | `local_addr()` | the bound socket | synthetic | synthetic | synthetic | synthetic |
//! | Covered on CI by | `tests/net.rs` (localhost) | `tests/net.rs` | compiled with `--features steam`; no live relay run | this module (`Via::Relay`) and `ponko_lobby`'s `tests/relay_sessions.rs` (the real server) | this module |
//!
//! Protocol guarantees pinned here, given any transport that keeps the
//! contract above:
//!
//! | Guarantee | Test |
//! |---|---|
//! | Every peer runs every tick with the same inputs, `input_delay` ticks after they were set, through latency | `every_peer_sees_the_same_inputs_through_latency` |
//! | Loss, duplication and reordering cost time, never inputs | `loss_duplication_and_reordering_cost_only_time` |
//! | A refused or failed send is a lost packet and is recovered by resends | `refused_and_failed_sends_are_loss_and_recover` |
//! | Receive errors are skipped and keep the session | `receive_errors_are_skipped_and_keep_the_session` |
//! | A transport failure ends the session with its message | `a_transport_failure_ends_the_session_with_its_message` |
//! | A silent peer is lost after the configured timeout, in simulated time | `a_silent_peer_is_lost_after_the_timeout_in_simulated_time` |
//! | Joining gives up after the timeout when nobody answers | `joining_gives_up_after_the_timeout_when_nobody_answers` |
//! | Short, foreign, wrong-version and forged packets are ignored; nobody can send another player's inputs | `malformed_foreign_and_forged_packets_are_ignored` |
//! | Datagrams over the transport's limit never arrive and change nothing | `oversized_datagrams_never_arrive` |
//! | Packets fit 1400 bytes with the largest inputs from every player, and every stream still advances | `packets_fit_the_limit_with_the_largest_inputs` |
//! | A full host and a different seed refuse the joiner with a reason | `a_full_host_and_a_different_seed_refuse_the_joiner` |
//! | Worlds that differ are reported on both sides with the component | `different_worlds_are_reported_on_both_sides` |
//! | A peer that stops keeps resending until the others have its inputs, then says goodbye | `a_leaving_peer_resends_until_the_others_have_its_inputs` |
//! | A leaving peer doesn't wait for a peer that went quiet | `a_leaving_peer_gives_up_on_a_quiet_peer` |
//! | The same script gives the same traffic | `the_same_script_gives_the_same_traffic` |
//!
//! # What the two protocols share, and what stays different
//!
//! Shared: the packet model (unconnected datagrams between socket-shaped
//! addresses, with synthetic addresses for transports that have none), the
//! polling model (the game thread calls in, nothing blocks, no thread or
//! async runtime), protocol-owned reliability and liveness (resends,
//! `BYE`, a silence timeout; the transport never reports a disconnect),
//! caller-owned time, and this scripted network, which implements both
//! transport traits on one `Port`.
//!
//! Different on purpose:
//!
//! | | Lockstep (`Transport`) | Messages (`PacketTransport`) | Why |
//! |---|---|---|---|
//! | `send` result | none | `Ok(false)` = not submitted, `Err` returned from `poll` | Lockstep resends everything unacknowledged in every packet, so a failed send needs no handling. Messages count what was sent and surface socket errors to the game, which owns the endpoint. |
//! | Receive errors | skipped | returned from `poll` | A lockstep tick can't act on an error: it either has every input or waits, and the timeout decides. A message endpoint is polled by game code that can log or reconnect. |
//! | Out-of-band failure | `Transport::error()` ends the session | an `Err` from `poll`; the session continues | Lockstep can't run without all its peers; a message host can lose one client and go on. |
//! | Largest packet | 1400 bytes, built by the protocol | 1500 bytes, enforced by the transport | Lockstep packs inputs itself; renet fragments messages to its own size. |
//! | Clock | an `Instant` per call (`ready_at`) | elapsed time per call (`poll_elapsed`) | Lockstep is driven by the game loop's ticks, which already read the clock; messages keep their own running time for renet. |
//! | Backpressure | none: a peer more than 256 ticks behind is a lost session | `WouldBlock` from `send` and `poll` | Lockstep stops the simulation instead of queueing. |
//!
//! The two traits therefore stay separate. Both are internal; a public
//! adapter constructor waits for an external transport that needs one.

use std::net::SocketAddr;
use std::time::Duration;

use super::scripted::{Datagram, EVERY_VIA, Network, Shared, Via, addr, port};
use super::{GONE, INPUTS, MAGIC, MAX_INPUT_BYTES, Net, NetState, PACKET, VERSION};
use crate::{perf::Instant, reflect::WorldReflect};
use ponko_ecs::World;

/// One scripted step: the network advances one tick, the clock 10 ms, then
/// every peer tries to run its next simulation tick.
const STEP: Duration = Duration::from_millis(10);
const SEED: u64 = 7;

/// A host (`nets[0]`, `addr(0)`) and guests (`addr(1)`, ...), each trying
/// one simulation tick per step. Through a relay, the server is `addr(0)`
/// and the host and guests `addr(1)`, ...
struct Session {
    net: Shared,
    /// The relay server, polled at every step, for sessions through one.
    #[cfg(feature = "lobby")]
    relay: Option<super::lobby::relay::testing::RelayServer>,
    nets: Vec<Net>,
    start: Instant,
    now: Duration,
    /// Per peer: the next simulation tick to run.
    ticks: Vec<u64>,
    /// Per peer, per tick it ran: every player's input.
    seen: Vec<Vec<Vec<Vec<u8>>>>,
    /// Bytes of input each player sets per tick.
    input_len: usize,
    /// Per peer: a world to check after each tick, if any.
    worlds: Vec<Option<World>>,
}

/// What endpoint `i` sets while its next tick is `tick`: used `delay` ticks
/// later. (Endpoints, not players: a joiner learns its player number late.)
fn input(i: usize, tick: u64, len: usize) -> Vec<u8> {
    let mut bytes = format!("e{i}t{tick}").into_bytes();
    bytes.resize(len.max(bytes.len()), b'.');
    bytes
}

impl Session {
    fn new(players: usize, tune: impl Fn(Net) -> Net, script: impl FnOnce(&mut Network)) -> Self {
        Self::with_seeds(players, &vec![SEED; players], tune, script)
    }
    /// [`new`](Self::new), with the peers reaching each other `via` that.
    fn via(via: Via, players: usize, tune: impl Fn(Net) -> Net, script: impl FnOnce(&mut Network)) -> Self {
        match via {
            Via::Direct => Self::new(players, tune, script),
            #[cfg(feature = "lobby")]
            Via::Relay => Self::relayed(players, tune, script),
            #[cfg(feature = "steam")]
            Via::SteamMessages | Via::SteamSockets => unreachable!("Steam carriers are message-protocol only"),
        }
    }
    /// `players` peers through a relay server: the room forms first (on
    /// the scripted network, already shaped by `script`), then the room's
    /// host hosts the session and the others join it.
    #[cfg(feature = "lobby")]
    fn relayed(players: usize, tune: impl Fn(Net) -> Net, script: impl FnOnce(&mut Network)) -> Self {
        let net: Shared = Default::default();
        script(&mut net.lock().unwrap());
        let (server, lobbies) = super::lobby::relay::testing::scripted_room(&net, players, STEP);
        let nets: Vec<Net> = lobbies
            .into_iter()
            .enumerate()
            .map(|(i, lobby)| {
                let n = if i == 0 {
                    Net::host_relay(lobby, players)
                } else {
                    Net::join_relay(lobby)
                };
                let mut n = tune(n.expect("in the room"));
                n.begin(SEED);
                n
            })
            .collect();
        Self {
            net,
            relay: Some(server),
            start: Instant::now(),
            now: Duration::ZERO,
            ticks: vec![0; nets.len()],
            seen: vec![Vec::new(); nets.len()],
            input_len: 0,
            worlds: nets.iter().map(|_| None).collect(),
            nets,
        }
    }
    /// One endpoint per seed; the host expects `players` (so more seeds than
    /// players is one joiner too many).
    fn with_seeds(players: usize, seeds: &[u64], tune: impl Fn(Net) -> Net, script: impl FnOnce(&mut Network)) -> Self {
        let net: Shared = Default::default();
        script(&mut net.lock().unwrap());
        let nets: Vec<Net> = seeds
            .iter()
            .enumerate()
            .map(|(i, &seed)| {
                let host = (i != 0).then(|| addr(0));
                let mut n = tune(Net::transported(port(&net, i), host, if i == 0 { players } else { 0 }));
                n.begin(seed);
                n
            })
            .collect();
        Self {
            net,
            #[cfg(feature = "lobby")]
            relay: None,
            start: Instant::now(),
            now: Duration::ZERO,
            ticks: vec![0; nets.len()],
            seen: vec![Vec::new(); nets.len()],
            input_len: 0,
            worlds: nets.iter().map(|_| None).collect(),
            nets,
        }
    }
    fn script(&self, f: impl FnOnce(&mut Network)) {
        f(&mut self.net.lock().unwrap());
    }
    fn at(&self) -> Instant {
        self.start + self.now
    }
    fn tick_clock(&mut self) {
        self.net.lock().unwrap().tick += 1;
        self.now += STEP;
        #[cfg(feature = "lobby")]
        if let Some(relay) = &mut self.relay {
            relay.poll(STEP);
        }
    }
    /// Peer `i` tries its next tick. True if it ran.
    fn advance(&mut self, i: usize) -> bool {
        let tick = self.ticks[i];
        self.nets[i].set_input_bytes(&input(i, tick, self.input_len));
        if !self.nets[i].ready_at(tick, self.start + self.now) {
            return false;
        }
        let inputs = (0..self.nets[i].players())
            .map(|p| self.nets[i].input_bytes(p).to_vec())
            .collect();
        self.seen[i].push(inputs);
        if let Some(world) = &self.worlds[i] {
            self.nets[i].end_tick(world, tick);
        }
        self.ticks[i] += 1;
        true
    }
    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            self.tick_clock();
            for i in 0..self.nets.len() {
                self.advance(i);
            }
        }
    }
    /// Steps until every peer ran `tick` ticks; returns the steps it took.
    fn run_to(&mut self, tick: u64) -> usize {
        for steps in 0..5000 {
            if self.ticks.iter().all(|&t| t >= tick) {
                return steps;
            }
            self.step(1);
        }
        panic!("stuck at ticks {:?}, states {:?}", self.ticks, self.states());
    }
    fn states(&self) -> Vec<NetState> {
        self.nets.iter().map(Net::state).collect()
    }
    /// Every peer saw, for every tick it ran, exactly what each player set
    /// `delay` ticks before (nothing for the first `delay` ticks).
    fn assert_inputs(&self, delay: u64) {
        // Player number → endpoint (join order depends on packet order).
        let mut endpoints = vec![0; self.nets.len()];
        for (i, net) in self.nets.iter().enumerate() {
            endpoints[net.player()] = i;
        }
        for (peer, ticks) in self.seen.iter().enumerate() {
            for (tick, inputs) in ticks.iter().enumerate() {
                let tick = tick as u64;
                let expected: Vec<Vec<u8>> = endpoints
                    .iter()
                    .map(|&e| {
                        if tick < delay {
                            Vec::new()
                        } else {
                            input(e, tick - delay, self.input_len)
                        }
                    })
                    .collect();
                assert!(inputs == &expected, "peer {peer}, tick {tick}: wrong inputs");
            }
        }
    }
}

fn header(kind: u8) -> Vec<u8> {
    let mut p = MAGIC.to_vec();
    p.extend([VERSION, kind]);
    p
}

/// An INPUTS packet carrying one tick of `stream`, acknowledging nothing.
fn forged_inputs(players: usize, stream: u8, tick: u64, data: &[u8]) -> Vec<u8> {
    let mut p = header(INPUTS);
    p.push(players as u8);
    for _ in 0..players {
        p.extend(0u64.to_le_bytes());
    }
    p.push(1);
    p.push(stream);
    p.extend(tick.to_le_bytes());
    p.push(1);
    p.extend((data.len() as u16).to_le_bytes());
    p.extend(data);
    p
}

#[test]
fn every_peer_sees_the_same_inputs_through_latency() {
    for via in EVERY_VIA {
        let instant = Session::via(via, 3, |n| n, |_| {}).run_to(40);
        let mut s = Session::via(via, 3, |n| n, |n| n.delay = 4);
        let delayed = s.run_to(40);
        assert!(
            delayed > instant,
            "{via:?}: latency costs steps: {delayed} vs {instant}"
        );
        s.assert_inputs(3);
        assert_eq!(s.states(), [NetState::Running; 3]);
        let mut players = [s.nets[1].player(), s.nets[2].player()];
        players.sort_unstable();
        assert_eq!(players, [1, 2], "{via:?}");
        assert_eq!(s.nets[2].players(), 3, "the host decides the player count");

        // The host's settings reach the joiners.
        let mut s = Session::via(via, 2, |n| n.input_delay(6), |_| {});
        s.run_to(20);
        s.assert_inputs(6);
    }
}

#[test]
fn loss_duplication_and_reordering_cost_only_time() {
    for via in EVERY_VIA {
        let mut s = Session::via(
            via,
            3,
            |n| n,
            |n| {
                n.delay = 2;
                via.drop_one_in(n, 3);
                n.duplicate = true;
                n.newest_first = true;
            },
        );
        s.run_to(200);
        s.assert_inputs(3);
        assert_eq!(s.states(), [NetState::Running; 3], "{via:?}");
    }
}

#[test]
fn random_loss_and_jitter_cost_only_time() {
    for via in EVERY_VIA {
        let mut s = Session::via(
            via,
            4,
            |n| n,
            |n| {
                n.delay = 3;
                n.jitter = 5;
                n.loss_per_mille = 100;
            },
        );
        s.run_to(150);
        s.assert_inputs(3);
        assert_eq!(s.states(), [NetState::Running; 4], "{via:?}");
    }
}

#[test]
fn refused_and_failed_sends_are_loss_and_recover() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(10);
    s.script(|n| {
        n.refuse.insert(addr(1), 25);
        n.fail_send.insert(addr(0), std::io::ErrorKind::ConnectionReset);
    });
    let before = s.ticks.clone();
    s.step(5);
    assert!(
        s.ticks[0] < before[0] + 5,
        "the host waits for inputs the guest could not send"
    );
    s.run_to(60);
    s.assert_inputs(3);
    assert_eq!(s.net.lock().unwrap().refused, 25);
    assert_eq!(s.states(), [NetState::Running; 2]);
}

#[test]
fn receive_errors_are_skipped_and_keep_the_session() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(10);
    for _ in 0..20 {
        s.script(|n| {
            n.fail_recv.insert(addr(0), std::io::ErrorKind::ConnectionReset);
            n.fail_recv.insert(addr(1), std::io::ErrorKind::Other);
        });
        s.step(1);
    }
    assert!(s.net.lock().unwrap().fail_recv.is_empty(), "every error was hit");
    s.run_to(60);
    s.assert_inputs(3);
    assert!(s.nets.iter().all(|n| n.error().is_none()));
}

#[test]
fn a_transport_failure_ends_the_session_with_its_message() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(10);
    s.script(|n| {
        n.fail_transport.insert(addr(1), "the lobby is gone".into());
    });
    s.step(1);
    assert_eq!(s.nets[1].state(), NetState::Lost);
    assert_eq!(s.nets[1].error(), Some("the lobby is gone"));
    let stopped = s.ticks[1];
    s.step(10);
    assert_eq!(s.ticks[1], stopped, "a lost session runs no more ticks");
}

#[test]
fn a_silent_peer_is_lost_after_the_timeout_in_simulated_time() {
    for via in EVERY_VIA {
        let timeout = Duration::from_millis(500);
        let mut s = Session::via(via, 2, |n| n.timeout(timeout), |_| {});
        s.run_to(10);
        s.script(|n| n.cut = true);
        let cut_at = s.now;
        // Silence shorter than the timeout only stalls.
        while s.now - cut_at + STEP <= timeout {
            s.step(1);
            assert_eq!(s.states(), [NetState::Running; 2], "{via:?} after {:?}", s.now - cut_at);
        }
        s.step(3);
        assert_eq!(s.states(), [NetState::Lost; 2], "{via:?}");
        assert_eq!(s.nets[0].error(), Some("player 1 stopped answering"));
        assert_eq!(s.nets[1].error(), Some("player 0 stopped answering"));
        // The link coming back doesn't revive it.
        s.script(|n| n.cut = false);
        let stopped = s.ticks.clone();
        s.step(20);
        assert_eq!(s.ticks, stopped);
    }
}

#[test]
fn joining_gives_up_after_the_timeout_when_nobody_answers() {
    let timeout = Duration::from_millis(300);
    let net: Shared = Default::default();
    let mut guest = Net::transported(port(&net, 1), Some(addr(0)), 0).timeout(timeout);
    guest.begin(SEED);
    let start = Instant::now();
    let mut now = Duration::ZERO;
    while now <= timeout {
        assert!(!guest.ready_at(0, start + now));
        assert!(guest.waited(start + now), "still trying after {now:?}");
        assert_eq!(guest.state(), NetState::Connecting);
        now += STEP;
    }
    assert!(!guest.waited(start + now));
    assert_eq!(guest.state(), NetState::Lost);
    assert_eq!(guest.error(), Some("nobody answered"));
    // It kept saying hello, to the host only.
    let n = net.lock().unwrap();
    assert!(n.log.len() >= 3, "{} hellos", n.log.len());
    assert!(n.log.iter().all(|d| d.to == addr(0)));
}

#[test]
fn malformed_foreign_and_forged_packets_are_ignored() {
    let mut s = Session::new(3, |n| n, |_| {});
    s.run_to(10);
    let stranger = addr(9);
    let everyone: Vec<SocketAddr> = (0..3).map(addr).collect();
    for round in 0..30u64 {
        let far = s.ticks.iter().max().unwrap() + 3;
        s.script(|n| {
            for &to in &everyone {
                // Noise: too short, not ours, another protocol version, an
                // unknown kind, a cut-off packet.
                n.inject(stranger, to, vec![]);
                n.inject(stranger, to, b"CR".to_vec());
                n.inject(stranger, to, b"HTTP/1.1 200 OK\r\n\r\n".to_vec());
                let mut other_version = MAGIC.to_vec();
                other_version.extend([VERSION + 1, INPUTS, 3]);
                n.inject(addr(0), to, other_version);
                n.inject(addr(0), to, header(99));
                let cut_off = forged_inputs(3, 1, far, b"evil");
                n.inject(addr(1), to, cut_off[..cut_off.len() - 5].to_vec());
                // A stranger can't send inputs, say goodbye or welcome anyone.
                n.inject(stranger, to, forged_inputs(3, 1, far, b"evil"));
                n.inject(stranger, to, header(super::BYE));
                n.inject(stranger, to, header(super::START));
            }
            // Guests can't send the host's or each other's inputs to the host...
            n.inject(addr(1), addr(0), forged_inputs(3, 0, far, b"evil"));
            n.inject(addr(1), addr(0), forged_inputs(3, 2, far, b"evil"));
            n.inject(addr(2), addr(0), forged_inputs(3, 7, far, b"evil"));
            // ...a guest doesn't take inputs from another guest, and nobody
            // can replace a guest's own inputs.
            n.inject(addr(2), addr(1), forged_inputs(3, 0, far, b"evil"));
            n.inject(addr(0), addr(1), forged_inputs(3, 1, far + round, b"evil"));
        });
        s.step(1);
    }
    s.run_to(80);
    s.assert_inputs(3);
    assert_eq!(s.states(), [NetState::Running; 3]);
}

#[test]
fn oversized_datagrams_never_arrive() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(10);
    let far = s.ticks[0] + 3;
    s.script(|n| {
        let big = forged_inputs(2, 1, far, &vec![b'x'; 1600]);
        n.inject(addr(1), addr(0), big);
    });
    s.run_to(60);
    s.assert_inputs(3);
}

#[test]
fn packets_fit_the_limit_with_the_largest_inputs() {
    for players in [2, 8] {
        let mut s = Session::new(players, |n| n, |n| n.drop_every = 5);
        s.input_len = MAX_INPUT_BYTES;
        s.run_to(40);
        s.assert_inputs(3);
        let largest = s.net.lock().unwrap().largest;
        assert!(
            largest > MAX_INPUT_BYTES && largest <= PACKET,
            "{players} players: largest packet {largest}"
        );
    }
}

/// Through the relay, the largest lockstep packets travel inside the lobby
/// protocol's datagrams, which stay within the transport's MTU.
#[cfg(feature = "lobby")]
#[test]
fn relayed_packets_with_the_largest_inputs_fit_the_mtu() {
    for players in [2, 8] {
        let mut s = Session::relayed(players, |n| n, |n| Via::Relay.drop_one_in(n, 5));
        s.input_len = MAX_INPUT_BYTES;
        s.run_to(40);
        s.assert_inputs(3);
        assert_eq!(s.states(), vec![NetState::Running; players]);
        let largest = s.net.lock().unwrap().largest;
        assert!(
            largest > MAX_INPUT_BYTES && largest <= super::scripted::MTU,
            "{players} players: largest datagram {largest}"
        );
    }
}

#[test]
fn a_full_host_and_a_different_seed_refuse_the_joiner() {
    // Three endpoints, a host for two: the second joiner is one too many.
    let mut s = Session::with_seeds(2, &[SEED; 3], |n| n, |_| {});
    s.step(30);
    assert_eq!(s.states()[..2], [NetState::Running; 2]);
    assert_eq!(s.nets[2].state(), NetState::Lost);
    assert_eq!(s.nets[2].error(), Some("the session is full"));
    assert!(s.ticks[0] > 10 && s.ticks[2] == 0, "the session went on without it");

    let mut s = Session::with_seeds(2, &[SEED, SEED + 1], |n| n, |_| {});
    s.step(30);
    assert_eq!(s.nets[1].state(), NetState::Lost);
    assert_eq!(s.nets[1].error(), Some("the host uses a different Game::seed"));
    assert_eq!(s.nets[0].state(), NetState::Connecting, "the host keeps waiting");
}

#[test]
fn different_worlds_are_reported_on_both_sides() {
    let world = |x: f32| {
        let mut w = World::new();
        w.reflect::<crate::d2::Transform>();
        w.spawn((crate::d2::Transform::at(glam::vec2(x, 0.0)),));
        Some(w)
    };
    let mut same = Session::new(2, |n| n.checksum_every(4), |n| n.delay = 2);
    same.worlds = vec![world(1.0), world(1.0)];
    same.run_to(40);
    assert!(same.nets.iter().all(|n| n.desync().is_none()));

    for via in EVERY_VIA {
        let mut s = Session::via(via, 2, |n| n.checksum_every(4), |n| n.delay = 2);
        s.worlds = vec![world(1.0), world(2.0)];
        s.run_to(40);
        for (i, net) in s.nets.iter().enumerate() {
            let d = net
                .desync()
                .unwrap_or_else(|| panic!("{via:?}: peer {i} noticed nothing"));
            assert_eq!(
                (d.tick, d.player),
                (3, 1 - i),
                "the first checked tick, the other player"
            );
            assert_eq!(d.components, ["Transform2D"]);
        }
        assert_eq!(s.states(), [NetState::Running; 2], "a desync is reported, not fatal");
    }
}

#[test]
fn a_leaving_peer_resends_until_the_others_have_its_inputs() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(20);
    // The guest's last packets are lost: it ran ticks the host has no
    // inputs for yet, then stops.
    s.script(|n| {
        n.refuse.insert(addr(1), 3);
    });
    s.tick_clock();
    assert!(s.advance(1), "the guest runs one more tick");
    let last = s.ticks[1];
    let needed = last + 3; // its inputs reach `input_delay` ticks ahead
    let end = s.at() + Duration::from_secs(1);
    let mut steps = 0;
    loop {
        s.tick_clock();
        steps += 1;
        assert!(steps < 100, "the goodbye never finished");
        let now = s.at();
        let done = s.nets[1].linger_step(now, end);
        while s.advance(0) {}
        if done {
            break;
        }
    }
    assert!(steps > 1, "it had to resend first");
    assert_eq!(s.ticks[0], needed, "the host ran every tick the guest had inputs for");
    assert!(s.at() < end, "settled, not timed out");
    s.assert_inputs(3);
    // The goodbye arrived: the host knows the guest left.
    s.tick_clock();
    s.advance(0);
    let host = s.nets[0].session.as_ref().unwrap();
    assert!(host.peers[0].left);
}

#[test]
fn a_leaving_peer_gives_up_on_a_quiet_peer() {
    let mut s = Session::new(2, |n| n, |_| {});
    s.run_to(20);
    s.script(|n| n.cut = true);
    s.tick_clock();
    assert!(s.advance(1));
    let left_at = s.now;
    let end = s.at() + Duration::from_secs(1);
    loop {
        let now = s.at();
        if s.nets[1].linger_step(now, end) {
            break;
        }
        s.tick_clock();
        assert!(s.now - left_at < Duration::from_secs(1), "waited for the deadline");
    }
    let waited = s.now - left_at;
    assert!(waited > GONE - 3 * STEP && waited <= GONE + 3 * STEP, "{waited:?}");
}

#[test]
fn the_same_script_gives_the_same_traffic() {
    for via in EVERY_VIA {
        same_traffic(via);
    }
}

/// Two runs of one script send the same datagrams. Through the relay, the
/// lobby connections start with random nonces, so the datagrams compare
/// by sender, receiver, size and due tick.
fn same_traffic(via: Via) {
    let run = || -> Vec<Datagram> {
        let mut s = Session::via(
            via,
            3,
            |n| n,
            |n| {
                n.delay = 1;
                via.drop_one_in(n, 4);
                n.duplicate = true;
            },
        );
        s.run_to(60);
        std::mem::take(&mut s.net.lock().unwrap().log)
    };
    let (a, b) = (run(), run());
    assert!(a.len() > 100);
    if via == Via::Direct {
        assert_eq!(a, b);
    } else {
        let shape = |log: &[Datagram]| -> Vec<_> { log.iter().map(|d| (d.from, d.to, d.bytes.len(), d.due)).collect() };
        assert_eq!(shape(&a), shape(&b), "{via:?}");
    }
}
