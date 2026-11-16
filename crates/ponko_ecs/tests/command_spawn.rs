//! `Commands::spawn` returns the new entity: ids reserved in the world.

mod common;

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use common::*;
use ponko_ecs::*;

#[derive(Component, Debug, PartialEq)]
struct Target(Entity);

/// The entity at slot `index`, generation `generation`.
fn at(index: u32, generation: u32) -> Entity {
    Entity::from_bits(u64::from(index) | u64::from(generation) << 32)
}

fn generation(e: Entity) -> u32 {
    (e.to_bits() >> 32) as u32
}

#[test]
fn a_spawned_id_can_be_used_by_later_commands_of_the_same_buffer() {
    let mut w = World::new();
    let gun = w.spawn((Tag,));
    let mut c = w.commands();
    let bullet = c.spawn((Pos(1.0, 0.0),));
    c.insert(bullet, Vel(2.0, 0.0));
    c.insert(gun, Target(bullet));
    let trail = c.spawn((Target(bullet),));
    w.apply(&mut c);
    assert_eq!(w.get::<Pos>(bullet), Some(&Pos(1.0, 0.0)));
    assert_eq!(w.get::<Vel>(bullet), Some(&Vel(2.0, 0.0)));
    assert_eq!(w.get::<Target>(gun), Some(&Target(bullet)));
    assert_eq!(w.get::<Target>(trail), Some(&Target(bullet)));
}

#[test]
fn a_spawned_id_is_not_alive_until_its_spawn_is_applied() {
    let mut w = World::new();
    let mut c = w.commands();
    let e = c.spawn((Pos(0.0, 0.0),));
    assert!(!w.is_alive(e));
    assert!(w.get::<Pos>(e).is_none());
    assert!(!w.despawn(e), "despawning a reserved id does nothing");
    assert!(!w.insert(e, Tag), "nor does inserting");
    assert_eq!(w.query::<Entity>().count(), 0);
    assert_eq!(w.len(), 0);
    w.apply(&mut c);
    assert!(w.is_alive(e));
    assert_eq!(w.len(), 1);
}

#[test]
fn no_handle_finds_a_reserved_slot_whatever_its_generation() {
    let mut w = World::new();
    let mut c = w.commands();
    // A reused slot, set aside at apply for the next spawn, and a new
    // index.
    let old = w.spawn((Pos(0.0, 0.0),));
    w.despawn(old);
    c.spawn((Pos(0.0, 0.0),));
    w.apply(&mut c);
    let reused = c.spawn((Pos(0.0, 0.0),));
    let fresh = c.spawn((Pos(0.0, 0.0),));
    assert_eq!(reused.index(), old.index());
    for e in [reused, fresh] {
        for g in [0, 1, generation(e), u32::MAX - 1, u32::MAX] {
            let made_up = at(e.index(), g);
            assert!(!w.is_alive(made_up), "{made_up:?}");
            assert!(w.get::<Pos>(made_up).is_none());
            assert!(!w.despawn(made_up));
            assert!(!w.insert(made_up, Tag));
        }
    }
    w.apply(&mut c);
    assert!(w.is_alive(reused) && w.is_alive(fresh));
    assert_eq!(w.len(), 3);
}

#[test]
fn reserved_ids_never_collide_with_spawns_made_directly() {
    let mut w = World::new();
    let mut c = w.commands();
    let mut ids = HashSet::new();
    let mut later = Vec::new();
    for round in 0..20 {
        for i in 0..10 {
            later.push(c.spawn((Pos(i as f32, round as f32),)));
            let now = w.spawn((Vel(i as f32, 0.0),));
            assert!(ids.insert(now), "{now:?} handed out twice");
            if i % 3 == 0 {
                w.despawn(now);
            }
        }
        for &e in &later {
            assert!(ids.insert(e), "{e:?} handed out twice");
        }
        w.apply(&mut c);
        for e in later.drain(..) {
            assert!(w.is_alive(e) && w.get::<Pos>(e).is_some());
            if round % 2 == 0 {
                c.despawn(e);
            }
        }
        w.apply(&mut c);
    }
}

#[test]
fn a_dropped_buffer_frees_its_ids_and_their_handles_stay_dead() {
    let mut w = World::new();
    let dropped: Vec<Entity> = {
        let mut c = w.commands();
        (0..4).map(|i| c.spawn((Pos(i as f32, 0.0),))).collect()
    };
    let mut c = w.commands();
    let again: Vec<Entity> = (0..4).map(|i| c.spawn((Pos(i as f32, 0.0),))).collect();
    w.apply(&mut c);
    for (old, new) in dropped.iter().zip(&again) {
        assert!(!w.is_alive(*old), "{old:?} came back to life");
        assert!(w.is_alive(*new));
    }
    // The slots were reused, one generation on.
    let olds: HashSet<u32> = dropped.iter().map(|e| e.index()).collect();
    assert!(again.iter().all(|e| olds.contains(&e.index()) && generation(*e) == 1));
    assert_eq!(w.entity_slots(), 4);
}

#[test]
fn a_cleared_buffer_frees_its_ids() {
    let mut w = World::new();
    let mut c = w.commands();
    let gone = c.spawn((Pos(0.0, 0.0),));
    c.clear();
    let kept = c.spawn((Pos(1.0, 0.0),));
    w.apply(&mut c);
    assert!(!w.is_alive(gone));
    assert!(w.is_alive(kept));
    assert_eq!((gone.index(), generation(kept)), (kept.index(), generation(gone) + 1));
}

#[test]
fn a_spawn_that_panics_frees_its_id() {
    let mut w = World::new();
    let mut c = w.commands();
    let bad = c.spawn((Pos(0.0, 0.0), Pos(1.0, 1.0)));
    let after = c.spawn((Tag,));
    let caught = catch_unwind(AssertUnwindSafe(|| w.apply(&mut c)));
    assert!(caught.is_err());
    assert!(!w.is_alive(bad) && !w.is_alive(after));
    assert!(c.is_empty());
    // Both ids come back, one generation on; their old handles stay dead.
    let next: Vec<Entity> = (0..2).map(|_| c.spawn((Tag,))).collect();
    w.apply(&mut c);
    for e in [bad, after] {
        assert!(next.contains(&at(e.index(), generation(e) + 1)));
    }
    assert_eq!(w.len(), 2);
    assert_eq!(w.entity_slots(), 2);
}

#[test]
fn a_panicking_command_frees_the_ids_of_the_spawns_after_it() {
    let mut w = World::new();
    let mut c = w.commands();
    let first = c.spawn((Tag,));
    c.add(|_| panic!("command failed"));
    let skipped = c.spawn((Tag,));
    assert!(catch_unwind(AssertUnwindSafe(|| w.apply(&mut c))).is_err());
    assert!(w.is_alive(first));
    assert!(!w.is_alive(skipped));
    let reused = c.spawn((Tag,));
    assert_eq!(reused, at(skipped.index(), generation(skipped) + 1));
}

#[test]
fn spawning_and_despawning_through_commands_reuses_slots() {
    let mut w = World::new();
    let mut c = w.commands();
    let mut live = Vec::new();
    let mut frame = |w: &mut World| {
        for e in live.drain(..) {
            c.despawn(e);
        }
        for i in 0..100 {
            live.push(c.spawn((Pos(i as f32, 0.0),)));
        }
        w.apply(&mut c);
        w.return_idle_ids();
    };
    for _ in 0..20 {
        frame(&mut w);
    }
    let settled = w.entity_slots();
    // Fewer under Miri, which is slow.
    for _ in 0..if cfg!(miri) { 30 } else { 500 } {
        frame(&mut w);
    }
    assert_eq!(w.len(), 100);
    assert_eq!(w.entity_slots(), settled, "the slot table keeps growing");
    // The live ones and the ones set aside for the next frame.
    assert!(settled <= 300, "slot table grew to {settled}");
}

#[test]
fn slots_set_aside_for_commands_go_back_to_the_world_once_unused() {
    let mut w = World::new();
    let mut c = w.commands();
    let mut live = Vec::new();
    for _ in 0..50 {
        for e in live.drain(..) {
            c.despawn(e);
        }
        live.extend((0..100).map(|_| c.spawn((Tag,))));
        w.apply(&mut c);
        w.return_idle_ids();
    }
    // The game stops spawning through commands.
    for e in live {
        w.despawn(e);
    }
    w.return_idle_ids();
    // One frame isn't enough: a buffer may still want them.
    let slots = w.entity_slots();
    w.spawn_batch((0..slots).map(|_| (Tag,)));
    assert!(w.entity_slots() > slots, "returned after one idle frame");
    w.clear();
    for _ in 0..2 {
        w.return_idle_ids();
    }
    let slots = w.entity_slots();
    w.spawn_batch((0..slots).map(|_| (Tag,)));
    assert_eq!(w.entity_slots(), slots, "the world reuses every slot");
}

#[test]
fn a_buffer_that_spawns_now_and_then_keeps_the_slot_table_bounded() {
    let mut w = World::new();
    let mut c = w.commands();
    let mut live = Vec::new();
    let mut slots_at = Vec::new();
    // Shorter under Miri, which is slow: the window then needs fewer
    // doublings to cover the gap between bursts.
    let (frames, every) = if cfg!(miri) { (200, 5) } else { (2000, 25) };
    for frame in 0..frames {
        for e in live.drain(..) {
            c.despawn(e);
        }
        // A burst every few frames, despawned the frame after.
        if frame % every == 0 {
            live.extend((0..100).map(|_| c.spawn((Tag,))));
        }
        w.apply(&mut c);
        w.return_idle_ids();
        slots_at.push(w.entity_slots());
    }
    let settled = slots_at[frames / 4];
    assert_eq!(slots_at[frames - 1], settled, "the slot table keeps growing");
    // A few bursts take new indices while the pool learns to wait for the
    // next one.
    assert!(settled <= 1000, "slot table grew to {settled}");
    // It still gives them back once the bursts stop.
    for e in live {
        w.despawn(e);
    }
    for _ in 0..200 {
        w.return_idle_ids();
    }
    w.spawn_batch((0..settled).map(|_| (Tag,)));
    assert_eq!(w.entity_slots(), settled, "the world reuses every slot");
}

#[test]
fn buffers_on_several_threads_never_hand_out_the_same_id() {
    const THREADS: usize = 8;
    const EACH: usize = 500;
    let mut w = World::new();
    // Some slots to reuse, set aside by an applied buffer.
    let mut warm = w.commands();
    let warmed: Vec<Entity> = (0..200).map(|_| warm.spawn((Tag,))).collect();
    w.apply(&mut warm);
    for e in warmed {
        w.despawn(e);
    }
    let mut buffers: Vec<Commands> = (0..THREADS).map(|_| w.commands()).collect();
    let (ids, direct) = std::thread::scope(|s| {
        let tasks: Vec<_> = buffers
            .iter_mut()
            .enumerate()
            .map(|(t, c)| {
                s.spawn(move || {
                    (0..EACH)
                        .map(|i| c.spawn((Pos(t as f32, i as f32),)))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        // The world keeps spawning and despawning meanwhile.
        let mut direct = Vec::new();
        for i in 0..1000 {
            let e = w.spawn((Vel(i as f32, 0.0),));
            if i % 2 == 0 {
                w.despawn(e);
            } else {
                direct.push(e);
            }
        }
        let ids: Vec<Entity> = tasks.into_iter().flat_map(|t| t.join().unwrap()).collect();
        (ids, direct)
    });
    let unique: HashSet<Entity> = ids.iter().chain(&direct).copied().collect();
    assert_eq!(unique.len(), THREADS * EACH + direct.len());
    let indices: HashSet<u32> = ids.iter().chain(&direct).map(|e| e.index()).collect();
    assert_eq!(indices.len(), unique.len(), "two live entities share a slot");
    for c in &mut buffers {
        w.apply(c);
    }
    assert!(ids.iter().all(|&e| w.get::<Pos>(e).is_some()));
    assert!(direct.iter().all(|&e| w.get::<Vel>(e).is_some()));
    assert_eq!(w.len(), THREADS * EACH + direct.len());
}

/// How many parents [`parents`] makes: fewer under Miri, which is slow.
#[cfg(feature = "parallel")]
const PARENTS: usize = if cfg!(miri) { 200 } else { 2000 };

/// A world of `PARENTS` parents and a buffer whose next spawns take slots
/// set aside for it, then new ones.
#[cfg(feature = "parallel")]
fn parents() -> (World, Commands) {
    let mut w = World::new();
    let mut c = w.commands();
    let old: Vec<Entity> = (0..PARENTS / 4).map(|i| c.spawn((Pos(i as f32, 1.0),))).collect();
    w.apply(&mut c);
    for e in old.into_iter().step_by(2) {
        c.despawn(e);
    }
    w.apply(&mut c);
    // Sets the freed slots aside for the buffer's next spawns.
    w.return_idle_ids();
    w.spawn_batch((0..PARENTS).map(|i| (Pos(i as f32, 0.0),)));
    (w, c)
}

/// Every parent's link to the child spawned for it, as id bits, sorted.
#[cfg(feature = "parallel")]
fn links(w: &mut World) -> Vec<(u64, u64)> {
    let mut links = Vec::new();
    w.query::<(Entity, &Target)>()
        .for_each(|(e, t)| links.push((e.to_bits(), t.0.to_bits())));
    links.sort_unstable();
    links
}

#[cfg(feature = "parallel")]
fn on_threads<R: Send>(threads: usize, f: impl FnOnce() -> R + Send) -> R {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap()
        .install(f)
}

/// The promise for spawning from inside a parallel query: every spawn gets
/// its own id and is applied, and the links made with those ids hold. Which
/// item gets which id depends on the order the threads take the lock.
#[cfg(feature = "parallel")]
#[test]
fn spawns_from_a_parallel_query_get_distinct_ids_and_all_apply_on_any_thread_count() {
    use std::sync::Mutex;
    for threads in [1, 2, 8] {
        on_threads(threads, || {
            let (mut w, c) = parents();
            let before = w.len();
            let shared = Mutex::new(c);
            w.query::<(Entity, &Pos)>().par_for_each(|(e, p)| {
                if p.1 == 0.0 {
                    let mut c = shared.lock().unwrap();
                    let child = c.spawn((Vel(p.0, 0.0),));
                    c.insert(e, Target(child));
                }
            });
            let mut c = shared.into_inner().unwrap();
            w.apply(&mut c);
            assert_eq!(w.len(), before + PARENTS);
            let links = links(&mut w);
            assert_eq!(links.len(), PARENTS);
            let children: HashSet<u64> = links.iter().map(|l| l.1).collect();
            assert_eq!(children.len(), PARENTS, "every parent got its own child");
            for (parent, child) in links {
                let x = w.get::<Pos>(Entity::from_bits(parent)).unwrap().0;
                assert_eq!(w.get::<Vel>(Entity::from_bits(child)), Some(&Vel(x, 0.0)));
            }
        });
    }
}

/// The deterministic way to spawn for the items of a parallel query:
/// compute in parallel, collect in query order, spawn in that order. The
/// ids are the same on every run and any number of threads.
#[test]
fn the_same_spawns_get_the_same_ids_through_one_buffer_or_several() {
    // As the engine does: one at a time, systems share the game's buffer;
    // scheduled, each view system has its own. Buffers take ids in chunks
    // sized by what they spawned before, so the chunks differ.
    fn run(shared: bool) -> Vec<Entity> {
        let mut w = World::new();
        let mut buffers = [w.commands(), w.commands()];
        let mut ids = Vec::new();
        for frame in 0..12 {
            for (system, n) in [(0, 1), (1, 2 + frame % 3)] {
                let c = &mut buffers[if shared { 0 } else { system }];
                for _ in 0..n {
                    ids.push(c.spawn((Pos(0.0, 0.0),)));
                }
                w.apply(c);
            }
            ids.push(w.spawn((Pos(1.0, 0.0),)));
            if frame % 2 == 1 {
                let old = ids.len() - 7;
                for &e in &ids[old..old + 4] {
                    w.despawn(e);
                }
            }
            w.return_idle_ids();
        }
        ids
    }
    let shared = run(true);
    assert_eq!(shared, run(false));
    assert!(
        shared.iter().any(|e| generation(*e) > 0),
        "slots were reused, or nothing was tested"
    );
}

#[cfg(feature = "parallel")]
#[test]
fn spawns_made_in_order_after_a_parallel_collect_get_the_same_ids_on_any_thread_count() {
    let run = |threads| {
        on_threads(threads, || {
            let (mut w, mut c) = parents();
            let mut wanted = Vec::new();
            w.query::<(Entity, &Pos)>()
                .par_collect_into(&mut wanted, |(e, p)| (p.1 == 0.0).then_some((e, p.0 * 2.0)));
            for (parent, x) in wanted.into_iter().flatten() {
                let child = c.spawn((Vel(x, 0.0),));
                c.insert(parent, Target(child));
            }
            w.apply(&mut c);
            links(&mut w)
        })
    };
    let reference = run(1);
    assert_eq!(reference.len(), PARENTS);
    assert!(
        reference.iter().any(|l| generation(Entity::from_bits(l.1)) > 0),
        "some children reuse slots"
    );
    for threads in [1, 2, 8] {
        for _ in 0..3 {
            assert!(run(threads) == reference, "{threads} threads gave other ids");
        }
    }
}

#[test]
#[should_panic(expected = "Commands::spawn needs a buffer tied to a world")]
fn spawning_into_a_buffer_tied_to_no_world_panics() {
    let mut c = Commands::new();
    c.spawn((Tag,));
}

#[test]
fn applying_a_buffer_ties_it_to_that_world() {
    let mut w = World::new();
    let mut c = Commands::new();
    assert!(!c.is_bound_to(&w));
    c.add(|w| w.insert_resource(1u32));
    w.apply(&mut c);
    assert!(c.is_bound_to(&w));
    let e = c.spawn((Tag,));
    w.apply(&mut c);
    assert!(w.is_alive(e));
}

#[test]
fn a_buffer_moves_to_a_replaced_world_once_applied_there() {
    let mut w = World::new();
    let mut c = w.commands();
    w = World::new();
    c.add(|w| w.insert_resource(2u32));
    w.apply(&mut c);
    let e = c.spawn((Tag,));
    w.apply(&mut c);
    assert!(w.is_alive(e));
}

#[test]
#[should_panic(expected = "commands tied to one world were applied to another")]
fn applying_spawns_to_another_world_panics() {
    let a = World::new();
    let mut b = World::new();
    let mut c = a.commands();
    c.spawn((Tag,));
    b.apply(&mut c);
}

#[test]
#[should_panic(expected = "can't be applied after it")]
fn spawns_recorded_before_a_reset_panic_when_applied_after_it() {
    let mut w = World::new();
    let mut c = w.commands();
    c.spawn((Tag,));
    w.reset();
    w.apply(&mut c);
}

#[test]
fn spawns_recorded_after_a_reset_apply_normally() {
    let mut w = World::new();
    let mut c = w.commands();
    let old = c.spawn((Tag,));
    w.reset();
    c.clear();
    let new = c.spawn((Tag,));
    let direct = w.spawn((Pos(0.0, 0.0),));
    w.apply(&mut c);
    assert!(w.is_alive(new) && w.is_alive(direct));
    assert_ne!(new, direct);
    assert_eq!(w.len(), 2);
    // `old` was reserved in the old numbering; after a reset ids start over,
    // as for every other entity handle.
    let _ = old;
}

#[test]
fn spawn_at_takes_back_a_slot_set_aside_for_commands_but_not_one_a_buffer_holds() {
    let mut w = World::new();
    let freed: Vec<Entity> = (0..4).map(|_| w.spawn((Tag,))).collect();
    for &e in &freed {
        w.despawn(e);
    }
    let mut c = w.commands();
    for _ in 0..4 {
        c.spawn((Tag,));
    }
    // Applying four spawns sets four free slots aside for the next ones.
    w.apply(&mut c);
    // A buffer that hasn't spawned yet takes one at a time.
    let mut c = w.commands();
    let held = c.spawn((Tag,));
    assert!(
        freed.iter().any(|e| e.index() == held.index()),
        "taken from the slots set aside"
    );
    let pooled: Vec<Entity> = freed
        .iter()
        .filter(|e| e.index() != held.index())
        .map(|&e| at(e.index(), generation(e) + 1))
        .collect();
    assert_eq!(pooled.len(), 3);
    assert!(!w.spawn_at(held, (Pos(0.0, 0.0),)), "a buffer holds it");
    for &e in &pooled {
        assert!(
            !w.spawn_at(at(e.index(), 0), (Pos(0.0, 0.0),)),
            "a despawned handle stays dead"
        );
        assert!(
            w.spawn_at(e, (Pos(0.0, 0.0),)),
            "{e:?} waits in the pool, so it is free"
        );
    }
    w.apply(&mut c);
    assert!(w.is_alive(held) && w.get::<Tag>(held).is_some());
    assert_eq!(w.len(), 8);
    // The buffer's next spawns don't hand out a slot spawn_at took.
    let next: Vec<Entity> = (0..4).map(|_| c.spawn((Tag,))).collect();
    w.apply(&mut c);
    assert!(next.iter().all(|n| pooled.iter().all(|p| p.index() != n.index())));
    assert_eq!(w.len(), 12);
}

#[test]
fn a_refused_spawn_at_leaves_a_slot_set_aside_free_for_later_spawns() {
    let mut w = World::new();
    let freed = w.spawn((Tag,));
    w.despawn(freed);
    let mut c = w.commands();
    c.spawn((Tag,));
    w.apply(&mut c);
    // `freed`'s slot waits for the buffer's next spawn; a stale handle
    // can't take it, but the refusal must not lose the slot.
    assert!(!w.spawn_at(freed, (Pos(0.0, 0.0),)));
    let slots = w.entity_slots();
    let reused = w.spawn((Pos(1.0, 0.0),));
    assert_eq!(reused, at(freed.index(), 1));
    assert_eq!(w.entity_slots(), slots);
}

#[test]
fn spawn_at_skips_indices_a_buffer_took_past_the_end() {
    let mut w = World::new();
    let mut c = w.commands();
    let reserved = c.spawn((Tag,));
    assert_eq!(reserved.index(), 0);
    assert!(!w.spawn_at(reserved, (Pos(0.0, 0.0),)));
    assert!(w.spawn_at(at(3, 0), (Pos(3.0, 0.0),)));
    let fresh = w.spawn((Pos(9.0, 0.0),));
    assert!(fresh.index() != 0 && fresh.index() != 3);
    w.apply(&mut c);
    assert!(w.is_alive(reserved));
    assert_eq!(w.len(), 3);
}

#[test]
fn direct_spawns_fill_new_indices_below_one_a_buffer_took() {
    let mut w = World::new();
    let first = w.spawn((Tag,));
    // The buffer's new index lies past the ones the world keeps for its own
    // spawns; the table grows over them.
    let mut c = w.commands();
    let deferred = c.spawn((Pos(1.0, 0.0),));
    w.apply(&mut c);
    let slots = w.entity_slots();
    assert!(slots > 2);
    let direct: Vec<Entity> = (0..slots + 10).map(|i| w.spawn((Pos(i as f32, 2.0),))).collect();
    let mut all: HashSet<Entity> = direct.iter().copied().collect();
    assert_eq!(all.len(), direct.len());
    assert!(all.insert(first) && all.insert(deferred), "no collisions");
    assert_eq!(w.get::<Pos>(deferred), Some(&Pos(1.0, 0.0)));
    for (i, &e) in direct.iter().enumerate() {
        assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, 2.0)));
    }
    assert_eq!(w.entity_slots(), w.len(), "no slot left unused");
}

#[test]
fn spawn_at_a_new_index_frees_the_new_ones_before_it() {
    let mut w = World::new();
    let first = w.spawn((Tag,));
    assert!(w.spawn_at(at(10, 0), (Pos(10.0, 0.0),)));
    assert_eq!(w.entity_slots(), 11);
    // Indices 1 to 9 are free slots now, and the next new one is 11.
    let more: Vec<Entity> = (0..10).map(|_| w.spawn((Tag,))).collect();
    let mut indices: Vec<u32> = more.iter().map(|e| e.index()).collect();
    indices.sort_unstable();
    assert_eq!(indices, [1, 2, 3, 4, 5, 6, 7, 8, 9, 11]);
    assert!(w.is_alive(first) && w.get::<Pos>(at(10, 0)).is_some());
    assert!(!w.spawn_at(at(10, 0), (Tag,)), "taken");
}
