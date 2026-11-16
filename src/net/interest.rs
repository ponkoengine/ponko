//! Interest management: which entities each peer should be sent, and how
//! often, by distance from where that peer is looking. Pure data structures
//! over positions: it decides, you send.

use std::collections::HashMap;

use glam::Vec3;

use super::PeerId;

/// The most tiers in an [`InterestConfig`].
pub const MAX_INTEREST_TIERS: usize = 8;

/// One ring of interest: entities up to `radius` from a viewer (and beyond
/// the previous, smaller ring) are sent every `every` ticks.
/// ```
/// let near = ponko::net::InterestTier { radius: 40.0, every: 1 };
/// assert_eq!(near.every, 1);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterestTier {
    /// Distance from the viewer, in world units.
    pub radius: f32,
    /// Send once every this many ticks (1 = every tick).
    pub every: u32,
}

/// Rings of interest, from near to far, plus how sticky their edges are.
/// Entities beyond the last ring are not sent at all.
///
/// Defaults: everything within 40 units every tick, within 120 units every
/// 4th tick, a hysteresis of 5 units, sends spread over ticks.
/// ```
/// let c = ponko::net::InterestConfig::default();
/// assert_eq!(c.tiers.len(), 2);
/// assert_eq!(c.hysteresis, 5.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct InterestConfig {
    /// 1 to [`MAX_INTEREST_TIERS`] rings with increasing radii.
    pub tiers: Vec<InterestTier>,
    /// An entity moves to a closer ring only once it is this far inside it,
    /// and to a farther ring (or out of interest) only once it is this far
    /// outside, so one standing on a boundary doesn't flip back and forth.
    /// Less than the first radius.
    pub hysteresis: f32,
    /// Spread the sends of slow rings over different ticks (by entity id),
    /// instead of every entity of a ring sending on the same tick.
    pub stagger: bool,
}
impl Default for InterestConfig {
    fn default() -> Self {
        Self {
            tiers: vec![
                InterestTier { radius: 40.0, every: 1 },
                InterestTier {
                    radius: 120.0,
                    every: 4,
                },
            ],
            hysteresis: 5.0,
            stagger: true,
        }
    }
}

impl InterestConfig {
    /// Checks the settings (what [`Interest::new`] panics about): 1 to
    /// [`MAX_INTEREST_TIERS`] tiers with positive, increasing radii and an
    /// `every` of at least 1, and a hysteresis from 0 up to (not including)
    /// the first radius.
    ///
    /// # Errors
    ///
    /// A message naming the first setting that breaks one of those rules.
    ///
    /// ```
    /// use ponko::net::InterestConfig;
    /// assert!(ponko::net::InterestConfig::default().validate().is_ok());
    /// assert!(ponko::net::InterestConfig { tiers: vec![], ..Default::default() }.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(1..=MAX_INTEREST_TIERS).contains(&self.tiers.len()) {
            return Err("InterestConfig: needs 1 to 8 tiers");
        }
        let mut last = 0.0;
        for t in &self.tiers {
            if !(t.radius.is_finite() && t.radius > last) {
                return Err("InterestConfig: tier radii must be positive and increasing");
            }
            if t.every < 1 {
                return Err("InterestConfig: a tier's `every` must be at least 1");
            }
            last = t.radius;
        }
        if !(self.hysteresis.is_finite() && self.hysteresis >= 0.0 && self.hysteresis < self.tiers[0].radius) {
            return Err("InterestConfig: hysteresis must be 0 or more and below the first radius");
        }
        Ok(())
    }
}

/// Somebody who looks at the world: a peer and where. A peer may have
/// several (split screen, a camera apart from its avatar); an entity counts
/// by its nearest.
/// ```
/// use ponko::prelude::*;
/// let v = ponko::net::InterestViewer { peer: ponko::net::PeerId::HOST, position: Vec3::ZERO };
/// assert_eq!(v.peer, ponko::net::PeerId::HOST);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterestViewer {
    /// Who receives updates.
    pub peer: PeerId,
    /// Where they look from.
    pub position: Vec3,
}

/// An entity in the world, for [`Interest::update`]. The id is yours (a
/// network id), unique within one update.
/// ```
/// use ponko::prelude::*;
/// let e = ponko::net::InterestEntity { id: 7, position: vec3(1.0, 2.0, 3.0) };
/// assert_eq!(e.id, 7);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterestEntity {
    /// Your identifier for it.
    pub id: u64,
    /// Where it is.
    pub position: Vec3,
}

/// One entity a peer is interested in, from [`Interest::entries`].
/// ```
/// let e = ponko::net::InterestEntry { entity: 1, tier: 0, distance: 3.0, send: true };
/// assert!(e.send);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterestEntry {
    /// The entity's id.
    pub entity: u64,
    /// Its ring: 0 is the nearest.
    pub tier: u8,
    /// Distance to the nearest viewer of this peer.
    pub distance: f32,
    /// Send its state to this peer on this tick. Always true on the tick it
    /// comes into interest.
    pub send: bool,
}

/// Whether an entity entered or left a peer's interest.
/// ```
/// assert_ne!(ponko::net::InterestChangeKind::Entered, ponko::net::InterestChangeKind::Left);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterestChangeKind {
    /// Start telling the peer about it (spawn it there).
    Entered,
    /// The peer no longer needs it (despawn it there), also when the entity
    /// is gone from the world.
    Left,
}

/// An entity entering or leaving a peer's interest, from
/// [`Interest::changes`].
/// ```
/// use ponko::prelude::*;
/// let c = ponko::net::InterestChange { peer: ponko::net::PeerId::HOST, entity: 3, kind: ponko::net::InterestChangeKind::Left };
/// assert_eq!(c.entity, 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterestChange {
    /// The peer concerned.
    pub peer: PeerId,
    /// The entity concerned.
    pub entity: u64,
    /// What happened.
    pub kind: InterestChangeKind,
}

#[derive(Default)]
struct PeerState {
    entries: Vec<InterestEntry>,
    /// Entity id → ring, as of the last update (the hysteresis memory).
    tiers: HashMap<u64, u8>,
    next_tiers: HashMap<u64, u8>,
    seen: bool,
}

/// Decides, tick by tick, which entities each peer receives and how often:
/// rings of distance around each viewer ([`InterestConfig`]), a hysteresis
/// at their edges, and a spatial grid so thousands of entities cost little.
///
/// Call [`update`](Self::update) once per send tick with the viewers and all
/// entities, then read [`entries`](Self::entries) per peer for what to send
/// and [`changes`](Self::changes) for who needs a spawn or despawn.
///
/// ```
/// use ponko::prelude::*;
/// let mut interest = ponko::net::Interest::new(ponko::net::InterestConfig::default());
/// let me = ponko::net::PeerId::HOST;
/// let viewers = [ponko::net::InterestViewer { peer: me, position: Vec3::ZERO }];
/// let world = [
///     ponko::net::InterestEntity { id: 1, position: vec3(10.0, 0.0, 0.0) },  // near
///     ponko::net::InterestEntity { id: 2, position: vec3(100.0, 0.0, 0.0) }, // mid
///     ponko::net::InterestEntity { id: 3, position: vec3(900.0, 0.0, 0.0) }, // far: never
/// ];
/// interest.update(0, &viewers, &world);
/// let ids: Vec<_> = interest.entries(me).iter().map(|e| (e.entity, e.tier)).collect();
/// assert_eq!(ids, [(1, 0), (2, 1)]);
/// // Entering is announced once; the mid entity then sends every 4th tick.
/// assert_eq!(interest.changes().len(), 2);
/// interest.update(1, &viewers, &world);
/// assert!(interest.changes().is_empty());
/// assert!(interest.entries(me)[0].send); // near: every tick
/// ```
pub struct Interest {
    config: InterestConfig,
    /// Largest distance anything can matter from: last radius + hysteresis.
    reach: f32,
    peers: HashMap<PeerId, PeerState>,
    changes: Vec<InterestChange>,
    // Scratch, reused by every update.
    cells: Vec<(u64, u32)>,
    order: Vec<u32>,
    dist: Vec<f32>,
    touched: Vec<u32>,
    left: Vec<u64>,
}

fn cell_of(p: Vec3, size: f32) -> [i32; 3] {
    let c = |v: f32| (v / size).floor() as i32;
    [c(p.x), c(p.y), c(p.z)]
}
fn cell_key(c: [i32; 3]) -> u64 {
    // 21 bits per axis; far-apart cells that alias only add candidates,
    // which the distance check then drops.
    let b = |v: i32| (v as u32 as u64) & 0x1F_FFFF;
    b(c[0]) << 42 | b(c[1]) << 21 | b(c[2])
}

impl Interest {
    /// A tracker for `config`.
    ///
    /// # Panics
    ///
    /// If the config is invalid: no tiers or more than
    /// [`MAX_INTEREST_TIERS`], radii not positive and increasing, an `every`
    /// of 0, or a hysteresis that is negative or not below the first radius.
    ///
    /// ```
    /// let interest = ponko::net::Interest::new(Default::default());
    /// assert_eq!(interest.config().tiers.len(), 2);
    /// ```
    pub fn new(config: InterestConfig) -> Self {
        if let Err(message) = config.validate() {
            panic!("{message}");
        }
        Self {
            reach: config.tiers.last().map_or(0.0, |t| t.radius) + config.hysteresis,
            config,
            peers: HashMap::new(),
            changes: Vec::new(),
            cells: Vec::new(),
            order: Vec::new(),
            dist: Vec::new(),
            touched: Vec::new(),
            left: Vec::new(),
        }
    }

    /// The settings it was made with.
    /// ```
    /// assert_eq!(ponko::net::Interest::new(Default::default()).config().hysteresis, 5.0);
    /// ```
    pub fn config(&self) -> &InterestConfig {
        &self.config
    }

    /// Which ring an entity at distance `d` is in, given the ring it was in
    /// (`None`: not in interest).
    fn decide(&self, prev: Option<u8>, d: f32) -> Option<u8> {
        let (tiers, h) = (&self.config.tiers, self.config.hysteresis);
        let n = tiers.len();
        let p = prev.map_or(n, usize::from);
        // Closer rings need the entity well inside them.
        if let Some(i) = (0..p).find(|&i| d <= tiers[i].radius - h) {
            return Some(i as u8);
        }
        if p == n {
            return None;
        }
        // Stay in this ring unless well outside it; then the nearest farther
        // ring it is not well outside of (or none).
        (p..n).find(|&j| d <= tiers[j].radius + h).map(|j| j as u8)
    }

    fn is_due(&self, tick: u64, id: u64, every: u32) -> bool {
        if every <= 1 {
            return true;
        }
        let phase = if self.config.stagger {
            id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32
        } else {
            0
        };
        (tick.wrapping_add(phase)) % u64::from(every) == 0
    }

    /// Recomputes every peer's interest for send tick `tick` from where the
    /// `viewers` are and where all `entities` are. Peers that have no viewer
    /// any more are forgotten, with their entities reported as left. Entities
    /// with a non-finite position are ignored. Reuses its buffers: no
    /// allocation once warmed up (apart from peers and entities coming and
    /// going).
    /// ```
    /// use ponko::prelude::*;
    /// let mut i = ponko::net::Interest::new(Default::default());
    /// let v = [ponko::net::InterestViewer { peer: ponko::net::PeerId::HOST, position: Vec3::ZERO }];
    /// let e = [ponko::net::InterestEntity { id: 1, position: vec3(5.0, 0.0, 0.0) }];
    /// i.update(0, &v, &e);
    /// assert_eq!(i.entries(ponko::net::PeerId::HOST).len(), 1);
    /// i.update(1, &[], &e); // nobody is looking: forgotten
    /// assert!(i.entries(ponko::net::PeerId::HOST).is_empty());
    /// assert_eq!(i.changes().len(), 1);
    /// ```
    pub fn update(&mut self, tick: u64, viewers: &[InterestViewer], entities: &[InterestEntity]) {
        self.changes.clear();
        for state in self.peers.values_mut() {
            state.seen = false;
        }

        // A grid of cells as big as the farthest reach: a viewer's reach
        // touches at most 3×3×3 of them.
        let size = self.reach;
        self.cells.clear();
        self.cells.extend(
            entities
                .iter()
                .enumerate()
                .filter(|(_, e)| e.position.is_finite())
                .map(|(i, e)| (cell_key(cell_of(e.position, size)), i as u32)),
        );
        self.cells.sort_unstable();
        self.dist.clear();
        self.dist.resize(entities.len(), f32::INFINITY);

        // Viewers grouped by peer, in a fixed order.
        self.order.clear();
        self.order
            .extend((0..viewers.len() as u32).filter(|&i| viewers[i as usize].position.is_finite()));
        self.order.sort_by_key(|&i| (viewers[i as usize].peer, i));

        let mut start = 0;
        while start < self.order.len() {
            let peer = viewers[self.order[start] as usize].peer;
            let end = start
                + self.order[start..]
                    .iter()
                    .take_while(|&&i| viewers[i as usize].peer == peer)
                    .count();
            self.touched.clear();
            for &vi in &self.order[start..end] {
                let at = viewers[vi as usize].position;
                let (lo, hi) = (
                    cell_of(at - Vec3::splat(self.reach), size),
                    cell_of(at + Vec3::splat(self.reach), size),
                );
                for x in lo[0]..=hi[0] {
                    for y in lo[1]..=hi[1] {
                        for z in lo[2]..=hi[2] {
                            let key = cell_key([x, y, z]);
                            let from = self.cells.partition_point(|c| c.0 < key);
                            for &(k, idx) in &self.cells[from..] {
                                if k != key {
                                    break;
                                }
                                let d = at.distance(entities[idx as usize].position);
                                if d <= self.reach {
                                    let slot = &mut self.dist[idx as usize];
                                    if slot.is_infinite() {
                                        self.touched.push(idx);
                                    }
                                    *slot = slot.min(d);
                                }
                            }
                        }
                    }
                }
            }
            self.touched.sort_unstable();

            let mut state = self.peers.remove(&peer).unwrap_or_default();
            state.seen = true;
            state.entries.clear();
            state.next_tiers.clear();
            for &idx in &self.touched {
                let e = &entities[idx as usize];
                let d = self.dist[idx as usize];
                self.dist[idx as usize] = f32::INFINITY; // ready for the next peer
                let prev = state.tiers.get(&e.id).copied();
                let Some(tier) = self.decide(prev, d) else { continue };
                let entered = prev.is_none();
                if entered {
                    self.changes.push(InterestChange {
                        peer,
                        entity: e.id,
                        kind: InterestChangeKind::Entered,
                    });
                }
                state.next_tiers.insert(e.id, tier);
                state.entries.push(InterestEntry {
                    entity: e.id,
                    tier,
                    distance: d,
                    send: entered || self.is_due(tick, e.id, self.config.tiers[tier as usize].every),
                });
            }
            self.report_left(peer, &mut state);
            self.peers.insert(peer, state);
            start = end;
        }

        // Peers without a viewer are gone.
        let mut gone: Vec<PeerId> = self.peers.iter().filter(|(_, s)| !s.seen).map(|(p, _)| *p).collect();
        gone.sort();
        for peer in gone {
            let mut state = self.peers.remove(&peer).unwrap_or_default();
            state.next_tiers.clear();
            self.report_left(peer, &mut state);
        }
    }

    /// Reports entities in `state.tiers` but not `state.next_tiers` as left,
    /// then makes `next_tiers` the memory.
    fn report_left(&mut self, peer: PeerId, state: &mut PeerState) {
        self.left.clear();
        self.left
            .extend(state.tiers.keys().filter(|id| !state.next_tiers.contains_key(id)));
        self.left.sort_unstable();
        self.changes.extend(self.left.iter().map(|&entity| InterestChange {
            peer,
            entity,
            kind: InterestChangeKind::Left,
        }));
        std::mem::swap(&mut state.tiers, &mut state.next_tiers);
    }

    /// Everything `peer` is interested in as of the last update, nearest ring
    /// first within the order the entities were given. Empty for unknown peers.
    /// ```
    /// assert!(ponko::net::Interest::new(Default::default()).entries(ponko::net::PeerId::HOST).is_empty());
    /// ```
    pub fn entries(&self, peer: PeerId) -> &[InterestEntry] {
        self.peers.get(&peer).map_or(&[], |s| &s.entries)
    }

    /// What to send `peer` this tick: its entries with `send` set.
    /// ```
    /// use ponko::prelude::*;
    /// let mut i = ponko::net::Interest::new(Default::default());
    /// let v = [ponko::net::InterestViewer { peer: ponko::net::PeerId::HOST, position: Vec3::ZERO }];
    /// let e = [ponko::net::InterestEntity { id: 1, position: vec3(5.0, 0.0, 0.0) }];
    /// i.update(0, &v, &e);
    /// assert_eq!(i.due(ponko::net::PeerId::HOST).count(), 1);
    /// ```
    pub fn due(&self, peer: PeerId) -> impl Iterator<Item = &InterestEntry> {
        self.entries(peer).iter().filter(|e| e.send)
    }

    /// Entities that entered or left a peer's interest in the last update:
    /// first for each peer (in id order) those that entered, in the order
    /// given, then those that left, by id.
    /// ```
    /// assert!(ponko::net::Interest::new(Default::default()).changes().is_empty());
    /// ```
    pub fn changes(&self) -> &[InterestChange] {
        &self.changes
    }

    /// Peers with a viewer in the last update.
    /// ```
    /// assert_eq!(ponko::net::Interest::new(Default::default()).peers().count(), 0);
    /// ```
    pub fn peers(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.keys().copied()
    }

    /// Forgets a peer (it disconnected) without reporting its entities as
    /// left. Unknown peers are ignored.
    /// ```
    /// let mut i = ponko::net::Interest::new(Default::default());
    /// i.remove_peer(ponko::net::PeerId::HOST);
    /// ```
    pub fn remove_peer(&mut self, peer: PeerId) {
        self.peers.remove(&peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::vec3;

    fn peer(n: u64) -> PeerId {
        PeerId::from_bits(n)
    }
    fn viewer(n: u64, at: Vec3) -> InterestViewer {
        InterestViewer {
            peer: peer(n),
            position: at,
        }
    }
    fn ent(id: u64, x: f32) -> InterestEntity {
        InterestEntity {
            id,
            position: vec3(x, 0.0, 0.0),
        }
    }
    fn ids(i: &Interest, p: u64) -> Vec<(u64, u8)> {
        i.entries(peer(p)).iter().map(|e| (e.entity, e.tier)).collect()
    }
    fn plain() -> Interest {
        Interest::new(InterestConfig {
            hysteresis: 0.0,
            stagger: false,
            ..Default::default()
        })
    }

    #[test]
    fn rings_decide_tier_and_far_entities_are_not_sent() {
        let mut i = plain();
        i.update(
            0,
            &[viewer(1, Vec3::ZERO)],
            &[
                ent(1, 0.0),
                ent(2, 39.0),
                ent(3, 41.0),
                ent(4, 119.0),
                ent(5, 121.0),
                ent(6, -500.0),
            ],
        );
        assert_eq!(ids(&i, 1), [(1, 0), (2, 0), (3, 1), (4, 1)]);
    }

    #[test]
    fn slow_rings_send_every_nth_tick() {
        let mut i = plain();
        let (v, e) = ([viewer(1, Vec3::ZERO)], [ent(1, 10.0), ent(2, 80.0)]);
        let mut sends = [0, 0];
        for tick in 0..40 {
            i.update(tick, &v, &e);
            for entry in i.entries(peer(1)) {
                sends[entry.entity as usize - 1] += u32::from(entry.send);
            }
        }
        assert_eq!(sends, [40, 10]);
    }

    #[test]
    fn staggering_spreads_a_slow_ring_over_ticks() {
        let mut i = Interest::new(InterestConfig::default());
        let v = [viewer(1, Vec3::ZERO)];
        let e: Vec<_> = (0..400).map(|id| ent(id, 80.0)).collect();
        i.update(0, &v, &e); // all just entered: everyone sends once
        let mut per_tick = [0u32; 4];
        for tick in 1..=4u64 {
            i.update(tick, &v, &e);
            per_tick[(tick - 1) as usize] = i.due(peer(1)).count() as u32;
        }
        assert_eq!(per_tick.iter().sum::<u32>(), 400, "each entity sends once per 4 ticks");
        assert!(per_tick.iter().all(|&n| (60..=140).contains(&n)), "{per_tick:?}");
    }

    #[test]
    fn entering_always_sends_and_is_reported_once() {
        let mut i = plain();
        let v = [viewer(1, Vec3::ZERO)];
        i.update(1, &v, &[]);
        i.update(1, &v, &[ent(9, 100.0)]); // tick 1 is not a 4th tick for a mid entity
        assert!(i.entries(peer(1))[0].send);
        assert_eq!(
            i.changes(),
            [InterestChange {
                peer: peer(1),
                entity: 9,
                kind: InterestChangeKind::Entered
            }]
        );
        i.update(2, &v, &[ent(9, 100.0)]);
        assert!(!i.entries(peer(1))[0].send);
        assert!(i.changes().is_empty());
    }

    #[test]
    fn leaving_by_distance_or_despawn_is_reported_once() {
        let mut i = plain();
        let v = [viewer(1, Vec3::ZERO)];
        i.update(0, &v, &[ent(1, 10.0), ent(2, 10.0), ent(3, 10.0)]);
        i.update(1, &v, &[ent(1, 10.0), ent(2, 500.0)]); // 2 walked off, 3 despawned
        let left: Vec<_> = i
            .changes()
            .iter()
            .filter(|c| c.kind == InterestChangeKind::Left)
            .map(|c| c.entity)
            .collect();
        assert_eq!(left, [2, 3]);
        i.update(2, &v, &[ent(1, 10.0), ent(2, 500.0)]);
        assert!(i.changes().is_empty());
        assert_eq!(ids(&i, 1), [(1, 0)]);
    }

    #[test]
    fn hysteresis_stops_flicker_at_a_ring_edge() {
        // Shake an entity ±3 units around the near/mid boundary (40): with a
        // hysteresis of 5 it never changes ring; without it, it flips.
        let shake = |hysteresis: f32| {
            let mut i = Interest::new(InterestConfig {
                hysteresis,
                ..Default::default()
            });
            let v = [viewer(1, Vec3::ZERO)];
            let mut flips = 0;
            let mut last = None;
            for tick in 0..100 {
                let x = 40.0 + if tick % 2 == 0 { 3.0 } else { -3.0 };
                i.update(tick, &v, &[ent(1, x)]);
                let tier = i.entries(peer(1)).first().map(|e| e.tier);
                flips += usize::from(last.is_some() && tier != last);
                last = tier;
            }
            flips
        };
        assert!(shake(0.0) > 90);
        assert_eq!(shake(5.0), 0);
    }

    #[test]
    fn hysteresis_stops_flicker_at_the_outer_edge_and_still_lets_go() {
        let mut i = Interest::new(InterestConfig::default()); // outer ring 120, h = 5
        let v = [viewer(1, Vec3::ZERO)];
        let mut changes = 0;
        for tick in 0..60 {
            let x = 120.0 + if tick % 2 == 0 { 3.0 } else { -3.0 };
            i.update(tick, &v, &[ent(1, x)]);
            changes += i.changes().len();
        }
        assert_eq!(changes, 0, "an entity just outside never entered, so nothing flickered");
        // Walk in past the margin: it enters, then wobbling at the edge keeps it.
        i.update(60, &v, &[ent(1, 114.0)]);
        assert_eq!(i.entries(peer(1)).len(), 1);
        for tick in 61..100 {
            let x = 120.0 + if tick % 2 == 0 { 4.0 } else { -4.0 };
            i.update(tick, &v, &[ent(1, x)]);
            assert_eq!(i.entries(peer(1)).len(), 1, "tick {tick}");
        }
        // Past the margin it is let go.
        i.update(100, &v, &[ent(1, 126.0)]);
        assert!(i.entries(peer(1)).is_empty());
    }

    #[test]
    fn rings_with_hysteresis_move_one_way_at_a_time() {
        let mut i = Interest::new(InterestConfig::default());
        let v = [viewer(1, Vec3::ZERO)];
        let tier_at = |i: &mut Interest, tick, x| {
            i.update(tick, &v, &[ent(1, x)]);
            i.entries(peer(1)).first().map(|e| e.tier)
        };
        assert_eq!(tier_at(&mut i, 0, 30.0), Some(0));
        assert_eq!(tier_at(&mut i, 1, 44.0), Some(0)); // past 40 but within the margin
        assert_eq!(tier_at(&mut i, 2, 46.0), Some(1));
        assert_eq!(tier_at(&mut i, 3, 37.0), Some(1)); // inside 40 but not by the margin
        assert_eq!(tier_at(&mut i, 4, 34.0), Some(0));
        // Entering fresh at 38 (inside 40 by less than the margin): mid first.
        let mut j = Interest::new(InterestConfig::default());
        assert_eq!(tier_at(&mut j, 0, 38.0), Some(1));
    }

    #[test]
    fn a_peer_with_several_viewers_sees_what_any_of_them_sees() {
        let mut i = plain();
        let v = [
            viewer(1, Vec3::ZERO),
            viewer(2, vec3(1000.0, 0.0, 0.0)),
            viewer(1, vec3(500.0, 0.0, 0.0)),
        ];
        i.update(0, &v, &[ent(1, 10.0), ent(2, 480.0), ent(3, 250.0), ent(4, 990.0)]);
        assert_eq!(ids(&i, 1), [(1, 0), (2, 0)]);
        assert_eq!(ids(&i, 2), [(4, 0)]);
        // The nearest viewer sets the distance.
        assert_eq!(i.entries(peer(1))[1].distance, 20.0);
    }

    #[test]
    fn peers_are_independent_and_removed_when_they_have_no_viewer() {
        let mut i = plain();
        let e = [ent(1, 0.0), ent(2, 300.0)];
        i.update(0, &[viewer(1, Vec3::ZERO), viewer(2, vec3(300.0, 0.0, 0.0))], &e);
        assert_eq!(ids(&i, 1), [(1, 0)]);
        assert_eq!(ids(&i, 2), [(2, 0)]);
        i.update(1, &[viewer(2, vec3(300.0, 0.0, 0.0))], &e);
        assert!(i.entries(peer(1)).is_empty());
        assert_eq!(
            i.changes(),
            [InterestChange {
                peer: peer(1),
                entity: 1,
                kind: InterestChangeKind::Left
            }]
        );
        assert_eq!(i.peers().collect::<Vec<_>>(), [peer(2)]);
        // Coming back is a fresh start: entered again.
        i.update(2, &[viewer(1, Vec3::ZERO), viewer(2, vec3(300.0, 0.0, 0.0))], &e);
        assert_eq!(i.changes().len(), 1);
        i.remove_peer(peer(1));
        assert!(i.entries(peer(1)).is_empty());
    }

    #[test]
    fn matches_brute_force_with_thousands_of_entities() {
        let mut state = 0x1234_5678u32;
        let mut rand = move || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 8) as f32 / (1u32 << 24) as f32
        };
        let entities: Vec<_> = (0..5000)
            .map(|id| InterestEntity {
                id,
                position: vec3(
                    rand() * 2000.0 - 1000.0,
                    rand() * 100.0 - 50.0,
                    rand() * 2000.0 - 1000.0,
                ),
            })
            .collect();
        let viewers: Vec<_> = (0..6)
            .map(|n| viewer(n, vec3(rand() * 1600.0 - 800.0, 0.0, rand() * 1600.0 - 800.0)))
            .collect();
        let config = InterestConfig::default();
        let mut i = Interest::new(config.clone());
        i.update(0, &viewers, &entities);
        let mut total = 0;
        for v in &viewers {
            let expected: Vec<(u64, u8)> = entities
                .iter()
                .filter_map(|e| {
                    let d = v.position.distance(e.position);
                    // First update: no memory, so each ring needs the margin.
                    config
                        .tiers
                        .iter()
                        .position(|t| d <= t.radius - config.hysteresis)
                        .map(|t| (e.id, t as u8))
                })
                .collect();
            total += expected.len();
            assert_eq!(ids(&i, v.peer.to_bits()), expected);
        }
        assert!(total > 50, "the test should have something to compare ({total})");
    }

    #[test]
    fn works_with_negative_and_huge_coordinates() {
        let mut i = plain();
        let far = 3.0e9;
        let v = [viewer(1, vec3(-far, -far, far)), viewer(2, Vec3::ZERO)];
        let e = [
            InterestEntity {
                id: 1,
                position: vec3(-far + 5.0, -far, far),
            },
            InterestEntity {
                id: 2,
                position: vec3(-5.0, -5.0, -5.0),
            },
            InterestEntity {
                id: 3,
                position: vec3(f32::NAN, 0.0, 0.0),
            },
        ];
        i.update(0, &v, &e);
        assert_eq!(ids(&i, 1), [(1, 0)]);
        assert_eq!(ids(&i, 2), [(2, 0)]);
    }

    #[test]
    fn single_ring_and_flat_worlds_work() {
        let mut i = Interest::new(InterestConfig {
            tiers: vec![InterestTier { radius: 10.0, every: 2 }],
            hysteresis: 0.0,
            stagger: false,
        });
        let v = [viewer(1, Vec3::ZERO)];
        i.update(1, &v, &[ent(1, 5.0), ent(2, 15.0)]);
        assert_eq!(ids(&i, 1), [(1, 0)]);
    }

    #[test]
    fn output_order_is_deterministic() {
        let e: Vec<_> = (0..200).map(|id| ent(id, (id % 90) as f32)).collect();
        let v = [viewer(2, Vec3::ZERO), viewer(1, vec3(30.0, 0.0, 0.0))];
        let run = || {
            let mut i = Interest::new(InterestConfig::default());
            i.update(0, &v, &e);
            i.update(1, &v, &e[..150]);
            (
                i.changes().to_vec(),
                i.entries(peer(1)).to_vec(),
                i.entries(peer(2)).to_vec(),
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    #[should_panic(expected = "needs 1 to 8 tiers")]
    fn no_tiers_is_refused() {
        Interest::new(InterestConfig {
            tiers: vec![],
            ..Default::default()
        });
    }

    #[test]
    #[should_panic(expected = "radii must be positive and increasing")]
    fn unordered_radii_are_refused() {
        Interest::new(InterestConfig {
            tiers: vec![
                InterestTier { radius: 50.0, every: 1 },
                InterestTier { radius: 20.0, every: 2 },
            ],
            ..Default::default()
        });
    }

    #[test]
    #[should_panic(expected = "`every` must be at least 1")]
    fn zero_rate_is_refused() {
        Interest::new(InterestConfig {
            tiers: vec![InterestTier { radius: 50.0, every: 0 }],
            ..Default::default()
        });
    }

    #[test]
    #[should_panic(expected = "hysteresis must be 0 or more and below the first radius")]
    fn oversized_hysteresis_is_refused() {
        Interest::new(InterestConfig {
            hysteresis: 40.0,
            ..Default::default()
        });
    }
}
