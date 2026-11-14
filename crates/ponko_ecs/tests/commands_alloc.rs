//! Recording and applying commands into a reused `Commands` doesn't allocate
//! once its buffers have grown. Its own test binary: it replaces the global
//! allocator. Only allocations on the thread between `start_counting` and
//! `counted` are counted, so the test harness's other threads (its main
//! thread files the running test after spawning it) can't fail it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use ponko_ecs::*;

struct Counting;

thread_local! {
    /// Allocations on this thread since `start_counting`; `None` when not counting.
    static ALLOCATIONS: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Counts an allocation if this thread is counting.
fn note() {
    // `try_with`: the allocator may run while this thread's locals are torn down.
    let _ = ALLOCATIONS.try_with(|n| n.set(n.get().map(|n| n + 1)));
}

// SAFETY: forwards to the system allocator, only counting calls.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: the caller's contract, forwarded.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Starts counting the heap allocations (and reallocations) of this thread.
fn start_counting() {
    ALLOCATIONS.with(|n| n.set(Some(0)));
}

/// Stops counting on this thread and returns the count since `start_counting`.
fn counted() -> usize {
    ALLOCATIONS
        .with(Cell::take)
        .expect("start_counting was called on this thread")
}

/// A sized component, so each command carries bytes, not just a type.
#[derive(Component)]
struct Marked(#[allow(dead_code)] u64);
#[derive(Component)]
struct Index(#[allow(dead_code)] u64);

#[test]
fn reused_commands_record_and_apply_without_allocating() {
    let mut world = World::new();
    let entities: Vec<_> = (0..256u64).map(|i| world.spawn((Index(i),))).collect();
    let mut commands = Commands::new();
    let frame = |world: &mut World, commands: &mut Commands| {
        for &e in &entities {
            commands.insert(e, Marked(7));
        }
        let total = 3u64;
        commands.add(move |w| *w.resource_mut::<u64>() += total);
        world.apply(commands);
        for &e in &entities {
            commands.remove::<Marked>(e);
        }
        world.apply(commands);
    };
    world.insert_resource(0u64);
    // The first frames grow the command buffers and the world's tables.
    for _ in 0..3 {
        frame(&mut world, &mut commands);
    }
    start_counting();
    for _ in 0..10 {
        frame(&mut world, &mut commands);
    }
    assert_eq!(counted(), 0);
    assert_eq!(*world.resource::<u64>(), 13 * 3);
}

/// The negative control: an allocation made on the measuring thread is
/// counted, so a zero above means nothing allocated.
#[test]
fn an_allocation_on_the_measuring_thread_is_counted() {
    start_counting();
    let boxed = std::hint::black_box(Box::new(7u64));
    assert_eq!(counted(), 1);
    let mut grown: Vec<u64> = Vec::with_capacity(1);
    start_counting();
    grown.extend(std::hint::black_box([1, 2, 3]));
    assert_eq!(counted(), 1, "growing a buffer counts too");
    start_counting();
    std::hint::black_box(*boxed + grown[2]);
    assert_eq!(counted(), 0);
}
