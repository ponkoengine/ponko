//! Deferred commands.

mod common;

use common::*;
use ponko_ecs::*;

#[test]
fn commands_apply_in_order() {
    let mut w = World::new();
    let es: Vec<_> = (0..10).map(|i| w.spawn((Pos(i as f32, 0.0),))).collect();
    let mut cmd = w.commands();
    w.query::<(Entity, &Pos)>().for_each(|(e, p)| {
        if p.0 < 5.0 {
            cmd.despawn(e);
        } else {
            cmd.insert(e, Tag);
        }
    });
    cmd.spawn((Pos(100.0, 0.0),));
    cmd.remove::<Tag>(es[9]);
    cmd.add(|w| w.insert_resource(42u32));
    assert_eq!(cmd.len(), 13);
    w.apply(&mut cmd);
    assert!(cmd.is_empty());
    assert_eq!(w.len(), 6);
    assert_eq!(w.query::<&Tag>().count(), 4);
    assert_eq!(*w.resource::<u32>(), 42);
}

/// Counts how many times values sharing its counter were dropped.
#[derive(Component)]
struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

fn counter() -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
    std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0))
}

fn count(c: &std::sync::Arc<std::sync::atomic::AtomicUsize>) -> usize {
    c.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn unapplied_commands_drop_their_values_once() {
    let dropped = counter();
    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0),));
    let mut cmd = w.commands();
    cmd.spawn((Counted(dropped.clone()),));
    cmd.insert(e, Counted(dropped.clone()));
    let held = Counted(dropped.clone());
    cmd.add(move |_| drop(held));
    drop(cmd);
    assert_eq!(count(&dropped), 3);
    assert_eq!(w.len(), 1);
}

#[test]
fn cleared_commands_drop_their_values_once_and_apply_nothing() {
    let drops = counter();
    let mut w = World::new();
    let e = w.spawn((Tag,));
    let mut c = w.commands();
    c.spawn((Counted(drops.clone()),));
    c.insert(e, Counted(drops.clone()));
    c.despawn(e);
    c.clear();
    assert_eq!(count(&drops), 2);
    assert!(c.is_empty());
    w.apply(&mut c);
    assert!(w.is_alive(e));
    assert_eq!(w.len(), 1);
    // The list is reusable, and dropping it drops nothing twice.
    c.spawn((Counted(drops.clone()),));
    w.apply(&mut c);
    assert_eq!(w.len(), 2);
    drop(c);
    assert_eq!(count(&drops), 2);
}

#[test]
fn a_value_whose_drop_panics_in_clear_is_never_dropped_twice() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static DROPS: AtomicUsize = AtomicUsize::new(0);
    #[derive(Component)]
    struct Loud;
    impl Drop for Loud {
        fn drop(&mut self) {
            if DROPS.fetch_add(1, Ordering::SeqCst) == 0 {
                panic!("first drop panics");
            }
        }
    }
    let w = World::new();
    let mut c = w.commands();
    c.spawn((Loud,));
    c.spawn((Loud,));
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.clear()));
    assert!(caught.is_err());
    // The first value was dropped once; the one after it is leaked, and
    // dropping the list reads neither again.
    assert!(c.is_empty());
    drop(c);
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
}

#[test]
fn applied_values_move_into_the_world_without_extra_drops() {
    let dropped = counter();
    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0),));
    let mut cmd = w.commands();
    cmd.insert(e, Counted(dropped.clone()));
    cmd.spawn((Counted(dropped.clone()), Pos(1.0, 0.0)));
    w.apply(&mut cmd);
    drop(cmd);
    assert_eq!(count(&dropped), 0);
    assert_eq!(w.query::<&Counted>().count(), 2);
    drop(w);
    assert_eq!(count(&dropped), 2);
}

#[test]
fn a_panicking_command_drops_the_rest_and_leaves_commands_reusable() {
    let dropped = counter();
    let mut w = World::new();
    let mut cmd = w.commands();
    cmd.add(|w| w.insert_resource(1u32));
    cmd.add(|_| panic!("command failed"));
    cmd.spawn((Counted(dropped.clone()),));
    let held = Counted(dropped.clone());
    cmd.add(move |_| drop(held));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.apply(&mut cmd)));
    assert!(result.is_err());
    assert_eq!(*w.resource::<u32>(), 1, "commands before the panic ran");
    assert_eq!(count(&dropped), 2, "commands after the panic were dropped, not run");
    assert!(cmd.is_empty());
    assert_eq!(w.len(), 0);

    cmd.spawn((Pos(2.0, 0.0),));
    w.apply(&mut cmd);
    assert_eq!(w.len(), 1);
}

#[test]
fn commands_carry_zero_sized_and_over_aligned_values() {
    #[derive(Debug, PartialEq, Clone, Copy, Component)]
    #[repr(align(64))]
    struct Wide([u8; 3]);
    #[derive(Component)]
    struct Byte(u8);

    let mut w = World::new();
    let e = w.spawn((Pos(0.0, 0.0),));
    let mut cmd = w.commands();
    // Odd sizes first, so later values start at unaligned offsets.
    cmd.insert(e, Byte(7));
    cmd.insert(e, Tag);
    cmd.insert(e, Wide([1, 2, 3]));
    cmd.add(|w| w.insert_resource(0x1234_5678_9abc_def0u64));
    cmd.spawn((Wide([4, 5, 6]), Tag));
    w.apply(&mut cmd);
    assert_eq!(w.get::<Byte>(e).unwrap().0, 7);
    assert!(w.get::<Tag>(e).is_some());
    assert_eq!(*w.get::<Wide>(e).unwrap(), Wide([1, 2, 3]));
    assert_eq!(*w.resource::<u64>(), 0x1234_5678_9abc_def0);
    let mut wides = Vec::new();
    w.query::<&Wide>().for_each(|x| wides.push(x.0));
    wides.sort();
    assert_eq!(wides, [[1, 2, 3], [4, 5, 6]]);
}

#[test]
fn reused_commands_apply_each_batch_once() {
    let mut w = World::new();
    let mut cmd = w.commands();
    for round in 0..3 {
        for i in 0..100 {
            cmd.spawn((Pos(i as f32, round as f32),));
        }
        assert_eq!(cmd.len(), 100);
        w.apply(&mut cmd);
        assert!(cmd.is_empty());
    }
    assert_eq!(w.len(), 300);
}
