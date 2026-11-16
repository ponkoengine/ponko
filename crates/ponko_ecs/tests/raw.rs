use ponko_ecs::{Component, World};

#[derive(Component, Debug, PartialEq)]
struct Small(u32);
#[derive(Component, Debug, PartialEq)]
struct Big(u64);
#[derive(Component)]
struct Marker;

#[test]
fn raw_views_share_storage_and_release_the_exclusive_borrow() {
    let mut world = World::new();
    let e = world.spawn((Small(7),));
    let id = world.component_id::<Small>().unwrap();
    assert!(world.raw_ref().get_dynamic(e, id).is_some());
    assert_eq!(
        world.raw_ref().component_by_name(std::any::type_name::<Small>()),
        Some(id)
    );
    {
        let mut raw = world.raw();
        let mut rows = 0;
        raw.for_each_chunk(&[id], |chunk| rows += chunk.len());
        assert_eq!(rows, 1);
        assert!(raw.remove_dynamic(e, id));
    }
    assert!(world.get::<Small>(e).is_none());
    assert!(!world.raw_ref().archetypes().is_empty());
}

#[test]
fn a_shared_raw_view_hands_out_borrows_of_the_world() {
    let mut world = World::new();
    world.spawn((Marker,));
    // The slice outlives the temporary view: it borrows the world itself.
    let tables = world.raw_ref().archetypes();
    assert!(!tables.is_empty());
}

#[test]
fn the_raw_view_spawns_and_inserts_through_pointers() {
    let mut world = World::new();
    world.spawn((Small(0), Big(0)));
    let small = world.component_id::<Small>().unwrap();
    let big = world.component_id::<Big>().unwrap();
    let (a, b, c) = (Small(5), Big(6), Small(7));
    // SAFETY: each pointer points to a valid value of its component, which
    // needs no drop, so the world may take a copy.
    let spawned = unsafe { world.raw().spawn_dynamic(&[(small, (&a as *const Small).cast())]) };
    assert_eq!(world.get::<Small>(spawned), Some(&Small(5)));
    // SAFETY: as above.
    assert!(unsafe { world.raw().insert_dynamic(spawned, big, (&b as *const Big).cast()) });
    assert_eq!(world.get::<Big>(spawned), Some(&Big(6)));
    let saved = ponko_ecs::Entity::from_bits(9 | 3 << 32);
    // SAFETY: as above.
    assert!(unsafe {
        world
            .raw()
            .spawn_at_dynamic(saved, &[(small, (&c as *const Small).cast())])
    });
    assert_eq!(world.get::<Small>(saved), Some(&Small(7)));
    // SAFETY: as above; the slot is taken now, so nothing is spawned.
    assert!(!unsafe {
        world
            .raw()
            .spawn_at_dynamic(saved, &[(small, (&c as *const Small).cast())])
    });
    // A dead entity takes nothing.
    world.despawn(spawned);
    // SAFETY: as above.
    assert!(!unsafe { world.raw().insert_dynamic(spawned, small, (&a as *const Small).cast()) });
    assert_eq!(world.len(), 2);
}

#[test]
fn a_deferred_raw_spawn_places_its_values_at_the_reserved_id() {
    let mut world = World::new();
    let small = world.register::<Small>();
    let mut commands = world.commands();
    let placed = commands.spawn_with(move |w, e| {
        let value = Small(11);
        // SAFETY: the pointer points to a valid `Small`, which needs no
        // drop, so the world may take a copy.
        unsafe {
            w.raw()
                .spawn_reserved_dynamic(e, &[(small, (&value as *const Small).cast())])
        };
        true
    });
    // A spawn that reports failure places nothing and frees its id.
    let refused = commands.spawn_with(|_, _| false);
    assert!(!world.is_alive(placed));
    world.apply(&mut commands);
    assert_eq!(world.get::<Small>(placed), Some(&Small(11)));
    assert!(!world.is_alive(refused));
    let reused = commands.spawn((Marker,));
    world.apply(&mut commands);
    assert_eq!(reused.index(), refused.index());
    assert_ne!(reused, refused);
    assert_eq!(world.len(), 2);
}

#[test]
#[should_panic(expected = "isn't a reserved id of this world")]
fn a_raw_spawn_at_an_id_nobody_reserved_panics() {
    let mut world = World::new();
    let id = world.register::<Small>();
    let taken = world.spawn((Marker,));
    let value = Small(1);
    // SAFETY: as above; the call panics before taking the value.
    unsafe {
        world
            .raw()
            .spawn_reserved_dynamic(taken, &[(id, (&value as *const Small).cast())])
    };
}
