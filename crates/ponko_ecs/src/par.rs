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

#[cfg(all(target_family = "wasm", target_feature = "atomics"))]
pub use web::{WebRunner, set_web_runner};

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
