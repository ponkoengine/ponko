//! World snapshots: `World::save_snapshot` and `World::restore_snapshot`.
//!
//! Small entity counts on purpose: these tests also run under Miri.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{Pos, Rng, Vel};
use ponko_ecs::{Changed, Component, Entity, SnapshotError, Tick, World, WorldSnapshot};

#[derive(Component, Clone, Debug, PartialEq)]
struct Name(String);

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct Hp(i32);

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct Marker;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
struct Extra(u8);

/// Counts its live values, to check that nothing is dropped twice or leaked.
#[derive(Component, Debug)]
struct Counted(Arc<AtomicUsize>);

impl Clone for Counted {
    fn clone(&self) -> Self {
        self.0.fetch_add(1, Ordering::Relaxed);
        Counted(self.0.clone())
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn counted(live: &Arc<AtomicUsize>) -> Counted {
    live.fetch_add(1, Ordering::Relaxed);
    Counted(live.clone())
}

/// Every entity with its components, in query order.
type Row = (Entity, Option<Pos>, Option<Vel>, Option<String>, Option<Hp>);

fn contents(world: &mut World) -> Vec<Row> {
    world
        .query::<(Entity, Option<&Pos>, Option<&Vel>, Option<&Name>, Option<&Hp>)>()
        .iter()
        .map(|(e, p, v, n, h)| (e, p.copied(), v.copied(), n.map(|n| n.0.clone()), h.copied()))
        .collect()
}

fn mixed_world(n: usize) -> World {
    let mut world = World::new();
    for i in 0..n {
        let f = i as f32;
        match i % 3 {
            0 => {
                world.spawn((Pos(f, 0.0), Vel(1.0, f)));
            }
            1 => {
                world.spawn((Pos(f, 1.0), Name(format!("n{i}"))));
            }
            _ => {
                world.spawn((Hp(i as i32), Marker));
            }
        }
    }
    world
}

#[test]
fn a_restore_brings_back_every_entity_and_component_unchanged() {
    let mut world = mixed_world(30);
    let before = contents(&mut world);
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    assert_eq!(snapshot.len(), 30);

    // Change values, despawn, spawn, move entities between archetypes.
    world.query::<&mut Pos>().for_each(|p| p.0 += 100.0);
    world.query::<&mut Name>().for_each(|n| n.0.push('!'));
    let some: Vec<Entity> = before.iter().step_by(4).map(|c| c.0).collect();
    for e in some {
        world.despawn(e);
    }
    let first = before[1].0;
    world.insert(first, Hp(7));
    world.spawn((Name("new".into()),));

    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(contents(&mut world), before);
    assert_eq!(world.len(), 30);
}

#[test]
fn spawns_after_a_restore_get_the_ids_they_got_after_the_save() {
    let mut world = mixed_world(20);
    // Holes in the free list, so ids are reused.
    let gone: Vec<Entity> = world.query::<(Entity, &Hp)>().iter().map(|(e, _)| e).take(4).collect();
    for e in gone {
        world.despawn(e);
    }
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();

    let first_run = spawn_some(&mut world);
    world.restore_snapshot(&snapshot).unwrap();
    // Another history in between: spawns and despawns that the restore undoes.
    let other = world.spawn((Hp(-1),));
    world.despawn(other);
    world.spawn((Hp(-2),));
    world.restore_snapshot(&snapshot).unwrap();
    let second_run = spawn_some(&mut world);
    assert_eq!(first_run, second_run);
}

/// Spawns directly, in a batch and through commands; returns the ids.
fn spawn_some(world: &mut World) -> Vec<Entity> {
    let mut ids = vec![world.spawn((Hp(1),)), world.spawn((Pos(0.0, 0.0),))];
    let mut commands = world.commands();
    let a = commands.spawn((Hp(2),));
    let b = commands.spawn((Name("cmd".into()),));
    world.apply(&mut commands);
    ids.extend([a, b]);
    let despawned = ids[0];
    world.despawn(despawned);
    ids.push(world.spawn((Hp(3),)));
    ids
}

#[test]
fn old_handles_stay_dead_and_restored_ones_come_back() {
    let mut world = World::new();
    let a = world.spawn((Hp(1),));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.despawn(a);
    let b = world.spawn((Hp(2),)); // reuses a's slot, one generation on
    assert_ne!(a, b);
    world.restore_snapshot(&snapshot).unwrap();
    assert!(world.is_alive(a));
    assert!(!world.is_alive(b), "an entity from after the save is gone");
    assert_eq!(world.get::<Hp>(a), Some(&Hp(1)));
}

#[test]
fn queries_visit_entities_in_the_saved_order_after_new_archetypes_came_and_went() {
    let mut world = mixed_world(12);
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();

    // What a world that never restored does next: a new archetype, then
    // another component set spawned after it.
    let mut straight = mixed_world(12);
    let next = |w: &mut World| {
        w.spawn((Pos(9.0, 9.0), Hp(9)));
        w.spawn((Vel(1.0, 1.0), Name("late".into())));
        contents(w)
    };
    let expected = next(&mut straight);

    // A detour that made other archetypes in the slots those would take.
    world.spawn((Extra(1), Hp(0)));
    world.spawn((Extra(2), Name("detour".into())));
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(next(&mut world), expected);
}

#[test]
fn a_restore_lays_out_slots_again_after_archetypes_were_reclaimed() {
    // Enough short-lived component sets that the world reclaims archetypes.
    let mut world = World::new();
    world.spawn((Pos(0.0, 0.0),));
    let mut snapshot = WorldSnapshot::new();
    let churn = |w: &mut World, seed: u64| {
        let mut rng = Rng::new(seed);
        for i in 0..200 {
            let e = w.spawn((Hp(i),));
            if rng.next_u64().is_multiple_of(2) {
                w.insert(e, Pos(0.0, i as f32));
            }
            if rng.next_u64().is_multiple_of(3) {
                w.insert(e, Extra(i as u8));
            }
            if rng.next_u64().is_multiple_of(2) {
                w.insert(e, Name(format!("{i}")));
            }
            if !rng.next_u64().is_multiple_of(4) {
                w.despawn(e);
            }
        }
    };
    churn(&mut world, 1);
    world.save_snapshot(&mut snapshot).unwrap();
    let mut straight = World::new();
    straight.spawn((Pos(0.0, 0.0),));
    churn(&mut straight, 1);
    churn(&mut straight, 2);
    let expected = contents(&mut straight);

    churn(&mut world, 3); // another history
    world.restore_snapshot(&snapshot).unwrap();
    churn(&mut world, 2);
    assert_eq!(contents(&mut world), expected);
}

#[test]
fn clones_are_dropped_exactly_once() {
    let live = Arc::new(AtomicUsize::new(0));
    {
        let mut world = World::new();
        let es: Vec<Entity> = (0..10).map(|_| world.spawn((counted(&live), Hp(0)))).collect();
        let mut snapshot = WorldSnapshot::new();
        world.save_snapshot(&mut snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 20, "the world's 10 and the snapshot's 10");
        // Saving again replaces the saved values.
        world.save_snapshot(&mut snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 20);
        world.despawn(es[0]);
        world.spawn((counted(&live),)); // another archetype
        world.restore_snapshot(&snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 20);
        world.restore_snapshot(&snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 20);
        // Fewer rows than the snapshot's buffers hold, then more.
        for &e in &es[..5] {
            world.despawn(e);
        }
        world.save_snapshot(&mut snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 10);
        for _ in 0..40 {
            world.spawn((counted(&live), Hp(1)));
        }
        world.save_snapshot(&mut snapshot).unwrap();
        assert_eq!(live.load(Ordering::Relaxed), 90);
    }
    assert_eq!(
        live.load(Ordering::Relaxed),
        0,
        "dropping the world and the snapshot drops every value"
    );
}

#[test]
fn snapshots_keep_strings_apart_from_the_world() {
    let mut world = World::new();
    let e = world.spawn((Name("a".into()),));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.get_mut::<Name>(e).unwrap().0.push_str("bc");
    world.save_snapshot(&mut snapshot).unwrap();
    world.get_mut::<Name>(e).unwrap().0.clear();
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.get::<Name>(e).unwrap().0, "abc");
    world.get_mut::<Name>(e).unwrap().0.push('d');
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.get::<Name>(e).unwrap().0, "abc");
}

#[test]
fn zero_sized_components_are_restored() {
    let mut world = World::new();
    let a = world.spawn((Marker,));
    let b = world.spawn((Marker, Hp(1)));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.remove::<Marker>(a);
    world.remove::<Marker>(b);
    world.restore_snapshot(&snapshot).unwrap();
    assert!(world.has::<Marker>(a) && world.has::<Marker>(b));
    assert_eq!(world.query::<&Marker>().iter().count(), 2);
}

#[test]
fn a_component_that_is_not_clone_is_named_and_nothing_is_saved() {
    #[derive(Component)]
    struct Socket(#[allow(dead_code)] u32);

    let mut world = World::new();
    world.spawn((Hp(1),));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.spawn((Socket(1),));
    let err = world.save_snapshot(&mut snapshot).unwrap_err();
    assert!(matches!(&err, SnapshotError::NotCloneable { component } if component.ends_with("Socket")));
    assert_eq!(snapshot.len(), 1, "the failed save left the snapshot as it was");
}

#[test]
fn a_generic_component_snapshots_after_register_clone() {
    #[derive(Component, Clone, Debug, PartialEq)]
    struct Tagged<T>(T);

    let mut world = World::new();
    let e = world.spawn((Tagged(5u8),));
    assert!(world.save_snapshot(&mut WorldSnapshot::new()).is_err());
    world.register_clone::<Tagged<u8>>();
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.get_mut::<Tagged<u8>>(e).unwrap().0 = 9;
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.get::<Tagged<u8>>(e), Some(&Tagged(5)));
}

#[test]
fn a_snapshot_restores_only_its_own_world() {
    let mut a = mixed_world(3);
    let mut b = mixed_world(3);
    let mut snapshot = WorldSnapshot::new();
    assert_eq!(a.restore_snapshot(&snapshot), Err(SnapshotError::OtherWorld));
    a.save_snapshot(&mut snapshot).unwrap();
    assert_eq!(b.restore_snapshot(&snapshot), Err(SnapshotError::OtherWorld));
    // One snapshot can move on to another world.
    b.save_snapshot(&mut snapshot).unwrap();
    b.restore_snapshot(&snapshot).unwrap();
    assert_eq!(a.restore_snapshot(&snapshot), Err(SnapshotError::OtherWorld));
}

#[test]
fn named_resources_are_restored_and_others_left_alone() {
    #[derive(Clone, Debug, PartialEq)]
    struct Score(Vec<u32>);
    struct Settings(u32);
    #[derive(Clone)]
    struct Later;

    let mut world = World::new();
    world.snapshot_resource::<Score>();
    world.snapshot_resource::<Later>();
    world.snapshot_resource::<Score>(); // twice is once
    world.insert_resource(Score(vec![1]));
    world.insert_resource(Settings(1));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.resource_mut::<Score>().0.push(2);
    world.resource_mut::<Settings>().0 = 2;
    world.insert_resource(Later); // not there when saved: removed again
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.resource::<Score>(), &Score(vec![1]));
    assert_eq!(world.resource::<Settings>().0, 2);
    assert!(world.get_resource::<Later>().is_none());
    world.remove_resource::<Score>();
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.resource::<Score>(), &Score(vec![1]));
}

/// The entities whose `Hp` changed after `seen`, in query order.
fn changed_hp(world: &mut World, seen: Tick) -> Vec<Entity> {
    world
        .query::<(Entity, &Hp)>()
        .filter::<Changed<Hp>>()
        .since(seen)
        .iter()
        .map(|(e, _)| e)
        .collect()
}

/// `tick` moved by as many ticks as the world went on since `saved_at`:
/// what a reader that kept `tick` before a save uses after the restore.
fn aged(world: &World, saved_at: Tick, tick: Tick) -> Tick {
    Tick::from_bits(world.change_tick().to_bits() - (saved_at.to_bits() - tick.to_bits()))
}

#[test]
fn restored_change_stamps_keep_their_age() {
    let mut world = World::new();
    world.track_changes::<Hp>();
    let old = world.spawn((Hp(1),));
    let fresh = world.spawn((Hp(2),));
    // Enough rows for several summary blocks, none of them changed lately.
    for i in 0..150 {
        world.spawn((Hp(i),));
    }
    let seen = world.advance_change_tick();
    world.get_mut::<Hp>(fresh).unwrap().0 = 3;
    world.advance_change_tick();
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    let saved_at = world.change_tick();
    assert_eq!(changed_hp(&mut world, seen), [fresh]);

    // Another history: every value changes, ticks go by.
    world.query::<&mut Hp>().for_each(|h| h.0 += 10);
    for _ in 0..5 {
        world.advance_change_tick();
    }

    world.restore_snapshot(&snapshot).unwrap();
    let seen = aged(&world, saved_at, seen);
    assert_eq!(
        changed_hp(&mut world, seen),
        [fresh],
        "only the value changed after the reader looked counts as changed, as before the restore"
    );
    let added: Vec<Entity> = world
        .query::<Entity>()
        .filter::<ponko_ecs::Added<Hp>>()
        .since(seen)
        .iter()
        .collect();
    assert!(added.is_empty(), "nothing counts as added again");
    let _ = old;
}

#[test]
fn restored_removal_logs_keep_their_age() {
    let mut world = World::new();
    world.track_changes::<Hp>();
    let a = world.spawn((Hp(1),));
    let b = world.spawn((Hp(2),));
    let c = world.spawn((Hp(3),));
    world.despawn(a);
    let seen = world.advance_change_tick();
    world.remove::<Hp>(b);
    world.advance_change_tick();
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    let saved_at = world.change_tick();

    world.despawn(c); // a removal the restore takes back
    world.advance_change_tick();
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.removed_since::<Hp>(aged(&world, saved_at, seen)), [b]);
    assert_eq!(world.removed_since::<Hp>(Tick::default()), [a, b]);
}

#[test]
fn components_tracked_after_the_save_count_as_added_on_restore() {
    let mut world = World::new();
    let e = world.spawn((Hp(1),));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.track_changes::<Hp>();
    let seen = world.advance_change_tick();
    assert!(changed_hp(&mut world, seen).is_empty());
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(changed_hp(&mut world, seen), [e]);
}

#[test]
fn hooks_do_not_run_on_restore() {
    #[derive(Default)]
    struct Calls(u32);
    let mut world = World::new();
    world.insert_resource(Calls::default());
    world.on_add::<Hp>(|w, _| w.resource_mut::<Calls>().0 += 1);
    world.on_remove::<Hp>(|w, _| w.resource_mut::<Calls>().0 += 1);
    let e = world.spawn((Hp(1),));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    world.despawn(e);
    world.spawn((Hp(2), Pos(0.0, 0.0))); // a new archetype, with hooks
    let calls = world.resource::<Calls>().0;
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.resource::<Calls>().0, calls);
    // Hooks still run for what happens after it, also in re-laid slots.
    world.spawn((Hp(3), Pos(0.0, 0.0)));
    assert_eq!(world.resource::<Calls>().0, calls + 1);
    world.despawn(e);
    assert_eq!(world.resource::<Calls>().0, calls + 2);
}

#[test]
#[should_panic(expected = "a spawn recorded before `World::reset` (or a load) can't be applied after it")]
fn commands_recorded_before_a_restore_cannot_spawn_after_it() {
    let mut world = World::new();
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    let mut commands = world.commands();
    commands.spawn((Hp(1),));
    world.restore_snapshot(&snapshot).unwrap();
    world.apply(&mut commands);
}

#[test]
fn a_panicking_clone_during_restore_leaves_an_empty_world() {
    static BREAK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    #[derive(Component, Debug)]
    struct Fragile;
    impl Clone for Fragile {
        fn clone(&self) -> Self {
            assert!(!BREAK.load(Ordering::Relaxed), "fragile value cloned");
            Fragile
        }
    }

    let mut world = World::new();
    world.spawn((Hp(1),));
    world.spawn((Fragile,));
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    BREAK.store(true, Ordering::Relaxed);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| world.restore_snapshot(&snapshot)));
    BREAK.store(false, Ordering::Relaxed);
    assert!(result.is_err());
    assert!(world.is_empty());
    assert_eq!(world.query::<&Hp>().iter().count(), 0);
    let fresh = world.spawn((Hp(5),));
    assert_eq!(world.get::<Hp>(fresh), Some(&Hp(5)));
    // And it can still be restored.
    world.restore_snapshot(&snapshot).unwrap();
    assert_eq!(world.len(), 2);
}

#[test]
fn plain_c_components_are_copied_as_bytes() {
    let mut world = World::new();
    // SAFETY: 8 plain bytes, no destructor.
    let id = unsafe { world.raw().register_component("Plain", 8, 4, None) };
    let value = [1u32, 2u32];
    // SAFETY: `value` is 8 readable bytes of the registered layout.
    let e = unsafe { world.raw().spawn_dynamic(&[(id, value.as_ptr().cast())]) };
    assert!(world.component_info(id).can_snapshot());
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    let p = world.raw().get_dynamic_mut(e, id).unwrap().cast::<u32>();
    // SAFETY: the live value of `e`, nothing else uses the world meanwhile.
    unsafe { p.write(9) };
    world.restore_snapshot(&snapshot).unwrap();
    let p = world.raw_ref().get_dynamic(e, id).unwrap().cast::<[u32; 2]>();
    // SAFETY: as above, read only.
    assert_eq!(unsafe { p.read_unaligned() }, [1, 2]);
}

#[test]
fn c_components_with_a_destructor_are_refused() {
    unsafe extern "C-unwind" fn nothing(_: *mut u8) {}
    let mut world = World::new();
    // SAFETY: 4 plain bytes; the destructor does nothing.
    let id = unsafe { world.raw().register_component("Owned", 4, 4, Some(nothing)) };
    let value = 1u32;
    // SAFETY: `value` is 4 readable bytes of the registered layout.
    unsafe { world.raw().spawn_dynamic(&[(id, (&raw const value).cast())]) };
    let err = world.save_snapshot(&mut WorldSnapshot::new()).unwrap_err();
    assert_eq!(
        err,
        SnapshotError::NotCloneable {
            component: "Owned".into()
        }
    );
}

#[test]
fn a_snapshot_reports_its_size() {
    let mut world = mixed_world(9);
    let mut snapshot = WorldSnapshot::new();
    world.save_snapshot(&mut snapshot).unwrap();
    assert_eq!(snapshot.len(), 9);
    assert!(!snapshot.is_empty());
    assert!(snapshot.heap_bytes() > 0);
    world.clear();
    world.save_snapshot(&mut snapshot).unwrap();
    assert!(snapshot.is_empty());
    assert!(format!("{snapshot:?}").contains("WorldSnapshot"));
}
