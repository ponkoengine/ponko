//! Rollback: what saving, loading and re-simulating cost, for 100, 1,000
//! and 10,000 entities, without and with 2D physics, and a whole two-peer
//! session against the same game offline.
//!
//! - `typed-save` / `typed-load`: the state a rollback game keeps, copied
//!   component by component (`(Entity, Pos, Vel)`) into a reused buffer,
//!   and written back with `get_mut`.
//! - `reflect-save` / `reflect-load`: the same world through `World::save`
//!   and `World::load` (JSON), the engine's only whole-world snapshot.
//! - `resim`: one tick of the game (the per-tick cost a rollback repeats).
//! - `physics-*`: rapier's state (bodies, colliders, islands, broad and
//!   narrow phase, joints, CCD) cloned and restored, and one step, for a
//!   pile of boxes that has settled for a second. The restore is checked to
//!   step bit for bit like the original.
//! - `session-*` / `offline-*`: 300 ticks of the game on two peers over a
//!   link with 4–8 ticks of latency and 10% loss, against one offline run.
//! - `snapshot-save` / `snapshot-load`: the same world through
//!   `World::save_snapshot` and `World::restore_snapshot` (every component,
//!   the entity allocator, the archetype layout), into a reused snapshot.
//! - `game-offline-*` / `game-session-*`: a whole `Game` (the crowd as a
//!   simulation system) running 300 ticks headless, offline and through a
//!   `GameRollback` whose remote player's input arrives 4 frames late and
//!   changes every 4 ticks, so most ticks roll back about 5 ticks. The
//!   engine saves the world, the events and the physics before every tick.
//!   `game-physics-*`: the crowd as 2D physics bodies.
use std::collections::VecDeque;

use ponko::d2::{Collider, RigidBody, Transform};
use ponko::ecs_core::WorldSnapshot;
use ponko::net::{GameRollback, Rollback, RollbackGame};
use ponko::prelude::{Component, Ctx, Entity, Game, Rng, Vec2, Without, World, vec2};
use ponko::reflect::WorldReflect;
use rapier2d::prelude as rp;
use serde::{Deserialize, Serialize};

use crate::Bench;

#[derive(Component, Serialize, Deserialize, Clone, Copy)]
struct Pos(Vec2);
#[derive(Component, Serialize, Deserialize, Clone, Copy)]
struct Vel(Vec2);
#[derive(Component, Serialize, Deserialize, Clone, Copy)]
struct Player(u8);

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
struct Pad(i8, i8);

struct Arena {
    world: World,
    tick: u64,
    pushers: Vec<Vec2>,
}

#[derive(Default)]
struct Saved {
    tick: u64,
    bodies: Vec<(Entity, Pos, Vel)>,
}

impl Arena {
    fn new(crowd: usize) -> Self {
        let mut world = World::new();
        world.reflect::<Pos>();
        world.reflect::<Vel>();
        world.reflect::<Player>();
        for p in 0..2u8 {
            world.spawn((Pos(vec2(f32::from(p) * 40.0, 0.0)), Vel(Vec2::ZERO), Player(p)));
        }
        let mut rng = Rng::new(3);
        let side = (crowd as f32).sqrt() * 10.0;
        world.spawn_batch((0..crowd).map(|_| {
            (
                Pos(vec2(rng.range(-side, side), rng.range(-side, side))),
                Vel(Vec2::ZERO),
            )
        }));
        Self {
            world,
            tick: 0,
            pushers: Vec::new(),
        }
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
            *self.world.get_mut::<Pos>(e).expect("fixed entities") = p;
            *self.world.get_mut::<Vel>(e).expect("fixed entities") = v;
        }
    }
    fn advance(&mut self, inputs: &[Pad]) {
        let dt = 1.0 / 60.0;
        let mut gust = Rng::new(self.tick);
        let gust = vec2(gust.range(-5.0, 5.0), gust.range(-5.0, 5.0));
        let pushers = &mut self.pushers;
        pushers.clear();
        self.world
            .query::<(&mut Pos, &mut Vel, &Player)>()
            .for_each(|(p, v, who)| {
                let Pad(x, y) = inputs[usize::from(who.0)];
                v.0 += (vec2(f32::from(x), f32::from(y)) * 600.0 + gust) * dt;
                v.0 *= 0.9;
                p.0 += v.0 * dt;
                pushers.push(p.0);
            });
        let pushers = &self.pushers;
        self.world
            .query::<(&mut Pos, &mut Vel)>()
            .filter::<Without<Player>>()
            .for_each(|(p, v)| {
                for &at in pushers {
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
}

fn pad(p: usize, t: u64) -> Pad {
    let mut rng = Rng::new(((p as u64) << 32) | (t / 4));
    Pad(rng.below(3) as i8 - 1, rng.below(3) as i8 - 1)
}

/// Every piece of rapier's state that a step reads or writes (the
/// pipelines hold only scratch buffers).
#[derive(Clone)]
struct PhysicsState {
    bodies: rp::RigidBodySet,
    colliders: rp::ColliderSet,
    islands: rp::IslandManager,
    broad_phase: rp::BroadPhaseBvh,
    narrow_phase: rp::NarrowPhase,
    impulse_joints: rp::ImpulseJointSet,
    multibody_joints: rp::MultibodyJointSet,
    ccd: rp::CCDSolver,
}

impl PhysicsState {
    fn of(w: &rp::PhysicsWorld) -> Self {
        Self {
            bodies: w.bodies.clone(),
            colliders: w.colliders.clone(),
            islands: w.islands.clone(),
            broad_phase: w.broad_phase.clone(),
            narrow_phase: w.narrow_phase.clone(),
            impulse_joints: w.impulse_joints.clone(),
            multibody_joints: w.multibody_joints.clone(),
            ccd: w.ccd_solver.clone(),
        }
    }
    fn save(&mut self, w: &rp::PhysicsWorld) {
        self.bodies.clone_from(&w.bodies);
        self.colliders.clone_from(&w.colliders);
        self.islands.clone_from(&w.islands);
        self.broad_phase.clone_from(&w.broad_phase);
        self.narrow_phase.clone_from(&w.narrow_phase);
        self.impulse_joints.clone_from(&w.impulse_joints);
        self.multibody_joints.clone_from(&w.multibody_joints);
        self.ccd.clone_from(&w.ccd_solver);
    }
    fn load(&self, w: &mut rp::PhysicsWorld) {
        w.bodies.clone_from(&self.bodies);
        w.colliders.clone_from(&self.colliders);
        w.islands.clone_from(&self.islands);
        w.broad_phase.clone_from(&self.broad_phase);
        w.narrow_phase.clone_from(&self.narrow_phase);
        w.impulse_joints.clone_from(&self.impulse_joints);
        w.multibody_joints.clone_from(&self.multibody_joints);
        w.ccd_solver.clone_from(&self.ccd);
    }
}

/// `n` boxes in a pile (the physics benchmark's scene), settled for a second.
fn pile(n: usize) -> rp::PhysicsWorld {
    let (boxes, width) = crate::physics::scene(n);
    let mut w = rp::PhysicsWorld::new();
    w.integration_parameters.length_unit = 50.0;
    w.integration_parameters.dt = 1.0 / 60.0;
    w.gravity = rp::Vector::new(0.0, 980.0);
    let mut wall = |x: f32, y: f32, hw: f32, hh: f32| {
        let body = w.insert_body(rp::RigidBodyBuilder::fixed().translation(rp::Vector::new(x, y)));
        w.insert_collider(rp::ColliderBuilder::cuboid(hw, hh), Some(body));
    };
    wall(0.0, 100.0, width / 2.0 + 20.0, 10.0);
    wall(-width / 2.0 - 10.0, -500.0, 10.0, 600.0);
    wall(width / 2.0 + 10.0, -500.0, 10.0, 600.0);
    for &(x, y) in &boxes {
        let body = w.insert_body(rp::RigidBodyBuilder::dynamic().translation(rp::Vector::new(x, y)));
        w.insert_collider(rp::ColliderBuilder::cuboid(5.0, 5.0), Some(body));
    }
    for _ in 0..60 {
        w.step();
    }
    w
}

/// Every body's position and velocity, bit for bit.
fn physics_bits(w: &rp::PhysicsWorld) -> Vec<u32> {
    w.bodies
        .iter()
        .flat_map(|(_, b)| {
            let (p, v) = (b.translation(), b.linvel());
            [p.x, p.y, v.x, v.y, b.rotation().angle()].map(f32::to_bits)
        })
        .collect()
}

/// Two peers over a link with 4–8 ticks of latency and 10% loss.
fn session(crowd: usize, ticks: u64) -> u64 {
    let mut peers: Vec<Rollback<Pad, Saved>> = (0..2).map(|p| Rollback::new(2, p)).collect();
    let mut games: Vec<Arena> = (0..2).map(|_| Arena::new(crowd)).collect();
    let mut rng = Rng::new(9);
    let mut wire: Vec<(usize, u64, Vec<u8>)> = Vec::new();
    let mut now = 0;
    while peers.iter().any(|p| p.frame() < ticks) {
        for p in 0..2 {
            if peers[p].frame() < ticks {
                let t = peers[p].frame();
                let _ = peers[p].advance(&mut games[p], pad(p, t));
            }
            if !rng.chance(0.1) {
                wire.push((1 - p, now + 4 + u64::from(rng.below(5)), peers[p].packet().to_vec()));
            }
        }
        now += 1;
        wire.retain(|(to, due, bytes)| {
            if *due <= now {
                peers[*to].receive(bytes);
                false
            } else {
                true
            }
        });
    }
    peers.iter().map(|p| p.stats().resimulated).sum()
}

/// The remote player of a [`game`] session: a bare session whose game
/// does nothing, so it only makes inputs and packets.
struct Remote;

impl RollbackGame<Pad> for Remote {
    type State = ();
    fn save(&mut self, (): &mut ()) {}
    fn load(&mut self, (): &()) {}
    fn advance(&mut self, _: &[Pad]) {}
}

/// Every player's input in a [`game`] tick: the session's, or offline, what
/// it would be with every input in at once.
fn game_inputs(ctx: &Ctx) -> [Pad; 2] {
    if let Some(i) = ctx.net.rollback_inputs::<Pad>() {
        return [i[0], i[1]];
    }
    let t = ctx.time.sim_tick();
    let at = |p| t.checked_sub(2).map_or(Pad::default(), |from| pad(p, from));
    [at(0), at(1)]
}

/// The arena's tick as a simulation system; with physics, players push
/// bodies with impulses and rapier moves everything.
fn crowd_system(ctx: &mut Ctx) {
    let inputs = game_inputs(ctx);
    let dt = 1.0 / 60.0;
    let mut gust = ctx.random().stream(0);
    let gust = vec2(gust.range(-5.0, 5.0), gust.range(-5.0, 5.0));
    if ctx.world.query::<&RigidBody>().count() > 0 {
        let mut pushes = Vec::new();
        ctx.world
            .query::<(Entity, &Player)>()
            .for_each(|(e, who)| pushes.push((e, inputs[usize::from(who.0)])));
        for (e, Pad(x, y)) in pushes {
            let mass = ctx.physics.mass(e).unwrap_or(1.0);
            ctx.physics
                .apply_impulse(e, (vec2(f32::from(x), f32::from(y)) * 600.0 + gust) * dt * mass);
        }
        return;
    }
    let mut pushers = Vec::new();
    ctx.world
        .query::<(&mut Pos, &mut Vel, &Player)>()
        .for_each(|(p, v, who)| {
            let Pad(x, y) = inputs[usize::from(who.0)];
            v.0 += (vec2(f32::from(x), f32::from(y)) * 600.0 + gust) * dt;
            v.0 *= 0.9;
            p.0 += v.0 * dt;
            pushers.push(p.0);
        });
    ctx.world
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
}

/// A whole game of `crowd` entities running `ticks` ticks headless, through
/// a rollback session if `session`; returns the ticks run again.
fn game(crowd: usize, ticks: u64, session: bool, physics: bool) -> u64 {
    let mut game = Game::new("rollback", 1, 1).headless_frames(true);
    if physics {
        game = game.physics(Vec2::ZERO);
    }
    let mut remote: Rollback<Pad, ()> = Rollback::new(2, 1).checksum_every(0);
    let mut wire: VecDeque<(u64, Vec<u8>)> = VecDeque::new();
    let mut frame = 0;
    let resimulated = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let out = std::sync::Arc::clone(&resimulated);
    game = game.sim_system(crowd_system);
    if session {
        game = game.system(move |ctx: &mut Ctx| {
            let t = ctx.time.sim_tick();
            let local = ctx.net.rollback::<Pad>().expect("the session runs");
            local.set_input(pad(0, t));
            remote.receive(local.packet());
            while remote.frame() <= t {
                let f = remote.frame();
                if !remote.advance(&mut Remote, pad(1, f)).ran() {
                    break;
                }
            }
            wire.push_back((frame + 4, remote.packet().to_vec()));
            while wire.front().is_some_and(|w| w.0 <= frame) {
                let (_, bytes) = wire.pop_front().expect("checked");
                local.receive(&bytes);
            }
            frame += 1;
            out.store(local.stats().resimulated, std::sync::atomic::Ordering::Relaxed);
        });
    }
    game.run_headless(ticks, move |ctx| {
        ctx.world.reflect::<Pos>();
        ctx.world.reflect::<Vel>();
        ctx.world.reflect::<Player>();
        let mut rng = Rng::new(3);
        let side = (crowd as f32).sqrt() * 10.0;
        let body = |ctx: &mut Ctx, e: Entity, at: Vec2, r: f32| {
            if physics {
                ctx.world.insert(e, Transform::at(at));
                ctx.world.insert(e, RigidBody::dynamic());
                ctx.world.insert(e, Collider::circle(r));
            }
        };
        for p in 0..2u8 {
            let at = vec2(f32::from(p) * 40.0, 0.0);
            let e = ctx.world.spawn((Pos(at), Vel(Vec2::ZERO), Player(p)));
            body(ctx, e, at, 8.0);
        }
        for _ in 0..crowd {
            let at = vec2(rng.range(-side, side), rng.range(-side, side));
            let e = ctx.world.spawn((Pos(at), Vel(Vec2::ZERO)));
            body(ctx, e, at, 3.0);
        }
        if session {
            let mut rb = GameRollback::<Pad>::new(2, 0);
            rb.set_input(pad(0, 0));
            ctx.net.start_rollback(rb);
        }
    });
    resimulated.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn run(b: &mut Bench) {
    for n in [100, 1_000, 10_000] {
        b.steady(
            &format!("rollback/typed-save-{n}"),
            50,
            || (Arena::new(n), Saved::default()),
            |(arena, saved)| arena.save(saved),
        );
        b.steady(
            &format!("rollback/typed-load-{n}"),
            50,
            || {
                let mut arena = Arena::new(n);
                let mut saved = Saved::default();
                arena.save(&mut saved);
                (arena, saved)
            },
            |(arena, saved)| arena.load(saved),
        );
        b.steady(
            &format!("rollback/reflect-save-{n}"),
            if n >= 10_000 { 10 } else { 30 },
            || Arena::new(n),
            |arena| {
                std::hint::black_box(arena.world.save());
            },
        );
        b.steady(
            &format!("rollback/reflect-load-{n}"),
            if n >= 10_000 { 10 } else { 30 },
            || {
                let arena = Arena::new(n);
                let save = arena.world.save();
                (arena, save)
            },
            |(arena, save)| arena.world.load(save).expect("loads"),
        );
        b.steady(
            &format!("rollback/snapshot-save-{n}"),
            50,
            || {
                let arena = Arena::new(n);
                let mut snapshot = WorldSnapshot::new();
                arena
                    .world
                    .save_snapshot(&mut snapshot)
                    .expect("every component is Clone");
                (arena, snapshot)
            },
            |(arena, snapshot)| arena.world.save_snapshot(snapshot).expect("saves"),
        );
        b.steady(
            &format!("rollback/snapshot-load-{n}"),
            50,
            || {
                let arena = Arena::new(n);
                let mut snapshot = WorldSnapshot::new();
                arena
                    .world
                    .save_snapshot(&mut snapshot)
                    .expect("every component is Clone");
                (arena, snapshot)
            },
            |(arena, snapshot)| arena.world.restore_snapshot(snapshot).expect("restores"),
        );
        b.steady(
            &format!("rollback/resim-{n}"),
            50,
            || Arena::new(n),
            |arena| {
                let t = arena.tick;
                arena.advance(&[pad(0, t), pad(1, t)]);
            },
        );
    }
    for n in [100, 1_000, 10_000] {
        let name = format!("rollback/physics-save-{n}");
        if !b.skips(&name) {
            // Restoring must give back exactly the same simulation.
            let mut w = pile(n);
            let saved = PhysicsState::of(&w);
            for _ in 0..30 {
                w.step();
            }
            let straight = physics_bits(&w);
            saved.load(&mut w);
            for _ in 0..30 {
                w.step();
            }
            assert!(
                physics_bits(&w) == straight,
                "a restored rapier world steps exactly like the original"
            );
            println!("rollback: {n} bodies restored and stepped 30 ticks, bit for bit the same");
        }
        b.steady(
            &name,
            if n >= 10_000 { 10 } else { 30 },
            || {
                let w = pile(n);
                let s = PhysicsState::of(&w);
                (w, s)
            },
            |(w, s)| s.save(w),
        );
        b.steady(
            &format!("rollback/physics-load-{n}"),
            if n >= 10_000 { 10 } else { 30 },
            || {
                let w = pile(n);
                let s = PhysicsState::of(&w);
                (w, s)
            },
            |(w, s)| s.load(w),
        );
        b.steady(
            &format!("rollback/physics-step-{n}"),
            if n >= 10_000 { 10 } else { 30 },
            || pile(n),
            |w| w.step(),
        );
    }
    const TICKS: u64 = 300;
    for n in [100, 1_000, 10_000] {
        b.run(
            &format!("rollback/offline-{n}"),
            3,
            || Arena::new(n),
            |arena| {
                for t in 0..TICKS {
                    arena.advance(&[pad(0, t), pad(1, t)]);
                }
            },
        );
        let mut resimulated = 0;
        b.run(
            &format!("rollback/session-{n}"),
            3,
            || (),
            |_| resimulated = session(n, TICKS),
        );
        if resimulated > 0 {
            println!("rollback: session-{n}: {resimulated} ticks re-simulated over 2 peers x {TICKS} ticks");
        }
    }
    for n in [100, 1_000, 10_000] {
        b.run(
            &format!("rollback/game-offline-{n}"),
            3,
            || (),
            |()| {
                game(n, TICKS, false, false);
            },
        );
        let mut resimulated = 0;
        b.run(
            &format!("rollback/game-session-{n}"),
            3,
            || (),
            |()| {
                resimulated = game(n, TICKS, true, false);
            },
        );
        if resimulated > 0 {
            println!("rollback: game-session-{n}: {resimulated} ticks run again over {TICKS} ticks");
        }
    }
    for n in [100, 1_000] {
        b.run(
            &format!("rollback/game-physics-offline-{n}"),
            3,
            || (),
            |()| {
                game(n, TICKS, false, true);
            },
        );
        let mut resimulated = 0;
        b.run(
            &format!("rollback/game-physics-session-{n}"),
            3,
            || (),
            |()| {
                resimulated = game(n, TICKS, true, true);
            },
        );
        if resimulated > 0 {
            println!("rollback: game-physics-session-{n}: {resimulated} ticks run again over {TICKS} ticks");
        }
    }
}
