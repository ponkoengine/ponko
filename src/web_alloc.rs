//! The allocator of web builds with threads: one heap per thread.
//!
//! The standard library's allocator for `wasm32-unknown-unknown` keeps one
//! heap behind one spin lock (a browser's page thread may not block), so
//! threads that allocate at the same time wait for each other: the frame
//! pool's workers built a world's JSON save about 10 times slower than the
//! page's thread alone. This one gives each thread its own heap (talc, behind
//! a lock of its own), picked on the thread's first allocation; a block freed
//! on another thread goes back to the heap it came from. The numbers are in
//! the guide's "Threads on the web".
//!
//! Only web builds with threads have it: the ordinary web build and native
//! builds keep the standard allocator. A game that sets its own
//! `#[global_allocator]` in a build with threads (such as
//! `ponko::perf::CountingAllocator`) builds with
//! `RUSTFLAGS="--cfg ponko_web_system_allocator"`, which leaves this one out.

/// Heaps: enough for the frame pool (at most 16 threads), the job workers
/// (at most 8) and the page's thread, each with one of its own. Threads past
/// that share heaps round-robin.
const HEAPS: usize = 32;

#[global_allocator]
static ALLOCATOR: orx_parallel_wasm_allocator::WasmParallelAllocator<HEAPS> =
    orx_parallel_wasm_allocator::WasmParallelAllocator::new();

/// Whether this build has the per-thread allocator (and so builds saves on
/// the frame pool).
pub(crate) const PER_THREAD: bool = true;
