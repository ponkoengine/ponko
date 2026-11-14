//! Reading a damaged asset pack doesn't reserve the memory its index
//! claims. This file has its own process (and a single test), so its
//! allocator sees only this test's allocations.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use ponko::prelude::*;

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
