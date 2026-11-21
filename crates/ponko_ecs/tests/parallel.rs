//! Multi-core queries (`par_for_each`).
#![cfg(feature = "parallel")]

mod common;

use common::*;
use ponko_ecs::*;

/// Extra marker for a third matching archetype.
#[derive(Component)]
struct Vel2;

#[test]
fn par_for_each_visits_every_entity_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    // Sizes relative to the largest parallel chunk (8192 rows, 64 under Miri):
    // several archetypes bigger than one chunk (with a partial last chunk),
    // one smaller than a chunk, a filtered-out one, and a non-matching one.
    let chunk = if cfg!(miri) { 64 } else { 8192 };
    let (big, mid, small, other) = (6 * chunk + 7, 2 * chunk + 3, chunk / 2, chunk / 8);
    let mut w = World::new();
    w.spawn_batch((0..big).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0))));
    w.spawn_batch((0..mid).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0), Tag)));
    w.spawn_batch((0..small).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0), Tag, Vel2)));
    w.spawn_batch((0..100).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0), Frozen)));
    w.spawn_batch((0..other).map(|i| (Pos(i as f32, 0.0),)));
    let matching = big + mid + small;
    let visits = AtomicUsize::new(0);
    w.query::<(&mut Pos, &Vel)>()
        .filter::<Without<Frozen>>()
        .par_for_each(|(p, v)| {
            p.1 += v.0;
            visits.fetch_add(1, Ordering::Relaxed);
        });
    assert_eq!(visits.into_inner(), matching);
    assert_eq!(w.query::<&Pos>().iter().filter(|p| p.1 == 1.0).count(), matching);
    assert_eq!(w.query::<&Pos>().iter().filter(|p| p.1 == 0.0).count(), 100 + other);
}

#[test]
fn par_for_each_indexed_matches_the_sequential_order() {
    use std::sync::atomic::{AtomicU32, Ordering};
    // Same mix of archetype sizes as above, so indices have to carry over
    // between archetypes and between chunks.
    let chunk = if cfg!(miri) { 64 } else { 8192 };
    let mut w = World::new();
    w.spawn_batch((0..3 * chunk + 5).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0))));
    w.spawn_batch((0..chunk / 2).map(|i| (Pos(i as f32 + 1e5, 0.0), Vel(1.0, 0.0), Tag)));
    w.spawn_batch((0..100).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0), Frozen)));
    w.spawn_batch((0..2 * chunk + 1).map(|i| (Pos(i as f32 + 2e5, 0.0), Vel(1.0, 0.0), Vel2)));
    let expected: Vec<u32> = w
        .query::<(&Pos, &Vel)>()
        .filter::<Without<Frozen>>()
        .iter()
        .map(|(p, _)| p.0 as u32)
        .collect();
    let out: Vec<AtomicU32> = (0..expected.len()).map(|_| AtomicU32::new(u32::MAX)).collect();
    w.query::<(&Pos, &Vel)>()
        .filter::<Without<Frozen>>()
        .par_for_each_indexed(|i, (p, _)| {
            let previous = out[i].swap(p.0 as u32, Ordering::Relaxed);
            assert_eq!(previous, u32::MAX, "index {i} visited twice");
        });
    let got: Vec<u32> = out.into_iter().map(AtomicU32::into_inner).collect();
    assert_eq!(got, expected);
}

#[test]
fn par_collect_into_matches_the_sequential_order_across_archetypes() {
    let mut world = World::new();
    world.spawn_batch((0..3000).map(|i| (Pos(i as f32, 0.0),)));
    world.spawn_batch((3000..5000).map(|i| (Pos(i as f32, 0.0), Tag)));
    let mut expected = Vec::new();
    world.query::<&Pos>().for_each(|p| expected.push(p.0));
    // Strings own memory: written once each, dropped once each.
    let mut out = vec!["stale".to_string(); 10];
    world.query::<&Pos>().par_collect_into(&mut out, |p| p.0.to_string());
    let want: Vec<String> = expected.iter().map(f32::to_string).collect();
    assert_eq!(out, want);
    // Reused: replaced, not appended to; filters apply.
    world
        .query::<&Pos>()
        .filter::<With<Tag>>()
        .par_collect_into(&mut out, |p| p.0.to_string());
    assert_eq!(out.len(), 2000);
    assert_eq!(out[0], "3000");
}

#[test]
fn par_collect_into_an_empty_query_empties_the_vec() {
    let mut world = World::new();
    let mut out = vec![1, 2, 3];
    world.query::<&Pos>().par_collect_into(&mut out, |p| p.0 as i32);
    assert!(out.is_empty());
}

fn assert_panicking_collect_drops_each_written_value(rows: usize, panic_at: usize) {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Component)]
    struct Id(usize);
    // Send but not Sync: cleanup must require only the API's R: Send bound.
    struct Counted {
        id: usize,
        drops: Arc<[AtomicUsize]>,
        _not_sync: Cell<()>,
    }
    impl Drop for Counted {
        fn drop(&mut self) {
            self.drops[self.id].fetch_add(1, Ordering::Relaxed);
        }
    }

    let mut world = World::new();
    // Finish one archetype before starting the possibly parallel one.
    world.spawn_batch((0..3).map(|i| (Id(i), Tag)));
    world.spawn_batch((3..rows).map(|i| (Id(i),)));
    let created: Vec<_> = (0..rows).map(|_| AtomicBool::new(false)).collect();
    let drops: Arc<[AtomicUsize]> = (0..=rows).map(|_| AtomicUsize::new(0)).collect();
    let make = |id| Counted {
        id,
        drops: drops.clone(),
        _not_sync: Cell::new(()),
    };
    let mut out = Vec::with_capacity(rows);
    out.push(make(rows));
    let capacity = out.capacity();
    let buffer = out.as_ptr();
    let result = catch_unwind(AssertUnwindSafe(|| {
        world.query::<&Id>().par_collect_into(&mut out, |id| {
            assert_ne!(id.0, panic_at, "mapper panic");
            created[id.0].store(true, Ordering::Relaxed);
            make(id.0)
        });
    }));
    assert!(result.is_err());
    assert!(out.is_empty());
    assert_eq!(drops[rows].load(Ordering::Relaxed), 1, "old output is dropped once");
    for i in 0..rows {
        assert_eq!(
            drops[i].load(Ordering::Relaxed),
            usize::from(created[i].load(Ordering::Relaxed)),
            "written result {i} is dropped exactly once"
        );
    }
    assert!(created[..3].iter().all(|c| c.load(Ordering::Relaxed)));

    // The same allocation remains usable after unwinding, including its length.
    world.query::<&Id>().par_collect_into(&mut out, |id| make(id.0));
    assert_eq!(out.len(), rows);
    assert_eq!(out.capacity(), capacity);
    assert_eq!(out.as_ptr(), buffer);
    assert_eq!(
        out.iter().map(|v| v.id).collect::<Vec<_>>(),
        (0..rows).collect::<Vec<_>>()
    );
    drop(out);
    for i in 0..rows {
        assert_eq!(
            drops[i].load(Ordering::Relaxed),
            1 + usize::from(created[i].load(Ordering::Relaxed))
        );
    }
}

#[test]
fn par_collect_into_drops_written_values_on_small_mapper_panic_and_reuses_the_vec() {
    assert_panicking_collect_drops_each_written_value(7, 5);
}

#[test]
fn par_collect_into_drops_each_written_value_across_chunks_and_archetypes_on_mapper_panic() {
    let chunk = if cfg!(miri) { 64 } else { 512 };
    rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap()
        .install(|| assert_panicking_collect_drops_each_written_value(3 + 2 * chunk + 7, 3 + chunk + 1));
}

#[test]
fn par_collect_into_drops_each_written_zero_sized_value_on_mapper_panic() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static DROPS: AtomicUsize = AtomicUsize::new(0);
    struct Counted;
    impl Drop for Counted {
        fn drop(&mut self) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }
    let chunk = if cfg!(miri) { 64 } else { 512 };
    let mut world = World::new();
    world.spawn_batch((0..2 * chunk + 7).map(|i| (Pos(i as f32, 0.0),)));
    let created = AtomicUsize::new(0);
    let mut out = Vec::new();
    let result = catch_unwind(AssertUnwindSafe(|| {
        world.query::<&Pos>().par_collect_into(&mut out, |p| {
            assert_ne!(p.0 as usize, chunk + 1, "mapper panic");
            created.fetch_add(1, Ordering::Relaxed);
            Counted
        });
    }));
    assert!(result.is_err());
    assert!(out.is_empty());
    assert_eq!(DROPS.load(Ordering::Relaxed), created.load(Ordering::Relaxed));
    drop(out);
    assert_eq!(DROPS.load(Ordering::Relaxed), created.load(Ordering::Relaxed));
}

#[test]
fn cached_parallel_chunks_follow_growth_and_preserve_order() {
    #[derive(Component)]
    struct Marker<const N: usize>;
    #[derive(Component, Clone, Copy, Debug, PartialEq)]
    struct Num(u64);
    let mut w = World::new();
    for bits in 0..128 {
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
    }
    assert_eq!(w.query::<&Num>().cached().filter::<With<Marker<0>>>().count(), 64);
    let rows = if cfg!(miri) { 129 } else { 1025 };
    // A new layout after warming, with enough rows to require multiple tasks.
    w.spawn_batch((0..rows).map(|i| (Num(i as u64), Marker::<0>, Marker::<9>)));
    w.query::<&mut Num>()
        .cached()
        .filter::<With<Marker<0>>>()
        .par_for_each(|v| v.0 += 1);
    let expected: Vec<_> = w.query::<&Num>().filter::<With<Marker<0>>>().iter().copied().collect();
    let mut actual = Vec::new();
    w.query::<&Num>()
        .cached()
        .filter::<With<Marker<0>>>()
        .par_collect_into(&mut actual, |v| *v);
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 64 + rows);
}

// ---- the work plan across archetypes of every size ---------------------------

/// Rows per parallel chunk at least (`PAR_MIN_CHUNK`). A query of more
/// than one chunk times its first rows on the calling thread and runs the
/// rest in parallel unless the whole query would take under 50 µs.
const MIN_CHUNK: usize = if cfg!(miri) { 64 } else { 512 };
/// Rows timed first (`PAR_PROBE_FIRST`): slow rows go parallel after these.
const PROBE_FIRST: usize = 64;

/// Asserts that a query over `n` rows reaches worker threads: more than four
/// chunks, so at least three parallel chunks remain after the timed first
/// one (chunks are `MIN_CHUNK` rows on a 4-thread pool at these sizes).
#[track_caller]
fn assert_reaches_workers(n: usize) {
    assert!(n > 4 * MIN_CHUNK, "{n} rows are too few to reach the workers");
}

/// Per-entity work that makes any query over more than one chunk of rows
/// slow enough to run in parallel: at least 200 ns per entity, so the timed
/// first chunk alone takes over 100 µs. Not needed under Miri, which always
/// runs the parallel path.
///
/// The spin is a lower bound, so it holds on a paused machine: a thread
/// that loses its core only spins longer, which keeps the query on the
/// parallel path. The tests that use it assert the order and the rows
/// visited, which are the same on either path, never a time.
fn slow(slow: bool) {
    if slow && !cfg!(miri) {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_nanos(200) {
            std::hint::spin_loop();
        }
    }
}

/// A unique value per entity, in spawn order.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
struct Id(u64);
#[derive(Component)]
struct M<const N: usize>;

/// Adds `rows(layout)` entities to each of `layouts` archetypes (one per set
/// of marker bits, up to 2^9). Moving entities through the layouts of the
/// lower bit sets creates those archetypes too, so a layout with no rows of
/// its own is left as an empty archetype.
fn spawn_layouts(w: &mut World, layouts: usize, rows: impl Fn(usize) -> usize) {
    assert!(layouts <= 1 << 9);
    let mut next = w.query::<&Id>().count() as u64;
    for bits in 0..layouts {
        for _ in 0..rows(bits) {
            let e = w.spawn((Id(next),));
            next += 1;
            macro_rules! marker {
                ($($n:literal)*) => {$(
                    if bits & (1 << $n) != 0 {
                        w.insert(e, M::<$n>);
                    }
                )*};
            }
            marker!(0 1 2 3 4 5 6 7 8);
        }
    }
}

/// Runs `f` on a pool of four threads, so every parallel path really
/// splits, whatever the machine's core count.
fn on_four_threads<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap()
        .install(f)
}

/// Every parallel way of running `query` agrees with its sequential order:
/// `par_for_each_indexed` visits each position once with the entity `iter`
/// yields there, `par_for_each` visits each entity once, and
/// `par_collect_into` (plain and owning results) fills that order. Checked
/// with cheap per-entity work and with [`slow`] work, which always reaches
/// the parallel path.
fn assert_parallel_order(w: &mut World, query: fn(&mut World) -> Query<'_, &Id>) -> usize {
    // Under Miri both kinds of work take the same path: check once.
    if !cfg!(miri) {
        assert_parallel_order_with(w, query, false);
    }
    assert_parallel_order_with(w, query, true)
}

fn assert_parallel_order_with(w: &mut World, query: fn(&mut World) -> Query<'_, &Id>, heavy: bool) -> usize {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    let expected: Vec<u64> = query(w).iter().map(|id| id.0).collect();
    on_four_threads(|| {
        let slots: Vec<AtomicU64> = expected.iter().map(|_| AtomicU64::new(u64::MAX)).collect();
        query(w).par_for_each_indexed(|i, id| {
            slow(heavy);
            let before = slots[i].swap(id.0, Ordering::Relaxed);
            assert_eq!(before, u64::MAX, "position {i} visited twice");
        });
        let got: Vec<u64> = slots.into_iter().map(AtomicU64::into_inner).collect();
        assert_eq!(got, expected, "par_for_each_indexed positions");

        let visits = AtomicUsize::new(0);
        query(w).par_for_each(|_| {
            slow(heavy);
            visits.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(visits.into_inner(), expected.len(), "par_for_each visits");

        let mut plain = vec![7, 8, 9];
        query(w).par_collect_into(&mut plain, |id| {
            slow(heavy);
            id.0
        });
        assert_eq!(plain, expected, "par_collect_into, plain results");

        let mut owned = vec!["stale".to_string()];
        query(w).par_collect_into(&mut owned, |id| {
            slow(heavy);
            id.0.to_string()
        });
        let want: Vec<String> = expected.iter().map(u64::to_string).collect();
        assert_eq!(owned, want, "par_collect_into, owning results");
    });
    expected.len()
}

fn all_ids(w: &mut World) -> Query<'_, &Id> {
    w.query::<&Id>()
}

#[test]
fn many_tiny_archetypes_keep_the_sequential_order() {
    // Several chunks of tiny archetypes, batched into chunks.
    let layouts = if cfg!(miri) { 48 } else { 512 };
    let mut w = World::new();
    spawn_layouts(
        &mut w,
        layouts,
        |bits| if cfg!(miri) { 6 + bits % 3 } else { 8 + bits % 8 },
    );
    let n = assert_parallel_order(&mut w, all_ids);
    assert_reaches_workers(n);
}

#[test]
fn a_few_tiny_archetypes_stay_on_the_calling_thread_and_keep_the_order() {
    let mut w = World::new();
    spawn_layouts(&mut w, 16, |bits| 1 + bits % 5);
    let n = assert_parallel_order(&mut w, all_ids);
    // At most one chunk: no thread is woken.
    assert!(n <= MIN_CHUNK);
    // The serial path runs `f` on the calling thread only.
    let caller = std::thread::current().id();
    on_four_threads(|| {
        let caller = std::thread::current().id();
        w.query::<&Id>()
            .par_for_each(|_| assert_eq!(std::thread::current().id(), caller));
    });
    assert_eq!(std::thread::current().id(), caller);
}

#[test]
fn a_few_huge_archetypes_keep_the_sequential_order() {
    let mut w = World::new();
    // Partial last chunks, and archetype boundaries inside chunks.
    spawn_layouts(&mut w, 3, |bits| (3 + bits) * MIN_CHUNK + 5 + bits);
    let n = assert_parallel_order(&mut w, all_ids);
    assert!(n > 9 * MIN_CHUNK);
}

#[test]
fn mixed_archetype_sizes_keep_the_sequential_order() {
    let mut w = World::new();
    // Huge, tiny and mid-sized archetypes interleaved, and a huge one last.
    let layouts = if cfg!(miri) { 24 } else { 256 };
    spawn_layouts(&mut w, layouts, |bits| match bits % 8 {
        0 if bits > 0 => 2 * MIN_CHUNK + 3,
        3 => MIN_CHUNK / 2 + 1,
        _ => 1 + bits % 2,
    });
    w.spawn_batch((0..3 * MIN_CHUNK + 1).map(|i| {
        (
            Id(1_000_000 + i as u64),
            M::<0>,
            M::<1>,
            M::<2>,
            M::<3>,
            M::<4>,
            M::<5>,
            M::<6>,
            M::<7>,
            M::<8>,
            Tag,
        )
    }));
    assert_parallel_order(&mut w, all_ids);
}

#[test]
fn empty_archetypes_take_no_positions() {
    let mut w = World::new();
    // Every odd layout has no rows of its own: an empty archetype between
    // filled ones.
    let layouts = if cfg!(miri) { 64 } else { 512 };
    spawn_layouts(&mut w, layouts, |bits| if bits % 2 == 1 { 0 } else { 10 + bits % 4 });
    // Empty some filled archetypes too, by despawning their rows.
    let gone: Vec<Entity> = w
        .query::<Entity>()
        .filter::<With<M<2>>>()
        .filter::<Without<M<1>>>()
        .iter()
        .collect();
    for e in gone {
        w.despawn(e);
    }
    assert!(
        w.query::<&Id>()
            .filter::<With<M<2>>>()
            .filter::<Without<M<1>>>()
            .count()
            == 0
    );
    let n = assert_parallel_order(&mut w, all_ids);
    assert_reaches_workers(n);
    // A query that matches only empty archetypes runs nothing.
    on_four_threads(|| {
        w.query::<&Id>()
            .filter::<With<M<2>>>()
            .filter::<Without<M<1>>>()
            .par_for_each(|_| panic!("no rows"));
        let mut out = vec![1];
        w.query::<&Id>()
            .filter::<With<M<2>>>()
            .filter::<Without<M<1>>>()
            .par_collect_into(&mut out, |id| id.0);
        assert!(out.is_empty());
    });
}

#[test]
fn filters_apply_to_the_plan_across_many_tiny_archetypes() {
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| 17 + bits % 4);
    fn with_without(w: &mut World) -> Query<'_, &Id> {
        w.query::<&Id>().filter::<With<M<0>>>().filter::<Without<M<3>>>()
    }
    fn either(w: &mut World) -> Query<'_, &Id> {
        w.query::<&Id>()
            .filter::<(Or<(With<M<1>>, With<M<4>>)>, Without<M<2>>)>()
    }
    let a = assert_parallel_order(&mut w, with_without);
    let b = assert_parallel_order(&mut w, either);
    assert_reaches_workers(a);
    assert_reaches_workers(b);
    assert!(a < w.query::<&Id>().count());
}

#[test]
fn queried_components_narrow_the_plan_and_optional_ones_do_not() {
    use std::sync::atomic::{AtomicU64, Ordering};
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| 9 + bits % 3);
    let expected: Vec<(u64, bool)> = w
        .query::<(&Id, Option<&M<1>>, &M<5>)>()
        .iter()
        .map(|(id, m, _)| (id.0, m.is_some()))
        .collect();
    assert!(expected.iter().any(|e| e.1) && expected.iter().any(|e| !e.1));
    assert_reaches_workers(expected.len());
    on_four_threads(|| {
        let slots: Vec<AtomicU64> = expected.iter().map(|_| AtomicU64::new(u64::MAX)).collect();
        w.query::<(&Id, Option<&M<1>>, &M<5>)>()
            .par_for_each_indexed(|i, (id, m, _)| {
                slow(true);
                assert_eq!(
                    slots[i].swap(id.0 * 2 + u64::from(m.is_some()), Ordering::Relaxed),
                    u64::MAX
                );
            });
        let got: Vec<(u64, bool)> = slots
            .into_iter()
            .map(|s| s.into_inner())
            .map(|v| (v / 2, v % 2 == 1))
            .collect();
        assert_eq!(got, expected);
    });
}

#[test]
fn cached_queries_plan_many_tiny_archetypes_in_order() {
    // Enough layouts for the matching cache to engage (128 and up).
    let layouts = if cfg!(miri) { 160 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| if cfg!(miri) { 3 } else { 12 + bits % 16 });
    fn cached(w: &mut World) -> Query<'_, &Id> {
        w.query::<&Id>().cached().filter::<Without<M<6>>>()
    }
    fn scanned(w: &mut World) -> Query<'_, &Id> {
        w.query::<&Id>().filter::<Without<M<6>>>()
    }
    let n = assert_parallel_order(&mut w, cached);
    assert_reaches_workers(n);
    let want: Vec<u64> = scanned(&mut w).iter().map(|id| id.0).collect();
    let mut got = Vec::new();
    cached(&mut w).par_collect_into(&mut got, |id| id.0);
    assert_eq!(got, want);
    // New tiny layouts after the cache warmed join the plan in order.
    spawn_layouts(&mut w, layouts, |_| 1);
    let n2 = assert_parallel_order(&mut w, cached);
    assert_eq!(n2, n + (0..layouts).filter(|bits| bits & (1 << 6) == 0).count());
}

#[test]
fn a_mutable_loop_over_many_tiny_archetypes_updates_each_row_once() {
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| 5 + bits % 5);
    let before: Vec<u64> = w.query::<&Id>().iter().map(|id| id.0).collect();
    assert_reaches_workers(before.len());
    on_four_threads(|| {
        w.query::<&mut Id>().par_for_each(|id| {
            slow(true);
            id.0 += 1_000_000;
        });
        w.query::<&mut Id>().par_for_each_indexed(|i, id| {
            slow(true);
            id.0 = id.0 * 10_000_000 + i as u64;
        });
    });
    let after: Vec<u64> = w.query::<&Id>().iter().map(|id| id.0).collect();
    let want: Vec<u64> = before
        .iter()
        .enumerate()
        .map(|(i, v)| (v + 1_000_000) * 10_000_000 + i as u64)
        .collect();
    assert_eq!(after, want);
}

#[test]
fn changed_filters_split_many_tiny_archetypes_into_runs_in_order() {
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    w.track_changes::<Id>();
    spawn_layouts(&mut w, layouts, |bits| 5 + bits % 6);
    let seen = w.advance_change_tick();
    // Change every third entity: each archetype yields several runs.
    let all: Vec<Entity> = w.query::<Entity>().iter().collect();
    for (i, &e) in all.iter().enumerate() {
        if i % 3 != 1 {
            w.get_mut::<Id>(e).unwrap().0 += 0;
        }
    }
    let later = w.advance_change_tick();
    fn changed(w: &mut World) -> Query<'_, &Id> {
        w.query::<&Id>().filter::<Changed<Id>>()
    }
    w.set_changes_since(seen);
    let n = assert_parallel_order(&mut w, changed);
    assert_eq!(n, (0..all.len()).filter(|i| i % 3 != 1).count());
    assert_reaches_workers(n);
    // A mutable parallel loop marks exactly the rows it visits.
    on_four_threads(|| {
        w.query::<&mut Id>()
            .filter::<Changed<Id>>()
            .since(seen)
            .par_for_each(|id| {
                slow(true);
                id.0 += 1;
            });
    });
    let marked = w.query::<&Id>().filter::<Changed<Id>>().since(later).count();
    assert_eq!(marked, n);
}

#[test]
fn a_mapper_panic_across_many_tiny_archetypes_drops_each_written_value_once() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| 3 + bits % 4);
    let n = w.query::<&Id>().count();
    // A panic in the timed first chunk, and one in the part after it.
    for (heavy, at) in [(false, MIN_CHUNK / 2), (true, n * 2 / 3)] {
        let panic_at = w.query::<&Id>().iter().nth(at).unwrap().0;
        let drops = Arc::new(AtomicUsize::new(0));
        let created = AtomicUsize::new(0);
        let mut out = Vec::new();
        on_four_threads(|| {
            let result = catch_unwind(AssertUnwindSafe(|| {
                w.query::<&Id>().par_collect_into(&mut out, |id| {
                    slow(heavy);
                    assert_ne!(id.0, panic_at, "mapper panic");
                    created.fetch_add(1, Ordering::Relaxed);
                    Counted(drops.clone())
                });
            }));
            assert!(result.is_err());
        });
        assert!(out.is_empty());
        assert_eq!(drops.load(Ordering::Relaxed), created.load(Ordering::Relaxed));
        // The world and the reused output still work after the unwind.
        on_four_threads(|| w.query::<&Id>().par_collect_into(&mut out, |_| Counted(drops.clone())));
        assert_eq!(out.len(), n);
    }
}

#[test]
fn a_panicking_parallel_loop_leaves_the_query_usable() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let layouts = if cfg!(miri) { 64 } else { 512 };
    let mut w = World::new();
    spawn_layouts(&mut w, layouts, |bits| 1 + bits % 4);
    let result = on_four_threads(|| {
        catch_unwind(AssertUnwindSafe(|| {
            w.query::<&Id>().par_for_each(|id| assert_ne!(id.0, 17, "loop panic"));
        }))
    });
    assert!(result.is_err());
    assert_parallel_order(&mut w, all_ids);
}

#[test]
#[cfg(not(miri))]
fn a_slow_query_over_tiny_archetypes_runs_on_more_than_one_thread() {
    use std::collections::HashSet;
    use std::sync::Mutex;
    if rayon::current_num_threads() < 2 {
        return; // One thread: nothing to spread over.
    }
    let mut w = World::new();
    spawn_layouts(&mut w, 512, |bits| 8 + bits % 8);
    let n = w.query::<&Id>().count();
    assert_reaches_workers(n);
    let threads = Mutex::new(HashSet::new());
    let here = std::thread::current().id();
    let rows_here = std::sync::atomic::AtomicUsize::new(0);
    // At least 2 µs per entity (a lower bound: a paused thread only spins
    // longer), so the first timed rows project the query far over the
    // serial budget. Those rows run here, on the test's own thread, which
    // is not one of the pool's, and every later row goes to the pool: rows
    // land on more than one thread however late the workers wake.
    w.query::<&Id>().par_for_each(|_| {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_micros(2) {
            std::hint::spin_loop();
        }
        if std::thread::current().id() == here {
            rows_here.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        threads.lock().unwrap().insert(std::thread::current().id());
    });
    assert_eq!(
        rows_here.into_inner(),
        PROBE_FIRST,
        "only the first timed rows run on the calling thread"
    );
    let threads = threads.into_inner().unwrap().len();
    assert!(threads > 1, "{threads} thread ran {n} rows of slow work");
}

#[derive(Component, Clone, Copy)]
struct Opt<const N: u8>(u32);

/// Sixteen archetypes, entity `i` with `Opt<k>` for each bit `k` of its
/// archetype's number, of sizes that make chunks start and end inside
/// archetypes and hold several of them. Returns each entity's id and bits.
fn option_world() -> (World, Vec<(u32, u32)>) {
    let chunk = if cfg!(miri) { 64 } else { 8192 };
    let mut w = World::new();
    let mut model = Vec::new();
    let mut id = 0;
    for m in 0..16u32 {
        let rows = match m % 4 {
            0 => chunk + chunk / 3,
            1 => chunk / 5,
            2 => 2 * chunk + 7,
            _ => 3,
        };
        for _ in 0..rows {
            let e = w.spawn((Id(u64::from(id)),));
            if m & 1 != 0 {
                w.insert(e, Opt::<0>(id));
            }
            if m & 2 != 0 {
                w.insert(e, Opt::<1>(id));
            }
            if m & 4 != 0 {
                w.insert(e, Opt::<2>(id));
            }
            if m & 8 != 0 {
                w.insert(e, Opt::<3>(id));
            }
            model.push((id, m));
            id += 1;
        }
    }
    (w, model)
}

#[test]
fn parallel_option_fetches_see_exactly_the_components_each_archetype_has() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let (mut w, model) = option_world();
    let packed = |opts: &[Option<u32>], id: u32| -> u32 {
        // Each present option holds its entity's id.
        assert!(opts.iter().flatten().all(|&v| v == id));
        opts.iter().enumerate().map(|(j, o)| (o.is_some() as u32) << j).sum()
    };
    let expect = |id: u32, bits: &[u32]| {
        let m = model[id as usize].1;
        bits.iter().enumerate().map(|(j, &k)| ((m >> k) & 1) << j).sum::<u32>()
    };

    // One option (split loops), positions in sequential order.
    let n = model.len();
    let out: Vec<AtomicU32> = (0..n).map(|_| AtomicU32::new(u32::MAX)).collect();
    let mut order = Vec::new();
    w.query::<(&Id, Option<&Opt<2>>)>()
        .for_each(|(id, _)| order.push(id.0 as u32));
    w.query::<(&Id, Option<&Opt<2>>)>().par_for_each_indexed(|i, (id, a)| {
        assert_eq!(packed(&[a.map(|o| o.0)], id.0 as u32), expect(id.0 as u32, &[2]));
        assert_eq!(
            out[i].swap(id.0 as u32, Ordering::Relaxed),
            u32::MAX,
            "position {i} twice"
        );
    });
    let got: Vec<u32> = out.iter().map(|a| a.load(Ordering::Relaxed)).collect();
    assert_eq!(got, order);

    // Two to four options, also spread over nested tuples (split loops),
    // and five (one loop that tests each row): collected in query order.
    let mut two = Vec::new();
    w.query::<(&Id, Option<&Opt<1>>, Option<&Opt<3>>)>()
        .par_collect_into(&mut two, |(id, a, b)| {
            ((id.0 as u32), packed(&[a.map(|o| o.0), b.map(|o| o.0)], id.0 as u32))
        });
    let mut nested = Vec::new();
    w.query::<((&Id, Option<&Opt<3>>), Option<&Opt<0>>)>()
        .par_collect_into(&mut nested, |((id, a), b)| {
            ((id.0 as u32), packed(&[a.map(|o| o.0), b.map(|o| o.0)], id.0 as u32))
        });
    let mut three = Vec::new();
    w.query::<(&Id, Option<&Opt<0>>, Option<&Opt<1>>, Option<&Opt<2>>)>()
        .par_collect_into(&mut three, |(id, a, b, c)| {
            (
                (id.0 as u32),
                packed(&[a.map(|o| o.0), b.map(|o| o.0), c.map(|o| o.0)], id.0 as u32),
            )
        });
    let mut four = Vec::new();
    w.query::<(
        (&Id, Option<&Opt<2>>),
        Option<&Opt<0>>,
        (Option<&Opt<3>>, Option<&Opt<1>>),
    )>()
    .par_collect_into(&mut four, |((id, a), b, (c, d))| {
        let all = [a.map(|o| o.0), b.map(|o| o.0), c.map(|o| o.0), d.map(|o| o.0)];
        ((id.0 as u32), packed(&all, id.0 as u32))
    });
    let mut five = Vec::new();
    w.query::<(
        &Id,
        Option<&Opt<0>>,
        Option<&Opt<1>>,
        Option<&Opt<2>>,
        Option<&Opt<3>>,
        Option<&Id>,
    )>()
    .par_collect_into(&mut five, |(id, a, b, c, d, again)| {
        assert_eq!(again, Some(id));
        let all = [a.map(|o| o.0), b.map(|o| o.0), c.map(|o| o.0), d.map(|o| o.0)];
        ((id.0 as u32), packed(&all, id.0 as u32))
    });
    for (got, bits) in [
        (&two, &[1, 3][..]),
        (&nested, &[3, 0]),
        (&three, &[0, 1, 2]),
        (&four, &[2, 0, 3, 1]),
        (&five, &[0, 1, 2, 3]),
    ] {
        assert_eq!(got.len(), n);
        let ids: Vec<u32> = got.iter().map(|x| x.0).collect();
        assert_eq!(ids, order, "the sequential order");
        for &(id, present) in got.iter() {
            assert_eq!(present, expect(id, bits), "entity {id}, options {bits:?}");
        }
    }

    // `Option<&mut T>` writes exactly the entities that have `T`, once each.
    w.query::<(Option<&mut Opt<1>>, Option<&mut Opt<2>>)>()
        .par_for_each(|(a, b)| {
            if let Some(a) = a {
                a.0 += 1_000_000;
            }
            if let Some(b) = b {
                b.0 += 2_000_000;
            }
        });
    let mut after = Vec::new();
    w.query::<(&Id, Option<&Opt<1>>, Option<&Opt<2>>)>()
        .for_each(|(id, a, b)| after.push(((id.0 as u32), a.map(|o| o.0), b.map(|o| o.0))));
    for (id, a, b) in after {
        let m = model[id as usize].1;
        assert_eq!(a, (m & 2 != 0).then_some(id + 1_000_000), "entity {id}");
        assert_eq!(b, (m & 4 != 0).then_some(id + 2_000_000), "entity {id}");
    }
}

#[test]
fn parallel_option_loops_mark_changes_only_where_the_component_is() {
    let (mut w, model) = option_world();
    w.track_changes::<Opt<1>>();
    let since = w.advance_change_tick();
    w.query::<(&Id, Option<&mut Opt<1>>, Option<&Opt<3>>)>()
        .par_for_each(|(_, a, b)| {
            if let (Some(a), Some(b)) = (a, b) {
                a.0 = b.0;
            }
        });
    let mut changed: Vec<u32> = w
        .query::<&Id>()
        .filter::<Changed<Opt<1>>>()
        .since(since)
        .iter()
        .map(|id| id.0 as u32)
        .collect();
    changed.sort_unstable();
    let with: Vec<u32> = model.iter().filter(|x| x.1 & 2 != 0).map(|x| x.0).collect();
    assert_eq!(changed, with);
}
