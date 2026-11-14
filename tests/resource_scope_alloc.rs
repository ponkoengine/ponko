//! The steady-state allocations of `resource_scope` and a query.
//!
//! Its own binary: it replaces the global allocator. The system counts only
//! its own thread's allocations, so other threads can't fail it.
use std::alloc::{GlobalAlloc, Layout};
use std::cell::Cell;

#[allow(unused_imports)]
use ponko::perf::CountingAllocator;
use ponko::prelude::*;

/// The engine's counting allocator, also counting each thread's own
/// allocations: profiling runs the systems on the calling thread, so that
/// count is exact, while libtest's main thread (which files the running test
/// after spawning it) can allocate at any time.
struct ThreadCountingAllocator;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

/// Allocations (and reallocations) made on this thread so far.
fn thread_allocs() -> u64 {
    ALLOCS.with(Cell::get)
}

fn note_allocation() {
    // Const-initialized: no allocation, no destructor. A thread being torn
    // down may still allocate after its locals are gone.
    let _ = ALLOCS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: every method forwards unchanged to the engine's counting allocator;
// the extra accounting only updates an allocation-free thread-local `Cell`.
unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller's contract, forwarded.
        unsafe { CountingAllocator.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller's contract, forwarded.
        unsafe { CountingAllocator.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        note_allocation();
        // SAFETY: the caller's contract, forwarded.
        unsafe { CountingAllocator.realloc(ptr, layout, size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, forwarded.
        unsafe { CountingAllocator.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

#[derive(Component)]
struct Count(u32);

fn scoped_tick(ctx: &mut Ctx) {
    let before = thread_allocs();
    ctx.world.resource_scope::<Vec<u32>, _>(|world, totals| {
        world.query::<&mut Count>().for_each(|n| {
            n.0 += 1;
            totals[0] += n.0;
        });
    });
    assert_eq!(thread_allocs(), before, "resource scope and query must not allocate");
}

#[test]
fn scoped_resource_and_query_allocate_nothing_in_steady_state() {
    let (_, report) = Game::new("scope", 1, 1)
        .sim_system(scoped_tick)
        .profile_headless(10, 100, |ctx| {
            ctx.world.insert_resource(vec![0u32]);
            ctx.world.spawn_batch((0..100).map(|n| (Count(n as u32),)));
        });
    // The report counts every thread, so its figure for the system is not
    // asserted to be zero here: the system's own check above is exact.
    assert!(report.allocs.is_some(), "allocation counting must be enabled");
    assert!(report.system("scoped_tick").is_some());
}

/// The negative control: an allocation on the counting thread is counted.
#[test]
fn an_allocation_on_this_thread_is_counted() {
    let before = thread_allocs();
    std::hint::black_box(Box::new(7u64));
    assert_eq!(thread_allocs() - before, 1);
}
