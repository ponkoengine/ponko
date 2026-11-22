//! The UDP message host's address cookies (book/src/security.md, "Forged
//! hellos"), over real sockets on localhost.
//!
//! A sender that never reads the challenge is, to the host, the same as a
//! forged source address: the host can't tell them apart, so these tests
//! stand in for spoofed traffic without raw sockets. One file of its own:
//! it installs a counting allocator for the host's thread.
use ponko::net::{MessageEvent, NetMessages, PeerId};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// A hello of the current protocol (version 2) without a cookie, padded
/// to 64 bytes as clients send it.
fn hello(client_nonce: u64) -> Vec<u8> {
    let mut packet = b"CRM2".to_vec();
    packet.push(1);
    packet.extend_from_slice(&client_nonce.to_le_bytes());
    packet.extend_from_slice(&0u64.to_le_bytes());
    packet.resize(64, 0);
    packet
}

#[test]
fn hellos_from_addresses_that_never_answer_leave_no_connection_and_no_memory() {
    let mut host = NetMessages::host("127.0.0.1:0", 4).unwrap();
    let to = host.local_addr().unwrap();
    host.poll_elapsed(Duration::ZERO).unwrap();
    // 32 "forged" addresses, 40 hellos each, every one with a new nonce.
    let senders: Vec<UdpSocket> = (0..32).map(|_| UdpSocket::bind("127.0.0.1:0").unwrap()).collect();
    let mut sent = 0usize;
    for round in 0..40u64 {
        for (i, s) in senders.iter().enumerate() {
            s.send_to(&hello(round * 100 + i as u64 + 1), to).unwrap();
            sent += 1;
        }
    }
    // Poll until the host has taken every hello in, counting what its
    // thread allocates meanwhile.
    let started = Instant::now();
    let deadline = started + Duration::from_secs(10);
    ALLOCS.with(|n| n.set(0));
    TRACK.with(|t| t.set(true));
    while host.stats().packets_received < sent as u64 && Instant::now() < deadline {
        host.poll_elapsed(Duration::from_millis(1)).unwrap();
    }
    TRACK.with(|t| t.set(false));
    let polling = started.elapsed();
    let allocations = ALLOCS.with(Cell::get);
    // Negative control: the counter does see this thread's allocations.
    TRACK.with(|t| t.set(true));
    std::hint::black_box(Box::new(1u8));
    TRACK.with(|t| t.set(false));
    assert_eq!(ALLOCS.with(Cell::get), allocations + 1, "the counter counts");
    assert_eq!(host.stats().packets_received, sent as u64, "every hello arrived");
    assert_eq!(host.connected_peers(), 0, "the connection table stays empty");
    assert_eq!(host.events().count(), 0);
    assert_eq!(allocations, 0, "no allocation for unproven addresses");

    // Each answer is a challenge of 41 bytes, smaller than the hello. All
    // the senders are on one network (127.0.0.0/24), which gets 16 answers
    // a second, not the host's 256 for everyone.
    let mut buf = [0u8; 1500];
    let mut answers = 0u64;
    for s in &senders {
        s.set_nonblocking(true).unwrap();
        while let Ok(n) = s.recv(&mut buf) {
            assert!(n <= 64, "an answer of {n} bytes outgrows the hello");
            assert_eq!(buf[4], 7, "a challenge");
            answers += 1;
        }
    }
    assert_eq!(answers, host.stats().packets_sent);
    assert!(answers > 0, "the host did answer");
    let budget = 16 * (1 + polling.as_secs() + 1);
    assert!(answers <= budget, "{answers} challenges to one network in {polling:?}");
}

#[test]
fn a_udp_client_connects_after_one_extra_round_trip() {
    let mut host = NetMessages::host("127.0.0.1:0", 4).unwrap();
    let mut client = NetMessages::join(host.local_addr().unwrap()).unwrap();
    // Protocol time stands still, so nothing is resent: each poll answers
    // only what arrived.
    let poll_until_received = |net: &mut NetMessages, count: u64| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            net.poll_elapsed(Duration::ZERO).unwrap();
            if net.stats().packets_received >= count || Instant::now() > deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    host.poll_elapsed(Duration::ZERO).unwrap();
    client.poll_elapsed(Duration::ZERO).unwrap(); // hello
    poll_until_received(&mut host, 1); // challenge
    poll_until_received(&mut client, 1); // hello with the cookie
    assert_eq!(client.connected_peers(), 0);
    poll_until_received(&mut host, 2); // welcome
    poll_until_received(&mut client, 2); // connected
    assert_eq!(client.events().next(), Some(MessageEvent::Connected(PeerId::HOST)));
    assert!(host.events().any(|e| matches!(e, MessageEvent::Connected(_))));
    assert_eq!((client.stats().packets_sent, host.stats().packets_sent), (2, 2));
}
