//! A steady predict-and-reconcile loop allocates nothing: inputs and states
//! of confirmed ticks go back to a per-thread pool. Its own test binary,
//! with a single test, because the library's counting allocator counts the
//! whole process while profiling is on (see `filters_alloc.rs`). The
//! measuring runs inside a game's setup callback (profiling needs a ctx),
//! so it records what it saw and the test checks afterwards.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct State {
    x: i32,
    vx: i32,
}

unsafe extern "C" fn step(_: *mut c_void, state: *mut c_void, _: usize, input: *const c_void, _: usize) -> bool {
    unsafe {
        let s = &mut *state.cast::<State>();
        s.vx += *input.cast::<i32>();
        s.x += s.vx;
    }
    true
}

#[derive(Default)]
struct Measured {
    ok: bool,
    allocations: u64,
    corrections: u64,
}

/// One tick of a client 3 ticks behind its server: predict, and reconcile
/// the state of 3 ticks ago, wrong every 8th tick.
unsafe fn tick(p: *mut PonkoPrediction, history: &mut [State], t: u32) -> bool {
    let input = (t % 3) as i32 - 1;
    let len = history.len();
    let mut seq = 0;
    unsafe {
        if ponko_prediction_predict(p, (&raw const input).cast(), Some(step), null_mut(), &mut seq) != PONKO_OK {
            return false;
        }
        let mut now = State::default();
        ponko_prediction_state(p, (&raw mut now).cast());
        history[seq as usize % len] = now;
        if t >= 3 {
            let ack = seq.wrapping_sub(3);
            let mut server = history[ack as usize % len];
            if t.is_multiple_of(8) {
                server.x += 1;
            }
            let mut r = PonkoReconciled::default();
            let status = ponko_prediction_reconcile(
                p,
                ack,
                (&raw const server).cast(),
                Some(step),
                null_mut(),
                &mut r,
                null_mut(),
            );
            if status != PONKO_OK {
                return false;
            }
            if r.outcome == PonkoReconcileOutcome::PONKO_RECONCILE_CORRECTED as u32 {
                // The client's later states moved with the correction.
                for k in 1..=3u16 {
                    let i = ack.wrapping_add(k) as usize % len;
                    history[i].x += 1;
                }
            }
        }
    }
    true
}

unsafe extern "C" fn measure(ctx: *mut PonkoCtx, user: *mut c_void) {
    let out = unsafe { &mut *user.cast::<Measured>() };
    let start = State::default();
    let mut p = null_mut();
    let mut history = vec![State::default(); 64];
    unsafe {
        if ponko_prediction_new((&raw const start).cast(), 8, 4, 0, &mut p) != PONKO_OK {
            return;
        }
        // Warm up: the pool and the history fill.
        out.ok = (0..64).all(|t| tick(p, &mut history, t));
        ponko_ctx_set_profiling(ctx, true);
        let before = ponko::perf::thread_alloc_counts().count;
        out.ok &= (64..1064).all(|t| tick(p, &mut history, t));
        out.allocations = ponko::perf::thread_alloc_counts().count - before;
        ponko_ctx_set_profiling(ctx, false);
        let mut stats = PonkoPredictionStats::default();
        ponko_prediction_stats(p, &mut stats);
        out.corrections = stats.corrections;
        ponko_prediction_free(p);
    }
}

#[test]
fn steady_prediction_and_reconciliation_allocate_nothing() {
    let mut measured = Measured::default();
    unsafe {
        let game = ponko_game_new(c"prediction".as_ptr(), 1, 1);
        let mut world = null_mut();
        let status = ponko_game_run_headless(game, 0, Some(measure), (&raw mut measured).cast(), &mut world);
        assert_eq!(status, PONKO_OK);
        ponko_world_free(world);
    }
    assert!(measured.ok, "a prediction call failed");
    assert!(
        measured.corrections >= 100,
        "corrections replayed: {}",
        measured.corrections
    );
    assert_eq!(measured.allocations, 0, "1000 ticks of predict + reconcile allocated");
}
