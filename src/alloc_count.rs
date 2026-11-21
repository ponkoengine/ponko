//! Counting heap allocations, to find code that allocates every frame.
//!
//! Install [`CountingAllocator`] as the global allocator and the profiler
//! counts allocations per frame and per system (the overlay, benchmark
//! reports and [`Perf::report`](crate::perf::Perf::report) show them). It
//! counts only while profiling is on, so a game that leaves it installed pays
//! one relaxed atomic load per allocation and per free.
//!
//! It also counts the bytes freed, so [`net_alloc_bytes`] tells how much of
//! what was allocated is still held: the number a leak check watches.
//!
//! Counts are for the whole process: allocations on rayon workers or other
//! threads count too, and a system that allocates inside `par_for_each` is
//! charged for it. Tests that assert on them should not run next to other
//! tests that allocate (give them a test file of their own).
//!
//! Each thread also keeps a count of its own ([`thread_alloc_counts`]), and
//! a profiling scope counts the allocations made on its thread while it is
//! open: what a piece of code allocates, without what a thread pool's
//! workers allocate for themselves meanwhile.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

use crate::scope::ACTIVE;

static COUNT: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static FREED: AtomicU64 = AtomicU64::new(0);
static SEEN: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// This thread's allocations while profiling was on. A `const` cell of
    /// plain numbers: no destructor to register and no lazy set-up, so the
    /// allocator can touch it without allocating (or recursing into itself).
    static THREAD: Cell<AllocCounts> = const { Cell::new(AllocCounts { count: 0, bytes: 0 }) };
}

/// Heap allocations made while profiling was on, from [`alloc_counts`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AllocCounts {
    /// Allocations (and reallocations).
    pub count: u64,
    /// Bytes requested by them.
    pub bytes: u64,
}

/// The system allocator, counting what it hands out while profiling is on.
///
/// ```ignore
/// use ponko::perf::CountingAllocator;
///
/// #[global_allocator]
/// static ALLOCATOR: ponko::perf::CountingAllocator = ponko::perf::CountingAllocator;
/// ```
pub struct CountingAllocator;

#[inline]
fn note(bytes: usize) {
    if ACTIVE.load(Relaxed) > 0 {
        // Not `fetch_add` on a flag: one plain store the first time only.
        if !SEEN.load(Relaxed) {
            SEEN.store(true, Relaxed);
        }
        COUNT.fetch_add(1, Relaxed);
        BYTES.fetch_add(bytes as u64, Relaxed);
        // `try_with`: never panic inside the allocator, even while a thread
        // is being torn down.
        let _ = THREAD.try_with(|t| {
            let c = t.get();
            t.set(AllocCounts {
                count: c.count.wrapping_add(1),
                bytes: c.bytes.wrapping_add(bytes as u64),
            });
        });
    }
}

#[inline]
fn note_free(bytes: usize) {
    if ACTIVE.load(Relaxed) > 0 {
        FREED.fetch_add(bytes as u64, Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the same arguments; the
// counting only touches atomics and never allocates.
unsafe impl GlobalAlloc for CountingAllocator {
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
        note_free(layout.size());
        // SAFETY: `ptr` came from `System` with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        note_free(layout.size());
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Counts one allocation of `bytes` made by something that isn't Rust's
/// allocator, such as C++'s `operator new` (see `PONKO_COUNT_CPP_ALLOCATIONS`
/// in `ponko.hpp`). Counts only while profiling is on.
pub fn note_allocation(bytes: usize) {
    note(bytes);
}

/// The allocations counted so far (while profiling was on). Take two readings
/// and subtract to count a stretch of code. All zero if
/// [`CountingAllocator`] isn't installed and nothing calls
/// [`note_allocation`].
///
/// ```
/// use ponko::perf::alloc_counts;
/// let before = ponko::perf::alloc_counts();
/// let after = ponko::perf::alloc_counts();
/// assert!(after.count >= before.count);
/// ```
pub fn alloc_counts() -> AllocCounts {
    AllocCounts {
        count: COUNT.load(Relaxed),
        bytes: BYTES.load(Relaxed),
    }
}

/// The allocations counted so far on the calling thread (while profiling was
/// on): [`alloc_counts`] for one thread. Take two readings on the same thread
/// and subtract to count what the code between them allocated there, without
/// what other threads did meanwhile (a rayon worker that parks for the first
/// time, an asset loader, the audio thread). Work the code hands to other
/// threads isn't in it either: put a [`profile_scope!`](crate::profile_scope)
/// inside that work, and the scope counts its allocations on whichever
/// thread runs it ([`ScopeReport::allocs_per_frame`](crate::perf::ScopeReport::allocs_per_frame)).
///
/// All zero if [`CountingAllocator`] isn't installed and nothing calls
/// [`note_allocation`]. It costs nothing while profiling is off.
///
/// ```
/// let before = ponko::perf::thread_alloc_counts();
/// let after = ponko::perf::thread_alloc_counts();
/// assert_eq!(after.count - before.count, 0, "nothing ran in between");
/// ```
pub fn thread_alloc_counts() -> AllocCounts {
    THREAD.try_with(Cell::get).unwrap_or_default()
}

/// Bytes allocated minus bytes freed, both counted while profiling was on
/// (a reallocation counts as a free of the old size and an allocation of
/// the new one). Take two readings with profiling on and subtract: the
/// difference is what the code between them still holds, so a loop that
/// gives back everything it takes reads (close to) zero however long it
/// runs, and a leak grows with every pass. Memory allocated before
/// profiling was turned on and freed after counts as a negative.
///
/// Like [`alloc_counts`] it covers the whole process, and it is zero
/// unless [`CountingAllocator`] is installed.
///
/// ```
/// let before = ponko::perf::net_alloc_bytes();
/// let after = ponko::perf::net_alloc_bytes();
/// assert_eq!(after - before, 0, "nothing ran in between");
/// ```
pub fn net_alloc_bytes() -> i64 {
    // Read freed first: an allocation and its free landing between the two
    // loads then can't make the figure smaller than it is.
    let freed = FREED.load(Relaxed);
    BYTES.load(Relaxed) as i64 - freed as i64
}

/// `true` once an allocation has been counted: the counting allocator is
/// installed (or C++ reports its own).
pub(crate) fn counting() -> bool {
    SEEN.load(Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_count_only_while_profiling() {
        let before = alloc_counts();
        crate::scope::enable();
        note_allocation(100);
        crate::scope::disable();
        let after = alloc_counts();
        // Other tests may be counting too, so only check the lower bound.
        assert!(after.count > before.count && after.bytes >= before.bytes + 100);
        assert!(counting());
    }

    /// Each method of the `unsafe impl GlobalAlloc` forwards to the system
    /// allocator: memory it hands out is usable, zeroed where asked, keeps
    /// its bytes through a reallocation and goes back once (Miri checks the
    /// last).
    #[test]
    fn the_counting_allocator_forwards_every_call_to_the_system_allocator() {
        let a = CountingAllocator;
        let layout = Layout::from_size_align(64, 16).unwrap();
        crate::scope::enable();
        let before = alloc_counts();
        // SAFETY: a non-zero layout; each block is used within its size and
        // freed once with the layout it has.
        unsafe {
            let p = a.alloc(layout);
            assert!(!p.is_null() && (p as usize).is_multiple_of(16));
            p.write_bytes(7, 64);
            let p = a.realloc(p, layout, 128);
            assert!(!p.is_null());
            assert!(
                std::slice::from_raw_parts(p, 64).iter().all(|&b| b == 7),
                "reallocation keeps the bytes"
            );
            a.dealloc(p, Layout::from_size_align(128, 16).unwrap());
            let z = a.alloc_zeroed(layout);
            assert!(std::slice::from_raw_parts(z, 64).iter().all(|&b| b == 0));
            a.dealloc(z, layout);
        }
        let after = alloc_counts();
        crate::scope::disable();
        assert!(
            after.count >= before.count + 3,
            "alloc, realloc and alloc_zeroed are counted"
        );
        assert!(after.bytes >= before.bytes + 64 + 128 + 64);
    }

    /// What one thread allocates shows in its own count and not in another
    /// thread's, through the allocator itself (the path Miri checks). The
    /// unit tests don't install the allocator globally, so only the calls
    /// made here are counted, and the counts are exact.
    #[test]
    fn each_thread_counts_only_its_own_allocations() {
        let layout = Layout::from_size_align(32, 8).unwrap();
        let alloc_and_free = move || {
            let a = CountingAllocator;
            let before = thread_alloc_counts();
            // SAFETY: a non-zero layout, freed once with the same layout.
            unsafe { a.dealloc(a.alloc(layout), layout) };
            let after = thread_alloc_counts();
            (after.count - before.count, after.bytes - before.bytes)
        };
        crate::scope::enable();
        let mine = thread_alloc_counts();
        let other = std::thread::spawn(alloc_and_free).join().unwrap();
        let untouched = thread_alloc_counts();
        let own = alloc_and_free();
        crate::scope::disable();
        assert_eq!(other, (1, 32), "the other thread counts its allocation");
        assert_eq!(untouched, mine, "and this thread doesn't");
        assert_eq!(own, (1, 32));
    }

    #[test]
    fn frees_count_only_while_profiling() {
        crate::scope::enable();
        let before = FREED.load(Relaxed);
        note_free(64);
        crate::scope::disable();
        assert!(FREED.load(Relaxed) >= before + 64);
    }
}
