//! Required components: `#[component(requires(..))]`, `Component::requires`,
//! `World::require` and `RawWorld::require_dynamic`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use ponko_ecs::{Added, Component, Entity, HookWorld, Requires, World};

#[derive(Component, Default, Debug, PartialEq, Clone, Copy)]
struct Position(f32, f32);

#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Health(u32);

#[derive(Component, Default, Debug, PartialEq, Clone, Copy)]
#[component(requires(Position, Health = Health(100)))]
struct Enemy;

#[derive(Component, Debug, PartialEq, Clone, Copy)]
#[component(requires(Enemy, Health = Health(1000)))]
struct Boss;

#[derive(Component, Debug, PartialEq, Clone, Copy)]
struct Plain(u8);

/// Requires `Ping`, which requires it back.
#[derive(Component, Default, Debug, PartialEq, Clone, Copy)]
#[component(requires(Pong))]
struct Ping;
#[derive(Component, Default, Debug, PartialEq, Clone, Copy)]
#[component(requires(Ping))]
struct Pong;

#[test]
fn spawning_a_component_adds_what_it_requires() {
    let mut world = World::new();
    let e = world.spawn((Enemy,));
    assert_eq!(world.get::<Position>(e), Some(&Position(0.0, 0.0)));
    assert_eq!(world.get::<Health>(e), Some(&Health(100)));
    assert_eq!(world.components_of(e).unwrap().len(), 3);
}

#[test]
fn values_in_the_bundle_win_over_required_ones() {
    let mut world = World::new();
    let e = world.spawn((Health(7), Enemy, Position(1.0, 2.0)));
    assert_eq!(world.get::<Health>(e), Some(&Health(7)));
    assert_eq!(world.get::<Position>(e), Some(&Position(1.0, 2.0)));
    let partly = world.spawn((Enemy, Health(9)));
    assert_eq!(world.get::<Health>(partly), Some(&Health(9)));
    assert_eq!(world.get::<Position>(partly), Some(&Position(0.0, 0.0)));
}

#[test]
fn requirements_go_through_and_the_nearest_value_wins() {
    let mut world = World::new();
    let boss = world.spawn((Boss,));
    // Boss -> Enemy -> Position; Boss names Health itself (1000), which
    // beats Enemy's 100.
    assert!(world.has::<Enemy>(boss));
    assert!(world.has::<Position>(boss));
    assert_eq!(world.get::<Health>(boss), Some(&Health(1000)));
}

#[test]
fn of_two_at_the_same_depth_the_earlier_component_wins() {
    #[derive(Component)]
    #[component(requires(Health = Health(1)))]
    struct First;
    #[derive(Component)]
    #[component(requires(Health = Health(2)))]
    struct Second;
    let mut world = World::new();
    let a = world.spawn((First, Second));
    let b = world.spawn((Second, First));
    assert_eq!(world.get::<Health>(a), Some(&Health(1)));
    assert_eq!(world.get::<Health>(b), Some(&Health(2)));
}

#[test]
fn cycles_add_each_component_once() {
    let mut world = World::new();
    let e = world.spawn((Ping,));
    assert!(world.has::<Pong>(e));
    assert_eq!(world.components_of(e).unwrap().len(), 2);
    let e = world.spawn((Pong,));
    assert!(world.has::<Ping>(e));
}

#[test]
fn every_kind_of_spawn_adds_what_is_required() {
    let mut world = World::new();
    world.spawn_batch((0..10).map(|_| (Enemy,)));
    assert_eq!(world.query::<(&Enemy, &Health, &Position)>().iter().count(), 10);

    let saved = Entity::from_bits(40 | 1 << 32);
    assert!(world.spawn_at(saved, (Enemy,)));
    assert_eq!(world.get::<Health>(saved), Some(&Health(100)));

    let mut commands = world.commands();
    let deferred = commands.spawn((Enemy,));
    world.apply(&mut commands);
    assert_eq!(world.get::<Health>(deferred), Some(&Health(100)));
    assert_eq!(world.len(), 12);
}

#[test]
fn inserting_a_component_adds_what_it_requires() {
    let mut world = World::new();
    let e = world.spawn((Plain(1),));
    assert!(world.insert(e, Enemy));
    assert_eq!(world.get::<Health>(e), Some(&Health(100)));
    assert_eq!(world.get::<Position>(e), Some(&Position(0.0, 0.0)));
    assert_eq!(world.get::<Plain>(e), Some(&Plain(1)));

    // Through commands too.
    let other = world.spawn((Plain(2),));
    let mut commands = world.commands();
    commands.insert(other, Boss);
    world.apply(&mut commands);
    assert_eq!(world.get::<Health>(other), Some(&Health(1000)));
    assert!(world.has::<Enemy>(other) && world.has::<Position>(other));
}

#[test]
fn inserting_keeps_the_values_an_entity_has() {
    let mut world = World::new();
    let e = world.spawn((Health(3),));
    world.insert(e, Enemy);
    assert_eq!(world.get::<Health>(e), Some(&Health(3)));
    assert!(world.has::<Position>(e));
}

#[test]
fn replacing_a_component_adds_nothing() {
    let mut world = World::new();
    let e = world.spawn((Enemy,));
    world.remove::<Health>(e);
    // `e` has an Enemy already: this replaces it, nothing arrives.
    world.insert(e, Enemy);
    assert!(!world.has::<Health>(e));
}

#[test]
fn a_required_component_can_be_removed_later() {
    let mut world = World::new();
    let e = world.spawn((Enemy,));
    assert_eq!(world.remove::<Position>(e), Some(Position(0.0, 0.0)));
    assert!(world.has::<Enemy>(e) && !world.has::<Position>(e));
}

#[derive(Default)]
struct Seen(Vec<(&'static str, bool, bool)>);

fn enemy_added(world: &mut HookWorld, e: Entity) {
    let (h, p) = (world.has::<Health>(e), world.has::<Position>(e));
    world.resource_mut::<Seen>().0.push(("enemy", h, p));
}
fn health_added(world: &mut HookWorld, e: Entity) {
    let (h, p) = (world.has::<Health>(e), world.has::<Position>(e));
    world.resource_mut::<Seen>().0.push(("health", h, p));
}

#[test]
fn on_add_hooks_see_the_required_components_and_run_for_them() {
    let mut world = World::new();
    world.insert_resource(Seen::default());
    world.on_add::<Enemy>(enemy_added);
    world.on_add::<Health>(health_added);
    world.spawn((Enemy,));
    let e = world.spawn((Plain(0),));
    world.insert(e, Enemy);
    let seen = std::mem::take(&mut world.resource_mut::<Seen>().0);
    // In component-id order (Enemy registered first, Health with it), the
    // entity complete each time; the same for the spawn and the insert.
    assert_eq!(seen.len(), 4);
    assert!(seen.iter().all(|&(_, h, p)| h && p), "{seen:?}");
    assert_eq!(seen[0..2], seen[2..4]);
}

#[test]
fn a_requirement_registered_later_applies_to_later_spawns_only() {
    let mut world = World::new();
    let before = world.spawn((Plain(1),));
    world.require::<Plain, _>(|| Health(5));
    let after = world.spawn((Plain(2),));
    assert!(!world.has::<Health>(before));
    assert_eq!(world.get::<Health>(after), Some(&Health(5)));
    // Registering it again replaces the value.
    world.require::<Plain, _>(|| Health(6));
    let again = world.spawn((Plain(3),));
    assert_eq!(world.get::<Health>(again), Some(&Health(6)));
    // A batch spawn of the cached bundle as well.
    world.spawn_batch((0..3).map(|i| (Plain(i),)));
    assert_eq!(world.query::<(&Plain, &Health)>().iter().count(), 5);
}

#[test]
fn requiring_itself_does_nothing() {
    let mut world = World::new();
    world.require::<Plain, _>(|| Plain(9));
    let e = world.spawn((Plain(1),));
    assert_eq!(world.get::<Plain>(e), Some(&Plain(1)));
    let plain = world.register::<Plain>();
    assert!(!world.requires(plain, plain));
}

#[test]
fn hand_written_requirements_work_like_the_derive() {
    struct Body;
    impl Component for Body {
        fn requires(r: &mut Requires<'_>) {
            r.add(Position::default).add(|| Health(42));
        }
    }
    let mut world = World::new();
    let e = world.spawn((Body,));
    assert_eq!(world.get::<Health>(e), Some(&Health(42)));
    let (body, health) = (world.register::<Body>(), world.register::<Health>());
    assert!(world.requires(body, health));
}

#[test]
fn generic_components_can_require_and_be_required() {
    #[derive(Component, Default, Debug, PartialEq)]
    struct Tagged<T>(T);
    #[derive(Component)]
    #[component(requires(Tagged<u32>, Tagged<u8> = Tagged(3)))]
    struct Holder;
    let mut world = World::new();
    let e = world.spawn((Holder,));
    assert_eq!(world.get::<Tagged<u32>>(e), Some(&Tagged(0)));
    assert_eq!(world.get::<Tagged<u8>>(e), Some(&Tagged(3)));
}

#[test]
fn required_components_count_as_added_for_change_detection() {
    let mut world = World::new();
    world.track_changes::<Health>();
    let since = world.change_tick();
    world.advance_change_tick();
    let spawned = world.spawn((Enemy,));
    let inserted = world.spawn((Plain(0),));
    world.insert(inserted, Enemy);
    let mut added: Vec<Entity> = world
        .query::<Entity>()
        .filter::<Added<Health>>()
        .since(since)
        .iter()
        .collect();
    added.sort_by_key(|e| e.to_bits());
    assert_eq!(added, vec![spawned, inserted]);
}

static MADE: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);

/// Counts its values made and dropped.
struct Counted;
impl Component for Counted {}
impl Counted {
    fn make() -> Self {
        MADE.fetch_add(1, Ordering::SeqCst);
        Counted
    }
}
impl Drop for Counted {
    fn drop(&mut self) {
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn required_values_are_dropped_once() {
    #[derive(Component)]
    struct NeedsCounted;
    let mut world = World::new();
    world.require::<NeedsCounted, _>(Counted::make);
    let a = world.spawn((NeedsCounted,));
    world.spawn_batch((0..5).map(|_| (NeedsCounted,)));
    let b = world.spawn((Plain(0),));
    world.insert(b, NeedsCounted);
    world.despawn(a);
    world.remove::<Counted>(b);
    let made = MADE.load(Ordering::SeqCst);
    assert_eq!(made, 7);
    assert_eq!(DROPPED.load(Ordering::SeqCst), 2);
    drop(world);
    assert_eq!(DROPPED.load(Ordering::SeqCst), made);
}

#[derive(Component)]
struct Fragile;
#[derive(Component, Default)]
struct MadeFirst;

fn explode() -> Health {
    panic!("no health today")
}

fn fragile_world() -> World {
    let mut world = World::new();
    world.require::<Fragile, _>(MadeFirst::default);
    world.require::<Fragile, _>(explode);
    world
}

#[test]
fn a_panicking_constructor_leaves_the_world_working() {
    // Spawn: the id goes back, nothing is half placed.
    let mut world = fragile_world();
    let keep = world.spawn((Plain(1),));
    assert!(catch_unwind(AssertUnwindSafe(|| world.spawn((Fragile, Plain(2))))).is_err());
    assert_eq!(world.len(), 1);
    assert_eq!(world.query::<&Plain>().iter().count(), 1);
    // Commands: the reserved id goes back too.
    let mut commands = world.commands();
    let deferred = commands.spawn((Fragile,));
    assert!(catch_unwind(AssertUnwindSafe(|| world.apply(&mut commands))).is_err());
    assert!(!world.is_alive(deferred));
    assert_eq!(world.len(), 1);
    // Insert: the entity stays as it was.
    assert!(catch_unwind(AssertUnwindSafe(|| world.insert(keep, Fragile))).is_err());
    assert_eq!(world.components_of(keep).unwrap().len(), 1);
    assert_eq!(world.get::<Plain>(keep), Some(&Plain(1)));
    // Dynamic spawn.
    let fragile = world.register::<Fragile>();
    // SAFETY: `Fragile` is a zero-sized Rust component.
    let spawned = catch_unwind(AssertUnwindSafe(|| unsafe {
        world
            .raw()
            .spawn_dynamic(&[(fragile, std::ptr::NonNull::<Fragile>::dangling().as_ptr().cast())])
    }));
    assert!(spawned.is_err());
    assert_eq!(world.len(), 1);
    // And the world goes on.
    let fresh = world.spawn((Plain(3),));
    world.despawn(keep);
    assert_eq!(world.len(), 1);
    assert!(world.is_alive(fresh));
}

#[test]
fn dynamic_spawns_and_inserts_get_what_is_required() {
    let mut world = World::new();
    let mut raw = world.raw();
    // SAFETY: plain bytes of the given layouts, no destructors.
    let (body, speed) = unsafe {
        (
            raw.register_component("Body", 4, 4, None),
            raw.register_component("Speed", 8, 4, None),
        )
    };
    let default_speed: [f32; 2] = [1.5, -2.0];
    // SAFETY: both ids are this world's; the value is a valid `Speed`.
    unsafe { raw.require_dynamic(body, speed, default_speed.as_ptr().cast()) };
    assert!(world.requires(body, speed));
    let one = 1u32;
    // SAFETY: a valid `Body` value.
    let e = unsafe { world.raw().spawn_dynamic(&[(body, (&raw const one).cast())]) };
    let read = |world: &World, e: Entity| {
        let p = world.raw_ref().get_dynamic(e, speed).expect("Speed was required");
        // SAFETY: `Speed` is two f32s.
        unsafe { p.cast::<[f32; 2]>().read_unaligned() }
    };
    assert_eq!(read(&world, e), [1.5, -2.0]);

    // Insert, and the typed side requiring a dynamic component.
    let e = world.spawn((Plain(0),));
    // SAFETY: as above.
    unsafe { world.raw().insert_dynamic(e, body, (&raw const one).cast()) };
    assert_eq!(read(&world, e), [1.5, -2.0]);
    let plain = world.register::<Plain>();
    // SAFETY: as above.
    unsafe { world.raw().require_dynamic(plain, speed, default_speed.as_ptr().cast()) };
    let e = world.spawn((Plain(1),));
    assert_eq!(read(&world, e), [1.5, -2.0]);
    // A dynamic spawn that gives the value keeps it.
    let given: [f32; 2] = [9.0, 9.0];
    // SAFETY: valid values of both.
    let e = unsafe {
        world
            .raw()
            .spawn_dynamic(&[(body, (&raw const one).cast()), (speed, given.as_ptr().cast())])
    };
    assert_eq!(read(&world, e), [9.0, 9.0]);
}

#[test]
#[should_panic(expected = "has a destructor, so it can't be copied from bytes")]
fn requiring_bytes_of_a_component_with_a_destructor_panics() {
    #[derive(Component)]
    struct Named(#[allow(dead_code)] String);
    let mut world = World::new();
    let plain = world.register::<Plain>();
    let named = world.register::<Named>();
    let value = Named(String::new());
    // SAFETY: valid ids and value (the call panics before copying).
    unsafe { world.raw().require_dynamic(plain, named, (&raw const value).cast()) };
}

#[test]
fn clear_and_reset_keep_requirements() {
    let mut world = World::new();
    world.spawn((Enemy,));
    world.clear();
    let e = world.spawn((Enemy,));
    assert!(world.has::<Health>(e));
    world.reset();
    let e = world.spawn((Enemy,));
    assert!(world.has::<Health>(e));
}

#[test]
fn bundles_that_lack_nothing_spawn_where_they_did() {
    // A world with requirements: bundles that give everything (or require
    // nothing) land in the archetype of exactly their components.
    let mut world = World::new();
    let full = world.spawn((Enemy, Health(1), Position(0.0, 0.0)));
    let filled = world.spawn((Enemy,));
    let plain = world.spawn((Plain(1),));
    assert_eq!(world.components_of(full), world.components_of(filled));
    assert_eq!(world.components_of(plain).unwrap().len(), 1);
    assert_eq!(world.query::<&Enemy>().iter().count(), 2);
}

#[test]
fn a_batch_fills_the_gaps_of_every_entity_and_keeps_its_own_values() {
    let mut world = World::new();
    // Some give Health, some don't; one batch of a known length, one that
    // grows as it goes (`filter` gives no lower bound).
    world.spawn_batch((0..150).map(|i| (Enemy, Plain(i as u8))));
    world.spawn_batch((0..150).map(|i| (Health(i), Enemy)));
    world.spawn_batch((0..90).filter(|i| i % 2 == 0).map(|i| (Boss, Plain(i as u8))));
    assert_eq!(world.len(), 345);
    let mut filled = world
        .query::<(&Plain, &Health, &Position)>()
        .filter::<ponko_ecs::Without<Boss>>();
    assert_eq!(filled.iter().count(), 150);
    assert!(
        filled
            .iter()
            .all(|(_, h, p)| *h == Health(100) && *p == Position(0.0, 0.0))
    );
    let mut own: Vec<u32> = world
        .query::<&Health>()
        .filter::<ponko_ecs::Without<Plain>>()
        .iter()
        .map(|h| h.0)
        .collect();
    own.sort_unstable();
    assert_eq!(own, (0..150).collect::<Vec<_>>());
    let mut bosses = world.query::<(&Boss, &Enemy, &Health, &Plain)>();
    assert_eq!(bosses.iter().count(), 45);
    assert!(bosses.iter().all(|(_, _, h, p)| *h == Health(1000) && p.0 % 2 == 0));
}

#[test]
fn a_batch_with_hooks_and_gaps_runs_each_hook_on_a_whole_entity() {
    let mut world = World::new();
    world.insert_resource(Seen::default());
    world.on_add::<Enemy>(enemy_added);
    world.on_add::<Health>(health_added);
    world.spawn_batch((0..40).map(|_| (Enemy,)));
    world.spawn_batch((0..10).filter(|_| true).map(|i| (Enemy, Health(i))));
    let seen = std::mem::take(&mut world.resource_mut::<Seen>().0);
    assert_eq!(seen.len(), 100);
    assert!(seen.iter().all(|&(_, h, p)| h && p), "{seen:?}");
    assert_eq!(world.query::<(&Enemy, &Health, &Position)>().iter().count(), 50);
    let own = world.query::<&Health>().iter().filter(|h| h.0 < 10).count();
    assert_eq!(own, 10);
}

static LIVES: AtomicUsize = AtomicUsize::new(0);

/// Panics once `LIVES` runs out.
fn mortal() -> Health {
    if LIVES.fetch_sub(1, Ordering::SeqCst) == 0 {
        panic!("out of lives")
    }
    Health(1)
}

#[test]
fn a_constructor_panicking_mid_batch_keeps_the_entities_before_it() {
    #[derive(Component)]
    struct Mortal;
    for hooked in [false, true] {
        let mut world = World::new();
        world.require::<Mortal, _>(MadeFirst::default);
        world.require::<Mortal, _>(mortal);
        if hooked {
            world.insert_resource(Seen::default());
            world.on_add::<Health>(health_added);
        }
        LIVES.store(3, Ordering::SeqCst);
        let batch = (0..10u8).map(|i| (Mortal, Plain(i)));
        assert!(catch_unwind(AssertUnwindSafe(|| world.spawn_batch(batch))).is_err());
        // The three made before the panic stay, whole; the fourth id went back.
        assert_eq!(world.len(), 3, "hooked: {hooked}");
        let mut kept: Vec<u8> = world
            .query::<(&Plain, &Health, &MadeFirst)>()
            .iter()
            .map(|q| q.0.0)
            .collect();
        kept.sort_unstable();
        assert_eq!(kept, [0, 1, 2]);
        // And the world goes on: the same batch spawns once lives return.
        LIVES.store(100, Ordering::SeqCst);
        world.spawn_batch((0..4u8).map(|i| (Mortal, Plain(i))));
        assert_eq!(world.query::<(&Plain, &Health)>().iter().count(), 7);
        let e = world.query::<Entity>().iter().next().unwrap();
        world.despawn(e);
        assert_eq!(world.len(), 6);
    }
}
