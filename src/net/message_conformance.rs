//! Transport conformance tests for the message protocol ([`NetMessages`]).
//!
//! The message protocol (session handshake + renet lanes) sits on a packet
//! transport: UDP, the in-memory loopback, the Steam relay or the lobby
//! server's relay (feature `lobby`) through the internal `PacketTransport`
//! seam. The tests that loop over `EVERY_VIA` also run through an
//! in-memory lobby relay server, with the same API as directly. These tests drive that seam with a
//! deterministic scripted transport and simulated time (`poll_elapsed`), so they
//! need no sockets, no Steam installation and no renderer, and they pin what
//! the protocol does when the transport delays, loses, duplicates, reorders,
//! refuses or fails packets.
//!
//! # Transport contract (message protocol)
//!
//! What a packet transport must do, and what each existing transport does.
//! "Scripted" is the test transport below; it does each of these on request.
//!
//! | Contract | UDP | Loopback (`loopback_lossy`) | Steam relay | Lobby relay | Scripted |
//! |---|---|---|---|---|---|
//! | `recv` never blocks; `Ok(None)` means no packet | `WouldBlock` → `None` | yes | yes | yes (reads the lobby's inbox) | yes |
//! | Other errors are returned from `poll` and don't end sessions | socket errors | never errors | lobby/protocol-mismatch errors | a failed or closed lobby connection, or leaving the room, once | one-shot injection |
//! | `send` `Ok(false)` = not submitted, treated as loss | `WouldBlock` | never (a full inbox drops silently) | peer not (yet) a lobby member | out of the room or the lobby lane is full | refusal window |
//! | Never deliver more than the receive buffer (1500 bytes) | OS-dependent | no check; only engine packets enter | drops > 1500 | whole relay frames; refused at creation if the server relays less than 1504 bytes | drops > 1500 |
//! | Reject outgoing datagrams above 1500 bytes | OS limit | no | `InvalidInput` | the server's `max_relay_bytes` | `InvalidInput` |
//! | May reorder | yes | always (newest first) | per Steam | per the lobby's links | on request |
//! | May lose / duplicate | yes / yes | every nth send / no | per Steam | yes / no (the lobby's unreliable lane drops duplicates) | on request |
//! | Address identity | socket address | `127.0.0.1:(i + 1)` | synthetic address per Steam ID; non-members filtered | synthetic IPv6 per member id; a client only hears its host | synthetic |
//! | Clock | none; protocol time comes from `poll`/`poll_elapsed` | same | same | the lobby follows the protocol time (`advance`) | same |
//! | Disconnect | none; protocol `BYE` + silence timeout | same | same (lobby leave is separate) | same (leaving the room is an error from `poll`) | link cut on request |
//! | `local_addr()` | `Some` | `None` | `None` | `None` | `None` |
//! | Covered on CI by | `tests/net_messages.rs` (localhost) | `tests/net_messages.rs` | fake Steam API (`--features steam`) | this module (`Via::Relay`), `net/lobby/relay.rs`, `ponko_lobby`'s `tests/relay_sessions.rs` | this module |
//! | Replication on top, covered by | `net/replication_conformance.rs` (with WebSocket) | `net/replication_conformance.rs` | `net/steam_messages.rs` (`--features steam`) | `net/lobby/relay.rs` | `net/replication_conformance.rs` |
//!
//! Protocol guarantees pinned here, given any transport that keeps the
//! contract above:
//!
//! | Guarantee | Test |
//! |---|---|
//! | Delivery with latency, every lane | `delivers_every_lane_through_latency` |
//! | Reliable lanes: exactly once (ordered lane in order) under loss, duplication and reordering; unreliable: never stale or duplicated | `reliable_lanes_survive_loss_duplication_and_reordering` |
//! | Several clients each get every reliable message through random loss and jitter | `several_clients_get_every_reliable_message_through_random_loss_and_jitter` |
//! | Transport refusal (`Ok(false)`) is loss: not counted as sent, recovered by resends | `refused_sends_count_as_loss_and_recover` |
//! | Transport errors surface from `poll` once and keep the session | `transport_errors_surface_once_and_keep_the_session` |
//! | `disconnect` is reported once on both sides, even when sending fails | `explicit_disconnect_is_reported_once_on_each_side`, `disconnect_reports_locally_even_when_the_transport_fails` |
//! | Hello retries keep the same link alive while the welcome is lost | `hello_retries_keep_the_host_link_alive_until_welcome_arrives` |
//! | Rejoining from the same address gets a fresh clock and peer ID | `a_rejoining_client_gets_a_fresh_clock_and_peer_id` |
//! | Silence times out after the configured timeout, in simulated time | `silent_link_times_out_both_sides_in_simulated_time` |
//! | Short, foreign, stale and unknown datagrams are ignored | `malformed_and_foreign_datagrams_are_ignored` |
//! | An undecodable payload inside a session ends only that session | `undecodable_session_payload_disconnects_only_that_peer` |
//! | Oversized messages are refused before sending; emitted datagrams fit 1500 bytes | `oversized_messages_are_refused_and_datagrams_fit_the_mtu` |
//! | Undrained events make `poll` return `WouldBlock` without losing messages | `undrained_events_backpressure_poll_without_loss` |
//! | Peer IDs follow handshake order; a full host refuses | `peer_ids_follow_handshake_order_and_full_hosts_refuse` |
//! | A UDP host proves a client's address with a cookie before it keeps anything; replayed and expired cookies are refused; one extra round trip | cookie tests in `net/messages.rs`, `tests/net_cookies.rs` |
//! | The same script gives the same traffic | `same_script_gives_the_same_traffic` |
//!
//! # Lockstep is a different contract
//!
//! Lockstep (`Net`, the private `Transport` trait in `net.rs`) has its own
//! contract table and tests in `net/lockstep_conformance.rs`, on the same
//! scripted network (`net/scripted.rs`). That module also says which parts
//! the two protocols share and which differences are intended.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use super::scripted::{EVERY_VIA, MTU, Network, Port, Shared, Via, addr, port};
use crate::net::{Lane, MAX_MESSAGE_BYTES, MessageEvent, NetMessages, PeerId};

/// One scripted step: the network advances one tick, then every endpoint polls.
const STEP: Duration = Duration::from_millis(10);

/// A host (`nets[0]`, address `addr(0)`) and clients (`addr(1)`, ...), with
/// every event drained into `logs` after each poll. Through a relay, the
/// server is `addr(0)` and the host and clients `addr(1)`, ...
struct Session {
    net: Shared,
    nets: Vec<NetMessages>,
    logs: Vec<Vec<MessageEvent>>,
    /// The relay server, polled at every step, for endpoints through one.
    #[cfg(feature = "lobby")]
    relay: Option<super::lobby::relay::testing::RelayServer>,
}
impl Session {
    /// [`new`](Self::new), with the endpoints reaching each other `via` that.
    fn via(via: Via, clients: usize, max_clients: usize, script: impl FnOnce(&mut Network)) -> Self {
        match via {
            Via::Direct => Self::new(clients, max_clients, script),
            #[cfg(feature = "lobby")]
            Via::Relay => {
                // The room forms first, on the network already shaped by
                // `script`; then the room's host hosts the endpoint.
                let net: Shared = Default::default();
                script(&mut net.lock().unwrap());
                let (server, lobbies) = super::lobby::relay::testing::scripted_room(&net, clients + 1, STEP);
                let mut lobbies = lobbies.into_iter();
                let mut nets = vec![NetMessages::host_relay(lobbies.next().unwrap(), max_clients).unwrap()];
                nets.extend(lobbies.map(|l| NetMessages::join_relay(l).unwrap()));
                let logs = nets.iter().map(|_| Vec::new()).collect();
                Self {
                    net,
                    nets,
                    logs,
                    relay: Some(server),
                }
            }
        }
    }
    fn new(clients: usize, max_clients: usize, script: impl FnOnce(&mut Network)) -> Self {
        let net: Shared = Default::default();
        script(&mut net.lock().unwrap());
        let port = |i| {
            Box::new(Port {
                net: net.clone(),
                me: addr(i),
            })
        };
        let mut nets = vec![NetMessages::transported(port(0), None, max_clients)];
        for i in 1..=clients {
            nets.push(NetMessages::transported(port(i), Some(addr(0)), 1));
        }
        let logs = nets.iter().map(|_| Vec::new()).collect();
        Self {
            net,
            nets,
            logs,
            #[cfg(feature = "lobby")]
            relay: None,
        }
    }
    fn script(&self, f: impl FnOnce(&mut Network)) {
        f(&mut self.net.lock().unwrap());
    }
    fn step(&mut self, steps: usize) {
        for _ in 0..steps {
            self.net.lock().unwrap().tick += 1;
            #[cfg(feature = "lobby")]
            if let Some(relay) = &mut self.relay {
                relay.poll(STEP);
            }
            for i in 0..self.nets.len() {
                self.poll(i).unwrap();
            }
        }
    }
    fn poll(&mut self, i: usize) -> io::Result<()> {
        let result = self.nets[i].poll_elapsed(STEP);
        self.logs[i].extend(self.nets[i].events());
        result
    }
    /// Step until every client and the host have seen `Connected`; returns
    /// the steps it took.
    fn connect(&mut self) -> usize {
        for steps in 0..200 {
            if self.logs.iter().enumerate().all(|(i, log)| {
                let n = log.iter().filter(|e| matches!(e, MessageEvent::Connected(_))).count();
                n == if i == 0 { self.nets.len() - 1 } else { 1 }
            }) {
                return steps;
            }
            self.step(1);
        }
        panic!("session did not connect");
    }
    fn take(&mut self, i: usize) -> Vec<MessageEvent> {
        std::mem::take(&mut self.logs[i])
    }
}

/// The 21-byte header of the latest datagram `from` sent inside an established
/// session (non-zero server nonce), to forge packets with.
fn session_header(net: &Shared, from: SocketAddr) -> Vec<u8> {
    let n = net.lock().unwrap();
    let d = n.log.iter().rev().find(|d| d.from == from && d.bytes[13..21] != [0; 8]);
    d.expect("an established session").bytes[..21].to_vec()
}
fn messages(events: &[MessageEvent], lane: Lane) -> Vec<u32> {
    events
        .iter()
        .filter_map(|e| match e {
            MessageEvent::Message { lane: l, bytes, .. } if *l == lane => {
                Some(u32::from_le_bytes(bytes.as_slice().try_into().unwrap()))
            }
            _ => None,
        })
        .collect()
}
fn disconnects(events: &[MessageEvent]) -> Vec<(PeerId, String)> {
    events
        .iter()
        .filter_map(|e| match e {
            MessageEvent::Disconnected(p, why) => Some((*p, why.clone())),
            _ => None,
        })
        .collect()
}
fn is_strictly_increasing(v: &[u32]) -> bool {
    v.windows(2).all(|w| w[0] < w[1])
}

#[test]
fn delivers_every_lane_through_latency() {
    for via in EVERY_VIA {
        let instant = Session::via(via, 1, 1, |_| {}).connect();
        let mut s = Session::via(via, 1, 1, |n| n.delay = 5);
        let delayed = s.connect();
        if via == Via::Direct {
            // The handshake is one round trip: two one-way delays. Without
            // delay the host already polls one step after the client (poll
            // order), hence 9.
            assert_eq!(delayed, instant + 9);
        } else {
            // Through the relay each way is two hops.
            assert!(delayed >= instant + 18, "{via:?}: {delayed} vs {instant}");
        }
        s.take(1);
        let peer = PeerId::from_bits(1);
        for i in 0u32..50 {
            for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
                s.nets[0].send(peer, lane, &i.to_le_bytes()).unwrap();
            }
            s.step(1);
        }
        s.step(30);
        let got = s.take(1);
        let all: Vec<u32> = (0..50).collect();
        assert_eq!(messages(&got, Lane::Reliable), all, "{via:?}");
        assert_eq!(messages(&got, Lane::ReliableUnordered), all, "{via:?}");
        assert_eq!(messages(&got, Lane::Unreliable), all, "{via:?}");
        assert!(disconnects(&got).is_empty());
    }
}

#[test]
fn reliable_lanes_survive_loss_duplication_and_reordering() {
    for via in EVERY_VIA {
        let mut s = Session::via(via, 1, 1, |n| {
            n.delay = 2;
            via.drop_one_in(n, 3);
            n.duplicate = true;
            n.newest_first = true;
        });
        s.connect();
        s.take(1);
        let peer = PeerId::from_bits(1);
        for i in 0u32..200 {
            for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
                s.nets[0].send(peer, lane, &i.to_le_bytes()).unwrap();
            }
            if i % 4 == 0 {
                s.step(1);
            }
        }
        s.step(300);
        let got = s.take(1);
        assert_eq!(messages(&got, Lane::Reliable), (0..200).collect::<Vec<_>>(), "{via:?}");
        let mut unordered = messages(&got, Lane::ReliableUnordered);
        unordered.sort_unstable();
        assert_eq!(unordered, (0..200).collect::<Vec<_>>(), "exactly once");
        let unreliable = messages(&got, Lane::Unreliable);
        assert!(!unreliable.is_empty());
        assert!(is_strictly_increasing(&unreliable), "never stale or duplicated");
        assert!(disconnects(&got).is_empty());
    }
}

#[test]
fn several_clients_get_every_reliable_message_through_random_loss_and_jitter() {
    for via in EVERY_VIA {
        let mut s = Session::via(via, 3, 3, |n| {
            n.delay = 3;
            n.jitter = 5;
            n.loss_per_mille = 100;
        });
        s.connect();
        for i in 1..=3 {
            s.take(i);
        }
        for i in 0u32..100 {
            for c in 1..=3 {
                s.nets[0]
                    .send(PeerId::from_bits(c), Lane::Reliable, &i.to_le_bytes())
                    .unwrap();
            }
            s.step(1);
        }
        s.step(200);
        for c in 1..=3 {
            let got = s.take(c);
            assert_eq!(
                messages(&got, Lane::Reliable),
                (0..100).collect::<Vec<_>>(),
                "{via:?}, client {c}"
            );
            assert!(disconnects(&got).is_empty());
        }
    }
}

#[test]
fn refused_sends_count_as_loss_and_recover() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.take(1);
    let peer = PeerId::from_bits(1);
    s.script(|n| {
        n.refuse.insert(addr(0), u32::MAX);
    });
    let sent = s.nets[0].stats().packets_sent;
    s.nets[0].send(peer, Lane::Reliable, &7u32.to_le_bytes()).unwrap();
    s.step(30);
    assert_eq!(s.nets[0].stats().packets_sent, sent, "refused sends are not counted");
    assert!(s.net.lock().unwrap().refused > 0);
    assert!(messages(&s.logs[1], Lane::Reliable).is_empty());
    s.script(|n| {
        n.refuse.clear();
    });
    s.step(30);
    assert!(s.nets[0].stats().packets_sent > sent);
    let got = s.take(1);
    assert_eq!(messages(&got, Lane::Reliable), vec![7]);
    assert!(disconnects(&got).is_empty());
}

#[test]
fn transport_errors_surface_once_and_keep_the_session() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.take(0);
    s.take(1);
    let peer = PeerId::from_bits(1);

    s.script(|n| {
        n.fail_recv.insert(addr(1), io::ErrorKind::ConnectionReset);
    });
    s.net.lock().unwrap().tick += 1;
    let err = s.poll(1).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::ConnectionReset);
    s.poll(1).unwrap();

    s.nets[0].send(peer, Lane::Reliable, &1u32.to_le_bytes()).unwrap();
    s.script(|n| {
        n.fail_send.insert(addr(0), io::ErrorKind::PermissionDenied);
    });
    let err = s.poll(0).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    s.poll(0).unwrap();

    // The packet that failed is lost; reliable resends still deliver it.
    s.step(40);
    assert_eq!(messages(&s.take(1), Lane::Reliable), vec![1]);
    assert!(disconnects(&s.take(0)).is_empty());
}

#[test]
fn explicit_disconnect_is_reported_once_on_each_side() {
    let mut s = Session::new(1, 1, |n| n.delay = 3);
    s.connect();
    s.take(0);
    s.take(1);
    s.nets[1].disconnect(PeerId::HOST).unwrap();
    let local: Vec<_> = s.nets[1].events().collect();
    assert_eq!(disconnects(&local), vec![(PeerId::HOST, "disconnected locally".into())]);
    assert_eq!(
        s.nets[1].send(PeerId::HOST, Lane::Reliable, b"x").unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
    // Three BYE packets arrive; only one event, then nothing more after the timeout.
    s.step(1200);
    assert_eq!(
        disconnects(&s.take(0)),
        vec![(PeerId::from_bits(1), "peer disconnected".into())]
    );
    assert!(s.take(1).is_empty());
    assert_eq!(
        s.nets[0]
            .send(PeerId::from_bits(1), Lane::Reliable, b"x")
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotConnected
    );
}

#[test]
fn disconnect_reports_locally_even_when_the_transport_fails() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.take(0);
    s.take(1);
    s.script(|n| {
        n.fail_send.insert(addr(1), io::ErrorKind::BrokenPipe);
    });
    let err = s.nets[1].disconnect(PeerId::HOST).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(disconnects(&s.nets[1].events().collect::<Vec<_>>()).len(), 1);
    assert_eq!(
        s.nets[1].disconnect(PeerId::HOST).unwrap_err().kind(),
        io::ErrorKind::NotConnected
    );
}

#[test]
fn hello_retries_keep_the_host_link_alive_until_welcome_arrives() {
    let net: Shared = Default::default();
    let mut s = Session {
        nets: vec![
            NetMessages::transported(port(&net, 0), None, 1).timeout(Duration::from_millis(250)),
            NetMessages::transported(port(&net, 1), Some(addr(0)), 1),
        ],
        logs: vec![Vec::new(), Vec::new()],
        #[cfg(feature = "lobby")]
        relay: None,
        net,
    };
    s.script(|n| {
        n.refuse.insert(addr(0), u32::MAX);
    });
    s.step(50);
    assert_eq!(s.take(0), vec![MessageEvent::Connected(PeerId::from_bits(1))]);
    assert!(s.take(1).is_empty());

    s.script(|n| {
        n.refuse.remove(&addr(0));
    });
    s.step(15);
    assert!(s.take(0).is_empty());
    assert_eq!(s.take(1), vec![MessageEvent::Connected(PeerId::HOST)]);
}

#[test]
fn a_rejoining_client_gets_a_fresh_clock_and_peer_id() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    assert_eq!(s.take(0), vec![MessageEvent::Connected(PeerId::from_bits(1))]);
    s.take(1);

    s.nets[1].disconnect(PeerId::HOST).unwrap();
    s.step(1);
    assert_eq!(disconnects(&s.take(0)).len(), 1);
    s.take(1);

    let mut client = NetMessages::transported(port(&s.net, 1), Some(addr(0)), 1);
    client.poll_elapsed(Duration::from_secs(30)).unwrap();
    s.nets[1] = client;
    s.step(2);

    assert_eq!(s.take(0), vec![MessageEvent::Connected(PeerId::from_bits(2))]);
    assert_eq!(s.take(1), vec![MessageEvent::Connected(PeerId::HOST)]);
}

#[test]
fn silent_link_times_out_both_sides_in_simulated_time() {
    let net: Shared = Default::default();
    let port = |i| {
        Box::new(Port {
            net: net.clone(),
            me: addr(i),
        })
    };
    let timeout = Duration::from_secs(1);
    let nets = vec![
        NetMessages::transported(port(0), None, 1).timeout(timeout),
        NetMessages::transported(port(1), Some(addr(0)), 1).timeout(timeout),
    ];
    let mut s = Session {
        net,
        logs: vec![Vec::new(), Vec::new()],
        nets,
        #[cfg(feature = "lobby")]
        relay: None,
    };
    s.connect();
    s.take(0);
    s.take(1);
    s.script(|n| n.cut = true);
    // Heartbeats go every 100 ms, so the last packet arrived at most that long
    // before the cut: no timeout until 900 ms of silence, one by 1000 ms.
    let ticks = |d: Duration| (d.as_millis() / STEP.as_millis()) as usize;
    s.step(ticks(Duration::from_millis(900)) - 1);
    assert!(disconnects(&s.logs[0]).is_empty() && disconnects(&s.logs[1]).is_empty());
    s.step(ticks(Duration::from_millis(100)) + 1);
    assert_eq!(
        disconnects(&s.take(0)),
        vec![(PeerId::from_bits(1), "peer timed out".into())]
    );
    assert_eq!(disconnects(&s.take(1)), vec![(PeerId::HOST, "peer timed out".into())]);
    s.step(ticks(timeout) * 2);
    assert!(s.take(0).is_empty() && s.take(1).is_empty(), "reported once");
}

#[test]
fn malformed_and_foreign_datagrams_are_ignored() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.step(20);
    s.take(0);
    s.take(1);
    // The client's real session header, to forge variants of it.
    let real = session_header(&s.net, addr(1));
    let with = |kind: u8, client_nonce: Option<u64>| {
        let mut b = real.clone();
        b[4] = kind;
        if let Some(cn) = client_nonce {
            b[5..13].copy_from_slice(&cn.to_le_bytes());
        }
        b
    };
    let received = s.nets[0].stats().packets_received;
    let junk = [
        (addr(1), Vec::new()),                                     // empty
        (addr(1), real[..20].to_vec()),                            // shorter than the header
        (addr(1), [b"XXXX".as_slice(), &real[4..]].concat()),      // wrong magic
        (addr(1), with(99, None)),                                 // unknown kind
        (addr(1), [with(3, Some(12345)), vec![1, 2, 3]].concat()), // stale session nonce
        (addr(7), [with(3, None), vec![1, 2, 3]].concat()),        // unknown address
        (addr(7), with(2, None)),                                  // WELCOME to a host
    ];
    let count = junk.len() as u64;
    s.script(|n| {
        for (from, bytes) in junk {
            n.inject(from, addr(0), bytes);
        }
    });
    s.poll(0).unwrap();
    assert_eq!(
        s.nets[0].stats().packets_received,
        received + count,
        "counted, then ignored"
    );
    s.nets[1]
        .send(PeerId::HOST, Lane::Reliable, &5u32.to_le_bytes())
        .unwrap();
    s.step(10);
    let got = s.take(0);
    assert_eq!(messages(&got, Lane::Reliable), vec![5]);
    assert!(
        got.iter().all(|e| matches!(e, MessageEvent::Message { .. })),
        "no connects or disconnects: {got:?}"
    );
}

#[test]
fn undecodable_session_payload_disconnects_only_that_peer() {
    let mut s = Session::new(2, 2, |_| {});
    s.connect();
    s.step(20);
    s.take(0);
    let header = session_header(&s.net, addr(1));
    let mut forged = header;
    forged[4] = 3; // DATA with the session's nonces, but a payload renet can't decode
    forged.extend_from_slice(&[0xff; 8]);
    s.script(|n| n.inject(addr(1), addr(0), forged));
    s.step(1);
    let got = disconnects(&s.take(0));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, PeerId::from_bits(1));
    // The other client keeps its session.
    s.nets[2]
        .send(PeerId::HOST, Lane::Reliable, &9u32.to_le_bytes())
        .unwrap();
    s.step(10);
    let got = s.take(0);
    assert_eq!(messages(&got, Lane::Reliable), vec![9]);
    assert!(disconnects(&got).is_empty());
}

#[test]
fn oversized_messages_are_refused_and_datagrams_fit_the_mtu() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.take(1);
    let peer = PeerId::from_bits(1);
    let stats = s.nets[0].stats();
    let too_big = vec![0; MAX_MESSAGE_BYTES + 1];
    for lane in [Lane::Reliable, Lane::ReliableUnordered, Lane::Unreliable] {
        assert_eq!(
            s.nets[0].send(peer, lane, &too_big).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        s.nets[0].send_to_all(Lane::Reliable, &too_big).unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(s.nets[0].stats(), stats, "nothing was sent");

    let big: Vec<u8> = (0..200_000).map(|i| (i % 251) as u8).collect();
    s.nets[0].send(peer, Lane::Reliable, &big).unwrap();
    s.nets[0].send(peer, Lane::Unreliable, &big).unwrap();
    s.step(40);
    assert!(s.net.lock().unwrap().largest <= MTU);
    let got: Vec<_> = s
        .take(1)
        .into_iter()
        .filter_map(|e| match e {
            MessageEvent::Message { lane, bytes, .. } => Some((lane, bytes)),
            _ => None,
        })
        .collect();
    assert_eq!(got, vec![(Lane::Reliable, big.clone()), (Lane::Unreliable, big)]);
}

#[test]
fn undrained_events_backpressure_poll_without_loss() {
    let mut s = Session::new(1, 1, |_| {});
    s.connect();
    s.take(1);
    let peer = PeerId::from_bits(1);
    let total = 6000u32;
    for i in 0..total {
        s.nets[0].send(peer, Lane::Reliable, &i.to_le_bytes()).unwrap();
    }
    // The client polls without draining until it is told to.
    let mut blocked = false;
    for _ in 0..100 {
        s.net.lock().unwrap().tick += 1;
        s.nets[0].poll_elapsed(STEP).unwrap();
        match s.nets[1].poll_elapsed(STEP) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                blocked = true;
                break;
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(blocked, "poll should report full events");
    let mut got = Vec::new();
    while let Some(e) = s.nets[1].next_event() {
        got.push(e);
    }
    assert!(got.len() >= 4096);
    s.step(100);
    got.extend(s.take(1));
    assert_eq!(messages(&got, Lane::Reliable), (0..total).collect::<Vec<_>>());
}

#[test]
fn peer_ids_follow_handshake_order_and_full_hosts_refuse() {
    let mut s = Session::new(3, 2, |_| {});
    // Clients greet in a fixed order: only client 1, then client 2, then 3.
    let mut order = Vec::new();
    for c in 1..=3 {
        for _ in 0..30 {
            s.net.lock().unwrap().tick += 1;
            s.poll(c).unwrap();
            s.poll(0).unwrap();
        }
        order.extend(s.take(0));
    }
    s.step(30);
    assert_eq!(
        order,
        vec![
            MessageEvent::Connected(PeerId::from_bits(1)),
            MessageEvent::Connected(PeerId::from_bits(2)),
        ]
    );
    assert_eq!(disconnects(&s.take(3)), vec![(PeerId::HOST, "host is full".into())]);
    assert!(s.take(0).is_empty());
}

#[test]
fn same_script_gives_the_same_traffic() {
    for via in EVERY_VIA {
        let run = || {
            let mut s = Session::via(via, 1, 1, |n| {
                n.delay = 1;
                via.drop_one_in(n, 4);
                n.newest_first = true;
            });
            s.connect();
            for i in 0u32..100 {
                s.nets[0]
                    .send(PeerId::from_bits(1), Lane::Unreliable, &i.to_le_bytes())
                    .unwrap();
                s.nets[0]
                    .send(PeerId::from_bits(1), Lane::Reliable, &i.to_le_bytes())
                    .unwrap();
                s.step(1);
            }
            s.step(50);
            let stats = (s.nets[0].stats(), s.nets[1].stats());
            (s.take(0), s.take(1), stats)
        };
        assert_eq!(run(), run(), "{via:?}");
    }
}
