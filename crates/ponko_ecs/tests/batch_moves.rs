//! `World::insert_many` and `World::remove_many`: the same world as
//! `insert` and `remove` on each entity in turn (component values, which
//! archetype and row each entity is in, change ticks and removal logs),
//! through the whole-archetype copy and the entity-by-entity path.

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use ponko_ecs::*;

#[derive(Debug, PartialEq, Clone, Copy, Component)]
struct Num(u32);
#[derive(Debug, PartialEq, Clone, Copy, Component)]
struct Mark(u8);
#[derive(Debug, PartialEq, Clone, Component)]
struct Name(String);

/// Everything a batch must leave as single moves would: per archetype
/// (by index), its components and its entities in row order, then each
/// live entity's values.
fn fingerprint(w: &mut World, all: &[Entity]) -> String {
    let mut out = String::new();
    for (i, a) in w.raw_ref().archetypes().iter().enumerate() {
        if !a.is_empty() {
            out += &format!("arch {i} {:?}: {:?}\n", a.component_ids(), a.entities());
        }
    }
    for &e in all {
        out += &format!(
            "{e:?} alive={} pos={:?} num={:?} mark={:?} name={:?}\n",
            w.is_alive(e),
            w.get::<Pos>(e),
            w.get::<Num>(e),
            w.get::<Mark>(e),
            w.get::<Name>(e)
        );
    }
    out
}

/// A world of three layouts, interleaved, and some dead handles; the same
/// handles in every call.
fn mixed(track: bool) -> (World, Vec<Entity>) {
    let mut w = World::new();
    if track {
        w.track_changes::<Mark>();
        w.track_changes::<Num>();
    }
    let mut all = Vec::new();
    for i in 0..60u32 {
        let e = match i % 3 {
            0 => w.spawn((Pos(i as f32, 0.0),)),
            1 => w.spawn((Pos(i as f32, 1.0), Num(i))),
            _ => w.spawn((Num(i), Mark(i as u8))),
        };
        all.push(e);
    }
    for &e in all.iter().step_by(7) {
        w.despawn(e);
    }
    (w, all)
}

/// Runs `batch` on one world and `single` on a copy, then compares them.
fn same_as_single(track: bool, batch: impl FnOnce(&mut World, &[Entity]), single: impl FnOnce(&mut World, &[Entity])) {
    let (mut a, all) = mixed(track);
    let (mut b, _) = mixed(track);
    batch(&mut a, &all);
    single(&mut b, &all);
    assert_eq!(fingerprint(&mut a, &all), fingerprint(&mut b, &all));
}

#[test]
fn insert_many_matches_inserting_one_by_one() {
    same_as_single(
        false,
        |w, all| {
            assert_eq!(
                w.insert_many(all, Mark(9)),
                all.iter().filter(|&&e| w.is_alive(e)).count()
            )
        },
        |w, all| all.iter().for_each(|&e| _ = w.insert(e, Mark(9))),
    );
}

#[test]
fn remove_many_matches_removing_one_by_one() {
    same_as_single(
        false,
        |w, all| _ = w.remove_many::<Num>(all),
        |w, all| all.iter().for_each(|&e| _ = w.remove::<Num>(e)),
    );
}

#[test]
fn shuffled_and_repeated_handles_match_single_moves() {
    // Out of row order, with duplicates: the entity-by-entity path.
    let order = |all: &[Entity]| {
        let mut v: Vec<Entity> = all.iter().rev().copied().collect();
        v.extend_from_slice(&all[10..20]);
        v.extend_from_slice(&all[..5]);
        v
    };
    same_as_single(
        false,
        |w, all| _ = w.insert_many(&order(all), Name("x".into())),
        |w, all| order(all).iter().for_each(|&e| _ = w.insert(e, Name("x".into()))),
    );
    same_as_single(
        false,
        |w, all| _ = w.remove_many::<Pos>(&order(all)),
        |w, all| order(all).iter().for_each(|&e| _ = w.remove::<Pos>(e)),
    );
}

#[test]
fn a_whole_archetype_in_row_order_moves_in_one_piece_with_the_same_rows() {
    let setup = || {
        let mut w = World::new();
        let es: Vec<Entity> = (0..1000).map(|i| w.spawn((Pos(i as f32, 0.0), Num(i)))).collect();
        // Some already in the destination, so the copy lands after them.
        let early = w.spawn((Pos(-1.0, 0.0), Num(7), Mark(1)));
        (w, es, early)
    };
    let (mut a, es, early) = setup();
    let (mut b, _, _) = setup();
    let mut all = es.clone();
    all.push(early);
    assert_eq!(a.insert_many(&es, Mark(3)), 1000);
    es.iter().for_each(|&e| _ = b.insert(e, Mark(3)));
    assert_eq!(fingerprint(&mut a, &all), fingerprint(&mut b, &all));

    // And back.
    let mut order: Vec<Entity> = Vec::new();
    for arch in a.raw_ref().archetypes() {
        if arch.component_ids().len() == 3 {
            order.extend_from_slice(arch.entities());
        }
    }
    assert_eq!(a.remove_many::<Mark>(&order), 1001);
    order.iter().for_each(|&e| _ = b.remove::<Mark>(e));
    assert_eq!(fingerprint(&mut a, &all), fingerprint(&mut b, &all));
    assert_eq!(a.query::<&Mark>().iter().count(), 0);
}

#[test]
fn change_ticks_and_removal_logs_match_single_moves() {
    let check = |batch: bool| {
        let (mut w, all) = mixed(true);
        let since = w.advance_change_tick();
        if batch {
            w.insert_many(&all, Mark(5));
            w.remove_many::<Num>(&all);
        } else {
            all.iter().for_each(|&e| _ = w.insert(e, Mark(5)));
            all.iter().for_each(|&e| _ = w.remove::<Num>(e));
        }
        let mut added: Vec<Entity> = w
            .query::<Entity>()
            .filter::<Added<Mark>>()
            .since(since)
            .iter()
            .collect();
        let mut changed: Vec<Entity> = w
            .query::<Entity>()
            .filter::<Changed<Mark>>()
            .since(since)
            .iter()
            .collect();
        added.sort_by_key(|e| e.to_bits());
        changed.sort_by_key(|e| e.to_bits());
        (added, changed, w.removed::<Num>().to_vec(), fingerprint(&mut w, &all))
    };
    let (batch, single) = (check(true), check(false));
    assert!(!batch.0.is_empty() && !batch.2.is_empty());
    assert_eq!(batch, single);
}

#[test]
fn entities_that_already_have_the_component_get_it_replaced() {
    let mut w = World::new();
    let a = w.spawn((Num(1), Mark(1)));
    let b = w.spawn((Num(2),));
    assert_eq!(w.insert_many(&[a, b], Mark(7)), 2);
    assert_eq!(w.get::<Mark>(a), Some(&Mark(7)));
    assert_eq!(w.get::<Mark>(b), Some(&Mark(7)));
}

#[test]
fn dead_entities_and_missing_components_are_skipped() {
    let mut w = World::new();
    let a = w.spawn((Num(1),));
    let b = w.spawn((Pos(0.0, 0.0),));
    w.despawn(a);
    assert_eq!(w.insert_many(&[a, b], Mark(0)), 1);
    assert_eq!(w.remove_many::<Num>(&[a, b]), 0);
    assert_eq!(w.remove_many::<Name>(&[b]), 0); // never registered
    assert_eq!(w.remove_many::<Mark>(&[a, b, b]), 1);
    assert_eq!(w.insert_many::<Mark>(&[], Mark(0)), 0);
}

/// Counts its drops, and panics in `clone` once `CLONES_LEFT` runs out.
#[derive(Component)]
struct Counted(Arc<AtomicUsize>, Arc<AtomicUsize>);

impl Clone for Counted {
    fn clone(&self) -> Self {
        if self.1.fetch_sub(1, Ordering::SeqCst) == 0 {
            panic!("clone budget spent");
        }
        Counted(self.0.clone(), self.1.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn removed_values_are_dropped_once() {
    let drops = Arc::new(AtomicUsize::new(0));
    let budget = Arc::new(AtomicUsize::new(usize::MAX));
    let mut w = World::new();
    let es: Vec<Entity> = (0..100).map(|i| w.spawn((Num(i),))).collect();
    let value = Counted(drops.clone(), budget.clone());
    assert_eq!(w.insert_many(&es, value), 100);
    assert_eq!(drops.load(Ordering::SeqCst), 1, "only the value passed in");
    // Half one by one (out of order), then the rest whole.
    let odd: Vec<Entity> = es.iter().rev().skip(1).step_by(2).copied().collect();
    assert_eq!(w.remove_many::<Counted>(&odd), 50);
    assert_eq!(drops.load(Ordering::SeqCst), 51);
    let rest: Vec<Entity> = w.query::<(Entity, &Counted)>().iter().map(|(e, _)| e).collect();
    assert_eq!(w.remove_many::<Counted>(&rest), 50);
    assert_eq!(drops.load(Ordering::SeqCst), 101);
    drop(w);
    assert_eq!(drops.load(Ordering::SeqCst), 101);
}

#[test]
fn a_clone_that_panics_leaves_the_world_sound() {
    for whole in [true, false] {
        let drops = Arc::new(AtomicUsize::new(0));
        let budget = Arc::new(AtomicUsize::new(10));
        let mut w = World::new();
        let mut es: Vec<Entity> = (0..50).map(|i| w.spawn((Num(i),))).collect();
        if !whole {
            es.reverse();
        }
        let value = Counted(drops.clone(), budget.clone());
        let r = catch_unwind(AssertUnwindSafe(|| w.insert_many(&es, value)));
        assert!(r.is_err());
        // Every entity is whole: it has its `Num`, and a `Counted` only if
        // it got one.
        let with = w.query::<&Counted>().iter().count();
        assert_eq!(w.query::<&Num>().iter().count(), 50);
        assert!(with <= 10);
        assert_eq!(w.raw_ref().archetypes().iter().map(|a| a.len()).sum::<usize>(), 50);
        budget.store(usize::MAX, Ordering::SeqCst);
        drop(w);
        // The world dropped what it held; nothing dropped twice.
        assert!(drops.load(Ordering::SeqCst) <= 11);
        assert!(drops.load(Ordering::SeqCst) > with);
    }
}

fn count_add(w: &mut HookWorld, _: Entity) {
    w.resource_mut::<Hits>().0 += 1;
}
fn count_remove(w: &mut HookWorld, _: Entity) {
    w.resource_mut::<Hits>().1 += 1;
}
#[derive(Default)]
struct Hits(usize, usize);

#[test]
fn hooks_run_once_per_entity() {
    let mut w = World::new();
    w.insert_resource(Hits::default());
    w.on_add::<Mark>(count_add);
    w.on_remove::<Mark>(count_remove);
    let es: Vec<Entity> = (0..20).map(|i| w.spawn((Num(i),))).collect();
    assert_eq!(w.insert_many(&es, Mark(1)), 20);
    assert_eq!(w.remove_many::<Mark>(&es[..5]), 5);
    let hits = w.resource::<Hits>();
    assert_eq!((hits.0, hits.1), (20, 5));
}

#[test]
fn required_components_arrive_with_the_batch() {
    let mut w = World::new();
    w.require::<Mark, Name>(|| Name("required".into()));
    let es: Vec<Entity> = (0..10).map(|i| w.spawn((Num(i),))).collect();
    assert_eq!(w.insert_many(&es, Mark(2)), 10);
    for &e in &es {
        assert_eq!(w.get::<Name>(e), Some(&Name("required".into())));
    }
}

#[test]
fn emptied_archetypes_can_be_reclaimed_mid_batch() {
    // Many small archetypes, each emptied by the batch in turn.
    #[derive(Debug, PartialEq, Clone, Copy, Component)]
    struct A;
    #[derive(Debug, PartialEq, Clone, Copy, Component)]
    struct B;
    #[derive(Debug, PartialEq, Clone, Copy, Component)]
    struct C;
    let setup = || {
        let mut w = World::new();
        let mut es = Vec::new();
        for i in 0..64u32 {
            let e = w.spawn((Num(i),));
            if i & 1 != 0 {
                w.insert(e, A);
            }
            if i & 2 != 0 {
                w.insert(e, B);
            }
            if i & 4 != 0 {
                w.insert(e, C);
            }
            es.push(e);
        }
        (w, es)
    };
    let (mut a, es) = setup();
    let (mut b, _) = setup();
    for round in 0..4u8 {
        a.insert_many(&es, Mark(round));
        a.remove_many::<Mark>(&es);
        a.remove_many::<A>(&es);
        es.iter().for_each(|&e| {
            b.insert(e, Mark(round));
        });
        es.iter().for_each(|&e| _ = b.remove::<Mark>(e));
        es.iter().for_each(|&e| _ = b.remove::<A>(e));
        assert_eq!(fingerprint(&mut a, &es), fingerprint(&mut b, &es));
    }
}
