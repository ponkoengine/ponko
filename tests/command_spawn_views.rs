//! `commands.spawn` returns the new entity in every kind of system: view
//! systems sharing a batch on threads, plain systems, and after the game
//! replaced its world. Headless.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ponko::access::SystemAccess;
use ponko::prelude::*;
use ponko::system_view::SystemView;

#[derive(Component)]
struct Shot(usize);
#[derive(Component)]
struct Owner(Entity);
#[derive(Component, Default)]
struct A;
#[derive(Component, Default)]
struct B;
#[derive(Component, Default)]
struct C;

/// A view system that spawns `n` pairs per tick: a shot, and a marker that
/// points at the shot spawned just before it in the same buffer.
fn shooter<T: Component + Default>(n: usize) -> impl FnMut(&mut SystemView) + Send + 'static {
    move |sys: &mut SystemView| {
        for i in 0..n {
            let shot = sys.commands.spawn((Shot(i),));
            sys.commands.spawn((T::default(), Owner(shot)));
        }
    }
}

#[test]
fn view_systems_on_threads_spawn_distinct_entities_they_can_link() {
    const N: usize = 200;
    const TICKS: u64 = 5;
    let mut world = Game::new("spawn", 1, 1)
        .parallel_systems_env(false)
        .parallel_systems(true)
        .parallel_threshold(Duration::ZERO)
        .add(
            Phase::AfterPhysics,
            shooter::<A>(N).access(SystemAccess::new().commands()),
        )
        .add(
            Phase::AfterPhysics,
            shooter::<B>(N).access(SystemAccess::new().commands()),
        )
        .add(
            Phase::AfterPhysics,
            shooter::<C>(N).access(SystemAccess::new().commands()),
        )
        // Despawns some every tick, so slots are reused.
        .sim_system(|ctx: &mut Ctx| {
            let old: Vec<Entity> = ctx.world.query::<(Entity, &A)>().iter().map(|(e, _)| e).collect();
            for e in old.into_iter().step_by(2) {
                ctx.world.despawn(e);
            }
        })
        .run_headless(TICKS, |_| {});
    assert!(
        world.query::<(&Owner, &A)>().count() < world.query::<(&Owner, &B)>().count(),
        "the despawns ran"
    );
    let owners: Vec<Entity> = world.query::<&Owner>().iter().map(|o| o.0).collect();
    assert_eq!(owners.iter().collect::<HashSet<_>>().len(), owners.len());
    for shot in owners {
        assert!(world.get::<Shot>(shot).is_some(), "each marker points at a live shot");
    }
    assert_eq!(world.query::<&Shot>().count(), 3 * N * TICKS as usize);
}

#[test]
fn a_plain_system_links_what_it_spawned() {
    let mut world = Game::new("spawn", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            let shot = ctx.commands.spawn((Shot(1),));
            ctx.commands.spawn((Owner(shot),));
        })
        .run_headless(3, |_| {});
    let owners: Vec<Entity> = world.query::<&Owner>().iter().map(|o| o.0).collect();
    assert_eq!(owners.len(), 3);
    for shot in owners {
        assert_eq!(world.get::<Shot>(shot).map(|s| s.0), Some(1));
    }
}

#[test]
fn spawns_go_to_a_world_that_replaced_the_old_one() {
    let mut world = Game::new("spawn", 1, 1)
        .sim_system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                ctx.world = World::new();
            }
        })
        .sim_system(|ctx: &mut Ctx| {
            let shot = ctx.commands.spawn((Shot(ctx.time.sim_tick() as usize),));
            ctx.commands.spawn((Owner(shot),));
        })
        .run_headless(4, |_| {});
    let ticks: HashSet<usize> = world.query::<&Shot>().iter().map(|s| s.0).collect();
    assert_eq!(ticks, HashSet::from([1, 2, 3]));
    let owners: Vec<Entity> = world.query::<&Owner>().iter().map(|o| o.0).collect();
    assert_eq!(owners.len(), 3);
    for shot in owners {
        assert!(world.get::<Shot>(shot).is_some());
    }
}

#[test]
fn a_game_that_stops_spawning_through_commands_gives_the_slots_back() {
    let mut live: Vec<Entity> = Vec::new();
    let mut world = Game::new("spawn", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            for e in live.drain(..) {
                ctx.commands.despawn(e);
            }
            // Sparks for 20 ticks, then none.
            if ctx.time.sim_tick() < 20 {
                live.extend((0..50).map(|i| ctx.commands.spawn((Shot(i),))));
            }
        })
        .run_headless(40, |_| {});
    assert_eq!(world.query::<&Shot>().count(), 0);
    let slots = world.entity_slots();
    world.spawn_batch((0..slots).map(|i| (Shot(i),)));
    assert_eq!(world.entity_slots(), slots, "every slot went back to the world");
}

/// Who spawned an entity, in which tick, and which of its spawns it was.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Made {
    by: u32,
    tick: u64,
    n: u32,
}

type Log = Arc<Mutex<Vec<(Made, Entity)>>>;

/// A view system that spawns `n` entities a tick, each pointing at the one
/// before, and logs the ids `spawn` returned.
fn logging_shooter(by: u32, n: u32, log: Log) -> impl FnMut(&mut SystemView) + Send + 'static {
    let mut tick = 0;
    move |sys: &mut SystemView| {
        tick += 1;
        let mut last = None;
        for i in 0..n {
            let made = Made { by, tick, n: i };
            let e = match last {
                Some(prev) => sys.commands.spawn((made, Owner(prev))),
                None => sys.commands.spawn((made,)),
            };
            last = Some(e);
            log.lock().unwrap().push((made, e));
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Off,
    InOrder,
    Threads,
}

/// Three view systems and two plain ones spawn through commands for eight
/// ticks; the plain ones also despawn (directly and through commands), so
/// slots are reused. Returns every id `spawn` handed out, by who made it.
fn spawned_ids(mode: Mode) -> HashMap<Made, Entity> {
    let log: Log = Arc::default();
    let game = Game::new("spawn", 1, 1).parallel_systems_env(false);
    let game = match mode {
        Mode::Off => game.parallel_systems(false),
        Mode::InOrder => game.parallel_systems(true).parallel_threshold(Duration::MAX),
        Mode::Threads => game.parallel_systems(true).parallel_threshold(Duration::ZERO),
    };
    let plain = |by: u32, log: Log| {
        move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            // Old ones go: every third directly, the others' next through commands.
            let old: Vec<(Entity, Made)> = ctx
                .world
                .query::<(Entity, &Made)>()
                .iter()
                .filter(|(_, m)| m.tick + 2 <= tick && (m.n + by).is_multiple_of(3))
                .map(|(e, m)| (e, *m))
                .collect();
            for (e, m) in old {
                if m.by.is_multiple_of(2) {
                    ctx.world.despawn(e);
                } else {
                    ctx.commands.despawn(e);
                }
            }
            for i in 0..(5 + by) {
                let made = Made { by, tick, n: i };
                let e = ctx.commands.spawn((made,));
                log.lock().unwrap().push((made, e));
            }
        }
    };
    let world = game
        .sim_system(plain(10, log.clone()))
        .add(
            Phase::AfterPhysics,
            logging_shooter(1, 40, log.clone()).access(SystemAccess::new().commands()),
        )
        .add(
            Phase::AfterPhysics,
            logging_shooter(2, 7, log.clone()).access(SystemAccess::new().commands()),
        )
        .add(
            Phase::AfterPhysics,
            logging_shooter(3, 120, log.clone()).access(SystemAccess::new().commands()),
        )
        .sim_system(plain(11, log.clone()))
        .run_headless(8, |_| {});
    let log = std::mem::take(&mut *log.lock().unwrap());
    let ids: HashMap<Made, Entity> = log.iter().copied().collect();
    assert_eq!(ids.len(), log.len(), "each spawn logged once");
    for (made, e) in &ids {
        if let Some(m) = world.get::<Made>(*e) {
            assert_eq!(m, made, "the id `spawn` returned is the entity it spawned");
        }
    }
    ids
}

#[test]
fn spawned_ids_are_the_same_with_the_scheduler_on_or_off_and_any_thread_count() {
    let reference = spawned_ids(Mode::Off);
    assert!(reference.len() > 1000);
    let reused = reference.values().filter(|e| (e.to_bits() >> 32) > 0).count();
    assert!(reused > 0, "some spawns reuse despawned slots");
    for mode in [Mode::Off, Mode::InOrder, Mode::Threads] {
        for threads in [1, 2, 8] {
            let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build().unwrap();
            let ids = pool.install(|| spawned_ids(mode));
            assert_eq!(ids.len(), reference.len());
            for (made, e) in &reference {
                assert_eq!(
                    ids.get(made),
                    Some(e),
                    "{mode:?} on {threads} threads gave {made:?} another id"
                );
            }
        }
    }
}
