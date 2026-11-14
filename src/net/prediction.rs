//! Client-side prediction and server reconciliation for the entities a
//! client controls: numbered inputs ([`Prediction`] on the client,
//! [`InputQueue`] on the server), replay of the inputs the server hasn't
//! confirmed when its state arrives, and [`Smoothing`] that hides the jump
//! a correction makes. Like the snapshot helpers, nothing here touches the
//! network: you encode the inputs and states in your own packets.

use std::collections::VecDeque;

use glam::{Quat, Vec2, Vec3};

use super::sequence::seq_diff;
use super::snapshots::Pose;

/// Inputs a [`Prediction`] or [`InputQueue`] keeps unless told otherwise:
/// about 4 seconds at 60 ticks a second.
const DEFAULT_CAPACITY: usize = 256;

/// What [`Prediction::reconcile`] did with a server state.
/// ```
/// use ponko::net::Reconciled;
/// let r: ponko::net::Reconciled<f32> = ponko::net::Reconciled::Corrected { before: 1.0, replayed: 3 };
/// assert!(r.corrected());
/// assert!(!ponko::net::Reconciled::<f32>::Confirmed.corrected());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum Reconciled<S> {
    /// Older than (or the same as) a state already reconciled, or for an
    /// input that was never predicted: nothing changed.
    Ignored,
    /// The server agrees with what was predicted for that input: nothing
    /// changed, nothing was replayed.
    Confirmed,
    /// The server disagreed: the prediction restarted from its state and
    /// replayed the inputs it hasn't confirmed yet.
    Corrected {
        /// The predicted state just before the correction, for
        /// [`Smoothing::correct`].
        before: S,
        /// How many inputs were replayed on top of the server's state.
        replayed: usize,
    },
}

impl<S> Reconciled<S> {
    /// Whether the predicted state changed.
    /// ```
    /// assert!(!ponko::net::Reconciled::<f32>::Ignored.corrected());
    /// ```
    pub fn corrected(&self) -> bool {
        matches!(self, Self::Corrected { .. })
    }
}

/// Counters of a [`Prediction`] ([`Prediction::stats`]).
/// ```
/// let p = ponko::net::Prediction::<f32, f32>::new(0.0);
/// assert_eq!(p.stats(), ponko::net::PredictionStats::default());
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PredictionStats {
    /// Ticks predicted.
    pub predicted: u64,
    /// Server states that matched the prediction.
    pub confirmed: u64,
    /// Server states that didn't: the prediction was corrected.
    pub corrections: u64,
    /// Inputs replayed by corrections, in total.
    pub replayed: u64,
    /// Inputs dropped unconfirmed because the history was full (the server
    /// stopped answering for longer than the capacity covers).
    pub dropped: u64,
}

#[derive(Clone, Debug)]
struct Pending<S, I> {
    seq: u16,
    input: I,
    /// The predicted state after this input.
    after: S,
}

/// Client-side prediction of a state the client controls (its character,
/// the car it drives): every tick, apply the local input at once instead of
/// waiting a round trip for the server, and keep the inputs the server
/// hasn't confirmed. When the server's state for an input arrives, compare;
/// if it differs, start again from the server's state and replay the newer
/// inputs ([`reconcile`](Self::reconcile)).
///
/// `S` is whatever the step changes (a position, a velocity, a grounded
/// flag); `I` is one tick's input. You give the step as a closure, the same
/// one the server runs for that input, so predicting and replaying move
/// the same way. Inputs are numbered with a wrapping 16-bit sequence; send
/// [`unacked`](Self::unacked) inputs every tick (repeating them covers lost
/// packets) and have the server answer with its state and the number of
/// the last input it applied ([`InputQueue`]).
///
/// ```
/// use ponko::net::{InputQueue, Prediction, Reconciled};
///
/// // A 1D walker: the input is a speed, the state a position.
/// let step = |x: &mut f32, speed: &f32| *x += speed * 0.1;
/// let mut client = Prediction::new(0.0f32);
/// let mut server = InputQueue::new();
/// let mut server_x = 0.0f32;
///
/// // Three ticks: the client moves at once.
/// for _ in 0..3 {
///     client.predict(10.0, step);
/// }
/// assert_eq!(*client.state(), 3.0);
///
/// // The first two inputs reach the server (the wire format is yours).
/// for (seq, input) in client.unacked().take(2) {
///     server.push(seq, *input);
/// }
/// while let Some((_, input)) = server.pop() {
///     step(&mut server_x, &input);
/// }
/// server_x += 0.5; // something the client didn't know pushed the player
///
/// // The answer: the server's state after its last input.
/// let r = client.reconcile(server.ack().unwrap(), server_x, step);
/// assert!(matches!(r, Reconciled::Corrected { replayed: 1, .. }));
/// assert_eq!(*client.state(), 3.5); // the push, plus the input still in flight
/// assert_eq!(client.unacked().len(), 1);
/// ```
#[derive(Clone, Debug)]
pub struct Prediction<S, I> {
    state: S,
    previous: S,
    pending: VecDeque<Pending<S, I>>,
    next_seq: u16,
    /// The newest input the server confirmed (by reconciling).
    acked: Option<u16>,
    /// Any input predicted yet (the first is sequence 0)?
    started: bool,
    capacity: usize,
    stats: PredictionStats,
}

impl<S: Clone + PartialEq, I> Prediction<S, I> {
    /// Predicting from `state`, keeping up to 256 unconfirmed inputs.
    /// ```
    /// let p = ponko::net::Prediction::<f32, u8>::new(5.0);
    /// assert_eq!(*p.state(), 5.0);
    /// ```
    pub fn new(state: S) -> Self {
        Self::with_capacity(state, DEFAULT_CAPACITY)
    }

    /// Predicting from `state`, keeping up to `capacity` unconfirmed inputs
    /// (more than a round trip's worth of ticks; when the server stops
    /// answering, the oldest are dropped). Panics unless `capacity` is
    /// 1..=32768 (half the sequence range, so numbers stay unambiguous).
    /// ```
    /// let p = ponko::net::Prediction::<f32, u8>::with_capacity(0.0, 64);
    /// assert_eq!(p.unacked().len(), 0);
    /// ```
    pub fn with_capacity(state: S, capacity: usize) -> Self {
        assert!(
            (1..=32768).contains(&capacity),
            "Prediction: capacity must be 1..=32768"
        );
        Self {
            previous: state.clone(),
            state,
            pending: VecDeque::with_capacity(capacity),
            next_seq: 0,
            acked: None,
            started: false,
            capacity,
            stats: PredictionStats::default(),
        }
    }

    /// Runs one tick: `step` applies `input` to the predicted state, and the
    /// input is kept (with its sequence number, returned) until the server
    /// confirms it. Call it once per fixed tick, in the same place the
    /// server applies inputs.
    /// ```
    /// let mut p = ponko::net::Prediction::new(0i32);
    /// assert_eq!(p.predict(2, |s, i| *s += i), 0);
    /// assert_eq!(p.predict(3, |s, i| *s += i), 1);
    /// assert_eq!((*p.state(), *p.previous()), (5, 2));
    /// ```
    pub fn predict(&mut self, input: I, step: impl FnOnce(&mut S, &I)) -> u16 {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        self.started = true;
        self.previous.clone_from(&self.state);
        step(&mut self.state, &input);
        if self.pending.len() == self.capacity {
            self.pending.pop_front();
            self.stats.dropped += 1;
        }
        self.pending.push_back(Pending {
            seq,
            input,
            after: self.state.clone(),
        });
        self.stats.predicted += 1;
        seq
    }

    /// Takes the server's `state` after input `ack`. If it matches what was
    /// predicted for that input (`==`, so give `S` a `PartialEq` with a
    /// tolerance if the two sides can differ by rounding), nothing changes.
    /// Otherwise the prediction starts again from `state` and `step` replays
    /// every newer input. Either way, inputs up to `ack` are forgotten.
    ///
    /// States for an older or the same `ack` as one already reconciled, or
    /// for an input never predicted, are [`Ignored`](Reconciled::Ignored),
    /// so packets may arrive late, twice or out of order.
    /// ```
    /// use ponko::net::Reconciled;
    /// let mut p = ponko::net::Prediction::new(0i32);
    /// for _ in 0..4 { p.predict(1, |s, i| *s += i); }
    /// assert_eq!(p.reconcile(1, 2, |s, i| *s += i), Reconciled::Confirmed);
    /// assert_eq!(p.reconcile(0, 9, |s, i| *s += i), Reconciled::Ignored); // late
    /// let r = p.reconcile(2, 10, |s, i| *s += i);
    /// assert_eq!(r, Reconciled::Corrected { before: 4, replayed: 1 });
    /// assert_eq!(*p.state(), 11);
    /// ```
    pub fn reconcile(&mut self, ack: u16, state: S, mut step: impl FnMut(&mut S, &I)) -> Reconciled<S> {
        if !self.started {
            return Reconciled::Ignored;
        }
        let newest = self.next_seq.wrapping_sub(1);
        if seq_diff(ack, newest) > 0 || self.acked.is_some_and(|a| seq_diff(ack, a) <= 0) {
            return Reconciled::Ignored;
        }
        // An ack for an input dropped from a full history is still news:
        // nothing to compare with, so it restarts from the server's state.
        self.acked = Some(ack);
        let mut confirmed = None;
        while let Some(p) = self.pending.front() {
            if seq_diff(p.seq, ack) > 0 {
                break;
            }
            let p = self.pending.pop_front().expect("front exists");
            if p.seq == ack {
                confirmed = Some(p.after);
            }
        }
        if confirmed.as_ref() == Some(&state) {
            self.stats.confirmed += 1;
            return Reconciled::Confirmed;
        }
        let before = std::mem::replace(&mut self.state, state);
        self.previous.clone_from(&self.state);
        let replayed = self.pending.len();
        for p in &mut self.pending {
            self.previous.clone_from(&self.state);
            step(&mut self.state, &p.input);
            p.after.clone_from(&self.state);
        }
        self.stats.corrections += 1;
        self.stats.replayed += replayed as u64;
        Reconciled::Corrected { before, replayed }
    }

    /// The predicted state after the newest input.
    /// ```
    /// assert_eq!(*ponko::net::Prediction::<i32, i32>::new(3).state(), 3);
    /// ```
    pub fn state(&self) -> &S {
        &self.state
    }

    /// The predicted state one tick earlier: draw the entity blended from
    /// this to [`state`](Self::state) by the time since the last tick.
    /// ```
    /// assert_eq!(*ponko::net::Prediction::<i32, i32>::new(3).previous(), 3);
    /// ```
    pub fn previous(&self) -> &S {
        &self.previous
    }

    /// Sets the predicted state outright (a respawn, a teleport the client
    /// starts) and forgets the unconfirmed inputs; numbering goes on. Server
    /// states for inputs predicted before it are ignored from now on, so one
    /// still in flight doesn't put the player back.
    /// ```
    /// use ponko::net::Reconciled;
    /// let mut p = ponko::net::Prediction::new(0i32);
    /// p.predict(1, |s, i| *s += i);
    /// p.reset(50);
    /// assert_eq!((*p.state(), p.unacked().len()), (50, 0));
    /// assert_eq!(p.reconcile(0, 1, |s, i| *s += i), Reconciled::Ignored);
    /// ```
    pub fn reset(&mut self, state: S) {
        self.previous.clone_from(&state);
        self.state = state;
        self.pending.clear();
        if self.started {
            self.acked = Some(self.next_seq.wrapping_sub(1));
        }
    }

    /// The inputs the server hasn't confirmed, oldest first, with their
    /// sequence numbers: send them (or the newest few, `.rev().take(n)`)
    /// every tick, so one packet that gets through makes up for the lost ones.
    /// ```
    /// let mut p = ponko::net::Prediction::new(0i32);
    /// p.predict(7, |s, i| *s += i);
    /// assert_eq!(p.unacked().collect::<Vec<_>>(), [(0, &7)]);
    /// ```
    pub fn unacked(&self) -> impl DoubleEndedIterator<Item = (u16, &I)> + ExactSizeIterator + '_ {
        self.pending.iter().map(|p| (p.seq, &p.input))
    }

    /// The newest input the server confirmed, once one was.
    /// ```
    /// assert_eq!(ponko::net::Prediction::<i32, i32>::new(0).acked(), None);
    /// ```
    pub fn acked(&self) -> Option<u16> {
        self.acked
    }

    /// The sequence number the next [`predict`](Self::predict) gives.
    /// ```
    /// assert_eq!(ponko::net::Prediction::<i32, i32>::new(0).next_seq(), 0);
    /// ```
    pub fn next_seq(&self) -> u16 {
        self.next_seq
    }

    /// What it did so far.
    /// ```
    /// let mut p = ponko::net::Prediction::new(0i32);
    /// p.predict(1, |s, i| *s += i);
    /// assert_eq!(p.stats().predicted, 1);
    /// ```
    pub fn stats(&self) -> PredictionStats {
        self.stats
    }
}

/// What happened to an input given to [`InputQueue::push`].
/// ```
/// assert_ne!(ponko::net::InputPush::Added, ponko::net::InputPush::Late);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPush {
    /// Queued, in sequence order.
    Added,
    /// Already queued: ignored.
    Duplicate,
    /// Not newer than an input already applied: ignored.
    Late,
}

/// Counters of an [`InputQueue`] ([`InputQueue::stats`]).
/// ```
/// assert_eq!(ponko::net::InputQueue::<u8>::new().stats().applied, 0);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputStats {
    /// Inputs queued.
    pub received: u64,
    /// Copies of queued inputs (resends that weren't needed).
    pub duplicates: u64,
    /// Inputs that came after a newer one was applied.
    pub late: u64,
    /// Inputs taken with [`InputQueue::pop`].
    pub applied: u64,
    /// Sequence numbers that never arrived before a newer input was taken
    /// (lost in every packet that carried them), or were dropped because the
    /// queue was full.
    pub skipped: u64,
}

/// The server's side of [`Prediction`]: one client's numbered inputs, in
/// order, each applied once. Push every input a packet carries (resent ones
/// too: duplicates and inputs already applied are ignored), and take them
/// with [`pop`](Self::pop) in the simulation. Answer the client with your
/// state and [`ack`](Self::ack), the number of the last input applied.
///
/// Two ways to apply them:
/// - **One step per input** (`while let Some(..) = queue.pop()`): the
///   server state after input `n` is exactly what the client predicted for
///   `n` (given the same step and world). A late packet makes the entity
///   catch up in one tick; limit the inputs per tick if that must not
///   happen (a client sending too many is cheating or its clock runs fast).
/// - **One input per tick**, when the step is the engine's (a physics body,
///   a `d3::Character` stepped with everything else):
///   when no input has arrived, keep the [`last`](Self::last) one. Those
///   ticks aren't any input's, so the client is corrected after them.
///
/// ```
/// use ponko::net::{InputPush, InputQueue};
/// let mut q = InputQueue::new();
/// assert_eq!(q.push(1, 'b'), InputPush::Added);
/// assert_eq!(q.push(0, 'a'), InputPush::Added); // reordered
/// assert_eq!(q.push(1, 'b'), InputPush::Duplicate);
/// assert_eq!(q.pop(), Some((0, 'a')));
/// assert_eq!(q.pop(), Some((1, 'b')));
/// assert_eq!(q.pop(), None);
/// assert_eq!(q.push(0, 'a'), InputPush::Late);
/// assert_eq!((q.ack(), q.last()), (Some(1), Some(&'b')));
/// ```
#[derive(Clone, Debug)]
pub struct InputQueue<I> {
    queued: VecDeque<(u16, I)>,
    /// The last input taken.
    last: Option<(u16, I)>,
    capacity: usize,
    stats: InputStats,
}

impl<I: Clone> Default for InputQueue<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I: Clone> InputQueue<I> {
    /// An empty queue holding up to 256 inputs.
    /// ```
    /// assert!(ponko::net::InputQueue::<u8>::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// An empty queue holding up to `capacity` inputs: when a client sends
    /// faster than the server applies, the oldest are dropped. Panics
    /// unless `capacity` is 1..=32768.
    /// ```
    /// let mut q = ponko::net::InputQueue::with_capacity(2);
    /// for seq in 0..3 { q.push(seq, ()); }
    /// assert_eq!(q.pop(), Some((1, ())));
    /// assert_eq!(q.stats().skipped, 1);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(
            (1..=32768).contains(&capacity),
            "InputQueue: capacity must be 1..=32768"
        );
        Self {
            queued: VecDeque::with_capacity(capacity),
            last: None,
            capacity,
            stats: InputStats::default(),
        }
    }

    /// Queues input `seq` (in order, whatever order packets came in).
    pub fn push(&mut self, seq: u16, input: I) -> InputPush {
        if self.last.as_ref().is_some_and(|(last, _)| seq_diff(seq, *last) <= 0) {
            self.stats.late += 1;
            return InputPush::Late;
        }
        let at = self.queued.partition_point(|(s, _)| seq_diff(*s, seq) < 0);
        if self.queued.get(at).is_some_and(|(s, _)| *s == seq) {
            self.stats.duplicates += 1;
            return InputPush::Duplicate;
        }
        self.queued.insert(at, (seq, input));
        self.stats.received += 1;
        if self.queued.len() > self.capacity {
            let (dropped, input) = self.queued.pop_front().expect("over capacity");
            // Applied as far as numbering goes: later copies are late.
            self.count_skipped(dropped);
            self.last = Some((dropped, input));
            self.stats.skipped += 1;
        }
        InputPush::Added
    }

    fn count_skipped(&mut self, next: u16) {
        if let Some((last, _)) = &self.last {
            self.stats.skipped += (seq_diff(next, *last) as i64 - 1).max(0) as u64;
        }
    }

    /// Takes the oldest queued input, if any. Inputs that never arrived
    /// before it are skipped (counted in [`InputStats::skipped`]).
    pub fn pop(&mut self) -> Option<(u16, I)> {
        let (seq, input) = self.queued.pop_front()?;
        self.count_skipped(seq);
        self.last = Some((seq, input.clone()));
        self.stats.applied += 1;
        Some((seq, input))
    }

    /// The number of the last input taken: send it with your state.
    /// ```
    /// assert_eq!(ponko::net::InputQueue::<u8>::new().ack(), None);
    /// ```
    pub fn ack(&self) -> Option<u16> {
        self.last.as_ref().map(|(seq, _)| *seq)
    }

    /// The last input taken: what to keep applying while none arrive.
    /// ```
    /// assert_eq!(ponko::net::InputQueue::<u8>::new().last(), None);
    /// ```
    pub fn last(&self) -> Option<&I> {
        self.last.as_ref().map(|(_, input)| input)
    }

    /// Inputs queued and not taken yet.
    /// ```
    /// assert_eq!(ponko::net::InputQueue::<u8>::new().len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.queued.len()
    }

    /// Whether nothing is queued.
    /// ```
    /// assert!(ponko::net::InputQueue::<u8>::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.queued.is_empty()
    }

    /// Forgets everything, for a client that reconnected (its numbering
    /// starts again).
    /// ```
    /// let mut q = ponko::net::InputQueue::new();
    /// q.push(5, ());
    /// q.pop();
    /// q.clear();
    /// assert_eq!(q.ack(), None);
    /// ```
    pub fn clear(&mut self) {
        self.queued.clear();
        self.last = None;
    }

    /// What it did so far.
    /// ```
    /// let mut q = ponko::net::InputQueue::new();
    /// q.push(0, ());
    /// q.push(0, ());
    /// assert_eq!(q.stats().duplicates, 1);
    /// ```
    pub fn stats(&self) -> InputStats {
        self.stats
    }
}

/// A value whose corrections [`Smoothing`] can spread over time: an offset
/// between two values, added back in part. Implemented for `f32`, `Vec2`,
/// `Vec3`, `Quat` and [`Pose`] (position and rotation).
/// ```
/// use ponko::net::Smooth;
/// use ponko::prelude::*;
/// let off = <Vec2 as ponko::net::Smooth>::offset(&vec2(3.0, 0.0), &vec2(1.0, 0.0));
/// assert_eq!(<Vec2 as ponko::net::Smooth>::add_offset(&Vec2::ZERO, &off, 0.5), vec2(1.0, 0.0));
/// assert_eq!(<Vec2 as ponko::net::Smooth>::offset_size(&off), 2.0);
/// ```
pub trait Smooth: Copy {
    /// What takes `to` back to `from`: the jump from `from` to `to`, reversed.
    fn offset(from: &Self, to: &Self) -> Self;
    /// `value` moved by the share `k` (0..=1) of `offset`.
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self;
    /// How big `offset` is: a distance (an angle in radians for a rotation).
    fn offset_size(offset: &Self) -> f32;
}

impl Smooth for f32 {
    fn offset(from: &Self, to: &Self) -> Self {
        from - to
    }
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self {
        value + offset * k
    }
    fn offset_size(offset: &Self) -> f32 {
        offset.abs()
    }
}
impl Smooth for Vec2 {
    fn offset(from: &Self, to: &Self) -> Self {
        *from - *to
    }
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self {
        *value + *offset * k
    }
    fn offset_size(offset: &Self) -> f32 {
        offset.length()
    }
}
impl Smooth for Vec3 {
    fn offset(from: &Self, to: &Self) -> Self {
        *from - *to
    }
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self {
        *value + *offset * k
    }
    fn offset_size(offset: &Self) -> f32 {
        offset.length()
    }
}
impl Smooth for Quat {
    fn offset(from: &Self, to: &Self) -> Self {
        let q = (*from * to.inverse()).normalize();
        // The short way round.
        if q.w < 0.0 { -q } else { q }
    }
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self {
        (Quat::IDENTITY.slerp(*offset, k) * *value).normalize()
    }
    fn offset_size(offset: &Self) -> f32 {
        offset.angle_between(Quat::IDENTITY)
    }
}
/// Position and rotation; the velocity is the value's own, never smoothed.
impl Smooth for Pose {
    fn offset(from: &Self, to: &Self) -> Self {
        Self {
            position: Vec3::offset(&from.position, &to.position),
            rotation: Quat::offset(&from.rotation, &to.rotation),
            velocity: None,
        }
    }
    fn add_offset(value: &Self, offset: &Self, k: f32) -> Self {
        Self {
            position: Vec3::add_offset(&value.position, &offset.position, k),
            rotation: Quat::add_offset(&value.rotation, &offset.rotation, k),
            velocity: value.velocity,
        }
    }
    fn offset_size(offset: &Self) -> f32 {
        Vec3::offset_size(&offset.position).max(Quat::offset_size(&offset.rotation))
    }
}

/// Hides the jump of a correction: the entity is drawn where it was shown
/// before and slides to the corrected place over a short time, while the
/// simulation uses the corrected state at once. Give it the jump with
/// [`correct`](Self::correct) and draw [`apply`](Self::apply) of the
/// predicted value; [`update`](Self::update) it every frame.
///
/// The leftover offset halves every [`half_life`](Self::half_life)
/// seconds (default 0.1), so a small correction fades in a few frames and
/// a big one doesn't look like a teleport. Jumps bigger than
/// [`snap`](Self::snap) are shown at once (a respawn).
///
/// ```
/// use ponko::net::Smoothing;
/// use ponko::prelude::*;
/// let mut smooth = Smoothing::new();
/// // A correction moved the player from x = 10 to x = 9.
/// smooth.correct(vec2(10.0, 0.0), vec2(9.0, 0.0));
/// assert_eq!(smooth.apply(vec2(9.0, 0.0)), vec2(10.0, 0.0)); // still drawn where it was
/// smooth.update(0.1);
/// assert!((smooth.apply(vec2(9.0, 0.0)).x - 9.5).abs() < 1e-5); // halfway there
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Smoothing<T: Smooth> {
    /// Seconds for the leftover offset to halve. 0 shows corrections at once.
    pub half_life: f32,
    /// Corrections bigger than this (in [`Smooth::offset_size`] units) are
    /// shown at once. Default: never.
    pub snap: f32,
    offset: Option<T>,
    left: f32,
}

impl<T: Smooth> Default for Smoothing<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// An offset smaller than this share of the original is gone.
const SETTLED: f32 = 1e-3;

impl<T: Smooth> Smoothing<T> {
    /// Halving every 0.1 s, never snapping.
    /// ```
    /// let s = ponko::net::Smoothing::<f32>::new();
    /// assert_eq!((s.half_life, s.snap), (0.1, f32::INFINITY));
    /// ```
    pub fn new() -> Self {
        Self {
            half_life: 0.1,
            snap: f32::INFINITY,
            offset: None,
            left: 0.0,
        }
    }

    /// A correction moved the value from `from` to `to`. The offset still
    /// left from earlier corrections is kept: what is drawn doesn't move.
    /// ```
    /// let mut s = ponko::net::Smoothing::new();
    /// s.correct(1.0f32, 0.0);
    /// s.correct(0.0, -1.0); // a second correction before the first faded
    /// assert_eq!(s.apply(-1.0), 1.0);
    /// ```
    pub fn correct(&mut self, from: T, to: T) {
        let shown = self.apply(from);
        let offset = T::offset(&shown, &to);
        if T::offset_size(&offset) > self.snap || self.half_life <= 0.0 {
            self.offset = None;
            self.left = 0.0;
        } else {
            self.offset = Some(offset);
            self.left = 1.0;
        }
    }

    /// Lets `dt` seconds of the offset fade.
    /// ```
    /// let mut s = ponko::net::Smoothing::new();
    /// s.correct(1.0f32, 0.0);
    /// s.update(10.0);
    /// assert!(!s.is_active());
    /// ```
    pub fn update(&mut self, dt: f32) {
        if self.offset.is_none() {
            return;
        }
        self.left *= (-std::f32::consts::LN_2 * dt.max(0.0) / self.half_life.max(f32::EPSILON)).exp();
        let size = self.offset.as_ref().map_or(0.0, |o| T::offset_size(o));
        if self.left < SETTLED || size * self.left < 1e-6 {
            self.offset = None;
            self.left = 0.0;
        }
    }

    /// Where to draw `value`: moved by what is left of the offset.
    /// ```
    /// assert_eq!(ponko::net::Smoothing::<f32>::new().apply(3.0), 3.0);
    /// ```
    pub fn apply(&self, value: T) -> T {
        match &self.offset {
            Some(offset) => T::add_offset(&value, offset, self.left),
            None => value,
        }
    }

    /// How far from the predicted value the drawn one still is.
    /// ```
    /// let mut s = ponko::net::Smoothing::new();
    /// s.correct(2.0f32, 0.0);
    /// assert_eq!(s.error(), 2.0);
    /// ```
    pub fn error(&self) -> f32 {
        self.offset.as_ref().map_or(0.0, |o| T::offset_size(o) * self.left)
    }

    /// Whether a correction is still fading.
    /// ```
    /// assert!(!ponko::net::Smoothing::<f32>::new().is_active());
    /// ```
    pub fn is_active(&self) -> bool {
        self.offset.is_some()
    }

    /// Drops what is left of the offset: draw the predicted value as it is.
    /// ```
    /// let mut s = ponko::net::Smoothing::new();
    /// s.correct(2.0f32, 0.0);
    /// s.clear();
    /// assert_eq!(s.apply(0.0), 0.0);
    /// ```
    pub fn clear(&mut self) {
        self.offset = None;
        self.left = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(x: &mut i64, input: &i64) {
        *x += input;
    }

    #[test]
    fn sequence_numbers_wrap_through_reconcile() {
        let mut p = Prediction::new(0i64);
        // Start near the wrap.
        p.next_seq = 65530;
        for _ in 0..10 {
            p.predict(1, step);
        }
        assert_eq!(p.next_seq(), 4);
        // Server confirmed through seq 2 (13th... the 9th input) with a
        // state off by 100.
        let r = p.reconcile(2, 109, step);
        assert_eq!(
            r,
            Reconciled::Corrected {
                before: 10,
                replayed: 1
            }
        );
        assert_eq!(*p.state(), 110);
        assert_eq!(p.acked(), Some(2));
        assert_eq!(p.reconcile(65535, 0, step), Reconciled::Ignored);
    }

    #[test]
    fn acks_for_inputs_never_predicted_are_ignored() {
        let mut p = Prediction::<i64, i64>::new(0);
        assert_eq!(p.reconcile(0, 5, step), Reconciled::Ignored);
        p.predict(1, step);
        assert_eq!(p.reconcile(1, 5, step), Reconciled::Ignored);
        assert_eq!(*p.state(), 1);
    }

    #[test]
    fn a_full_history_drops_the_oldest_and_still_reconciles() {
        let mut p = Prediction::with_capacity(0i64, 4);
        for _ in 0..6 {
            p.predict(1, step);
        }
        assert_eq!(p.stats().dropped, 2);
        assert_eq!(p.unacked().map(|(s, _)| s).collect::<Vec<_>>(), [2, 3, 4, 5]);
        // An ack for a dropped input (1): restart from it, replay the 4 kept.
        assert_eq!(
            p.reconcile(1, 0, step),
            Reconciled::Corrected { before: 6, replayed: 4 }
        );
        assert_eq!(*p.state(), 4);
    }

    #[test]
    fn previous_is_one_replayed_step_behind_after_a_correction() {
        let mut p = Prediction::new(0i64);
        for _ in 0..3 {
            p.predict(1, step);
        }
        p.reconcile(0, 10, step);
        assert_eq!((*p.previous(), *p.state()), (11, 12));
    }

    #[test]
    fn input_queue_skips_inputs_lost_in_every_packet() {
        let mut q = InputQueue::new();
        q.push(0, 0);
        q.push(3, 3);
        assert_eq!(q.pop(), Some((0, 0)));
        assert_eq!(q.pop(), Some((3, 3)));
        assert_eq!(q.stats().skipped, 2);
        assert_eq!(q.push(1, 1), InputPush::Late);
    }

    #[test]
    fn input_queue_orders_across_the_wrap() {
        let mut q = InputQueue::new();
        q.push(1, 'c');
        q.push(65535, 'a');
        q.push(0, 'b');
        let order: Vec<_> = std::iter::from_fn(|| q.pop()).map(|(_, c)| c).collect();
        assert_eq!(order, ['a', 'b', 'c']);
    }

    #[test]
    fn quat_smoothing_takes_the_short_way() {
        let a = Quat::from_rotation_y(0.1);
        let b = -Quat::from_rotation_y(-0.1); // same rotation as +, other sign
        let off = Quat::offset(&a, &b);
        assert!((Quat::offset_size(&off) - 0.2).abs() < 1e-4);
        let half = Quat::add_offset(&b, &off, 0.5);
        assert!(half.angle_between(Quat::IDENTITY) < 1e-3);
    }

    #[test]
    fn smoothing_snaps_big_jumps() {
        let mut s = Smoothing::new();
        s.snap = 5.0;
        s.correct(Vec3::ZERO, Vec3::X * 10.0);
        assert!(!s.is_active());
        s.correct(Vec3::ZERO, Vec3::X);
        assert!(s.is_active());
    }
}
