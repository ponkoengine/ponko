//! A deterministic scripted network for the transport conformance tests of
//! both protocols: `net/message_conformance.rs` ([`NetMessages`](crate::net::NetMessages),
//! through `PacketTransport`) and `net/lockstep_conformance.rs`
//! ([`Net`](crate::net::Net), through `Transport`). No sockets, no threads, no
//! wall clock: a test advances `Network::tick` and decides what the network
//! does to each datagram.

use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use super::Transport;
use super::messages::PacketTransport;

/// Largest datagram the protocol emits and the transports accept.
pub(super) const MTU: usize = 1500;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Datagram {
    pub(super) from: SocketAddr,
    pub(super) to: SocketAddr,
    pub(super) bytes: Vec<u8>,
    pub(super) due: u64,
}

/// The scripted network shared by every endpoint of one test.
#[derive(Default)]
pub(super) struct Network {
    pub(super) tick: u64,
    pub(super) in_flight: Vec<Datagram>,
    /// Ticks between send and earliest delivery.
    pub(super) delay: u64,
    /// Drop every nth submitted datagram (zero: none).
    pub(super) drop_every: u64,
    /// Drop this many of every 1000 submitted datagrams, picked by a hash
    /// of their number (the same script loses the same ones).
    pub(super) loss_per_mille: u64,
    /// Extra ticks of delay, 0 to this many per datagram, picked by a hash
    /// of its number: datagrams overtake each other.
    pub(super) jitter: u64,
    /// Deliver each datagram twice.
    pub(super) duplicate: bool,
    /// Deliver the newest ready datagram first.
    pub(super) newest_first: bool,
    /// Drop every datagram (a cut link).
    pub(super) cut: bool,
    /// Per sender: refuse this many sends with `Ok(false)`.
    pub(super) refuse: BTreeMap<SocketAddr, u32>,
    /// Per sender / receiver: fail the next send / recv with this error kind.
    pub(super) fail_send: BTreeMap<SocketAddr, io::ErrorKind>,
    pub(super) fail_recv: BTreeMap<SocketAddr, io::ErrorKind>,
    /// Per endpoint: a failure the transport reports on its own (lockstep's
    /// `Transport::error`), once.
    pub(super) fail_transport: BTreeMap<SocketAddr, String>,
    /// Counters for assertions.
    pub(super) submitted: u64,
    pub(super) refused: u64,
    pub(super) largest: usize,
    /// Every submitted datagram, for copying real headers.
    pub(super) log: Vec<Datagram>,
}
impl Network {
    pub(super) fn inject(&mut self, from: SocketAddr, to: SocketAddr, bytes: Vec<u8>) {
        let due = self.tick;
        self.in_flight.push(Datagram { from, to, bytes, due });
    }
}

pub(super) type Shared = Arc<Mutex<Network>>;

pub(super) struct Port {
    pub(super) net: Shared,
    pub(super) me: SocketAddr,
}
impl PacketTransport for Port {
    fn send(&mut self, to: SocketAddr, bytes: &[u8]) -> io::Result<bool> {
        let mut n = self.net.lock().unwrap();
        if bytes.len() > MTU {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "datagram exceeds MTU"));
        }
        if let Some(kind) = n.fail_send.remove(&self.me) {
            return Err(io::Error::new(kind, "scripted send failure"));
        }
        if let Some(left) = n.refuse.get_mut(&self.me)
            && *left > 0
        {
            *left -= 1;
            n.refused += 1;
            return Ok(false);
        }
        n.submitted += 1;
        n.largest = n.largest.max(bytes.len());
        let roll = mix(n.submitted);
        let jitter = if n.jitter == 0 {
            0
        } else {
            (roll >> 32) % (n.jitter + 1)
        };
        let datagram = Datagram {
            from: self.me,
            to,
            bytes: bytes.to_vec(),
            due: n.tick + n.delay + jitter,
        };
        n.log.push(datagram.clone());
        let lost = n.loss_per_mille != 0 && roll % 1000 < n.loss_per_mille;
        if n.cut || lost || (n.drop_every != 0 && n.submitted.is_multiple_of(n.drop_every)) {
            return Ok(true);
        }
        if n.duplicate {
            n.in_flight.push(datagram.clone());
        }
        n.in_flight.push(datagram);
        Ok(true)
    }
    fn recv(&mut self, buf: &mut [u8]) -> io::Result<Option<(SocketAddr, usize)>> {
        let mut n = self.net.lock().unwrap();
        if let Some(kind) = n.fail_recv.remove(&self.me) {
            return Err(io::Error::new(kind, "scripted recv failure"));
        }
        loop {
            let tick = n.tick;
            let mut ready = n
                .in_flight
                .iter()
                .enumerate()
                .filter(|(_, d)| d.to == self.me && d.due <= tick);
            let found = if n.newest_first {
                ready.next_back()
            } else {
                ready.next()
            };
            let Some(i) = found.map(|(i, _)| i) else {
                return Ok(None);
            };
            let d = n.in_flight.remove(i);
            // Contract: drop datagrams that don't fit, never truncate them.
            if d.bytes.len() > buf.len() || d.bytes.len() > MTU {
                continue;
            }
            buf[..d.bytes.len()].copy_from_slice(&d.bytes);
            return Ok(Some((d.from, d.bytes.len())));
        }
    }
}

/// Lockstep's view of the same port. Its sends are fire-and-forget, so a
/// refused or failed send is a lost packet; a failed receive is skipped,
/// as the UDP transport does.
impl Transport for Port {
    fn send(&mut self, to: SocketAddr, data: &[u8]) {
        let _ = PacketTransport::send(self, to, data);
    }
    fn recv(&mut self, buf: &mut [u8]) -> Option<(SocketAddr, usize)> {
        loop {
            match PacketTransport::recv(self, buf) {
                Ok(packet) => return packet,
                Err(_) => continue, // one-shot: the next call succeeds
            }
        }
    }
    fn local_addr(&self) -> SocketAddr {
        self.me
    }
    fn error(&mut self) -> Option<String> {
        self.net.lock().unwrap().fail_transport.remove(&self.me)
    }
}

/// A fixed scramble of `n` (splitmix64): the scripted network's dice.
fn mix(n: u64) -> u64 {
    let mut z = n.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

pub(super) fn addr(i: usize) -> SocketAddr {
    SocketAddr::from(([10, 0, 0, 1], 1000 + i as u16))
}

/// A port of `net` at `addr(i)`.
pub(super) fn port(net: &Shared, i: usize) -> Box<Port> {
    Box::new(Port {
        net: net.clone(),
        me: addr(i),
    })
}
