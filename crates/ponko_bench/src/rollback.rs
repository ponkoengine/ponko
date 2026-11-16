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
use ponko::net::{Rollback, RollbackGame};
use ponko::prelude::{Component, Entity, Rng, Vec2, Without, World, vec2};
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
}
