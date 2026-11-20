//! Read-only queries on a shared world (`World::query_ref`): the same
//! entities as `World::query`, nothing registered, nothing counted as
//! changed, and several at once.

mod common;

use common::{Frozen, Pos, Rng, Tag, Vel};
use ponko_ecs::*;

#[derive(Debug, PartialEq, Component)]
struct Health(i32);
#[derive(Component)]
struct NeverSpawned;

fn world() -> World {
    let mut w = World::new();
    w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0)));
    w.spawn((Pos(1.0, 0.0), Tag));
    w.spawn((Pos(2.0, 0.0), Vel(2.0, 0.0), Frozen));
    w.spawn((Vel(3.0, 0.0),));
    w
}

#[test]
fn read_only_queries_run_on_a_shared_world() {
    let w = world();
    let shared: &World = &w;
    let xs: Vec<f32> = shared.query_ref::<&Pos>().iter().map(|p| p.0).collect();
    assert_eq!(xs, [0.0, 1.0, 2.0]);
    assert_eq!(shared.query_ref::<(&Pos, &Vel)>().count(), 2);
    let mut sum = 0.0;
    shared.query_ref::<&Vel>().for_each(|v| sum += v.0);
    assert_eq!(sum, 6.0);
}

#[test]
fn read_only_queries_visit_what_a_world_query_visits_in_the_same_order() {
    let mut rng = Rng::new(7);
    let mut w = World::new();
    let mut alive = Vec::new();
    for i in 0..500 {
        let x = i as f32;
        let e = match rng.below(4) {
            0 => w.spawn((Pos(x, 0.0),)),
            1 => w.spawn((Pos(x, 1.0), Vel(x, 0.0))),
            2 => w.spawn((Pos(x, 2.0), Tag)),
            _ => w.spawn((Vel(x, 3.0), Frozen)),
        };
        alive.push(e);
        if rng.one_in(5) {
            let gone = alive.swap_remove(rng.below(alive.len()));
            w.despawn(gone);
        }
    }
    let by_ref: Vec<(Entity, Pos, Option<Vel>)> = w
        .query_ref::<(Entity, &Pos, Option<&Vel>)>()
        .iter()
        .map(|(e, p, v)| (e, *p, v.copied()))
        .collect();
    let by_mut: Vec<(Entity, Pos, Option<Vel>)> = w
        .query::<(Entity, &Pos, Option<&Vel>)>()
        .iter()
        .map(|(e, p, v)| (e, *p, v.copied()))
        .collect();
    assert_eq!(by_ref, by_mut);
    assert!(!by_ref.is_empty());
}

#[test]
fn several_read_only_queries_are_held_at_once() {
    let w = world();
    let positions = w.query_ref::<(Entity, &Pos)>();
    let velocities = w.query_ref::<&Vel>();
    // A query looping over itself, with another looked up inside.
    let mut pairs = 0;
    for (a, pa) in positions.iter() {
        for (b, pb) in positions.iter() {
            if a < b && (pa.0 - pb.0).abs() <= 1.0 {
                pairs += 1;
                assert_eq!(velocities.get(a).is_some(), pa.0 != 1.0);
            }
        }
    }
    assert_eq!(pairs, 2);
    // Items live as long as the world's borrow, not the query's.
    let kept: Vec<&Pos> = w.query_ref::<&Pos>().iter().collect();
    assert_eq!(kept.len(), 3);
}

#[test]
fn an_unknown_component_matches_nothing_and_reads_as_none() {
    let w = world();
    assert_eq!(w.query_ref::<&NeverSpawned>().count(), 0);
    assert_eq!(w.query_ref::<(&Pos, &NeverSpawned)>().iter().count(), 0);
    let all: Vec<bool> = w
        .query_ref::<(&Pos, Option<&NeverSpawned>)>()
        .iter()
        .map(|(_, n)| n.is_none())
        .collect();
    assert_eq!(all, [true, true, true]);
    assert!(w.component_id::<NeverSpawned>().is_none(), "nothing was registered");
    // Filters on it: `With` matches nothing, `Without` everything.
    assert_eq!(w.query_ref::<&Pos>().filter::<With<NeverSpawned>>().count(), 0);
    assert_eq!(w.query_ref::<&Pos>().filter::<Without<NeverSpawned>>().count(), 3);
}

#[test]
fn read_only_queries_take_the_filters_of_a_world_query() {
    let w = world();
    assert_eq!(w.query_ref::<&Pos>().filter::<With<Vel>>().count(), 2);
    assert_eq!(
        w.query_ref::<&Pos>().filter::<(With<Vel>, Without<Frozen>)>().count(),
        1
    );
    assert_eq!(
        w.query_ref::<&Pos>().filter::<Or<(With<Tag>, With<Frozen>)>>().count(),
        2
    );
}

#[test]
fn read_only_change_filters_see_tracked_changes_and_make_none() {
    let mut w = World::new();
    w.track_changes::<Health>();
    let calm = w.spawn((Health(10),));
    let since = w.advance_change_tick();
    let hurt = w.spawn((Health(10),));
    w.get_mut::<Health>(calm).unwrap().0 = 5;
    w.set_changes_since(since);

    let changed: Vec<Entity> = w.query_ref::<Entity>().filter::<Changed<Health>>().iter().collect();
    assert_eq!(changed, [calm, hurt]);
    let added: Vec<Entity> = w.query_ref::<Entity>().filter::<Added<Health>>().iter().collect();
    assert_eq!(added, [hurt]);
    // Reading isn't a change: a later window sees nothing.
    let later = w.advance_change_tick();
    w.query_ref::<&Health>().for_each(|h| assert!(h.0 > 0));
    assert_eq!(
        w.query_ref::<Entity>().filter::<Changed<Health>>().since(later).count(),
        0
    );
}

#[test]
#[should_panic(expected = "`Changed<query_ref::NeverSpawned>` needs `query_ref::NeverSpawned`'s changes tracked")]
fn a_change_filter_on_an_unknown_component_names_the_fix() {
    let w = world();
    let _ = w.query_ref::<&Pos>().filter::<Changed<NeverSpawned>>();
}

#[test]
#[should_panic(expected = "World::query_ref: `Added<query_ref::common::Pos>` needs")]
fn a_change_filter_on_an_untracked_component_names_the_fix() {
    let w = world();
    let _ = w.query_ref::<&Vel>().filter::<Added<Pos>>();
}

#[test]
fn single_and_get_answer_from_a_shared_world() {
    let w = world();
    let (e, p) = w.query_ref::<(Entity, &Pos)>().filter::<With<Tag>>().single().unwrap();
    assert_eq!(p, &Pos(1.0, 0.0));
    assert_eq!(w.query_ref::<&Pos>().get(e), Some(&Pos(1.0, 0.0)));
    assert_eq!(w.query_ref::<&Vel>().get(e), None, "it has no Vel");
    assert_eq!(w.query_ref::<&Pos>().single(), Err(SingleError::MultipleEntities));
    assert_eq!(w.query_ref::<&Health>().single(), Err(SingleError::NoEntities));
}

#[test]
fn read_only_queries_use_the_archetype_index_and_agree_with_scans() {
    // Over 64 archetypes, so the world indexes repeated signatures.
    #[derive(Component)]
    struct Kind<const N: usize>;
    let mut w = World::new();
    macro_rules! kinds {
        ($($n:literal)*) => {$(
            w.spawn((Pos($n as f32, 0.0), Kind::<$n>));
            w.spawn((Pos($n as f32, 1.0), Vel(0.0, 0.0), Kind::<$n>));
        )*};
    }
    kinds!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39);
    w.query::<&Vel>().cached().count(); // indexed while held exclusively
    let scanned = w.query_ref::<&Vel>().uncached().count();
    let indexed = w.query_ref::<&Vel>().cached().count();
    assert_eq!((scanned, indexed), (40, 40));
    // A shared query asks for a signature the index lacks; it still scans right.
    assert_eq!(w.query_ref::<(&Pos, &Vel)>().cached().count(), 40);
    assert_eq!(w.query::<(&Pos, &Vel)>().count(), 40);
    assert_eq!(w.query_ref::<(&Pos, &Vel)>().count(), 40);
}

#[test]
fn read_only_queries_share_a_world_across_threads() {
    let w = world();
    let shared = &w;
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| shared.query_ref::<&Pos>().iter().map(|p| p.0).sum::<f32>());
        let b = s.spawn(|| shared.query_ref::<&Pos>().iter().map(|p| p.0).sum::<f32>());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!((a, b), (3.0, 3.0));
}
