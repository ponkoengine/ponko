//! One heap per thread, for web builds with threads that ask for it with
//! `RUSTFLAGS="--cfg ponko_web_thread_heaps"`.
//!
//! The standard library's allocator for `wasm32-unknown-unknown` keeps one
//! heap behind one spin lock (a browser's page thread may not block), so
//! threads that allocate at the same time wait for each other. This one
//! gives each thread its own heap (talc, behind a lock of its own), picked
//! on the thread's first allocation; a block freed on another thread goes
//! back to the heap it came from.
//!
//! It isn't the default because in the threads fixture it made allocation
//! on the page's thread, where systems run, slower: a world's JSON save
//! there took 61 ms instead of 41 ms, parsing JSON 15 ms instead of 13 ms.
//! What it buys is work that allocates on the workers: a parallel query
//! allocating on every item took 1.6 ms instead of 78 ms, and the JSON save
//! 40 ms instead of 700 ms (so a build with it builds saves on the frame
//! pool). The build is about 3% larger with brotli. The numbers are in the
//! guide's "Threads on the web".
//!
//! A worker's first two allocations (its stack and its thread-local block,
//! made by wasm-bindgen's start code) come before that thread has its
//! thread-local storage, so the heap they come from is read at the bottom of
//! the memory, the deepest end of the page thread's stack, which is normally
//! untouched (zero: the first heap).

/// Heaps: enough for the frame pool (at most 16 threads), the job workers
/// (at most 8) and the page's thread, each with one of its own. Threads past
/// that share heaps round-robin.
const HEAPS: usize = 32;

#[global_allocator]
static ALLOCATOR: orx_parallel_wasm_allocator::WasmParallelAllocator<HEAPS> =
    orx_parallel_wasm_allocator::WasmParallelAllocator::new();
