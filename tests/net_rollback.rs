//! Rollback netcode ([`ponko::net::Rollback`]) over a scripted network:
//! peers with latency, jitter, loss and duplication end in the same world,
//! and that world is the one an offline run with the same inputs reaches.
//!
//! The game is an ECS world: players driven by inputs push a crowd around.
//! Its rollback state is a typed copy of the components that change (see
//! the guide's "Rollback" section), not a reflected save.

use std::hash::{DefaultHasher, Hash, Hasher};

use ponko::net::{Advance, Rollback, RollbackGame};
use ponko::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Pos(Vec2);

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Vel(Vec2);

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Player(u8);

/// One player's input for one tick: a direction on each axis, -1 to 1.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Pad(i8, i8);

/// The game: an ECS world, and the tick it is at (part of the state).
struct Arena {
    world: World,
    tick: u64,
}

/// What a rollback saves: every (entity, position, velocity) and the tick.
#[derive(Default)]
struct Saved {
    tick: u64,
    bodies: Vec<(Entity, Pos, Vel)>,
}

impl Arena {
    fn new(players: u8, crowd: usize) -> Self {
        let mut world = World::new();
        world.reflect::<Pos>();
        world.reflect::<Vel>();
        world.reflect::<Player>();
        for p in 0..players {
            world.spawn((Pos(vec2(f32::from(p) * 40.0, 0.0)), Vel(Vec2::ZERO), Player(p)));
        }
        let mut rng = Rng::new(3);
        world.spawn_batch((0..crowd).map(|_| {
            (
                Pos(vec2(rng.range(-100.0, 100.0), rng.range(-100.0, 100.0))),
                Vel(Vec2::ZERO),
            )
        }));
        Self { world, tick: 0 }
    }

    /// A hash of everything reflected: the "world hash" peers compare.
    fn hash(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.world.save().to_string().hash(&mut h);
        self.tick.hash(&mut h);
        h.finish()
    }
}

impl RollbackGame<Pad> for Arena {
    type State = Saved;

    fn save(&mut self, into: &mut Saved) {
        into.tick = self.tick;
        into.bodies.clear();
        into.bodies.extend(
            self.world
                .query::<(Entity, &Pos, &Vel)>()
                .iter()
                .map(|(e, p, v)| (e, *p, *v)),
        );
    }

    fn load(&mut self, from: &Saved) {
        self.tick = from.tick;
        for &(e, p, v) in &from.bodies {
            *self.world.get_mut::<Pos>(e).expect("the crowd never changes") = p;
            *self.world.get_mut::<Vel>(e).expect("the crowd never changes") = v;
        }
    }

    fn advance(&mut self, inputs: &[Pad]) {
        let dt = 1.0 / 60.0;
        // Players steer, with a random gust per tick (seeded by the tick:
        // the same on every peer, and again on every re-simulation).
        let mut gust = Rng::new(self.tick);
        let gust = vec2(gust.range(-5.0, 5.0), gust.range(-5.0, 5.0));
        let mut pushers = Vec::new();
        self.world
            .query::<(&mut Pos, &mut Vel, &Player)>()
            .for_each(|(p, v, who)| {
                let Pad(x, y) = inputs[usize::from(who.0)];
                v.0 += (vec2(f32::from(x), f32::from(y)) * 600.0 + gust) * dt;
                v.0 *= 0.9;
                p.0 += v.0 * dt;
                pushers.push(p.0);
            });
        // The crowd is pushed away from players and slows down.
        self.world
            .query::<(&mut Pos, &mut Vel)>()
            .filter::<Without<Player>>()
            .for_each(|(p, v)| {
                for &at in &pushers {
                    let d = p.0 - at;
                    let r2 = d.length_squared();
                    if r2 < 400.0 && r2 > 1e-6 {
                        v.0 += d / r2.sqrt() * 50.0;
                    }
                }
                v.0 *= 0.95;
                p.0 += v.0 * dt;
            });
        self.tick += 1;
    }

    fn checksum(state: &Saved) -> Option<u64> {
        let mut h = DefaultHasher::new();
        state.tick.hash(&mut h);
        for (e, p, v) in &state.bodies {
            e.to_bits().hash(&mut h);
            for f in [p.0.x, p.0.y, v.0.x, v.0.y] {
                f.to_bits().hash(&mut h);
            }
        }
        Some(h.finish())
    }
}

/// What player `p` presses at tick `t`: a new direction every few ticks,
/// so predictions are often wrong.
fn pad(p: usize, t: u64, active: u64) -> Pad {
    if t >= active {
        return Pad::default();
    }
    let mut rng = Rng::new(((p as u64) << 32) | (t / 4));
    Pad(rng.below(3) as i8 - 1, rng.below(3) as i8 - 1)
}

/// How the scripted network treats packets.
#[derive(Clone, Copy)]
struct Link {
    /// Ticks every packet takes at least.
    latency: u64,
    /// Up to this many more ticks, at random per packet (so packets reorder).
    jitter: u64,
    /// The chance a packet is lost.
    loss: f32,
    /// The chance a packet arrives twice.
    duplicate: f32,
}

struct InFlight {
    to: usize,
    due: u64,
    bytes: Vec<u8>,
}

struct Outcome {
    hashes: Vec<u64>,
    reference: u64,
    rollbacks: Vec<u64>,
    deepest: u32,
    stalls: u64,
    lost: u64,
}

/// `players` peers run `ticks` ticks each (inputs for the first `active`,
/// then none) over `link`, then the offline reference with the same inputs.
fn play(players: usize, ticks: u64, active: u64, link: Link, delay: u32) -> Outcome {
    let mut peers: Vec<Rollback<Pad, Saved>> = (0..players)
        .map(|p| {
            Rollback::new(players, p)
                .input_delay(delay)
                .max_prediction(8)
                .checksum_every(1)
        })
        .collect();
    let mut games: Vec<Arena> = (0..players).map(|_| Arena::new(players as u8, 200)).collect();
    let mut rng = Rng::new(99);
    let mut wire: Vec<InFlight> = Vec::new();
    let mut lost = 0;
    let mut now = 0u64;
    // Each peer ticks once per network tick until it ran `ticks` ticks;
    // packets keep flowing until everyone is done.
    while peers.iter().any(|p| p.frame() < ticks) {
        assert!(now < ticks * 4, "the session made no progress");
        for p in 0..players {
            if peers[p].frame() < ticks {
                let t = peers[p].frame();
                let _ = peers[p].advance(&mut games[p], pad(p, t, active));
            }
            let bytes = peers[p].packet().to_vec();
            for to in (0..players).filter(|&to| to != p) {
                if rng.chance(link.loss) {
                    lost += 1;
                    continue;
                }
                let copies = if rng.chance(link.duplicate) { 2 } else { 1 };
                for _ in 0..copies {
                    let due = now + link.latency + u64::from(rng.below(link.jitter as u32 + 1));
                    wire.push(InFlight {
                        to,
                        due,
                        bytes: bytes.clone(),
                    });
                }
            }
        }
        now += 1;
        // Deliver what is due, in a jittered order.
        let mut due: Vec<InFlight> = Vec::new();
        wire.retain_mut(|f| {
            if f.due <= now {
                due.push(InFlight {
                    to: f.to,
                    due: f.due,
                    bytes: std::mem::take(&mut f.bytes),
                });
                false
            } else {
                true
            }
        });
        due.sort_by_key(|f| (f.due, f.to));
        for f in due {
            assert!(peers[f.to].receive(&f.bytes), "every scripted packet is valid");
        }
    }
    for (p, peer) in peers.iter().enumerate() {
        assert_eq!(peer.desync(), None, "peer {p}: checksums of confirmed ticks agree");
    }

    // The offline reference: every input real, applied `delay` ticks late.
    let mut reference = Arena::new(players as u8, 200);
    let mut inputs = vec![Pad::default(); players];
    for t in 0..ticks {
        for (p, input) in inputs.iter_mut().enumerate() {
            *input = t
                .checked_sub(u64::from(delay))
                .map_or(Pad::default(), |from| pad(p, from, active));
        }
        reference.advance(&inputs);
    }
    Outcome {
        hashes: games.iter().map(Arena::hash).collect(),
        reference: reference.hash(),
        rollbacks: peers.iter().map(|p| p.stats().rollbacks).collect(),
        deepest: peers.iter().map(|p| p.stats().deepest).max().unwrap_or(0),
        stalls: peers.iter().map(|p| p.stats().stalls).sum(),
        lost,
    }
}

const ROUGH: Link = Link {
    latency: 4,
    jitter: 4,
    loss: 0.1,
    duplicate: 0.05,
};

#[test]
fn two_peers_with_latency_jitter_and_loss_end_in_the_same_world_as_offline() {
    // 600 ticks of inputs, then 60 of none, so the last predictions settle.
    let out = play(2, 660, 600, ROUGH, 2);
    assert_eq!(out.hashes[0], out.hashes[1], "both peers end in the same world");
    assert_eq!(out.hashes[0], out.reference, "and it is the offline world");
    // The script really exercised the rollback path.
    assert!(out.lost > 50, "packets were lost: {}", out.lost);
    assert!(
        out.rollbacks.iter().all(|&r| r > 50),
        "both peers rolled back: {:?}",
        out.rollbacks
    );
    assert!(out.deepest >= 3, "rollbacks went several ticks deep: {}", out.deepest);
    assert!(out.deepest <= 8, "never deeper than max_prediction: {}", out.deepest);
}

#[test]
fn zero_input_delay_and_a_slow_link_still_converge_through_stalls() {
    // 7 to 12 ticks one way (a 230 to 400 ms round trip at 60 Hz) is past
    // the 8-tick prediction window: peers stall and wait, and still agree.
    let slow = Link {
        latency: 7,
        jitter: 5,
        loss: 0.2,
        duplicate: 0.0,
    };
    let out = play(2, 300, 240, slow, 0);
    assert_eq!(out.hashes[0], out.hashes[1]);
    assert_eq!(out.hashes[0], out.reference);
    assert!(out.stalls > 0, "a link this slow stalls the session");
}

#[test]
fn four_peers_in_a_mesh_end_in_the_same_world() {
    let out = play(4, 400, 340, ROUGH, 2);
    for h in &out.hashes {
        assert_eq!(*h, out.reference);
    }
}

#[test]
fn a_perfect_link_with_enough_input_delay_never_rolls_back() {
    let perfect = Link {
        latency: 1,
        jitter: 0,
        loss: 0.0,
        duplicate: 0.0,
    };
    // A packet leaves after the tick and is read before the next one: two
    // ticks of delay cover it, so every input is in before its tick runs.
    let out = play(2, 200, 200, perfect, 2);
    assert_eq!(out.rollbacks, [0, 0]);
    assert_eq!(out.hashes[0], out.reference);
}

/// A tiny game whose state is the tick count and a sum of everyone's
/// inputs, for the session's own rules.
#[derive(Clone, Default, PartialEq, Debug)]
struct Sum {
    tick: u64,
    total: i64,
}

impl RollbackGame<i8> for Sum {
    type State = Sum;
    fn save(&mut self, into: &mut Sum) {
        into.clone_from(self);
    }
    fn load(&mut self, from: &Sum) {
        self.clone_from(from);
    }
    fn advance(&mut self, inputs: &[i8]) {
        self.tick += 1;
        self.total = self.total * 3 + inputs.iter().map(|&i| i64::from(i)).sum::<i64>();
        self.total %= 1_000_003;
    }
    fn checksum(state: &Sum) -> Option<u64> {
        Some(state.total as u64 ^ state.tick << 40)
    }
}

#[test]
fn a_peer_without_packets_stalls_after_max_prediction_ticks() {
    let mut rb: Rollback<i8, Sum> = Rollback::new(2, 0).input_delay(0).max_prediction(5);
    let mut game = Sum::default();
    let ran: Vec<bool> = (0..8).map(|_| rb.advance(&mut game, 1).ran()).collect();
    assert_eq!(ran, [true, true, true, true, true, false, false, false]);
    assert_eq!(rb.frame(), 5);
    assert_eq!(rb.stats().stalls, 3);
    assert_eq!(rb.confirmed_frame(), None);
}

#[test]
fn foreign_and_mismatched_packets_are_rejected_and_change_nothing() {
    let mut a: Rollback<i8, Sum> = Rollback::new(2, 0);
    let mut b: Rollback<i8, Sum> = Rollback::new(2, 1);
    let mut other_delay: Rollback<i8, Sum> = Rollback::new(2, 1).input_delay(5);
    let mut three: Rollback<i8, Sum> = Rollback::new(3, 1);
    let mut game = Sum::default();
    b.advance(&mut game, 1);
    let good = b.packet().to_vec();
    let own = a.packet().to_vec();
    for bad in [
        &b"hello"[..],
        &[][..],
        &good[..good.len() - 1],
        &own[..],
        other_delay.packet(),
        three.packet(),
    ] {
        assert!(!a.receive(bad));
    }
    assert_eq!(a.stats().rejected, 6);
    assert_eq!(a.confirmed_frame(), Some(1), "only the default ticks are in");
    assert!(a.receive(&good));
    assert!(a.receive(&good), "a duplicate is fine");
    assert_eq!(a.stats().rejected, 6);
}

/// A game that cheats: player 1's peer adds one more to its total.
struct Drift(Sum, bool);
impl RollbackGame<i8> for Drift {
    type State = Sum;
    fn save(&mut self, into: &mut Sum) {
        self.0.save(into);
    }
    fn load(&mut self, from: &Sum) {
        self.0.load(from);
    }
    fn advance(&mut self, inputs: &[i8]) {
        self.0.advance(inputs);
        if self.1 && self.0.tick == 30 {
            self.0.total += 1;
        }
    }
    fn checksum(state: &Sum) -> Option<u64> {
        Sum::checksum(state)
    }
}

#[test]
fn states_that_differ_are_reported_as_a_desync_on_both_peers() {
    let mut peers: Vec<Rollback<i8, Sum>> = (0..2).map(|p| Rollback::new(2, p).checksum_every(10)).collect();
    let mut games = [Drift(Sum::default(), false), Drift(Sum::default(), true)];
    for _ in 0..80 {
        let packets: Vec<Vec<u8>> = (0..2)
            .map(|p| {
                peers[p].advance(&mut games[p], 1);
                peers[p].packet().to_vec()
            })
            .collect();
        assert!(peers[0].receive(&packets[1]));
        assert!(peers[1].receive(&packets[0]));
    }
    // The drift happens in tick 29 (the 30th), so the state saved before
    // tick 30, which is checked, is the first to differ.
    let d0 = peers[0].desync().expect("peer 0 sees it");
    let d1 = peers[1].desync().expect("peer 1 sees it");
    assert_eq!((d0.frame, d0.player), (30, 1));
    assert_eq!((d1.frame, d1.player), (30, 0));
}

#[test]
fn packets_fit_the_limit_with_the_largest_inputs() {
    #[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
    struct Big(String);
    #[derive(Default)]
    struct Nothing;
    impl RollbackGame<Big> for Nothing {
        type State = ();
        fn save(&mut self, _: &mut ()) {}
        fn load(&mut self, _: &()) {}
        fn advance(&mut self, _: &[Big]) {}
    }
    let mut a: Rollback<Big, ()> = Rollback::new(2, 0).max_prediction(32);
    let mut b: Rollback<Big, ()> = Rollback::new(2, 1).max_prediction(32);
    let (mut ga, mut gb) = (Nothing, Nothing);
    // 1,000-character inputs, a dozen ticks unacknowledged: one per packet.
    for _ in 0..12 {
        a.advance(&mut ga, Big("x".repeat(1000)));
        assert!(a.packet().len() <= 1400);
    }
    for _ in 0..20 {
        let p = a.packet().to_vec();
        assert!(p.len() <= 1400);
        assert!(b.receive(&p));
        b.advance(&mut gb, Big::default());
        let back = b.packet().to_vec();
        assert!(a.receive(&back));
    }
    assert!(
        b.confirmed_frame().unwrap() >= 12,
        "every input got through, a few per packet"
    );
}

#[test]
fn a_world_load_does_not_restore_which_ids_new_spawns_get() {
    // Why the rollback state above is a typed copy of a fixed set of
    // entities and not `World::save`/`load`: loading resets the entity
    // allocator, so an entity spawned after a load can get another id than
    // the same spawn in a world that never loaded. Peers that rolled back a
    // different number of times would then disagree on ids.
    let mut world = World::new();
    world.reflect::<Pos>();
    let a = world.spawn((Pos(Vec2::ZERO),));
    let b = world.spawn((Pos(Vec2::ONE),));
    let _c = world.spawn((Pos(Vec2::X),));
    world.despawn(a);
    world.despawn(b);
    let save = world.save();
    let without_load = world.spawn((Pos(Vec2::Y),));

    let mut loaded = World::new();
    loaded.reflect::<Pos>();
    loaded.load(&save).unwrap();
    let after_load = loaded.spawn((Pos(Vec2::Y),));
    assert_ne!(
        without_load, after_load,
        "the free list and generations are not part of a save"
    );
}

#[test]
fn rollback_advance_reports_what_it_did() {
    let mut a: Rollback<i8, Sum> = Rollback::new(2, 0).input_delay(0);
    let mut b: Rollback<i8, Sum> = Rollback::new(2, 1).input_delay(0);
    let (mut ga, mut gb) = (Sum::default(), Sum::default());
    // b presses 1 from the start; a predicts "nothing" for three ticks.
    for _ in 0..3 {
        assert_eq!(a.advance(&mut ga, 0), Advance::Ran { resimulated: 0 });
        b.advance(&mut gb, 1);
    }
    assert!(a.receive(b.packet()));
    assert_eq!(a.advance(&mut ga, 0), Advance::Ran { resimulated: 3 });
    assert_eq!(a.stats().rollbacks, 1);
    assert_eq!(a.stats().deepest, 3);
}

#[test]
#[should_panic(expected = "2 to 8 players")]
fn a_session_needs_two_to_eight_players() {
    let _ = Rollback::<i8, Sum>::new(9, 0);
}

#[test]
#[should_panic(expected = "input delay is at most 16 ticks")]
fn input_delay_is_at_most_sixteen_ticks() {
    let _ = Rollback::<i8, Sum>::new(2, 0).input_delay(17);
}

#[test]
#[should_panic(expected = "max prediction is 1 to 32 ticks")]
fn the_prediction_window_is_one_to_thirty_two_ticks() {
    let _ = Rollback::<i8, Sum>::new(2, 0).max_prediction(0);
}

#[test]
#[should_panic(expected = "input is over 1024 bytes")]
fn an_input_over_the_limit_panics_when_it_is_sent() {
    #[derive(Default)]
    struct Nothing;
    impl RollbackGame<String> for Nothing {
        type State = ();
        fn save(&mut self, _: &mut ()) {}
        fn load(&mut self, _: &()) {}
        fn advance(&mut self, _: &[String]) {}
    }
    let mut rb: Rollback<String, ()> = Rollback::new(2, 0).input_delay(0);
    rb.advance(&mut Nothing, "x".repeat(2000));
    let _ = rb.packet();
}
