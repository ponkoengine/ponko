//! Rollback for a whole `Game` ([`ponko::net::GameRollback`]): two peers,
//! each a headless game on its own thread, over a scripted network with
//! latency, jitter, loss and duplicates. The peers compare world hashes of
//! every confirmed tick (no desync), and each one's world at the end, once
//! every input that changed is in, has the hash an offline run with the
//! same inputs has at that tick, with and without physics.
//!
//! The game spawns and despawns entities through commands, sends events
//! read in the next tick, plays sounds, reads `ctx.random()` and the sim
//! clock: everything a rollback has to restore.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

#[cfg(feature = "physics")]
use ponko::d2::{Collider, RigidBody, Transform};
use ponko::net::GameRollback;
use ponko::prelude::*;
use ponko::reflect::world_hash;
use serde::{Deserialize, Serialize};

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Pos(Vec2);

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Vel(Vec2);

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Player {
    id: u8,
    score: u32,
}

/// A shot: flies for `ttl` ticks.
#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Bolt {
    by: u8,
    ttl: u32,
}

/// Sent when a bolt passes near a member of the crowd; read a tick later.
struct Hit {
    by: u8,
    target: Entity,
    from: Vec2,
}

/// One player's input for one tick.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Pad {
    x: i8,
    y: i8,
    fire: bool,
}

/// Counters that are not part of the rollback state (not registered for
/// snapshots), so they count every run of a tick.
#[derive(Default)]
struct Tally {
    /// Ticks a sim system ran while `is_resimulating`.
    again: u64,
    /// `ctx.audio.play` calls.
    plays: u64,
    /// Sounds the audio service reported as started.
    heard: u64,
}

const DELAY: u32 = 2;
const ACTIVE: u64 = 110;
const PASSES: u64 = 200;

/// What player `p` presses at tick `t`: a new direction every few ticks,
/// so predictions are often wrong; nothing from `ACTIVE` on.
fn pad(p: usize, t: u64) -> Pad {
    if t >= ACTIVE {
        return Pad::default();
    }
    let mut rng = Rng::new(((p as u64) << 32) | (t / 3));
    Pad {
        x: rng.below(3) as i8 - 1,
        y: rng.below(3) as i8 - 1,
        fire: rng.chance(0.5),
    }
}

/// Every player's input this tick: the session's, or offline, every input
/// real and applied `DELAY` ticks late (what the session schedules).
fn inputs(ctx: &Ctx) -> [Pad; 2] {
    if let Some(i) = ctx.net.rollback_inputs::<Pad>() {
        return [i[0], i[1]];
    }
    let t = ctx.time.sim_tick();
    let at = |p| {
        t.checked_sub(u64::from(DELAY))
            .map_or(Pad::default(), |from| pad(p, from))
    };
    [at(0), at(1)]
}

fn setup(ctx: &mut Ctx, physics: bool) {
    ctx.world.reflect::<Pos>();
    ctx.world.reflect::<Vel>();
    ctx.world.reflect::<Player>();
    ctx.world.reflect::<Bolt>();
    let beep = ctx.audio.create(8000, &[0.0; 80]);
    ctx.world.insert_resource(beep);
    ctx.world.insert_resource(Tally::default());
    for id in 0..2u8 {
        let at = vec2(f32::from(id) * 60.0 - 30.0, 0.0);
        let player = ctx.world.spawn((Player { id, score: 0 }, Pos(at), Vel(Vec2::ZERO)));
        #[cfg(feature = "physics")]
        if physics {
            ctx.world.insert(player, Transform::at(at));
            ctx.world.insert(player, RigidBody::dynamic());
            ctx.world.insert(player, Collider::circle(8.0));
        }
        let _ = (player, physics);
    }
    let mut rng = Rng::new(7);
    for _ in 0..60 {
        let at = vec2(rng.range(-120.0, 120.0), rng.range(-120.0, 120.0));
        let e = ctx.world.spawn((Pos(at), Vel(Vec2::ZERO)));
        #[cfg(feature = "physics")]
        if physics {
            ctx.world.insert(e, Transform::at(at));
            ctx.world.insert(e, RigidBody::dynamic());
            ctx.world.insert(e, Collider::circle(5.0));
        }
        let _ = (e, physics);
    }
}

/// Players steer (with a gust from `ctx.random()`) and fire bolts every
/// fifth tick, spawned through commands.
fn steer(ctx: &mut Ctx) {
    let pads = inputs(ctx);
    let mut gust = ctx.random().stream(0);
    let gust = vec2(gust.range(-5.0, 5.0), gust.range(-5.0, 5.0));
    let tick = ctx.time.sim_tick();
    #[cfg(feature = "physics")]
    let physics = ctx.world.query::<&RigidBody>().count() > 0;
    #[cfg(not(feature = "physics"))]
    let physics = false;
    let mut shots = Vec::new();
    let mut pushes = Vec::new();
    ctx.world
        .query::<(Entity, &Player, &mut Pos, &mut Vel)>()
        .for_each(|(e, player, pos, vel)| {
            let pad = pads[usize::from(player.id)];
            let push = vec2(f32::from(pad.x), f32::from(pad.y)) * 600.0 + gust;
            if physics {
                pushes.push((e, push));
            } else {
                vel.0 = (vel.0 + push / 60.0) * 0.9;
                pos.0 += vel.0 / 60.0;
            }
            if pad.fire && tick.is_multiple_of(5) {
                shots.push((player.id, pos.0, vel.0.normalize_or(Vec2::X) * 240.0));
            }
        });
    #[cfg(feature = "physics")]
    for (e, push) in pushes {
        let mass = ctx.physics.mass(e).unwrap_or(1.0);
        ctx.physics.apply_impulse(e, push / 60.0 * mass);
    }
    #[cfg(not(feature = "physics"))]
    let _ = pushes;
    for (by, at, v) in shots {
        ctx.commands.spawn((Bolt { by, ttl: 30 }, Pos(at), Vel(v)));
    }
}

/// Reads last tick's hits: knocks the target back, scores and beeps.
fn hits(ctx: &mut Ctx) {
    let beep = *ctx.world.resource::<Sound>();
    let mut scored = [0u32; 2];
    let mut knocks = Vec::new();
    for hit in ctx.events.read::<Hit>() {
        scored[usize::from(hit.by)] += 1;
        knocks.push((hit.target, hit.from));
    }
    let mut plays = 0;
    for (target, from) in knocks {
        if let Some(pos) = ctx.world.get::<Pos>(target).copied()
            && let Some(vel) = ctx.world.get_mut::<Vel>(target)
        {
            vel.0 += (pos.0 - from).normalize_or(Vec2::Y) * 30.0;
        }
        ctx.audio.play(beep);
        plays += 1;
    }
    ctx.world
        .query::<&mut Player>()
        .for_each(|p| p.score += scored[usize::from(p.id)]);
    let again = ctx.net.is_resimulating();
    let heard = ctx.audio.started().len() as u64;
    let tally = ctx.world.resource_mut::<Tally>();
    tally.plays += plays;
    tally.heard += heard;
    tally.again += u64::from(again);
}

/// Bolts fly, age and vanish; one near a member of the crowd sends a hit.
fn bolts(ctx: &mut Ctx) {
    let crowd: Vec<(Entity, Vec2)> = ctx
        .world
        .query::<(Entity, &Pos)>()
        .filter::<(Without<Bolt>, Without<Player>)>()
        .iter()
        .map(|(e, p)| (e, p.0))
        .collect();
    let mut gone = Vec::new();
    let mut sent = Vec::new();
    ctx.world
        .query::<(Entity, &mut Bolt, &mut Pos, &Vel)>()
        .for_each(|(e, bolt, pos, vel)| {
            pos.0 += vel.0 / 60.0;
            bolt.ttl -= 1;
            if bolt.ttl == 0 {
                gone.push(e);
            }
            if let Some(&(target, _)) = crowd.iter().find(|c| c.1.distance_squared(pos.0) < 100.0) {
                sent.push(Hit {
                    by: bolt.by,
                    target,
                    from: pos.0,
                });
            }
        });
    for e in gone {
        ctx.commands.despawn(e);
    }
    for hit in sent {
        ctx.events.send(hit);
    }
}

/// The crowd drifts and slows down; with physics, positions follow bodies.
fn drift(ctx: &mut Ctx) {
    #[cfg(feature = "physics")]
    if ctx.world.query::<&RigidBody>().count() > 0 {
        ctx.world
            .query::<(&mut Pos, &Transform)>()
            .for_each(|(p, t)| p.0 = t.position);
        return;
    }
    ctx.world
        .query::<(&mut Pos, &mut Vel)>()
        .filter::<(Without<Bolt>, Without<Player>)>()
        .for_each(|(p, v)| {
            v.0 *= 0.95;
            p.0 += v.0 / 60.0;
        });
}

fn game(name: &str, physics: bool) -> Game {
    let game = Game::new(name, 1, 1).headless_frames(true);
    #[cfg(feature = "physics")]
    let game = if physics { game.physics(Vec2::ZERO) } else { game };
    #[cfg(not(feature = "physics"))]
    assert!(!physics, "needs the physics feature");
    // `hits` before `bolts`: hits sent in one tick are read in the next.
    game.sim_system(steer)
        .sim_system(hits)
        .sim_system(bolts)
        .sim_system(drift)
}

/// The world hash at the start of every tick (before it runs) of an
/// offline run.
fn offline(physics: bool, ticks: u64) -> BTreeMap<u64, u64> {
    let hashes = Arc::new(Mutex::new(BTreeMap::new()));
    let seen = Arc::clone(&hashes);
    game("offline", physics)
        .system(move |ctx: &mut Ctx| {
            seen.lock().unwrap().insert(ctx.time.sim_tick(), world_hash(&ctx.world));
        })
        .run_headless(ticks, move |ctx| setup(ctx, physics));
    Arc::try_unwrap(hashes).unwrap().into_inner().unwrap()
}

/// Both threads wait here once a frame, so the scripted network's clock
/// (frames) is the same on both. Panics instead of hanging when the other
/// thread is gone.
struct Gate {
    state: Mutex<(u32, u64)>,
    turn: Condvar,
}

impl Gate {
    fn wait(&self, parties: u32) {
        let mut s = self.state.lock().unwrap();
        let generation = s.1;
        s.0 += 1;
        if s.0 == parties {
            *s = (0, generation + 1);
            self.turn.notify_all();
            return;
        }
        while s.1 == generation {
            let (next, timeout) = self.turn.wait_timeout(s, Duration::from_secs(30)).unwrap();
            s = next;
            assert!(!timeout.timed_out() || s.1 != generation, "the other peer stopped");
        }
    }
}

struct Packet {
    to: usize,
    due: u64,
    from: usize,
    seq: u64,
    bytes: Vec<u8>,
}

/// The scripted network both peers share.
struct Wire {
    gate: Gate,
    packets: Mutex<Vec<Packet>>,
    latency: u64,
    jitter: u32,
    loss: f32,
    duplicate: f32,
}

struct Peer {
    /// World hashes at the start of ticks that every input was in for.
    /// World hashes at the start of ticks that every input was in for.
    confirmed: BTreeMap<u64, u64>,
    /// The last frame's tick, world hash and confirmed tick (before that
    /// frame's packets were taken).
    last: (u64, u64, Option<u64>),
    rollbacks: u64,
    resimulated: u64,
    stalls: u64,
    tally: Tally,
    desync: bool,
    /// How far the players got from where they started.
    travelled: f32,
}

fn peer(me: usize, wire: Arc<Wire>, physics: bool) -> Peer {
    let confirmed = Arc::new(Mutex::new(BTreeMap::new()));
    let seen = Arc::clone(&confirmed);
    let last = Arc::new(Mutex::new((0, 0, None)));
    let at_end = Arc::clone(&last);
    let stats = Arc::new(Mutex::new((0, 0, 0, false)));
    let out = Arc::clone(&stats);
    let mut frame = 0u64;
    let mut seq = 0u64;
    let mut rng = Rng::new(100 + me as u64);
    let mut world = game("peer", physics)
        .system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let rb = ctx.net.rollback::<Pad>().expect("the session runs");
            // Inputs received last frame were applied by this frame's tick.
            let hash = world_hash(&ctx.world);
            *at_end.lock().unwrap() = (tick, hash, rb.confirmed_frame());
            if rb.confirmed_frame().is_some_and(|c| c + 1 >= tick) {
                seen.lock().unwrap().insert(tick, hash);
            }
            rb.set_input(pad(me, tick));
            let bytes = rb.packet().to_vec();
            {
                let mut packets = wire.packets.lock().unwrap();
                if !rng.chance(wire.loss) {
                    let copies = if rng.chance(wire.duplicate) { 2 } else { 1 };
                    for _ in 0..copies {
                        seq += 1;
                        packets.push(Packet {
                            to: 1 - me,
                            due: frame + wire.latency + u64::from(rng.below(wire.jitter + 1)),
                            from: me,
                            seq,
                            bytes: bytes.clone(),
                        });
                    }
                }
            }
            wire.gate.wait(2);
            frame += 1;
            // Packets due now were all sent at least `latency` frames ago.
            let mut due: Vec<Packet> = Vec::new();
            {
                let mut packets = wire.packets.lock().unwrap();
                let mut i = 0;
                while i < packets.len() {
                    if packets[i].to == me && packets[i].due <= frame {
                        due.push(packets.swap_remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            due.sort_by_key(|p| (p.due, p.from, p.seq));
            for p in due {
                assert!(rb.receive(&p.bytes), "every scripted packet is valid");
            }
            let s = rb.stats();
            *out.lock().unwrap() = (s.rollbacks, s.resimulated, s.stalls, rb.desync().is_some());
        })
        .run_headless(PASSES, move |ctx| {
            setup(ctx, physics);
            let mut session = GameRollback::<Pad>::new(2, me)
                .input_delay(DELAY)
                .max_prediction(8)
                .checksum_every(1);
            // The input for the first tick; frame systems give the rest.
            session.set_input(pad(me, 0));
            ctx.net.start_rollback(session);
        });
    let (rollbacks, resimulated, stalls, desync) = *stats.lock().unwrap();
    Peer {
        confirmed: Arc::try_unwrap(confirmed).unwrap().into_inner().unwrap(),
        last: *last.lock().unwrap(),
        rollbacks,
        resimulated,
        stalls,
        tally: world.remove_resource::<Tally>().unwrap(),
        travelled: world
            .query::<(&Player, &Pos)>()
            .iter()
            .map(|(p, pos)| pos.0.distance(vec2(f32::from(p.id) * 60.0 - 30.0, 0.0)))
            .sum(),
        desync,
    }
}

fn two_peers(physics: bool, latency: u64, jitter: u32, loss: f32) -> [Peer; 2] {
    let wire = Arc::new(Wire {
        gate: Gate {
            state: Mutex::new((0, 0)),
            turn: Condvar::new(),
        },
        packets: Mutex::new(Vec::new()),
        latency,
        jitter,
        loss,
        duplicate: 0.05,
    });
    let threads: Vec<_> = (0..2)
        .map(|me| {
            let wire = Arc::clone(&wire);
            std::thread::spawn(move || peer(me, wire, physics))
        })
        .collect();
    let mut peers = threads.into_iter().map(|t| t.join().expect("peer thread"));
    [peers.next().unwrap(), peers.next().unwrap()]
}

/// No desync between the peers, and each one's last world, with every
/// input that changed in, is the offline run's at that tick (as is every
/// hash taken at a tick that was confirmed then).
fn check(peers: &[Peer; 2], physics: bool) {
    let end = peers.iter().map(|p| p.last.0).max().unwrap();
    let reference = offline(physics, end + 1);
    for (i, peer) in peers.iter().enumerate() {
        assert!(!peer.desync, "peer {i}: the checksums of every confirmed tick agree");
        assert!(peer.rollbacks > 0, "peer {i} rolled back (inputs change often)");
        assert!(
            peer.travelled > 20.0,
            "peer {i}: the players moved ({})",
            peer.travelled
        );
        let (tick, hash, confirmed) = peer.last;
        assert!(
            confirmed.is_some_and(|c| c > ACTIVE + u64::from(DELAY)),
            "peer {i}: every input that changed is in by the end ({confirmed:?})"
        );
        assert_eq!(
            Some(&hash),
            reference.get(&tick),
            "peer {i}: the offline world at tick {tick}"
        );
        for (tick, hash) in &peer.confirmed {
            assert_eq!(
                Some(hash),
                reference.get(tick),
                "peer {i}, tick {tick}: the offline world"
            );
        }
        // Each tick that ran again ran every sim system once more, and its
        // sounds weren't heard again.
        assert_eq!(peer.tally.again, peer.resimulated, "peer {i}: ticks run again");
        assert!(
            peer.tally.heard < peer.tally.plays,
            "peer {i}: sounds of ticks run again are dropped"
        );
    }
}

#[test]
fn two_peers_over_a_rough_link_reach_the_offline_world() {
    let peers = two_peers(false, 3, 4, 0.1);
    check(&peers, false);
}

#[cfg(feature = "physics")]
#[test]
fn two_peers_with_physics_over_a_rough_link_reach_the_offline_world() {
    let peers = two_peers(true, 3, 4, 0.1);
    check(&peers, true);
}

#[test]
fn a_slow_link_stalls_and_still_reaches_the_offline_world() {
    let peers = two_peers(false, 12, 2, 0.0);
    assert!(
        peers.iter().all(|p| p.stalls > 0),
        "12 frames of latency outrun 8 ticks of prediction"
    );
    check(&peers, false);
}

#[test]
#[should_panic(expected = "start and stop sessions in setup or frame systems")]
fn starting_a_session_in_a_tick_the_session_runs_panics() {
    Game::new("t", 1, 1)
        .sim_system(|ctx: &mut Ctx| ctx.net.start_rollback(GameRollback::<u8>::new(2, 0)))
        .run_headless(2, |ctx| ctx.net.start_rollback(GameRollback::<u8>::new(2, 0)));
}

/// Not `Clone`: a snapshot can't copy it.
#[derive(Component)]
struct Opaque;

#[test]
#[should_panic(expected = "rollback: component")]
fn a_component_that_cant_be_copied_stops_the_first_tick() {
    Game::new("t", 1, 1).run_headless(1, |ctx| {
        ctx.world.spawn((Opaque,));
        ctx.net.start_rollback(GameRollback::<u8>::new(2, 0));
    });
}

#[test]
fn stopping_the_session_gives_it_back_and_ticks_run_as_before() {
    let world = Game::new("t", 1, 1)
        .headless_frames(true)
        .sim_system(|ctx: &mut Ctx| {
            let n = ctx.net.rollback_inputs::<u8>().map_or(0, <[u8]>::len);
            ctx.world.insert_resource(n);
        })
        .system(|ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 3 {
                let session = ctx.net.stop_rollback::<u8>().expect("the session of u8 inputs");
                assert_eq!(session.frame(), 3);
                assert!(ctx.net.rollback::<u8>().is_none());
            }
        })
        .run_headless(5, |ctx| ctx.net.start_rollback(GameRollback::<u8>::new(2, 0)));
    assert_eq!(world.get_resource::<usize>(), Some(&0), "no inputs once it stopped");
}
