//! A regression check for the mixer's per-frame path: a snapshot fading and
//! bus volumes changing every frame allocate nothing.
//!
//! One test per binary, and a thread-local counter: only the thread running
//! the game counts, so other threads in the process can't disturb the
//! zero-allocation assertion.
use ponko::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn fading_snapshots_and_bus_changes_allocate_nothing_per_frame() {
    let mut tick = 0;
    Game::new("allocations", 1, 1)
        .sim_system(move |ctx: &mut Ctx| {
            tick += 1;
            let music = ctx.audio.bus("music");
            if tick == 3 {
                ALLOCS.set(0);
                TRACK.set(true);
            }
            // A slider being dragged: a new volume every frame.
            ctx.audio.set_bus_volume(music, 0.5 + 0.01 * (tick % 10) as f32);
            let _ = ctx.audio.bus_gain(music);
            if tick == 30 {
                TRACK.set(false);
                assert!(ctx.audio.snapshot_weight("underwater") < 1.0, "still fading");
                assert_eq!(ALLOCS.get(), 0, "allocations while the mix moved");
            }
        })
        .run_headless(30, |ctx| {
            ctx.audio
                .apply_mixer_json(
                    r#"{"buses": [{"name": "master"},
                        {"name": "music", "parent": "master", "ducking": [{"key": "dialogue"}]},
                        {"name": "dialogue", "parent": "master"},
                        {"name": "world", "parent": "master", "effects": [{"filter": {}}, {"reverb": {}}]}],
                     "snapshots": [{"name": "underwater", "buses": [
                        {"bus": "world", "volume": 0.5, "effects": [{"index": 0, "effect": {"filter": {"cutoff_hz": 400}}}]},
                        {"bus": "music", "volume": 0.3}]}]}"#,
                )
                .unwrap();
            ctx.audio.start_snapshot("underwater", 10.0);
        });
}
