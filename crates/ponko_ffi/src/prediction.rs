//! Client-side prediction from C (Rust `net::Prediction` and `net::InputQueue`),
//! with the state and the input as fixed-size byte blobs and the step as a
//! callback.
//!
//! The state and every input are copied in and out: the library never keeps
//! a pointer the caller gave it. Buffers handed to the step callback are
//! 16-byte aligned, so C can cast them to its own types. Inputs and states
//! of finished ticks go back to a small per-thread pool, so a steady
//! predict-and-reconcile loop allocates nothing.
//!
//! The step callback returns `true` when it stepped. `false` (a C++
//! exception it caught, say) stops the call at once: the remaining replay
//! steps are skipped, the call fails with `PONKO_INVALID_ARGUMENT`, and the
//! predicted state is whatever the finished steps left; call
//! `ponko_prediction_reset` with a state you trust before predicting again.

use std::cell::{Cell, RefCell, UnsafeCell};
use std::ffi::c_void;

use ponko::net::{InputPush, InputQueue, Prediction, Reconciled};

use crate::ctx::out;
use crate::{Failure, PonkoStatus, boundary, error, invalid, invalid_owned};

// ---- 16-byte aligned, pooled byte buffers -----------------------------------

/// 16 bytes, 16-aligned: what a buffer is made of, so C may cast it.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct Chunk([u8; 16]);

/// Buffers no prediction uses any more, reused before allocating.
const POOL_MAX: usize = 1024;

thread_local! {
    static POOL: RefCell<Vec<Vec<Chunk>>> = const { RefCell::new(Vec::new()) };
}

/// A state or an input: `len` bytes in 16-aligned chunks, recycled on drop.
struct Bytes {
    chunks: Vec<Chunk>,
    len: usize,
}

impl Bytes {
    /// A copy of `len` bytes at `src`.
    ///
    /// # Safety
    /// `src` readable for `len` bytes.
    unsafe fn copy_of(src: *const u8, len: usize) -> Self {
        let mut chunks = POOL.with(|p| p.borrow_mut().pop()).unwrap_or_default();
        chunks.clear();
        chunks.resize(len.div_ceil(16), Chunk([0; 16]));
        // SAFETY: caller contract; `chunks` holds at least `len` bytes.
        unsafe { std::ptr::copy_nonoverlapping(src, chunks.as_mut_ptr().cast::<u8>(), len) };
        Self { chunks, len }
    }
    fn as_ptr(&self) -> *const u8 {
        self.chunks.as_ptr().cast()
    }
    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.chunks.as_mut_ptr().cast()
    }
    fn as_slice(&self) -> &[u8] {
        // SAFETY: the chunks hold at least `len` initialized bytes.
        unsafe { std::slice::from_raw_parts(self.as_ptr(), self.len) }
    }
    /// Copies the bytes to `dst`.
    ///
    /// # Safety
    /// `dst` writable for `len` bytes.
    unsafe fn copy_to(&self, dst: *mut c_void) {
        // SAFETY: caller contract.
        unsafe { std::ptr::copy_nonoverlapping(self.as_ptr(), dst.cast::<u8>(), self.len) };
    }
}

impl Clone for Bytes {
    fn clone(&self) -> Self {
        // SAFETY: our own bytes.
        unsafe { Self::copy_of(self.as_ptr(), self.len) }
    }
    fn clone_from(&mut self, source: &Self) {
        self.chunks.clone_from(&source.chunks);
        self.len = source.len;
    }
}

impl PartialEq for Bytes {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Drop for Bytes {
    fn drop(&mut self) {
        let chunks = std::mem::take(&mut self.chunks);
        if chunks.capacity() > 0 {
            // `try_with`: a buffer dropped while the thread exits is freed.
            let _ = POOL.try_with(|p| {
                let mut p = p.borrow_mut();
                if p.len() < POOL_MAX {
                    p.push(chunks);
                }
            });
        }
    }
}

// ---- the step callback ------------------------------------------------------

/// One tick of the simulation: apply `input` (`input_size` bytes) to `state`
/// (`state_size` bytes, changed in place), exactly as the server does for
/// that input. Both buffers are 16-byte aligned. Return true when it
/// stepped, false to stop the call (see `ponko_prediction_predict`). It must
/// not call functions of the prediction it steps (they fail with
/// `PONKO_BUSY`), and must not unwind (a C++ exception or a Rust panic) into
/// the library: catch it, return false, and rethrow after the call, as
/// `ponko::prediction` does.
pub type PonkoPredictStepFn = Option<
    unsafe extern "C" fn(
        user: *mut c_void,
        state: *mut c_void,
        state_size: usize,
        input: *const c_void,
        input_size: usize,
    ) -> bool,
>;

/// Runs the C step over `Prediction`'s closure parameters, remembering a
/// failure and skipping every later step of the call.
struct Stepper {
    step: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *const c_void, usize) -> bool,
    user: *mut c_void,
    failed: bool,
}

impl Stepper {
    fn new(step: PonkoPredictStepFn, user: *mut c_void) -> Result<Self, Failure> {
        let step = step.ok_or_else(|| invalid("the step callback is null"))?;
        Ok(Self {
            step,
            user,
            failed: false,
        })
    }
    fn run(&mut self, state: &mut Bytes, input: &Bytes) {
        if self.failed {
            return;
        }
        let (state_size, input_size) = (state.len, input.len);
        // SAFETY: the caller's callback, with buffers of the sizes it was
        // given at creation; it may only write `state_size` bytes of state.
        let stepped = unsafe {
            (self.step)(
                self.user,
                state.as_mut_ptr().cast(),
                state_size,
                input.as_ptr().cast(),
                input_size,
            )
        };
        self.failed = !stepped;
    }
    fn result(&self) -> Result<(), Failure> {
        if self.failed {
            Err(invalid(
                "the step callback failed; the predicted state may be half-stepped, so reset it",
            ))
        } else {
            Ok(())
        }
    }
}

// ---- the prediction ---------------------------------------------------------

/// What a `PonkoPrediction` keeps unless told otherwise (Rust
/// `DEFAULT_CAPACITY`): 256 inputs, about 4 seconds at 60 ticks a second.
pub const PONKO_PREDICTION_DEFAULT_CAPACITY: usize = 256;

/// Client-side prediction of a state the client controls (Rust
/// `net::Prediction`), over fixed-size byte states and inputs. From
/// `ponko_prediction_new`, freed with `ponko_prediction_free`. Not
/// thread-safe: use it from one thread at a time.
pub struct PonkoPrediction {
    /// Set while a call runs, so a step that calls back in is refused
    /// instead of aliasing `inner`.
    busy: Cell<bool>,
    inner: UnsafeCell<Predicting>,
}

struct Predicting {
    prediction: Prediction<Bytes, Bytes>,
    state_size: usize,
    input_size: usize,
}

/// Clears `busy` when a call ends, panicking or not.
struct Busy<'a>(&'a Cell<bool>);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

/// Runs `f` on the prediction behind `p`, refusing null and calls from its
/// own step callback.
///
/// # Safety
/// `p` null or a live prediction.
unsafe fn with_prediction(
    p: *const PonkoPrediction,
    f: impl FnOnce(&mut Predicting) -> Result<(), Failure>,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller contract; every mutation goes through the cells.
        let p = unsafe { p.as_ref() }.ok_or_else(|| invalid("prediction is null"))?;
        if p.busy.replace(true) {
            return Err(error(
                PonkoStatus::PONKO_BUSY,
                "a prediction's step callback can't call that prediction",
            ));
        }
        let _busy = Busy(&p.busy);
        // SAFETY: `busy` was clear, so no other reference to `inner` lives.
        f(unsafe { &mut *p.inner.get() })
    })
}

/// Checks a size given to `ponko_prediction_new` or `ponko_input_queue_new`.
fn size_ok(size: usize, what: &'static str) -> Result<(), Failure> {
    if size == 0 || size > 1 << 20 {
        return Err(invalid_owned(format!("{what} must be 1 byte to 1 MiB")));
    }
    Ok(())
}

/// A capacity: 0 for the default, else 1 to 32768.
fn capacity_of(capacity: usize) -> Result<usize, Failure> {
    match capacity {
        0 => Ok(PONKO_PREDICTION_DEFAULT_CAPACITY),
        1..=32768 => Ok(capacity),
        _ => Err(invalid("capacity must be 0 (the default, 256) or 1 to 32768")),
    }
}

/// Starts predicting from `state` (`state_size` bytes, copied), with inputs
/// of `input_size` bytes, keeping up to `capacity` unconfirmed inputs (0:
/// 256; at most 32768). Sizes are 1 byte to 1 MiB. Free it with
/// `ponko_prediction_free`.
///
/// States are compared byte for byte (`memcmp`): give the state no padding
/// (or zero it), and make the step produce the same bytes on the client and
/// the server (round floats, or use fixed point, if they can differ).
///
/// Ownership: on success `out_prediction` receives an owned prediction, the
/// caller's; free it with ponko_prediction_free.
///
/// # Safety
/// `state` readable for `state_size` bytes; `out_prediction` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut p = std::ptr::null_mut();
/// let state = 0i32;
/// let s = unsafe { ponko_prediction_new((&raw const state).cast(), 4, 4, 0, &mut p) };
/// assert_eq!(s, PonkoStatus::PONKO_OK);
/// unsafe { ponko_prediction_free(p) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_new(
    state: *const c_void,
    state_size: usize,
    input_size: usize,
    capacity: usize,
    out_prediction: *mut *mut PonkoPrediction,
) -> PonkoStatus {
    boundary(|| {
        size_ok(state_size, "state_size")?;
        size_ok(input_size, "input_size")?;
        let capacity = capacity_of(capacity)?;
        if state.is_null() {
            return Err(invalid("state is null"));
        }
        if out_prediction.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: caller contract.
        let state = unsafe { Bytes::copy_of(state.cast(), state_size) };
        let p = PonkoPrediction {
            busy: Cell::new(false),
            inner: UnsafeCell::new(Predicting {
                prediction: Prediction::with_capacity(state, capacity),
                state_size,
                input_size,
            }),
        };
        // SAFETY: checked non-null; caller contract.
        unsafe { out(out_prediction, Box::into_raw(Box::new(p))) }
    })
}

/// Frees a prediction; null does nothing.
/// # Safety
/// `prediction` null or from `ponko_prediction_new`, freed once, not from
/// its own step callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_free(prediction: *mut PonkoPrediction) {
    if !prediction.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(prediction) });
    }
}

/// Runs one tick (Rust `Prediction::predict`): `step` applies `input`
/// (`input_size` bytes, copied and kept until the server confirms it) to
/// the predicted state, and the input's sequence number goes to `*out_seq`
/// (null: not wanted). Call it once per fixed tick, where the server applies
/// inputs. When `step` returns false the input is still kept and numbered,
/// the call fails with `PONKO_INVALID_ARGUMENT`, and the state is what the
/// step left: reset it.
///
/// Ownership: `user` stays the caller's and is only passed to `step` during
/// this call; the engine keeps no pointer to it after returning.
///
/// # Safety
/// `prediction` null or live; `input` readable for `input_size` bytes;
/// `step` valid with `user`; `out_seq` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_predict(
    prediction: *mut PonkoPrediction,
    input: *const c_void,
    step: PonkoPredictStepFn,
    user: *mut c_void,
    out_seq: *mut u16,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            let mut stepper = Stepper::new(step, user)?;
            if input.is_null() {
                return Err(invalid("input is null"));
            }
            let input = Bytes::copy_of(input.cast(), p.input_size);
            let seq = p.prediction.predict(input, |s, i| stepper.run(s, i));
            if !out_seq.is_null() {
                out_seq.write(seq);
            }
            stepper.result()
        })
    }
}

/// What `ponko_prediction_reconcile` did with a server state (Rust
/// `Reconciled`), passed as `uint32_t`.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoReconcileOutcome {
    /// Older than (or the same as) a state already reconciled, or for an
    /// input never predicted: nothing changed.
    PONKO_RECONCILE_IGNORED = 0,
    /// The server agrees with what was predicted for that input.
    PONKO_RECONCILE_CONFIRMED = 1,
    /// The server disagreed: the prediction restarted from its state and
    /// replayed the inputs it hasn't confirmed yet.
    PONKO_RECONCILE_CORRECTED = 2,
}

/// The result of `ponko_prediction_reconcile`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoReconciled {
    /// A `PonkoReconcileOutcome`.
    pub outcome: u32,
    /// How many inputs were replayed on top of the server's state (with
    /// `PONKO_RECONCILE_CORRECTED`), else 0.
    pub replayed: u32,
}

/// Takes the server's `state` (`state_size` bytes) after input `ack` (Rust
/// `Prediction::reconcile`). If it matches what was predicted for that
/// input, byte for byte, nothing changes. Otherwise the prediction starts
/// again from `state` and `step` replays every newer input, oldest first,
/// with the same inputs it was given: a deterministic step gives the same
/// result every time. Inputs up to `ack` are forgotten. Late, repeated and
/// out-of-order states are ignored. With a correction, the predicted state
/// just before it goes to `out_before` (null: not wanted; `state_size`
/// bytes), for smoothing. When `step` returns false the replay stops, the
/// call fails with `PONKO_INVALID_ARGUMENT` and the state is half-replayed:
/// reset it.
///
/// Ownership: `user` stays the caller's and is only passed to `step` during
/// this call; the engine keeps no pointer to it after returning.
///
/// # Safety
/// `prediction` null or live; `state` readable for `state_size` bytes;
/// `step` valid with `user`; `out_result` null or writable; `out_before`
/// null or writable for `state_size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_reconcile(
    prediction: *mut PonkoPrediction,
    ack: u16,
    state: *const c_void,
    step: PonkoPredictStepFn,
    user: *mut c_void,
    out_result: *mut PonkoReconciled,
    out_before: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            let mut stepper = Stepper::new(step, user)?;
            if state.is_null() {
                return Err(invalid("state is null"));
            }
            let server = Bytes::copy_of(state.cast(), p.state_size);
            let result = match p.prediction.reconcile(ack, server, |s, i| stepper.run(s, i)) {
                Reconciled::Ignored => PonkoReconciled {
                    outcome: PonkoReconcileOutcome::PONKO_RECONCILE_IGNORED as u32,
                    replayed: 0,
                },
                Reconciled::Confirmed => PonkoReconciled {
                    outcome: PonkoReconcileOutcome::PONKO_RECONCILE_CONFIRMED as u32,
                    replayed: 0,
                },
                Reconciled::Corrected { before, replayed } => {
                    if !out_before.is_null() {
                        before.copy_to(out_before);
                    }
                    PonkoReconciled {
                        outcome: PonkoReconcileOutcome::PONKO_RECONCILE_CORRECTED as u32,
                        replayed: replayed as u32,
                    }
                }
            };
            if !out_result.is_null() {
                out_result.write(result);
            }
            stepper.result()
        })
    }
}

/// Copies the predicted state after the newest input (`state_size` bytes)
/// to `out_state`.
/// # Safety
/// `prediction` null or live; `out_state` writable for `state_size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_state(
    prediction: *const PonkoPrediction,
    out_state: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            if out_state.is_null() {
                return Err(invalid("output is null"));
            }
            p.prediction.state().copy_to(out_state);
            Ok(())
        })
    }
}

/// Copies the predicted state one tick earlier (`state_size` bytes) to
/// `out_state`: draw blended from it to `ponko_prediction_state` by the time
/// since the last tick.
/// # Safety
/// `prediction` null or live; `out_state` writable for `state_size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_previous(
    prediction: *const PonkoPrediction,
    out_state: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            if out_state.is_null() {
                return Err(invalid("output is null"));
            }
            p.prediction.previous().copy_to(out_state);
            Ok(())
        })
    }
}

/// Sets the predicted state outright (`state_size` bytes: a respawn, a
/// teleport) and forgets the unconfirmed inputs; numbering goes on, and
/// server states for inputs before it are ignored (Rust `Prediction::reset`).
/// # Safety
/// `prediction` null or live; `state` readable for `state_size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_reset(prediction: *mut PonkoPrediction, state: *const c_void) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            if state.is_null() {
                return Err(invalid("state is null"));
            }
            p.prediction.reset(Bytes::copy_of(state.cast(), p.state_size));
            Ok(())
        })
    }
}

/// The inputs the server hasn't confirmed, oldest first: writes up to
/// `capacity` sequence numbers to `out_seqs` and their inputs, one after
/// another (`input_size` bytes each), to `out_inputs` (either may be null
/// if not wanted), and how many there are to `*out_count`. Send them every
/// tick, so one packet that gets through makes up for lost ones.
/// # Safety
/// `prediction` null or live; `out_seqs` null or writable for `capacity`
/// values; `out_inputs` null or writable for `capacity * input_size`
/// bytes; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_unacked(
    prediction: *const PonkoPrediction,
    out_seqs: *mut u16,
    out_inputs: *mut c_void,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            if out_count.is_null() {
                return Err(invalid("output is null"));
            }
            let mut n = 0;
            for (seq, input) in p.prediction.unacked() {
                if n < capacity {
                    if !out_seqs.is_null() {
                        out_seqs.add(n).write(seq);
                    }
                    if !out_inputs.is_null() {
                        input.copy_to(out_inputs.cast::<u8>().add(n * p.input_size).cast());
                    }
                }
                n += 1;
            }
            out_count.write(n);
            Ok(())
        })
    }
}

/// The newest input the server confirmed: `*out_has` says whether there is
/// one yet, `*out_ack` is its number (0 when not).
/// # Safety
/// `prediction` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_acked(
    prediction: *const PonkoPrediction,
    out_has: *mut bool,
    out_ack: *mut u16,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            if out_has.is_null() || out_ack.is_null() {
                return Err(invalid("output is null"));
            }
            let acked = p.prediction.acked();
            out_has.write(acked.is_some());
            out_ack.write(acked.unwrap_or(0));
            Ok(())
        })
    }
}

/// The sequence number the next `ponko_prediction_predict` gives.
/// # Safety
/// `prediction` null or live; `out_seq` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_next_seq(
    prediction: *const PonkoPrediction,
    out_seq: *mut u16,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_prediction(prediction, |p| out(out_seq, p.prediction.next_seq())) }
}

/// Counters of a prediction (Rust `PredictionStats`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoPredictionStats {
    /// Ticks predicted.
    pub predicted: u64,
    /// Server states that matched the prediction.
    pub confirmed: u64,
    /// Server states that didn't: the prediction was corrected.
    pub corrections: u64,
    /// Inputs replayed by corrections, in total.
    pub replayed: u64,
    /// Inputs dropped unconfirmed because the history was full.
    pub dropped: u64,
}

/// What a prediction did so far.
/// # Safety
/// `prediction` null or live; `out_stats` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_prediction_stats(
    prediction: *const PonkoPrediction,
    out_stats: *mut PonkoPredictionStats,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_prediction(prediction, |p| {
            let s = p.prediction.stats();
            out(
                out_stats,
                PonkoPredictionStats {
                    predicted: s.predicted,
                    confirmed: s.confirmed,
                    corrections: s.corrections,
                    replayed: s.replayed,
                    dropped: s.dropped,
                },
            )
        })
    }
}

// ---- the server's input queue -----------------------------------------------

/// The server's side of prediction (Rust `net::InputQueue`): one client's
/// inputs of a fixed size, put in sequence order whatever order the packets
/// came in, duplicates and late ones dropped. From `ponko_input_queue_new`,
/// freed with `ponko_input_queue_free`.
pub struct PonkoInputQueue {
    queue: InputQueue<Bytes>,
    input_size: usize,
}

/// What happened to an input given to `ponko_input_queue_push` (Rust
/// `InputPush`), passed as `uint32_t`.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoInputPush {
    /// Queued, in sequence order.
    PONKO_INPUT_ADDED = 0,
    /// Already queued: ignored.
    PONKO_INPUT_DUPLICATE = 1,
    /// Not newer than an input already applied: ignored.
    PONKO_INPUT_LATE = 2,
}

/// An empty queue of `input_size`-byte inputs (1 byte to 1 MiB), holding up
/// to `capacity` (0: 256; at most 32768). Free it with
/// `ponko_input_queue_free`.
///
/// Ownership: on success `out_queue` receives an owned queue, the caller's;
/// free it with ponko_input_queue_free.
///
/// # Safety
/// `out_queue` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut q = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_input_queue_new(4, 0, &mut q) }, PonkoStatus::PONKO_OK);
/// unsafe { ponko_input_queue_free(q) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_queue_new(
    input_size: usize,
    capacity: usize,
    out_queue: *mut *mut PonkoInputQueue,
) -> PonkoStatus {
    boundary(|| {
        size_ok(input_size, "input_size")?;
        let capacity = capacity_of(capacity)?;
        if out_queue.is_null() {
            return Err(invalid("output is null"));
        }
        let q = PonkoInputQueue {
            queue: InputQueue::with_capacity(capacity),
            input_size,
        };
        // SAFETY: checked non-null; caller contract.
        unsafe { out(out_queue, Box::into_raw(Box::new(q))) }
    })
}

/// Frees a queue; null does nothing.
/// # Safety
/// `queue` null or from `ponko_input_queue_new`, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_queue_free(queue: *mut PonkoInputQueue) {
    if !queue.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(queue) });
    }
}

fn queue_mut<'a>(q: *mut PonkoInputQueue) -> Result<&'a mut PonkoInputQueue, Failure> {
    // SAFETY: callers pass null or a live queue.
    unsafe { q.as_mut() }.ok_or_else(|| invalid("input queue is null"))
}

/// Queues input `seq` (`input_size` bytes, copied), in order; what happened
/// goes to `*out_push` (a `PonkoInputPush`; null: not wanted).
/// # Safety
/// `queue` null or live; `input` readable for `input_size` bytes; `out_push`
/// null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_queue_push(
    queue: *mut PonkoInputQueue,
    seq: u16,
    input: *const c_void,
    out_push: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        let q = queue_mut(queue)?;
        if input.is_null() {
            return Err(invalid("input is null"));
        }
        // SAFETY: caller contract.
        let input = unsafe { Bytes::copy_of(input.cast(), q.input_size) };
        let push = match q.queue.push(seq, input) {
            InputPush::Added => PonkoInputPush::PONKO_INPUT_ADDED,
            InputPush::Duplicate => PonkoInputPush::PONKO_INPUT_DUPLICATE,
            InputPush::Late => PonkoInputPush::PONKO_INPUT_LATE,
        };
        if !out_push.is_null() {
            // SAFETY: caller contract.
            unsafe { out_push.write(push as u32) };
        }
        Ok(())
    })
}

/// Takes the oldest queued input: `*out_found` says whether there was one;
/// if so its number goes to `*out_seq` and its bytes to `out_input`
/// (`input_size` bytes). Apply it with the same step the client predicts
/// with, then answer with your state and `ponko_input_queue_ack`.
/// # Safety
/// `queue` null or live; outputs writable (`out_input` for `input_size`
/// bytes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_queue_pop(
    queue: *mut PonkoInputQueue,
    out_found: *mut bool,
    out_seq: *mut u16,
    out_input: *mut c_void,
) -> PonkoStatus {
    boundary(|| {
        let q = queue_mut(queue)?;
        if out_found.is_null() || out_seq.is_null() || out_input.is_null() {
            return Err(invalid("output is null"));
        }
        let popped = q.queue.pop();
        // SAFETY: caller contract.
        unsafe {
            out_found.write(popped.is_some());
            if let Some((seq, input)) = popped {
                out_seq.write(seq);
                input.copy_to(out_input);
            }
        }
        Ok(())
    })
}

/// The number of the last input taken with `ponko_input_queue_pop`, the
/// one to answer with: `*out_has` says whether one was taken yet.
/// # Safety
/// `queue` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_queue_ack(
    queue: *const PonkoInputQueue,
    out_has: *mut bool,
    out_ack: *mut u16,
) -> PonkoStatus {
    boundary(|| {
        let q = queue_mut(queue.cast_mut())?;
        if out_has.is_null() || out_ack.is_null() {
            return Err(invalid("output is null"));
        }
        let ack = q.queue.ack();
        // SAFETY: caller contract.
        unsafe {
            out_has.write(ack.is_some());
            out_ack.write(ack.unwrap_or(0));
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffers_are_sixteen_byte_aligned_and_compare_their_bytes_only() {
        let a = unsafe { Bytes::copy_of([1u8, 2, 3].as_ptr(), 3) };
        let mut b = unsafe { Bytes::copy_of([1u8, 2, 3, 4].as_ptr(), 3) };
        assert_eq!(a.as_ptr() as usize % 16, 0);
        assert!(a == b);
        b.clone_from(&unsafe { Bytes::copy_of([9u8].as_ptr(), 1) });
        assert_eq!(b.as_slice(), [9]);
        assert!(a != b);
        let c = a.clone();
        assert!(c == a && c.as_ptr() != a.as_ptr(), "a clone owns a copy of the bytes");
    }
}
