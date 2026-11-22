//! Rollback for a whole `Game` with 3D physics: two peers, each a headless
//! game on its own thread, whose packets arrive late and out of order, and
//! one stretch where a peer's packets are held back long enough to force a
//! rollback of several ticks. The peers compare world hashes every tick (no
//! desync), and both end on the world an offline run with the same inputs
//! has at that tick.
#![cfg(feature = "physics3d")]

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ponko::d3::{Collider, RigidBody, Transform};
use ponko::net::GameRollback;
use ponko::prelude::*;
use ponko::reflect::world_hash;
use serde::{Deserialize, Serialize};

/// A copy of the body's position, so world hashes see the physics.
#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Pos(Vec3);

#[derive(Component, Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
struct Player(u8);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
struct Pad {
    x: i8,
    z: i8,
    jump: bool,
}

const DELAY: u32 = 2;
const ACTIVE: u64 = 100;
const PASSES: u64 = 180;
/// Frames in which peer 1's packets are held back, then all arrive at once.
const HELD: std::ops::Range<u64> = 40..47;

fn pad(p: usize, t: u64) -> Pad {
    if t >= ACTIVE {
        return Pad::default();
    }
    let mut rng = Rng::new(((p as u64) << 32) | (t / 4));
    Pad {
        x: rng.below(3) as i8 - 1,
        z: rng.below(3) as i8 - 1,
        jump: rng.chance(0.2),
    }
}

fn inputs(ctx: &Ctx) -> [Pad; 2] {
    if let Some(i) = ctx.net.rollback_inputs::<Pad>() {
        return [i[0], i[1]];
    }
    let t = ctx.time.sim_tick();
    let at = |p| t.checked_sub(u64::from(DELAY)).map_or(Pad::default(), |from| pad(p, from));
    [at(0), at(1)]
}

fn setup(ctx: &mut Ctx) {
    ctx.world.reflect::<Pos>();
    ctx.world.reflect::<Player>();
    ctx.world.spawn((
        Transform::at(vec3(0.0, -0.5, 0.0)),
        RigidBody::fixed(),
        Collider::cuboid(vec3(30.0, 1.0, 30.0)),
    ));
    for id in 0..2u8 {
        let at = vec3(f32::from(id) * 4.0 - 2.0, 0.5, 0.0);
        ctx.world.spawn((
            Player(id),
            Pos(at),
            Transform::at(at),
            RigidBody::dynamic(),
            Collider::sphere(0.5),
        ));
    }
    // A pile of boxes the players run into.
    for i in 0..18 {
        let at = vec3((i % 3) as f32 * 0.7 - 0.7, 0.5 + (i / 3) as f32 * 1.05, 3.0);
        ctx.world.spawn((Pos(at), Transform::at(at), RigidBody::dynamic(), Collider::cuboid(Vec3::ONE)));
    }
}

fn steer(ctx: &mut Ctx) {
    let pads = inputs(ctx);
    let players: Vec<(Entity, u8)> = ctx.world.query::<(Entity, &Player)>().iter().map(|(e, p)| (e, p.0)).collect();
    for (e, id) in players {
        let pad = pads[usize::from(id)];
        let push = vec3(f32::from(pad.x), if pad.jump { 4.0 } else { 0.0 }, f32::from(pad.z)) * 0.4;
        let mass = ctx.physics3d.mass(e).unwrap_or(1.0);
        ctx.physics3d.apply_impulse(e, push * mass);
    }
}

fn follow(ctx: &mut Ctx) {
    ctx.world.query::<(&mut Pos, &Transform)>().for_each(|(p, t)| p.0 = t.position);
}

fn game(name: &str) -> Game {
    Game::new(name, 1, 1)
        .headless_frames(true)
        .physics3d(vec3(0.0, -9.81, 0.0))
        .sim_system(steer)
        .sim_system(follow)
}

fn offline(ticks: u64) -> BTreeMap<u64, u64> {
    let hashes = Arc::new(Mutex::new(BTreeMap::new()));
    let seen = Arc::clone(&hashes);
    game("offline")
        .system(move |ctx: &mut Ctx| {
            seen.lock().unwrap().insert(ctx.time.sim_tick(), world_hash(&ctx.world));
        })
        .run_headless(ticks, setup);
    Arc::try_unwrap(hashes).unwrap().into_inner().unwrap()
}

/// Both threads meet here once a frame. Panics instead of hanging when the
/// other thread is gone.
#[derive(Default)]
struct Gate {
    state: Mutex<(u32, u64)>,
    turn: Condvar,
}

impl Gate {
    fn wait(&self) {
        let mut s = self.state.lock().unwrap();
        let generation = s.1;
        s.0 += 1;
        if s.0 == 2 {
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

/// `(to, due frame, sequence, bytes)`.
type Packet = (usize, u64, u64, Vec<u8>);

#[derive(Default)]
struct Wire {
    gate: Gate,
    packets: Mutex<Vec<Packet>>,
}

struct Peer {
    /// World hashes at the start of ticks every input was in for.
    confirmed: BTreeMap<u64, u64>,
    /// The last frame's tick and world hash.
    last: (u64, u64),
    confirmed_frame: Option<u64>,
    rollbacks: u64,
    deepest: u32,
    desync: bool,
}

fn peer(me: usize, wire: Arc<Wire>) -> Peer {
    let confirmed = Arc::new(Mutex::new(BTreeMap::new()));
    let seen = Arc::clone(&confirmed);
    let result = Arc::new(Mutex::new(((0, 0), None, 0, 0, false)));
    let out = Arc::clone(&result);
    let mut frame = 0u64;
    let mut seq = 0u64;
    let mut rng = Rng::new(40 + me as u64);
    game("peer")
        .system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick();
            let hash = world_hash(&ctx.world);
            let rb = ctx.net.rollback::<Pad>().expect("the session runs");
            if rb.confirmed_frame().is_some_and(|c| c + 1 >= tick) {
                seen.lock().unwrap().insert(tick, hash);
            }
            rb.set_input(pad(me, tick));
            seq += 1;
            // 1 to 5 frames of latency: later packets often overtake.
            let mut due = frame + 1 + u64::from(rng.below(5));
            if me == 1 && HELD.contains(&frame) {
                due = HELD.end + 1;
            }
            wire.packets.lock().unwrap().push((1 - me, due, seq, rb.packet().to_vec()));
            wire.gate.wait();
            frame += 1;
            let mut arrived: Vec<Packet> = Vec::new();
            {
                let mut packets = wire.packets.lock().unwrap();
                let mut i = 0;
                while i < packets.len() {
                    if packets[i].0 == me && packets[i].1 <= frame {
                        arrived.push(packets.swap_remove(i));
                    } else {
                        i += 1;
                    }
                }
            }
            // Newest first: the order a network may hand them over in.
            arrived.sort_by_key(|p| std::cmp::Reverse(p.2));
            for p in &arrived {
                assert!(rb.receive(&p.3), "every packet is valid");
            }
            let s = rb.stats();
            *out.lock().unwrap() = ((tick, hash), rb.confirmed_frame(), s.rollbacks, s.deepest, rb.desync().is_some());
        })
        .run_headless(PASSES, move |ctx| {
            setup(ctx);
            let mut session = GameRollback::<Pad>::new(2, me)
                .input_delay(DELAY)
                .max_prediction(12)
                .checksum_every(1);
            session.set_input(pad(me, 0));
            ctx.net.start_rollback(session);
        });
    let (last, confirmed_frame, rollbacks, deepest, desync) = *result.lock().unwrap();
    Peer {
        confirmed: Arc::try_unwrap(confirmed).unwrap().into_inner().unwrap(),
        last,
        confirmed_frame,
        rollbacks,
        deepest,
        desync,
    }
}

#[test]
fn two_peers_with_3d_physics_and_late_reordered_inputs_end_in_the_same_world() {
    let wire = Arc::new(Wire::default());
    let threads: Vec<_> = (0..2)
        .map(|me| {
            let wire = Arc::clone(&wire);
            std::thread::spawn(move || peer(me, wire))
        })
        .collect();
    let peers: Vec<Peer> = threads.into_iter().map(|t| t.join().expect("peer thread")).collect();

    assert!(
        peers[0].deepest >= 5,
        "holding peer 1's packets back forces peer 0 to roll back several ticks at once ({})",
        peers[0].deepest
    );
    let end = peers.iter().map(|p| p.last.0).max().unwrap();
    let reference = offline(end + 1);
    for (i, peer) in peers.iter().enumerate() {
        assert!(!peer.desync, "peer {i}: the world hashes of every confirmed tick agree");
        assert!(peer.rollbacks > 0, "peer {i} rolled back");
        assert!(
            peer.confirmed_frame.is_some_and(|c| c > ACTIVE + u64::from(DELAY)),
            "peer {i}: every input that changed is in by the end"
        );
        let (tick, hash) = peer.last;
        assert_eq!(Some(&hash), reference.get(&tick), "peer {i}: the offline world at tick {tick}");
        for (tick, hash) in &peer.confirmed {
            assert_eq!(Some(hash), reference.get(tick), "peer {i}, tick {tick}: the offline world");
        }
    }
    assert_eq!(peers[0].last, peers[1].last, "both peers end on the same tick with the same world hash");
    // Ticks both peers saw confirmed at the time: the same world.
    let common: Vec<_> = peers[0]
        .confirmed
        .iter()
        .filter(|(t, _)| peers[1].confirmed.contains_key(t))
        .collect();
    for (tick, hash) in common {
        assert_eq!(peers[1].confirmed[tick], *hash, "tick {tick}: both peers' worlds");
    }
}
