//! A parallel loop carries its workers' per-thread counters back to the
//! thread that started it, and only the loop's work. Its own test binary:
//! the meter is registered once per process.
#![cfg(feature = "parallel")]

use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use ponko_ecs::*;

thread_local! {
    static WORK: Cell<u64> = const { Cell::new(0) };
}
static ON: AtomicBool = AtomicBool::new(true);

fn on() -> bool {
    ON.load(Relaxed)
}
fn read() -> [u64; 3] {
    [WORK.with(Cell::get), 0, 0]
}
fn credit([n, _, _]: [u64; 3]) {
    WORK.with(|w| w.set(w.get() + n));
}
/// Registers the meter, and keeps the tests apart (one turns it off).
fn install() -> std::sync::MutexGuard<'static, ()> {
    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
    let _ = set_thread_meter(ThreadMeter { on, read, credit });
    ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}
fn work() {
    WORK.with(|w| w.set(w.get() + 1));
}
fn mine() -> u64 {
    WORK.with(Cell::get)
}

#[derive(Component)]
struct Pos(f32);

const N: usize = 20_000;

fn world() -> World {
    let mut world = World::new();
    world.spawn_batch((0..N).map(|i| (Pos(i as f32),)));
    world
}

#[test]
fn a_parallel_loop_credits_its_workers_work_to_the_thread_that_started_it() {
    let _one = install();
    let mut world = world();
    // Started on a thread outside the pool (the test's) and on one of the
    // pool's own threads, where rayon runs some pieces on the starter.
    let starters: [&dyn Fn(&mut World) -> u64; 2] = [
        &|w: &mut World| {
            let before = mine();
            w.query::<&Pos>().par_for_each(|_| work());
            mine() - before
        },
        &|w: &mut World| {
            rayon::scope(|_| {
                let before = mine();
                w.query::<&Pos>().par_for_each(|_| work());
                mine() - before
            })
        },
    ];
    for start in starters {
        assert_eq!(start(&mut world), N as u64, "every item once, on whichever thread");
    }
    let before = mine();
    let mut out = Vec::new();
    world.query::<&Pos>().par_collect_into(&mut out, |p| {
        work();
        p.0
    });
    assert_eq!(mine() - before, N as u64);
    let before = mine();
    let mut strings = Vec::new();
    world.query::<&Pos>().par_collect_into(&mut strings, |p| {
        work();
        p.0.to_string()
    });
    assert_eq!(mine() - before, N as u64, "the path for results that need dropping");
}

#[test]
fn work_the_pool_does_outside_the_loop_is_not_credited() {
    let _one = install();
    let before = mine();
    // The negative control: rayon work that no loop of the crate started.
    rayon::broadcast(|_| work());
    assert_eq!(mine() - before, 0);
    // The same work as pieces of a loop: each worker's piece, once.
    let tally = LoopTally::start();
    rayon::broadcast(|_| tally.count(work));
    assert_eq!(mine() - before, 0, "credited when the loop is done");
    tally.finish();
    assert_eq!(mine() - before, rayon::current_num_threads() as u64);
}

#[test]
fn nothing_is_credited_while_the_meter_is_off() {
    let _one = install();
    let tally = {
        ON.store(false, Relaxed);
        let t = LoopTally::start();
        ON.store(true, Relaxed);
        t
    };
    let before = mine();
    rayon::broadcast(|_| tally.count(work));
    let here = mine() - before;
    tally.finish();
    assert_eq!(mine() - before, here, "only this thread's own work");
}
