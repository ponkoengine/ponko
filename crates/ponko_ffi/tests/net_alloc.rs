//! `ponko_net_alloc_bytes` from C: a leak check a native game writes itself.
//! A file of its own: the counts are for the whole process, so other tests
//! allocating next to it would blur them.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;

#[derive(Default)]
struct Seen {
    balanced: i64,
    leaking: i64,
    worlds: i64,
    leaked_worlds: i64,
    measured: bool,
}

const PASSES: i64 = 50;
const BLOCK: usize = 4096;

unsafe extern "C" fn setup(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe { assert_eq!(ponko_ctx_set_profiling(ctx, true), PONKO_OK) };
}

unsafe extern "C" fn measure(_: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        let seen = &mut *user.cast::<Seen>();
        if seen.measured {
            return;
        }
        seen.measured = true;
        // The game's own memory, reported as PONKO_COUNT_CPP_ALLOCATIONS does.
        let before = ponko_net_alloc_bytes();
        for _ in 0..PASSES {
            ponko_note_allocation(BLOCK);
            ponko_note_deallocation(BLOCK);
        }
        seen.balanced = ponko_net_alloc_bytes() - before;
        let before = ponko_net_alloc_bytes();
        for _ in 0..PASSES {
            ponko_note_allocation(BLOCK);
        }
        seen.leaking = ponko_net_alloc_bytes() - before;
        // The library's own memory: worlds made and freed, then kept.
        let before = ponko_net_alloc_bytes();
        for _ in 0..PASSES {
            let w = ponko_world_new();
            assert_eq!(ponko_world_free(w), PONKO_OK);
        }
        seen.worlds = ponko_net_alloc_bytes() - before;
        let mut kept = Vec::with_capacity(PASSES as usize);
        let before = ponko_net_alloc_bytes();
        for _ in 0..PASSES {
            kept.push(ponko_world_new());
        }
        seen.leaked_worlds = ponko_net_alloc_bytes() - before;
        for w in kept {
            assert_eq!(ponko_world_free(w), PONKO_OK);
        }
    }
}

#[test]
fn net_allocated_bytes_stay_flat_for_code_that_frees_and_grow_for_a_leak() {
    let mut seen = Seen::default();
    unsafe {
        let g = ponko_game_new(c"net".as_ptr(), 1, 1);
        assert_eq!(
            ponko_game_sim_system(g, c"measure".as_ptr(), 1, Some(measure), (&raw mut seen).cast()),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 2, Some(setup), null_mut(), &mut world),
            PONKO_OK
        );
        assert_eq!(ponko_world_free(world), PONKO_OK);
    }
    assert!(seen.measured);
    let leak = PASSES * BLOCK as i64;
    // Other threads may allocate meanwhile: a slack far below the leak.
    let slack = leak / 4;
    assert!(seen.balanced.abs() < slack, "balanced: {}", seen.balanced);
    assert!(
        seen.leaking >= leak && seen.leaking < leak + slack,
        "leaking: {}",
        seen.leaking
    );
    assert!(seen.worlds.abs() < slack, "worlds made and freed: {}", seen.worlds);
    assert!(
        seen.leaked_worlds > seen.worlds.abs() + PASSES * 64,
        "kept worlds hold memory: {} vs {}",
        seen.leaked_worlds,
        seen.worlds
    );
}
