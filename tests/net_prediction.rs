//! Client prediction and reconciliation: a client and a server exchange
//! numbered inputs and states (in a packet format of the test's own) over a
//! lossy in-memory link with 150 ms of round trip, and the walking
//! character controller is replayed bit for bit.

use std::time::Duration;

use ponko::net::InputQueue;
use ponko::net::Lane;
use ponko::net::MessageEvent;
use ponko::net::NetMessages;
use ponko::net::PeerId;
use ponko::net::Prediction;
use ponko::net::Reconciled;
use ponko::net::Smoothing;
use ponko::prelude::{Vec2, vec2};

const DT: f32 = 1.0 / 60.0;
/// Each way; with the jitter, a round trip of 150–180 ms.
const ONE_WAY: f64 = 0.075;
const SPEED: f32 = 5.0;

// ---- the game: a walker steered by a stick --------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
struct Walker {
    pos: Vec2,
    vel: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stick {
    x: i8,
    y: i8,
}

/// The step both sides run for one input.
fn walk(w: &mut Walker, stick: &Stick) {
    let target = vec2(f32::from(stick.x), f32::from(stick.y)) * SPEED;
    w.vel += (target - w.vel) * 0.2;
    w.pos += w.vel * DT;
}

/// What the player does on tick `t`.
fn script(t: u32) -> Stick {
    match t {
        30..150 => Stick { x: 1, y: 0 },
        150..240 => Stick { x: 0, y: 1 },
        240..300 => Stick { x: -1, y: -1 },
        _ => Stick { x: 0, y: 0 },
    }
}

/// Things only the server knows about.
#[derive(Clone, Copy, Default)]
struct World {
    /// A conveyor belt between x = 2 and x = 3 that carries the walker up
    /// at this speed.
    conveyor: f32,
    /// A shove (an explosion) on this tick of the server's.
    shove: Option<(u32, Vec2)>,
}

// ---- the game's own packet format: the engine never sees it ---------------

const INPUTS: u8 = 1;
const STATE: u8 = 2;
/// Inputs per packet: the newest unconfirmed ones.
const REDUNDANCY: usize = 16;

fn encode_inputs<'a>(inputs: impl Iterator<Item = (u16, &'a Stick)>) -> Vec<u8> {
    let mut b = vec![INPUTS];
    for (seq, s) in inputs {
        b.extend(seq.to_le_bytes());
        b.extend([s.x as u8, s.y as u8]);
    }
    b
}

fn decode_inputs(b: &[u8]) -> impl Iterator<Item = (u16, Stick)> + '_ {
    let body = if b.first() == Some(&INPUTS) { &b[1..] } else { &[][..] };
    body.as_chunks::<4>().0.iter().map(|c| {
        (
            u16::from_le_bytes([c[0], c[1]]),
            Stick {
                x: c[2] as i8,
                y: c[3] as i8,
            },
        )
    })
}

fn encode_state(ack: u16, w: &Walker) -> Vec<u8> {
    let mut b = vec![STATE];
    b.extend(ack.to_le_bytes());
    for v in [w.pos.x, w.pos.y, w.vel.x, w.vel.y] {
        b.extend(v.to_le_bytes());
    }
    b
}

fn decode_state(b: &[u8]) -> Option<(u16, Walker)> {
    if b.first() != Some(&STATE) || b.len() != 19 {
        return None;
    }
    let f = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    Some((
        u16::from_le_bytes([b[1], b[2]]),
        Walker {
            pos: vec2(f(3), f(7)),
            vel: vec2(f(11), f(15)),
        },
    ))
}

// ---- the link ---------------------------------------------------------------

/// Holds each packet back before the transport sees it: delay plus jitter,
/// on top of the transport's own loss and reordering.
#[derive(Default)]
struct DelayLine {
    held: Vec<(f64, Vec<u8>)>,
}
impl DelayLine {
    fn send(&mut self, due: f64, bytes: Vec<u8>) {
        self.held.push((due, bytes));
    }
    fn release(&mut self, now: f64, mut out: impl FnMut(Vec<u8>)) {
        let mut i = 0;
        while i < self.held.len() {
            if self.held[i].0 <= now {
                out(self.held.remove(i).1);
            } else {
                i += 1;
            }
        }
    }
}

fn poll(nets: &mut [NetMessages]) {
    for net in nets.iter_mut() {
        net.poll_elapsed(Duration::from_secs_f64(f64::from(DT))).unwrap();
    }
}

fn connect(nets: &mut [NetMessages]) -> PeerId {
    let mut guest = None;
    for _ in 0..200 {
        poll(nets);
        guest = guest.or_else(|| {
            nets[0].events().find_map(|e| match e {
                MessageEvent::Connected(p) => Some(p),
                _ => None,
            })
        });
        if let Some(guest) = guest
            && nets[1].events().any(|e| matches!(e, MessageEvent::Connected(_)))
        {
            return guest;
        }
    }
    panic!("no connection");
}

// ---- one session ------------------------------------------------------------

#[derive(Default)]
struct Run {
    client: Option<Walker>,
    server: Option<Walker>,
    /// Ticks from the stick moving to the predicted walker moving.
    response_ticks: Option<u32>,
    corrections: u64,
    confirmed: u64,
    skipped: u64,
    applied: u64,
    /// The biggest jump a correction made to the predicted position.
    raw_jump: f32,
    /// The biggest move of the drawn position from one frame to the next,
    /// beyond the walker's own step that tick: what of a correction shows.
    shown_jump: f32,
    /// The same while the conveyor caused the corrections (before the shove).
    shown_jump_before_shove: f32,
}

/// 600 ticks of play at 60 Hz, then a second for the last packets to land:
/// the client predicts, the server applies one step per input and answers
/// every other tick.
fn play(drop_one_in: u32, world: World) -> Run {
    let mut nets = NetMessages::loopback_lossy(2, drop_one_in);
    let guest = connect(&mut nets);
    let (mut up, mut down) = (DelayLine::default(), DelayLine::default());
    let start = Walker {
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
    };
    let mut prediction = Prediction::<Walker, Stick>::new(start);
    let mut smoothing = Smoothing::<Vec2>::new();
    let mut queue = InputQueue::<Stick>::new();
    let mut server = start;
    let mut run = Run::default();
    let mut last_shown = start.pos;
    let mut server_ticks = 0u32;

    for t in 0..660u32 {
        let now = f64::from(t) * f64::from(DT);
        let jitter = f64::from((t * 7919) % 31) / 1000.0; // 0–30 ms

        // Client: predict this tick's input, send the unconfirmed ones.
        let moved_before = prediction.state().vel != Vec2::ZERO;
        let before_step = prediction.state().pos;
        if t < 600 {
            prediction.predict(script(t), walk);
        }
        let stepped = prediction.state().pos - before_step;
        if !moved_before && prediction.state().vel != Vec2::ZERO && run.response_ticks.is_none() {
            run.response_ticks = Some(t - 30);
        }
        let skip = prediction.unacked().len().saturating_sub(REDUNDANCY);
        if prediction.unacked().len() > 0 {
            up.send(now + ONE_WAY + jitter, encode_inputs(prediction.unacked().skip(skip)));
        }

        // The network.
        let client_net = &mut nets[1];
        up.release(now, |b| client_net.send(PeerId::HOST, Lane::Unreliable, &b).unwrap());
        let server_net = &mut nets[0];
        down.release(now, |b| server_net.send(guest, Lane::Unreliable, &b).unwrap());
        poll(&mut nets);

        // Server: queue what arrived, one step per input.
        for event in nets[0].events().collect::<Vec<_>>() {
            if let MessageEvent::Message { bytes, .. } = event {
                for (seq, stick) in decode_inputs(&bytes) {
                    queue.push(seq, stick);
                }
            }
        }
        while let Some((_, stick)) = queue.pop() {
            walk(&mut server, &stick);
            if (2.0..3.0).contains(&server.pos.x) {
                server.pos.y += world.conveyor * DT;
            }
            if let Some((tick, shove)) = world.shove
                && tick == server_ticks
            {
                server.pos += shove;
            }
            server_ticks += 1;
        }
        if t.is_multiple_of(2)
            && let Some(ack) = queue.ack()
        {
            down.send(now + ONE_WAY + jitter, encode_state(ack, &server));
        }

        // Client: reconcile with what the server said.
        for event in nets[1].events().collect::<Vec<_>>() {
            let MessageEvent::Message { bytes, .. } = event else {
                continue;
            };
            let Some((ack, state)) = decode_state(&bytes) else {
                continue;
            };
            if let Reconciled::Corrected { before, .. } = prediction.reconcile(ack, state, walk) {
                run.raw_jump = run.raw_jump.max(before.pos.distance(prediction.state().pos));
                smoothing.correct(before.pos, prediction.state().pos);
            }
        }

        // Client: draw.
        smoothing.update(DT);
        let shown = smoothing.apply(prediction.state().pos);
        let change = (shown - last_shown - stepped).length();
        run.shown_jump = run.shown_jump.max(change);
        if world.shove.is_none_or(|(tick, _)| server_ticks < tick) {
            run.shown_jump_before_shove = run.shown_jump_before_shove.max(change);
        }
        last_shown = shown;
    }
    let stats = prediction.stats();
    run.corrections = stats.corrections;
    run.confirmed = stats.confirmed;
    run.skipped = queue.stats().skipped;
    run.applied = queue.stats().applied;
    run.client = Some(*prediction.state());
    run.server = Some(server);
    run
}

// ---- tests ------------------------------------------------------------------

#[test]
fn the_local_player_moves_on_the_tick_of_the_input() {
    let run = play(0, World::default());
    assert_eq!(run.response_ticks, Some(0), "no round trip before moving");
}

#[test]
fn predictions_survive_loss_and_reordering_without_corrections() {
    // Every 5th datagram lost, newest first, 150+ ms round trip.
    let run = play(5, World::default());
    assert_eq!(run.corrections, 0, "the same step on both sides always agrees");
    // Of 300 states sent, a fifth are lost and reordered ones are stale.
    assert!(run.confirmed > 150, "{} states confirmed", run.confirmed);
    assert_eq!(run.skipped, 0, "repeated inputs made up for every lost packet");
    assert_eq!(run.applied, 600, "each input applied exactly once");
    assert_eq!(run.client, run.server);
}

#[test]
fn a_late_joiner_numbers_its_inputs_like_the_running_stream() {
    // The server's input stream for this seat is near the end of its
    // numbers, so the new prediction picks up its numbering.
    let mut p = Prediction::<Walker, Stick>::new(Walker {
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
    })
    .starting_at(65_534);
    let right = Stick { x: 1, y: 0 };
    let seqs: Vec<u16> = (0..3).map(|_| p.predict(right, walk)).collect();
    assert_eq!(seqs, [65_534, 65_535, 0]);
    // The server acknowledges by those numbers; the first two are dropped.
    let mut server = Walker {
        pos: Vec2::ZERO,
        vel: Vec2::ZERO,
    };
    walk(&mut server, &right);
    walk(&mut server, &right);
    p.reconcile(65_535, server, walk);
    assert_eq!(p.unacked().map(|(seq, _)| seq).collect::<Vec<_>>(), [0]);
}

#[test]
fn server_side_pushes_are_corrected_and_smoothed() {
    let world = World {
        conveyor: 1.0,
        shove: Some((200, vec2(0.0, -1.0))),
    };
    let run = play(7, world);
    assert!(run.corrections > 5, "{} corrections", run.corrections);
    assert_eq!(run.client, run.server, "the client ends where the server is");
    // Negative control: the shove shows as a whole jump without smoothing...
    assert!(run.raw_jump > 0.9, "raw jump {}", run.raw_jump);
    // ...and as a slide of a tenth of it per frame with.
    assert!(run.shown_jump < 0.12, "drawn jump {}", run.shown_jump);
    // Small, steady mispredictions (the conveyor) show as the belt carrying
    // the walker (1 unit/s, 0.017 a frame), with no steps.
    assert!(
        run.shown_jump_before_shove < 0.02,
        "conveyor corrections drawn as {} per frame",
        run.shown_jump_before_shove
    );
}

#[path = "../examples/net_prediction.rs"]
#[allow(dead_code)]
mod example;

/// The example (150 ms, every 10th packet lost) runs and its own checks hold.
#[test]
fn the_prediction_example_runs() {
    example::main();
}

// ---- the character controller ---------------------------------------------

#[cfg(feature = "physics3d")]
mod character {
    use super::{DelayLine, InputQueue, Prediction, Reconciled};
    use ponko::d3::{Character, Collider, RigidBody, Transform};
    use ponko::game::Phase;
    use ponko::prelude::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Feet {
        feet: Vec3,
        velocity: Vec3,
        grounded: bool,
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct Stick {
        walk: Vec2,
        jump: bool,
    }

    /// Puts an input on a character: the same on the client and the server.
    fn steer(c: &mut Character, stick: &Stick) {
        c.velocity.x = stick.walk.x * 4.0;
        c.velocity.z = stick.walk.y * 4.0;
        if stick.jump && c.grounded {
            c.velocity.y = 5.0;
        }
    }

    fn script(t: u64) -> Stick {
        Stick {
            walk: match t {
                30..120 => vec2(1.0, 0.0),
                120..200 => vec2(0.6, 0.8),
                200..260 => vec2(-1.0, 0.0),
                _ => Vec2::ZERO,
            },
            jump: t == 150 || t == 230,
        }
    }

    /// A floor, a ramp, a step and a wall.
    fn level(ctx: &mut Ctx) {
        let fixed = RigidBody::fixed();
        ctx.world.spawn((
            Transform::at(vec3(0.0, -0.5, 0.0)),
            fixed,
            Collider::cuboid(vec3(80.0, 1.0, 80.0)),
        ));
        ctx.world.spawn((
            Transform::at(vec3(4.0, 0.0, 0.0)).rotated(Quat::from_rotation_z(0.3)),
            fixed,
            Collider::cuboid(vec3(3.0, 0.4, 3.0)),
        ));
        ctx.world.spawn((
            Transform::at(vec3(6.0, 0.1, 3.0)),
            fixed,
            Collider::cuboid(vec3(2.0, 0.2, 2.0)),
        ));
        ctx.world.spawn((
            Transform::at(vec3(9.0, 1.0, 0.0)),
            fixed,
            Collider::cuboid(vec3(0.5, 2.0, 20.0)),
        ));
    }

    fn player(ctx: &mut Ctx) -> Entity {
        ctx.world.query::<(Entity, &Character)>().iter().next().unwrap().0
    }

    fn feet_of(ctx: &mut Ctx, e: Entity) -> Feet {
        let c = *ctx.world.get::<Character>(e).unwrap();
        Feet {
            feet: ctx.physics3d.position(e).unwrap(),
            velocity: c.velocity,
            grounded: c.grounded,
        }
    }

    #[test]
    fn move_character_matches_the_physics_step_bit_for_bit() {
        #[derive(Default)]
        struct Expected(Option<Feet>, u32);
        let world = Game::new("t", 1, 1)
            .physics3d(vec3(0.0, -9.81, 0.0))
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                let e = player(ctx);
                let t = ctx.time.sim_tick();
                let mut c = *ctx.world.get::<Character>(e).unwrap();
                steer(&mut c, &script(t));
                *ctx.world.get_mut::<Character>(e).unwrap() = c;
                let Some(feet) = ctx.physics3d.position(e) else {
                    return; // joins at the first step
                };
                let dt = ctx.time.fixed_dt();
                let feet = ctx.physics3d.move_character(e, &mut c, feet, dt);
                ctx.world.resource_mut::<Expected>().0 = Some(Feet {
                    feet,
                    velocity: c.velocity,
                    grounded: c.grounded,
                });
            })
            .add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                let e = player(ctx);
                let now = feet_of(ctx, e);
                let t = ctx.time.sim_tick();
                let checked = ctx.world.resource_mut::<Expected>();
                if let Some(expected) = checked.0.take() {
                    assert_eq!(now, expected, "tick {t}");
                    checked.1 += 1;
                }
            })
            .run_headless(300, |ctx| {
                level(ctx);
                ctx.world.insert_resource(Expected::default());
                ctx.world
                    .spawn((Transform::at(vec3(0.0, 0.5, 0.0)), Character::new(1.8, 0.3)));
            });
        assert!(world.resource::<Expected>().1 > 290);
    }

    #[derive(Default)]
    struct Session {
        prediction: Option<Prediction<Feet, Stick>>,
        queue: InputQueue<Stick>,
        up: DelayLine,
        down: DelayLine,
        sent: u32,
        holds: u32,
        corrections: u32,
        server_started: bool,
    }

    /// One game holds both sides: the server's character is stepped by the
    /// engine, one input per tick; the client predicts the same character
    /// with `move_character`, looking through the server's capsule. Inputs
    /// and states take 4 ticks (80 ms) each way and every 6th packet is lost.
    #[test]
    fn a_predicted_character_ends_where_the_server_has_it() {
        const LAG: f64 = 4.0;
        let world = Game::new("t", 1, 1)
            .physics3d(vec3(0.0, -9.81, 0.0))
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                let t = ctx.time.sim_tick();
                if t < 20 {
                    return; // let the character land first
                }
                let e = player(ctx);
                let dt = ctx.time.fixed_dt();
                let mut s = ctx.world.remove_resource::<Session>().unwrap();
                let now = t as f64;
                let start = feet_of(ctx, e);
                let prediction = s.prediction.get_or_insert_with(|| Prediction::new(start));
                let physics = &ctx.physics3d;
                let mut step = |f: &mut Feet, stick: &Stick| {
                    let mut c = Character::new(1.8, 0.3);
                    c.velocity = f.velocity;
                    c.grounded = f.grounded;
                    steer(&mut c, stick);
                    f.feet = physics.move_character(e, &mut c, f.feet, dt);
                    f.velocity = c.velocity;
                    f.grounded = c.grounded;
                };

                // Client: states that arrived, then this tick's input.
                let mut arrived = Vec::new();
                s.down.release(now, |b| arrived.push(b));
                for b in arrived {
                    let ack = u16::from_le_bytes([b[0], b[1]]);
                    let state: Feet = decode(&b[2..]);
                    if let Reconciled::Corrected { .. } = prediction.reconcile(ack, state, &mut step) {
                        s.corrections += 1;
                    }
                }
                prediction.predict(script(t - 20), step);
                s.sent += 1;
                if !s.sent.is_multiple_of(6) {
                    let mut packet = Vec::new();
                    for (seq, stick) in prediction.unacked().rev().take(8) {
                        packet.extend(seq.to_le_bytes());
                        packet.extend(stick.walk.x.to_le_bytes());
                        packet.extend(stick.walk.y.to_le_bytes());
                        packet.push(u8::from(stick.jump));
                    }
                    s.up.send(now + LAG, packet);
                }

                // Server: inputs that arrived; one per tick, or the last again.
                let mut arrived = Vec::new();
                s.up.release(now, |b| arrived.push(b));
                for b in arrived {
                    for c in b.as_chunks::<11>().0 {
                        let f = |i: usize| f32::from_le_bytes(c[i..i + 4].try_into().unwrap());
                        let stick = Stick {
                            walk: vec2(f(2), f(6)),
                            jump: c[10] == 1,
                        };
                        s.queue.push(u16::from_le_bytes([c[0], c[1]]), stick);
                    }
                }
                let stick = match s.queue.pop() {
                    Some((_, stick)) => Some(stick),
                    None => {
                        s.holds += u32::from(s.server_started);
                        s.queue.last().copied()
                    }
                };
                if let Some(stick) = stick {
                    s.server_started = true;
                    let c = ctx.world.get_mut::<Character>(e).unwrap();
                    steer(c, &stick);
                }
                ctx.world.insert_resource(s);
            })
            .add(Phase::AfterPhysics, |ctx: &mut Ctx| {
                let t = ctx.time.sim_tick();
                let e = player(ctx);
                let state = feet_of(ctx, e);
                let mut s = ctx.world.remove_resource::<Session>().unwrap();
                if let Some(ack) = s.queue.ack()
                    && t.is_multiple_of(2)
                    && !t.is_multiple_of(14)
                {
                    let mut packet = ack.to_le_bytes().to_vec();
                    packet.extend(encode(&state));
                    s.down.send(t as f64 + LAG, packet);
                }
                ctx.world.insert_resource(s);
            })
            .run_headless(400, |ctx| {
                level(ctx);
                ctx.world.insert_resource(Session::default());
                ctx.world
                    .spawn((Transform::at(vec3(0.0, 0.5, 0.0)), Character::new(1.8, 0.3)));
            });
        let mut world = world;
        let e = world.query::<(Entity, &Character)>().iter().next().unwrap().0;
        let c = *world.get::<Character>(e).unwrap();
        let s = world.resource::<Session>();
        let client = *s.prediction.as_ref().unwrap().state();
        assert!(client.feet.x > 3.0, "walked: {}", client.feet);
        assert_eq!((client.velocity, client.grounded), (c.velocity, c.grounded));
        assert!(
            client.feet.distance(world.get::<Transform>(e).unwrap().position) < 1e-5,
            "client {} server {}",
            client.feet,
            world.get::<Transform>(e).unwrap().position
        );
        assert!(
            s.corrections <= s.holds,
            "{} corrections for {} ticks the server guessed",
            s.corrections,
            s.holds
        );
    }

    fn encode(f: &Feet) -> Vec<u8> {
        let mut b = Vec::new();
        for v in f.feet.to_array().into_iter().chain(f.velocity.to_array()) {
            b.extend(v.to_le_bytes());
        }
        b.push(u8::from(f.grounded));
        b
    }

    fn decode(b: &[u8]) -> Feet {
        let f = |i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        Feet {
            feet: vec3(f(0), f(1), f(2)),
            velocity: vec3(f(3), f(4), f(5)),
            grounded: b[24] == 1,
        }
    }
}
