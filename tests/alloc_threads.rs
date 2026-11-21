//! A system's allocation count is its own thread's, plus what the workers
//! of its ECS parallel loops allocate for it: another thread allocating at
//! the same time never lands in it. Exact counts, first run, while a
//! thread allocates as fast as it can next to the game.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use ponko::perf::CountingAllocator;
use ponko::prelude::*;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Component)]
struct Mote(u32);

const MOTES: u32 = 5_000;
const TICKS: u64 = 60;

/// Allocates nothing.
fn tidy(ctx: &mut Ctx) {
    ctx.world.query::<&mut Mote>().for_each(|m| m.0 += 1);
}

/// Allocates nothing, on all cores.
fn tidy_parallel(ctx: &mut Ctx) {
    ctx.world.query::<&mut Mote>().par_for_each(|m| m.0 += 1);
}

/// One allocation per mote, on whichever worker runs it.
fn boxes_every_mote(ctx: &mut Ctx) {
    ctx.world.query::<&Mote>().par_for_each(|m| {
        std::hint::black_box(Box::new(m.0));
    });
}

/// Allocates on another thread for as long as the game runs.
struct Noise {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<u64>>,
}

impl Noise {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut made = 0u64;
            while !flag.load(Relaxed) {
                std::hint::black_box(vec![made; 8]);
                made += 1;
            }
            made
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    /// Stops it; how many allocations it made.
    fn stop(mut self) -> u64 {
        self.stop.store(true, Relaxed);
        self.thread.take().map_or(0, |t| t.join().unwrap_or(0))
    }
}

#[test]
fn another_threads_allocations_never_land_in_a_systems_count() {
    let noise = Noise::start();
    let (_world, report) = Game::new("t", 1, 1)
        .sim_system(tidy)
        .sim_system(tidy_parallel)
        .sim_system(boxes_every_mote)
        .profile_headless(10, TICKS, |ctx| {
            ctx.world.spawn_batch((0..MOTES).map(|i| (Mote(i),)));
        });
    let noisy = noise.stop();
    assert!(noisy > 1000, "the other thread allocated all along: {noisy}");
    let allocs = report.allocs.expect("the counting allocator is installed");
    assert!(
        allocs.per_frame > 1000.0,
        "the frame's figure is still the whole process's: {report}"
    );
    // Allocations over the counted ticks.
    let made = |name: &str| {
        let s = report.system(name).unwrap_or_else(|| panic!("{name} ran: {report}"));
        (f64::from(s.allocs_per_frame) * TICKS as f64).round() as u64
    };
    assert_eq!(made("tidy"), 0, "{report}");
    // The only allocation of rayon's own on the starting thread: the queue it
    // hands a loop to takes a new block every 63 loops (two loops a tick
    // here), allocated by whichever loop's push reaches the block's end. A
    // number of loops, not a matter of load.
    let injector_blocks = (2 * (TICKS + 10)).div_ceil(63);
    let parallel = made("tidy_parallel");
    let boxes = made("boxes_every_mote");
    assert!(parallel <= injector_blocks, "{parallel}: {report}");
    let loops = u64::from(MOTES) * TICKS;
    assert!(
        (loops..=loops + injector_blocks - parallel).contains(&boxes),
        "every worker's allocation for the loop counts, once: {boxes}: {report}"
    );
}

#[test]
fn a_threads_net_bytes_leave_out_what_other_threads_hold() {
    let noise = Noise::start();
    let (_world, _report) = Game::new("t", 1, 1).profile_headless(0, 1, |ctx| {
        ctx.perf.set_profiling(true);
        let before = ponko::perf::thread_net_alloc_bytes();
        let block = std::hint::black_box(vec![0u8; 4096]);
        let holding = ponko::perf::thread_net_alloc_bytes();
        drop(block);
        let released = ponko::perf::thread_net_alloc_bytes();
        ctx.perf.set_profiling(false);
        assert_eq!(holding - before, 4096);
        assert_eq!(released, before);
    });
    noise.stop();
}
