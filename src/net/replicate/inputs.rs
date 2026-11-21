//! Inputs and client-predicted entities: a client numbers its input every
//! tick, runs it at once through the step of each predicted component of
//! the entities it owns ([`Prediction`]), and repeats the inputs the server
//! hasn't applied in every packet. The server queues them per client
//! ([`InputQueue`]), runs each once through the same steps, and tells the
//! client the last one it applied, so the client can reconcile.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use ponko_ecs::{Component, Entity, World};

use super::ClientStats;
use super::bits::{BitReader, BitWriter};
use super::fields::{FieldSpec, Replicate, quantize_in_place, read_words, schema_of, schema_of_value, write_words};
use super::wire::INPUT_REDUNDANCY;
use crate::net::{InputPush, InputQueue, PeerId, Prediction, Reconciled};

/// Inputs a server applies for one client in one tick at most: enough to
/// catch up after a late packet, few enough that a client sending too many
/// can't move much faster than everyone else.
pub(crate) const MAX_INPUTS_PER_TICK: usize = 2;
/// Inputs a client keeps for resending until the server applies them.
const UNACKED: usize = 256;

/// The type-erased input stream of a replication (one input type per game).
pub(crate) trait InputStream: Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn name(&self) -> &str;
    fn specs(&self) -> &[FieldSpec];
    /// Kinds (by index) that are predicted, as a mask.
    fn predicted(&self) -> u64;

    /// Server: `count` inputs from `peer`'s packet, numbered from `first`.
    fn receive(&mut self, peer: PeerId, first: u16, count: usize, r: &mut BitReader<'_>) -> Option<()>;
    /// Server: the last input applied for `peer`.
    fn ack(&self, peer: PeerId) -> Option<u16>;
    /// Server: applies waiting inputs to the entities their senders own,
    /// and the host's own input to the host's.
    fn server_tick(&mut self, world: &mut World, owned: &[(PeerId, Entity)]);
    /// Server: `peer` left.
    fn forget(&mut self, peer: PeerId);

    /// Client: numbers this tick's input, predicts our entities with it,
    /// and writes the inputs the server hasn't applied into `w`.
    fn client_tick(&mut self, world: &mut World, mine: &[Entity], w: &mut BitWriter);
    /// Client: the server applied inputs up to `ack`.
    fn client_acked(&mut self, ack: u16);
    /// Client: the server's state of predicted kind `kind` of our entity `e`.
    #[allow(clippy::too_many_arguments)]
    fn client_state(
        &mut self,
        world: &mut World,
        e: Entity,
        kind: usize,
        words: &[u32],
        ack: Option<u16>,
        epoch: u16,
        stats: &mut ClientStats,
    );
    /// Client: `e` isn't ours any more (or is gone): stop predicting it.
    fn client_release(&mut self, world: &mut World, e: Entity);
    /// Forgets all runtime state (a new session).
    fn reset(&mut self);
}

/// One predicted component kind, typed on the input.
pub(crate) trait PredictedKind<I>: Send + Sync {
    fn kind(&self) -> usize;
    fn server_step(&self, world: &mut World, e: Entity, input: &I, scratch: &mut Vec<u32>);
    fn client_predict(&self, world: &mut World, e: Entity, input: &I, seq: u16, scratch: &mut Vec<u32>);
    #[allow(clippy::too_many_arguments)]
    fn client_state(
        &self,
        world: &mut World,
        e: Entity,
        words: &[u32],
        ack: Option<u16>,
        epoch: u16,
        unsent: Unapplied<'_, I>,
        scratch: &mut Vec<u32>,
        stats: &mut ClientStats,
    );
    fn release(&self, world: &mut World, e: Entity);
}

/// The inputs a client sent that the server hasn't applied yet (oldest
/// first, numbered without gaps), and the number of the next one.
#[derive(Clone, Copy)]
pub(crate) struct Unapplied<'a, I> {
    pub(crate) inputs: &'a VecDeque<(u16, I)>,
    pub(crate) next_seq: u16,
}

/// The step a predicted kind runs for one input, on both sides.
pub(crate) type Step<S, I> = Arc<dyn Fn(&mut S, &I) + Send + Sync>;

/// Component on a client's own predicted entity: its prediction, and the
/// authority epoch it was started under.
#[derive(Component)]
pub(crate) struct PredictionState<S: Send + Sync + 'static, I: Send + Sync + 'static> {
    prediction: Prediction<S, I>,
    epoch: u16,
    /// The first input predicted on this entity.
    start: u16,
}

pub(crate) struct Predicted<S, I> {
    pub(crate) kind: usize,
    pub(crate) step: Step<S, I>,
    words: usize,
}

impl<S: Replicate, I> Predicted<S, I> {
    pub(crate) fn new(kind: usize, step: Step<S, I>) -> Self {
        Self {
            kind,
            step,
            words: schema_of::<S>().len(),
        }
    }
}

/// `step`, then the rounding the wire does: both sides keep the state
/// exactly as it travels, so a prediction can match the server bit for bit.
fn step_quantized<S: Replicate, I>(step: &Step<S, I>, s: &mut S, input: &I, scratch: &mut [u32]) {
    step(s, input);
    quantize_in_place(s, scratch);
}

impl<S, I> PredictedKind<I> for Predicted<S, I>
where
    S: Replicate + Component + PartialEq,
    I: Replicate,
{
    fn kind(&self) -> usize {
        self.kind
    }

    fn server_step(&self, world: &mut World, e: Entity, input: &I, scratch: &mut Vec<u32>) {
        scratch.resize(self.words, 0);
        if let Some(s) = world.get_mut::<S>(e) {
            step_quantized(&self.step, s, input, scratch);
        }
    }

    fn client_predict(&self, world: &mut World, e: Entity, input: &I, seq: u16, scratch: &mut Vec<u32>) {
        scratch.resize(self.words, 0);
        let Some(ps) = world.get_mut::<PredictionState<S, I>>(e) else {
            return;
        };
        let step = &self.step;
        let got = ps
            .prediction
            .predict(input.clone(), |s, i| step_quantized(step, s, i, scratch));
        debug_assert_eq!(got, seq, "predictions follow the input stream's numbers");
        let state = ps.prediction.state().clone();
        if let Some(s) = world.get_mut::<S>(e) {
            *s = state;
        }
    }

    fn client_state(
        &self,
        world: &mut World,
        e: Entity,
        words: &[u32],
        ack: Option<u16>,
        epoch: u16,
        unsent: Unapplied<'_, I>,
        scratch: &mut Vec<u32>,
        stats: &mut ClientStats,
    ) {
        scratch.resize(self.words, 0);
        let mut server = world.get::<S>(e).cloned().unwrap_or_default();
        read_words(&mut server, words);
        let step = &self.step;
        let fresh = world.get::<PredictionState<S, I>>(e).is_none_or(|ps| ps.epoch != epoch);
        if fresh {
            // Ours from now on (or again, after a handoff): from the
            // server's state, replay what it hasn't applied yet, so the
            // prediction is exact from the start.
            let pending = unsent
                .inputs
                .iter()
                .filter(|(seq, _)| ack.is_none_or(|a| crate::net::seq_newer(*seq, a)));
            let start = pending.clone().next().map_or(unsent.next_seq, |(seq, _)| *seq);
            let mut prediction = Prediction::<S, I>::new(server).starting_at(start);
            for (_, input) in pending {
                prediction.predict(input.clone(), |s, i| step_quantized(step, s, i, scratch));
            }
            let state = prediction.state().clone();
            world.insert(
                e,
                PredictionState {
                    prediction,
                    epoch,
                    start,
                },
            );
            if let Some(s) = world.get_mut::<S>(e) {
                *s = state;
            } else {
                world.insert(e, state);
            }
            return;
        }
        let Some(ack) = ack else { return };
        let Some(ps) = world.get_mut::<PredictionState<S, I>>(e) else {
            return;
        };
        if crate::net::seq_diff(ack, ps.start) < 0 {
            return; // nothing applied since predicting started
        }
        match ps
            .prediction
            .reconcile(ack, server, |s, i| step_quantized(step, s, i, scratch))
        {
            Reconciled::Confirmed => stats.confirmed += 1,
            Reconciled::Corrected { replayed, .. } => {
                stats.corrections += 1;
                stats.replayed += replayed as u64;
            }
            Reconciled::Ignored => {}
        }
        let state = ps.prediction.state().clone();
        if let Some(s) = world.get_mut::<S>(e) {
            *s = state;
        }
    }

    fn release(&self, world: &mut World, e: Entity) {
        world.remove::<PredictionState<S, I>>(e);
    }
}

/// The input stream of input type `I`.
pub(crate) struct Inputs<I: Replicate> {
    name: String,
    specs: Vec<FieldSpec>,
    pub(crate) steps: Vec<Box<dyn PredictedKind<I>>>,
    // Client.
    pub(crate) pending: Option<I>,
    next_seq: u16,
    unacked: VecDeque<(u16, I)>,
    // Server.
    queues: BTreeMap<PeerId, InputQueue<I>>,
    pub(crate) current: BTreeMap<PeerId, I>,
    /// The host's own input for the next tick.
    pub(crate) local: Option<I>,
    /// "Nothing pressed": what a tick without an input sends, and what
    /// received inputs are read into (`I::default()` for Rust types; a
    /// zeroed value of the declared layout for inputs declared from C).
    pub(crate) proto: I,
    scratch: Vec<u32>,
    step_scratch: Vec<u32>,
}

impl<I: Replicate> Inputs<I> {
    pub(crate) fn new() -> Self {
        Self::with_proto(
            crate::reflect::short_name(std::any::type_name::<I>()).to_string(),
            I::default(),
        )
    }

    /// An input stream named `name` whose empty input is `proto`.
    pub(crate) fn with_proto(name: String, proto: I) -> Self {
        Self {
            name,
            specs: schema_of_value(&proto),
            steps: Vec::new(),
            pending: None,
            next_seq: 0,
            unacked: VecDeque::new(),
            queues: BTreeMap::new(),
            current: BTreeMap::new(),
            local: None,
            proto,
            scratch: Vec::new(),
            step_scratch: Vec::new(),
        }
    }

    fn apply(&mut self, world: &mut World, owned: &[(PeerId, Entity)], peer: PeerId, input: &I) {
        for &(owner, e) in owned {
            if owner == peer {
                for step in &self.steps {
                    step.server_step(world, e, input, &mut self.step_scratch);
                }
            }
        }
    }
}

impl<I: Replicate> InputStream for Inputs<I> {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn specs(&self) -> &[FieldSpec] {
        &self.specs
    }
    fn predicted(&self) -> u64 {
        self.steps.iter().fold(0, |m, s| m | 1 << s.kind())
    }

    fn receive(&mut self, peer: PeerId, first: u16, count: usize, r: &mut BitReader<'_>) -> Option<()> {
        if count > INPUT_REDUNDANCY {
            return None;
        }
        self.scratch.resize(self.specs.len(), 0);
        let queue = self.queues.entry(peer).or_default();
        for i in 0..count {
            for (word, spec) in self.scratch.iter_mut().zip(&self.specs) {
                *word = r.read(u32::from(spec.bits))?;
            }
            let mut input = self.proto.clone();
            read_words(&mut input, &self.scratch);
            let _: InputPush = queue.push(first.wrapping_add(i as u16), input);
        }
        Some(())
    }

    fn ack(&self, peer: PeerId) -> Option<u16> {
        self.queues.get(&peer)?.ack()
    }

    fn server_tick(&mut self, world: &mut World, owned: &[(PeerId, Entity)]) {
        // Remote clients: each input once, in order.
        let peers: Vec<PeerId> = self.queues.keys().copied().collect();
        for peer in peers {
            for _ in 0..MAX_INPUTS_PER_TICK {
                let Some((_, input)) = self.queues.get_mut(&peer).and_then(InputQueue::pop) else {
                    break;
                };
                self.apply(world, owned, peer, &input);
                self.current.insert(peer, input);
            }
        }
        // The host's own player: one input every tick ("nothing pressed"
        // when the game set none), as an offline game would.
        let mut input = self.local.take().unwrap_or_else(|| self.proto.clone());
        self.scratch.resize(self.specs.len(), 0);
        quantize_in_place(&mut input, &mut self.scratch);
        self.apply(world, owned, PeerId::HOST, &input);
        self.current.insert(PeerId::HOST, input);
    }

    fn forget(&mut self, peer: PeerId) {
        self.queues.remove(&peer);
        self.current.remove(&peer);
    }

    fn client_tick(&mut self, world: &mut World, mine: &[Entity], w: &mut BitWriter) {
        // Predict with the input as the server will get it.
        let mut input = self.pending.take().unwrap_or_else(|| self.proto.clone());
        self.scratch.resize(self.specs.len(), 0);
        quantize_in_place(&mut input, &mut self.scratch);
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        for &e in mine {
            for step in &self.steps {
                step.client_predict(world, e, &input, seq, &mut self.step_scratch);
            }
        }
        if self.unacked.len() == UNACKED {
            self.unacked.pop_front();
        }
        self.unacked.push_back((seq, input));
        let count = self.unacked.len().min(INPUT_REDUNDANCY);
        w.write_varint(count as u64);
        if count == 0 {
            return;
        }
        let from = self.unacked.len() - count;
        w.write(u32::from(self.unacked[from].0), 16);
        self.scratch.resize(self.specs.len(), 0);
        for (_, input) in self.unacked.iter().skip(from) {
            write_words(input, &mut self.scratch);
            for (word, spec) in self.scratch.iter().zip(&self.specs) {
                w.write(*word, u32::from(spec.bits));
            }
        }
    }

    fn client_acked(&mut self, ack: u16) {
        while let Some(&(seq, _)) = self.unacked.front() {
            if crate::net::seq_diff(seq, ack) > 0 {
                break;
            }
            self.unacked.pop_front();
        }
    }

    fn client_state(
        &mut self,
        world: &mut World,
        e: Entity,
        kind: usize,
        words: &[u32],
        ack: Option<u16>,
        epoch: u16,
        stats: &mut ClientStats,
    ) {
        for step in &self.steps {
            if step.kind() == kind {
                step.client_state(
                    world,
                    e,
                    words,
                    ack,
                    epoch,
                    Unapplied {
                        inputs: &self.unacked,
                        next_seq: self.next_seq,
                    },
                    &mut self.step_scratch,
                    stats,
                );
            }
        }
    }

    fn client_release(&mut self, world: &mut World, e: Entity) {
        for step in &self.steps {
            step.release(world, e);
        }
    }

    fn reset(&mut self) {
        self.pending = None;
        self.next_seq = 0;
        self.unacked.clear();
        self.queues.clear();
        self.current.clear();
        self.local = None;
    }
}
