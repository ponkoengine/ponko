//! Borrowing several resources, carrying resources into a query, looking up
//! other entities and `get_many_mut` do not allocate. Its own test binary: it
//! replaces the global allocator. Only allocations on the thread between
//! `start_counting` and `counted` are counted, so the test harness's other
//! threads can't fail it.

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

struct Speed(u64);
struct Score(u64);
struct Total(u64);
#[derive(Component)]
struct Target(Entity);
#[derive(Component)]
struct Sum(u64);
#[derive(Component)]
struct Count(u32);

#[test]
fn the_combined_borrows_do_not_allocate() {
    let mut w = World::new();
    w.insert_resource(Speed(2));
    w.insert_resource(Score(0));
    w.insert_resource(Total(0));
    let hub = w.spawn((Sum(0),));
    let other = w.spawn((Sum(1),));
    w.spawn_batch((0..1000).map(|_| (Target(hub), Count(1))));

    let frame = |w: &mut World| {
        let (speed, score, total) = w.resources_mut::<(Speed, Score, Total)>();
        score.0 += speed.0;
        total.0 += score.0;

        w.query::<&mut Count>()
            .with_resources::<(&Speed, &mut Score)>()
            .for_each(|n, (speed, score)| {
                n.0 = n.0.wrapping_add(speed.0 as u32);
                score.0 += 1;
            });

        w.query::<(&Target, &Count)>()
            .with_lookup::<&mut Sum>()
            .for_each(|(target, n), others| {
                if let Some(sum) = others.get_mut(target.0) {
                    sum.0 += u64::from(n.0);
                }
            });

        if let Some([a, b]) = w.get_many_mut::<Sum, 2>([hub, other]) {
            std::mem::swap(a, b);
        }
    };
    // The first frame registers what it touches.
    frame(&mut w);
    start_counting();
    for _ in 0..100 {
        frame(&mut w);
    }
    assert_eq!(counted(), 0);
    assert_eq!(w.resource::<Score>().0, 101 * 2 + 101 * 1000);
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
