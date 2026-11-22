//! Reading a damaged asset pack doesn't reserve the memory its index
//! claims, and an entry that inflates past the per-file cap never holds more
//! than the cap. This file has its own process and its tests take turns, so
//! its allocator sees only the running test's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use std::sync::Mutex;

use ponko::assets::{AssetSource, PackLimits, PackSource};

/// The tests share the allocator's counter: one at a time.
static TURN: Mutex<()> = Mutex::new(());

/// The system allocator, noting the largest single request.
struct Largest;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

// SAFETY: forwards every call to the system allocator unchanged.
unsafe impl GlobalAlloc for Largest {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed on.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::Relaxed);
        // SAFETY: the caller's contract, passed on.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Largest = Largest;

#[test]
fn a_compressed_entry_claiming_a_gigabyte_reserves_only_what_it_can_inflate_to() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // One deflate entry: 16 stored bytes whose index says they unpack to 1 GiB.
    let name = b"big.bin";
    let mut index = Vec::new();
    index.extend((name.len() as u16).to_le_bytes());
    index.extend(name);
    index.push(1); // deflate
    index.extend(0u64.to_le_bytes()); // offset
    index.extend(16u64.to_le_bytes()); // stored
    index.extend((1u64 << 30).to_le_bytes()); // size
    index.extend(0u32.to_le_bytes()); // crc
    let mut pack = b"PONKOPAK".to_vec();
    pack.extend(1u32.to_le_bytes());
    pack.extend(1u32.to_le_bytes());
    pack.extend((index.len() as u64).to_le_bytes());
    pack.extend(16u64.to_le_bytes());
    pack.extend(index);
    pack.extend([0x4b; 16]);

    let pack = PackSource::from_bytes("damaged", pack).unwrap();
    LARGEST.store(0, Ordering::Relaxed);
    let err = pack.read("big.bin").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{err}");
    let largest = LARGEST.load(Ordering::Relaxed);
    // The decoder's own buffers are tens of kilobytes; the index claims 1 GiB.
    assert!(largest < 1 << 20, "reserved {largest} bytes for 16 stored bytes");
}

/// A one-entry deflate pack whose index declares `size` bytes (and a zero CRC).
fn deflate_pack(stored: &[u8], size: u64) -> Vec<u8> {
    let name = b"bomb.bin";
    let mut index = Vec::new();
    index.extend((name.len() as u16).to_le_bytes());
    index.extend(name);
    index.push(1); // deflate
    index.extend(0u64.to_le_bytes());
    index.extend((stored.len() as u64).to_le_bytes());
    index.extend(size.to_le_bytes());
    index.extend(0u32.to_le_bytes());
    let mut pack = b"PONKOPAK".to_vec();
    pack.extend(1u32.to_le_bytes());
    pack.extend(1u32.to_le_bytes());
    pack.extend((index.len() as u64).to_le_bytes());
    pack.extend((stored.len() as u64).to_le_bytes());
    pack.extend(index);
    pack.extend(stored);
    pack
}

#[test]
fn an_entry_inflating_past_the_cap_never_holds_more_than_the_cap() {
    use std::io::Write;
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // 64 MiB of zeros in a few tens of kilobytes.
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..64 {
        encoder.write_all(&zeros).unwrap();
    }
    let stored = encoder.finish().unwrap();
    let cap: u64 = 64 << 10;
    let limits = PackLimits::default().with_max_entry_size(cap);
    // The index tells the truth (64 MiB), lies low (exactly the cap), or lies a little high.
    for declared in [64u64 << 20, cap, cap * 2] {
        let pack = PackSource::from_bytes_with_limits("bomb", deflate_pack(&stored, declared), limits).unwrap();
        LARGEST.store(0, Ordering::Relaxed);
        let err = pack.read("bomb.bin").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{err}");
        let largest = LARGEST.load(Ordering::Relaxed);
        // The cap, plus a doubled buffer and the decoder's own tens of kilobytes: nowhere near 64 MiB.
        assert!(
            largest <= 4 * cap as usize,
            "declared {declared}: one request of {largest} bytes"
        );
    }
}
