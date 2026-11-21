//! A load test with hostile clients: the server's memory stays flat while
//! they flood it with everything the protocol lets a stranger send, and
//! the honest players' rooms keep working.
//!
//! Everything runs in memory on a simulated clock (the same protocol,
//! framing and reliability layer as UDP), and the heap of the whole process
//! (server and every client) is counted by the allocator below.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;

use ponko::net::lobby::protocol::{self, Request};
use ponko::net::lobby::{LobbyClient, LobbyEvent};
use ponko::net::{Lane, NetMessages, PeerId};
use ponko_lobby::{AddressKey, LobbyLimits, LobbyServer};

/// Bytes allocated and not freed, by anyone in this test binary.
static LIVE: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: forwards to the system allocator, counting sizes on the way.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size as isize - layout.size() as isize, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const STEP: Duration = Duration::from_millis(10);

/// xorshift64*: the same "random" attacks every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
    fn text(&mut self, n: usize) -> String {
        (0..n).map(|_| char::from(b'a' + self.below(26) as u8)).collect()
    }
}

/// A hostile client: a raw endpoint speaking the wire protocol directly.
struct Hostile {
    net: NetMessages,
    /// Reads nothing it is sent (its replies pile up on the server's side
    /// of the link until the link's own limits apply).
    deaf: bool,
    /// Stops polling at this step: the link goes half-open, without a
    /// goodbye.
    vanishes_at: Option<usize>,
}

impl Hostile {
    fn send(&mut self, bytes: &[u8]) {
        // Full queues are the attacker's problem, not the test's.
        let _ = self.net.send(PeerId::HOST, Lane::Reliable, bytes);
    }

    /// One step's worth of abuse.
    fn attack(&mut self, rng: &mut Rng, codes: &[String]) {
        for _ in 0..4 {
            match rng.below(10) {
                // Garbage, and requests far past the size limit.
                0 => {
                    let n = rng.below(2048) as usize;
                    self.send(&rng.bytes(n));
                }
                1 => self.send(&protocol::encode_control(&Request::SetMeta {
                    meta: rng.text(64 * 1024),
                })),
                // Room churn: make rooms with the most text, leave them.
                2 => self.send(&protocol::encode_control(&Request::Create {
                    size: 2 + rng.below(20) as u16,
                    meta: rng.text(256),
                })),
                3 => self.send(&protocol::encode_control(&Request::Leave)),
                // Queues under names never seen before, then cancelled.
                4 => self.send(&protocol::encode_control(&Request::Queue {
                    mode: {
                        let n = 1 + rng.below(300) as usize;
                        rng.text(n)
                    },
                    size: 2 + rng.below(8) as u16,
                    rating: rng.next() as i32,
                })),
                5 => self.send(&protocol::encode_control(&Request::CancelQueue)),
                // Guessed codes, and the honest rooms' real ones.
                6 => {
                    let code = match codes.get(rng.below(codes.len() as u64 + 1) as usize) {
                        Some(code) => code.clone(),
                        None => rng.text(6).to_ascii_uppercase(),
                    };
                    self.send(&protocol::encode_control(&Request::Join { code }));
                }
                // Relay floods: the largest payloads, oversized ones, to
                // everyone and to members that don't exist.
                7 | 8 => {
                    let mut frame = Vec::new();
                    let to = if rng.below(2) == 0 {
                        protocol::EVERYONE
                    } else {
                        rng.next() as u32
                    };
                    let len = if rng.below(4) == 0 { 64 * 1024 } else { 4096 };
                    protocol::encode_relay(to, &rng.bytes(len), &mut frame);
                    let lane = if rng.below(2) == 0 {
                        Lane::Reliable
                    } else {
                        Lane::Unreliable
                    };
                    let _ = self.net.send(PeerId::HOST, lane, &frame);
                }
                _ => {
                    let n = rng.below(8) as usize;
                    self.send(&rng.bytes(n));
                }
            }
        }
    }
}

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

#[test]
fn hostile_clients_keep_server_memory_flat_and_honest_rooms_working() {
    const HONEST: usize = 4;
    const HOSTILE: usize = 12;
    // 120 simulated seconds: every timeout (idle connections, silent
    // links, rate windows) runs many times over.
    const STEPS: usize = 12_000;
    // Past every vanished client's link timeout (the last vanishes at 31 s
    // and is dropped 10 s later): until then the in-memory transport keeps
    // the packets the server still sends them (the attackers' memory, not
    // the server's, but the same heap here).
    const WARM: usize = 5_000;

    let mut limits = LobbyLimits::default();
    // Every in-memory client has the same IP: give each its own budgets,
    // the harder case for the server (as many attackers as connections).
    limits.address_key = AddressKey::Connection;
    let mut server = LobbyServer::new(limits).seed(9);
    let mut endpoints = NetMessages::loopback(1 + HONEST + HOSTILE).into_iter();
    server.listen(endpoints.next().unwrap());
    let mut honest: Vec<LobbyClient> = endpoints.by_ref().take(HONEST).map(LobbyClient::new).collect();
    let mut hostile: Vec<Hostile> = endpoints
        .enumerate()
        .map(|(i, net)| Hostile {
            net,
            deaf: i % 3 == 1,
            vanishes_at: (i % 4 == 3).then_some(2_000 + i * 100),
        })
        .collect();
    let vanishing = hostile.iter().filter(|h| h.vanishes_at.is_some()).count();

    let mut rng = Rng(0x5eed_1234_5678_9abc);
    let mut codes: Vec<String> = Vec::new();
    let mut relayed = [0u64; HONEST];
    // The live heap once a simulated second after the warm-up.
    let mut samples: Vec<isize> = Vec::new();
    let wall = std::time::Instant::now();
    for step in 0..STEPS {
        server.poll_elapsed(STEP);
        // The honest players: two rooms of two, relaying every step.
        for (i, client) in honest.iter_mut().enumerate() {
            client.poll_elapsed(STEP).unwrap();
            while let Some(event) = client.next_event() {
                match event {
                    LobbyEvent::Joined(room) if i % 2 == 0 => codes.push(room.code.clone()),
                    LobbyEvent::Relayed { .. } => relayed[i] += 1,
                    _ => {}
                }
            }
            if step == 50 && i % 2 == 0 {
                client.create_room(2).unwrap();
            }
            if step > 100 && client.room().is_some() {
                let _ = client.relay(None, Lane::Unreliable, &[i as u8; 64]);
            }
        }
        if step == 100 {
            assert_eq!(codes.len(), HONEST / 2, "the honest hosts made their rooms");
            for (i, client) in honest.iter_mut().enumerate().filter(|(i, _)| i % 2 == 1) {
                client.join_room(&codes[i / 2]).unwrap();
            }
        }
        for h in &mut hostile {
            if h.vanishes_at.is_some_and(|at| step >= at) {
                continue;
            }
            let _ = h.net.poll_elapsed(STEP);
            if !h.deaf {
                while h.net.next_event().is_some() {}
            }
            // The real codes once the honest rooms are full (before, a
            // guess that wins takes the honest joiner's seat, as it would).
            h.attack(&mut rng, if step > 300 { &codes } else { &[] });
        }
        if step % 1000 == 0 {
            let s = server.stats();
            eprintln!(
                "t={:>4} s live={:.2} MiB conns={} rooms={} queued={} socket_errors={}",
                step / 100,
                live() as f64 / 1048576.0,
                s.connections,
                s.rooms,
                s.queued,
                s.socket_errors
            );
        }
        if step >= WARM && step % 100 == 0 {
            samples.push(live());
        }
    }
    let stats = server.stats();
    let mean = |s: &[isize]| s.iter().sum::<isize>() / s.len() as isize;
    let (first, second) = samples.split_at(samples.len() / 2);
    let (before, after) = (mean(first), mean(second));
    let peak = samples.iter().copied().max().unwrap_or(0);
    let mib = |b: isize| b as f64 / 1048576.0;
    eprintln!(
        "hostile load: {STEPS} steps ({:.0} s simulated) in {:.1?}; live heap {:.2} MiB on average from \
         {} to {} s, {:.2} MiB from {} to {} s, peak {:.2} MiB; {} requests, {} refused, {} rate limited, \
         {} relay in, {} dropped, {} rooms now, {} connections now",
        STEPS as f64 * STEP.as_secs_f64(),
        wall.elapsed(),
        mib(before),
        WARM / 100,
        WARM / 100 + first.len(),
        mib(after),
        WARM / 100 + first.len(),
        STEPS / 100,
        mib(peak),
        stats.requests,
        stats.refused,
        stats.rate_limited,
        stats.relay_in,
        stats.relay_dropped,
        stats.rooms,
        stats.connections,
    );
    // Memory is flat: after the first 50 s the heap has no upward trend
    // (its average over the last 35 s is within 512 KiB of the 35 s
    // before: no state per message, per guess, per queue name or per
    // refused request survives; at about 3000 refused requests a second a
    // leak of a few bytes each would show), and no moment is more than
    // 2 MiB above that average (what a step's attacks have in flight). The
    // deaf attackers' own unread events fill slowly in the same heap.
    assert!(
        after - before < 512 << 10,
        "the heap grew under attack: {:.2} MiB, then {:.2} MiB on average",
        mib(before),
        mib(after)
    );
    assert!(peak - before < 2 << 20, "the heap reached {:.2} MiB", mib(peak));
    // The attackers hit every limit.
    assert!(stats.refused > 1000 && stats.rate_limited > 1000, "{stats:?}");
    assert!(stats.relay_dropped > 1000, "{stats:?}");
    // The vanished ones were dropped (half-open links time out), the rest
    // still connected: nobody was dropped for being refused.
    assert_eq!(stats.connections, HONEST + HOSTILE - vanishing, "{stats:?}");
    // The honest rooms stayed up and their relay kept flowing: a member
    // hears its partner every step, all but the few lost to its own
    // budget being shared with nobody.
    assert!(
        honest.iter().all(|c| c.room().is_some()),
        "an honest player lost its room"
    );
    let expected = (STEPS - 200) as u64;
    for (i, &n) in relayed.iter().enumerate() {
        assert!(
            n * 10 >= expected * 9,
            "player {i} heard {n} of about {expected} relay messages"
        );
    }
}
