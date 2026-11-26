//! Component hooks: `World::on_add`, `on_replace`, `on_remove`.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};

use common::{Pos, Tag, Vel};
use ponko_ecs::{Component, Entity, HookWorld, World};

/// What the hooks saw, in order.
#[derive(Default)]
struct Log(Vec<(&'static str, Entity, i32)>);

#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Hp(i32);
#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Armor(i32);
#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Marker;

fn log(world: &mut HookWorld, what: &'static str, e: Entity, value: i32) {
    world.resource_mut::<Log>().0.push((what, e, value));
}

fn hp_added(w: &mut HookWorld, e: Entity) {
    let v = w.get::<Hp>(e).expect("on_add sees the new value").0;
    log(w, "add hp", e, v);
}
fn hp_removed(w: &mut HookWorld, e: Entity) {
    let v = w.get::<Hp>(e).expect("on_remove sees the value that goes").0;
    log(w, "remove hp", e, v);
}
fn hp_replaced(w: &mut HookWorld, e: Entity, new: &Hp) {
    let old = w.get::<Hp>(e).expect("on_replace sees the old value").0;
    log(w, "replace hp from", e, old);
    log(w, "replace hp to", e, new.0);
}
fn armor_added(w: &mut HookWorld, e: Entity) {
    let v = w.get::<Armor>(e).unwrap().0;
    log(w, "add armor", e, v);
}
fn armor_removed(w: &mut HookWorld, e: Entity) {
    let v = w.get::<Armor>(e).unwrap().0;
    log(w, "remove armor", e, v);
}

fn logged() -> World {
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_add::<Hp>(hp_added);
    world.on_remove::<Hp>(hp_removed);
    world.on_replace::<Hp>(hp_replaced);
    world
}

fn take_log(world: &mut World) -> Vec<(&'static str, Entity, i32)> {
    std::mem::take(&mut world.resource_mut::<Log>().0)
}

#[test]
fn on_add_runs_after_every_kind_of_spawn_with_the_value_in_place() {
    let mut world = logged();
    let a = world.spawn((Hp(1), Pos(0.0, 0.0)));
    world.spawn_batch([(Hp(2),), (Hp(3),)]);
    let at = Entity::from_bits(40);
    assert!(world.spawn_at(at, (Hp(4),)));
    let mut commands = world.commands();
    let c = commands.spawn((Hp(5),));
    world.apply(&mut commands);
    let id = world.component_id::<Hp>().unwrap();
    let raw = Hp(6);
    // SAFETY: the pointer is to a valid `Hp`, which the world now owns.
    let d = unsafe { world.raw().spawn_dynamic(&[(id, (&raw as *const Hp).cast())]) };
    let values: Vec<i32> = take_log(&mut world).iter().map(|l| l.2).collect();
    assert_eq!(values, [1, 2, 3, 4, 5, 6]);
    assert!([a, at, c, d].iter().all(|&e| world.is_alive(e)));
}

#[test]
fn insert_runs_on_add_when_it_adds_and_on_replace_when_it_overwrites() {
    let mut world = logged();
    let e = world.spawn((Pos(0.0, 0.0),));
    assert!(world.insert(e, Hp(10)));
    assert!(world.insert(e, Hp(20)));
    assert_eq!(world.get::<Hp>(e), Some(&Hp(20)));
    assert_eq!(
        take_log(&mut world),
        [("add hp", e, 10), ("replace hp from", e, 10), ("replace hp to", e, 20)]
    );
}

#[test]
fn on_remove_runs_before_remove_and_the_value_still_comes_back() {
    let mut world = logged();
    let e = world.spawn((Hp(7), Pos(1.0, 2.0)));
    take_log(&mut world);
    assert_eq!(world.remove::<Hp>(e), Some(Hp(7)));
    assert_eq!(take_log(&mut world), [("remove hp", e, 7)]);
    assert!(!world.has::<Hp>(e) && world.has::<Pos>(e));
    assert_eq!(world.remove::<Hp>(e), None, "nothing left to remove");
    assert!(take_log(&mut world).is_empty(), "no hook without a value");
}

#[test]
fn despawn_runs_every_components_on_remove_in_id_order_while_the_entity_is_whole() {
    let mut world = logged();
    world.on_add::<Armor>(armor_added);
    world.on_remove::<Armor>(armor_removed);
    // Hp was registered first, so its hooks come first.
    let e = world.spawn((Armor(3), Hp(9)));
    assert_eq!(take_log(&mut world), [("add hp", e, 9), ("add armor", e, 3)]);
    assert!(world.despawn(e));
    assert_eq!(take_log(&mut world), [("remove hp", e, 9), ("remove armor", e, 3)]);
    assert!(!world.is_alive(e));
    assert!(!world.despawn(e));
    assert!(take_log(&mut world).is_empty());
}

#[test]
fn clear_and_reset_run_on_remove_and_reset_drops_what_hooks_recorded() {
    fn respawn(w: &mut HookWorld, _: Entity) {
        w.commands().spawn((Marker,));
    }
    let mut world = logged();
    world.on_remove::<Hp>(respawn);
    let (a, b) = (world.spawn((Hp(1),)), world.spawn((Hp(2),)));
    take_log(&mut world);
    world.clear();
    let mut seen: Vec<Entity> = take_log(&mut world).iter().map(|l| l.1).collect();
    seen.sort();
    assert_eq!(seen, [a, b]);
    // `clear` applies the hooks' commands after it despawned everything.
    assert_eq!(world.query::<&Marker>().count(), 2);

    let c = world.spawn((Hp(3),));
    take_log(&mut world);
    world.reset();
    assert_eq!(take_log(&mut world), [("remove hp", c, 3)]);
    assert!(world.is_empty(), "a reset starts over: the hooks' spawns are dropped");
    // The world works as new afterwards, hooks included.
    let e = world.spawn((Hp(4),));
    assert_eq!(take_log(&mut world), [("add hp", e, 4)]);
}

#[test]
fn components_without_hooks_run_none_whenever_their_archetype_was_made() {
    let mut world = World::new();
    world.insert_resource(Log::default());
    // Archetypes made before and after the hook is registered.
    let before = world.spawn((Pos(0.0, 0.0), Hp(1)));
    let plain = world.spawn((Pos(0.0, 0.0),));
    world.on_add::<Hp>(hp_added);
    world.on_remove::<Hp>(hp_removed);
    let after = world.spawn((Vel(0.0, 0.0), Hp(2)));
    let plain2 = world.spawn((Vel(0.0, 0.0),));
    world.insert(plain, Tag);
    world.remove::<Tag>(plain);
    world.despawn(plain);
    world.despawn(plain2);
    assert!(
        take_log(&mut world).iter().all(|l| l.1 == after),
        "only `after` got Hp since"
    );
    world.despawn(before);
    world.despawn(after);
    let log = take_log(&mut world);
    assert_eq!(log, [("remove hp", before, 1), ("remove hp", after, 2)]);
}

#[test]
fn a_reused_archetype_slot_takes_the_hooks_of_its_new_components() {
    #[derive(Component)]
    struct Kind<const N: usize>;
    let mut world = logged();
    // Fill and empty many archetypes, so their slots are reclaimed and
    // reused for new component sets, with and without hooks.
    fn churn<const N: usize>(world: &mut World, hp: bool) {
        let e = world.spawn((Kind::<N>, Pos(0.0, 0.0)));
        if hp {
            world.insert(e, Hp(N as i32));
        }
        world.despawn(e);
    }
    macro_rules! many {
        ($($n:literal)*) => {$(
            churn::<$n>(&mut world, $n % 2 == 0);
        )*};
    }
    many!(0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29
          30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59
          60 61 62 63 64 65 66 67 68 69 70 71 72 73 74 75 76 77 78 79 80 81 82 83 84 85 86 87 88 89
          90 91 92 93 94 95 96 97 98 99 100 101 102 103 104 105 106 107 108 109 110 111 112 113 114 115);
    let log = take_log(&mut world);
    let adds = log.iter().filter(|l| l.0 == "add hp").count();
    let removes = log.iter().filter(|l| l.0 == "remove hp").count();
    assert_eq!((adds, removes), (58, 58));
    assert!(log.chunks(2).all(|p| p[0].2 == p[1].2 && p[0].2 % 2 == 0));
}

/// Hooks that log and record commands, to see when commands run.
fn hp_added_spawns_marker(w: &mut HookWorld, e: Entity) {
    log(w, "add hp", e, 0);
    let m = w.commands().spawn((Marker,));
    log(w, "recorded", m, 0);
}
fn marker_added(w: &mut HookWorld, e: Entity) {
    log(w, "add marker", e, 0);
}

#[test]
fn commands_a_hook_records_are_applied_before_the_call_returns() {
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_add::<Hp>(hp_added_spawns_marker);
    world.on_add::<Marker>(marker_added);
    let e = world.spawn((Hp(1),));
    let log = take_log(&mut world);
    let m = log[1].1;
    assert_eq!(log, [("add hp", e, 0), ("recorded", m, 0), ("add marker", m, 0)]);
    assert!(world.has::<Marker>(m), "spawned with the id the hook got at once");
}

#[test]
fn a_batch_or_an_apply_runs_its_hooks_commands_after_all_of_it() {
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_add::<Hp>(hp_added_spawns_marker);
    world.on_add::<Marker>(marker_added);
    world.spawn_batch([(Hp(1),), (Hp(2),)]);
    let kinds: Vec<&str> = take_log(&mut world).iter().map(|l| l.0).collect();
    assert_eq!(
        kinds,
        ["add hp", "recorded", "add hp", "recorded", "add marker", "add marker"]
    );

    let mut commands = world.commands();
    commands.spawn((Hp(3),));
    commands.spawn((Hp(4),));
    world.apply(&mut commands);
    let kinds: Vec<&str> = take_log(&mut world).iter().map(|l| l.0).collect();
    assert_eq!(
        kinds,
        ["add hp", "recorded", "add hp", "recorded", "add marker", "add marker"]
    );
}

/// A chain: Hp's hook adds Armor, Armor's adds Marker; both log.
fn chain_hp(w: &mut HookWorld, e: Entity) {
    log(w, "add hp", e, 0);
    w.commands().insert(e, Armor(1));
}
fn chain_armor(w: &mut HookWorld, e: Entity) {
    log(w, "add armor", e, 0);
    w.commands().insert(e, Marker);
}

#[test]
fn commands_from_hooks_run_first_recorded_first_until_none_are_left() {
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_add::<Hp>(chain_hp);
    world.on_add::<Armor>(chain_armor);
    world.on_add::<Marker>(marker_added);
    let mut commands = world.commands();
    let a = commands.spawn((Hp(1),));
    let b = commands.spawn((Hp(2),));
    world.apply(&mut commands);
    // Breadth first: both spawns' hooks, then both Armor inserts' hooks,
    // then both Markers'.
    let log: Vec<(&str, Entity)> = take_log(&mut world).iter().map(|l| (l.0, l.1)).collect();
    assert_eq!(
        log,
        [
            ("add hp", a),
            ("add hp", b),
            ("add armor", a),
            ("add armor", b),
            ("add marker", a),
            ("add marker", b)
        ]
    );
    assert!(world.has::<Marker>(a) && world.has::<Marker>(b));
}

#[test]
fn several_hooks_of_one_component_run_in_the_order_registered() {
    fn first(w: &mut HookWorld, e: Entity) {
        log(w, "first", e, 0);
    }
    fn second(w: &mut HookWorld, e: Entity) {
        log(w, "second", e, 0);
    }
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_add::<Hp>(first);
    world.on_add::<Hp>(second);
    world.on_remove::<Hp>(second);
    world.on_remove::<Hp>(first);
    let e = world.spawn((Hp(1),));
    world.despawn(e);
    let kinds: Vec<&str> = take_log(&mut world).iter().map(|l| l.0).collect();
    assert_eq!(kinds, ["first", "second", "second", "first"]);
}

#[test]
fn raw_paths_run_the_hooks_of_rust_components() {
    let mut world = logged();
    let e = world.spawn((Pos(0.0, 0.0),));
    let id = world.register::<Hp>();
    let (one, two) = (Hp(1), Hp(2));
    // SAFETY: valid `Hp` values, which the world takes over.
    unsafe {
        assert!(world.raw().insert_dynamic(e, id, (&one as *const Hp).cast()));
        assert!(world.raw().insert_dynamic(e, id, (&two as *const Hp).cast()));
    }
    assert!(world.raw().remove_dynamic(e, id));
    assert!(!world.raw().remove_dynamic(e, id));
    assert_eq!(
        take_log(&mut world),
        [
            ("add hp", e, 1),
            ("replace hp from", e, 1),
            ("replace hp to", e, 2),
            ("remove hp", e, 2)
        ]
    );
}

#[test]
fn hooks_and_change_detection_see_the_same_changes() {
    let mut world = logged();
    world.track_changes::<Hp>();
    let e = world.spawn((Hp(1),));
    world.remove::<Hp>(e);
    assert_eq!(world.removed::<Hp>(), [e]);
    assert_eq!(take_log(&mut world).len(), 2);
}

/// Counts drops, to check values are dropped once around hooks.
static DROPS: AtomicUsize = AtomicUsize::new(0);

#[derive(Component)]
struct Counted(#[allow(dead_code)] Box<u32>);
impl Drop for Counted {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn values_are_dropped_once_around_hooks() {
    fn seen(w: &mut HookWorld, e: Entity) {
        assert!(w.get::<Counted>(e).is_some());
    }
    fn swapped(w: &mut HookWorld, e: Entity, new: &Counted) {
        assert!(w.get::<Counted>(e).is_some() && *new.0 == 2);
    }
    let mut world = World::new();
    world.on_add::<Counted>(seen);
    world.on_remove::<Counted>(seen);
    world.on_replace::<Counted>(swapped);
    let before = DROPS.load(Ordering::Relaxed);
    let e = world.spawn((Counted(Box::new(1)),));
    world.insert(e, Counted(Box::new(2)));
    assert_eq!(DROPS.load(Ordering::Relaxed) - before, 1, "the old value");
    let taken = world.remove::<Counted>(e);
    assert_eq!(DROPS.load(Ordering::Relaxed) - before, 1, "returned, not dropped");
    drop(taken);
    world.insert(e, Counted(Box::new(3)));
    world.despawn(e);
    assert_eq!(DROPS.load(Ordering::Relaxed) - before, 3);
}

#[test]
fn a_panicking_hook_leaves_the_world_working() {
    fn boom(_: &mut HookWorld, _: Entity) {
        panic!("hook failed");
    }
    let mut world = logged();
    world.on_add::<Marker>(boom);
    world.on_add::<Armor>(hp_added_spawns_marker_on_armor);
    fn hp_added_spawns_marker_on_armor(w: &mut HookWorld, _: Entity) {
        w.commands().spawn((Marker,));
    }
    let e = world.spawn((Pos(0.0, 0.0),));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        world.insert(e, Armor(1));
    }));
    assert!(caught.is_err());
    // The spawn itself happened; the hook that panicked ran after it.
    assert_eq!(world.query::<&Marker>().count(), 1);
    // Hooks and their commands still work afterwards.
    let h = world.spawn((Hp(5),));
    assert_eq!(take_log(&mut world), [("add hp", h, 5)]);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        world.spawn((Marker,));
    }));
    assert!(caught.is_err());
    world.despawn(h);
    assert_eq!(take_log(&mut world), [("remove hp", h, 5)]);
}

#[test]
fn hooks_are_not_counted_as_the_callers_access() {
    fn touch(w: &mut HookWorld, _: Entity) {
        w.resource_mut::<Log>().0.clear();
        w.commands().spawn((Marker,));
    }
    let mut world = World::new();
    world.insert_resource(Log::default());
    world.on_replace::<Hp>(|w, e, _| touch(w, e));
    let e = world.spawn((Hp(1),));
    world.record_access();
    world.insert(e, Hp(2));
    let log = world.take_access_log();
    if let Some(log) = log {
        // Recording exists in debug builds only.
        let hp = world.component_id::<Hp>().unwrap();
        assert!(log.resources.is_empty(), "the hook's resource isn't the caller's");
        assert!(log.components.iter().all(|c| c.0 == hp), "nor its spawn's components");
    }
    assert_eq!(world.query::<&Marker>().count(), 1);
}

#[cfg(feature = "parallel")]
#[test]
fn hooks_run_in_the_same_order_however_commands_were_recorded_in_parallel() {
    use std::sync::Mutex;

    use ponko_ecs::Commands;
    fn run() -> Vec<(&'static str, Entity, i32)> {
        let mut world = logged();
        for i in 0..64 {
            world.spawn((Pos(i as f32, 0.0),));
        }
        take_log(&mut world);
        // Recorded on several threads, one buffer per entity, applied in
        // entity order: the order of the hooks follows that, not the threads.
        let buffers: Mutex<Vec<(Entity, Commands)>> = Mutex::new(Vec::new());
        world.query::<(Entity, &Pos)>().par_for_each(|(e, p)| {
            let mut c = Commands::new();
            c.insert(e, Hp(p.0 as i32));
            buffers.lock().unwrap().push((e, c));
        });
        let mut buffers = buffers.into_inner().unwrap();
        buffers.sort_by_key(|b| b.0);
        for (_, mut c) in buffers {
            world.apply(&mut c);
        }
        take_log(&mut world)
    }
    let first = run();
    assert_eq!(first.len(), 64);
    assert_eq!(first, run());
}

#[test]
fn a_hook_reads_into_a_resource_with_resource_scope() {
    #[derive(Default)]
    struct Total(i32);
    fn add(w: &mut HookWorld, e: Entity) {
        w.resource_scope::<Total, _>(|w, total| total.0 += w.get::<Hp>(e).unwrap().0);
    }
    let mut world = World::new();
    world.insert_resource(Total::default());
    world.on_add::<Hp>(add);
    world.spawn_batch((1..=4).map(|i| (Hp(i),)));
    assert_eq!(world.resource::<Total>().0, 10);
}

#[test]
fn a_hook_can_query_and_write_other_entities() {
    #[derive(Component)]
    struct Count(u32);
    fn bump(w: &mut HookWorld, _: Entity) {
        w.query::<&mut Count>().for_each(|c| c.0 += 1);
    }
    let mut world = World::new();
    let counter = world.spawn((Count(0),));
    world.on_add::<Tag>(bump);
    for _ in 0..3 {
        world.spawn((Tag,));
    }
    assert_eq!(world.get::<Count>(counter).unwrap().0, 3);
}

#[test]
fn a_bundle_spawned_before_its_hook_was_registered_runs_it_after() {
    let mut world = World::new();
    world.insert_resource(Log::default());
    // Each spawn form caches its bundle's archetype on first use.
    world.spawn((Hp(1), Pos(0.0, 0.0)));
    world.spawn_batch([(Hp(2), Vel(0.0, 0.0))]);
    world.on_add::<Hp>(hp_added);
    let a = world.spawn((Hp(3), Pos(0.0, 0.0)));
    world.spawn_batch([(Hp(4), Vel(0.0, 0.0)), (Hp(5), Vel(0.0, 0.0))]);
    let values: Vec<i32> = take_log(&mut world).iter().map(|l| l.2).collect();
    assert_eq!(values, [3, 4, 5]);
    // And a bundle without hooks stays without, before and after.
    world.spawn((Pos(1.0, 1.0),));
    world.on_remove::<Pos>(hp_removed_if_any);
    world.spawn((Pos(2.0, 2.0),));
    assert!(take_log(&mut world).is_empty());
    assert!(world.is_alive(a));
}

fn hp_removed_if_any(w: &mut HookWorld, e: Entity) {
    if let Some(hp) = w.get::<Hp>(e).copied() {
        log(w, "remove hp", e, hp.0);
    }
}

// ---- the hook world's conveniences: get_mut, insert_resource, get_resource_mut -

/// The entity a hook writes to.
struct Victim(Entity);

/// On a new `Hp`: the victim's armor goes up by the hp, through `HookWorld::get_mut`.
fn hp_hardens_the_victim(w: &mut HookWorld, e: Entity) {
    let hp = w.get::<Hp>(e).unwrap().0;
    let victim = w.resource::<Victim>().0;
    w.get_mut::<Armor>(victim).expect("the victim has armor").0 += hp;
}

/// What a hook found out, as plain text.
#[derive(Default)]
struct Notes(Vec<String>);

#[test]
fn a_hook_changes_another_entitys_component_through_get_mut_and_change_detection_sees_it() {
    use ponko_ecs::{Changed, Tick};
    let changed = |w: &mut World, since: Tick| -> Vec<Entity> {
        let mut out: Vec<Entity> = w
            .query::<Entity>()
            .filter::<Changed<Armor>>()
            .since(since)
            .iter()
            .collect();
        out.sort_by_key(|e| e.to_bits());
        out
    };
    let mut world = World::new();
    world.track_changes::<Armor>();
    let victim = world.spawn((Armor(1),));
    let bystander = world.spawn((Armor(100),));
    world.insert_resource(Victim(victim));
    world.on_add::<Hp>(hp_hardens_the_victim);
    let before = world.advance_change_tick();
    assert_eq!(changed(&mut world, before), [], "nothing changed yet");

    let attacker = world.spawn((Hp(5),));

    assert_eq!(
        world.get::<Armor>(victim),
        Some(&Armor(6)),
        "the hook changed the victim"
    );
    assert_eq!(world.get::<Armor>(bystander), Some(&Armor(100)), "and only the victim");
    assert!(world.get::<Armor>(attacker).is_none());
    assert_eq!(
        changed(&mut world, before),
        [victim],
        "a change made in a hook counts for change detection, as World::get_mut does"
    );
    // The next run sees nothing new until the hook writes again.
    let after = world.advance_change_tick();
    assert_eq!(changed(&mut world, after), []);
    world.spawn((Hp(2),));
    assert_eq!(world.get::<Armor>(victim), Some(&Armor(8)));
    assert_eq!(changed(&mut world, after), [victim]);
}

#[test]
fn get_mut_in_a_hook_is_none_for_a_missing_component_or_a_dead_entity() {
    fn probe(w: &mut HookWorld, e: Entity) {
        let victim = w.resource::<Victim>().0;
        let hp = w.get::<Hp>(e).unwrap().0;
        let note = format!("{hp}: armor {}", w.get_mut::<Armor>(victim).is_some());
        w.resource_mut::<Notes>().0.push(note);
    }
    let mut world = World::new();
    world.insert_resource(Notes::default());
    let victim = world.spawn((Pos(0.0, 0.0),));
    world.insert_resource(Victim(victim));
    world.on_add::<Hp>(probe);
    world.spawn((Hp(1),));
    world.insert(victim, Armor(1));
    world.spawn((Hp(2),));
    world.despawn(victim);
    world.spawn((Hp(3),));
    assert_eq!(
        world.resource::<Notes>().0,
        ["1: armor false", "2: armor true", "3: armor false"]
    );
}

#[derive(Debug, PartialEq, Clone, Copy)]
struct Latest(i32);

fn hp_becomes_the_latest(w: &mut HookWorld, e: Entity) {
    let hp = w.get::<Hp>(e).unwrap().0;
    w.insert_resource(Latest(hp));
}

#[test]
fn a_hook_inserts_a_missing_resource_and_inserting_again_replaces_it() {
    let mut world = World::new();
    world.on_add::<Hp>(hp_becomes_the_latest);
    assert!(world.get_resource::<Latest>().is_none(), "no resource yet");
    world.spawn((Hp(3),));
    assert_eq!(world.get_resource::<Latest>(), Some(&Latest(3)), "the hook made it");
    world.spawn((Hp(8),));
    assert_eq!(
        world.get_resource::<Latest>(),
        Some(&Latest(8)),
        "the second insert replaced it"
    );
}

#[derive(Debug, PartialEq)]
struct Counter(i32);

/// Adds one to the `Counter` if there is one, and notes whether there was.
fn count_if_there_is_a_counter(w: &mut HookWorld, _e: Entity) {
    let note = match w.get_resource_mut::<Counter>() {
        Some(counter) => {
            counter.0 += 1;
            format!("some {}", counter.0)
        }
        None => "none".to_string(),
    };
    w.resource_mut::<Notes>().0.push(note);
}

#[test]
fn a_hook_gets_none_for_a_missing_resource_and_a_mutable_one_when_there_is_one() {
    let mut world = World::new();
    world.insert_resource(Notes::default());
    world.on_add::<Hp>(count_if_there_is_a_counter);
    world.spawn((Hp(1),));
    assert!(
        world.get_resource::<Counter>().is_none(),
        "a missing resource stays missing"
    );
    world.insert_resource(Counter(10));
    world.spawn((Hp(2),));
    world.spawn((Hp(3),));
    assert_eq!(world.resource::<Notes>().0, ["none", "some 11", "some 12"]);
    assert_eq!(
        world.resource::<Counter>(),
        &Counter(12),
        "the hook's changes are in the world"
    );
}
