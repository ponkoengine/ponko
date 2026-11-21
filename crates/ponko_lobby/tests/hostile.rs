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
                0 => self.send(&rng.bytes(rng.below(2048) as usize)),
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
                    mode: rng.text(1 + rng.below(300) as usize),
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
                _ => self.send(&rng.bytes(rng.below(8) as usize)),
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
    const WARM: usize = 3_000;

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
            vanishes_at: (i % 4 == 3).then_some(4_000 + i * 100),
        })
        .collect();
    let vanishing = hostile.iter().filter(|h| h.vanishes_at.is_some()).count();

    let mut rng = Rng(0x5eed_1234_5678_9abc);
    let mut codes: Vec<String> = Vec::new();
    let mut relayed = [0u64; HONEST];
    let (mut warm_live, mut peak_after_warm) = (0isize, isize::MIN);
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
        if step == WARM {
            warm_live = live();
        }
        if step > WARM {
            peak_after_warm = peak_after_warm.max(live());
        }
    }
    let stats = server.stats();
    let growth = peak_after_warm - warm_live;
    eprintln!(
        "hostile load: {STEPS} steps ({:.0} s simulated) in {:.1?}; live heap {:.2} MiB after warm-up, \
         peak {:.2} MiB later (growth {:+.1} KiB); {} requests, {} refused, {} rate limited, \
         {} relay in, {} dropped, {} rooms now, {} connections now",
        STEPS as f64 * STEP.as_secs_f64(),
        wall.elapsed(),
        warm_live as f64 / 1048576.0,
        peak_after_warm as f64 / 1048576.0,
        growth as f64 / 1024.0,
        stats.requests,
        stats.refused,
        stats.rate_limited,
        stats.relay_in,
        stats.relay_dropped,
        stats.rooms,
        stats.connections,
    );
    // Memory: whatever the attackers did after the first 30 s, the heap
    // grew by less than 1 MiB (no state per message, per guess, per
    // queue name or per refused request survives).
    assert!(growth < 1 << 20, "the heap grew by {growth} bytes under attack");
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
