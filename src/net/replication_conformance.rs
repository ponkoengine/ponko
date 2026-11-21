//! Conformance tests for declared replication ([`Replication`]) over the
//! message protocol: a server world and client worlds driven tick by tick,
//! on the deterministic scripted network (`net/scripted.rs`) with simulated
//! time, plus a run over every transport the message protocol has.
//!
//! # What replication guarantees
//!
//! Given a transport that keeps the message protocol's contract
//! (`net/message_conformance.rs`):
//!
//! | Guarantee | Test |
//! |---|---|
//! | Every client converges to the server's entities and values once changes stop, under loss, jitter, reordering and duplication; no packet ever names a baseline the client lost | `replicas_converge_under_loss_jitter_reordering_and_duplication` |
//! | Each spawn reaches a client at most once (one that dies before any packet with it gets through never needs to), each despawn removes its replica | `spawns_and_despawns_reach_every_client_once` |
//! | Changes travel as deltas against acknowledged baselines, much smaller than full states; an idle world sends headers only | `small_changes_travel_as_small_deltas`, `an_idle_world_sends_only_packet_headers` |
//! | Bytes per client stay within the budget; higher priority entities are fresher | `the_budget_caps_bandwidth_and_sends_the_most_important_first` |
//! | Interest: only entities near a client's own reach it; ones that leave are despawned there | `interest_sends_nearby_entities_and_despawns_ones_that_leave` |
//! | Owned entities are predicted exactly under loss and jitter (no corrections), and agree with the server | `owned_entities_predict_exactly_under_loss_and_jitter` |
//! | An ownership handoff moves prediction to the new owner | `an_ownership_handoff_moves_prediction_to_the_new_owner` |
//! | Events arrive once, in order, both ways, under loss | `events_arrive_once_and_in_order_both_ways_under_loss` |
//! | Raw events (bytes, declared by name, for C and C++) arrive once, unchanged, both ways, under loss, in one order with typed events | `raw_events_arrive_once_unchanged_and_in_order_with_typed_ones` |
//! | A raw event that isn't declared or is too long is refused with an error and nothing is sent; a raw event never matches a typed one | `raw_events_that_cannot_go_are_refused`, `a_raw_event_never_matches_a_typed_one` |
//! | A client with different declarations is refused, and told why | `a_client_with_different_declarations_is_refused` |
//! | Off, replication does nothing; on, its traffic never reaches the game's own message events | `replication_off_does_nothing`, `replication_traffic_never_reaches_the_games_message_events` |
//! | A client that leaves is forgotten; a client whose server is gone loses its replicas | `a_client_that_leaves_is_forgotten_and_a_lost_server_takes_its_replicas` |
//! | The same script gives the same traffic | `same_script_gives_the_same_replication_traffic` |
//! | Only entities whose replicated components changed are read and compared (the world's change tracking); a borrow that writes nothing sends nothing | `only_entities_that_changed_are_read_and_compared` |
//! | A component taken off an entity is taken off its replicas | `a_removed_component_is_removed_from_replicas` |
//! | A write the world can't see still arrives, within a sweep | `writes_the_world_cannot_see_arrive_within_a_sweep` |
//! | A world the game replaced is read in full | `a_replaced_world_is_read_in_full` |
//! | A server without clients keeps nothing pending, and its first client gets everything | `a_server_without_clients_keeps_nothing_pending` |
//! | A change is sent once; while its acknowledgement is on the way it isn't sent again | `a_change_is_sent_once_while_its_acknowledgement_is_on_the_way` |
//! | What isn't acknowledged in time (changes, despawns) is sent again after the resend timeout, not every tick, and arrives once the link is back | `unacknowledged_changes_are_resent_after_the_timeout_not_every_tick` |
//!
//! Over every transport the message protocol lists:
//!
//! | Transport | Test |
//! |---|---|
//! | Scripted (loss, jitter, reordering, duplication) | every test above |
//! | Loopback (`loopback_lossy`) | `replicates_over_the_lossy_loopback` |
//! | UDP, and WebSocket (what browsers use; a native client here) | `replicates_over_udp_and_websocket` |
//! | Steam relay (fake Steam API, `--features steam`) | `replicates_over_the_steam_relay` in `net/steam_messages.rs` |
//!
//! A browser joining a server is checked by `web/smoke/replication.mjs` (a
//! headless tab of the web build and a native dedicated server). Playing in
//! a real browser and two machines are checks for a person (the guide's
//! "Replication" chapter lists them).

use std::collections::BTreeMap;
use std::time::Duration;

use glam::{Vec2, vec2};
use ponko_ecs::{Component, Entity, World};
use serde::{Deserialize, Serialize};

use super::scripted::{Network, Shared, addr, port};
use crate::error::ErrorKind;
use crate::events::Events;
use crate::net::{
    InterestConfig, InterestTier, Interpolate, MAX_RAW_EVENT_BYTES, MessageEvent, NetEvent, NetMessages, PeerId,
    RawEvent, Replica, Replicate, Replicated, Replication, ReplicationRole,
};

pub(super) const HZ: f64 = 60.0;
const STEP: Duration = Duration::from_micros(16_667);

/// A plain replicated position, interpolated on clients.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Pos {
    #[replicate(min = -2000.0, max = 2000.0, step = 0.01)]
    pub(super) at: Vec2,
}

impl Interpolate for Pos {
    fn interpolate(a: &Self, b: &Self, span: f32, t: f32) -> Self {
        Self {
            at: Vec2::interpolate(&a.at, &b.at, span, t),
        }
    }
    fn extrapolate(prev: &Self, last: &Self, span: f32, ahead: f32) -> Self {
        Self {
            at: Vec2::extrapolate(&prev.at, &last.at, span, ahead),
        }
    }
}

/// A replicated number, set at once.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Hp(#[replicate(bits = 8)] pub(super) u8);

/// A predicted body, moved by its owner's input.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Body {
    #[replicate(min = -1000.0, max = 1000.0, step = 0.01)]
    pub(super) at: Vec2,
}

/// The input.
#[derive(Replicate, Clone, Copy, Default, PartialEq, Debug)]
pub(super) struct Stick {
    #[replicate(min = -1.0, max = 1.0, step = 0.01)]
    pub(super) dir: Vec2,
}

/// An event.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(super) struct Shout(pub(super) u32);

fn walk(body: &mut Body, stick: &Stick) {
    body.at = (body.at + stick.dir * 2.0).clamp(Vec2::splat(-500.0), Vec2::splat(500.0));
}

/// What every test declares, on every side.
pub(super) fn declarations() -> Replication {
    Replication::new()
        .interpolated::<Pos>()
        .component::<Hp>()
        .input::<Stick>()
        .predicted::<Body, Stick>(walk)
        .event::<Shout>()
}

/// One game: its world, events, endpoint and replication.
pub(super) struct Side {
    pub(super) world: World,
    pub(super) events: Events,
    pub(super) net: Option<NetMessages>,
    pub(super) rep: Replication,
    pub(super) tick: u64,
}

impl Side {
    pub(super) fn new(net: Option<NetMessages>, rep: Replication) -> Self {
        Self {
            world: World::new(),
            events: Events::default(),
            net,
            rep,
            tick: 0,
        }
    }
    /// A tick, as the engine runs one: replication takes what arrived,
    /// the game runs, replication sends.
    fn tick(&mut self, game: impl FnOnce(&mut Self)) {
        if let Some(net) = &mut self.net {
            let _ = net.poll_elapsed(STEP);
            net.events().for_each(drop);
        }
        self.rep
            .begin_tick(self.net.as_mut(), &mut self.world, &mut self.events, self.tick, HZ);
        game(self);
        self.rep.end_tick(self.net.as_mut(), &mut self.world);
        self.rep.frame(&mut self.world, 0.0);
        self.tick += 1;
    }
}

/// A server (`sides[0]`) and its clients.
pub(super) struct Session {
    net: Option<Shared>,
    pub(super) sides: Vec<Side>,
    /// Real sockets: give packets a moment each tick.
    real: bool,
}

impl Session {
    /// Over the scripted network, every side declaring `rep()`.
    fn scripted(clients: usize, rep: impl FnMut() -> Replication, script: impl FnOnce(&mut Network)) -> Self {
        let net: Shared = Shared::default();
        script(&mut net.lock().unwrap());
        let mut nets = vec![NetMessages::transported(port(&net, 0), None, clients.max(1))];
        for i in 1..=clients {
            nets.push(NetMessages::transported(port(&net, i), Some(addr(0)), 1));
        }
        let mut s = Self::over(nets, rep, false);
        s.net = Some(net);
        s
    }

    /// Over endpoints made elsewhere (`nets[0]` hosts).
    pub(super) fn over(nets: Vec<NetMessages>, mut rep: impl FnMut() -> Replication, real: bool) -> Self {
        Self {
            net: None,
            sides: nets.into_iter().map(|n| Side::new(Some(n), rep())).collect(),
            real,
        }
    }

    /// One tick on every side, `game(side index, side)` in the middle.
    pub(super) fn step(&mut self, mut game: impl FnMut(usize, &mut Side)) {
        if let Some(net) = &self.net {
            net.lock().unwrap().tick += 1;
        }
        if self.real {
            std::thread::sleep(Duration::from_millis(2));
        }
        for (i, side) in self.sides.iter_mut().enumerate() {
            side.tick(|s| game(i, s));
        }
    }

    pub(super) fn run(&mut self, ticks: usize, mut game: impl FnMut(usize, &mut Side)) {
        for _ in 0..ticks {
            self.step(&mut game);
        }
    }

    /// Ticks until every client joined (at most `limit`).
    pub(super) fn join(&mut self, limit: usize) {
        for _ in 0..limit {
            if self.sides[1..].iter().all(|s| s.rep.is_joined()) && self.server_sees_all() {
                return;
            }
            self.step(|_, _| {});
        }
        panic!(
            "clients did not join: {:?}",
            self.sides.iter().map(|s| s.rep.error()).collect::<Vec<_>>()
        );
    }

    fn server_sees_all(&self) -> bool {
        self.sides[0].rep.clients().len() == self.sides.len() - 1
    }

    pub(super) fn me(&self, client: usize) -> PeerId {
        self.sides[client].rep.me().expect("joined")
    }
}

/// Server entities by net id.
fn originals(world: &mut World) -> BTreeMap<u32, Entity> {
    let mut out = BTreeMap::new();
    world.query::<(Entity, &Replicated)>().for_each(|(e, r)| {
        if let Some(id) = r.net_id() {
            out.insert(id.index(), e);
        }
    });
    out
}

/// Client replicas by net id.
pub(super) fn replicas(world: &mut World) -> BTreeMap<u32, Entity> {
    let mut out = BTreeMap::new();
    world.query::<(Entity, &Replica)>().for_each(|(e, r)| {
        out.insert(r.id().index(), e);
    });
    out
}

fn close(a: Vec2, b: Vec2) -> bool {
    (a - b).abs().max_element() <= 0.006
}

/// Whether the replica `c` in `client` shows what `s` has in `server`.
fn same(server: &World, s: Entity, client: &World, c: Entity) -> bool {
    let pos = match (server.get::<Pos>(s), client.get::<Pos>(c)) {
        (Some(a), Some(b)) => close(a.at, b.at),
        (None, None) => true,
        _ => false,
    };
    let body = match (server.get::<Body>(s), client.get::<Body>(c)) {
        (Some(a), Some(b)) => a == b,
        (None, None) => true,
        _ => false,
    };
    pos && body && server.get::<Hp>(s) == client.get::<Hp>(c)
}

/// Every client shows exactly the server's entities (those `seen` says
/// it should see), with the same values.
fn assert_converged(session: &mut Session, seen: impl Fn(usize, &World, Entity) -> bool) {
    let all = originals(&mut session.sides[0].world);
    let have: Vec<BTreeMap<u32, Entity>> = session.sides.iter_mut().map(|s| replicas(&mut s.world)).collect();
    let server = &session.sides[0].world;
    for (i, side) in session.sides.iter().enumerate().skip(1) {
        let want: Vec<u32> = all
            .iter()
            .filter(|&(_, &e)| seen(i, server, e))
            .map(|(&id, _)| id)
            .collect();
        let have = &have[i];
        assert_eq!(have.keys().copied().collect::<Vec<_>>(), want, "client {i}'s entities");
        for id in want {
            assert!(
                same(server, all[&id], &side.world, have[&id]),
                "client {i}, id {id}: {:?} / {:?} vs {:?} / {:?}",
                server.get::<Pos>(all[&id]),
                server.get::<Hp>(all[&id]),
                side.world.get::<Pos>(have[&id]),
                side.world.get::<Hp>(have[&id]),
            );
        }
        let stats = side.rep.client_stats();
        assert_eq!(stats.packets_bad, 0, "client {i} never lacked a baseline: {stats:?}");
    }
}

/// The world changes a little every tick: entities move, some spawn, some
/// die, health changes. Deterministic in `tick`.
fn churn(world: &mut World, tick: u64) {
    let mut alive: Vec<Entity> = Vec::new();
    world.query::<(Entity, &mut Pos, &mut Hp)>().for_each(|(e, p, hp)| {
        let k = e.index() as f32;
        p.at = vec2(
            (tick as f32 * 0.37 + k * 13.0).sin() * 400.0,
            k * 3.0 - tick as f32 * 0.1,
        );
        if (tick + u64::from(e.index())).is_multiple_of(7) {
            hp.0 = hp.0.wrapping_add(3);
        }
        alive.push(e);
    });
    if tick.is_multiple_of(10) {
        world.spawn((
            Pos {
                at: vec2(tick as f32, 0.0),
            },
            Hp(100),
            Replicated::new(),
        ));
        if let Some(&old) = alive.get((tick as usize / 10) % alive.len().max(1)) {
            world.despawn(old);
        }
    }
}

fn spawn_crowd(world: &mut World, n: usize) {
    for i in 0..n {
        world.spawn((
            Pos {
                at: vec2(i as f32, -(i as f32)),
            },
            Hp(i as u8),
            Replicated::new(),
        ));
    }
}

#[test]
fn replicas_converge_under_loss_jitter_reordering_and_duplication() {
    let mut s = Session::scripted(3, declarations, |n| {
        n.delay = 2;
        n.jitter = 6;
        n.loss_per_mille = 200;
        n.duplicate = true;
        n.newest_first = true;
    });
    spawn_crowd(&mut s.sides[0].world, 40);
    s.join(400);
    s.run(300, |i, side| {
        if i == 0 {
            churn(&mut side.world, side.tick);
        }
    });
    // Quiet: what's in flight lands, losses are repaired.
    s.run(120, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
    for side in &s.sides[1..] {
        let stats = side.rep.client_stats();
        assert!(stats.packets_stale > 0, "reordering happened: {stats:?}");
    }
    let sent = s.sides[0].rep.peer_stats(s.me(1)).expect("client 1");
    assert!(sent.deltas_sent > sent.fulls_sent, "{sent:?}");
}

#[test]
fn packets_of_one_tick_apply_in_any_order() {
    // No budget and many changing entities: several packets a tick, each
    // with different entities, delivered newest first (ticks stay in
    // order). Every record is news for its entity, late packet or not.
    let mut s = Session::scripted(
        2,
        || declarations().budget(u32::MAX),
        |n| {
            n.delay = 1;
            n.newest_first = true;
        },
    );
    spawn_crowd(&mut s.sides[0].world, 400);
    s.join(400);
    let ticks = 120;
    s.run(ticks, |i, side| {
        if i == 0 {
            churn(&mut side.world, side.tick);
        }
    });
    s.run(10, |_, _| {});
    let sent = s.sides[0].rep.peer_stats(s.me(1)).expect("client 1");
    let got = s.sides[1].rep.client_stats();
    assert!(sent.packets_sent > 2 * ticks as u64, "several packets a tick: {sent:?}");
    assert!(got.packets_stale > ticks as u64, "packets arrived late: {got:?}");
    assert_eq!(got.records_applied, sent.records_sent, "every record applied: {got:?}");
    assert_eq!(got.packets_bad, 0, "{got:?}");
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn spawns_and_despawns_reach_every_client_once() {
    let mut s = Session::scripted(2, declarations, |n| {
        n.jitter = 3;
        n.loss_per_mille = 250;
        n.duplicate = true;
    });
    s.join(400);
    let mut spawned = 0;
    let mut despawned = 0;
    let mut alive: Vec<Entity> = Vec::new();
    for round in 0..40u32 {
        s.step(|i, side| {
            if i != 0 {
                return;
            }
            // Despawn first: none of these lives for less than a tick.
            if round % 2 == 1 {
                for e in alive.drain(..alive.len().min(4)) {
                    side.world.despawn(e);
                    despawned += 1;
                }
            }
            for k in 0..3 {
                alive.push(side.world.spawn((Hp(k), Replicated::new())));
                spawned += 1;
            }
        });
    }
    s.run(120, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
    for side in &s.sides[1..] {
        let stats = side.rep.client_stats();
        assert_eq!(
            stats.spawned - stats.despawned,
            (spawned - despawned) as u64,
            "{stats:?}"
        );
        // At most once each: one that died before a packet with it got
        // through never needs to appear.
        assert!(
            stats.spawned <= spawned as u64 && stats.spawned >= spawned as u64 * 9 / 10,
            "{stats:?}"
        );
    }
}

#[test]
fn small_changes_travel_as_small_deltas() {
    let mut s = Session::scripted(1, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 100);
    s.join(200);
    s.run(30, |_, _| {});
    let peer = s.me(1);
    let before = s.sides[0].rep.peer_stats(peer).unwrap();
    // Every entity moves one step of 0.01 a tick.
    s.run(60, |i, side| {
        if i == 0 {
            side.world.query::<&mut Pos>().for_each(|p| p.at.x += 0.01);
        }
    });
    let after = s.sides[0].rep.peer_stats(peer).unwrap();
    let packets = after.packets_sent - before.packets_sent;
    let per_packet = (after.bytes_sent - before.bytes_sent) as f64 / packets as f64;
    // A full state of 100 entities is 100 × (id, record type, owner,
    // epoch, kinds, 2 × 19 bits of position, 8 of health) > 900 bytes; a
    // delta of one small change is about 4 bytes an entity.
    assert!(per_packet < 450.0, "{per_packet} bytes a packet");
    assert!(after.deltas_sent - before.deltas_sent >= 100 * 50, "{after:?}");
    s.run(30, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn an_idle_world_sends_only_packet_headers() {
    let mut s = Session::scripted(1, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 200);
    s.join(200);
    s.run(60, |_, _| {});
    let peer = s.me(1);
    let before = s.sides[0].rep.peer_stats(peer).unwrap();
    s.run(60, |_, _| {});
    let after = s.sides[0].rep.peer_stats(peer).unwrap();
    assert_eq!(after.records_sent, before.records_sent, "nothing changed, nothing sent");
    let per_packet = (after.bytes_sent - before.bytes_sent) / (after.packets_sent - before.packets_sent);
    // Kind, sequence, tick, input ack, no destroys, end: 9 bytes.
    assert!(per_packet <= 10, "{per_packet} bytes a packet");
}

/// A counter, for watching how far behind a client is.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
struct Count(u32);

#[test]
fn the_budget_caps_bandwidth_and_sends_the_most_important_first() {
    const BUDGET: u32 = 6_000;
    let mut s = Session::scripted(1, || Replication::new().component::<Count>().budget(BUDGET), |_| {});
    {
        let world = &mut s.sides[0].world;
        for i in 0..400 {
            let priority = if i < 20 { 20.0 } else { 1.0 };
            world.spawn((Count(0), Replicated::new().with_priority(priority)));
        }
    }
    s.join(200);
    // Let the spawns through (the budget sends them over a second or so).
    s.run(300, |_, _| {});
    assert_eq!(replicas(&mut s.sides[1].world).len(), 400);
    let peer = s.me(1);
    let before = s.sides[0].rep.peer_stats(peer).unwrap();
    let ticks = 300;
    // Every entity changes every tick: far more than fits.
    s.run(ticks, |i, side| {
        if i == 0 {
            side.world.query::<&mut Count>().for_each(|c| c.0 += 1);
        }
    });
    let after = s.sides[0].rep.peer_stats(peer).unwrap();
    let seconds = ticks as f64 / HZ;
    let rate = (after.bytes_sent - before.bytes_sent) as f64 / seconds;
    // The bucket may hold four ticks' worth plus one packet at the start.
    let saved = f64::from(BUDGET) / HZ * 4.0 + 1100.0;
    assert!(rate <= f64::from(BUDGET) * 1.05 + saved / seconds, "{rate} B/s");
    assert!(rate >= f64::from(BUDGET) * 0.8, "the budget is used: {rate} B/s");
    assert!(after.deferred > before.deferred, "{after:?}");
    // How far behind the client is, by priority.
    let have = replicas(&mut s.sides[1].world);
    let all = originals(&mut s.sides[0].world);
    let server = &s.sides[0].world;
    let client = &s.sides[1].world;
    let (mut high, mut low) = (Vec::new(), Vec::new());
    for (id, e) in all {
        let now = server.get::<Count>(e).unwrap().0;
        let shown = client.get::<Count>(have[&id]).unwrap().0;
        let behind = f64::from(now - shown);
        let priority = server.get::<Replicated>(e).unwrap().priority();
        if priority > 1.0 { &mut high } else { &mut low }.push(behind);
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    assert!(
        mean(&high) * 4.0 < mean(&low),
        "high priority is fresher: {} vs {} ticks behind",
        mean(&high),
        mean(&low)
    );
}

#[test]
fn interest_sends_nearby_entities_and_despawns_ones_that_leave() {
    let config = InterestConfig {
        tiers: vec![InterestTier {
            radius: 100.0,
            every: 1,
        }],
        hysteresis: 5.0,
        stagger: false,
    };
    let rep = || {
        declarations().interest(config.clone()).position(|w, e| {
            w.get::<Pos>(e)
                .map(|p| p.at.extend(0.0))
                .or_else(|| w.get::<Body>(e).map(|b| b.at.extend(0.0)))
        })
    };
    let mut s = Session::scripted(1, rep, |_| {});
    {
        let world = &mut s.sides[0].world;
        for i in 0..20 {
            world.spawn((
                Pos {
                    at: vec2(i as f32 * 20.0, 0.0),
                },
                Replicated::new(),
            ));
        }
        world.spawn((Hp(1), Replicated::new())); // no position: everyone's
        world.spawn((Pos { at: vec2(1500.0, 0.0) }, Replicated::new().global()));
    }
    s.join(200);
    let peer = s.me(1);
    let viewer = s.sides[0]
        .world
        .spawn((Body { at: vec2(0.0, 0.0) }, Replicated::owned_by(peer)));
    s.run(30, |_, _| {});
    // Entering takes being the hysteresis inside the ring: 95 units.
    let near = |x: f32| x.abs() <= 95.0;
    let seen = |world: &World, e: Entity, at: f32| {
        world.get::<Replicated>(e).is_some_and(Replicated::is_global)
            || world.get::<Pos>(e).is_none_or(|p| near(p.at.x - at))
    };
    assert_converged(&mut s, |_, w, e| seen(w, e, 0.0));
    // The viewer walks to x = 300: entities at 0..=80 leave, 220..=380 arrive.
    s.sides[0].world.get_mut::<Body>(viewer).unwrap().at.x = 300.0;
    s.run(30, |_, _| {});
    assert_converged(&mut s, |_, w, e| seen(w, e, 300.0));
    assert_eq!(s.sides[1].rep.client_stats().despawned, 5);
}

#[test]
fn owned_entities_predict_exactly_under_loss_and_jitter() {
    let mut s = Session::scripted(2, declarations, |n| {
        n.delay = 3;
        n.jitter = 4;
        n.loss_per_mille = 150;
        n.newest_first = true;
    });
    s.join(400);
    let (a, b) = (s.me(1), s.me(2));
    let world = &mut s.sides[0].world;
    let mine_a = world.spawn((Body::default(), Replicated::owned_by(a)));
    let mine_b = world.spawn((Body { at: vec2(50.0, 0.0) }, Replicated::owned_by(b)));
    let host = world.spawn((Body { at: vec2(-50.0, 0.0) }, Replicated::owned_by(PeerId::HOST)));
    s.run(300, |i, side| {
        let t = side.tick as f32 * 0.05;
        let dir = match i {
            0 => vec2(0.0, 1.0),
            1 => vec2(t.cos(), t.sin()),
            _ => vec2(-t.sin(), 0.5),
        };
        side.rep.set_input(Stick { dir });
    });
    // Stop pressing, let everything land.
    s.run(120, |_, side| side.rep.set_input(Stick::default()));
    for (i, side) in s.sides.iter().enumerate().skip(1) {
        let stats = side.rep.client_stats();
        assert_eq!(stats.corrections, 0, "client {i} predicted exactly: {stats:?}");
        assert!(stats.confirmed > 100, "{stats:?}");
    }
    assert_converged(&mut s, |_, _, _| true);
    let server = &s.sides[0].world;
    assert!(server.get::<Body>(mine_a).unwrap().at.length() > 10.0, "client 1 moved");
    assert!(
        server.get::<Body>(mine_b).unwrap().at != vec2(50.0, 0.0),
        "client 2 moved"
    );
    assert_eq!(
        server.get::<Body>(host).unwrap().at,
        vec2(-50.0, 500.0),
        "the host's own input moved its body"
    );
    let replica = replicas(&mut s.sides[1].world);
    let mine: Vec<bool> = replica
        .values()
        .map(|&e| s.sides[1].world.get::<Replica>(e).unwrap().is_mine())
        .collect();
    assert_eq!(mine.iter().filter(|m| **m).count(), 1, "client 1 owns one");
}

#[test]
fn an_ownership_handoff_moves_prediction_to_the_new_owner() {
    let mut s = Session::scripted(2, declarations, |n| n.delay = 2);
    s.join(200);
    let (a, b) = (s.me(1), s.me(2));
    let ball = s.sides[0].world.spawn((Body::default(), Replicated::owned_by(a)));
    let push = |_: usize, side: &mut Side| side.rep.set_input(Stick { dir: vec2(1.0, 0.0) });
    s.run(60, push);
    let at_handoff = s.sides[0].world.get::<Body>(ball).unwrap().at.x;
    assert!(at_handoff > 50.0);
    s.sides[0].world.get_mut::<Replicated>(ball).unwrap().set_owner(Some(b));
    s.run(60, push);
    s.run(60, |_, side| side.rep.set_input(Stick::default()));
    let mine = |side: &mut Side| {
        let r = replicas(&mut side.world);
        side.world.get::<Replica>(r[&0]).copied().unwrap()
    };
    assert!(!mine(&mut s.sides[1]).is_mine() && mine(&mut s.sides[2]).is_mine());
    assert_eq!(mine(&mut s.sides[2]).epoch(), 1, "a new authority epoch");
    let x = s.sides[0].world.get::<Body>(ball).unwrap().at.x;
    // b's 60 pushes, plus the few it sent just before the handoff that
    // arrived after it; a's last ones in flight no longer apply.
    assert!(
        (x - (at_handoff + 120.0)).abs() <= 12.0,
        "b pushed it on: {at_handoff} -> {x}"
    );
    // The new owner starts from the server's state plus the inputs it
    // hasn't applied yet: exact from the first tick.
    for (i, side) in s.sides.iter().enumerate().skip(1) {
        let stats = side.rep.client_stats();
        assert_eq!(stats.corrections, 0, "client {i}: {stats:?}");
    }
    assert!(s.sides[2].rep.client_stats().confirmed > 30);
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn events_arrive_once_and_in_order_both_ways_under_loss() {
    let mut s = Session::scripted(2, declarations, |n| {
        n.loss_per_mille = 300;
        n.jitter = 5;
        n.duplicate = true;
    });
    s.join(600);
    let mut got: Vec<Vec<(PeerId, u32)>> = vec![Vec::new(); 3];
    for round in 0..40u32 {
        s.step(|i, side| {
            for e in side.events.read::<NetEvent<Shout>>() {
                got[i].push((e.from, e.event.0));
            }
            match i {
                0 => {
                    side.rep.to_clients(&Shout(round));
                    if round == 7 {
                        side.rep.to_server(&Shout(1000)); // the host itself
                    }
                }
                _ => side.rep.to_server(&Shout(round * 10 + i as u32)),
            }
        });
    }
    s.run(200, |i, side| {
        for e in side.events.read::<NetEvent<Shout>>() {
            got[i].push((e.from, e.event.0));
        }
    });
    for (i, events) in got.iter().enumerate().skip(1) {
        let numbers: Vec<u32> = events.iter().map(|e| e.1).collect();
        assert_eq!(numbers, (0..40).collect::<Vec<_>>(), "client {i}: all, once, in order");
        assert!(events.iter().all(|e| e.0 == PeerId::HOST));
    }
    for i in 1..=2u32 {
        let from = s.me(i as usize);
        let numbers: Vec<u32> = got[0].iter().filter(|e| e.0 == from).map(|e| e.1).collect();
        assert_eq!(numbers, (0..40).map(|r| r * 10 + i).collect::<Vec<_>>());
    }
    assert_eq!(
        got[0]
            .iter()
            .filter(|e| e.0 == PeerId::HOST)
            .map(|e| e.1)
            .collect::<Vec<_>>(),
        [1000]
    );
}

#[test]
fn a_client_with_different_declarations_is_refused() {
    let mut first = true;
    let mut s = Session::scripted(
        1,
        || {
            let r = if first {
                declarations()
            } else {
                declarations().component::<Pos2>()
            };
            first = false;
            r
        },
        |_| {},
    );
    spawn_crowd(&mut s.sides[0].world, 3);
    s.run(100, |_, _| {});
    assert!(!s.sides[1].rep.is_joined());
    assert!(s.sides[1].rep.error().is_some_and(|e| e.contains("different")));
    assert!(s.sides[0].rep.clients().is_empty());
    assert!(replicas(&mut s.sides[1].world).is_empty(), "nothing replicated");
}

/// Declared only by a client that doesn't match.
#[derive(Component, Replicate, Clone, Copy, Default, PartialEq, Debug)]
struct Pos2(f32);

#[test]
fn replication_off_does_nothing() {
    let mut s = Session::scripted(1, Replication::default, |_| {});
    spawn_crowd(&mut s.sides[0].world, 10);
    s.run(100, |_, _| {});
    for side in &s.sides {
        assert_eq!(side.rep.role(), ReplicationRole::Off);
        assert!(!side.rep.is_active());
    }
    assert!(originals(&mut s.sides[0].world).is_empty(), "no ids handed out");
    assert!(replicas(&mut s.sides[1].world).is_empty());
    // Only the session's own traffic: the same as with no entities at all.
    let bytes = |with: usize| {
        let mut s = Session::scripted(1, Replication::default, |_| {});
        spawn_crowd(&mut s.sides[0].world, with);
        s.run(100, |_, _| {});
        s.sides[0].net.as_ref().unwrap().stats().bytes_sent
    };
    assert_eq!(bytes(10), bytes(0));
}

#[test]
fn replication_traffic_never_reaches_the_games_message_events() {
    let net: Shared = Shared::default();
    let host = NetMessages::transported(port(&net, 0), None, 1);
    let client = NetMessages::transported(port(&net, 1), Some(addr(0)), 1);
    let mut sides = [
        Side::new(Some(host), declarations()),
        Side::new(Some(client), declarations()),
    ];
    spawn_crowd(&mut sides[0].world, 20);
    let mut seen: Vec<Vec<MessageEvent>> = vec![Vec::new(), Vec::new()];
    for t in 0..200 {
        net.lock().unwrap().tick += 1;
        for (i, side) in sides.iter_mut().enumerate() {
            let m = side.net.as_mut().unwrap();
            m.poll_elapsed(STEP).unwrap();
            seen[i].extend(m.events());
            side.rep
                .begin_tick(side.net.as_mut(), &mut side.world, &mut side.events, side.tick, HZ);
            if i == 1 && t == 100 {
                let m = side.net.as_mut().unwrap();
                m.send(PeerId::HOST, crate::net::Lane::Reliable, b"mine").unwrap();
            }
            side.rep.end_tick(side.net.as_mut(), &mut side.world);
            side.tick += 1;
        }
    }
    assert_eq!(replicas(&mut sides[1].world).len(), 20, "replication ran");
    let messages: Vec<&MessageEvent> = seen[0]
        .iter()
        .filter(|e| matches!(e, MessageEvent::Message { .. }))
        .collect();
    assert_eq!(messages.len(), 1, "only the game's own message: {messages:?}");
    assert!(seen[1].iter().all(|e| matches!(e, MessageEvent::Connected(_))));
}

#[test]
fn a_client_that_leaves_is_forgotten_and_a_lost_server_takes_its_replicas() {
    let mut s = Session::scripted(2, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 10);
    s.join(200);
    let leaver = s.me(2);
    s.sides[0].world.spawn((Body::default(), Replicated::owned_by(leaver)));
    s.run(30, |_, _| {});
    assert_eq!(replicas(&mut s.sides[2].world).len(), 11);
    s.sides[2].net.as_mut().unwrap().disconnect(PeerId::HOST).unwrap();
    s.run(30, |_, _| {});
    assert_eq!(s.sides[0].rep.clients(), [s.me(1)], "the leaver is forgotten");
    assert!(s.sides[0].rep.peer_stats(leaver).is_none());
    assert!(
        replicas(&mut s.sides[2].world).is_empty(),
        "its replicas went with the server"
    );
    assert_eq!(replicas(&mut s.sides[1].world).len(), 11, "the others carry on");
}

/// What a client holds of one entity: its net id and the values.
type Seen = (u32, Option<Pos>, Option<Hp>);

#[test]
fn same_script_gives_the_same_replication_traffic() {
    let run = || {
        let mut s = Session::scripted(2, declarations, |n| {
            n.loss_per_mille = 100;
            n.jitter = 3;
        });
        spawn_crowd(&mut s.sides[0].world, 30);
        s.run(200, |i, side| {
            if i == 0 {
                churn(&mut side.world, side.tick);
            }
            side.rep.set_input(Stick { dir: vec2(0.5, -0.5) });
        });
        // Datagrams carry the session's random nonces: compare what
        // replication did instead, side by side.
        let stats: Vec<_> = (1..s.sides.len())
            .map(|i| {
                let peer = s.me(i);
                (s.sides[i].rep.client_stats(), s.sides[0].rep.peer_stats(peer))
            })
            .collect();
        let worlds: Vec<Vec<Seen>> = s
            .sides
            .iter_mut()
            .skip(1)
            .map(|side| {
                replicas(&mut side.world)
                    .into_iter()
                    .map(|(id, e)| (id, side.world.get::<Pos>(e).copied(), side.world.get::<Hp>(e).copied()))
                    .collect()
            })
            .collect();
        (stats, worlds)
    };
    let (a, b) = (run(), run());
    assert!(a.0[0].0.records_applied > 0);
    assert_eq!(a, b);
}

/// What the transport tests run: a crowd that moves, a body each client
/// drives, events both ways; then everything must match.
pub(super) fn exercise(mut s: Session, join_limit: usize, ticks: usize) {
    spawn_crowd(&mut s.sides[0].world, 25);
    s.join(join_limit);
    let owners: Vec<PeerId> = (1..s.sides.len()).map(|i| s.me(i)).collect();
    for &peer in &owners {
        s.sides[0].world.spawn((Body::default(), Replicated::owned_by(peer)));
    }
    let mut heard = vec![0u32; s.sides.len()];
    s.run(ticks, |i, side| {
        heard[i] += side.events.read::<NetEvent<Shout>>().len() as u32;
        if i == 0 {
            churn(&mut side.world, side.tick);
            if side.tick.is_multiple_of(10) {
                side.rep.to_clients(&Shout(1));
            }
        } else {
            side.rep.set_input(Stick { dir: vec2(0.3, 0.2) });
            if side.tick.is_multiple_of(10) {
                side.rep.to_server(&Shout(2));
            }
        }
    });
    s.run(ticks / 2, |i, side| {
        heard[i] += side.events.read::<NetEvent<Shout>>().len() as u32;
        side.rep.set_input(Stick::default());
    });
    assert_converged(&mut s, |_, _, _| true);
    assert!(heard.iter().all(|&h| h > 0), "events both ways: {heard:?}");
    for (i, side) in s.sides.iter_mut().enumerate().skip(1) {
        let stats = side.rep.client_stats();
        assert!(stats.confirmed > 0, "client {i} predicted: {stats:?}");
        let r = replicas(&mut side.world);
        assert_eq!(
            r.values()
                .filter(|&&e| side.world.get::<Replica>(e).is_some_and(Replica::is_mine))
                .count(),
            1
        );
    }
}

#[test]
fn replicates_over_the_lossy_loopback() {
    let nets = NetMessages::loopback_lossy(3, 7);
    exercise(Session::over(nets, declarations, false), 400, 240);
}

#[test]
fn replicates_over_udp_and_websocket() {
    let host = NetMessages::host("127.0.0.1:0", 2)
        .unwrap()
        .accept_web("127.0.0.1:0")
        .unwrap();
    let udp = NetMessages::join(host.local_addr().unwrap()).unwrap();
    let web = NetMessages::join_web(&format!("ws://{}", host.web_addr().unwrap())).unwrap();
    exercise(Session::over(vec![host, udp, web], declarations, true), 600, 120);
}

/// The net ids of `entities` in the server's world, sorted.
fn net_ids(world: &World, entities: &[Entity]) -> Vec<u32> {
    let mut ids: Vec<u32> = entities
        .iter()
        .filter_map(|&e| world.get::<Replicated>(e)?.net_id())
        .map(|id| id.index())
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn only_entities_that_changed_are_read_and_compared() {
    let mut s = Session::scripted(1, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 200);
    s.join(200);
    s.run(60, |_, _| {});
    let crowd: Vec<Entity> = originals(&mut s.sides[0].world).into_values().collect();
    let movers: Vec<Entity> = crowd.iter().copied().step_by(20).collect();
    let mover_ids = net_ids(&s.sides[0].world, &movers);
    assert_eq!(mover_ids.len(), 10);
    for tick in 0..30 {
        s.step(|i, side| {
            if i == 0 {
                for &e in &movers {
                    side.world.get_mut::<Pos>(e).unwrap().at.x += 1.0 + tick as f32;
                }
            }
        });
        assert_eq!(s.sides[0].rep.server_changed(), mover_ids, "tick {tick}");
    }
    // Every entity borrowed mutably, none written: nothing to send.
    s.run(10, |_, _| {});
    let peer = s.me(1);
    let before = s.sides[0].rep.peer_stats(peer).unwrap();
    for _ in 0..20 {
        s.step(|i, side| {
            if i == 0 {
                side.world.query::<&mut Pos>().for_each(|p| p.at.x += 0.0);
            }
        });
        assert!(s.sides[0].rep.server_changed().is_empty());
    }
    let after = s.sides[0].rep.peer_stats(peer).unwrap();
    assert_eq!(after.records_sent, before.records_sent);
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn a_removed_component_is_removed_from_replicas() {
    let mut s = Session::scripted(1, declarations, |n| n.loss_per_mille = 200);
    spawn_crowd(&mut s.sides[0].world, 20);
    s.join(200);
    s.run(60, |_, _| {});
    let target = *originals(&mut s.sides[0].world).values().nth(5).unwrap();
    s.step(|i, side| {
        if i == 0 {
            side.world.remove::<Hp>(target);
        }
    });
    s.run(60, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
    s.step(|i, side| {
        if i == 0 {
            side.world.insert(target, Hp(7));
        }
    });
    s.run(60, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn writes_the_world_cannot_see_arrive_within_a_sweep() {
    let mut s = Session::scripted(1, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 100);
    s.join(200);
    s.run(60, |_, _| {});
    let target = *originals(&mut s.sides[0].world).values().nth(3).unwrap();
    {
        let world = &mut s.sides[0].world;
        let id = world.component_id::<Pos>().unwrap();
        let p = world.raw().get_dynamic_mut(target, id).unwrap();
        // SAFETY: `p` points to `target`'s live `Pos`, and nothing else
        // touches the world while it is written.
        unsafe { p.cast::<Pos>().write(Pos { at: vec2(321.0, -12.5) }) };
    }
    // A sweep re-reads every entity within 64 ticks.
    s.run(64 + 30, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
    let replica = replicas(&mut s.sides[1].world)[&net_ids(&s.sides[0].world, &[target])[0]];
    assert!(close(
        s.sides[1].world.get::<Pos>(replica).unwrap().at,
        vec2(321.0, -12.5)
    ));
}

#[test]
fn a_replaced_world_is_read_in_full() {
    let mut s = Session::scripted(1, declarations, |_| {});
    spawn_crowd(&mut s.sides[0].world, 30);
    s.join(200);
    s.run(60, |_, _| {});
    s.step(|i, side| {
        if i == 0 {
            // A loaded level: a new world whose change ticks start over.
            side.world = World::new();
            spawn_crowd(&mut side.world, 12);
        }
    });
    s.run(60, |_, _| {});
    assert_eq!(replicas(&mut s.sides[1].world).len(), 12);
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn a_server_without_clients_keeps_nothing_pending() {
    let mut s = Session::scripted(1, declarations, |_| {});
    // The client's endpoint stays quiet: the server ticks alone, spawning.
    for tick in 0..200u64 {
        let server = &mut s.sides[0];
        server.tick(|side| {
            churn(&mut side.world, tick);
            if tick.is_multiple_of(3) {
                spawn_crowd(&mut side.world, 1);
            }
        });
        assert!(
            server.rep.server_changed().is_empty(),
            "nothing is read while nobody listens"
        );
        assert_eq!(server.rep.server_pending(), 0, "tick {tick}: nothing piles up");
    }
    s.join(200);
    s.run(60, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
}

/// Records the server sent client `client` so far.
fn records_to(s: &Session, client: usize) -> u64 {
    s.sides[0].rep.peer_stats(s.me(client)).expect("joined").records_sent
}

#[test]
fn a_change_is_sent_once_while_its_acknowledgement_is_on_the_way() {
    // Six ticks each way: an acknowledgement is back twelve ticks after
    // its packet left.
    let mut s = Session::scripted(1, declarations, |n| n.delay = 6);
    spawn_crowd(&mut s.sides[0].world, 20);
    s.join(400);
    s.run(120, |_, _| {});
    let rtt = s.sides[0].rep.peer_rtt_ticks(s.me(1)).expect("measured");
    assert!((11.0..=14.0).contains(&rtt), "round trip {rtt} ticks");
    let movers: Vec<Entity> = originals(&mut s.sides[0].world).into_values().take(5).collect();
    let before = records_to(&s, 1);
    // Five entities change once every 30 ticks, ten times.
    for round in 0..10 {
        s.step(|i, side| {
            if i == 0 {
                for &e in &movers {
                    side.world.get_mut::<Pos>(e).unwrap().at.x += 1.0 + round as f32;
                }
            }
        });
        s.run(29, |_, _| {});
    }
    // Sending every tick until acknowledged would have sent each change
    // about twelve times.
    assert_eq!(records_to(&s, 1) - before, 5 * 10, "each change went once");
    assert_converged(&mut s, |_, _, _| true);
}

#[test]
fn unacknowledged_changes_are_resent_after_the_timeout_not_every_tick() {
    let mut s = Session::scripted(1, declarations, |n| n.delay = 3);
    spawn_crowd(&mut s.sides[0].world, 20);
    s.join(400);
    s.run(120, |_, _| {});
    let crowd: Vec<Entity> = originals(&mut s.sides[0].world).into_values().collect();
    let peer = s.me(1);
    let before = s.sides[0].rep.peer_stats(peer).unwrap();
    // The link goes down; five entities change once and one dies.
    s.net.as_ref().unwrap().lock().unwrap().cut = true;
    s.step(|i, side| {
        if i == 0 {
            for &e in &crowd[..5] {
                side.world.get_mut::<Pos>(e).unwrap().at.y += 50.0;
            }
            side.world.despawn(crowd[19]);
        }
    });
    let ticks = 60;
    s.run(ticks - 1, |_, _| {});
    let after = s.sides[0].rep.peer_stats(peer).unwrap();
    let records = after.records_sent - before.records_sent;
    let destroys = after.destroys_sent - before.destroys_sent;
    // A round trip of six ticks: resent about every seventh tick, not
    // in each of the 60 ticks (which would be 300 records, 60 despawns).
    assert!(
        (5 * 5..=5 * 12).contains(&records),
        "{records} records in {ticks} ticks"
    );
    assert!((5..=12).contains(&destroys), "{destroys} despawns in {ticks} ticks");
    // Back up: everything arrives.
    s.net.as_ref().unwrap().lock().unwrap().cut = false;
    s.run(30, |_, _| {});
    assert_converged(&mut s, |_, _, _| true);
}

/// The bytes of raw event number `n`: its length varies (some empty, some
/// longer than a datagram), its content says which it is.
fn blob(n: u32) -> Vec<u8> {
    let len = match n % 4 {
        0 => 0,
        1 => 3,
        2 => 40,
        _ => 3000,
    };
    (0..len).map(|i| (n as usize * 31 + i) as u8).collect()
}

#[test]
fn raw_events_arrive_once_unchanged_and_in_order_with_typed_ones() {
    let rep = || declarations().raw_event("blob");
    let mut s = Session::scripted(2, rep, |n| {
        n.loss_per_mille = 300;
        n.jitter = 5;
        n.duplicate = true;
    });
    s.join(600);
    // What each side heard: typed `Shout`s and raw blobs, each type in
    // the order it arrived.
    #[derive(Debug, PartialEq)]
    enum Heard {
        Shout(u32),
        Blob(Vec<u8>),
    }
    let mut got: Vec<Vec<(PeerId, Heard)>> = (0..3).map(|_| Vec::new()).collect();
    let mut hear = |i: usize, side: &mut Side| {
        for e in side.events.read::<NetEvent<Shout>>() {
            got[i].push((e.from, Heard::Shout(e.event.0)));
        }
        for e in side.events.read::<NetEvent<RawEvent>>() {
            assert_eq!(e.event.name(), "blob");
            got[i].push((e.from, Heard::Blob(e.event.bytes.clone())));
        }
    };
    for round in 0..40u32 {
        s.step(|i, side| {
            hear(i, side);
            match i {
                0 => {
                    side.rep.to_clients(&Shout(round));
                    side.rep.to_clients_raw("blob", &blob(round)).unwrap();
                    if round == 7 {
                        side.rep.to_server_raw("blob", b"host").unwrap(); // the host itself
                    }
                }
                _ => {
                    side.rep.to_server_raw("blob", &blob(round * 10 + i as u32)).unwrap();
                }
            }
        });
    }
    s.run(200, |i, side| hear(i, side));
    for (i, heard) in got.iter().enumerate().skip(1) {
        assert!(heard.iter().all(|e| e.0 == PeerId::HOST));
        let shouts: Vec<u32> = heard
            .iter()
            .filter_map(|e| match e.1 {
                Heard::Shout(n) => Some(n),
                Heard::Blob(_) => None,
            })
            .collect();
        assert_eq!(
            shouts,
            (0..40).collect::<Vec<_>>(),
            "client {i}: typed, all, once, in order"
        );
        let blobs: Vec<&Vec<u8>> = heard
            .iter()
            .filter_map(|e| match &e.1 {
                Heard::Blob(b) => Some(b),
                Heard::Shout(_) => None,
            })
            .collect();
        let want: Vec<Vec<u8>> = (0..40).map(blob).collect();
        assert_eq!(blobs.len(), want.len(), "client {i}: every raw event, once");
        for (n, (b, w)) in blobs.iter().zip(&want).enumerate() {
            assert_eq!(*b, w, "client {i}: raw event {n} unchanged and in order");
        }
    }
    for i in 1..=2u32 {
        let from = s.me(i as usize);
        let blobs: Vec<&Vec<u8>> = got[0]
            .iter()
            .filter(|e| e.0 == from)
            .filter_map(|e| match &e.1 {
                Heard::Blob(b) => Some(b),
                Heard::Shout(_) => None,
            })
            .collect();
        let want: Vec<Vec<u8>> = (0..40).map(|r| blob(r * 10 + i)).collect();
        assert_eq!(blobs, want.iter().collect::<Vec<_>>(), "from client {i}");
    }
    let own: Vec<&(PeerId, Heard)> = got[0].iter().filter(|e| e.0 == PeerId::HOST).collect();
    assert_eq!(own, [&(PeerId::HOST, Heard::Blob(b"host".to_vec()))]);
}

#[test]
fn raw_and_typed_events_keep_one_order() {
    // A typed and a raw event sent alternately in one tick arrive in that
    // order (their payloads say where they were in it).
    let rep = || declarations().raw_event("blob");
    let mut s = Session::scripted(1, rep, |n| n.loss_per_mille = 200);
    s.join(600);
    s.step(|i, side| {
        if i == 1 {
            for n in 0..10u32 {
                side.rep.to_server(&Shout(n * 2));
                side.rep.to_server_raw("blob", &(n * 2 + 1).to_le_bytes()).unwrap();
            }
        }
    });
    let mut order: Vec<u32> = Vec::new();
    s.run(200, |i, side| {
        if i != 0 {
            return;
        }
        // Every one of them arrives in the same tick: the reliable lane
        // delivers them together, in order.
        let shouts: Vec<u32> = side
            .events
            .read::<NetEvent<Shout>>()
            .iter()
            .map(|e| e.event.0)
            .collect();
        let blobs: Vec<u32> = side
            .events
            .read::<NetEvent<RawEvent>>()
            .iter()
            .map(|e| u32::from_le_bytes(e.event.bytes[..4].try_into().unwrap()))
            .collect();
        if !shouts.is_empty() || !blobs.is_empty() {
            assert_eq!((shouts.len(), blobs.len()), (10, 10), "all in one tick");
            order.extend(shouts.iter().zip(&blobs).flat_map(|(a, b)| [*a, *b]));
        }
    });
    assert_eq!(order, (0..20).collect::<Vec<_>>());
}

#[test]
fn raw_events_that_cannot_go_are_refused() {
    let mut s = Session::scripted(1, || declarations().raw_event("blob"), |_| {});
    s.join(400);
    let errors = {
        let client = &mut s.sides[1].rep;
        [
            client.to_server_raw("nope", b"x").unwrap_err().kind(),
            client
                .to_server_raw("blob", &vec![0; MAX_RAW_EVENT_BYTES + 1])
                .unwrap_err()
                .kind(),
            client.to_client_raw(PeerId::HOST, "nope", b"x").unwrap_err().kind(),
        ]
    };
    assert_eq!(errors, [ErrorKind::NotFound, ErrorKind::Limit, ErrorKind::NotFound]);
    assert!(
        s.sides[1]
            .rep
            .to_server_raw("blob", &vec![7; MAX_RAW_EVENT_BYTES])
            .is_ok(),
        "the longest one goes"
    );
    let mut heard = 0;
    s.run(60, |i, side| {
        if i == 0 {
            heard += side.events.read::<NetEvent<RawEvent>>().len();
        }
    });
    assert_eq!(heard, 1, "only the one that was accepted arrived");
    // Replication off: nothing is declared.
    let mut off = Replication::default();
    assert_eq!(off.to_server_raw("blob", b"").unwrap_err().kind(), ErrorKind::NotFound);
    assert!(!off.is_active(), "a refused send doesn't turn replication on");
}

#[test]
#[should_panic(expected = "raw event `blob` is declared twice; declare each event once")]
fn a_raw_event_declared_twice_is_refused() {
    let _ = Replication::new().raw_event("blob").raw_event("blob");
}

#[test]
fn a_raw_event_never_matches_a_typed_one() {
    // The server reads `Shout` as bytes, the client as a Rust type: the
    // bytes would be misread, so the client is refused.
    let mut first = true;
    let mut s = Session::scripted(
        1,
        || {
            let r = if first {
                Replication::new().component::<Hp>().raw_event("Shout")
            } else {
                Replication::new().component::<Hp>().event::<Shout>()
            };
            first = false;
            r
        },
        |_| {},
    );
    s.run(100, |_, _| {});
    assert!(!s.sides[1].rep.is_joined());
    assert!(s.sides[1].rep.error().is_some_and(|e| e.contains("different")));
}
