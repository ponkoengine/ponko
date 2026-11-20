//! The server's side: which entities replicate (net ids), what each client
//! has acknowledged (baselines), what goes into each client's packets this
//! tick (interest, priority, budget), and what the clients send back
//! (acknowledgements, inputs).

use std::collections::BTreeMap;

use glam::Vec3;
use ponko_ecs::{ChunkQuery, ComponentId, Entity, Tick, World};

use super::bits::{BitReader, BitWriter};
use super::inputs::InputStream;
use super::kinds::Kind;
use super::wire::{self, ACK_WINDOW, HISTORY, MAX_PACKET, Meta, View};
use super::{PeerStats, Replicated};
use crate::net::{Interest, InterestEntity, InterestViewer, NetMessages, PeerId};

const NONE: u8 = 0;
const CREATING: u8 = 1;
const LIVE: u8 = 2;
const DESTROYING: u8 = 3;

/// Destroy records in one packet at most; the rest go in the next ones.
const MAX_DESTROYS: usize = 64;

/// Every live entity is read again at least once in this many ticks, changed
/// or not: a write the world can't see (through a raw pointer, without
/// `mark_changed`) is still sent, about a second later at 60 Hz.
const SWEEP_TICKS: usize = 64;

/// A replicated entity, by net id.
#[derive(Clone)]
pub(crate) struct Slot {
    pub(crate) entity: Entity,
    /// Bumped each time the id is given to another entity.
    pub(crate) generation: u32,
    /// The entity exists and is still marked [`Replicated`].
    pub(crate) alive: bool,
    seen: u64,
    pub(crate) present: u64,
    pub(crate) meta: Meta,
    priority: f32,
    global: bool,
    pub(crate) position: Option<Vec3>,
    /// Clients whose view of this id isn't `NONE`: the id is reused only
    /// once this is 0 and the entity is gone.
    known_by: u32,
    /// The tick (`sync_count`) it was last listed in `changed`.
    listed: u64,
}

/// What one client has of one entity.
#[derive(Clone, Copy, Default)]
struct ViewState {
    state: u8,
    generation: u32,
    /// The newest record of it the client acknowledged: the baseline.
    base_seq: u16,
    base_present: u64,
    base_meta: Meta,
    /// The baseline's place among the records sent (see `sent`).
    base_index: u32,
    /// Records of it sent so far (CREATE, FULL or DELTA).
    sent: u32,
    /// Priority accumulated while waiting to be sent.
    acc: f32,
    /// The entity changed, or the baseline did, since its words were last
    /// compared with the baseline: compare them this tick.
    dirty: bool,
    /// Ring of interest this tick, plus 1 (0: out of interest).
    scope: u8,
    /// Due this tick by its ring's rate.
    due: bool,
}

#[derive(Clone, Copy)]
struct Entry {
    slot: u32,
    generation: u32,
    index: u32,
    present: u64,
    meta: Meta,
    at: u32,
}

#[derive(Default)]
struct PacketRecord {
    seq: u16,
    live: bool,
    acked: bool,
    entries: Vec<Entry>,
    words: Vec<u32>,
    destroys: Vec<(u32, u32)>,
}

/// One client of the server.
pub(crate) struct Conn {
    pub(crate) ready: bool,
    pub(crate) rejected: bool,
    next_seq: u16,
    views: Vec<ViewState>,
    base_words: Vec<u32>,
    records: Vec<PacketRecord>,
    tokens: f64,
    pub(crate) stats: PeerStats,
    pub(crate) viewer: Option<Vec3>,
}

impl Conn {
    fn new() -> Self {
        Self {
            ready: false,
            rejected: false,
            next_seq: 0,
            views: Vec::new(),
            base_words: Vec::new(),
            records: (0..ACK_WINDOW).map(|_| PacketRecord::default()).collect(),
            tokens: 0.0,
            stats: PeerStats::default(),
            viewer: None,
        }
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    slot: u32,
    forced: bool,
    acc: f32,
}

/// Settings the server sends by.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SendSettings {
    /// Bytes per second per client.
    pub(crate) budget: u32,
    pub(crate) hz: f64,
    pub(crate) tick: u64,
}

/// The server reads only what changed: the replicated components' changes
/// are tracked, and this is where it stopped looking.
struct Tracking {
    /// [`World::id`]: another world (the game replaced it) is read in full.
    world: u64,
    since: Tick,
    /// Each kind's component.
    ids: Vec<ComponentId>,
    replicated: ComponentId,
}

#[derive(Default)]
pub(crate) struct Server {
    pub(crate) slots: Vec<Slot>,
    free: Vec<u32>,
    pub(crate) words: Vec<u32>,
    /// Ids whose words, components or owner changed this tick.
    changed: Vec<u32>,
    /// Ids given out or handed over since the last extraction: read in full.
    fresh: Vec<u32>,
    tracking: Option<Tracking>,
    /// The next id the rolling re-read (`SWEEP_TICKS`) looks at.
    sweep: usize,
    pub(crate) clients: BTreeMap<PeerId, Conn>,
    pub(crate) interest: Option<Interest>,
    sync_count: u64,
    // Scratch, reused every tick.
    found: Vec<Entity>,
    candidates: Vec<Candidate>,
    viewers: Vec<InterestViewer>,
    entities: Vec<InterestEntity>,
    writer: BitWriter,
    owned: Vec<(PeerId, Entity)>,
    touched: Vec<Entity>,
    read: Vec<u32>,
}

impl Server {
    /// Brings the ids up to date with the world: new replicated entities
    /// get ids, gone ones are marked dead, owners and priorities are read.
    pub(crate) fn refresh(&mut self, world: &mut World) {
        self.sync_count += 1;
        let stamp = self.sync_count;
        self.found.clear();
        {
            let slots = &mut self.slots;
            let found = &mut self.found;
            let fresh = &mut self.fresh;
            world.query::<(Entity, &Replicated)>().for_each(|(e, rep)| {
                match rep.id.and_then(|id| slots.get_mut(id as usize).map(|slot| (id, slot))) {
                    Some((id, slot)) if slot.alive && slot.entity == e => {
                        slot.seen = stamp;
                        if slot.meta.owner != rep.owner {
                            // A handoff: a new authority epoch (as `Authority::handoff`).
                            slot.meta.owner = rep.owner;
                            slot.meta.epoch = slot.meta.epoch.wrapping_add(1);
                            fresh.push(id);
                        }
                        slot.priority = rep.priority;
                        slot.global = rep.global;
                    }
                    _ => found.push(e),
                }
            });
        }
        for i in 0..self.found.len() {
            let e = self.found[i];
            let id = match self.free.pop() {
                Some(id) => id,
                None => {
                    self.slots.push(Slot {
                        entity: e,
                        generation: 0,
                        alive: false,
                        seen: 0,
                        present: 0,
                        meta: Meta::default(),
                        priority: 1.0,
                        global: false,
                        position: None,
                        known_by: 0,
                        listed: 0,
                    });
                    (self.slots.len() - 1) as u32
                }
            };
            let Some(rep) = world.get_mut::<Replicated>(e) else {
                continue;
            };
            rep.id = Some(id);
            let (owner, priority, global) = (rep.owner, rep.priority, rep.global);
            let slot = &mut self.slots[id as usize];
            slot.entity = e;
            slot.generation = slot.generation.wrapping_add(1);
            slot.alive = true;
            slot.seen = stamp;
            slot.meta = Meta { owner, epoch: 0 };
            slot.priority = priority;
            slot.global = global;
            self.fresh.push(id);
        }
        for (id, slot) in self.slots.iter_mut().enumerate() {
            if slot.alive && slot.seen != stamp {
                slot.alive = false;
                slot.position = None;
                if slot.known_by == 0 {
                    self.free.push(id as u32);
                }
            }
        }
    }

    /// Brings the live entities' words and kinds up to date with the world
    /// (after [`refresh`](Self::refresh) and the inputs), and lists the
    /// ids that changed. Only entities whose replicated components changed
    /// are read (the world's change tracking says which), plus the new
    /// ones and a few more each tick (`SWEEP_TICKS`); the first time, or
    /// in a world it hasn't seen, everything is. With interest, positions
    /// are read for every live entity.
    pub(crate) fn extract(
        &mut self,
        world: &mut World,
        kinds: &[Kind],
        stride: usize,
        position: &dyn Fn(&World, Entity) -> Option<Vec3>,
    ) {
        self.words.resize(self.slots.len() * stride, 0);
        self.changed.clear();
        let stamp = self.sync_count;
        let n = self.slots.len();
        match &mut self.tracking {
            Some(t) if t.world == world.id() => {
                self.touched.clear();
                for &id in &t.ids {
                    // Only replicated entities: the archetypes with both.
                    let ids = [id, t.replicated];
                    let query = ChunkQuery::new(&ids).writes(0).changed(1).since(t.since);
                    let touched = &mut self.touched;
                    world
                        .raw()
                        .query_chunks(query, |chunk| touched.extend_from_slice(chunk.entities));
                    // A component taken off an entity that stays.
                    self.touched
                        .extend_from_slice(world.raw_ref().removed_dynamic(id, t.since));
                }
                t.since = world.advance_change_tick();
                for i in 0..self.touched.len() {
                    let e = self.touched[i];
                    let Some(id) = world.get::<Replicated>(e).and_then(|r| r.id) else {
                        continue;
                    };
                    if self.slots.get(id as usize).is_some_and(|s| s.alive && s.entity == e) {
                        self.read_slot(world, kinds, stride, id, false, stamp);
                    }
                }
                for _ in 0..n.div_ceil(SWEEP_TICKS).min(n) {
                    let id = (self.sweep % n) as u32;
                    self.sweep = self.sweep.wrapping_add(1);
                    self.read_slot(world, kinds, stride, id, false, stamp);
                }
            }
            _ => {
                let ids = kinds.iter().map(|k| k.access.track(world)).collect();
                let replicated = world.register::<Replicated>();
                self.tracking = Some(Tracking {
                    world: world.id(),
                    since: world.advance_change_tick(),
                    ids,
                    replicated,
                });
                for id in 0..n as u32 {
                    self.read_slot(world, kinds, stride, id, false, stamp);
                }
            }
        }
        for i in 0..self.fresh.len() {
            let id = self.fresh[i];
            self.read_slot(world, kinds, stride, id, true, stamp);
        }
        self.fresh.clear();
        if self.interest.is_some() {
            for slot in self.slots.iter_mut().filter(|s| s.alive) {
                slot.position = position(world, slot.entity);
            }
        }
    }

    /// Reads one live entity's words and kinds; lists it in `changed` if
    /// they differ from what it had (or `force`).
    fn read_slot(&mut self, world: &World, kinds: &[Kind], stride: usize, id: u32, force: bool, stamp: u64) {
        let Some(slot) = self.slots.get_mut(id as usize).filter(|s| s.alive) else {
            return;
        };
        self.read.resize(stride, 0);
        let mut present = 0u64;
        for (k, kind) in kinds.iter().enumerate() {
            let out = &mut self.read[kind.offset..kind.offset + kind.words()];
            if kind.access.read(world, slot.entity, out) {
                present |= 1 << k;
            } else {
                out.fill(0);
            }
        }
        let words = &mut self.words[id as usize * stride..(id as usize + 1) * stride];
        if force || present != slot.present || *words != self.read[..] {
            words.copy_from_slice(&self.read);
            slot.present = present;
            if slot.listed != stamp {
                slot.listed = stamp;
                self.changed.push(id);
            }
        }
    }

    /// No client is ready, so nothing is extracted: forget where reading
    /// stopped (the first client's tick reads everything) and what was
    /// waiting to be read in full, so nothing piles up meanwhile.
    pub(crate) fn idle(&mut self) {
        self.fresh.clear();
        self.tracking = None;
    }

    /// Ids waiting to be read in full.
    #[cfg(test)]
    pub(crate) fn pending(&self) -> usize {
        self.fresh.len()
    }

    /// The ids listed as changed by the last extraction.
    #[cfg(test)]
    pub(crate) fn changed(&self) -> &[u32] {
        &self.changed
    }

    /// The tick up to which the world's changes have been read, for
    /// keeping the logs of removed components until then.
    pub(crate) fn changes_seen(&self, world: &World) -> Option<Tick> {
        self.tracking
            .as_ref()
            .filter(|t| t.world == world.id())
            .map(|t| t.since)
    }

    /// The live entity with net id `id`.
    pub(crate) fn entity(&self, id: u32) -> Option<Entity> {
        self.slots.get(id as usize).filter(|s| s.alive).map(|s| s.entity)
    }

    /// Entities owned by each client (and the host), for applying inputs.
    pub(crate) fn owned(&mut self) -> &[(PeerId, Entity)] {
        self.owned.clear();
        self.owned.extend(
            self.slots
                .iter()
                .filter(|s| s.alive)
                .filter_map(|s| s.meta.owner.map(|o| (o, s.entity))),
        );
        &self.owned
    }

    /// A client that connected.
    pub(crate) fn add_client(&mut self, peer: PeerId) {
        self.clients.entry(peer).or_insert_with(Conn::new);
    }

    /// A client that left: what it knew is forgotten, and ids only it still
    /// knew can be reused.
    pub(crate) fn remove_client(&mut self, peer: PeerId) {
        let Some(conn) = self.clients.remove(&peer) else { return };
        for (id, view) in conn.views.iter().enumerate() {
            if view.state == NONE {
                continue;
            }
            let slot = &mut self.slots[id];
            if view.generation == slot.generation {
                slot.known_by = slot.known_by.saturating_sub(1);
                if !slot.alive && slot.known_by == 0 {
                    self.free.push(id as u32);
                }
            }
        }
        if let Some(interest) = &mut self.interest {
            interest.remove_peer(peer);
        }
    }

    /// A `CLIENT` packet: acknowledgements, then inputs.
    pub(crate) fn receive(
        &mut self,
        peer: PeerId,
        bytes: &[u8],
        stride: usize,
        inputs: Option<&mut (dyn InputStream + 'static)>,
    ) -> Option<()> {
        let mut r = BitReader::new(bytes);
        if r.read(wire::KIND_BITS)? != wire::CLIENT {
            return None;
        }
        let conn = self.clients.get_mut(&peer).filter(|c| c.ready)?;
        if r.read_bool()? {
            let latest = r.read(16)? as u16;
            let mask = u64::from(r.read(32)?) | u64::from(r.read(32)?) << 32;
            for back in 0..=ACK_WINDOW as u16 {
                if back > 0 && mask & (1 << (back - 1)) == 0 {
                    continue;
                }
                let seq = latest.wrapping_sub(back);
                acknowledge(conn, &mut self.slots, &mut self.free, seq, stride);
            }
        }
        let count = r.read_varint()? as usize;
        if count > 0 {
            let first = r.read(16)? as u16;
            inputs?.receive(peer, first, count, &mut r)?;
        }
        Some(())
    }

    /// Sends every ready client its packets for this tick.
    pub(crate) fn send(
        &mut self,
        endpoint: &mut NetMessages,
        kinds: &[Kind],
        stride: usize,
        settings: SendSettings,
        inputs: Option<&(dyn InputStream + 'static)>,
    ) {
        self.update_interest(settings.tick);
        let peers: Vec<PeerId> = self.clients.keys().copied().collect();
        for peer in peers {
            let input_ack = inputs.and_then(|i| i.ack(peer));
            self.send_to(peer, endpoint, kinds, stride, settings, input_ack);
        }
    }

    fn update_interest(&mut self, tick: u64) {
        let Some(interest) = &mut self.interest else { return };
        self.viewers.clear();
        self.entities.clear();
        for (peer, conn) in &self.clients {
            if !conn.ready {
                continue;
            }
            if let Some(position) = conn.viewer {
                self.viewers.push(InterestViewer { peer: *peer, position });
            }
        }
        for (id, slot) in self.slots.iter().enumerate() {
            if !slot.alive {
                continue;
            }
            let Some(position) = slot.position else { continue };
            if let Some(owner) = slot.meta.owner
                && self.clients.get(&owner).is_some_and(|c| c.ready && c.viewer.is_none())
            {
                self.viewers.push(InterestViewer { peer: owner, position });
            }
            if !slot.global {
                self.entities.push(InterestEntity {
                    id: id as u64,
                    position,
                });
            }
        }
        interest.update(tick, &self.viewers, &self.entities);
    }

    fn send_to(
        &mut self,
        peer: PeerId,
        endpoint: &mut NetMessages,
        kinds: &[Kind],
        stride: usize,
        settings: SendSettings,
        input_ack: Option<u16>,
    ) {
        let Self {
            slots,
            words,
            changed,
            clients,
            interest,
            candidates,
            writer,
            ..
        } = self;
        let Some(conn) = clients.get_mut(&peer) else { return };
        if !conn.ready {
            return;
        }
        if conn.views.len() < slots.len() {
            conn.views.resize(slots.len(), ViewState::default());
            conn.base_words.resize(slots.len() * stride, 0);
        }
        for &id in changed.iter() {
            conn.views[id as usize].dirty = true;
        }

        // Interest: which ring each entity is in for this client, this tick.
        let tiers = interest.as_ref().map(|i| {
            i.config()
                .tiers
                .iter()
                .map(|t| 1.0 / t.every.max(1) as f32)
                .collect::<Vec<_>>()
        });
        for view in &mut conn.views {
            view.scope = 0;
            view.due = false;
        }
        match interest {
            Some(interest) => {
                for e in interest.entries(peer) {
                    let view = &mut conn.views[e.entity as usize];
                    view.scope = e.tier + 1;
                    view.due = e.send;
                }
                for (id, slot) in slots.iter().enumerate() {
                    if slot.alive && (slot.global || slot.position.is_none() || slot.meta.owner == Some(peer)) {
                        conn.views[id].scope = 1;
                        conn.views[id].due = true;
                    }
                }
            }
            None => {
                for (id, slot) in slots.iter().enumerate() {
                    if slot.alive {
                        conn.views[id].scope = 1;
                        conn.views[id].due = true;
                    }
                }
            }
        }

        // Start, keep and stop telling the client about entities.
        candidates.clear();
        let mut destroying = 0usize;
        for (id, slot) in slots.iter_mut().enumerate() {
            let view = &mut conn.views[id];
            let wanted = slot.alive && view.scope > 0;
            if view.state != NONE && view.generation != slot.generation {
                // Defensive: an id reused under a view that wasn't cleared.
                view.state = DESTROYING;
            }
            match (view.state, wanted) {
                (NONE, true) => {
                    *view = ViewState {
                        state: CREATING,
                        generation: slot.generation,
                        scope: view.scope,
                        due: view.due,
                        ..ViewState::default()
                    };
                    slot.known_by += 1;
                }
                (CREATING | LIVE, false) => view.state = DESTROYING,
                _ => {}
            }
            match view.state {
                CREATING => {
                    view.acc += slot.priority.max(0.0) * 2.0 + 1.0;
                    candidates.push(Candidate {
                        slot: id as u32,
                        forced: slot.meta.owner == Some(peer),
                        acc: view.acc,
                    });
                }
                LIVE => {
                    // Only entities that changed (or whose baseline did) are
                    // compared; the rest are known to match their baseline.
                    let changed = view.dirty && {
                        let base = &conn.base_words[id * stride..(id + 1) * stride];
                        let now = &words[id * stride..(id + 1) * stride];
                        view.base_present != slot.present
                            || view.base_meta != slot.meta
                            || changed_words(kinds, slot.present, base, now)
                    };
                    view.dirty = changed;
                    let owned = slot.meta.owner == Some(peer);
                    if changed || owned {
                        let weight = tiers
                            .as_ref()
                            .and_then(|t| t.get(usize::from(view.scope.max(1) - 1)).copied())
                            .unwrap_or(1.0);
                        view.acc += slot.priority.max(0.0) * weight;
                        if owned || view.due {
                            candidates.push(Candidate {
                                slot: id as u32,
                                forced: owned,
                                acc: view.acc,
                            });
                        }
                    }
                }
                DESTROYING => destroying += 1,
                _ => {}
            }
        }
        candidates.sort_by(|a, b| b.forced.cmp(&a.forced).then(b.acc.total_cmp(&a.acc)));

        // The budget: a token bucket refilled every tick, allowed to save
        // up a few ticks' worth for bursts (spawning many at once).
        let per_tick = f64::from(settings.budget) / settings.hz.max(1.0);
        conn.tokens = (conn.tokens + per_tick).min(per_tick * 4.0 + MAX_PACKET as f64);

        let mut next = 0usize;
        loop {
            let seq = conn.next_seq;
            conn.next_seq = seq.wrapping_add(1);
            let record = &mut conn.records[usize::from(seq) % ACK_WINDOW];
            if record.live && !record.acked {
                conn.stats.packets_lost += 1;
            }
            record.seq = seq;
            record.live = true;
            record.acked = false;
            record.entries.clear();
            record.words.clear();
            record.destroys.clear();

            writer.clear();
            writer.write(wire::STATE, wire::KIND_BITS);
            writer.write(u32::from(seq), 16);
            writer.write_varint(settings.tick);
            writer.write_bool(input_ack.is_some());
            if let Some(ack) = input_ack {
                writer.write(u32::from(ack), 16);
            }
            // Destroys ride in every packet until one carrying them is acked.
            let n_destroy = destroying.min(MAX_DESTROYS);
            writer.write_varint(n_destroy as u64);
            for (id, view) in conn.views.iter().enumerate() {
                if record.destroys.len() == n_destroy {
                    break;
                }
                if view.state == DESTROYING {
                    writer.write_varint(id as u64);
                    record.destroys.push((id as u32, view.generation));
                }
            }
            conn.stats.destroys_sent += n_destroy as u64;
            // Each record is preceded by a 1 bit; a 0 bit ends the packet.
            let mut records = 0u64;
            while next < candidates.len() {
                let c = candidates[next];
                let id = c.slot as usize;
                let slot = &slots[id];
                let view = &mut conn.views[id];
                let now = &words[id * stride..(id + 1) * stride];
                let current = View {
                    present: slot.present,
                    meta: slot.meta,
                    words: now,
                };
                let create = view.state == CREATING;
                // Name the baseline only while the client surely still has
                // it: among the newest HISTORY records it was sent.
                let base =
                    (view.state == LIVE && view.sent.wrapping_sub(view.base_index) < HISTORY as u32).then(|| {
                        (
                            seq.wrapping_sub(view.base_seq),
                            View {
                                present: view.base_present,
                                meta: view.base_meta,
                                words: &conn.base_words[id * stride..(id + 1) * stride],
                            },
                        )
                    });
                let mark = writer.bit_len();
                writer.write_bool(true);
                wire::write_record(writer, kinds, c.slot, create, current, base);
                let bytes = writer.byte_len() + 1;
                let over_packet = bytes > MAX_PACKET && records > 0;
                let over_budget = !c.forced && bytes as f64 > conn.tokens;
                if over_packet || over_budget {
                    writer.truncate(mark);
                    break;
                }
                view.sent = view.sent.wrapping_add(1);
                view.acc = 0.0;
                record.entries.push(Entry {
                    slot: c.slot,
                    generation: view.generation,
                    index: view.sent,
                    present: slot.present,
                    meta: slot.meta,
                    at: record.words.len() as u32,
                });
                record.words.extend_from_slice(now);
                records += 1;
                match (create, base.is_some()) {
                    (true, _) => conn.stats.creates_sent += 1,
                    (false, true) => conn.stats.deltas_sent += 1,
                    (false, false) => conn.stats.fulls_sent += 1,
                }
                next += 1;
            }
            writer.write_bool(false);
            let bytes = writer.finish();
            let sent = bytes.len();
            if endpoint.send_internal(peer, false, bytes).is_err() {
                record_unsent(&mut conn.records[usize::from(seq) % ACK_WINDOW]);
                break;
            }
            conn.stats.packets_sent += 1;
            conn.stats.bytes_sent += sent as u64;
            conn.stats.records_sent += records;
            conn.tokens -= sent as f64;
            let more = next < candidates.len();
            if !more || records == 0 || (conn.tokens <= 0.0 && !candidates[next].forced) {
                break;
            }
        }
        conn.stats.deferred += (candidates.len() - next) as u64;
    }
}

/// A packet the transport refused: nothing in it can be acknowledged.
fn record_unsent(record: &mut PacketRecord) {
    record.live = false;
}

/// Whether any present kind's words differ.
fn changed_words(kinds: &[Kind], present: u64, base: &[u32], now: &[u32]) -> bool {
    kinds.iter().enumerate().any(|(i, k)| {
        present & (1 << i) != 0 && base[k.offset..k.offset + k.words()] != now[k.offset..k.offset + k.words()]
    })
}

/// The client has packet `seq`: what it carried becomes the baselines.
fn acknowledge(conn: &mut Conn, slots: &mut [Slot], free: &mut Vec<u32>, seq: u16, stride: usize) {
    let ring = usize::from(seq) % ACK_WINDOW;
    let Conn {
        records,
        views,
        base_words,
        stats,
        ..
    } = conn;
    let record = &mut records[ring];
    if !record.live || record.acked || record.seq != seq {
        return;
    }
    record.acked = true;
    stats.packets_acked += 1;

    for entry in &record.entries {
        let Some(view) = views.get_mut(entry.slot as usize) else {
            continue;
        };
        if view.generation != entry.generation || !(view.state == CREATING || view.state == LIVE) {
            continue;
        }
        if view.state == LIVE && entry.index.wrapping_sub(view.base_index) as i32 <= 0 {
            continue; // an older record than the baseline
        }
        view.state = LIVE;
        view.dirty = true; // a new baseline: compare it with the entity
        view.base_seq = seq;
        view.base_present = entry.present;
        view.base_meta = entry.meta;
        view.base_index = entry.index;
        let at = entry.at as usize;
        let id = entry.slot as usize;
        base_words[id * stride..(id + 1) * stride].copy_from_slice(&record.words[at..at + stride]);
    }
    for &(id, generation) in &record.destroys {
        let Some(view) = views.get_mut(id as usize) else {
            continue;
        };
        if view.state != DESTROYING || view.generation != generation {
            continue;
        }
        view.state = NONE;
        let slot = &mut slots[id as usize];
        if slot.generation == generation {
            slot.known_by = slot.known_by.saturating_sub(1);
            if !slot.alive && slot.known_by == 0 {
                free.push(id);
            }
        }
    }
}
