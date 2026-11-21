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
//! Experimental: the guide's "One heap per thread" says what has been
//! checked. It isn't the default because in the threads fixture it made allocation
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

use std::alloc::{GlobalAlloc, Layout};

static HEAPS_ALLOCATOR: orx_parallel_wasm_allocator::WasmParallelAllocator<HEAPS> =
    orx_parallel_wasm_allocator::WasmParallelAllocator::new();

/// The alignment of every block. The standard allocator of wasm32 aligns
/// every block to 8 bytes whatever is asked, and code that reads a byte
/// buffer as 8-byte values relies on it: the heaps align a block to what is
/// asked behind a 4-byte header, and a page of the threads fixture stopped
/// on such a read (`bytemuck`'s `cast_slice`) after its WebGPU device was
/// lost. Every request asks for at least this much.
const MIN_ALIGN: usize = 8;

/// `layout`, aligned to at least [`MIN_ALIGN`].
fn widened(layout: Layout) -> Layout {
    match layout.align_to(MIN_ALIGN) {
        Ok(wider) => wider,
        // Only a size near `isize::MAX` can't be aligned more; the heaps
        // refuse such a size anyway.
        Err(_) => layout,
    }
}

/// Forwards to the heaps out of line, with every block aligned to
/// [`MIN_ALIGN`]: the allocator lives in the engine's crate, so without
/// this the compiler copied its code into every place that allocates or
/// frees, and the build grew by 2 MB (11%).
struct Heaps;

// SAFETY: every method forwards to the heaps with the caller's layout
// widened the same way ([`widened`]), so a block is freed and grown with
// the layout it was allocated with.
unsafe impl GlobalAlloc for Heaps {
    #[inline(never)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract is the heaps', and a wider
        // alignment still satisfies the caller's.
        unsafe { HEAPS_ALLOCATOR.alloc(widened(layout)) }
    }

    #[inline(never)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as above.
        unsafe { HEAPS_ALLOCATOR.alloc_zeroed(widened(layout)) }
    }

    #[inline(never)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from the heaps with this layout, widened.
        unsafe { HEAPS_ALLOCATOR.dealloc(ptr, widened(layout)) }
    }

    #[inline(never)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as above; the new block keeps the widened alignment.
        unsafe { HEAPS_ALLOCATOR.realloc(ptr, widened(layout), new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Heaps = Heaps;
