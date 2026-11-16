//! Physics: the same pile of boxes stepped through Ponko (`Game::physics`,
//! components synced every tick) and through rapier directly. The gap is
//! what Ponko's sync costs.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::d2::Collider;
use ponko::d2::RigidBody;
use ponko::d2::Transform as Transform2D;
use ponko::d2::{Hit, Physics, QueryFilter};
use ponko::prelude::{Ctx, Game, Vec2, vec2};
use rapier2d::prelude as rp;

use crate::Bench;

pub const TICKS: u64 = 300;

/// `n` 10 px boxes in rows above a floor between two walls.
pub fn scene(n: usize) -> (Vec<(f32, f32)>, f32) {
    let per_row = (n as f32).sqrt().ceil() as usize * 2;
    let width = per_row as f32 * 12.0;
    let boxes = (0..n)
        .map(|i| {
            (
                (i % per_row) as f32 * 12.0 - width / 2.0 + 6.0,
                -((i / per_row) as f32) * 12.0,
            )
        })
        .collect();
    (boxes, width)
}

pub fn run(b: &mut Bench) {
    rays(b);
    for n in [1_000, 10_000] {
        let (boxes, width) = scene(n);
        b.run(
            &format!("physics/ponko-{n}"),
            3,
            || (),
            |_| {
                let world = Game::new("bench", 1, 1)
                    .physics(vec2(0.0, 980.0))
                    .run_headless(TICKS, |ctx| {
                        let wall = |x: f32, y: f32, w: f32, h: f32| {
                            (
                                Transform2D::at(vec2(x, y)),
                                RigidBody::fixed(),
                                Collider::rect(vec2(w, h)),
                            )
                        };
                        ctx.world.spawn(wall(0.0, 100.0, width + 40.0, 20.0));
                        ctx.world.spawn(wall(-width / 2.0 - 10.0, -500.0, 20.0, 1200.0));
                        ctx.world.spawn(wall(width / 2.0 + 10.0, -500.0, 20.0, 1200.0));
                        for &(x, y) in &boxes {
                            ctx.world.spawn((
                                Transform2D::at(vec2(x, y)),
                                RigidBody::dynamic(),
                                Collider::rect(vec2(10.0, 10.0)),
                            ));
                        }
                    });
                std::hint::black_box(world);
            },
        );
        b.run(
            &format!("physics/rapier-{n}"),
            3,
            || (),
            |_| {
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
                    w.insert_collider(
                        rp::ColliderBuilder::cuboid(5.0, 5.0).active_events(rp::ActiveEvents::COLLISION_EVENTS),
                        Some(body),
                    );
                }
                let (a, ra) = std::sync::mpsc::channel();
                let (bb, rb) = std::sync::mpsc::channel();
                let (c, rc) = std::sync::mpsc::channel();
                let collector = rp::ChannelEventCollector::new(a, bb, c);
                for _ in 0..TICKS {
                    w.step_with_events(&(), &collector);
                    while ra.try_recv().is_ok() {}
                }
                drop((rb, rc));
                std::hint::black_box(w);
            },
        );
    }
}

/// Ray casts per run of the query benchmarks.
pub const RAYS: usize = 10_000;

/// A 2D level of fixed bodies: `n` 20 px boxes and circles on a 40 px grid.
fn level(n: usize) -> Vec<(Vec2, bool)> {
    let per_row = (n as f32).sqrt().ceil() as usize;
    (0..n)
        .map(|i| {
            (
                vec2((i % per_row) as f32 * 40.0, (i / per_row) as f32 * 40.0),
                i % 3 == 0,
            )
        })
        .collect()
}

/// Ray `i` of a run: from points along the level's left edge, fanning right
/// across it (deterministic, no RNG).
fn ray(i: usize, size: f32) -> (Vec2, Vec2) {
    let t = i as f32 / RAYS as f32;
    let origin = vec2(-20.0, t * size);
    let direction = Vec2::from_angle((t * 37.0).sin() * 0.6);
    (origin, direction)
}

/// The physics service of a headless game that stepped once over `level`.
fn stepped(level: &[(Vec2, bool)]) -> Physics {
    let taken = Rc::new(RefCell::new(None));
    let slot = taken.clone();
    let level = level.to_vec();
    Game::new("bench", 1, 1)
        .physics(Vec2::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                *slot.borrow_mut() = Some(std::mem::take(&mut ctx.physics));
            }
        })
        .run_headless(2, move |ctx| {
            for &(at, round) in &level {
                let collider = if round {
                    Collider::circle(10.0)
                } else {
                    Collider::rect(vec2(20.0, 20.0))
                };
                ctx.world.spawn((Transform2D::at(at), RigidBody::fixed(), collider));
            }
        });
    taken.borrow_mut().take().expect("stepped")
}

/// 10k ray casts (nearest hit), all-hit ray casts and circle casts over a
/// 1,000-body level, through Ponko's queries and through rapier's directly.
fn rays(b: &mut Bench) {
    let level = level(1_000);
    let size = (level.len() as f32).sqrt().ceil() * 40.0;
    let filter = QueryFilter::default();
    b.run(
        "physics/rays-10k/ponko-cast_ray",
        5,
        || stepped(&level),
        |p| {
            let mut found = 0;
            for i in 0..RAYS {
                let (o, d) = ray(i, size);
                found += usize::from(p.cast_ray(o, d, 2.0 * size, filter).is_some());
            }
            std::hint::black_box(found);
        },
    );
    b.run(
        "physics/rays-10k/ponko-cast_ray_all",
        5,
        || (stepped(&level), Vec::<Hit>::with_capacity(256)),
        |(p, hits)| {
            let mut found = 0;
            for i in 0..RAYS {
                let (o, d) = ray(i, size);
                p.cast_ray_all(o, d, 2.0 * size, filter, hits);
                found += hits.len();
            }
            std::hint::black_box(found);
        },
    );
    b.run(
        "physics/rays-10k/ponko-cast_circle",
        5,
        || stepped(&level),
        |p| {
            let mut found = 0;
            for i in 0..RAYS {
                let (o, d) = ray(i, size);
                found += usize::from(p.cast_circle(o, 4.0, d, 2.0 * size, filter).is_some());
            }
            std::hint::black_box(found);
        },
    );
    b.run(
        "physics/rays-10k/rapier-cast_ray",
        5,
        || {
            let mut w = rp::PhysicsWorld::new();
            w.integration_parameters.length_unit = 50.0;
            for &(at, round) in &level {
                let body = w.insert_body(rp::RigidBodyBuilder::fixed().translation(rp::Vector::new(at.x, at.y)));
                let collider = if round {
                    rp::ColliderBuilder::ball(10.0)
                } else {
                    rp::ColliderBuilder::cuboid(10.0, 10.0)
                };
                w.insert_collider(collider, Some(body));
            }
            w.step();
            w
        },
        |w| {
            let mut found = 0;
            let filter = rp::QueryFilter::default().exclude_sensors();
            for i in 0..RAYS {
                let (o, d) = ray(i, size);
                let ray = rp::Ray::new(rp::Vector::new(o.x, o.y), rp::Vector::new(d.x, d.y));
                found += usize::from(w.cast_ray_and_get_normal(&ray, 2.0 * size, true, filter).is_some());
            }
            std::hint::black_box(found);
        },
    );
}
