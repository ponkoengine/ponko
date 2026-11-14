//! A global allocator that counts heap allocations, so every benchmark can
//! report how many it makes per timed run.
//!
//! It always counts (three relaxed atomic adds per allocation, for the
//! count, the bytes and the bytes still live, and one per free, on top of
//! the system allocator) and counts
//! the whole process: allocations on rayon or
//! bevy worker threads during a timed run are charged to that run.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed};

static COUNT: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
/// Bytes allocated and not yet freed.
static LIVE: AtomicI64 = AtomicI64::new(0);

/// The system allocator, counting allocations and reallocations.
pub struct Counting;

// SAFETY: every method forwards to `System` with the same arguments; the
// counting only touches atomics and never allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: the caller's contract is `System`'s.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        // SAFETY: `ptr` came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[inline]
fn note(bytes: usize) {
    COUNT.fetch_add(1, Relaxed);
    BYTES.fetch_add(bytes as u64, Relaxed);
    LIVE.fetch_add(bytes as i64, Relaxed);
}

/// Heap bytes the whole process has allocated and not freed yet.
pub fn live_bytes() -> i64 {
    LIVE.load(Relaxed)
}

/// Allocations (with reallocations) and bytes requested so far, in the
/// whole process. Take two readings and subtract.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub count: u64,
    pub bytes: u64,
}

impl Counts {
    pub fn now() -> Self {
        Counts {
            count: COUNT.load(Relaxed),
            bytes: BYTES.load(Relaxed),
        }
    }

    /// What was counted between `earlier` and `self`.
    pub fn since(self, earlier: Counts) -> Counts {
        Counts {
            count: self.count - earlier.count,
            bytes: self.bytes - earlier.bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_are_counted() {
        let before = Counts::now();
        let v: Vec<u8> = black_box_vec(4096);
        let counted = Counts::now().since(before);
        drop(v);
        // Other tests allocate at the same time, so only a lower bound holds.
        assert!(counted.count >= 1 && counted.bytes >= 4096, "{counted:?}");
    }

    fn black_box_vec(n: usize) -> Vec<u8> {
        std::hint::black_box(Vec::with_capacity(n))
    }
}
