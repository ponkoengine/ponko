//! When the automatic archetype index pays: selectivity, churn, a game-like
//! frame and memory, each with the index off (`scan`), with the default
//! policy (`auto`) and, where it says so, forced (`index`). The numbers and
//! what they decided are in `QUERY_CACHE.md`.
//!
//! Every world has one layout per subset of some zero-sized markers, so
//! `layouts` is always a power of two and a query that requires `k` markers
//! matches `layouts / 2^k` of them.

use crate::{Bench, Pos, Vel};
use ponko_ecs::{
    ChunkQuery, CompiledFilter, ComponentId, DynFilter, Entity, Or, QueryCachePolicy, With, Without, World,
};
use std::hint::black_box;

#[derive(ponko_ecs::Component)]
struct Value(u64);
#[derive(ponko_ecs::Component)]
struct Marker<const N: usize>;

/// Calls `$f::<Marker<N>>(args)` for every marker whose bit is set in `$bits`.
macro_rules! for_markers {
    ($bits:expr, $f:ident($w:expr, $e:expr)) => {
        for_markers!(@each $bits, $f($w, $e), 0 1 2 3 4 5 6 7 8 9 10 11 12 13)
    };
    (@each $bits:expr, $f:ident($w:expr, $e:expr), $($n:literal)*) => {
        $(if $bits & (1 << $n) != 0 {
            $f::<Marker<$n>>($w, $e);
        })*
    };
}

fn add<M: ponko_ecs::Component + MarkerValue>(w: &mut World, e: Entity) {
    w.insert(e, M::VALUE);
}

fn toggle<M: ponko_ecs::Component + MarkerValue>(w: &mut World, e: Entity) {
    if w.remove::<M>(e).is_none() {
        w.insert(e, M::VALUE);
    }
}

trait MarkerValue: Sized {
    const VALUE: Self;
}
impl<const N: usize> MarkerValue for Marker<N> {
    const VALUE: Self = Marker;
}

#[derive(Clone, Copy, PartialEq)]
enum Strategy {
    /// The index off for the world: what every query did before.
    Scan,
    /// The default policy.
    Auto,
    /// Every signature indexed from one archetype up, whatever it matches.
    Index,
}

impl Strategy {
    fn name(self) -> &'static str {
        match self {
            Strategy::Scan => "scan",
            Strategy::Auto => "auto",
            Strategy::Index => "index",
        }
    }

    fn apply(self, w: &mut World) {
        match self {
            Strategy::Scan => w.set_query_cache_policy(QueryCachePolicy::off()),
            Strategy::Auto => {}
            Strategy::Index => {
                let mut policy = QueryCachePolicy::new();
                policy.min_archetypes = 1;
                policy.promote_after = 1;
                policy.max_match_percent = 100;
                w.set_query_cache_policy(policy);
            }
        }
    }
}

/// The world's layouts, not counting the one without components.
fn layouts_of(w: &World) -> usize {
    let all = w.raw_ref().archetypes();
    all.iter().filter(|a| !a.component_ids().is_empty()).count()
}

/// One entity with a `Value` in each of `layouts` layouts.
fn sparse_world(layouts: usize, strategy: Strategy) -> World {
    let mut w = World::new();
    strategy.apply(&mut w);
    for bits in 0..layouts {
        let e = w.spawn((Value(bits as u64),));
        for_markers!(bits, add(&mut w, e));
    }
    assert_eq!(layouts_of(&w), layouts);
    w
}

/// `count()` of a query that requires the first `k` markers.
fn count_with(w: &mut World, k: usize) -> usize {
    let q = w.query::<&Value>();
    match k {
        0 => {
            let mut q = q;
            q.count()
        }
        1 => q.filter::<With<Marker<0>>>().count(),
        2 => q.filter::<With<Marker<0>>>().filter::<With<Marker<1>>>().count(),
        4 => q
            .filter::<(With<Marker<0>>, With<Marker<1>>, With<Marker<2>>, With<Marker<3>>)>()
            .count(),
        _ => q
            .filter::<(
                With<Marker<0>>,
                With<Marker<1>>,
                With<Marker<2>>,
                With<Marker<3>>,
                With<Marker<4>>,
                With<Marker<5>>,
            )>()
            .count(),
    }
}

/// `for_each` of the same queries.
fn sum_with(w: &mut World, k: usize) -> u64 {
    let mut sum = 0;
    let q = w.query::<&Value>();
    let mut q = match k {
        0 => q,
        1 => q.filter::<With<Marker<0>>>(),
        2 => q.filter::<With<Marker<0>>>().filter::<With<Marker<1>>>(),
        4 => q.filter::<(With<Marker<0>>, With<Marker<1>>, With<Marker<2>>, With<Marker<3>>)>(),
        _ => q.filter::<(
            With<Marker<0>>,
            With<Marker<1>>,
            With<Marker<2>>,
            With<Marker<3>>,
            With<Marker<4>>,
            With<Marker<5>>,
        )>(),
    };
    q.for_each(|v| sum += v.0);
    sum
}

/// Selectivity and the crossover: 1,000 queries per batch that match all,
/// 1/2, 1/4, 1/16 or 1/64 of the layouts.
fn selectivity(b: &mut Bench) {
    for layouts in [8, 16, 32, 64, 128, 256, 1024, 4096] {
        for (share, k) in [("all", 0), ("1of2", 1), ("1of4", 2), ("1of16", 4), ("1of64", 6)] {
            if 1 << k > layouts {
                continue;
            }
            for strategy in [Strategy::Scan, Strategy::Auto, Strategy::Index] {
                b.steady(
                    &format!("query-policy/select/{layouts}/{share}/count/{}", strategy.name()),
                    100,
                    || sparse_world(layouts, strategy),
                    |w| {
                        for _ in 0..1000 {
                            black_box(count_with(w, k));
                        }
                    },
                );
                b.steady(
                    &format!("query-policy/select/{layouts}/{share}/each/{}", strategy.name()),
                    100,
                    || sparse_world(layouts, strategy),
                    |w| {
                        for _ in 0..1000 {
                            black_box(sum_with(w, k));
                        }
                    },
                );
            }
        }
    }
}

/// Four signatures a system might repeat: 1/4, 1/16, 3/4 (an `Or`) and 1/8.
fn four_queries(w: &mut World) -> usize {
    let a = w
        .query::<&Value>()
        .filter::<With<Marker<0>>>()
        .filter::<With<Marker<1>>>()
        .count();
    let b = count_with(w, 4);
    let c = w
        .query::<&Value>()
        .filter::<Or<(With<Marker<2>>, With<Marker<3>>)>>()
        .count();
    let d = w
        .query::<&Value>()
        .filter::<(With<Marker<4>>, With<Marker<5>>, Without<Marker<6>>)>()
        .count();
    a + b + c + d
}

struct Churn {
    w: World,
    entities: Vec<Entity>,
    step: usize,
}

fn churn_world(strategy: Strategy) -> Churn {
    let mut w = sparse_world(1024, strategy);
    let mut entities = Vec::new();
    w.query::<Entity>().for_each(|e| entities.push(e));
    // The signatures are known (and, with `auto`, indexed) before timing.
    for _ in 0..3 {
        black_box(four_queries(&mut w));
    }
    Churn { w, entities, step: 0 }
}

/// Structural changes between queries, 1024 layouts, 4 queries after each
/// change: layouts created, entities moved between layouts, entities
/// spawned and despawned (emptying layouts), components registered.
fn churn(b: &mut Bench) {
    for strategy in [Strategy::Scan, Strategy::Auto] {
        let name = strategy.name();
        // 256 new layouts per sample, each followed by the four queries:
        // the index lists a new layout before its next use.
        b.run(
            &format!("query-policy/churn/new-layouts/{name}"),
            50,
            || churn_world(strategy),
            |s| {
                for i in 0..256 {
                    // Markers 10 and 11 are new to this world: every entity
                    // moved gets a layout nobody has had.
                    let e = s.entities[i * 4];
                    s.w.insert(e, Marker::<10>);
                    if i % 2 == 0 {
                        s.w.insert(e, Marker::<11>);
                    }
                    black_box(four_queries(&mut s.w));
                }
            },
        );
        // Moves between existing layouts.
        b.steady(
            &format!("query-policy/churn/moves/{name}"),
            100,
            || churn_world(strategy),
            |s| {
                for _ in 0..256 {
                    s.step = s
                        .step
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let e = s.entities[(s.step >> 33) % s.entities.len()];
                    let bit = 1usize << ((s.step >> 20) % 10);
                    for_markers!(bit, toggle(&mut s.w, e));
                    black_box(four_queries(&mut s.w));
                }
            },
        );
        // Despawns that empty a layout, and spawns that fill it again.
        b.steady(
            &format!("query-policy/churn/spawn-despawn/{name}"),
            100,
            || churn_world(strategy),
            |s| {
                for _ in 0..256 {
                    s.step = s
                        .step
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    let slot = (s.step >> 33) % s.entities.len();
                    let bits = (s.step >> 20) % 1024;
                    s.w.despawn(s.entities[slot]);
                    black_box(four_queries(&mut s.w));
                    let e = s.w.spawn((Value(bits as u64),));
                    for_markers!(bits, add(&mut s.w, e));
                    s.entities[slot] = e;
                    black_box(four_queries(&mut s.w));
                }
            },
        );
        // Components registered late, between queries.
        b.steady(
            &format!("query-policy/churn/late-register/{name}"),
            100,
            || churn_world(strategy),
            |s| {
                for _ in 0..256 {
                    s.step += 1;
                    black_box(register_late(&mut s.w, s.step));
                    black_box(four_queries(&mut s.w));
                }
            },
        );
    }
}

/// Registers component `late-<n>` (4 plain bytes) between queries.
fn register_late(w: &mut World, n: usize) -> ComponentId {
    // SAFETY: a plain 4-byte component without a destructor.
    unsafe { w.raw().register_component(&format!("late-{n}"), 4, 4, None) }
}

struct Game {
    w: World,
    entities: Vec<Entity>,
    layouts: usize,
    step: usize,
    /// `(Or<(With<Marker<1>>, With<Marker<2>>)>, Without<Marker<0>>)`, as
    /// the C API would hold it.
    filter: CompiledFilter,
    pos: ComponentId,
}

/// `entities` entities over `layouts` layouts: half of them in the first 8
/// layouts, the rest spread evenly. Every entity has `Pos`; the ones with
/// marker 0 also have `Vel`.
fn game_world(layouts: usize, entities: usize, strategy: Strategy) -> Game {
    let mut w = World::new();
    strategy.apply(&mut w);
    let mut all = Vec::with_capacity(entities);
    let spawn = |w: &mut World, bits: usize, i: usize| {
        let pos = Pos { x: i as f32, y: 0.0 };
        let e = if bits & 1 != 0 {
            w.spawn((pos, Vel { x: 1.0, y: 0.5 }, Marker::<0>))
        } else {
            w.spawn((pos,))
        };
        let rest = bits & !1;
        for_markers!(rest, add(w, e));
        e
    };
    // One of each first, in order, so the world has exactly `layouts` layouts.
    for bits in 0..layouts {
        all.push(spawn(&mut w, bits, bits));
    }
    for i in layouts..entities {
        let bits = if i % 2 == 0 { i / 2 % 8 } else { i / 2 % layouts };
        all.push(spawn(&mut w, bits, i));
    }
    assert_eq!(layouts_of(&w), layouts);
    let (m0, m1, m2) = (
        w.register::<Marker<0>>(),
        w.register::<Marker<1>>(),
        w.register::<Marker<2>>(),
    );
    let filter = DynFilter::All(vec![
        DynFilter::Or(vec![DynFilter::With(m1), DynFilter::With(m2)]),
        DynFilter::Without(m0),
    ])
    .compile()
    .unwrap();
    let pos = w.register::<Pos>();
    Game {
        w,
        entities: all,
        layouts,
        step: 0,
        filter,
        pos,
    }
}

/// One frame: 20 queries of mixed selectivity (dense movement, selective
/// gameplay queries, `Or` filters, counts, an iterator, a parallel loop and a
/// runtime-filtered chunk query), then 64 moves, 32 despawns and 32 spawns.
fn frame(g: &mut Game) -> f32 {
    let w = &mut g.w;
    let dt = black_box(1.0 / 60.0);
    let mut acc = 0.0;
    // Movement: half the layouts, most of the rows.
    w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
    // Everything.
    acc += w.query::<&Pos>().count() as f32;
    // Selective gameplay queries: 1/4 to 1/64 of the layouts.
    w.query::<&mut Pos>()
        .filter::<With<Marker<1>>>()
        .filter::<With<Marker<2>>>()
        .for_each(|p| p.y += dt);
    w.query::<&mut Pos>()
        .filter::<With<Marker<2>>>()
        .filter::<Without<Marker<3>>>()
        .for_each(|p| p.y -= dt);
    w.query::<(&mut Pos, &Vel)>()
        .filter::<With<Marker<3>>>()
        .filter::<With<Marker<4>>>()
        .for_each(|(p, v)| p.x -= v.x * dt);
    w.query::<&Pos>()
        .filter::<(With<Marker<1>>, With<Marker<3>>, With<Marker<5>>)>()
        .for_each(|p| acc += p.x);
    w.query::<&Pos>()
        .filter::<(With<Marker<2>>, With<Marker<4>>, Without<Marker<5>>, Without<Marker<1>>)>()
        .for_each(|p| acc += p.y);
    w.query::<&Pos>()
        .filter::<(With<Marker<1>>, With<Marker<2>>, With<Marker<3>>, With<Marker<4>>)>()
        .for_each(|p| acc += p.x);
    w.query::<&Pos>()
        .filter::<(
            With<Marker<0>>,
            With<Marker<1>>,
            With<Marker<2>>,
            With<Marker<3>>,
            With<Marker<4>>,
            With<Marker<5>>,
        )>()
        .for_each(|p| acc += p.x);
    w.query::<(&Pos, &Vel)>()
        .filter::<Without<Marker<1>>>()
        .filter::<Without<Marker<2>>>()
        .filter::<Without<Marker<3>>>()
        .for_each(|(p, v)| acc += p.x * v.y);
    // `Or` filters: 3/4 of the layouts, and 5/8.
    w.query::<&Pos>()
        .filter::<Or<(With<Marker<1>>, With<Marker<2>>)>>()
        .for_each(|p| acc += p.y);
    w.query::<&Pos>()
        .filter::<Or<((With<Marker<3>>, With<Marker<4>>), With<Marker<5>>)>>()
        .for_each(|p| acc += p.y);
    w.query::<&Pos>()
        .filter::<(Or<(With<Marker<4>>, Without<Marker<5>>)>, With<Marker<3>>)>()
        .for_each(|p| acc += p.x);
    // Counts and an iterator.
    acc += w.query::<&Vel>().filter::<With<Marker<5>>>().count() as f32;
    acc += w
        .query::<Entity>()
        .filter::<Without<Marker<0>>>()
        .filter::<With<Marker<4>>>()
        .count() as f32;
    acc += w
        .query::<&Pos>()
        .filter::<With<Marker<3>>>()
        .filter::<With<Marker<5>>>()
        .count() as f32;
    acc += w
        .query::<&Pos>()
        .filter::<With<Marker<1>>>()
        .filter::<With<Marker<4>>>()
        .iter()
        .map(|p| p.x)
        .sum::<f32>();
    acc += w
        .query::<(Entity, &Pos)>()
        .filter::<With<Marker<2>>>()
        .filter::<With<Marker<5>>>()
        .iter()
        .map(|(_, p)| p.y)
        .sum::<f32>();
    // A parallel loop over 1/4 of the layouts.
    w.query::<&mut Pos>()
        .filter::<With<Marker<4>>>()
        .filter::<With<Marker<5>>>()
        .par_for_each(|p| p.x += dt);
    // The C path: raw chunks with a runtime filter.
    let ids = [g.pos];
    let mut rows = 0;
    w.raw()
        .query_chunks(ChunkQuery::new(&ids).writes(0).filter(&g.filter), |chunk| {
            rows += chunk.len()
        });
    acc += rows as f32;
    // Structural changes: moves between layouts, despawns and spawns.
    let bits = g.layouts.trailing_zeros() as usize;
    for i in 0..64 {
        g.step = g
            .step
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let slot = (g.step >> 33) % g.entities.len();
        let e = g.entities[slot];
        // Marker 0 goes with `Vel`: leave it alone.
        let bit = 2usize << ((g.step >> 20) % (bits - 1));
        for_markers!(bit, toggle(w, e));
        if i % 2 == 0 {
            w.despawn(e);
            let layout = ((g.step >> 12) % g.layouts) & !1;
            let e = w.spawn((Pos { x: i as f32, y: 0.0 },));
            for_markers!(layout, add(w, e));
            g.entities[slot] = e;
        }
    }
    acc
}

/// Frame time of the mixed workload, 50,000 entities.
fn frames(b: &mut Bench) {
    for layouts in [16, 64, 256, 1024, 4096] {
        for strategy in [Strategy::Scan, Strategy::Auto] {
            b.steady(
                &format!("query-policy/frame/{layouts}/{}", strategy.name()),
                300,
                || game_world(layouts, 50_000, strategy),
                |g| {
                    black_box(frame(g));
                },
            );
        }
    }
}

/// Peak resident memory of this process so far, in bytes.
/// The index's own bytes (the `KiB` column) and the process's peak resident
/// memory after 100 frames of the mixed workload. Run one case per process
/// (`-- query-policy/mem/4096/auto`): the peak never goes down.
fn memory(b: &mut Bench) {
    for layouts in [64, 1024, 4096, 16384] {
        for strategy in [Strategy::Scan, Strategy::Auto, Strategy::Index] {
            let name = format!("query-policy/mem/{layouts}/{}", strategy.name());
            if b.skips(&name) {
                continue;
            }
            let mut stats = None;
            b.steady_with_bytes(
                &name,
                100,
                || game_world(layouts, 50_000, strategy),
                |g| {
                    black_box(frame(g));
                },
                |g| {
                    stats = Some((g.w.query_cache_stats(), g.w.allocated_bytes()));
                    Some(g.w.query_cache_stats().bytes)
                },
            );
            if let Some((stats, world)) = stats {
                println!(
                    "    index: {} signatures, {} indexed, {} indices, {} bytes; world storage {} KiB; peak RSS {}",
                    stats.signatures,
                    stats.indexed,
                    stats.indices,
                    stats.bytes,
                    world / 1024,
                    crate::alloc::peak_rss().map_or("unavailable".into(), |b| format!("{} KiB", b / 1024)),
                );
            }
        }
    }
}

/// What `promote_after` trades: 45 different signatures (two of ten
/// markers each, 1/4 of 1,024 layouts) in a fresh world, each used once,
/// twice, 4 or 16 times, indexed on the first, second or eighth use. A
/// signature used once pays for a list it never reads when the first use
/// indexes it; one used often scans longer when a late use does.
fn promotion(b: &mut Bench) {
    for uses in [1, 2, 4, 16] {
        for promote_after in [1, 2, 8] {
            b.run(
                &format!("query-policy/promote/uses-{uses}/after-{promote_after}"),
                40,
                || {
                    let mut w = sparse_world(1024, Strategy::Auto);
                    let mut policy = QueryCachePolicy::new();
                    policy.promote_after = promote_after;
                    w.set_query_cache_policy(policy);
                    let value = w.register::<Value>();
                    let markers = [
                        w.register::<Marker<0>>(),
                        w.register::<Marker<1>>(),
                        w.register::<Marker<2>>(),
                        w.register::<Marker<3>>(),
                        w.register::<Marker<4>>(),
                        w.register::<Marker<5>>(),
                        w.register::<Marker<6>>(),
                        w.register::<Marker<7>>(),
                        w.register::<Marker<8>>(),
                        w.register::<Marker<9>>(),
                    ];
                    let mut signatures = Vec::new();
                    for (i, a) in markers.iter().enumerate() {
                        for b in &markers[i + 1..] {
                            signatures.push([value, *a, *b]);
                        }
                    }
                    (w, signatures)
                },
                |(w, signatures)| {
                    let mut rows = 0;
                    for _ in 0..uses {
                        for ids in signatures.iter() {
                            w.raw().for_each_chunk(ids, |chunk| rows += chunk.len());
                        }
                    }
                    assert_eq!(rows, uses * signatures.len() * 256);
                    black_box(rows);
                },
            );
        }
    }
}

pub fn run(b: &mut Bench) {
    selectivity(b);
    churn(b);
    frames(b);
    memory(b);
    promotion(b);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The strategies are compared on the same work: a frame leaves the
    /// same world behind whether the index is off, automatic or forced.
    #[test]
    fn a_frame_does_the_same_work_with_and_without_the_index() {
        let mut results = Vec::new();
        for strategy in [Strategy::Scan, Strategy::Auto, Strategy::Index] {
            let mut g = game_world(256, 4096, strategy);
            let mut acc = Vec::new();
            for _ in 0..8 {
                acc.push(frame(&mut g).to_bits());
            }
            let mut rows = Vec::new();
            g.w.query::<(Entity, &Pos)>()
                .for_each(|(e, p)| rows.push((e, p.x.to_bits(), p.y.to_bits())));
            results.push((acc, rows, g.w.query_cache_stats().hits > 0));
        }
        assert_eq!(results[0].0, results[1].0);
        assert_eq!(results[0].1, results[1].1);
        assert_eq!(results[0].1, results[2].1);
        // And the index was really off, and really used.
        assert_eq!([results[0].2, results[1].2, results[2].2], [false, true, true]);
    }

    #[test]
    fn the_selectivity_queries_match_the_share_they_are_named_for() {
        let mut w = sparse_world(256, Strategy::Auto);
        for (k, expected) in [(0, 256), (1, 128), (2, 64), (4, 16), (6, 4)] {
            assert_eq!(count_with(&mut w, k), expected);
            assert!(sum_with(&mut w, k) > 0);
        }
    }

    /// Registering components between queries changes no query's answer.
    #[test]
    fn components_registered_late_keep_query_results() {
        let mut s = churn_world(Strategy::Auto);
        let before = four_queries(&mut s.w);
        let ids: Vec<ComponentId> = (0..16).map(|n| register_late(&mut s.w, n)).collect();
        assert!(ids.windows(2).all(|p| p[0] != p[1]));
        assert_eq!(s.w.component_info(ids[3]).name(), "late-3");
        assert_eq!(four_queries(&mut s.w), before);
    }

    #[test]
    fn churn_creates_layouts_and_keeps_query_results_equal() {
        let mut scan = churn_world(Strategy::Scan);
        let mut auto = churn_world(Strategy::Auto);
        for i in 0..64 {
            for s in [&mut scan, &mut auto] {
                let e = s.entities[i * 4];
                s.w.insert(e, Marker::<10>);
            }
            assert_eq!(four_queries(&mut scan.w), four_queries(&mut auto.w));
        }
        assert_eq!(layouts_of(&auto.w), 1024 + 64);
        assert!(auto.w.query_cache_stats().hits > 0);
    }
}
