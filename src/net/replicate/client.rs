//! The client's side: decoding the server's state packets into this world
//! (spawning, updating and despawning [`Replica`] entities), keeping the
//! records deltas refer to, answering with acknowledgements and inputs,
//! and showing interpolated kinds a little in the past.

use ponko_ecs::{Entity, World};

use super::bits::{BitReader, BitWriter};
use super::inputs::InputStream;
use super::kinds::Kind;
use super::wire::{self, HISTORY, MAX_IDS, Meta, View};
use super::{ClientStats, NetId, Replica};
use crate::net::{ClockSync, NetMessages, PeerId, seq_diff, seq_newer};

/// The most destroys one packet may carry (the server sends at most 64).
const MAX_DESTROYS: u64 = 1024;

/// One id as this client knows it.
#[derive(Clone, Copy, Default)]
struct Slot {
    entity: Option<Entity>,
    present: u64,
    meta: Meta,
    mine: bool,
    /// The packet ([widened](Client::widen)) of the newest record or
    /// destroy applied to this id: anything older about it is out of
    /// date. Packets of one tick carry different entities and may arrive
    /// in any order, so this is per id.
    seq: Option<u64>,
}

/// One received record of an id, kept as a possible baseline.
#[derive(Clone, Copy, Default)]
struct Hist {
    /// Its packet, [widened](Client::widen).
    seq: u64,
    valid: bool,
    present: u64,
    meta: Meta,
}

/// A record decoded from the packet being read, applied once the whole
/// packet decoded.
struct Pending {
    id: u32,
    present: u64,
    meta: Meta,
    /// Offset of its words in `Client::decoded_words`.
    at: usize,
}

#[derive(Default)]
pub(crate) struct Client {
    /// This client's peer id, from the server's hello.
    pub(crate) me: Option<PeerId>,
    /// The server's tick rate, from its hello.
    pub(crate) hz: f64,
    /// The server accepted this client's declarations.
    pub(crate) joined: bool,
    /// Why the server (or this client) refused the session.
    pub(crate) error: Option<String>,
    slots: Vec<Slot>,
    hist: Vec<[Hist; HISTORY]>,
    hist_words: Vec<u32>,
    /// The newest state packet applied ([unwrapped](Self::unwrap)), and a
    /// mask of the 64 before it.
    latest: Option<u64>,
    mask: u64,
    /// The newest input the server said it applied.
    pub(crate) input_ack: Option<u16>,
    pub(crate) clock: ClockSync,
    /// The newest server tick received.
    pub(crate) server_tick: u64,
    pub(crate) stats: ClientStats,
    // Scratch, reused every packet.
    pending: Vec<Pending>,
    decoded_words: Vec<u32>,
    base_words: Vec<u32>,
    destroys: Vec<u32>,
    mine: Vec<Entity>,
    writer: BitWriter,
}

impl Client {
    /// Despawns every replica and forgets the session.
    pub(crate) fn reset(&mut self, world: &mut World, inputs: Option<&mut (dyn InputStream + 'static)>) {
        let mut inputs = inputs;
        for slot in &self.slots {
            if let Some(e) = slot.entity {
                if slot.mine
                    && let Some(i) = inputs.as_deref_mut()
                {
                    i.client_release(world, e);
                }
                world.despawn(e);
            }
        }
        let stats = self.stats;
        *self = Self::default();
        self.stats = stats;
        if let Some(i) = inputs {
            i.reset();
        }
    }

    /// The entity replicating net id `id` here, if there is one.
    pub(crate) fn entity(&self, id: NetId) -> Option<Entity> {
        self.slots.get(id.0 as usize).and_then(|s| s.entity)
    }

    fn grow(&mut self, id: usize, stride: usize) {
        if id >= self.slots.len() {
            let n = id + 1;
            self.slots.resize(n, Slot::default());
            self.hist.resize(n, [Hist::default(); HISTORY]);
            self.hist_words.resize(n * HISTORY * stride, 0);
        }
    }

    /// A packet's 16-bit sequence as a 64-bit one that never wraps: the
    /// one nearest the newest packet received. Packets of the same number
    /// 65536 apart then stay apart.
    fn widen(&self, seq: u16) -> u64 {
        match self.latest {
            None => (1 << 32) + u64::from(seq),
            Some(latest) => latest.wrapping_add_signed(i64::from(seq_diff(seq, latest as u16))),
        }
    }

    /// Where the record of `id` from packet `seq` is in its history ring,
    /// if it is still kept.
    fn baseline(&self, id: usize, seq: u64) -> Option<usize> {
        let ring = self.hist.get(id)?;
        (0..HISTORY).find(|&i| ring[i].valid && ring[i].seq == seq)
    }

    /// Keeps the decoded record at `at` as a possible baseline of `id`, in
    /// place of its oldest one. The ring keeps the newest records, however
    /// late they arrive: the server names only recent ones.
    fn remember(&mut self, id: usize, seq: u64, present: u64, meta: Meta, at: usize, stride: usize) {
        let ring = &self.hist[id];
        if self.baseline(id, seq).is_some() {
            return; // a repeated packet
        }
        let i = match (0..HISTORY).find(|&i| !ring[i].valid) {
            Some(free) => free,
            None => {
                let oldest = (0..HISTORY).min_by_key(|&i| ring[i].seq).unwrap_or(0);
                if ring[oldest].seq > seq {
                    return; // older than everything kept
                }
                oldest
            }
        };
        self.hist[id][i] = Hist {
            seq,
            valid: true,
            present,
            meta,
        };
        let to = (id * HISTORY + i) * stride;
        self.hist_words[to..to + stride].copy_from_slice(&self.decoded_words[at..at + stride]);
    }

    /// Forgets the entity of `id`, destroyed in packet `seq`. Its history
    /// stays: a late packet may still name those records as baselines
    /// (records of a later entity with this id have newer sequences).
    fn forget(&mut self, id: usize, seq: u64) {
        self.slots[id] = Slot {
            seq: Some(seq),
            ..Slot::default()
        };
    }

    /// Whether packet `seq` is newer than everything applied to `id`.
    fn is_news(&self, id: usize, seq: u64) -> bool {
        self.slots[id].seq.is_none_or(|last| seq > last)
    }

    /// A `STATE` packet. Decodes all of it first: a packet that doesn't
    /// decode (a baseline that's gone, a short packet) changes nothing and
    /// isn't acknowledged, so the server sends its contents again. Of a
    /// late packet, only what is newer than this client's state of each
    /// entity is applied; all of it becomes possible baselines.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn receive_state(
        &mut self,
        bytes: &[u8],
        kinds: &[Kind],
        stride: usize,
        world: &mut World,
        inputs: Option<&mut (dyn InputStream + 'static)>,
        local_now: f64,
    ) {
        self.stats.packets_received += 1;
        self.stats.bytes_received += bytes.len() as u64;
        let mut head = BitReader::new(bytes);
        let seq = match (head.read(wire::KIND_BITS), head.read(16)) {
            (Some(wire::STATE), Some(seq)) => self.widen(seq as u16),
            _ => {
                self.stats.packets_bad += 1;
                return;
            }
        };
        let late = self.latest.is_some_and(|latest| seq <= latest);
        let Some((tick, input_ack)) = self.decode(bytes, kinds, stride, seq) else {
            // A late packet may name baselines this client has moved past:
            // nothing in it is news anyway.
            if late {
                self.stats.packets_stale += 1;
            } else {
                self.stats.packets_bad += 1;
            }
            return;
        };
        let hz = if self.hz > 0.0 { self.hz } else { 60.0 };
        let remote_time = tick as f64 / hz;
        match self.latest {
            Some(latest) if late => {
                // Late (or repeated): acknowledged, applied only where it
                // is newer than what this client has of an entity.
                self.stats.packets_stale += 1;
                let back = latest - seq;
                if (1..=64).contains(&back) {
                    self.mask |= 1u64 << (back - 1);
                }
            }
            latest => {
                self.clock.observe(remote_time, local_now);
                self.mask = match latest {
                    None => 0,
                    Some(latest) => {
                        let ahead = seq - latest;
                        if ahead > 64 {
                            0
                        } else {
                            (self.mask << (ahead - 1) << 1) | 1u64 << (ahead - 1)
                        }
                    }
                };
                self.latest = Some(seq);
            }
        }
        self.server_tick = self.server_tick.max(tick);
        let mut inputs = inputs;
        if let Some(ack) = input_ack
            && self.input_ack.is_none_or(|a| seq_newer(ack, a))
        {
            self.input_ack = Some(ack);
            if let Some(i) = inputs.as_deref_mut() {
                i.client_acked(ack);
            }
        }

        for k in 0..self.destroys.len() {
            let id = self.destroys[k] as usize;
            let Some(slot) = self.slots.get(id).copied() else {
                continue;
            };
            if !self.is_news(id, seq) {
                continue;
            }
            if let Some(e) = slot.entity {
                if slot.mine
                    && let Some(i) = inputs.as_deref_mut()
                {
                    i.client_release(world, e);
                }
                world.despawn(e);
                self.stats.despawned += 1;
            }
            self.forget(id, seq);
        }

        let predicted = inputs.as_deref().map_or(0, |i| i.predicted());
        let pending = std::mem::take(&mut self.pending);
        for p in &pending {
            let id = p.id as usize;
            if !self.is_news(id, seq) {
                continue;
            }
            let words = &self.decoded_words[p.at..p.at + stride];
            let slot = self.slots[id];
            let mine = self.me.is_some() && p.meta.owner == self.me;
            let replica = Replica {
                id: NetId(p.id),
                owner: p.meta.owner,
                epoch: p.meta.epoch,
                mine,
            };
            let e = match slot.entity.filter(|&e| world.is_alive(e)) {
                Some(e) => {
                    if let Some(r) = world.get_mut::<Replica>(e)
                        && *r != replica
                    {
                        *r = replica;
                    }
                    e
                }
                None => {
                    self.stats.spawned += 1;
                    world.spawn((replica,))
                }
            };
            if slot.mine
                && !mine
                && let Some(i) = inputs.as_deref_mut()
            {
                i.client_release(world, e);
            }
            for (k, kind) in kinds.iter().enumerate() {
                let bit = 1u64 << k;
                let value = &words[kind.offset..kind.offset + kind.words()];
                if p.present & bit == 0 {
                    if slot.present & bit != 0 {
                        kind.access.remove(world, e);
                    }
                    continue;
                }
                if mine {
                    kind.access.unbuffer(world, e);
                    if predicted & bit != 0
                        && let Some(i) = inputs.as_deref_mut()
                    {
                        i.client_state(world, e, k, value, input_ack, p.meta.epoch, &mut self.stats);
                        continue;
                    }
                } else if kind.access.buffer(world, e, remote_time, value) {
                    continue;
                }
                kind.access.write(world, e, value);
            }
            let s = &mut self.slots[id];
            s.entity = Some(e);
            s.present = p.present;
            s.meta = p.meta;
            s.mine = mine;
            s.seq = Some(seq);
            self.stats.records_applied += 1;
        }
        // The records become possible baselines.
        for p in &pending {
            self.remember(p.id as usize, seq, p.present, p.meta, p.at, stride);
        }
        self.pending = pending;
    }

    /// Decodes a whole state packet (number `seq`, unwrapped) into
    /// `pending`, `decoded_words` and `destroys`: (server tick, input ack).
    fn decode(&mut self, bytes: &[u8], kinds: &[Kind], stride: usize, seq: u64) -> Option<(u64, Option<u16>)> {
        let mut r = BitReader::new(bytes);
        if r.read(wire::KIND_BITS)? != wire::STATE {
            return None;
        }
        r.read(16)?;
        let tick = r.read_varint()?;
        let input_ack = if r.read_bool()? { Some(r.read(16)? as u16) } else { None };
        self.pending.clear();
        self.decoded_words.clear();
        self.destroys.clear();
        let destroys = r.read_varint()?;
        if destroys > MAX_DESTROYS {
            return None;
        }
        for _ in 0..destroys {
            let id = u32::try_from(r.read_varint()?).ok().filter(|&id| id < MAX_IDS)?;
            self.destroys.push(id);
        }
        while r.read_bool()? {
            let (id, record, distance) = wire::read_record_head(&mut r)?;
            if id >= MAX_IDS {
                return None;
            }
            self.grow(id as usize, stride);
            let at = self.decoded_words.len();
            self.decoded_words.resize(at + stride, 0);
            // The baseline is copied out first: the history ring and the
            // decoded words both live in `self`.
            let base = if record == wire::DELTA {
                let i = self.baseline(id as usize, seq.checked_sub(u64::from(distance))?)?;
                let h = self.hist[id as usize][i];
                let from = (id as usize * HISTORY + i) * stride;
                self.base_words.clear();
                self.base_words.extend_from_slice(&self.hist_words[from..from + stride]);
                Some((h.present, h.meta))
            } else {
                None
            };
            let base_view = base.map(|(present, meta)| View {
                present,
                meta,
                words: &self.base_words,
            });
            let out = &mut self.decoded_words[at..at + stride];
            let d = wire::read_record_body(&mut r, kinds, record, base_view, out)?;
            if self.pending.iter().any(|p| p.id == id) {
                return None; // one record per id per packet
            }
            self.pending.push(Pending {
                id,
                present: d.present,
                meta: d.meta,
                at,
            });
        }
        Some((tick, input_ack))
    }

    /// The `CLIENT` packet for this tick: acknowledgements, then inputs
    /// (predicting this tick's input on our own entities first).
    pub(crate) fn send(
        &mut self,
        endpoint: &mut NetMessages,
        world: &mut World,
        inputs: Option<&mut (dyn InputStream + 'static)>,
    ) {
        let w = &mut self.writer;
        w.clear();
        w.write(wire::CLIENT, wire::KIND_BITS);
        w.write_bool(self.latest.is_some());
        if let Some(latest) = self.latest {
            w.write(u32::from(latest as u16), 16);
            w.write(self.mask as u32, 32);
            w.write((self.mask >> 32) as u32, 32);
        }
        match inputs {
            Some(i) => {
                self.mine.clear();
                self.mine
                    .extend(self.slots.iter().filter(|s| s.mine).filter_map(|s| s.entity));
                i.client_tick(world, &self.mine, w);
            }
            None => w.write_varint(0),
        }
        let bytes = w.finish();
        if endpoint.send_internal(PeerId::HOST, false, bytes).is_ok() {
            self.stats.packets_sent += 1;
            self.stats.bytes_sent += bytes.len() as u64;
        }
    }

    /// Shows interpolated kinds as they were a little before `local_now`.
    pub(crate) fn sample(&self, world: &mut World, kinds: &[Kind], local_now: f64) {
        let Some(remote_now) = self.clock.to_remote(local_now) else {
            return;
        };
        for kind in kinds {
            kind.access.sample(world, remote_now);
        }
    }
}
