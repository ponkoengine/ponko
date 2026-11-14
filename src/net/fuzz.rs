//! Fuzzing entry points for the two network protocols: whatever arrives
//! from the network must never crash, hang or make a peer allocate without
//! bound. The `fuzz/` crate's targets call these (built with `--cfg
//! fuzzing`, as `cargo fuzz` does), and the tests at the bottom run them
//! over hand-made inputs on every `cargo test`.
//!
//! Both take the fuzzer's bytes as a list of datagrams, each written as
//! `[flags: u8][length: u16 LE][bytes]`, and feed them to a session that
//! already connected over the in-memory loopback, so the fuzzer reaches the
//! parsers behind the handshake. Flags:
//!
//! - bit 0: deliver to the host (0) or the joiner (1);
//! - bit 1: from the other peer (1) or a stranger's address (0);
//! - bit 2: write a valid header first (the protocol's magic, version and,
//!   for messages, a kind byte from the datagram and the link's nonces),
//!   so mutations go to what follows.
//!
//! The clock moves 10 ms per datagram, and both peers run a tick or poll
//! after each one, so resends, checksums and timeouts run too.

use std::net::SocketAddr;
use std::time::Duration;

use super::messages::{MessageEvent, NetMessages};
use super::{Local, MAGIC, Net, NetState, Settings, VERSION};
use crate::perf::Instant;
use ponko_ecs::World;

/// Splits fuzzer bytes into `(flags, datagram)` pairs.
fn datagrams(mut data: &[u8]) -> impl Iterator<Item = (u8, &[u8])> {
    std::iter::from_fn(move || {
        let (&flags, rest) = data.split_first()?;
        let len = match rest {
            [a, b, ..] => usize::from(u16::from_le_bytes([*a, *b])),
            _ => return None,
        };
        let rest = &rest[2..];
        let len = len.min(rest.len());
        let (packet, rest) = rest.split_at(len);
        data = rest;
        Some((flags, packet))
    })
}

const STRANGER: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 9)), 9);

/// Feeds lockstep datagrams to a running two-player session ([`Net`]).
pub fn lockstep_packets(data: &[u8]) {
    let start = Instant::now();
    let mut nets = Net::loopback(2);
    for net in &mut nets {
        // Dropping a started session waits for its peers on the wall clock.
        net.linger = false;
        net.begin(7);
    }
    let world = World::new();
    let mut ticks = [0u64; 2];
    let mut now = start;
    let step = |nets: &mut [Net], now: Instant, ticks: &mut [u64; 2]| {
        for (i, net) in nets.iter_mut().enumerate() {
            net.set_input_bytes(&[i as u8, ticks[i] as u8]);
            if net.ready_at(ticks[i], now) {
                net.end_tick(&world, ticks[i]);
                ticks[i] += 1;
            }
        }
    };
    // Connect: both peers run a few ticks.
    for _ in 0..50 {
        now += Duration::from_millis(10);
        step(&mut nets, now, &mut ticks);
        if ticks.iter().all(|&t| t >= 4) {
            break;
        }
    }
    debug_assert!(ticks.iter().all(|&t| t >= 4), "the loopback session runs");
    let mut packet = Vec::new();
    for (flags, bytes) in datagrams(data) {
        let to = usize::from(flags & 1);
        let from = if flags & 2 != 0 { Local::addr(1 - to) } else { STRANGER };
        packet.clear();
        if flags & 4 != 0 {
            packet.extend_from_slice(&MAGIC);
            packet.push(VERSION);
        }
        packet.extend_from_slice(bytes);
        let net = &mut nets[to];
        if net.state() != NetState::Lost
            && let Some(s) = net.session.as_deref_mut()
        {
            s.receive(
                from,
                &packet,
                now,
                &mut Settings {
                    player: &mut net.player,
                    players: &mut net.players,
                    delay: &mut net.delay,
                    checksum_every: &mut net.checksum_every,
                    desync: &mut net.desync,
                },
            );
        }
        now += Duration::from_millis(10);
        step(&mut nets, now, &mut ticks);
    }
}

/// Feeds message-protocol datagrams to a connected host and client
/// ([`NetMessages`]); received messages are decoded as JSON, as
/// [`MessageEvent::decode`](super::MessageEvent::decode) does for games.
pub fn message_packets(data: &[u8]) {
    let mut nets = NetMessages::loopback(2);
    // Returns how many connections it saw.
    let drain = |nets: &mut [NetMessages]| {
        let mut connected = 0;
        for net in nets.iter_mut() {
            while let Some(event) = net.next_event() {
                connected += usize::from(matches!(event, MessageEvent::Connected(_)));
                let _ = event.decode::<serde_json::Value>();
            }
        }
        connected
    };
    let poll = |nets: &mut [NetMessages]| {
        for net in nets.iter_mut() {
            // `WouldBlock` (events full) is drained below; socket errors can't happen in memory.
            let _ = net.poll_elapsed(Duration::from_millis(10));
        }
    };
    // Hellos go out every 100 ms.
    let mut connected = 0;
    for _ in 0..50 {
        poll(&mut nets);
        connected += drain(&mut nets);
        if connected == 2 {
            break;
        }
    }
    debug_assert_eq!(connected, 2, "the loopback host and client connect");
    for (flags, bytes) in datagrams(data) {
        let to = usize::from(flags & 1);
        let from = if flags & 2 != 0 {
            NetMessages::loopback_addr(1 - to)
        } else {
            STRANGER
        };
        // Like a datagram from the wire: at most 1500 bytes.
        let packet = &bytes[..bytes.len().min(1500)];
        let _ = nets[to].fuzz_receive(from, packet, flags & 4 != 0);
        poll(&mut nets);
        drain(&mut nets);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flags: u8, bytes: &[u8]) -> Vec<u8> {
        let mut out = vec![flags];
        out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(bytes);
        out
    }

    #[test]
    fn datagrams_split_records_and_cut_the_last_short() {
        let mut data = record(1, b"abc");
        data.extend(record(6, b""));
        data.extend([3, 9, 0, b'x']);
        let got: Vec<_> = datagrams(&data).collect();
        assert_eq!(got, [(1, &b"abc"[..]), (6, &b""[..]), (3, &b"x"[..])]);
    }

    #[test]
    fn lockstep_inputs_past_the_last_tick_are_ignored() {
        // INPUTS from the joiner: no acks, one stream (the joiner's), first
        // tick u64::MAX with two ticks: `first + t` must not overflow.
        let mut inputs = vec![super::super::INPUTS, 0, 1, 1];
        inputs.extend_from_slice(&u64::MAX.to_le_bytes());
        inputs.push(2);
        for _ in 0..2 {
            inputs.extend_from_slice(&1u16.to_le_bytes());
            inputs.push(42);
        }
        lockstep_packets(&record(0b110, &inputs));
    }

    #[test]
    fn lockstep_survives_garbage_and_forged_headers() {
        let mut data = Vec::new();
        for kind in 0..=12u8 {
            for flags in 0..8u8 {
                data.extend(record(
                    flags,
                    &[kind, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 7],
                ));
                data.extend(record(flags, &[kind]));
            }
        }
        lockstep_packets(&data);
    }

    #[test]
    fn messages_survive_garbage_and_forged_headers() {
        let mut data = Vec::new();
        for kind in 0..=8u8 {
            for flags in 0..8u8 {
                let mut bytes = vec![b'C', b'R', b'M', b'1', kind];
                bytes.extend([0xab; 30]);
                data.extend(record(flags, &bytes));
                data.extend(record(flags, &[kind; 3]));
            }
        }
        message_packets(&data);
    }
}
