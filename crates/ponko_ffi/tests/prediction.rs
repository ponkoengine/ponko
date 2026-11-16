//! Client-side prediction from C: byte states and inputs, a step callback,
//! reconciliation with deterministic replay, failing and re-entering steps,
//! and the server's input queue.
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

/// A walker: position and velocity in fixed point (no float rounding, so
/// both sides produce the same bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct State {
    x: i32,
    vx: i32,
}

/// One tick's input: an acceleration.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Input {
    ax: i32,
}

/// The rule both sides run. Counts its calls in `user` (an `i32`).
unsafe extern "C" fn step(
    user: *mut c_void,
    state: *mut c_void,
    state_size: usize,
    input: *const c_void,
    input_size: usize,
) -> bool {
    assert_eq!((state_size, input_size), (size_of::<State>(), size_of::<Input>()));
    assert_eq!(state as usize % 16, 0, "the state buffer is 16-byte aligned");
    unsafe {
        let s = &mut *state.cast::<State>();
        let i = &*input.cast::<Input>();
        s.vx += i.ax;
        s.x += s.vx;
        if !user.is_null() {
            *user.cast::<i32>() += 1;
        }
    }
    true
}

fn rust_step(s: &mut State, i: &Input) {
    s.vx += i.ax;
    s.x += s.vx;
}

unsafe fn new(state: State) -> *mut PonkoPrediction {
    let mut p = null_mut();
    unsafe {
        assert_eq!(
            ponko_prediction_new(
                (&raw const state).cast(),
                size_of::<State>(),
                size_of::<Input>(),
                0,
                &mut p
            ),
            PONKO_OK
        );
    }
    p
}

unsafe fn state_of(p: *const PonkoPrediction) -> State {
    let mut s = State::default();
    unsafe { assert_eq!(ponko_prediction_state(p, (&raw mut s).cast()), PONKO_OK) };
    s
}

unsafe fn predict(p: *mut PonkoPrediction, ax: i32, calls: &mut i32) -> u16 {
    let input = Input { ax };
    let mut seq = 0;
    unsafe {
        assert_eq!(
            ponko_prediction_predict(
                p,
                (&raw const input).cast(),
                Some(step),
                (calls as *mut i32).cast(),
                &mut seq
            ),
            PONKO_OK
        );
    }
    seq
}

#[test]
fn a_correction_replays_the_unconfirmed_inputs_deterministically() {
    unsafe {
        let p = new(State::default());
        let inputs = [1, 2, -1, 3, 0, 2];
        let mut calls = 0;
        for (n, &ax) in inputs.iter().enumerate() {
            assert_eq!(predict(p, ax, &mut calls), n as u16);
        }
        // The server applied the first three inputs, but something pushed the
        // walker 10 further: its state differs.
        let mut server = State::default();
        for &ax in &inputs[..3] {
            rust_step(&mut server, &Input { ax });
        }
        server.x += 10;
        let mut result = PonkoReconciled::default();
        let mut before = State::default();
        let predicted = state_of(p);
        calls = 0;
        assert_eq!(
            ponko_prediction_reconcile(
                p,
                2,
                (&raw const server).cast(),
                Some(step),
                (&raw mut calls).cast(),
                &mut result,
                (&raw mut before).cast()
            ),
            PONKO_OK
        );
        assert_eq!(result.outcome, PonkoReconcileOutcome::PONKO_RECONCILE_CORRECTED as u32);
        assert_eq!((result.replayed, calls), (3, 3), "the three newer inputs, once each");
        assert_eq!(before, predicted);
        // The replay is exactly the Rust rule on the server state.
        let mut want = server;
        for &ax in &inputs[3..] {
            rust_step(&mut want, &Input { ax });
        }
        assert_eq!(state_of(p), want);
        assert_eq!(state_of(p).x, predicted.x + 10);

        // The unconfirmed inputs are the last three, oldest first.
        let (mut seqs, mut ins, mut n) = ([0u16; 8], [Input::default(); 8], 0);
        assert_eq!(
            ponko_prediction_unacked(p, seqs.as_mut_ptr(), ins.as_mut_ptr().cast(), 8, &mut n),
            PONKO_OK
        );
        assert_eq!(n, 3);
        assert_eq!(seqs[..3], [3, 4, 5]);
        assert_eq!(ins[..3].iter().map(|i| i.ax).collect::<Vec<_>>(), [3, 0, 2]);
        let mut only_count = 0;
        assert_eq!(
            ponko_prediction_unacked(p, null_mut(), null_mut(), 0, &mut only_count),
            PONKO_OK
        );
        assert_eq!(only_count, 3);

        // A server state that agrees confirms without replaying; a late one is ignored.
        let mut agree = server;
        rust_step(&mut agree, &Input { ax: 3 });
        calls = 0;
        assert_eq!(
            ponko_prediction_reconcile(
                p,
                3,
                (&raw const agree).cast(),
                Some(step),
                (&raw mut calls).cast(),
                &mut result,
                null_mut()
            ),
            PONKO_OK
        );
        assert_eq!(
            (result.outcome, calls),
            (PonkoReconcileOutcome::PONKO_RECONCILE_CONFIRMED as u32, 0)
        );
        assert_eq!(
            ponko_prediction_reconcile(
                p,
                1,
                (&raw const agree).cast(),
                Some(step),
                null_mut(),
                &mut result,
                null_mut()
            ),
            PONKO_OK
        );
        assert_eq!(result.outcome, PonkoReconcileOutcome::PONKO_RECONCILE_IGNORED as u32);
        let (mut has, mut ack) = (false, 0);
        assert_eq!(ponko_prediction_acked(p, &mut has, &mut ack), PONKO_OK);
        assert_eq!((has, ack), (true, 3));
        let mut next = 0;
        assert_eq!(ponko_prediction_next_seq(p, &mut next), PONKO_OK);
        assert_eq!(next, 6);
        let mut stats = PonkoPredictionStats::default();
        assert_eq!(ponko_prediction_stats(p, &mut stats), PONKO_OK);
        assert_eq!(
            (stats.predicted, stats.corrections, stats.confirmed, stats.replayed),
            (6, 1, 1, 3)
        );
        ponko_prediction_free(p);
    }
}

#[test]
fn the_same_inputs_replay_to_the_same_bytes_every_time() {
    unsafe {
        let mut finals = Vec::new();
        for _ in 0..3 {
            let p = new(State { x: 5, vx: 0 });
            let mut calls = 0;
            for ax in [3, -2, 7, 1, -4, 0, 2, 2] {
                predict(p, ax, &mut calls);
            }
            let server = State { x: 100, vx: 1 };
            let mut r = PonkoReconciled::default();
            assert_eq!(
                ponko_prediction_reconcile(
                    p,
                    2,
                    (&raw const server).cast(),
                    Some(step),
                    null_mut(),
                    &mut r,
                    null_mut()
                ),
                PONKO_OK
            );
            assert_eq!(r.replayed, 5);
            let mut previous = State::default();
            assert_eq!(ponko_prediction_previous(p, (&raw mut previous).cast()), PONKO_OK);
            finals.push((state_of(p), previous));
            ponko_prediction_free(p);
        }
        assert!(finals.windows(2).all(|w| w[0] == w[1]), "{finals:?}");
    }
}

/// A step that fails on an input of 99.
unsafe extern "C" fn picky(user: *mut c_void, state: *mut c_void, ss: usize, input: *const c_void, is: usize) -> bool {
    if unsafe { (*input.cast::<Input>()).ax } == 99 {
        return false;
    }
    unsafe { step(user, state, ss, input, is) }
}

#[test]
fn a_failing_step_stops_the_call_and_reset_recovers() {
    unsafe {
        let p = new(State::default());
        let mut calls = 0;
        predict(p, 1, &mut calls);
        let bad = Input { ax: 99 };
        let mut seq = 0;
        assert_eq!(
            ponko_prediction_predict(p, (&raw const bad).cast(), Some(picky), null_mut(), &mut seq),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(seq, 1, "the input is still numbered and kept");
        // A correction replays the 99 again: the replay stops there and the
        // inputs after it are skipped.
        let good = Input { ax: 1 };
        let mut counted = 0;
        assert_eq!(
            ponko_prediction_predict(
                p,
                (&raw const good).cast(),
                Some(picky),
                (&raw mut counted).cast(),
                &mut seq
            ),
            PONKO_OK
        );
        counted = 0;
        let server = State { x: 50, vx: 0 };
        assert_eq!(
            ponko_prediction_reconcile(
                p,
                0,
                (&raw const server).cast(),
                Some(picky),
                (&raw mut counted).cast(),
                null_mut(),
                null_mut()
            ),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(counted, 0, "nothing ran after the failed step");
        assert_eq!(state_of(p), server, "half-replayed: the server state, no steps");
        let restart = State { x: 7, vx: 0 };
        assert_eq!(ponko_prediction_reset(p, (&raw const restart).cast()), PONKO_OK);
        let mut n = 9;
        assert_eq!(ponko_prediction_unacked(p, null_mut(), null_mut(), 0, &mut n), PONKO_OK);
        assert_eq!(n, 0);
        assert_eq!(state_of(p), restart);
        // A null step is an argument error that changes nothing.
        assert_eq!(
            ponko_prediction_predict(p, (&raw const good).cast(), None, null_mut(), &mut seq),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(state_of(p), restart);
        ponko_prediction_free(p);
    }
}

/// A step that calls its own prediction (passed as `user`): refused.
unsafe extern "C" fn reentrant(user: *mut c_void, state: *mut c_void, _: usize, _: *const c_void, _: usize) -> bool {
    let mut other = State::default();
    let status = unsafe { ponko_prediction_state(user.cast(), (&raw mut other).cast()) };
    unsafe { (*state.cast::<State>()).x = status as i32 };
    true
}

#[test]
fn a_step_that_calls_its_own_prediction_is_refused_as_busy() {
    unsafe {
        let p = new(State::default());
        let input = Input { ax: 0 };
        assert_eq!(
            ponko_prediction_predict(p, (&raw const input).cast(), Some(reentrant), p.cast(), null_mut()),
            PONKO_OK
        );
        assert_eq!(state_of(p).x, PONKO_BUSY as i32);
        ponko_prediction_free(p);
    }
}

#[test]
fn sizes_capacity_and_null_arguments_are_checked() {
    unsafe {
        let s = State::default();
        let mut p = null_mut();
        for (state_size, input_size, capacity) in [(0, 4, 0), (8, 0, 0), (8, 4, 32769), (8, (1 << 20) + 1, 0)] {
            assert_eq!(
                ponko_prediction_new((&raw const s).cast(), state_size, input_size, capacity, &mut p),
                PONKO_INVALID_ARGUMENT,
                "{state_size} {input_size} {capacity}"
            );
        }
        assert_eq!(ponko_prediction_new(null(), 8, 4, 0, &mut p), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_prediction_state(null(), (&raw mut p).cast()),
            PONKO_INVALID_ARGUMENT
        );
        ponko_prediction_free(null_mut());

        // A capacity of 2 drops the oldest unconfirmed input.
        assert_eq!(ponko_prediction_new((&raw const s).cast(), 8, 4, 2, &mut p), PONKO_OK);
        let mut calls = 0;
        for ax in [1, 2, 3] {
            predict(p, ax, &mut calls);
        }
        let mut stats = PonkoPredictionStats::default();
        assert_eq!(ponko_prediction_stats(p, &mut stats), PONKO_OK);
        assert_eq!(stats.dropped, 1);
        ponko_prediction_free(p);
    }
}

#[test]
fn the_input_queue_orders_inputs_and_drops_repeats_and_late_ones() {
    unsafe {
        let mut q = null_mut();
        assert_eq!(ponko_input_queue_new(size_of::<Input>(), 0, &mut q), PONKO_OK);
        let mut push = 9;
        for seq in [1u16, 0, 2] {
            let i = Input { ax: seq as i32 * 10 };
            assert_eq!(
                ponko_input_queue_push(q, seq, (&raw const i).cast(), &mut push),
                PONKO_OK
            );
            assert_eq!(push, PonkoInputPush::PONKO_INPUT_ADDED as u32);
        }
        let dup = Input { ax: 0 };
        assert_eq!(
            ponko_input_queue_push(q, 2, (&raw const dup).cast(), &mut push),
            PONKO_OK
        );
        assert_eq!(push, PonkoInputPush::PONKO_INPUT_DUPLICATE as u32);
        let (mut has, mut ack) = (true, 7);
        assert_eq!(ponko_input_queue_ack(q, &mut has, &mut ack), PONKO_OK);
        assert!(!has);
        let mut got = Vec::new();
        loop {
            let (mut found, mut seq, mut input) = (false, 0, Input::default());
            assert_eq!(
                ponko_input_queue_pop(q, &mut found, &mut seq, (&raw mut input).cast()),
                PONKO_OK
            );
            if !found {
                break;
            }
            got.push((seq, input.ax));
        }
        assert_eq!(got, [(0, 0), (1, 10), (2, 20)]);
        assert_eq!(ponko_input_queue_ack(q, &mut has, &mut ack), PONKO_OK);
        assert_eq!((has, ack), (true, 2));
        assert_eq!(
            ponko_input_queue_push(q, 1, (&raw const dup).cast(), &mut push),
            PONKO_OK
        );
        assert_eq!(push, PonkoInputPush::PONKO_INPUT_LATE as u32);
        assert_eq!(ponko_input_queue_push(q, 3, null(), &mut push), PONKO_INVALID_ARGUMENT);
        ponko_input_queue_free(q);
        ponko_input_queue_free(null_mut());
        assert_eq!(ponko_input_queue_new(0, 0, &mut q), PONKO_INVALID_ARGUMENT);
    }
}

#[test]
fn a_client_and_a_server_agree_after_a_lossy_exchange() {
    // The client predicts 20 ticks; every 4th packet is lost; the server
    // answers each tick it applied. With the same rule, every answer
    // confirms and the client's state is the server's.
    unsafe {
        let p = new(State::default());
        let mut q = null_mut();
        assert_eq!(ponko_input_queue_new(size_of::<Input>(), 0, &mut q), PONKO_OK);
        let mut server = State::default();
        let mut calls = 0;
        for tick in 0..20 {
            predict(p, (tick % 5) - 2, &mut calls);
            if tick % 4 == 3 {
                continue; // lost
            }
            // Resend every unconfirmed input.
            let (mut seqs, mut ins, mut n) = ([0u16; 32], [Input::default(); 32], 0);
            assert_eq!(
                ponko_prediction_unacked(p, seqs.as_mut_ptr(), ins.as_mut_ptr().cast(), 32, &mut n),
                PONKO_OK
            );
            for k in 0..n {
                ponko_input_queue_push(q, seqs[k], (&raw const ins[k]).cast(), null_mut());
            }
            loop {
                let (mut found, mut seq, mut input) = (false, 0, Input::default());
                ponko_input_queue_pop(q, &mut found, &mut seq, (&raw mut input).cast());
                if !found {
                    break;
                }
                rust_step(&mut server, &input);
            }
            let (mut has, mut ack) = (false, 0);
            ponko_input_queue_ack(q, &mut has, &mut ack);
            let mut r = PonkoReconciled::default();
            assert_eq!(
                ponko_prediction_reconcile(
                    p,
                    ack,
                    (&raw const server).cast(),
                    Some(step),
                    null_mut(),
                    &mut r,
                    null_mut()
                ),
                PONKO_OK
            );
            assert_eq!(
                r.outcome,
                PonkoReconcileOutcome::PONKO_RECONCILE_CONFIRMED as u32,
                "tick {tick}"
            );
        }
        let mut stats = PonkoPredictionStats::default();
        ponko_prediction_stats(p, &mut stats);
        assert_eq!(stats.corrections, 0);
        ponko_prediction_free(p);
        ponko_input_queue_free(q);
    }
}
