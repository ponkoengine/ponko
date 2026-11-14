//! Repeated queries do not allocate after matching-cache warmup. Its own test
//! binary: it replaces the global allocator. Only allocations on the thread
//! between `start_counting` and `counted` are counted, so the test harness's
//! other threads can't fail it.

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

#[derive(Component)]
struct Marker<const N: usize>;
#[derive(Component)]
struct Num(u64);
#[derive(Component)]
struct Extra(#[allow(dead_code)] u32);

fn warmed_matching_and_reconstructed_queries_do_not_allocate() {
    let mut w = World::new();
    for bits in 0..256 {
        let e = w.spawn((Num(bits as u64),));
        macro_rules! m {
            ($n:literal) => {
                if bits & (1 << $n) != 0 {
                    w.insert(e, Marker::<$n>);
                }
            };
        }
        m!(0);
        m!(1);
        m!(2);
        m!(3);
        m!(4);
        m!(5);
        m!(6);
        m!(7);
    }
    let frame = |w: &mut World| {
        let mut q = w
            .query::<&mut Num>()
            .cached()
            .filter::<With<Marker<0>>>()
            .filter::<Without<Marker<1>>>();
        assert_eq!(q.count(), 64);
        q.for_each(|v| v.0 += 1);
        assert_eq!(q.iter().count(), 64);
    };
    frame(&mut w);
    start_counting();
    for _ in 0..10 {
        frame(&mut w);
    }
    assert_eq!(counted(), 0);
}

fn fragmented(layouts: u64) -> World {
    let mut w = World::new();
    for bits in 0..layouts {
        let e = w.spawn((Num(bits),));
        macro_rules! m {
            ($n:literal) => {
                if bits & (1 << $n) != 0 {
                    w.insert(e, Marker::<$n>);
                }
            };
        }
        m!(0);
        m!(1);
        m!(2);
        m!(3);
        m!(4);
        m!(5);
        m!(6);
        m!(7);
    }
    w
}

/// Queries nobody hinted: indexed ones, ones the policy scans (dense, or
/// seen once per frame under different filters), `Or` filters and the chunk
/// path with a runtime filter.
fn automatic_frame(w: &mut World, filter: &CompiledFilter, ids: &[ComponentId]) -> usize {
    let mut rows = 0;
    w.query::<&mut Num>()
        .filter::<With<Marker<0>>>()
        .filter::<Without<Marker<1>>>()
        .for_each(|v| v.0 += 1);
    rows += w.query::<&Num>().count();
    rows += w
        .query::<&Num>()
        .filter::<Or<((With<Marker<2>>, With<Marker<3>>), Without<Marker<4>>)>>()
        .iter()
        .count();
    rows += w
        .query::<(Entity, &Num)>()
        .filter::<(With<Marker<5>>, With<Marker<6>>, Or<(With<Marker<0>>, With<Marker<7>>)>)>()
        .count();
    w.raw()
        .query_chunks(ChunkQuery::new(ids).filter(filter), |chunk| rows += chunk.len());
    w.raw().for_each_chunk(ids, |chunk| rows += chunk.len());
    rows
}

fn automatic_indexing_allocates_nothing_once_warm() {
    let mut w = fragmented(256);
    let ids = [w.register::<Num>(), w.register::<Marker<1>>()];
    let filter = DynFilter::Or(vec![
        DynFilter::With(w.register::<Marker<2>>()),
        DynFilter::Without(w.register::<Marker<3>>()),
    ])
    .compile()
    .unwrap();
    // Two frames: every signature is seen, then decided.
    let rows = automatic_frame(&mut w, &filter, &ids);
    assert_eq!(automatic_frame(&mut w, &filter, &ids), rows);
    start_counting();
    for _ in 0..10 {
        assert_eq!(automatic_frame(&mut w, &filter, &ids), rows);
    }
    assert_eq!(counted(), 0);
    let stats = w.query_cache_stats();
    assert!(
        stats.indexed >= 2 && stats.hits >= 20,
        "the index was in use: {stats:?}"
    );
}

fn a_small_world_remembers_and_allocates_nothing() {
    let mut w = fragmented(16);
    let ids = [w.register::<Num>(), w.register::<Marker<1>>()];
    let filter = DynFilter::With(w.register::<Marker<2>>()).compile().unwrap();
    let rows = automatic_frame(&mut w, &filter, &ids);
    start_counting();
    for _ in 0..10 {
        assert_eq!(automatic_frame(&mut w, &filter, &ids), rows);
        assert_eq!(w.query::<&Num>().filter::<With<Marker<0>>>().cached().count(), 8);
    }
    assert_eq!(counted(), 0);
    assert_eq!(w.query_cache_stats().bytes, 0, "the index was never created");
}

fn a_full_table_and_its_sweeps_allocate_nothing() {
    let mut w = fragmented(256);
    let mut policy = QueryCachePolicy::new();
    policy.max_signatures = 2;
    w.set_query_cache_policy(policy);
    let frame = |w: &mut World| {
        // Four signatures for two places: the extra ones scan, and get in
        // when another goes unused.
        let a = w
            .query::<&Num>()
            .filter::<With<Marker<0>>>()
            .filter::<With<Marker<1>>>()
            .count();
        let b = w
            .query::<&Num>()
            .filter::<With<Marker<2>>>()
            .filter::<With<Marker<3>>>()
            .count();
        let c = w
            .query::<&Num>()
            .filter::<With<Marker<4>>>()
            .filter::<With<Marker<5>>>()
            .count();
        let d = w
            .query::<&Num>()
            .filter::<With<Marker<6>>>()
            .filter::<With<Marker<7>>>()
            .count();
        assert_eq!((a, b, c, d), (64, 64, 64, 64));
    };
    for _ in 0..3 {
        frame(&mut w);
    }
    start_counting();
    for _ in 0..200 {
        frame(&mut w);
    }
    assert_eq!(counted(), 0);
    assert_eq!(w.query_cache_stats().signatures, 2);
}

/// A view only reads the index: hits, scans of signatures it doesn't hold
/// and the notes of what was scanned allocate nothing.
fn views_reading_the_index_allocate_nothing() {
    let mut w = fragmented(256);
    let mut state = ViewState::new().read::<Num>();
    let frame = |w: &mut World, state: &mut ViewState| {
        state.run(w, |mut view| {
            let mut rows = view
                .query::<&Num>()
                .filter::<With<Marker<0>>>()
                .filter::<Without<Marker<1>>>()
                .count();
            rows += view.query::<&Num>().count();
            rows += view.query::<&Num>().filter::<With<Marker<2>>>().cached().iter().count();
            rows
        })
    };
    // Scanned and noted, counted, indexed.
    let rows = frame(&mut w, &mut state);
    for _ in 0..3 {
        assert_eq!(frame(&mut w, &mut state), rows);
    }
    start_counting();
    for _ in 0..10 {
        assert_eq!(frame(&mut w, &mut state), rows);
    }
    assert_eq!(counted(), 0);
    assert_eq!(w.query_cache_stats().indexed, 2);

    // A new archetype: the next view scans and notes it, and the index
    // catches up when the view after that is prepared. The note reuses its
    // storage.
    w.spawn((Num(7), Marker::<0>, Marker::<2>, Extra(1)));
    start_counting();
    assert_eq!(frame(&mut w, &mut state), rows + 3);
    assert_eq!(counted(), 0);
}

/// A world with a table of two signatures, both in use: one only by a
/// view, one by exclusive queries. Returns it warm, with both indexed.
fn full_table_with_a_view_signature() -> (World, ViewState) {
    let mut w = fragmented(256);
    let mut policy = QueryCachePolicy::new();
    policy.max_signatures = 2;
    w.set_query_cache_policy(policy);
    let mut state = ViewState::new().read::<Num>();
    for _ in 0..3 {
        let seen = state.run(&mut w, |mut view| {
            view.query::<&Num>()
                .filter::<With<Marker<0>>>()
                .filter::<With<Marker<1>>>()
                .count()
        });
        assert_eq!(seen, 64);
        assert_eq!(
            w.query::<&Num>()
                .filter::<With<Marker<2>>>()
                .filter::<With<Marker<3>>>()
                .count(),
            64
        );
    }
    let warm = w.query_cache_stats();
    assert_eq!((warm.signatures, warm.indexed, warm.evictions), (2, 2, 0));
    (w, state)
}

/// A full table sweeps at the end of every frame (32 refusals of a third
/// signature), so the view's use of its signature is the first thing after
/// each sweep. It counts as a use: the signature is never dropped and
/// rebuilt, and nothing allocates.
fn a_signature_only_views_use_survives_sweeps_without_allocating() {
    let (mut w, mut state) = full_table_with_a_view_signature();
    let frame = |w: &mut World, state: &mut ViewState| {
        let seen = state.run(w, |mut view| {
            view.query::<&Num>()
                .filter::<With<Marker<0>>>()
                .filter::<With<Marker<1>>>()
                .count()
        });
        assert_eq!(seen, 64);
        assert_eq!(
            w.query::<&Num>()
                .filter::<With<Marker<2>>>()
                .filter::<With<Marker<3>>>()
                .count(),
            64
        );
        for _ in 0..32 {
            assert_eq!(
                w.query::<&Num>()
                    .filter::<With<Marker<4>>>()
                    .filter::<With<Marker<5>>>()
                    .count(),
                64
            );
        }
    };
    frame(&mut w, &mut state);
    start_counting();
    for _ in 0..200 {
        frame(&mut w, &mut state);
    }
    assert_eq!(counted(), 0);
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.evictions), (2, 2, 0));
}

/// View signatures a full table has no room for scan every frame without
/// being noted (and refused) every frame. If they were, their refusals
/// would sweep the table every few frames and drop a signature that is
/// used only now and then, to be rebuilt at its next use.
fn views_a_full_table_refuses_do_not_push_out_a_rarely_used_signature() {
    let (mut w, mut state) = full_table_with_a_view_signature();
    let frame = |w: &mut World, state: &mut ViewState, n: usize| {
        let seen = state.run(w, |mut view| {
            (
                view.query::<&Num>()
                    .filter::<With<Marker<0>>>()
                    .filter::<With<Marker<1>>>()
                    .count(),
                view.query::<&Num>()
                    .filter::<With<Marker<6>>>()
                    .filter::<With<Marker<7>>>()
                    .count(),
                view.query::<&Num>()
                    .filter::<With<Marker<6>>>()
                    .filter::<Without<Marker<7>>>()
                    .cached()
                    .count(),
            )
        });
        assert_eq!(seen, (64, 64, 64));
        // The table's other signature: an exclusive query that runs once
        // in 40 frames.
        if n.is_multiple_of(40) {
            assert_eq!(
                w.query::<&Num>()
                    .filter::<With<Marker<2>>>()
                    .filter::<With<Marker<3>>>()
                    .count(),
                64
            );
        }
    };
    // The first frames note the new signatures and have them refused.
    for n in 0..3 {
        frame(&mut w, &mut state, n);
    }
    start_counting();
    let hits = w.query_cache_stats().hits;
    // Long enough for the views to ask again several times.
    for n in 0..400 {
        frame(&mut w, &mut state, n);
    }
    assert_eq!(counted(), 0);
    let stats = w.query_cache_stats();
    assert_eq!((stats.signatures, stats.indexed, stats.evictions), (2, 2, 0));
    assert_eq!(
        stats.hits - hits,
        10,
        "the rare query was answered from the index every time"
    );
}

/// Not under Miri: these are 256-archetype worlds run for hundreds of
/// frames, and Miri runs the same paths in the small worlds of
/// `query_cache.rs`, `query_cache_views.rs` and the unit tests of the index.
#[test]
#[cfg_attr(miri, ignore = "big worlds; allocation counts need no interpreter")]
fn the_index_allocates_nothing_in_steady_state() {
    warmed_matching_and_reconstructed_queries_do_not_allocate();
    automatic_indexing_allocates_nothing_once_warm();
    a_small_world_remembers_and_allocates_nothing();
    a_full_table_and_its_sweeps_allocate_nothing();
    views_reading_the_index_allocate_nothing();
    a_signature_only_views_use_survives_sweeps_without_allocating();
    views_a_full_table_refuses_do_not_push_out_a_rarely_used_signature();
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
