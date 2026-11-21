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
