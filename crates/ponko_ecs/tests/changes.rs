//! Change detection: `Added`, `Changed`, removals and ticks, for every way a
//! component can be written.

mod common;

use common::*;
use ponko_ecs::*;

#[derive(Debug, PartialEq, Clone, Copy, Component)]
struct Health(i32);
#[derive(Debug, PartialEq, Clone, Copy, Component)]
struct Armor(i32);
#[derive(Component)]
struct Player;

/// Entities whose `T` changed after `since`, in handle order.
fn changed<T: Component>(w: &mut World, since: Tick) -> Vec<Entity> {
    let mut out: Vec<Entity> = w.query::<Entity>().filter::<Changed<T>>().since(since).iter().collect();
    out.sort_by_key(|e| e.to_bits());
    out
}

/// Entities whose `T` was added after `since`, in handle order.
fn added<T: Component>(w: &mut World, since: Tick) -> Vec<Entity> {
    let mut out: Vec<Entity> = w.query::<Entity>().filter::<Added<T>>().since(since).iter().collect();
    out.sort_by_key(|e| e.to_bits());
    out
}

fn sorted(mut entities: Vec<Entity>) -> Vec<Entity> {
    entities.sort_by_key(|e| e.to_bits());
    entities
}

/// A world tracking `Health` with three entities, and the tick after which
/// nothing has happened yet.
fn three() -> (World, [Entity; 3], Tick) {
    let mut w = World::new();
    w.track_changes::<Health>();
    let es = [1, 2, 3].map(|hp| w.spawn((Health(hp),)));
    let seen = w.advance_change_tick();
    (w, es, seen)
}

// ---- opting in ---------------------------------------------------------------

#[test]
fn nothing_is_tracked_until_asked() {
    let mut w = World::new();
    let e = w.spawn((Health(1), Armor(2)));
    w.get_mut::<Health>(e).unwrap().0 = 5;
    assert!(w.components().iter().all(|info| !info.tracks_changes()));
    let row_bytes = |w: &World| {
        w.raw_ref()
            .archetypes()
            .iter()
            .find(|a| a.len() == 1)
            .unwrap()
            .bytes_per_entity()
    };
    let (bytes, row) = (w.allocated_bytes(), row_bytes(&w));
    let health = w.track_changes::<Health>();
    assert!(w.component_info(health).tracks_changes());
    assert!(!w.component_info(w.component_id::<Armor>().unwrap()).tracks_changes());
    // Tracking costs two 4-byte stamps per row of the tracked component and
    // a 4-byte summary per 64 rows (one block here), and nothing for the
    // others.
    assert_eq!(row_bytes(&w), row + 8);
    let stamped = w.allocated_bytes() - 4;
    assert_eq!(stamped / (row + 8), bytes / row, "the same capacity");
    assert_eq!(stamped % (row + 8), 0);
    assert_eq!(w.track_changes::<Health>(), health, "tracking twice is harmless");
}

#[test]
fn values_that_exist_when_tracking_starts_count_as_added_and_changed() {
    let mut w = World::new();
    let a = w.spawn((Health(1),));
    let b = w.spawn((Health(2), Armor(0)));
    let before = w.advance_change_tick();
    w.track_changes::<Health>();
    assert_eq!(changed::<Health>(&mut w, before), [a, b]);
    assert_eq!(added::<Health>(&mut w, before), [a, b]);
    let after = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, after), []);
}

#[test]
#[should_panic(expected = "`Changed<changes::Health>` needs `changes::Health`'s changes tracked")]
fn a_change_filter_on_an_untracked_component_panics() {
    let mut w = World::new();
    w.spawn((Health(1),));
    // Tracking is explicit: the filter doesn't start it.
    let _ = w.query::<Entity>().filter::<Changed<Health>>().count();
}

#[test]
#[should_panic(expected = "`Added<changes::Armor>` needs `changes::Armor`'s changes tracked")]
fn a_change_filter_inside_an_or_needs_tracking_too() {
    let mut w = World::new();
    w.spawn((Armor(1),));
    let _ = w.query::<Entity>().filter::<Or<(Added<Armor>, With<Player>)>>().count();
}

#[test]
fn a_refused_change_filter_leaves_the_component_untracked() {
    let mut w = World::new();
    let e = w.spawn((Health(1),));
    let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = w.query::<Entity>().filter::<Changed<Health>>().count();
    }));
    assert!(refused.is_err());
    assert!(!w.component_info(w.component_id::<Health>().unwrap()).tracks_changes());
    // Asked for, it is tracked from then on: what exists counts as changed.
    w.track_changes::<Health>();
    assert_eq!(w.query::<Entity>().filter::<Changed<Health>>().count(), 1);
    let seen = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, seen), []);
    w.get_mut::<Health>(e).unwrap().0 = 2;
    assert_eq!(changed::<Health>(&mut w, seen), [e]);
}

#[test]
fn tracking_keeps_every_existing_query_result() {
    // The same operations on a tracked and an untracked world give the same
    // values, counts and order through every way to run a query.
    let build = |track: bool| {
        let mut w = World::new();
        if track {
            w.track_changes::<Health>();
            w.track_changes::<Armor>();
        }
        let mut es = Vec::new();
        for i in 0..40 {
            es.push(match i % 3 {
                0 => w.spawn((Health(i),)),
                1 => w.spawn((Health(i), Armor(i))),
                _ => w.spawn((Health(i), Armor(i), Player)),
            });
        }
        for e in es.iter().step_by(4) {
            w.despawn(*e);
        }
        for e in es.iter().skip(1).step_by(5) {
            w.insert(*e, Player);
            w.remove::<Armor>(*e);
        }
        w.sort_by::<Health>(|h| (100 - h.0) as u64);
        w.query::<(&mut Health, &Armor)>().for_each(|(h, a)| h.0 += a.0);
        w
    };
    let (mut plain, mut tracked) = (build(false), build(true));
    for w in [&mut plain, &mut tracked] {
        w.advance_change_tick();
    }
    let seen = |w: &mut World| {
        let mut for_each = Vec::new();
        w.query::<(Entity, &Health, Option<&Armor>)>()
            .for_each(|(e, h, a)| for_each.push((e, *h, a.copied())));
        let iter: Vec<_> = w
            .query::<(Entity, &Health, Option<&Armor>)>()
            .iter()
            .map(|(e, h, a)| (e, *h, a.copied()))
            .collect();
        assert_eq!(for_each, iter);
        let players = w.query::<&Health>().filter::<With<Player>>().count();
        let unarmored = w.query::<&Health>().filter::<Without<Armor>>().count();
        let one = for_each[3].0;
        let got = w.query::<&mut Health>().get(one).map(|h| *h);
        (for_each, players, unarmored, got)
    };
    assert_eq!(seen(&mut plain), seen(&mut tracked));
}

// ---- what counts as added or changed ------------------------------------------

#[test]
fn spawning_counts_as_added_and_changed() {
    let (mut w, _, seen) = three();
    let one = w.spawn((Health(4),));
    w.spawn_batch((5..8).map(|hp| (Health(hp), Armor(0))));
    let at = Entity::from_bits(100);
    assert!(w.spawn_at(at, (Health(9),)));
    let new: Vec<Entity> = w
        .query::<(Entity, &Health)>()
        .iter()
        .filter(|(_, h)| h.0 >= 4)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(new.len(), 5);
    assert!(new.contains(&one) && new.contains(&at));
    assert_eq!(added::<Health>(&mut w, seen), sorted(new.clone()));
    assert_eq!(changed::<Health>(&mut w, seen), sorted(new));
}

#[test]
fn inserting_a_missing_component_counts_as_added_and_changed() {
    let mut w = World::new();
    w.track_changes::<Armor>();
    let a = w.spawn((Health(1),));
    let b = w.spawn((Health(2),));
    let seen = w.advance_change_tick();
    assert!(w.insert(b, Armor(5)));
    assert_eq!(added::<Armor>(&mut w, seen), [b]);
    assert_eq!(changed::<Armor>(&mut w, seen), [b]);
    let _ = a;
}

#[test]
fn replacing_a_value_counts_as_changed_but_not_added() {
    let (mut w, [a, b, _], seen) = three();
    assert!(w.insert(b, Health(20)));
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
    assert_eq!(added::<Health>(&mut w, seen), []);
    assert_eq!(w.removed_since::<Health>(seen), [], "replacing isn't removing");
    let _ = a;
}

#[test]
fn get_mut_counts_as_changed_and_get_does_not() {
    let (mut w, [a, b, c], seen) = three();
    assert_eq!(w.get::<Health>(a), Some(&Health(1)));
    assert!(w.has::<Health>(b));
    // A mutable borrow counts even if nothing is written through it.
    let _ = w.get_mut::<Health>(c);
    assert_eq!(changed::<Health>(&mut w, seen), [c]);
    assert_eq!(added::<Health>(&mut w, seen), []);
}

#[test]
fn a_mutable_query_marks_every_entity_it_visits() {
    let mut w = World::new();
    w.track_changes::<Health>();
    let plain = w.spawn((Health(1),));
    let armored = w.spawn((Health(2), Armor(1)));
    let player = w.spawn((Health(3), Armor(1), Player));
    let seen = w.advance_change_tick();

    // Reading marks nothing.
    w.query::<(&Health, Option<&Armor>)>().for_each(|_| {});
    assert_eq!(w.query::<&Health>().count(), 3);
    assert_eq!(changed::<Health>(&mut w, seen), []);

    // Writing marks what the query matches, written or not; nothing else.
    w.query::<(&mut Health, &Armor)>().for_each(|_| {});
    assert_eq!(changed::<Health>(&mut w, seen), [armored, player]);

    let seen = w.advance_change_tick();
    w.query::<&mut Health>().filter::<With<Player>>().for_each(|h| h.0 += 1);
    assert_eq!(changed::<Health>(&mut w, seen), [player]);

    let seen = w.advance_change_tick();
    w.query::<&mut Health>()
        .filter::<Without<Armor>>()
        .for_each(|h| h.0 += 1);
    assert_eq!(changed::<Health>(&mut w, seen), [plain]);
    assert_eq!(added::<Health>(&mut w, seen), []);
}

#[test]
fn an_optional_mutable_fetch_marks_the_entities_that_have_the_component() {
    let mut w = World::new();
    w.track_changes::<Armor>();
    let bare = w.spawn((Health(1),));
    let armored = w.spawn((Health(2), Armor(1)));
    let seen = w.advance_change_tick();
    let mut visited = 0;
    w.query::<(&Health, Option<&mut Armor>)>().for_each(|_| visited += 1);
    assert_eq!(visited, 2);
    assert_eq!(changed::<Armor>(&mut w, seen), [armored]);
    let _ = bare;
}

#[test]
fn only_the_queried_component_is_marked() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    let e = w.spawn((Health(1), Armor(1)));
    let seen = w.advance_change_tick();
    w.query::<(&mut Health, &Armor)>().for_each(|(h, a)| h.0 += a.0);
    assert_eq!(changed::<Health>(&mut w, seen), [e]);
    assert_eq!(changed::<Armor>(&mut w, seen), []);
}

#[test]
fn an_iterator_marks_the_archetypes_it_reached() {
    let mut w = World::new();
    w.track_changes::<Health>();
    let first = [w.spawn((Health(1),)), w.spawn((Health(2),))];
    let second = w.spawn((Health(3), Armor(0)));
    let seen = w.advance_change_tick();
    // Stops inside the first archetype: all of it was borrowed, the second wasn't.
    assert!(w.query::<&mut Health>().iter().next().is_some());
    assert_eq!(changed::<Health>(&mut w, seen), first);
    let seen = w.advance_change_tick();
    assert_eq!(w.query::<&mut Health>().iter().count(), 3);
    assert_eq!(changed::<Health>(&mut w, seen), [first[0], first[1], second]);
}

#[test]
fn query_get_marks_one_entity_and_count_marks_none() {
    let (mut w, [a, b, c], seen) = three();
    assert_eq!(w.query::<&mut Health>().count(), 3);
    assert_eq!(changed::<Health>(&mut w, seen), []);
    assert!(w.query::<&Health>().get(a).is_some());
    assert_eq!(changed::<Health>(&mut w, seen), []);
    w.query::<&mut Health>().get(b).unwrap().0 = 9;
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
    // A handle the query doesn't match marks nothing.
    assert!(w.query::<&mut Health>().filter::<With<Player>>().get(c).is_none());
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
}

#[test]
fn mark_changed_reports_a_write_the_world_cannot_see() {
    let (mut w, [a, b, _], seen) = three();
    assert!(w.mark_changed::<Health>(b));
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
    assert_eq!(added::<Health>(&mut w, seen), []);
    assert!(!w.mark_changed::<Armor>(a), "no such component on the entity");
    w.despawn(a);
    assert!(!w.mark_changed::<Health>(a), "dead entity");
    // Untracked components accept the call and do nothing.
    let e = w.spawn((Armor(1),));
    assert!(w.mark_changed::<Armor>(e));
    assert!(!w.component_info(w.component_id::<Armor>().unwrap()).tracks_changes());
}

// ---- structural changes --------------------------------------------------------

#[test]
fn moving_between_archetypes_keeps_the_stamps_of_kept_components() {
    let (mut w, [a, b, c], seen) = three();
    // `a` moves out (the last row fills its place), then back.
    assert!(w.insert(a, Armor(1)));
    assert!(w.insert(a, Player));
    assert_eq!(w.remove::<Armor>(a), Some(Armor(1)));
    assert_eq!(changed::<Health>(&mut w, seen), [], "Health itself was never touched");
    assert_eq!(added::<Health>(&mut w, seen), []);
    // The rows that were shuffled around still carry their own stamps.
    w.get_mut::<Health>(c).unwrap().0 = 30;
    assert_eq!(changed::<Health>(&mut w, seen), [c]);
    let seen = w.advance_change_tick();
    w.get_mut::<Health>(a).unwrap().0 = 10;
    assert!(w.insert(a, Armor(2)));
    assert!(w.insert(c, Armor(2)));
    assert_eq!(changed::<Health>(&mut w, seen), [a]);
    let _ = b;
}

#[test]
fn removing_logs_the_entity_and_inserting_again_counts_as_added() {
    let (mut w, [a, b, c], seen) = three();
    assert_eq!(w.remove::<Health>(b), Some(Health(2)));
    assert_eq!(w.removed_since::<Health>(seen), [b]);
    assert_eq!(changed::<Health>(&mut w, seen), []);
    let later = w.advance_change_tick();
    assert!(w.insert(b, Health(7)));
    assert_eq!(added::<Health>(&mut w, later), [b]);
    assert_eq!(changed::<Health>(&mut w, later), [b]);
    assert_eq!(
        w.removed_since::<Health>(later),
        [],
        "seen removals aren't reported again"
    );
    // Removed twice: listed twice, oldest first.
    assert_eq!(w.remove::<Health>(b), Some(Health(7)));
    assert_eq!(w.removed_since::<Health>(seen), [b, b]);
    let _ = (a, c);
}

#[test]
fn despawning_logs_a_removal_for_each_tracked_component() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    let a = w.spawn((Health(1), Armor(1), Player));
    let b = w.spawn((Health(2),));
    let c = w.spawn((Health(3), Armor(3)));
    let seen = w.advance_change_tick();
    assert!(w.despawn(a));
    assert!(w.despawn(b));
    assert!(!w.despawn(b), "a second despawn does nothing and logs nothing");
    assert_eq!(w.removed_since::<Health>(seen), [a, b]);
    assert_eq!(w.removed_since::<Armor>(seen), [a]);
    // The survivor's row moved; its stamps moved with it.
    assert_eq!(changed::<Health>(&mut w, seen), []);
    w.get_mut::<Armor>(c).unwrap().0 = 4;
    assert_eq!(changed::<Armor>(&mut w, seen), [c]);
}

/// Despawns move the last row's stamps into the freed row, across the
/// 64-row blocks change filters skip by: changed rows that move stay
/// visible, still rows that move stay quiet, in every block.
#[test]
fn despawns_across_many_blocks_keep_change_filters_exact() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    // Under Miri: five blocks of 64 and fewer rounds, still crossing blocks.
    let (n, rounds) = if cfg!(miri) { (320, 6) } else { (1000, 20) };
    let mut live: Vec<Entity> = (0..n).map(|i| w.spawn((Health(i), Armor(i)))).collect();
    let mut rng = Rng::new(136);
    for round in 0..rounds {
        let seen = w.advance_change_tick();
        let mut health = Vec::new();
        let mut armor = Vec::new();
        for _ in 0..10 {
            let e = live[rng.below(live.len())];
            if !health.contains(&e) {
                w.get_mut::<Health>(e).unwrap().0 += 1;
                health.push(e);
            }
        }
        for _ in 0..3 {
            let e = live[rng.below(live.len())];
            if !armor.contains(&e) {
                assert!(w.mark_changed::<Armor>(e));
                armor.push(e);
            }
        }
        // Despawn some, changed or not; the last rows move into their places.
        for _ in 0..25 {
            let e = live.swap_remove(rng.below(live.len()));
            assert!(w.despawn(e));
            health.retain(|&h| h != e);
            armor.retain(|&a| a != e);
        }
        assert_eq!(changed::<Health>(&mut w, seen), sorted(health), "round {round}");
        assert_eq!(changed::<Armor>(&mut w, seen), sorted(armor), "round {round}");
        assert_eq!(added::<Health>(&mut w, seen), [], "round {round}");
    }
    assert_eq!(w.len(), (n - 25 * rounds) as usize);
}

#[test]
fn a_reused_entity_slot_starts_with_fresh_stamps() {
    let (mut w, [a, _, _], _) = three();
    w.despawn(a);
    let seen = w.advance_change_tick();
    let again = w.spawn((Health(1),));
    assert_ne!(again, a);
    assert_eq!(added::<Health>(&mut w, seen), [again]);
    let later = w.advance_change_tick();
    assert_eq!(added::<Health>(&mut w, later), []);
    assert_eq!(changed::<Health>(&mut w, later), []);
}

#[test]
fn clear_and_reset_log_removals_and_restored_entities_count_as_added() {
    let (mut w, [a, b, c], seen) = three();
    w.clear();
    assert_eq!(sorted(w.removed_since::<Health>(seen).to_vec()), [a, b, c]);
    let d = w.spawn((Health(4),));
    let seen = w.advance_change_tick();
    w.reset();
    assert_eq!(w.removed_since::<Health>(seen), [d]);
    // Restoring a save: the old handles come back, as newly added values.
    assert!(w.spawn_at(a, (Health(1),)));
    assert_eq!(added::<Health>(&mut w, seen), [a]);
    assert_eq!(changed::<Health>(&mut w, seen), [a]);
}

#[test]
fn sorting_moves_stamps_with_their_rows() {
    let mut w = World::new();
    w.track_changes::<Health>();
    let es: Vec<Entity> = (0..20).map(|hp| w.spawn((Health(hp),))).collect();
    let seen = w.advance_change_tick();
    for &e in es.iter().step_by(3) {
        w.get_mut::<Health>(e).unwrap().0 += 100;
    }
    let expected: Vec<Entity> = es.iter().copied().step_by(3).collect();
    // Odd values first, so every row moves.
    w.sort_by::<Health>(|h| ((h.0 % 2 == 0) as u64) << 32 | (200 - h.0) as u64);
    assert_ne!(
        w.query::<Entity>().iter().collect::<Vec<_>>(),
        es,
        "the sort moved rows"
    );
    assert_eq!(changed::<Health>(&mut w, seen), expected, "sorting isn't a change");
    assert_eq!(added::<Health>(&mut w, seen), []);
}

/// One byte, alignment 1: its stamps need a stricter alignment than it does.
#[derive(Clone, Copy, PartialEq, Debug, Component)]
struct Small(u8);
/// No bytes at all: a tracked column of it holds only stamps.
#[derive(Component)]
struct Marker;

#[test]
fn small_and_zero_sized_components_keep_values_and_stamps_through_growth_and_sorting() {
    // Rows before and after the sort: enough for several reallocations.
    let (first, total) = if cfg!(miri) { (70, 200) } else { (600, 2000) };
    // Tracking starts when rows exist, without capacity, and before the
    // component is first used.
    for start in ["with rows", "empty", "unused"] {
        let mut w = World::new();
        if start == "unused" {
            w.track_changes::<Small>();
            w.track_changes::<Marker>();
        }
        let mut es: Vec<Entity> = Vec::new();
        if start == "with rows" {
            es.extend((0..3).map(|i| w.spawn((Small(i), Marker, Health(i as i32)))));
        }
        w.track_changes::<Small>();
        w.track_changes::<Marker>();
        for (i, &e) in es.iter().enumerate() {
            assert_eq!(
                *w.get::<Small>(e).unwrap(),
                Small(i as u8),
                "{start}: tracking keeps the values"
            );
        }
        let seen = w.advance_change_tick();
        // Growth, many times over; the first rows' stamps stay old.
        es.extend((es.len()..first).map(|i| w.spawn((Small(i as u8), Marker, Health(i as i32)))));
        let old = if start == "with rows" { 3 } else { 0 };
        assert_eq!(added::<Small>(&mut w, seen), sorted(es[old..].to_vec()), "{start}");
        assert_eq!(added::<Marker>(&mut w, seen), sorted(es[old..].to_vec()), "{start}");
        let later = w.advance_change_tick();
        let touched: Vec<Entity> = es.iter().copied().step_by(7).collect();
        for &e in &touched {
            w.get_mut::<Small>(e).unwrap().0 ^= 0x80;
            assert!(w.mark_changed::<Marker>(e));
        }
        // Sorting reorders values and stamps together.
        w.sort_by::<Health>(|h| (1000 - h.0) as u64);
        assert_eq!(changed::<Small>(&mut w, later), sorted(touched.clone()), "{start}");
        assert_eq!(changed::<Marker>(&mut w, later), sorted(touched.clone()), "{start}");
        assert_eq!(added::<Small>(&mut w, later), [], "{start}");
        for (i, &e) in es.iter().enumerate() {
            let flipped = if i % 7 == 0 { 0x80 } else { 0 };
            assert_eq!(
                *w.get::<Small>(e).unwrap(),
                Small(i as u8 ^ flipped),
                "{start}: row {i}"
            );
            assert_eq!(w.get::<Health>(e).unwrap().0, i as i32);
        }
        // More growth after the sort, then removal from the middle.
        es.extend((first..total).map(|i| w.spawn((Small(i as u8), Marker, Health(i as i32)))));
        for &e in es.iter().step_by(3) {
            w.despawn(e);
        }
        // Left: the touched old rows and the new ones, minus every third.
        let left: Vec<Entity> = es
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 3 != 0 && (*i >= first || i % 7 == 0))
            .map(|(_, e)| *e)
            .collect();
        assert_eq!(changed::<Small>(&mut w, later), sorted(left.clone()), "{start}");
        assert_eq!(changed::<Marker>(&mut w, later), sorted(left), "{start}");
    }
}

#[test]
fn stamps_survive_column_growth() {
    let (mut w, [a, b, c], seen) = three();
    w.get_mut::<Health>(b).unwrap().0 = 0;
    let later = w.advance_change_tick();
    // Many reallocations of the column and its stamp arrays.
    let batch = if cfg!(miri) { 400 } else { 5000 };
    w.spawn_batch((0..batch).map(|hp| (Health(hp),)));
    for hp in 0..100 {
        w.spawn((Health(hp),));
    }
    assert_eq!(changed::<Health>(&mut w, seen).len(), batch as usize + 101);
    assert_eq!(changed::<Health>(&mut w, later).len(), batch as usize + 100);
    assert!(changed::<Health>(&mut w, seen).contains(&b));
    assert!(!changed::<Health>(&mut w, later).contains(&b));
    let _ = (a, c);
}

#[test]
fn zero_sized_components_can_be_tracked() {
    let mut w = World::new();
    w.track_changes::<Player>();
    let a = w.spawn((Player,));
    let seen = w.advance_change_tick();
    let b = w.spawn((Health(1),));
    w.insert(b, Player);
    assert_eq!(added::<Player>(&mut w, seen), [b]);
    w.remove::<Player>(a);
    assert_eq!(w.removed_since::<Player>(seen), [a]);
}

// ---- commands -------------------------------------------------------------------

#[test]
fn commands_are_stamped_when_applied() {
    let (mut w, [a, b, c], _) = three();
    let mut commands = w.commands();
    commands.insert(a, Health(10));
    commands.insert(b, Armor(1));
    commands.remove::<Health>(c);
    commands.spawn((Health(4),));
    commands.add(move |w| {
        w.get_mut::<Health>(b).unwrap().0 = 20;
    });
    // Recording changes nothing, and several ticks pass before applying.
    let recorded = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, recorded), []);
    w.advance_change_tick();
    let before_apply = w.advance_change_tick();
    w.apply(&mut commands);
    let spawned = w.query::<(Entity, &Health)>().iter().find(|(_, h)| h.0 == 4).unwrap().0;
    assert_eq!(changed::<Health>(&mut w, before_apply), sorted(vec![a, b, spawned]));
    assert_eq!(added::<Health>(&mut w, before_apply), [spawned]);
    assert_eq!(w.removed_since::<Health>(before_apply), [c]);
    let after = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, after), []);
}

// ---- readers ----------------------------------------------------------------------

#[test]
fn every_reader_sees_each_change_exactly_once_whatever_the_order() {
    // Three "systems" in one loop: a reader before the writer, the writer,
    // a reader after it, and a slow reader that only runs every third step.
    let (mut w, es, _) = three();
    let (mut early, mut late, mut slow) = (Tick::default(), Tick::default(), Tick::default());
    let (mut early_seen, mut late_seen, mut slow_seen) = (Vec::new(), Vec::new(), Vec::new());
    let mut writes = Vec::new();
    for step in 0..9 {
        early_seen.extend(changed::<Health>(&mut w, early));
        early = w.advance_change_tick();

        let target = es[step % 3];
        if step % 2 == 0 {
            w.get_mut::<Health>(target).unwrap().0 += 1;
            writes.push(target);
        }
        w.advance_change_tick();

        late_seen.extend(changed::<Health>(&mut w, late));
        late = w.advance_change_tick();

        if step % 3 == 2 {
            slow_seen.extend(changed::<Health>(&mut w, slow));
            slow = w.advance_change_tick();
        }
    }
    // One more look for the readers that ran before the last write.
    early_seen.extend(changed::<Health>(&mut w, early));
    // The early reader first sees the three spawns, then every write once,
    // each on its next look.
    let expected: Vec<Entity> = es.iter().chain(&writes).copied().collect();
    assert_eq!(sorted(early_seen), sorted(expected));
    // An entity is reported once per look, however often it changed since
    // the last one: the late reader's first look covers the spawns and the
    // first write together, then it sees every later write in the same step.
    let expected: Vec<Entity> = es.iter().chain(&writes[1..]).copied().collect();
    assert_eq!(sorted(late_seen), sorted(expected));
    // The slow reader looks after steps 2, 5 and 8: all three entities, then
    // the two written in steps 3..=5 (only step 4 writes: one), then steps 6..=8.
    assert_eq!(slow_seen.len(), 3 + 1 + 2);
}

#[test]
fn a_reader_that_advances_after_looking_is_not_shown_its_own_changes() {
    let (mut w, [a, b, _], mut seen) = three();
    for _ in 0..3 {
        // Reacts to a change by writing: must not trigger itself forever.
        let hurt = changed::<Health>(&mut w, seen);
        for &e in &hurt {
            w.get_mut::<Health>(e).unwrap().0 -= 1;
        }
        seen = w.advance_change_tick();
        assert_eq!(changed::<Health>(&mut w, seen), []);
    }
    // Someone else's change is seen; the reader's own write to `a` isn't.
    w.get_mut::<Health>(b).unwrap().0 = 0;
    w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
    w.get_mut::<Health>(a).unwrap().0 = 0;
    seen = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, seen), []);
}

#[test]
fn a_reader_that_advances_before_looking_sees_its_own_changes() {
    let (mut w, [a, _, _], _) = three();
    let seen = w.advance_change_tick();
    w.get_mut::<Health>(a).unwrap().0 = 0;
    assert_eq!(changed::<Health>(&mut w, seen), [a]);
}

#[test]
fn the_world_default_tick_drives_filters_and_removals() {
    let (mut w, [a, b, _], seen) = three();
    assert_eq!(w.changes_since(), Tick::default());
    // Before anyone sets it, everything counts.
    assert_eq!(w.query::<Entity>().filter::<Changed<Health>>().count(), 3);
    w.set_changes_since(seen);
    assert_eq!(w.changes_since(), seen);
    assert_eq!(w.query::<Entity>().filter::<Changed<Health>>().count(), 0);
    w.get_mut::<Health>(a).unwrap().0 = 0;
    w.despawn(b);
    assert_eq!(
        w.query::<Entity>()
            .filter::<Changed<Health>>()
            .iter()
            .collect::<Vec<_>>(),
        [a]
    );
    assert_eq!(w.removed::<Health>(), [b]);
    // A query can still name its own tick.
    assert_eq!(
        w.query::<Entity>()
            .filter::<Changed<Health>>()
            .since(Tick::default())
            .count(),
        2
    );
    assert_eq!(Tick::from_bits(seen.to_bits()), seen);
    assert!(w.change_tick() > seen);
}

#[test]
fn forgetting_removals_drops_only_what_every_reader_has_seen() {
    let (mut w, [a, b, c], start) = three();
    w.despawn(a);
    let first = w.advance_change_tick();
    w.despawn(b);
    let second = w.advance_change_tick();
    w.despawn(c);
    w.forget_removed(first);
    assert_eq!(w.removed_since::<Health>(start), [b, c]);
    assert_eq!(w.removed_since::<Health>(first), [b, c]);
    assert_eq!(w.removed_since::<Health>(second), [c]);
    w.forget_removed(w.change_tick());
    assert_eq!(w.removed_since::<Health>(start), []);
}

#[test]
#[should_panic(expected = "aren't tracked; call `world.track_changes::<changes::Armor>()` first")]
fn asking_for_removals_of_an_untracked_component_panics() {
    let mut w = World::new();
    w.spawn((Armor(1),));
    let _ = w.removed::<Armor>();
}

// ---- filters ----------------------------------------------------------------------

#[test]
fn a_changed_filter_with_a_mutable_fetch_marks_only_the_rows_it_passes() {
    let (mut w, [a, b, c], seen) = three();
    w.get_mut::<Health>(b).unwrap().0 = 20;
    let mut visited = Vec::new();
    w.query::<(Entity, &mut Health)>()
        .filter::<Changed<Health>>()
        .since(seen)
        .for_each(|(e, h)| {
            h.0 += 1;
            visited.push(e);
        });
    assert_eq!(visited, [b]);
    assert_eq!(w.get::<Health>(b), Some(&Health(21)));
    let later = w.advance_change_tick();
    // Only `b` was handed out, so only `b` was marked by that query.
    assert_eq!(changed::<Health>(&mut w, seen), [b]);
    assert_eq!(changed::<Health>(&mut w, later), []);
    let _ = (a, c);
}

#[test]
fn row_filters_split_an_archetype_into_runs() {
    // Changed rows at the start, in the middle (two in a row) and at the end.
    let mut w = World::new();
    w.track_changes::<Health>();
    let es: Vec<Entity> = (0..12).map(|hp| w.spawn((Health(hp),))).collect();
    let seen = w.advance_change_tick();
    let touched = [0, 4, 5, 9, 11];
    for &i in &touched {
        w.get_mut::<Health>(es[i]).unwrap().0 += 100;
    }
    let expected: Vec<Entity> = touched.iter().map(|&i| es[i]).collect();
    let mut q = w.query::<Entity>().filter::<Changed<Health>>().since(seen);
    assert_eq!(q.count(), 5);
    assert_eq!(q.iter().collect::<Vec<_>>(), expected);
    let mut via_for_each = Vec::new();
    q.for_each(|e| via_for_each.push(e));
    assert_eq!(via_for_each, expected);
    for (i, &e) in es.iter().enumerate() {
        assert_eq!(q.get(e).is_some(), touched.contains(&i));
    }
    // Nothing changed: no rows at all.
    let later = w.advance_change_tick();
    assert_eq!(
        w.query::<Entity>()
            .filter::<Changed<Health>>()
            .since(later)
            .iter()
            .count(),
        0
    );
}

#[test]
fn change_filters_combine_with_the_other_filters() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    let both = w.spawn((Health(1), Armor(1)));
    let health = w.spawn((Health(2),));
    let armor = w.spawn((Armor(3),));
    let player = w.spawn((Health(4), Armor(4), Player));
    let idle = w.spawn((Health(5), Armor(5)));
    let seen = w.advance_change_tick();
    w.get_mut::<Health>(both).unwrap().0 = 0;
    w.get_mut::<Armor>(both).unwrap().0 = 0;
    w.get_mut::<Health>(health).unwrap().0 = 0;
    w.get_mut::<Armor>(armor).unwrap().0 = 0;
    w.get_mut::<Armor>(player).unwrap().0 = 0;
    let fresh = w.spawn((Armor(6),));

    let matches = |q: Query<'_, Entity>| {
        let mut q = q.since(seen);
        let found = sorted(q.iter().collect());
        assert_eq!(q.count(), found.len());
        found
    };
    // Both must hold: a tuple.
    assert_eq!(
        matches(w.query::<Entity>().filter::<(Changed<Health>, Changed<Armor>)>()),
        [both]
    );
    // Either: an `Or`. Entities without one of the components still match on the other.
    assert_eq!(
        matches(w.query::<Entity>().filter::<Or<(Changed<Health>, Changed<Armor>)>>()),
        [both, health, armor, player, fresh]
    );
    // With presence and absence terms.
    assert_eq!(
        matches(w.query::<Entity>().filter::<(Changed<Armor>, Without<Player>)>()),
        [both, armor, fresh]
    );
    assert_eq!(
        matches(w.query::<Entity>().filter::<Changed<Armor>>().filter::<With<Health>>()),
        [both, player]
    );
    // A presence term inside the `Or` passes its whole archetype.
    assert_eq!(
        matches(w.query::<Entity>().filter::<Or<(Changed<Health>, With<Player>)>>()),
        [both, health, player]
    );
    assert_eq!(
        matches(w.query::<Entity>().filter::<Or<(Added<Armor>, Without<Armor>)>>()),
        [health, fresh]
    );
    // `Added` and `Changed` of the same component: added implies changed.
    assert_eq!(
        matches(w.query::<Entity>().filter::<(Added<Armor>, Changed<Armor>)>()),
        [fresh]
    );
    // Nested `Or`s flatten.
    assert_eq!(
        matches(
            w.query::<Entity>()
                .filter::<Or<(Or<(Added<Armor>, Changed<Health>)>, With<Player>)>>()
        ),
        [both, health, player, fresh]
    );
    let _ = idle;
}

#[test]
fn a_changed_filter_also_requires_the_component() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.spawn((Armor(1),));
    let e = w.spawn((Health(1),));
    assert_eq!(
        w.query::<Entity>()
            .filter::<Changed<Health>>()
            .iter()
            .collect::<Vec<_>>(),
        [e]
    );
}

#[test]
fn repeating_a_change_filter_is_harmless() {
    let (mut w, [a, _, _], seen) = three();
    w.get_mut::<Health>(a).unwrap().0 = 0;
    let n = w
        .query::<Entity>()
        .filter::<(Changed<Health>, Changed<Health>)>()
        .filter::<Changed<Health>>()
        .filter::<(Changed<Health>, Changed<Health>, Changed<Health>)>()
        .since(seen)
        .count();
    assert_eq!(n, 1);
}

#[test]
#[should_panic(expected = "at most 4 `Added`/`Changed` filters per query outside `Or`")]
fn too_many_change_filters_panic() {
    #[derive(Component)]
    struct A;
    #[derive(Component)]
    struct B;
    #[derive(Component)]
    struct C;
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    w.track_changes::<A>();
    w.track_changes::<B>();
    w.track_changes::<C>();
    let _ = w
        .query::<Entity>()
        .filter::<(Changed<Health>, Changed<Armor>, Changed<A>, Changed<B>, Changed<C>)>();
}

#[test]
fn cached_queries_filter_rows_too() {
    // Enough archetypes for `cached()` to use its match list.
    #[derive(Component)]
    struct Marker<const N: usize>;
    fn layout<const N: usize>(w: &mut World, hp: i32) -> Entity {
        let e = w.spawn((Health(hp),));
        w.insert(e, Marker::<N>);
        e
    }
    let mut w = World::new();
    w.track_changes::<Health>();
    let mut es = Vec::new();
    for hp in 0..40 {
        es.push(layout::<0>(&mut w, hp));
        es.push(layout::<1>(&mut w, hp));
        es.push(layout::<2>(&mut w, hp));
        es.push(layout::<3>(&mut w, hp));
    }
    // Pad the world with layouts the query doesn't match.
    for i in 0..140u32 {
        let e = w.spawn((Armor(0),));
        for bit in 0..8 {
            if i >> bit & 1 != 0 {
                match bit {
                    0 => w.insert(e, Marker::<10>),
                    1 => w.insert(e, Marker::<11>),
                    2 => w.insert(e, Marker::<12>),
                    3 => w.insert(e, Marker::<13>),
                    4 => w.insert(e, Marker::<14>),
                    5 => w.insert(e, Marker::<15>),
                    6 => w.insert(e, Marker::<16>),
                    _ => w.insert(e, Marker::<17>),
                };
            }
        }
    }
    assert!(w.raw_ref().archetypes().len() >= 128);
    let mut seen = w.advance_change_tick();
    for round in 0..3 {
        let touched: Vec<Entity> = es.iter().copied().skip(round).step_by(7).collect();
        for &e in &touched {
            w.get_mut::<Health>(e).unwrap().0 += 1;
        }
        let mut cached = w.query::<Entity>().filter::<Changed<Health>>().cached().since(seen);
        assert_eq!(cached.count(), touched.len());
        assert_eq!(sorted(cached.iter().collect()), sorted(touched));
        seen = w.advance_change_tick();
    }
}

// ---- parallel loops ------------------------------------------------------------------

#[cfg(feature = "parallel")]
mod parallel {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Rows per parallel chunk are at most this (64 under Miri).
    const CHUNK: usize = if cfg!(miri) { 64 } else { 8192 };

    /// Two archetypes of `Health`, each several chunks long, and the entities
    /// whose `Health` is changed after the returned tick: long runs, single
    /// rows, and the first and last row.
    fn world() -> (World, Vec<Entity>, Tick) {
        let mut w = World::new();
        w.track_changes::<Health>();
        let n = 3 * CHUNK + 5;
        let mut es: Vec<Entity> = (0..n).map(|hp| w.spawn((Health(hp as i32),))).collect();
        es.extend((0..n).map(|hp| w.spawn((Health(hp as i32), Armor(0)))));
        let seen = w.advance_change_tick();
        let mut touched = Vec::new();
        for (i, &e) in es.iter().enumerate() {
            let in_arch = i % n;
            if in_arch < 2 * CHUNK + 9 || in_arch.is_multiple_of(7) || in_arch == n - 1 {
                w.get_mut::<Health>(e).unwrap().0 += 1_000_000;
                touched.push(e);
            }
        }
        w.advance_change_tick();
        (w, touched, seen)
    }

    #[test]
    fn parallel_loops_visit_exactly_the_rows_that_pass() {
        let (mut w, touched, seen) = world();
        let visits = AtomicUsize::new(0);
        let mut q = w.query::<&Health>().filter::<Changed<Health>>().since(seen);
        q.par_for_each(|h| {
            assert!(h.0 >= 1_000_000);
            visits.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(visits.into_inner(), touched.len());
        assert_eq!(q.count(), touched.len());
    }

    #[test]
    fn parallel_indices_follow_the_filtered_order() {
        let (mut w, touched, seen) = world();
        let mut q = w.query::<Entity>().filter::<Changed<Health>>().since(seen);
        let sequential: Vec<Entity> = q.iter().collect();
        assert_eq!(sorted(sequential.clone()), sorted(touched));
        let slots: Vec<AtomicUsize> = (0..q.count()).map(|_| AtomicUsize::new(usize::MAX)).collect();
        q.par_for_each_indexed(|i, e| {
            let before = slots[i].swap(e.to_bits() as usize, Ordering::Relaxed);
            assert_eq!(before, usize::MAX, "every index is used once");
        });
        let by_index: Vec<usize> = slots.into_iter().map(AtomicUsize::into_inner).collect();
        let expected: Vec<usize> = sequential.iter().map(|e| e.to_bits() as usize).collect();
        assert_eq!(by_index, expected);
    }

    #[test]
    fn parallel_collect_keeps_the_filtered_order_for_plain_and_owned_results() {
        let (mut w, _, seen) = world();
        let mut q = w.query::<&Health>().filter::<Changed<Health>>().since(seen);
        let sequential: Vec<i32> = q.iter().map(|h| h.0).collect();
        let mut plain = Vec::new();
        q.par_collect_into(&mut plain, |h| h.0);
        assert_eq!(plain, sequential);
        // Results with a destructor take the other collection path.
        let mut owned = Vec::new();
        q.par_collect_into(&mut owned, |h| h.0.to_string());
        let expected: Vec<String> = sequential.iter().map(i32::to_string).collect();
        assert_eq!(owned, expected);
    }

    #[test]
    fn a_parallel_mutable_loop_marks_what_it_visits() {
        let (mut w, touched, seen) = world();
        let later = w.advance_change_tick();
        // Unfiltered: every `Health` with an `Armor` is marked.
        w.query::<(&mut Health, &Armor)>().par_for_each(|(h, _)| h.0 += 1);
        let armored = w.query::<&Armor>().count();
        assert_eq!(changed::<Health>(&mut w, later).len(), armored);
        // Filtered: only the rows that pass are visited and marked again.
        let latest = w.advance_change_tick();
        let mut out = Vec::new();
        w.query::<(Entity, &mut Health)>()
            .filter::<Changed<Health>>()
            .since(seen)
            .par_collect_into(&mut out, |(e, h)| {
                h.0 += 1;
                e
            });
        let expected: Vec<Entity> = sorted(
            touched
                .into_iter()
                .chain(w.query::<Entity>().filter::<With<Armor>>().iter())
                .collect(),
        );
        let mut expected = expected;
        expected.dedup();
        assert_eq!(sorted(out), expected);
        assert_eq!(changed::<Health>(&mut w, latest), expected);
    }
}

#[test]
fn single_marks_a_mutable_fetch_and_a_failed_one_marks_nothing() {
    let (mut w, [a, b, c], seen) = three();
    // Three matches: an error, and nothing counts as changed.
    assert_eq!(
        w.query::<&mut Health>().single().err(),
        Some(SingleError::MultipleEntities)
    );
    assert_eq!(changed::<Health>(&mut w, seen), []);
    // A change filter picks the row: the only one that changed.
    w.get_mut::<Health>(b).unwrap().0 = 20;
    let later = w.advance_change_tick();
    let only = w
        .query::<(Entity, &Health)>()
        .filter::<Changed<Health>>()
        .since(seen)
        .single();
    assert_eq!(only, Ok((b, &Health(20))));
    assert_eq!(changed::<Health>(&mut w, later), [], "reading it changed nothing");
    // Fetching `&mut` through it is a change, as in every query.
    w.query::<&mut Health>()
        .filter::<Changed<Health>>()
        .since(seen)
        .single()
        .unwrap()
        .0 = 21;
    assert_eq!(changed::<Health>(&mut w, later), [b]);
    let _ = (a, c);
}

// ---- runtime-registered components (the C/C++ path) -------------------------------------

mod dynamic {
    use super::*;

    /// A Rust type written through the raw-pointer calls, so typed filters
    /// can observe what those calls stamped.
    #[derive(Debug, PartialEq, Clone, Copy, Component)]
    struct Speed(f32);

    fn ptr(value: &f32) -> *const u8 {
        (value as *const f32).cast()
    }

    #[test]
    fn raw_spawns_inserts_and_removals_are_tracked() {
        let mut w = World::new();
        let speed = w.register::<Speed>();
        w.raw().track_changes_dynamic(speed);
        assert!(w.component_info(speed).tracks_changes());
        // SAFETY: f32 is plain bytes.
        let mass = unsafe { w.raw().register_component("Mass", 4, 4, None) };
        let seen = w.advance_change_tick();

        // SAFETY: each pointer is to a live f32 of the right component.
        let a = unsafe { w.raw().spawn_dynamic(&[(speed, ptr(&1.0))]) };
        let b = unsafe { w.raw().spawn_dynamic(&[(mass, ptr(&5.0))]) };
        let saved = Entity::from_bits(50);
        assert!(unsafe {
            w.raw()
                .spawn_at_dynamic(saved, &[(speed, ptr(&2.0)), (mass, ptr(&6.0))])
        });
        assert_eq!(added::<Speed>(&mut w, seen), [a, saved]);

        let seen = w.advance_change_tick();
        // Replace on `a`, add to `b`.
        assert!(unsafe { w.raw().insert_dynamic(a, speed, ptr(&3.0)) });
        assert!(unsafe { w.raw().insert_dynamic(b, speed, ptr(&4.0)) });
        assert_eq!(changed::<Speed>(&mut w, seen), [a, b]);
        assert_eq!(added::<Speed>(&mut w, seen), [b]);
        assert_eq!(w.get::<Speed>(b), Some(&Speed(4.0)));

        let seen = w.advance_change_tick();
        assert!(w.raw().remove_dynamic(a, speed));
        assert!(!w.raw().remove_dynamic(a, speed));
        assert!(w.raw().remove_dynamic(saved, mass), "an untracked neighbor");
        w.despawn(b);
        assert_eq!(w.raw_ref().removed_dynamic(speed, seen), [a, b]);
        assert_eq!(w.removed_since::<Speed>(seen), [a, b]);
        assert_eq!(changed::<Speed>(&mut w, seen), [], "the kept value moved, unchanged");
    }

    #[test]
    fn a_tracked_runtime_component_is_dropped_once_when_removed_or_replaced() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        unsafe extern "C-unwind" fn count(_: *mut u8) {
            DROPS.fetch_add(1, Ordering::Relaxed);
        }
        let mut w = World::new();
        // SAFETY: plain bytes with a destructor that only counts.
        let id = unsafe { w.raw().register_component("Counted", 4, 4, Some(count)) };
        w.raw().track_changes_dynamic(id);
        // SAFETY: valid values of the component.
        let e = unsafe { w.raw().spawn_dynamic(&[(id, ptr(&1.0))]) };
        assert!(unsafe { w.raw().insert_dynamic(e, id, ptr(&2.0)) });
        assert_eq!(DROPS.load(Ordering::Relaxed), 1);
        assert!(w.raw().remove_dynamic(e, id));
        assert_eq!(DROPS.load(Ordering::Relaxed), 2);
        assert_eq!(w.raw_ref().removed_dynamic(id, Tick::default()), [e]);
        let kept = unsafe { w.raw().spawn_dynamic(&[(id, ptr(&3.0))]) };
        drop(w);
        assert_eq!(DROPS.load(Ordering::Relaxed), 3);
        let _ = kept;
    }

    #[test]
    fn chunk_loops_mark_every_requested_column() {
        let mut w = World::new();
        let speed = w.track_changes::<Speed>();
        let health = w.track_changes::<Health>();
        let a = w.spawn((Speed(1.0), Health(1)));
        let b = w.spawn((Speed(2.0), Health(2), Player));
        let lone = w.spawn((Health(3),));
        let seen = w.advance_change_tick();
        let mut rows = 0;
        w.raw().for_each_chunk(&[speed], |chunk| {
            // SAFETY: the column holds `len` live `Speed` values.
            let speeds = unsafe { std::slice::from_raw_parts_mut(chunk.columns[0].cast::<Speed>(), chunk.len()) };
            speeds.iter_mut().for_each(|s| s.0 *= 2.0);
            rows += chunk.len();
        });
        assert_eq!(rows, 2);
        assert_eq!(changed::<Speed>(&mut w, seen), [a, b]);
        assert_eq!(changed::<Health>(&mut w, seen), [], "a column that wasn't requested");
        let seen = w.advance_change_tick();
        w.raw().for_each_chunk(&[health, speed], |_| {});
        assert_eq!(changed::<Health>(&mut w, seen), [a, b]);
        assert_eq!(changed::<Speed>(&mut w, seen), [a, b]);
        let _ = (lone, health);
    }

    #[test]
    fn raw_pointers_need_an_explicit_mark() {
        let mut w = World::new();
        let speed = w.track_changes::<Speed>();
        let a = w.spawn((Speed(1.0),));
        let b = w.spawn((Speed(2.0),));
        let seen = w.advance_change_tick();
        let p = w.raw().get_dynamic_mut(a, speed).unwrap().cast::<Speed>();
        // SAFETY: the pointer is to `a`'s live value.
        unsafe { (*p).0 = 9.0 };
        assert_eq!(changed::<Speed>(&mut w, seen), [], "the world can't see the write");
        assert!(w.raw().mark_changed_dynamic(a, speed));
        assert_eq!(changed::<Speed>(&mut w, seen), [a]);
        assert!(!w.raw().mark_changed_dynamic(Entity::from_bits(999), speed));
        let _ = b;
    }

    /// The entities a chunk query visits, and the chunk sizes it used.
    fn chunks(w: &mut World, query: ChunkQuery<'_>) -> (Vec<Entity>, Vec<usize>) {
        let (mut entities, mut sizes) = (Vec::new(), Vec::new());
        w.raw().query_chunks(query, |chunk| {
            assert_eq!(chunk.columns.len(), query.ids().len());
            assert!(!chunk.is_empty());
            entities.extend_from_slice(chunk.entities);
            sizes.push(chunk.len());
        });
        (entities, sizes)
    }

    #[test]
    fn chunk_queries_mark_only_the_columns_they_write() {
        let mut w = World::new();
        let speed = w.track_changes::<Speed>();
        let health = w.track_changes::<Health>();
        let a = w.spawn((Speed(1.0), Health(1)));
        let b = w.spawn((Speed(2.0), Health(2), Player));
        let seen = w.advance_change_tick();
        let ids = [speed, health];
        // Read-only: nothing changes.
        let all = chunks(&mut w, ChunkQuery::new(&ids).writes(0));
        assert_eq!(all, (vec![a, b], vec![1, 1]));
        assert_eq!(changed::<Speed>(&mut w, seen), []);
        assert_eq!(changed::<Health>(&mut w, seen), []);
        // Writing the second column only.
        chunks(&mut w, ChunkQuery::new(&ids).writes(0b10));
        assert_eq!(changed::<Speed>(&mut w, seen), []);
        assert_eq!(changed::<Health>(&mut w, seen), [a, b]);
        // The default writes everything, like `for_each_chunk`.
        let seen = w.advance_change_tick();
        chunks(&mut w, ChunkQuery::new(&ids));
        assert_eq!(changed::<Speed>(&mut w, seen), [a, b]);
        assert_eq!(changed::<Health>(&mut w, seen), [a, b]);
    }

    #[test]
    fn chunk_queries_visit_runs_of_changed_rows_with_matching_columns() {
        let mut w = World::new();
        let speed = w.track_changes::<Speed>();
        let es: Vec<Entity> = (0..10).map(|i| w.spawn((Speed(i as f32), Health(i)))).collect();
        let health = w.component_id::<Health>().unwrap();
        let seen = w.advance_change_tick();
        for i in [1, 2, 3, 7, 9] {
            assert!(w.mark_changed::<Speed>(es[i]));
        }
        let ids = [speed, health];
        let world_window = ChunkQuery::new(&ids).writes(0).changed(0b01);
        let query = world_window.since(seen);
        // Three runs: rows 1..4, 7 and 9.
        let mut rows = Vec::new();
        let mut sizes = Vec::new();
        w.raw().query_chunks(query, |chunk| {
            // SAFETY: each column holds `len` live values of its component,
            // starting at the chunk's first row.
            let (speeds, healths) = unsafe {
                (
                    std::slice::from_raw_parts(chunk.columns[0].cast::<Speed>(), chunk.len()),
                    std::slice::from_raw_parts(chunk.columns[1].cast::<Health>(), chunk.len()),
                )
            };
            for ((e, s), h) in chunk.entities.iter().zip(speeds).zip(healths) {
                rows.push((*e, s.0 as i32, h.0));
            }
            sizes.push(chunk.len());
        });
        assert_eq!(sizes, [3, 1, 1]);
        let expected: Vec<_> = [1, 2, 3, 7, 9].iter().map(|&i| (es[i], i as i32, i as i32)).collect();
        assert_eq!(rows, expected);
        // Reading didn't mark anything: a second look sees the same rows.
        assert_eq!(chunks(&mut w, query).0.len(), 5);
        // Writing through a filtered query marks only the rows it visited.
        let later = w.advance_change_tick();
        chunks(&mut w, query.writes(0b01));
        assert_eq!(changed::<Speed>(&mut w, later).len(), 5);
        // `since` defaults to the world's tick.
        w.set_changes_since(w.change_tick());
        assert_eq!(chunks(&mut w, world_window).0, []);
        // `None` keeps the world's tick, so an optional tick chains as it is.
        let no_tick: Option<Tick> = None;
        assert_eq!(chunks(&mut w, world_window.since(no_tick)).0, []);
        assert_eq!(chunks(&mut w, world_window.since(Some(seen))).0.len(), 5);
        assert_eq!(chunks(&mut w, query.since(no_tick)).0, []);
    }

    #[test]
    fn chunk_query_filters_combine_as_all_or_any() {
        let mut w = World::new();
        let speed = w.register::<Speed>();
        let health = w.register::<Health>();
        let both = w.spawn((Speed(1.0), Health(1)));
        let fast = w.spawn((Speed(2.0), Health(2)));
        let hurt = w.spawn((Speed(3.0), Health(3)));
        let idle = w.spawn((Speed(4.0), Health(4)));
        let ids = [speed, health];
        let filter = ChunkQuery::new(&ids).writes(0).changed(0b11);
        // Tracking starts when it is asked for: what exists then counts as
        // changed.
        w.raw().track_changes_dynamic(speed);
        w.raw().track_changes_dynamic(health);
        assert_eq!(chunks(&mut w, filter).0.len(), 4);
        let seen = w.advance_change_tick();
        for e in [both, fast] {
            w.get_mut::<Speed>(e).unwrap().0 += 1.0;
        }
        for e in [both, hurt] {
            w.get_mut::<Health>(e).unwrap().0 += 1;
        }
        let fresh = w.spawn((Speed(5.0), Health(5)));
        let all = filter.since(seen);
        assert_eq!(chunks(&mut w, all).0, [both, fresh]);
        let any = all.match_any(true);
        assert_eq!(chunks(&mut w, any), (vec![both, fast, hurt, fresh], vec![3, 1]));
        let added = all.changed(0).added(0b10);
        assert_eq!(chunks(&mut w, added).0, [fresh]);
        // Bits past the last column are ignored.
        let stray = all.changed(0b01 | 1 << 20).added(1 << 31);
        assert_eq!(chunks(&mut w, stray).0, [both, fast, fresh]);
        let _ = idle;
    }

    #[test]
    #[should_panic(expected = "at most 8 `changed` and `added` filters")]
    fn too_many_chunk_query_filters_panic() {
        let mut w = World::new();
        let ids: Vec<ComponentId> = (0..5)
            .map(|i| {
                // SAFETY: plain bytes.
                unsafe { w.raw().register_component(&format!("C{i}"), 4, 4, None) }
            })
            .collect();
        let query = ChunkQuery::new(&ids).changed(0b11111).added(0b01111);
        w.raw().query_chunks(query, |_| {});
    }

    #[test]
    #[should_panic(expected = "a `changed` or `added` filter needs `Mass`'s changes tracked")]
    fn a_chunk_query_filter_on_an_untracked_component_panics() {
        let mut w = World::new();
        // SAFETY: f32 is plain bytes.
        let mass = unsafe { w.raw().register_component("Mass", 4, 4, None) };
        // Tracking is explicit: the filter doesn't start it.
        w.raw().query_chunks(ChunkQuery::new(&[mass]).added(1), |_| {});
    }

    #[test]
    #[should_panic(expected = "removals of `Mass` aren't tracked")]
    fn removals_of_an_untracked_runtime_component_panic() {
        let mut w = World::new();
        // SAFETY: f32 is plain bytes.
        let mass = unsafe { w.raw().register_component("Mass", 4, 4, None) };
        let _ = w.raw_ref().removed_dynamic(mass, Tick::default());
    }
}

// ---- generated sequences -------------------------------------------------------------------

/// What the model knows about one tracked component of one entity.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Stamp {
    added: u64,
    changed: u64,
}

#[derive(Default)]
struct ModelEntity {
    health: Option<Stamp>,
    armor: Option<Stamp>,
    player: bool,
}

/// Random spawns, despawns, inserts, replacements, removals, mutable
/// borrows, mutable queries, sorts and deferred commands, checked against a
/// model that records the tick of every add and change. After every step,
/// for several readers at once, the world must report exactly the entities
/// the model says were added, changed or removed since each reader's tick.
#[test]
fn generated_sequences_report_exactly_what_the_model_changed() {
    let (cases, ops) = if cfg!(miri) { (2, 60) } else { (40, 600) };
    for seed in 0..cases {
        let mut rng = Rng::new(0xC0FFEE + seed);
        let mut w = World::new();
        if seed % 2 == 0 {
            w.track_changes::<Health>();
            w.track_changes::<Armor>();
        }
        let mut model: Vec<(Entity, ModelEntity)> = Vec::new();
        let mut removed_health: Vec<(Entity, u64)> = Vec::new();
        let mut readers: Vec<Tick> = vec![Tick::default()];
        // Late tracking stamps what exists; the model learns the same.
        let mut tracking = seed % 2 == 0;
        let mut commands = w.commands();
        // Entities with a recorded, not yet applied `insert(e, Armor(7))`.
        let mut pending: Vec<Entity> = Vec::new();
        for op in 0..ops {
            let now = w.change_tick().to_bits();
            let stamp = Stamp {
                added: now,
                changed: now,
            };
            let pick =
                |rng: &mut Rng, model: &Vec<(Entity, ModelEntity)>| (!model.is_empty()).then(|| rng.below(model.len()));
            if !tracking && op == ops / 3 {
                // Start tracking in the middle: everything alive counts as new.
                w.track_changes::<Health>();
                w.track_changes::<Armor>();
                tracking = true;
                for (_, m) in &mut model {
                    m.health = m.health.map(|_| stamp);
                    m.armor = m.armor.map(|_| stamp);
                }
                readers = vec![Tick::default()];
                removed_health.clear();
            }
            match rng.below(12) {
                0 | 1 => {
                    let (e, m) = match rng.below(3) {
                        0 => (
                            w.spawn((Health(op),)),
                            ModelEntity {
                                health: Some(stamp),
                                ..Default::default()
                            },
                        ),
                        1 => (
                            w.spawn((Health(op), Armor(0))),
                            ModelEntity {
                                health: Some(stamp),
                                armor: Some(stamp),
                                player: false,
                            },
                        ),
                        _ => (
                            w.spawn((Armor(0), Player)),
                            ModelEntity {
                                health: None,
                                armor: Some(stamp),
                                player: true,
                            },
                        ),
                    };
                    model.push((e, m));
                }
                2 => {
                    if let Some(i) = pick(&mut rng, &model) {
                        let (e, m) = model.swap_remove(i);
                        assert!(w.despawn(e));
                        if m.health.is_some() {
                            removed_health.push((e, now));
                        }
                    }
                }
                3 => {
                    // Insert or replace `Health`.
                    if let Some(i) = pick(&mut rng, &model) {
                        let (e, m) = &mut model[i];
                        assert!(w.insert(*e, Health(-1)));
                        m.health = Some(match m.health {
                            Some(old) => Stamp { changed: now, ..old },
                            None => stamp,
                        });
                    }
                }
                4 => {
                    if let Some(i) = pick(&mut rng, &model) {
                        let (e, m) = &mut model[i];
                        assert_eq!(w.remove::<Health>(*e).is_some(), m.health.is_some());
                        if m.health.take().is_some() {
                            removed_health.push((*e, now));
                        }
                    }
                }
                5 => {
                    // An untracked neighbor comes and goes: no stamps move.
                    if let Some(i) = pick(&mut rng, &model) {
                        let (e, m) = &mut model[i];
                        if m.player {
                            w.remove::<Player>(*e);
                        } else {
                            w.insert(*e, Player);
                        }
                        m.player = !m.player;
                    }
                }
                6 => {
                    if let Some(i) = pick(&mut rng, &model) {
                        let (e, m) = &mut model[i];
                        if let Some(h) = w.get_mut::<Health>(*e) {
                            h.0 += 1;
                            m.health.as_mut().unwrap().changed = now;
                        }
                        if let Some(armor) = &mut m.armor
                            && rng.one_in(2)
                        {
                            assert!(w.mark_changed::<Armor>(*e));
                            armor.changed = now;
                        }
                    }
                }
                7 => {
                    // A mutable query over one kind of entity.
                    let players = rng.one_in(2);
                    let mut q = w.query::<(&mut Armor, Option<&Health>)>();
                    if players {
                        q = q.filter::<With<Player>>();
                    } else {
                        q = q.filter::<Without<Player>>();
                    }
                    if rng.one_in(2) {
                        q.for_each(|(a, _)| a.0 += 1);
                    } else {
                        for (a, _) in q.iter() {
                            a.0 += 1;
                        }
                    }
                    for (_, m) in &mut model {
                        if m.player == players
                            && let Some(armor) = &mut m.armor
                        {
                            armor.changed = now;
                        }
                    }
                }
                8 => {
                    w.sort_by::<Armor>(|a| (a.0 as u64).wrapping_mul(0x9E37_79B9) % 97);
                }
                9 => {
                    // Deferred: recorded now, stamped when applied below.
                    if let Some(i) = pick(&mut rng, &model) {
                        commands.insert(model[i].0, Armor(7));
                        pending.push(model[i].0);
                    }
                }
                10 => {
                    // Apply at a later tick than the commands were recorded.
                    w.advance_change_tick();
                    let now = w.change_tick().to_bits();
                    w.apply(&mut commands);
                    for target in pending.drain(..) {
                        // A target despawned in the meantime is skipped.
                        if let Some((_, m)) = model.iter_mut().find(|(e, _)| *e == target) {
                            m.armor = Some(match m.armor {
                                Some(old) => Stamp { changed: now, ..old },
                                None => Stamp {
                                    added: now,
                                    changed: now,
                                },
                            });
                        }
                    }
                }
                _ => {
                    readers.push(w.advance_change_tick());
                    if readers.len() > 6 {
                        readers.remove(1 + rng.below(4));
                    }
                }
            }
            if !tracking {
                continue;
            }
            for &reader in &readers {
                let since = reader.to_bits();
                let expect = |f: &dyn Fn(&ModelEntity) -> bool| {
                    sorted(model.iter().filter(|(_, m)| f(m)).map(|(e, _)| *e).collect())
                };
                let context = format!("seed {seed}, op {op}, reader {since}");
                assert_eq!(
                    changed::<Health>(&mut w, reader),
                    expect(&|m| m.health.is_some_and(|s| s.changed > since)),
                    "changed Health: {context}"
                );
                assert_eq!(
                    added::<Health>(&mut w, reader),
                    expect(&|m| m.health.is_some_and(|s| s.added > since)),
                    "added Health: {context}"
                );
                assert_eq!(
                    added::<Armor>(&mut w, reader),
                    expect(&|m| m.armor.is_some_and(|s| s.added > since)),
                    "added Armor: {context}"
                );
                assert_eq!(
                    changed::<Armor>(&mut w, reader),
                    expect(&|m| m.armor.is_some_and(|s| s.changed > since)),
                    "changed Armor: {context}"
                );
                let removed: Vec<Entity> = removed_health
                    .iter()
                    .filter(|(_, tick)| *tick > since)
                    .map(|(e, _)| *e)
                    .collect();
                assert_eq!(w.removed_since::<Health>(reader), removed, "removed Health: {context}");
            }
        }
    }
}

// ---- world views --------------------------------------------------------------

#[test]
fn a_view_marks_what_it_borrows_mutably_like_a_world_query() {
    let (mut w, [a, b, c], _) = three();
    w.insert(c, Armor(1));
    let seen = w.advance_change_tick();
    let mut state = ViewState::new().write::<Health>().read::<Armor>();

    // Reading marks nothing.
    state.run(&mut w, |mut view| {
        assert_eq!(view.query::<&Health>().count(), 3);
        view.query::<&Health>().for_each(|_| {});
        assert_eq!(view.query::<&Health>().iter().count(), 3);
        assert!(view.get::<Health>(a).is_some());
        assert_eq!(view.query::<&mut Health>().count(), 3, "count borrows nothing");
    });
    assert_eq!(changed::<Health>(&mut w, seen), []);

    // `get_mut` marks one entity, a query `get` the one it fetches.
    state.run(&mut w, |mut view| {
        view.get_mut::<Health>(a).unwrap().0 += 1;
    });
    assert_eq!(changed::<Health>(&mut w, seen), [a]);
    let seen = w.advance_change_tick();
    state.run(&mut w, |mut view| {
        assert!(view.query::<&mut Health>().get(b).is_some());
    });
    assert_eq!(changed::<Health>(&mut w, seen), [b]);

    // A mutable loop marks every entity it visits, in each archetype.
    let seen = w.advance_change_tick();
    state.run(&mut w, |mut view| {
        view.query::<(&mut Health, &Armor)>().for_each(|(h, _)| h.0 += 1);
    });
    assert_eq!(changed::<Health>(&mut w, seen), [c]);
    let seen = w.advance_change_tick();
    state.run(&mut w, |mut view| {
        view.query::<&mut Health>().for_each(|h| h.0 += 1);
    });
    assert_eq!(changed::<Health>(&mut w, seen), [a, b, c]);

    // An iterator marks the archetypes it reached.
    let seen = w.advance_change_tick();
    state.run(&mut w, |mut view| {
        assert_eq!(view.query::<&mut Health>().iter().count(), 3);
    });
    assert_eq!(changed::<Health>(&mut w, seen), [a, b, c]);
    assert_eq!(added::<Health>(&mut w, seen), [], "a write never counts as added");
}

#[test]
fn views_on_two_threads_stamp_their_own_components() {
    let mut w = World::new();
    w.track_changes::<Health>();
    w.track_changes::<Armor>();
    let n = if cfg!(miri) { 40 } else { 2000 };
    let es: Vec<Entity> = (0..n).map(|i| w.spawn((Health(i), Armor(i)))).collect();
    let only_health = w.spawn((Health(-1),));
    let seen = w.advance_change_tick();

    let heal = ViewState::new().write::<Health>();
    let plate = ViewState::new().write::<Armor>();
    let first = es[0];
    // Each state with its job: 0 heals everyone, 1 plates the first.
    let mut states = [(heal, 0u8), (plate, 1u8)];
    let group = ViewGroup::new(states.iter().map(|s| &s.0)).unwrap();
    group.run(
        &mut w,
        &mut states,
        true,
        |s| (&mut s.0, &mut s.1),
        |job, mut view| {
            if *job == 0 {
                view.query::<&mut Health>().for_each(|h| h.0 += 1);
            } else {
                view.get_mut::<Armor>(first).unwrap().0 += 1;
            }
        },
    );

    let mut all = es.clone();
    all.push(only_health);
    assert_eq!(changed::<Health>(&mut w, seen), sorted(all));
    assert_eq!(changed::<Armor>(&mut w, seen), [es[0]]);
}

// ---- declared access ------------------------------------------------------------

/// A change filter depends on the stamps that writers of the component
/// leave, so it is recorded as reading the component.
#[cfg(debug_assertions)]
#[test]
fn a_change_filter_is_recorded_as_reading_its_component() {
    let (mut w, _, _) = three();
    w.spawn((Armor(1), Health(1)));
    let (health, armor) = (w.component_id::<Health>().unwrap(), w.component_id::<Armor>().unwrap());
    w.track_changes::<Armor>();

    w.record_access();
    let _ = w.query::<Entity>().filter::<Changed<Health>>().count();
    assert_eq!(w.take_access_log().unwrap().components, [(health, false)]);

    w.record_access();
    let _ = w
        .query::<Entity>()
        .filter::<Or<(Added<Armor>, Changed<Health>)>>()
        .count();
    let mut read = w.take_access_log().unwrap().components;
    read.sort_by_key(|&(id, _)| id.index());
    let mut expected = vec![(health, false), (armor, false)];
    expected.sort_by_key(|&(id, _)| id.index());
    assert_eq!(read, expected);
}

/// The filters skip blocks of rows nobody wrote to (see `RowFilter`): every
/// way a stamp is written or moved must keep its block's summary newer than
/// any reader that hasn't seen it. Random edits over a few thousand
/// entities (many blocks, two archetypes, swaps, moves, sorts, growth)
/// against a plain model of when each `Health` was added and last changed.
#[test]
fn change_filters_that_skip_still_blocks_find_exactly_what_changed() {
    use std::collections::HashMap;

    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n as u64) as usize
        }
    }

    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut w = World::new();
    w.track_changes::<Health>();
    // Entity -> (added, changed) ticks of its `Health`.
    let mut model: HashMap<Entity, (Tick, Tick)> = HashMap::new();
    let mut live: Vec<Entity> = Vec::new();
    let mut readers: Vec<Tick> = vec![Tick::default()];
    let mut peak = 0;
    let check = |w: &mut World, model: &HashMap<Entity, (Tick, Tick)>, readers: &[Tick], step: usize| {
        for &since in readers {
            let want_changed = sorted(model.iter().filter(|(_, t)| t.1 > since).map(|(&e, _)| e).collect());
            let want_added = sorted(model.iter().filter(|(_, t)| t.0 > since).map(|(&e, _)| e).collect());
            assert_eq!(
                changed::<Health>(w, since),
                want_changed,
                "step {step}, since {since:?}"
            );
            assert_eq!(added::<Health>(w, since), want_added, "step {step}, since {since:?}");
            let mut q = w.query::<Entity>().filter::<Changed<Health>>().since(since);
            assert_eq!(q.count(), want_changed.len(), "step {step}: count");
            let mut each = Vec::new();
            q.for_each(|e| each.push(e));
            assert_eq!(sorted(each), want_changed, "step {step}: for_each");
            let mut either = w
                .query::<Entity>()
                .filter::<Or<(Changed<Health>, With<Player>)>>()
                .since(since)
                .iter()
                .collect::<Vec<_>>();
            either.retain(|&e| !w.has::<Player>(e));
            assert_eq!(sorted(either), want_changed, "step {step}: in an `Or`");
            let id = w.component_id::<Health>().unwrap();
            let ids = [id];
            let mut chunks = Vec::new();
            w.raw()
                .query_chunks(ChunkQuery::new(&ids).writes(0).changed(1).since(since), |chunk| {
                    chunks.extend_from_slice(chunk.entities)
                });
            assert_eq!(sorted(chunks), want_changed, "step {step}: query_chunks");
        }
    };

    // Under Miri a smaller model: fewer steps and operations, and spawn
    // bursts of a bit over two blocks instead of three, so the world still
    // spans several blocks (see the end) but every check stays small. The
    // full size took over an hour there.
    let (steps, max_ops, burst) = if cfg!(miri) {
        (50, 12, 2 * 64 + 5)
    } else {
        (400, 40, 200)
    };
    for step in 0..steps {
        let now = w.change_tick();
        let ops = 1 + rng.below(max_ops);
        for _ in 0..ops {
            match rng.below(12) {
                // Spawn, now and then a burst that grows the archetypes.
                0 | 1 => {
                    let n = if rng.below(10) == 0 { burst } else { 1 };
                    for _ in 0..n {
                        let e = if rng.below(2) == 0 {
                            w.spawn((Health(0),))
                        } else {
                            w.spawn((Health(0), Armor(0)))
                        };
                        model.insert(e, (now, now));
                        live.push(e);
                    }
                }
                2 if !live.is_empty() => {
                    let e = live.swap_remove(rng.below(live.len()));
                    w.despawn(e);
                    model.remove(&e);
                }
                3 if !live.is_empty() => {
                    let e = live[rng.below(live.len())];
                    if let Some(h) = w.get_mut::<Health>(e) {
                        h.0 += 1;
                        model.get_mut(&e).unwrap().1 = now;
                    }
                }
                4 if !live.is_empty() => {
                    let e = live[rng.below(live.len())];
                    if w.mark_changed::<Health>(e) && model.contains_key(&e) {
                        model.get_mut(&e).unwrap().1 = now;
                    }
                }
                // Moves between archetypes keep the stamps.
                5 if !live.is_empty() => {
                    let e = live[rng.below(live.len())];
                    if w.has::<Armor>(e) {
                        w.remove::<Armor>(e);
                    } else {
                        w.insert(e, Armor(1));
                    }
                }
                // Removing and inserting again counts as added.
                6 if !live.is_empty() => {
                    let e = live[rng.below(live.len())];
                    if w.remove::<Health>(e).is_some() {
                        model.remove(&e);
                    } else {
                        w.insert(e, Health(2));
                        model.insert(e, (now, now));
                    }
                }
                // Replacing a value is a change, not an addition.
                7 if !live.is_empty() => {
                    let e = live[rng.below(live.len())];
                    if w.has::<Health>(e) {
                        w.insert(e, Health(3));
                        model.get_mut(&e).unwrap().1 = now;
                    }
                }
                // A query marks every entity it borrows mutably.
                8 if rng.below(8) == 0 => {
                    w.query::<(Entity, &mut Health, &Armor)>().for_each(|(e, h, _)| {
                        h.0 += 1;
                        model.get_mut(&e).unwrap().1 = now;
                    });
                }
                9 if rng.below(8) == 0 => {
                    w.sort_by::<Health>(|h| h.0 as u64 ^ 0x55);
                }
                _ => {}
            }
        }
        peak = peak.max(live.len());
        if step % 7 == 0 {
            check(&mut w, &model, &readers, step);
        }
        let seen = w.advance_change_tick();
        if rng.below(4) == 0 {
            readers.push(seen);
            if readers.len() > 6 {
                readers.remove(1);
            }
        }
    }
    check(&mut w, &model, &readers, steps);
    // The model has to have spanned several blocks of rows for the skipped
    // blocks to have been tested.
    assert!(peak > 2 * 64, "the model peaked at {peak} entities");
}

#[test]
fn a_change_moved_to_another_archetype_is_seen_in_its_new_block() {
    let mut w = World::new();
    w.track_changes::<Health>();
    let es: Vec<_> = (0..200).map(|i| w.spawn((Health(i),))).collect();
    // Something already in the destination, stamped long ago.
    let old = w.spawn((Health(-1), Armor(0)));
    let seen = w.advance_change_tick();
    w.get_mut::<Health>(es[10]).unwrap().0 = 1000;
    w.get_mut::<Health>(es[150]).unwrap().0 = 2000;
    w.advance_change_tick();
    // `es[10]` joins a block that only held a still row, `es[150]` starts
    // nothing new: both keep their stamps, and readers still see them.
    w.insert(es[10], Armor(1));
    w.insert(es[150], Armor(2));
    assert_eq!(changed::<Health>(&mut w, seen), sorted(vec![es[10], es[150]]));
    // A move into the first row of an empty archetype starts its block.
    w.insert(es[150], Player);
    assert_eq!(changed::<Health>(&mut w, seen), sorted(vec![es[10], es[150]]));
    let _ = old;
}

#[test]
fn tracking_a_component_many_entities_already_have_counts_them_all_as_changed() {
    let mut w = World::new();
    let es: Vec<_> = (0..300).map(|i| w.spawn((Health(i),))).collect();
    let before = w.advance_change_tick();
    w.track_changes::<Health>();
    assert_eq!(changed::<Health>(&mut w, before), es);
    assert_eq!(added::<Health>(&mut w, before), es);
    let now = w.advance_change_tick();
    assert_eq!(changed::<Health>(&mut w, now), []);
}
