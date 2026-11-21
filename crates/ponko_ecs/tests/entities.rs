//! Spawning, despawning, handles, single-entity access, archetype moves, drops.

mod common;

use std::sync::Arc;

use common::*;
use ponko_ecs::*;

/// A component that counts its live copies through the `Arc`.
#[derive(Component, Clone)]
struct Shared(#[allow(dead_code)] Arc<()>);

#[test]
fn spawn_get_despawn() {
    let mut w = World::new();
    let a = w.spawn((Pos(1.0, 2.0), Vel(3.0, 4.0)));
    let b = w.spawn((Pos(5.0, 6.0),));
    assert_eq!(w.len(), 2);
    assert_eq!(w.get::<Pos>(a), Some(&Pos(1.0, 2.0)));
    assert_eq!(w.get::<Vel>(b), None);
    assert!(w.has::<Vel>(a) && !w.has::<Vel>(b));
    assert!(w.despawn(a));
    assert!(!w.despawn(a));
    assert!(!w.is_alive(a));
    assert_eq!(w.get::<Pos>(a), None);
    assert_eq!(w.len(), 1);
}

#[test]
fn stale_handles_stay_dead() {
    let mut w = World::new();
    let a = w.spawn((Pos(0.0, 0.0),));
    w.despawn(a);
    let c = w.spawn((Pos(7.0, 8.0),));
    assert_eq!(c.index(), a.index(), "slot is reused");
    assert_ne!(a, c);
    assert_eq!(w.get::<Pos>(a), None, "old handle must not see the new entity");
    assert_eq!(w.get::<Pos>(c), Some(&Pos(7.0, 8.0)));
    assert_eq!(Entity::from_bits(c.to_bits()), c);
}

#[test]
fn swap_remove_keeps_rows_valid() {
    let mut w = World::new();
    let es: Vec<_> = (0..10).map(|i| w.spawn((Pos(i as f32, 0.0),))).collect();
    w.despawn(es[2]);
    w.despawn(es[0]);
    w.despawn(es[9]);
    for (i, &e) in es.iter().enumerate() {
        if [0, 2, 9].contains(&i) {
            assert!(!w.is_alive(e));
        } else {
            assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, 0.0)));
        }
    }
}

#[test]
fn insert_and_remove_move_between_archetypes() {
    let mut w = World::new();
    let es: Vec<_> = (0..5).map(|i| w.spawn((Pos(i as f32, 0.0),))).collect();
    assert!(w.insert(es[1], Vel(1.0, 1.0)));
    assert!(w.insert(es[3], Tag));
    assert_eq!(w.get::<Vel>(es[1]), Some(&Vel(1.0, 1.0)));
    for (i, &e) in es.iter().enumerate() {
        assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, 0.0)));
    }
    assert_eq!(w.remove::<Vel>(es[1]), Some(Vel(1.0, 1.0)));
    assert_eq!(w.remove::<Vel>(es[1]), None);
    assert_eq!(w.get::<Pos>(es[1]), Some(&Pos(1.0, 0.0)));
    w.insert(es[0], Vel(9.0, 9.0));
    w.insert(es[0], Vel(2.0, 2.0)); // replaces
    assert_eq!(w.get::<Vel>(es[0]), Some(&Vel(2.0, 2.0)));
    // Removing the last component leaves an empty entity that is still alive.
    let e = w.spawn((Tag,));
    assert_eq!(w.remove::<Tag>(e), Some(Tag));
    assert!(w.is_alive(e));
    // Dead entities.
    w.despawn(es[4]);
    assert!(!w.insert(es[4], Tag));
    assert_eq!(w.remove::<Pos>(es[4]), None);
}

#[test]
fn get_mut_changes_value() {
    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0),));
    w.get_mut::<Pos>(e).unwrap().0 = 5.0;
    assert_eq!(w.get::<Pos>(e), Some(&Pos(5.0, 0.0)));
}

#[test]
#[should_panic(expected = "same component twice")]
fn duplicate_bundle_panics() {
    World::new().spawn((Pos(0.0, 0.0), Pos(1.0, 1.0)));
}

#[test]
fn drops_run_exactly_once() {
    let rc = Arc::new(());
    {
        let mut w = World::new();
        let a = w.spawn((Shared(rc.clone()), Pos(0.0, 0.0)));
        let b = w.spawn((Shared(rc.clone()), Pos(0.0, 0.0)));
        let c = w.spawn((Shared(rc.clone()),));
        assert_eq!(Arc::strong_count(&rc), 4);
        w.despawn(a);
        assert_eq!(Arc::strong_count(&rc), 3);
        w.insert(b, Vel(0.0, 0.0)); // moves the Arc to another archetype
        assert_eq!(Arc::strong_count(&rc), 3);
        let taken = w.remove::<Shared>(b).unwrap();
        assert_eq!(Arc::strong_count(&rc), 3);
        drop(taken);
        assert_eq!(Arc::strong_count(&rc), 2);
        w.insert(c, Shared(rc.clone())); // replace drops the old one
        assert_eq!(Arc::strong_count(&rc), 2);
        let id = w.component_id::<Shared>().unwrap();
        assert!(w.raw().remove_dynamic(c, id));
        assert_eq!(Arc::strong_count(&rc), 1);
        w.spawn((Shared(rc.clone()),));
    }
    assert_eq!(Arc::strong_count(&rc), 1, "dropping the world drops its components");
}

#[test]
fn zero_sized_components() {
    let mut w = World::new();
    let es: Vec<_> = (0..20).map(|_| w.spawn((Tag, Pos(1.0, 1.0)))).collect();
    w.despawn(es[3]);
    assert_eq!(w.query::<(&Tag, &Pos)>().count(), 19);
    let e = w.spawn((Tag,));
    assert_eq!(w.get::<Tag>(e), Some(&Tag));
}

#[test]
fn spawn_batch_and_many_archetypes() {
    let mut w = World::new();
    w.spawn_batch((0..1000).map(|i| (Pos(i as f32, 0.0), Vel(1.0, 0.0))));
    assert_eq!(w.len(), 1000);
    let es: Vec<_> = w.query::<Entity>().iter().collect();
    for (i, &e) in es.iter().enumerate() {
        if i % 2 == 0 {
            w.insert(e, Tag);
        }
        if i % 7 == 0 {
            w.despawn(e);
        }
    }
    assert_eq!(w.query::<&Pos>().count(), 1000 - 143);
    assert_eq!(w.query::<&Pos>().filter::<With<Tag>>().count(), 500 - 72);
}

#[test]
fn world_is_send_and_sync() {
    fn check<T: Send + Sync>() {}
    fn send<T: Send>() {}
    check::<World>();
    send::<Commands>();
}

#[test]
fn sort_by_reorders_rows_and_keeps_handles_valid() {
    let rc = Arc::new(());
    let mut w = World::new();
    let n: usize = if cfg!(miri) { 50 } else { 2000 };
    let mut es = Vec::new();
    for i in 0..n {
        // Scrambled keys, two archetypes, a non-Copy component and a ZST.
        let x = ((i * 7919) % n) as f32;
        let e = if i % 3 == 0 {
            w.spawn((Pos(x, i as f32), Shared(rc.clone()), Tag))
        } else {
            w.spawn((Pos(x, i as f32), Vel(x, 0.0)))
        };
        es.push(e);
    }
    w.sort_by::<Pos>(|p| p.0 as u64);
    // Each archetype is now in key order.
    let mut last = std::collections::HashMap::new();
    w.query::<(&Pos, Option<&Tag>)>().for_each(|(p, tag)| {
        let prev = last.insert(tag.is_some(), p.0).unwrap_or(f32::MIN);
        assert!(p.0 >= prev);
    });
    // Handles still point at their own data.
    for (i, &e) in es.iter().enumerate() {
        assert_eq!(w.get::<Pos>(e).unwrap().1, i as f32);
        if i % 3 != 0 {
            assert_eq!(w.get::<Vel>(e).unwrap().0, w.get::<Pos>(e).unwrap().0);
        }
    }
    // Despawn after sorting still swaps rows correctly.
    w.despawn(es[1]);
    assert_eq!(w.get::<Pos>(es[2]).unwrap().1, 2.0);
    assert_eq!(Arc::strong_count(&rc), 1 + n.div_ceil(3));
    drop(w);
    assert_eq!(Arc::strong_count(&rc), 1, "no value dropped twice or leaked");
}

#[test]
fn sort_by_without_the_component_is_a_no_op() {
    let mut w = World::new();
    let e = w.spawn((Vel(1.0, 2.0),));
    w.sort_by::<Pos>(|p| p.0 as u64);
    assert_eq!(w.get::<Vel>(e), Some(&Vel(1.0, 2.0)));
}

// ---------------------------------------------------------- restoring ids

#[test]
fn spawn_at_restores_exact_handles() {
    let mut world = World::new();
    let ids: Vec<Entity> = [(3, 0), (0, 5), (10, 2)]
        .iter()
        .map(|&(i, g)| Entity::from_bits(i | (g as u64) << 32))
        .collect();
    for (n, &e) in ids.iter().enumerate() {
        assert!(world.spawn_at(e, (Pos(n as f32, 0.0),)));
    }
    assert_eq!(world.len(), 3);
    for (n, &e) in ids.iter().enumerate() {
        assert_eq!(world.get::<Pos>(e).unwrap().0, n as f32);
    }
}

#[test]
fn fresh_spawns_never_reuse_restored_slots() {
    let mut world = World::new();
    let restored: Vec<Entity> = (0..50).step_by(3).map(Entity::from_bits).collect();
    for &e in &restored {
        assert!(world.spawn_at(e, (Tag,)));
    }
    let fresh: Vec<Entity> = (0..100).map(|_| world.spawn((Tag,))).collect();
    for e in &fresh {
        assert!(!restored.contains(e));
    }
    assert_eq!(world.len(), restored.len() + fresh.len());
    for &e in restored.iter().chain(&fresh) {
        assert!(world.is_alive(e));
    }
}

#[test]
fn spawn_at_refuses_taken_and_dead_handles() {
    let mut world = World::new();
    let e = world.spawn((Tag,));
    assert!(!world.spawn_at(e, (Tag,)), "taken");
    world.despawn(e);
    assert!(!world.spawn_at(e, (Tag,)), "despawned handles stay dead");
    let newer = Entity::from_bits(e.to_bits() + (1 << 32));
    assert!(
        world.spawn_at(newer, (Tag,)),
        "a newer generation of a free slot is fine"
    );
    assert!(!world.is_alive(e));
    assert_eq!(world.len(), 1);
}

#[test]
fn clear_despawns_everything_but_keeps_resources() {
    let mut world = World::new();
    world.insert_resource(5u32);
    let es: Vec<Entity> = (0..10).map(|i| world.spawn((Pos(i as f32, 0.0),))).collect();
    world.spawn((Tag, Vel(1.0, 1.0)));
    world.clear();
    assert!(world.is_empty());
    assert!(es.iter().all(|&e| !world.is_alive(e)));
    assert_eq!(*world.resource::<u32>(), 5);
    let again = world.spawn((Tag,));
    assert!(world.is_alive(again));
}

#[test]
fn reset_frees_every_id_for_restoring() {
    let mut world = World::new();
    let old: Vec<Entity> = (0..5).map(|_| world.spawn((Tag,))).collect();
    world.despawn(old[1]);
    world.reset();
    assert!(world.is_empty());
    for &e in &old {
        assert!(world.spawn_at(e, (Tag,)), "{e:?}");
    }
}

#[test]
fn memory_accounting_counts_component_bytes() {
    let mut world = World::new();
    assert_eq!(world.allocated_bytes(), 0);
    world.spawn_batch((0..1000).map(|i| (Pos(i as f32, 0.0), Vel(0.0, 0.0))));
    let a = world.raw_ref().archetypes().iter().find(|a| a.len() == 1000).unwrap();
    // Pos and Vel are two f32 each; plus the 8-byte handle.
    assert_eq!(a.bytes_per_entity(), 8 + 8 + 8);
    assert!(world.allocated_bytes() >= 1000 * 24);
    assert!(
        world.allocated_bytes() < 1000 * 24 * 3,
        "capacity stays near what's used"
    );
}

#[test]
fn components_follow_archetype_moves_and_reject_stale_handles() {
    let mut w = World::new();
    let e = w.spawn((Pos(1.0, 2.0),));
    assert_eq!(w.components_of(e).unwrap().len(), 1);
    w.insert(e, Vel(0.0, 0.0));
    let ids = w.components_of(e).unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.windows(2).all(|p| p[0] < p[1]));
    assert!(ids.iter().any(|&id| w.component_info(id).name().ends_with("Vel")));
    w.remove::<Pos>(e);
    assert_eq!(w.components_of(e).unwrap().len(), 1);
    w.despawn(e);
    let replacement = w.spawn((Tag,));
    assert_eq!(replacement.index(), e.index());
    assert!(w.components_of(e).is_none());
    assert_eq!(w.components_of(replacement).unwrap().len(), 1);
}

/// Copies of saved entities: only values typed `Entity` read through the
/// remap, and nested remaps restore the outer one.
#[cfg(feature = "serde")]
#[test]
fn deserialized_entities_follow_the_innermost_remap() {
    use std::collections::HashMap;
    let (saved, outer, inner) = (Entity::from_bits(2), Entity::from_bits(40), Entity::from_bits(50));
    // An entity and a plain number, both saved as 2.
    let read = || serde_json::from_str::<(Entity, u64)>("[2, 2]").unwrap();
    let (a, b, c) = Entity::with_remap(&HashMap::from([(saved, outer)]), || {
        let a = read();
        let b = Entity::with_remap(&HashMap::from([(saved, inner)]), read);
        (a, b, read())
    });
    assert_eq!(a, (outer, 2), "the plain number stays");
    assert_eq!(b.0, inner);
    assert_eq!(c.0, outer, "the outer map is back");
    assert_eq!(read().0, saved, "and none outside");
}

#[test]
fn bundle_values_of_every_size_and_alignment_land_whole() {
    // Spawns move each value with a copy of its own size: odd sizes,
    // high alignments, large and zero-sized values all come back intact,
    // through one-by-one spawns and batches alike.
    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Byte(u8);
    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Odd([u8; 3]);
    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    #[repr(align(32))]
    struct Wide(u64, u32);
    #[derive(Component, Debug, PartialEq, Clone, Copy)]
    struct Big([u64; 40]);
    let value = |i: u64| {
        (
            Byte(i as u8),
            Tag,
            Odd([i as u8, (i >> 8) as u8, 0xA5]),
            Wide(i * 3, !(i as u32)),
            Big(std::array::from_fn(|k| i * 100 + k as u64)),
        )
    };
    let mut w = World::new();
    let one: Vec<_> = (0..70).map(|i| (i, w.spawn(value(i)))).collect();
    w.spawn_batch((70..300).map(value));
    // A batch that doesn't know its length grows the columns as it goes.
    w.spawn_batch((300..400).filter(|i| i % 3 != 0).map(value));
    w.despawn(one[5].1);
    let mut seen = 0;
    for (b, o, wide, big) in w.query::<(&Byte, &Odd, &Wide, &Big)>().iter() {
        let i = wide.0 / 3;
        assert_eq!((b, o, wide, big), (&value(i).0, &value(i).2, &value(i).3, &value(i).4));
        seen += 1;
    }
    assert_eq!(seen, 70 + 230 + 66 - 1);
    assert_eq!(w.get::<Big>(one[69].1), Some(&value(69).4));
}
