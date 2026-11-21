//! Where the engine's own parallel passes run: the frame pool, as parallel
//! queries do.
//!
//! Natively that is rayon's pool, and [`run`] calls its pass directly. In a
//! web build with threads the page's thread may not call into rayon (its
//! global pool is the page's thread alone, and waiting would block), so a
//! pass started there goes to the frame pool's workers through the runner
//! the ECS's parallel queries use, while the page's thread spins. Passes
//! that grow with the world (spatial grid rebuilds, flow fields, 2D skeleton
//! drawing, 3D character physics, the hierarchy) go through [`run`] and
//! size their pieces with [`threads`].

/// Runs `op`, which starts rayon work, on the frame pool and returns its
/// result: here natively (and on a web worker, or a page without threads),
/// through the web runner from the page's thread.
#[inline]
pub(crate) fn run<R: Send>(op: impl FnOnce() -> R + Send) -> R {
    #[cfg(feature = "bench-internals")]
    SHARED_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    return crate::web_threads::on_frame_pool(op);
    #[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
    op()
}

/// The threads a pass started on this thread gets through [`run`]: rayon's
/// natively, the frame pool's on a web page with threads, 1 on a web page
/// without. A pass cuts its work into pieces for this many.
#[inline]
pub(crate) fn threads() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    return rayon::current_num_threads();
    #[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
    return if crate::web_threads::on_worker() {
        rayon::current_num_threads()
    } else {
        crate::web_threads::frame_threads().max(1)
    };
    // Without threads rayon isn't asked: that would set up a pool of the
    // page's own thread for nothing.
    #[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
    return 1;
}

/// Whether the engine's passes with a size threshold share their work out
/// to the frame pool: by their thresholds (the default), never, or always.
/// For the threshold sweeps of the web threads fixture
/// (`examples/web_threads.rs`, `--case thresholds`), which time both sides
/// at each size. Unstable (`bench-internals`).
#[doc(hidden)]
#[cfg(feature = "bench-internals")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sharing {
    /// Each pass decides by its threshold.
    Auto,
    /// Every pass stays on the calling thread.
    Never,
    /// Every pass is shared out, however small.
    Always,
}

#[cfg(feature = "bench-internals")]
static SHARING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Sets how the passes with a size threshold decide (see [`Sharing`]), for
/// every thread. Unstable (`bench-internals`).
#[doc(hidden)]
#[cfg(feature = "bench-internals")]
pub fn set_sharing(sharing: Sharing) {
    let v = match sharing {
        Sharing::Auto => 0,
        Sharing::Never => 1,
        Sharing::Always => 2,
    };
    SHARING.store(v, std::sync::atomic::Ordering::Relaxed);
}

/// Whether a pass shares its work out: `by_threshold`, the pass's own
/// decision, unless a benchmark forced it ([`set_sharing`]).
#[inline]
pub(crate) fn share(by_threshold: impl FnOnce() -> bool) -> bool {
    #[cfg(feature = "bench-internals")]
    match SHARING.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    by_threshold()
}

#[cfg(feature = "bench-internals")]
static SHARED_RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Passes handed to the frame pool ([`run`]) so far, on any thread: the
/// difference around a call says whether it shared its work out.
/// Unstable (`bench-internals`).
#[doc(hidden)]
#[cfg(feature = "bench-internals")]
pub fn shared_runs() -> u64 {
    SHARED_RUNS.load(std::sync::atomic::Ordering::Relaxed)
}
