//! Many small archetypes, and building queries over them, in ponko_ecs,
//! bevy_ecs and hecs.
//!
//! The world: 1024 archetypes of 16 entities each (16384 in total). Every
//! entity has `(Pos, Vel, Spr)` plus a different subset of 10 marker
//! components. Games with many component combinations look like this: lots of
//! archetypes, few entities in each, so per-archetype costs dominate.
//!
//! - `archetypes-1024/*`: `pos += vel * dt` over all of them, with the query
//!   built once (bevy keeps a `QueryState`; ponko builds a `Query` per run,
//!   which is how systems use it).
//! - `query-build-x1000/*`: build the `(&mut Pos, &Vel)` query 1000 times.
//!   bevy matches archetypes when it builds a `QueryState`; ponko resolves
//!   component ids only and matches while iterating, so the two numbers measure
//!   different work. Compare them together with `query-build+count`.
//! - `query-build+count-x1000/*`: build the query and count its matches, 1000
//!   times in the same world. For ponko and bevy this is the same work:
//!   resolve, match all 1024 archetypes, read lengths (ponko's archetype
//!   index stays off for this query, which matches every archetype). hecs
//!   keeps a per-world cache of each query type's matching archetypes,
//!   rebuilt only when the archetype set changes, so its row is a cache hit
//!   (a lock, a hash lookup, summing lengths) after the first build.
//! - `query-build+count-cold/*`: the first query of a world: build it and
//!   count its matches once, in a freshly built world (rebuilt untimed before
//!   every run), so no library has anything cached. This is the row where all
//!   three really match all 1024 archetypes, including the bookkeeping each
//!   does for a world's first query.
//!
//! hecs, like ponko, matches archetypes when a query runs, not when it is
//! built, so its `query-build` row only borrows the world.
//!
//! Change filters over the same archetypes (ponko only), with `Pos`
//! tracked and nothing written since the reader's tick unless named:
//!
//! - `archetypes-1024-tracked/ponko`: the `&Pos` loop without a filter, in
//!   the tracked world: what the loop costs before any filter.
//! - `archetypes-1024-changed-none/ponko`: a `Changed<Pos>` loop that
//!   finds nothing: the cost of matching the filter in every archetype.
//! - `archetypes-1024-changed-1in16/ponko`: the same with one entity in 16
//!   changed (one per archetype).
//! - `query-build+count-changed-x1000/ponko`: build a `Changed<Pos>` query
//!   and count it, 1000 times, nothing changed.

use bevy_ecs::component::Component;
use std::hint::black_box;

use crate::{Bench, DT, Pos, Vel, ent};

/// Marker components: entity `i` has `Mk<k>` when bit `k` of its archetype index is set.
#[derive(Component, Clone, Copy, Default, ponko_ecs::Component)]
struct Mk<const K: usize>;

pub const MARKERS: usize = 10;
pub const ARCHETYPES: usize = 1 << MARKERS;
pub const PER_ARCHETYPE: usize = 16;
pub const ENTITIES: usize = ARCHETYPES * PER_ARCHETYPE;
const QUERY_BUILDS: usize = 1000;

/// Runs `$body` with `$m` naming the marker type for bit `$k`.
macro_rules! with_marker {
    ($k:expr, |$m:ident| $body:expr) => {
        match $k {
            0 => {
                type $m = Mk<0>;
                $body
            }
            1 => {
                type $m = Mk<1>;
                $body
            }
            2 => {
                type $m = Mk<2>;
                $body
            }
            3 => {
                type $m = Mk<3>;
                $body
            }
            4 => {
                type $m = Mk<4>;
                $body
            }
            5 => {
                type $m = Mk<5>;
                $body
            }
            6 => {
                type $m = Mk<6>;
                $body
            }
            7 => {
                type $m = Mk<7>;
                $body
            }
            8 => {
                type $m = Mk<8>;
                $body
            }
            9 => {
                type $m = Mk<9>;
                $body
            }
            _ => unreachable!("only {MARKERS} markers"),
        }
    };
}

/// The marker bits of entity `i`. Consecutive entities land in different archetypes.
fn bits(i: usize) -> impl Iterator<Item = usize> {
    (0..MARKERS).filter(move |k| (i % ARCHETYPES) & (1 << k) != 0)
}

// Markers are added in ascending bit order, so every intermediate component
// set is itself one of the 1024 final sets: no extra archetypes appear.

fn ponko_world() -> ponko_ecs::World {
    let mut w = ponko_ecs::World::new();
    for i in 0..ENTITIES {
        let e = w.spawn(ent(i));
        for k in bits(i) {
            with_marker!(k, |M| w.insert(e, M::default()));
        }
    }
    w
}

fn bevy_world() -> bevy_ecs::world::World {
    let mut w = bevy_ecs::world::World::new();
    for i in 0..ENTITIES {
        let mut e = w.spawn(ent(i));
        for k in bits(i) {
            with_marker!(k, |M| e.insert(M::default()));
        }
    }
    w
}

/// [`ponko_world`] with `Pos` tracked, the reader's tick taken after it was
/// built, and one entity in `changed_every` changed since (none for 0).
fn ponko_tracked_world(changed_every: usize) -> (ponko_ecs::World, ponko_ecs::Tick) {
    let mut w = ponko_world();
    w.track_changes::<Pos>();
    let seen = w.advance_change_tick();
    if changed_every > 0 {
        let es: Vec<_> = w.query::<ponko_ecs::Entity>().iter().step_by(changed_every).collect();
        for e in es {
            w.get_mut::<Pos>(e).unwrap().x += 1.0;
        }
    }
    (w, seen)
}

fn changed_ponko(w: &mut ponko_ecs::World, seen: ponko_ecs::Tick) -> usize {
    let mut n = 0;
    w.query::<&Pos>()
        .filter::<ponko_ecs::Changed<Pos>>()
        .since(seen)
        .for_each(|p| {
            black_box(p);
            n += 1;
        });
    n
}

fn hecs_world() -> hecs::World {
    let mut w = hecs::World::new();
    for i in 0..ENTITIES {
        let e = w.spawn(ent(i));
        for k in bits(i) {
            with_marker!(k, |M| w.insert_one(e, M::default()).unwrap());
        }
    }
    w
}

fn step_ponko(w: &mut ponko_ecs::World, dt: f32) {
    w.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

type BevyMove = bevy_ecs::query::QueryState<(&'static mut Pos, &'static Vel)>;

fn step_bevy(w: &mut bevy_ecs::world::World, q: &mut BevyMove, dt: f32) {
    q.iter_mut(w).for_each(|(mut p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

fn step_hecs(w: &mut hecs::World, dt: f32) {
    w.query_mut::<(&mut Pos, &Vel)>().into_iter().for_each(|(p, v)| {
        p.x += v.x * dt;
        p.y += v.y * dt;
    });
}

fn count_hecs(w: &mut hecs::World) -> usize {
    w.query_mut::<(&mut Pos, &Vel)>().into_iter().len()
}

fn count_ponko(w: &mut ponko_ecs::World) -> usize {
    w.query::<(&mut Pos, &Vel)>().count()
}

fn count_bevy(w: &mut bevy_ecs::world::World) -> usize {
    w.query::<(&mut Pos, &Vel)>().iter_mut(w).len()
}

pub fn run(b: &mut Bench) {
    b.steady_with_bytes(
        "archetypes-1024/ponko",
        200,
        ponko_world,
        |w| step_ponko(w, black_box(DT)),
        |w| Some(w.allocated_bytes()),
    );
    b.steady(
        "archetypes-1024/bevy",
        200,
        || {
            let mut w = bevy_world();
            let q = w.query::<(&mut Pos, &Vel)>();
            (w, q)
        },
        |(w, q)| step_bevy(w, q, black_box(DT)),
    );
    b.steady("archetypes-1024/hecs", 200, hecs_world, |w| step_hecs(w, black_box(DT)));

    b.steady(&format!("query-build-x{QUERY_BUILDS}/ponko"), 100, ponko_world, |w| {
        for _ in 0..QUERY_BUILDS {
            drop(black_box(w.query::<(&mut Pos, &Vel)>()));
        }
    });
    b.steady(&format!("query-build-x{QUERY_BUILDS}/bevy"), 100, bevy_world, |w| {
        for _ in 0..QUERY_BUILDS {
            black_box(w.query::<(&mut Pos, &Vel)>());
        }
    });
    b.steady(&format!("query-build-x{QUERY_BUILDS}/hecs"), 100, hecs_world, |w| {
        for _ in 0..QUERY_BUILDS {
            black_box(w.query_mut::<(&mut Pos, &Vel)>());
        }
    });

    b.steady(
        &format!("query-build+count-x{QUERY_BUILDS}/ponko"),
        100,
        ponko_world,
        |w| {
            for _ in 0..QUERY_BUILDS {
                black_box(count_ponko(w));
            }
        },
    );
    b.steady(
        &format!("query-build+count-x{QUERY_BUILDS}/bevy"),
        100,
        bevy_world,
        |w| {
            for _ in 0..QUERY_BUILDS {
                black_box(count_bevy(w));
            }
        },
    );
    b.steady(
        &format!("query-build+count-x{QUERY_BUILDS}/hecs"),
        100,
        hecs_world,
        |w| {
            for _ in 0..QUERY_BUILDS {
                black_box(count_hecs(w));
            }
        },
    );

    b.steady(
        "archetypes-1024-tracked/ponko",
        200,
        || ponko_tracked_world(0),
        |(w, _)| {
            w.query::<&Pos>().for_each(|p| {
                black_box(p);
            })
        },
    );
    b.steady(
        "archetypes-1024-changed-none/ponko",
        200,
        || ponko_tracked_world(0),
        |(w, seen)| {
            black_box(changed_ponko(w, *seen));
        },
    );
    b.steady(
        "archetypes-1024-changed-1in16/ponko",
        200,
        || ponko_tracked_world(16),
        |(w, seen)| {
            black_box(changed_ponko(w, *seen));
        },
    );
    b.steady(
        &format!("query-build+count-changed-x{QUERY_BUILDS}/ponko"),
        100,
        || ponko_tracked_world(0),
        |(w, seen)| {
            for _ in 0..QUERY_BUILDS {
                black_box(
                    w.query::<&Pos>()
                        .filter::<ponko_ecs::Changed<Pos>>()
                        .since(*seen)
                        .count(),
                );
            }
        },
    );

    b.run("query-build+count-cold/ponko", 200, ponko_world, |w| {
        black_box(count_ponko(w));
    });
    b.run("query-build+count-cold/bevy", 200, bevy_world, |w| {
        black_box(count_bevy(w));
    });
    b.run("query-build+count-cold/hecs", 200, hecs_world, |w| {
        black_box(count_hecs(w));
    });
}

// Correctness of the workloads, checked apart from their timing: every
// library must build the same world and do the same work on it.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worlds_have_1024_archetypes_of_16_entities() {
        let w = ponko_world();
        let sizes: Vec<_> = w
            .raw_ref()
            .archetypes()
            .iter()
            .map(|a| a.len())
            .filter(|&n| n > 0)
            .collect();
        assert_eq!(sizes.len(), ARCHETYPES);
        assert!(sizes.iter().all(|&n| n == PER_ARCHETYPE));
        assert_eq!(w.len(), ENTITIES);

        // bevy also keeps entities of its own, so only count archetypes with `Pos`.
        let w = bevy_world();
        let pos = w.component_id::<Pos>().unwrap();
        let sizes: Vec<_> = w
            .archetypes()
            .iter()
            .filter(|a| a.contains(pos))
            .map(|a| a.len())
            .filter(|&n| n > 0)
            .collect();
        assert_eq!(sizes.len(), ARCHETYPES);
        assert!(sizes.iter().all(|&n| n == PER_ARCHETYPE as u32));

        let w = hecs_world();
        let sizes: Vec<_> = w.archetypes().map(|a| a.len()).filter(|&n| n > 0).collect();
        assert_eq!(sizes.len(), ARCHETYPES);
        assert!(sizes.iter().all(|&n| n == PER_ARCHETYPE as u32));
    }

    #[test]
    fn change_filter_rows_find_what_changed() {
        let (mut w, seen) = ponko_tracked_world(0);
        assert_eq!(changed_ponko(&mut w, seen), 0);
        let (mut w, seen) = ponko_tracked_world(16);
        assert_eq!(changed_ponko(&mut w, seen), ENTITIES / 16);
        assert_eq!(
            w.query::<&Pos>()
                .filter::<ponko_ecs::Changed<Pos>>()
                .since(seen)
                .count(),
            ENTITIES / 16
        );
    }

    #[test]
    fn cold_counts_match_every_archetype_of_a_fresh_world() {
        // ponko: nothing remembered before, and the count scans (no index hit).
        let mut w = ponko_world();
        assert_eq!(w.query_cache_stats().signatures, 0);
        assert_eq!(count_ponko(&mut w), ENTITIES);
        assert_eq!(w.query_cache_stats().hits, 0);
        assert_eq!(count_bevy(&mut bevy_world()), ENTITIES);

        // hecs caches matches per world and query type. A fresh world has
        // none, so its first count matches; a new archetype invalidates the
        // cache, which is what makes the warm row a cache hit and this one not.
        let mut h = hecs_world();
        assert_eq!(count_hecs(&mut h), ENTITIES);
        let generation = h.archetypes_generation();
        assert_eq!(count_hecs(&mut h), ENTITIES);
        assert_eq!(
            h.archetypes_generation(),
            generation,
            "a warm count changes no archetypes"
        );
        h.spawn((Pos::default(), Vel::default(), 0u8));
        assert_ne!(h.archetypes_generation(), generation);
        assert_eq!(count_hecs(&mut h), ENTITIES + 1);
    }

    #[test]
    fn queries_count_every_entity_in_both() {
        assert_eq!(count_ponko(&mut ponko_world()), ENTITIES);
        assert_eq!(count_bevy(&mut bevy_world()), ENTITIES);
        assert_eq!(count_hecs(&mut hecs_world()), ENTITIES);
    }

    #[test]
    fn steps_move_every_entity_the_same() {
        let mut pw = ponko_world();
        let mut bw = bevy_world();
        let mut q = bw.query::<(&mut Pos, &Vel)>();
        let mut hw = hecs_world();
        for _ in 0..3 {
            step_ponko(&mut pw, DT);
            step_bevy(&mut bw, &mut q, DT);
            step_hecs(&mut hw, DT);
        }
        let mut a: Vec<_> = pw
            .query::<&Pos>()
            .iter()
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        let mut b: Vec<_> = bw
            .query::<&Pos>()
            .iter(&bw)
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        let mut h: Vec<_> = hw
            .query_mut::<&Pos>()
            .into_iter()
            .map(|p| (p.x.to_bits(), p.y.to_bits()))
            .collect();
        a.sort_unstable();
        b.sort_unstable();
        h.sort_unstable();
        assert_eq!(a, b);
        assert_eq!(a, h);
        // Moved by 3 steps of (1.0, 0.5) * dt from (i, 0).
        let y = (0.0f32 + 0.5 * DT + 0.5 * DT + 0.5 * DT).to_bits();
        assert!(a.iter().all(|&(_, py)| py == y));
        assert_eq!(a.len(), ENTITIES);
    }
}
