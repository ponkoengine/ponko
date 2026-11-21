//! Inputs, prediction, events and interpolation for games that declare
//! their types at run time (from C and C++): values are plain bytes whose
//! fields the declaration lists ([`RawField`]), instead of Rust types.
//!
//! - [`RawValue`]: an input (or a predicted state) as bytes with their
//!   layout, travelling as a [`Replicate`] value does.
//! - [`RawPredicted`]: a predicted component kind whose step is a function
//!   over bytes; each entity's predictions live in one [`RawPredictions`]
//!   component, one per kind.
//! - [`RawInterpolated`]: an interpolated component kind; its float fields
//!   are shown between received values, the others set when they arrive.
//! - [`RawNetEvent`]: an event of a kind declared by name, delivered with
//!   its bytes.

use std::sync::{Arc, Mutex};

use ponko_ecs::{Component, ComponentId, Entity, World};

use super::ClientStats;
use super::fields::{Fields, Quantize, Replicate, quantize_in_place};
use super::inputs::{PredictedKind, Unapplied};
use super::kinds::{KindAccess, Raw, RawField, RawType, visit_raw};
use crate::net::{Interpolate, PeerId, Prediction, Reconciled, SnapshotBuffer, SnapshotConfig};

/// Bytes with the fields that travel: an input, or a predicted state, of a
/// type declared at run time.
#[derive(Clone, Debug, Default)]
pub(crate) struct RawValue {
    /// The fields, all inside `bytes` (checked when declared). Empty for
    /// the default value, which has no fields.
    pub(crate) fields: Arc<[RawField]>,
    pub(crate) bytes: Vec<u8>,
}

impl RawValue {
    /// A zeroed value of `size` bytes with `fields`.
    pub(crate) fn zeroed(fields: Arc<[RawField]>, size: usize) -> Self {
        Self {
            fields,
            bytes: vec![0; size],
        }
    }

    /// The same layout holding `bytes` (copied; missing bytes stay zero,
    /// extra ones are ignored).
    pub(crate) fn with_bytes(&self, bytes: &[u8]) -> Self {
        let mut v = self.clone();
        let n = bytes.len().min(v.bytes.len());
        v.bytes[..n].copy_from_slice(&bytes[..n]);
        v.bytes[n..].fill(0);
        v
    }
}

/// Two states are the same prediction when their bytes are.
impl PartialEq for RawValue {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Replicate for RawValue {
    fn fields(&mut self, f: &mut Fields<'_>) {
        let fields = Arc::clone(&self.fields);
        // SAFETY: a value's fields lie inside its bytes: `zeroed` and
        // `with_bytes` keep the declared size, which the declaration
        // checked every field against.
        unsafe { visit_raw(&fields, self.bytes.as_mut_ptr(), f) };
    }
}

/// Checks that every field of a run-time declaration lies inside `size`
/// bytes and that its quantization fits its type.
pub(crate) fn check_fields(what: &str, name: &str, size: usize, fields: &[RawField]) {
    for f in fields {
        assert!(
            f.offset.checked_add(f.ty.size()).is_some_and(|end| end <= size),
            "Replication::{what}: field `{}` of `{name}` lies outside its {size} bytes",
            f.name
        );
    }
}

/// The step of a predicted kind declared at run time: changes the state's
/// bytes for one input's bytes.
pub(crate) type RawStep = Arc<dyn Fn(&mut [u8], &[u8]) + Send + Sync>;

/// One kind's prediction on one entity.
struct RawPredictionState {
    kind: usize,
    prediction: Prediction<RawValue, RawValue>,
    epoch: u16,
    start: u16,
}

/// Component on a client's own predicted entity, for kinds declared at run
/// time: the prediction of each of its predicted kinds.
#[derive(Component, Default)]
pub(crate) struct RawPredictions {
    states: Vec<RawPredictionState>,
}

impl RawPredictions {
    fn get_mut(&mut self, kind: usize) -> Option<&mut RawPredictionState> {
        self.states.iter_mut().find(|s| s.kind == kind)
    }
}

/// A predicted component kind declared at run time.
pub(crate) struct RawPredicted {
    pub(crate) kind: usize,
    raw: Arc<Raw>,
    step: RawStep,
    words: usize,
}

impl RawPredicted {
    pub(crate) fn new(kind: usize, raw: Arc<Raw>, step: RawStep) -> Self {
        let words = raw.schema().len();
        Self { kind, raw, step, words }
    }

    /// The entity's component as a value (zeroed if it has none).
    fn value_of(&self, world: &World, e: Entity) -> RawValue {
        let mut v = RawValue::zeroed(self.raw.fields.clone().into(), self.raw.size);
        if let Some(p) = world.raw_ref().get_dynamic(e, self.raw.id) {
            // SAFETY: `p` points to the entity's live value of `size` bytes.
            unsafe { std::ptr::copy_nonoverlapping(p, v.bytes.as_mut_ptr(), self.raw.size) };
        }
        v
    }

    /// Writes `value` into the entity's component (adding it if missing).
    fn store(&self, world: &mut World, e: Entity, value: &RawValue) {
        if !world.is_alive(e) {
            return;
        }
        let (id, size) = (self.raw.id, self.raw.size);
        if world.raw_ref().get_dynamic(e, id).is_none() {
            // SAFETY: a value of the registered size; registration requires
            // plain bytes without a destructor.
            unsafe { world.raw().insert_dynamic(e, id, value.bytes.as_ptr()) };
        } else if let Some(p) = world.raw().get_dynamic_mut(e, id) {
            // SAFETY: `p` points to the entity's live value of `size` bytes.
            unsafe { std::ptr::copy_nonoverlapping(value.bytes.as_ptr(), p, size) };
            world.raw().mark_changed_dynamic(e, id);
        }
    }

    /// `step`, then the rounding the wire does (as typed prediction does).
    fn step_quantized(&self, state: &mut RawValue, input: &RawValue, scratch: &mut [u32]) {
        (self.step)(&mut state.bytes, &input.bytes);
        quantize_in_place(state, scratch);
    }
}

impl PredictedKind<RawValue> for RawPredicted {
    fn kind(&self) -> usize {
        self.kind
    }

    fn server_step(&self, world: &mut World, e: Entity, input: &RawValue, scratch: &mut Vec<u32>) {
        scratch.resize(self.words, 0);
        if world.raw_ref().get_dynamic(e, self.raw.id).is_none() {
            return;
        }
        let mut state = self.value_of(world, e);
        self.step_quantized(&mut state, input, scratch);
        self.store(world, e, &state);
    }

    fn client_predict(&self, world: &mut World, e: Entity, input: &RawValue, seq: u16, scratch: &mut Vec<u32>) {
        scratch.resize(self.words, 0);
        let Some(ps) = world.get_mut::<RawPredictions>(e).and_then(|p| p.get_mut(self.kind)) else {
            return;
        };
        let got = ps
            .prediction
            .predict(input.clone(), |s, i| self.step_quantized(s, i, scratch));
        debug_assert_eq!(got, seq, "predictions follow the input stream's numbers");
        let state = ps.prediction.state().clone();
        self.store(world, e, &state);
    }

    fn client_state(
        &self,
        world: &mut World,
        e: Entity,
        words: &[u32],
        ack: Option<u16>,
        epoch: u16,
        unsent: Unapplied<'_, RawValue>,
        scratch: &mut Vec<u32>,
        stats: &mut ClientStats,
    ) {
        scratch.resize(self.words, 0);
        let mut server = self.value_of(world, e);
        super::fields::read_words(&mut server, words);
        let fresh = world
            .get::<RawPredictions>(e)
            .and_then(|p| p.states.iter().find(|s| s.kind == self.kind))
            .is_none_or(|s| s.epoch != epoch);
        if fresh {
            let pending = unsent
                .inputs
                .iter()
                .filter(|(seq, _)| ack.is_none_or(|a| crate::net::seq_newer(*seq, a)));
            let start = pending.clone().next().map_or(unsent.next_seq, |(seq, _)| *seq);
            let mut prediction = Prediction::<RawValue, RawValue>::new(server).starting_at(start);
            for (_, input) in pending {
                prediction.predict(input.clone(), |s, i| self.step_quantized(s, i, scratch));
            }
            let state = prediction.state().clone();
            let entry = RawPredictionState {
                kind: self.kind,
                prediction,
                epoch,
                start,
            };
            if !world.has::<RawPredictions>(e) {
                world.insert(e, RawPredictions::default());
            }
            if let Some(p) = world.get_mut::<RawPredictions>(e) {
                p.states.retain(|s| s.kind != self.kind);
                p.states.push(entry);
            }
            self.store(world, e, &state);
            return;
        }
        let Some(ack) = ack else { return };
        let Some(ps) = world.get_mut::<RawPredictions>(e).and_then(|p| p.get_mut(self.kind)) else {
            return;
        };
        if crate::net::seq_diff(ack, ps.start) < 0 {
            return;
        }
        match ps
            .prediction
            .reconcile(ack, server, |s, i| self.step_quantized(s, i, scratch))
        {
            Reconciled::Confirmed => stats.confirmed += 1,
            Reconciled::Corrected { replayed, .. } => {
                stats.corrections += 1;
                stats.replayed += replayed as u64;
            }
            Reconciled::Ignored => {}
        }
        let state = ps.prediction.state().clone();
        self.store(world, e, &state);
    }

    fn release(&self, world: &mut World, e: Entity) {
        let empty = match world.get_mut::<RawPredictions>(e) {
            Some(p) => {
                p.states.retain(|s| s.kind != self.kind);
                p.states.is_empty()
            }
            None => false,
        };
        if empty {
            world.remove::<RawPredictions>(e);
        }
    }
}

/// Float fields an interpolated kind declared at run time can have.
pub(crate) const MAX_INTERPOLATED_FLOATS: usize = 32;

/// The float fields of one received value, for the snapshot buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RawSnap {
    values: [f32; MAX_INTERPOLATED_FLOATS],
    /// Which of them are angles (shortest way round).
    angles: u32,
    len: u8,
}

impl Default for RawSnap {
    fn default() -> Self {
        Self {
            values: [0.0; MAX_INTERPOLATED_FLOATS],
            angles: 0,
            len: 0,
        }
    }
}

/// The signed difference `b - a` of two angles, the short way round.
fn angle_delta(a: f32, b: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let d = (b - a).rem_euclid(tau);
    if d > tau / 2.0 { d - tau } else { d }
}

impl Interpolate for RawSnap {
    fn interpolate(a: &Self, b: &Self, _span: f32, t: f32) -> Self {
        let mut out = *b;
        for i in 0..usize::from(b.len.min(a.len)) {
            let (x, y) = (a.values[i], b.values[i]);
            out.values[i] = if b.angles & (1 << i) != 0 {
                x + angle_delta(x, y) * t
            } else {
                x + (y - x) * t
            };
        }
        out
    }
    fn extrapolate(prev: &Self, last: &Self, span: f32, ahead: f32) -> Self {
        let mut out = *last;
        if span > 0.0 {
            for i in 0..usize::from(last.len.min(prev.len)) {
                let (x, y) = (prev.values[i], last.values[i]);
                let d = if last.angles & (1 << i) != 0 {
                    angle_delta(x, y)
                } else {
                    y - x
                };
                out.values[i] = y + d / span * ahead;
            }
        }
        out
    }
}

/// Client copies of interpolated kinds declared at run time: one buffer
/// per kind, keyed by the component's id.
#[derive(Component, Default)]
pub(crate) struct RawNetBuffers {
    buffers: Vec<(ComponentId, SnapshotBuffer<RawSnap>)>,
}

/// An interpolated component kind declared at run time.
pub(crate) struct RawInterpolated {
    raw: Raw,
    config: SnapshotConfig,
    /// The float fields: their offsets, and which are angles.
    floats: Vec<usize>,
    angles: u32,
    /// Entities and values to write while sampling (kept for its capacity).
    scratch: Mutex<Vec<(Entity, RawSnap)>>,
}

impl RawInterpolated {
    /// # Panics
    /// Past [`MAX_INTERPOLATED_FLOATS`] float fields.
    pub(crate) fn new(raw: Raw, config: SnapshotConfig, name: &str) -> Self {
        let float_fields: Vec<&RawField> = raw.fields.iter().filter(|f| f.ty == RawType::F32).collect();
        assert!(
            float_fields.len() <= MAX_INTERPOLATED_FLOATS,
            "Replication: `{name}` has {} float fields; an interpolated component declared at run time takes at most {MAX_INTERPOLATED_FLOATS}",
            float_fields.len()
        );
        let floats = float_fields.iter().map(|f| f.offset).collect();
        let angles = float_fields
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.quantize, Quantize::Angle { .. }))
            .fold(0u32, |m, (i, _)| m | 1 << i);
        Self {
            raw,
            config,
            floats,
            angles,
            scratch: Mutex::new(Vec::new()),
        }
    }

    fn snap_of(&self, bytes: &[u8]) -> RawSnap {
        let mut snap = RawSnap {
            angles: self.angles,
            len: self.floats.len() as u8,
            ..RawSnap::default()
        };
        for (slot, &offset) in snap.values.iter_mut().zip(&self.floats) {
            let mut b = [0u8; 4];
            b.copy_from_slice(&bytes[offset..offset + 4]);
            *slot = f32::from_ne_bytes(b);
        }
        snap
    }
}

impl KindAccess for RawInterpolated {
    fn read(&self, world: &World, e: Entity, out: &mut [u32]) -> bool {
        self.raw.read(world, e, out)
    }
    fn write(&self, world: &mut World, e: Entity, words: &[u32]) {
        self.raw.write(world, e, words);
    }
    fn remove(&self, world: &mut World, e: Entity) {
        self.raw.remove(world, e);
        self.unbuffer(world, e);
    }
    fn track(&self, world: &mut World) -> ComponentId {
        self.raw.track(world)
    }
    fn buffer(&self, world: &mut World, e: Entity, time: f64, words: &[u32]) -> bool {
        if !world.is_alive(e) {
            return true;
        }
        let (id, size) = (self.raw.id, self.raw.size);
        let had = world.raw_ref().get_dynamic(e, id).is_some();
        // The received value, over what the entity shows now.
        let mut value = vec![0u8; size.max(1)];
        if let Some(p) = world.raw_ref().get_dynamic(e, id) {
            // SAFETY: `p` points to the entity's live value of `size` bytes.
            unsafe { std::ptr::copy_nonoverlapping(p, value.as_mut_ptr(), size) };
        }
        let shown: Vec<u8> = value.clone();
        // SAFETY: `value` holds `size` bytes, which cover every field.
        unsafe { self.raw.visit(value.as_mut_ptr(), &mut Fields::reader(words)) };
        let snap = self.snap_of(&value);
        if had {
            // Fields that aren't floats change at once; floats wait for the
            // buffer's delay.
            for &offset in &self.floats {
                value[offset..offset + 4].copy_from_slice(&shown[offset..offset + 4]);
            }
            if let Some(p) = world.raw().get_dynamic_mut(e, id) {
                // SAFETY: as above.
                unsafe { std::ptr::copy_nonoverlapping(value.as_ptr(), p, size) };
            }
        } else {
            // SAFETY: a value of the registered size, plain bytes.
            unsafe { world.raw().insert_dynamic(e, id, value.as_ptr()) };
        }
        if !world.has::<RawNetBuffers>(e) {
            world.insert(e, RawNetBuffers::default());
        }
        if let Some(b) = world.get_mut::<RawNetBuffers>(e) {
            match b.buffers.iter_mut().find(|(k, _)| *k == id) {
                Some((_, buffer)) => {
                    buffer.push(time, snap);
                }
                None => {
                    let mut buffer = SnapshotBuffer::with_config(self.config);
                    buffer.push(time, snap);
                    b.buffers.push((id, buffer));
                }
            }
        }
        true
    }
    fn sample(&self, world: &mut World, remote_now: f64) {
        let id = self.raw.id;
        let mut todo = self.scratch.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        todo.clear();
        world.query::<(Entity, &RawNetBuffers)>().for_each(|(e, b)| {
            if let Some((_, buffer)) = b.buffers.iter().find(|(k, _)| *k == id)
                && let Some(s) = buffer.sample(remote_now)
            {
                todo.push((e, s.value));
            }
        });
        for (e, snap) in todo.iter() {
            let Some(p) = world.raw().get_dynamic_mut(*e, id) else {
                continue;
            };
            for (value, &offset) in snap.values.iter().zip(&self.floats) {
                // SAFETY: `offset + 4 <= size` (checked when declared), and
                // `p` points to the entity's live value.
                unsafe { p.add(offset).cast::<f32>().write_unaligned(*value) };
            }
        }
    }
    fn unbuffer(&self, world: &mut World, e: Entity) {
        let id = self.raw.id;
        let empty = match world.get_mut::<RawNetBuffers>(e) {
            Some(b) => {
                b.buffers.retain(|(k, _)| *k != id);
                b.buffers.is_empty()
            }
            None => false,
        };
        if empty {
            world.remove::<RawNetBuffers>(e);
        }
    }
}

/// An event of a kind declared at run time
/// ([`Replication::raw_event`](super::Replication::raw_event)), read from
/// [`ctx.events`](crate::ctx::Ctx::events) as itself: who sent it, the
/// server tick it was sent at, its kind and its bytes. Its layout is fixed
/// (`repr(C)`), so C reads a slice of them in place.
#[repr(C)]
pub struct RawNetEvent {
    from: u64,
    tick: u64,
    kind: u32,
    data: *mut u8,
    len: usize,
}

// SAFETY: the event owns its bytes (a leaked box, freed on drop) and
// nothing else refers to them; reading them from another thread is reading
// plain bytes.
unsafe impl Send for RawNetEvent {}
// SAFETY: as for Send: shared access only reads the owned bytes.
unsafe impl Sync for RawNetEvent {}

impl RawNetEvent {
    pub(crate) fn new(from: PeerId, tick: u64, kind: u16, bytes: &[u8]) -> Self {
        let boxed: Box<[u8]> = bytes.into();
        let len = boxed.len();
        Self {
            from: from.to_bits(),
            tick,
            kind: u32::from(kind),
            data: Box::into_raw(boxed).cast::<u8>(),
            len,
        }
    }

    /// Who sent it ([`PeerId::HOST`] for the server's own player).
    #[must_use]
    pub fn from(&self) -> PeerId {
        PeerId::from_bits(self.from)
    }

    /// The server tick it was sent at.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Its kind: the index [`Replication::raw_event_kind`](super::Replication::raw_event_kind)
    /// gives for its name.
    #[must_use]
    pub fn kind(&self) -> u16 {
        self.kind as u16
    }

    /// Its bytes, as sent.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        // SAFETY: `data` and `len` came from a boxed slice this event owns.
        unsafe { std::slice::from_raw_parts(self.data, self.len) }
    }

    /// Byte offsets of its fields (from, tick, kind, data, len) and its
    /// size, for bindings that read events in place.
    #[doc(hidden)]
    pub const LAYOUT: [usize; 6] = [
        std::mem::offset_of!(Self, from),
        std::mem::offset_of!(Self, tick),
        std::mem::offset_of!(Self, kind),
        std::mem::offset_of!(Self, data),
        std::mem::offset_of!(Self, len),
        std::mem::size_of::<Self>(),
    ];
}

impl Drop for RawNetEvent {
    fn drop(&mut self) {
        // SAFETY: `data` and `len` came from `Box::into_raw` of a boxed
        // slice in `new`, freed only here.
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.data, self.len)) });
    }
}

impl std::fmt::Debug for RawNetEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawNetEvent")
            .field("from", &self.from())
            .field("tick", &self.tick)
            .field("kind", &self.kind)
            .field("bytes", &self.bytes())
            .finish()
    }
}

/// Delivers an event of a kind declared at run time.
pub(crate) fn deliver_raw(
    events: &mut crate::events::Events,
    from: PeerId,
    tick: u64,
    kind: u16,
    bytes: &[u8],
) -> bool {
    events.send(RawNetEvent::new(from, tick, kind, bytes));
    true
}
