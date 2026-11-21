//! Where the crate's parallel loops run (feature `parallel`).
//!
//! Natively they run on rayon's pool: the calling thread's, else the global
//! one. A browser's main thread may never block (not even on a lock), and
//! calling into rayon from it would. So a web build with threads (wasm32
//! with the `atomics` target feature) lets the engine register a
//! [`WebRunner`]: a parallel loop started on a thread outside the runner's
//! pool is handed to the runner, which runs it on the pool's workers while
//! the calling thread spins. Without a runner, or on one of its threads,
//! loops run exactly as they do natively. Everywhere else this module
//! compiles to direct calls.
//!
//! A loop also carries per-thread counters back to the thread that started
//! it (see [`ThreadMeter`]): the engine's allocation counts, so a system
//! that runs a parallel loop is charged for what its workers allocate.

#[cfg(all(target_family = "wasm", target_feature = "atomics"))]
pub use web::{WebRunner, set_web_runner};

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Three counters that each thread keeps for itself (the engine's: the
/// allocations made on the thread, the bytes they asked for and the bytes
/// freed), and that the crate's parallel loops carry back to the thread
/// that started them.
///
/// Each piece of a loop that runs on another thread reads the counters
/// before and after it runs, and when the loop is done the starting thread
/// is credited with the sum. So code that reads its own thread's counters
/// around a [`par_for_each`](crate::Query::par_for_each) sees what the
/// loop's work did on every thread, and nothing the thread pool did for
/// itself meanwhile (a worker parking, an idle worker's bookkeeping). The
/// cost is two calls of `read` per piece of a loop (a few per thread), and
/// nothing at all while `on` says no.
///
/// Covered: [`Query::par_for_each`](crate::Query::par_for_each),
/// [`par_for_each_indexed`](crate::Query::par_for_each_indexed),
/// [`par_collect_into`](crate::Query::par_collect_into) and
/// [`ViewGroup::run`](crate::ViewGroup::run) on threads. A loop written
/// directly with rayon is not: its work counts on the threads that run it.
/// A worker that picks up unrelated work while it waits inside a piece of
/// the loop (rayon may do that) charges it to the loop.
#[derive(Clone, Copy, Debug)]
pub struct ThreadMeter {
    /// Whether counting is on right now; checked once per loop.
    pub on: fn() -> bool,
    /// The calling thread's counters.
    pub read: fn() -> [u64; 3],
    /// Adds to the calling thread's counters what other threads did for it.
    pub credit: fn([u64; 3]),
}

static METER: OnceLock<ThreadMeter> = OnceLock::new();

/// Registers the meter every parallel loop carries counters back with;
/// `false` if one was registered before (the first one stays).
///
/// ```
/// fn on() -> bool { false }
/// fn read() -> [u64; 3] { [0, 0, 0] }
/// fn credit(_: [u64; 3]) {}
/// ponko_ecs::set_thread_meter(ponko_ecs::ThreadMeter { on, read, credit });
/// ```
pub fn set_thread_meter(meter: ThreadMeter) -> bool {
    METER.set(meter).is_ok()
}

thread_local! {
    /// Its address tells threads apart without allocating.
    static HERE: u8 = const { 0 };
}

fn this_thread() -> usize {
    HERE.with(|h| std::ptr::from_ref(h) as usize)
}

/// One parallel loop's counters from other threads: start it on the thread
/// that starts the loop, wrap each piece of the loop in [`count`](Self::count),
/// and [`finish`](Self::finish) it on the starting thread once the loop is
/// done. Does nothing unless a [`ThreadMeter`] is registered and on.
///
/// ```
/// use rayon::prelude::*;
/// let tally = ponko_ecs::LoopTally::start();
/// (0..4).into_par_iter().for_each(|i| {
///     tally.count(|| std::hint::black_box(i));
/// });
/// tally.finish();
/// ```
pub struct LoopTally {
    meter: Option<ThreadMeter>,
    home: usize,
    sums: [AtomicU64; 3],
}

impl LoopTally {
    /// A tally for a loop the calling thread is about to start.
    #[inline]
    pub fn start() -> Self {
        let meter = METER.get().copied().filter(|m| (m.on)());
        Self {
            home: if meter.is_some() { this_thread() } else { 0 },
            meter,
            sums: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
        }
    }

    /// Runs one piece of the loop, on any thread, counting what it did
    /// there when that isn't the starting thread (whose counters already
    /// have it).
    #[inline]
    pub fn count<R>(&self, op: impl FnOnce() -> R) -> R {
        let Some(meter) = self.meter else { return op() };
        if this_thread() == self.home {
            return op();
        }
        let before = (meter.read)();
        let out = op();
        let after = (meter.read)();
        for (sum, (a, b)) in self.sums.iter().zip(after.into_iter().zip(before)) {
            let d = a.wrapping_sub(b);
            if d != 0 {
                sum.fetch_add(d, Relaxed);
            }
        }
        out
    }

    /// Credits the starting thread with what the loop did on other threads.
    /// Call it on the thread that called [`start`](Self::start).
    #[inline]
    pub fn finish(self) {
        if let Some(meter) = self.meter {
            let sums = self.sums.each_ref().map(|s| s.load(Relaxed));
            if sums != [0, 0, 0] {
                (meter.credit)(sums);
            }
        }
    }
}

/// How many threads a parallel loop started on this thread may use.
#[inline]
pub(crate) fn threads() -> usize {
    #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
    if let Some(runner) = web::runner_for_this_thread() {
        return runner.threads.max(1);
    }
    rayon::current_num_threads()
}

/// Runs `op`, which starts rayon work, where that work may run, and
/// returns its result: here (natively, and on a thread of the web runner's
/// pool), or through the web runner.
#[inline]
pub(crate) fn run<R: Send>(op: impl FnOnce() -> R + Send) -> R {
    #[cfg(all(target_family = "wasm", target_feature = "atomics"))]
    if let Some(runner) = web::runner_for_this_thread() {
        return web::run_with(runner, op);
    }
    op()
}

#[cfg(all(target_family = "wasm", target_feature = "atomics"))]
mod web {
    use std::sync::OnceLock;

    /// How a web build with threads runs the crate's parallel loops when
    /// they start on a thread that may not block (the page's own).
    ///
    /// The engine registers one with [`set_web_runner`] once its worker
    /// threads are up.
    #[derive(Clone, Copy, Debug)]
    pub struct WebRunner {
        /// The threads of the runner's pool: how many pieces a loop is cut
        /// into.
        pub threads: usize,
        /// Whether the calling thread is one of the pool's own. Loops
        /// started there use rayon directly, as on the desktop.
        pub in_pool: fn() -> bool,
        /// Calls `op` once on the pool and returns after it has returned.
        /// The calling thread must not block while it waits.
        pub run: fn(op: &mut (dyn FnMut() + Send)),
    }

    static RUNNER: OnceLock<WebRunner> = OnceLock::new();

    /// Registers the runner; `false` if one was registered before (the
    /// first one stays).
    ///
    /// # Safety
    ///
    /// `runner.run` must call its `op` exactly once and return only after
    /// that call has returned: `op` borrows the caller's stack. `in_pool`
    /// must be `true` only on threads where rayon calls run on the
    /// runner's pool.
    pub unsafe fn set_web_runner(runner: WebRunner) -> bool {
        RUNNER.set(runner).is_ok()
    }

    /// The runner, unless there is none or this thread is one of its pool.
    #[inline]
    pub(super) fn runner_for_this_thread() -> Option<WebRunner> {
        RUNNER.get().copied().filter(|r| !(r.in_pool)())
    }

    /// `op` through `runner`.
    pub(super) fn run_with<R: Send>(runner: WebRunner, op: impl FnOnce() -> R + Send) -> R {
        let mut op = Some(op);
        let mut out = None;
        (runner.run)(&mut || {
            if let Some(op) = op.take() {
                out = Some(op());
            }
        });
        match out {
            Some(out) => out,
            // invariant: `set_web_runner`'s contract makes `run` call `op`
            // before returning.
            None => unreachable!("ponko_ecs: the web runner returned without running the parallel loop"),
        }
    }
}
