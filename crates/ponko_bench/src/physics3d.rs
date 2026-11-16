//! 3D physics: the same pile of boxes stepped through Ponko
//! (`Game::physics3d`, components synced every tick) and through rapier3d
//! directly, plus characters walking and cars driving over heightfield
//! terrain, and 10,000 queries (rays, shape casts, closest points,
//! penetrations) through a level of fixed bodies.
use std::cell::RefCell;
use std::rc::Rc;

use ponko::d3::Character as Character3D;
use ponko::d3::Collider as Collider3D;
use ponko::d3::Physics as Physics3D;
use ponko::d3::RigidBody as RigidBody3D;
use ponko::d3::Transform as Transform3D;
use ponko::d3::Vehicle as Vehicle3D;
use ponko::d3::Wheel as Wheel3D;
use ponko::d3::{Hit as Hit3D, Penetration as Penetration3D, QueryFilter as QueryFilter3D};
use ponko::prelude::{Ctx, Game, Quat, Vec3, vec3};
use rapier3d::prelude as rp;

use crate::Bench;
use crate::alloc::live_bytes;

pub const TICKS: u64 = 250; // 5 s at 50 Hz

/// `n` 0.5 m boxes in layers above a 1 m slab floor, inside four walls.
fn scene(n: usize) -> (Vec<Vec3>, f32) {
    let side = ((n as f32 / 4.0).sqrt().ceil() as usize).max(1);
    let width = side as f32 * 0.6;
    let boxes = (0..n)
        .map(|i| {
            let layer = i / (side * side);
            let (x, z) = ((i % side) as f32, (i / side % side) as f32);
            vec3(
                x * 0.6 - width / 2.0 + 0.3,
                0.5 + layer as f32 * 0.6,
                z * 0.6 - width / 2.0 + 0.3,
            )
        })
        .collect();
    (boxes, width)
}

/// Walls as (center, size).
fn walls(width: f32) -> [(Vec3, Vec3); 5] {
    let h = 20.0;
    [
        (vec3(0.0, -0.5, 0.0), vec3(width + 2.0, 1.0, width + 2.0)),
        (vec3(-width / 2.0 - 0.5, h / 2.0, 0.0), vec3(1.0, h, width + 2.0)),
        (vec3(width / 2.0 + 0.5, h / 2.0, 0.0), vec3(1.0, h, width + 2.0)),
        (vec3(0.0, h / 2.0, -width / 2.0 - 0.5), vec3(width + 2.0, h, 1.0)),
        (vec3(0.0, h / 2.0, width / 2.0 + 0.5), vec3(width + 2.0, h, 1.0)),
    ]
}

pub fn run(b: &mut Bench) {
    queries(b);
    for n in [1_000, 10_000] {
        let (boxes, width) = scene(n);
        b.run(
            &format!("physics3d/ponko-{n}"),
            3,
            || (),
            |_| {
                let world = Game::new("bench", 1, 1)
                    .physics3d(vec3(0.0, -9.81, 0.0))
                    .run_headless(TICKS, |ctx| {
                        for (at, size) in walls(width) {
                            ctx.world
                                .spawn((Transform3D::at(at), RigidBody3D::fixed(), Collider3D::cuboid(size)));
                        }
                        for &p in &boxes {
                            ctx.world.spawn((
                                Transform3D::at(p),
                                RigidBody3D::dynamic(),
                                Collider3D::cuboid(Vec3::splat(0.5)),
                            ));
                        }
                    });
                std::hint::black_box(world);
            },
        );
        b.run(
            &format!("physics3d/rapier-{n}"),
            3,
            || (),
            |_| {
                let mut w = rp::PhysicsWorld::new();
                w.integration_parameters.dt = 1.0 / 50.0;
                w.gravity = rp::Vector::new(0.0, -9.81, 0.0);
                for (at, size) in walls(width) {
                    let body =
                        w.insert_body(rp::RigidBodyBuilder::fixed().translation(rp::Vector::new(at.x, at.y, at.z)));
                    w.insert_collider(
                        rp::ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0),
                        Some(body),
                    );
                }
                for &p in &boxes {
                    let body =
                        w.insert_body(rp::RigidBodyBuilder::dynamic().translation(rp::Vector::new(p.x, p.y, p.z)));
                    w.insert_collider(
                        rp::ColliderBuilder::cuboid(0.25, 0.25, 0.25)
                            .density(1000.0)
                            .active_events(rp::ActiveEvents::COLLISION_EVENTS),
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

    // Characters walking in circles over rolling terrain (64 x 64 m).
    for n in [100, 1_000] {
        b.run(
            &format!("physics3d/characters-{n}"),
            3,
            || (),
            |_| {
                let world = Game::new("bench", 1, 1)
                    .sim_system(|ctx: &mut ponko::prelude::Ctx| {
                        let t = ctx.time.sim_tick() as f32 * 0.02;
                        ctx.world.query::<&mut Character3D>().for_each(|c| {
                            let a = t + c.radius * 100.0;
                            c.velocity.x = 3.0 * a.cos();
                            c.velocity.z = 3.0 * a.sin();
                        });
                    })
                    .physics3d(vec3(0.0, -9.81, 0.0))
                    .run_headless(TICKS, |ctx| {
                        let cells = 65;
                        let heights: Vec<f32> = (0..cells * cells)
                            .map(|i| {
                                let (x, z) = ((i % cells) as f32, (i / cells) as f32);
                                (x * 0.2).sin() * 1.5 + (z * 0.15).cos()
                            })
                            .collect();
                        let ground = ctx.physics3d.heightfield(&heights, 64.0);
                        ctx.world.spawn((
                            Transform3D::at(vec3(-32.0, 0.0, -32.0)),
                            RigidBody3D::fixed(),
                            Collider3D::mesh(ground),
                        ));
                        let side = (n as f32).sqrt().ceil() as usize;
                        for i in 0..n {
                            let (x, z) = ((i % side) as f32, (i / side) as f32);
                            let mut c = Character3D::new(1.8, 0.3 + (i % 7) as f32 * 0.001);
                            c.push_mass = 0.0;
                            ctx.world
                                .spawn((Transform3D::at(vec3(x * 1.5 - 24.0, 4.0, z * 1.5 - 24.0)), c));
                        }
                    });
                std::hint::black_box(world);
            },
        );
    }
    // Cars on four raycast wheels driving in circles over rolling terrain
    // (512 x 512 m); 0 cars is the terrain alone.
    for n in [0, 1, 10, 100] {
        b.run(
            &format!("physics3d/vehicles-{n}"),
            5,
            || (),
            |_| {
                let world = Game::new("bench", 1, 1)
                    .sim_system(|ctx: &mut ponko::prelude::Ctx| {
                        ctx.world.query::<&mut Vehicle3D>().for_each(|v| {
                            for (i, w) in v.wheels_mut().iter_mut().enumerate() {
                                if i < 2 {
                                    w.steer_angle = 0.25;
                                } else {
                                    w.motor_torque = 400.0;
                                }
                            }
                        });
                    })
                    .physics3d(vec3(0.0, -9.81, 0.0))
                    .run_headless(TICKS, |ctx| {
                        let cells = 129;
                        let heights: Vec<f32> = (0..cells * cells)
                            .map(|i| {
                                let (x, z) = ((i % cells) as f32 * 4.0, (i / cells) as f32 * 4.0);
                                (x * 0.03).sin() * 3.0 + (z * 0.025).cos() * 4.0
                            })
                            .collect();
                        let ground = ctx.physics3d.heightfield(&heights, 512.0);
                        ctx.world.spawn((
                            Transform3D::at(vec3(-256.0, 0.0, -256.0)),
                            RigidBody3D::fixed(),
                            Collider3D::mesh(ground),
                        ));
                        for i in 0..n {
                            let (x, z) = ((i % 10) as f32 * 40.0 - 180.0, (i / 10) as f32 * 40.0 - 180.0);
                            let mut car = Vehicle3D::new();
                            for (wx, wz) in [(-0.8, -1.3), (0.8, -1.3), (-0.8, 1.3), (0.8, 1.3)] {
                                car = car.with_wheel(Wheel3D::new(vec3(wx, -0.3, wz), 0.35));
                            }
                            ctx.world.spawn((
                                Transform3D::at(vec3(x, 9.0, z)),
                                RigidBody3D::dynamic().mass(1200.0).center_of_mass(vec3(0.0, -0.3, 0.0)),
                                Collider3D::cuboid(vec3(1.8, 0.6, 4.2)),
                                car,
                            ));
                        }
                    });
                std::hint::black_box(world);
            },
        );
    }

    // Looking bodies up by entity (`mass`, `apply_impulse`, ...; the same
    // lookup `step_physics3d` does per entity to sync it): 10 000 fixed
    // boxes, each looked up 20 times per tick, for 50 ticks. Prints the time per
    // lookup (the loops alone).
    b.run(
        "physics3d/lookups-10000",
        5,
        || (),
        |_| {
            let world = Game::new("bench", 1, 1)
                .sim_system(|ctx: &mut ponko::prelude::Ctx| {
                    let mut found = 0.0f32;
                    let bodies = ctx.world.resource::<Bodies>();
                    let t = std::time::Instant::now();
                    for _ in 0..20 {
                        for &e in &bodies.0 {
                            found += ctx.physics3d.mass(e).unwrap_or(0.0);
                        }
                    }
                    let ns = t.elapsed().as_nanos() as i64;
                    LOOKUP_NS.fetch_add(ns, std::sync::atomic::Ordering::Relaxed);
                    std::hint::black_box(found);
                })
                .physics3d(Vec3::ZERO)
                .run_headless(50, |ctx| {
                    let bodies = (0..10_000)
                        .map(|i| {
                            let at = vec3((i % 100) as f32 * 2.0, 0.0, (i / 100) as f32 * 2.0);
                            ctx.world
                                .spawn((Transform3D::at(at), RigidBody3D::fixed(), Collider3D::cuboid(Vec3::ONE)))
                        })
                        .collect();
                    ctx.world.insert_resource(Bodies(bodies));
                });
            let ns = LOOKUP_NS.swap(0, std::sync::atomic::Ordering::Relaxed);
            println!("    physics3d/lookups-10000: {:.1} ns per lookup", ns as f64 / 1e7);
            std::hint::black_box(world);
        },
    );

    // Streaming churn: each tick 200 bodies leave and 200 come, among 400
    // new plain entities (scenery that stays), so entity indices climb while
    // the live bodies stay at 200. Prints the live heap on the last tick
    // (everything: the world's entities, rapier, the links table).
    b.run(
        "physics3d/churn-200",
        3,
        || (),
        |_| {
            let world = Game::new("bench", 1, 1)
                .sim_system(|ctx: &mut ponko::prelude::Ctx| {
                    let mut bodies = std::mem::take(&mut ctx.world.resource_mut::<Bodies>().0);
                    for e in bodies.drain(..) {
                        ctx.world.despawn(e);
                    }
                    let tick = ctx.time.sim_tick() as f32;
                    for i in 0..200 {
                        for _ in 0..2 {
                            ctx.world.spawn((Transform3D::default(),));
                        }
                        let at = vec3((i % 20) as f32 * 3.0, 2.0 + tick * 1e-3, (i / 20) as f32 * 3.0);
                        bodies.push(ctx.world.spawn((
                            Transform3D::at(at),
                            RigidBody3D::dynamic(),
                            Collider3D::sphere(0.5),
                        )));
                    }
                    ctx.world.resource_mut::<Bodies>().0 = bodies;
                    // The last tick's store is the one read.
                    LAST_LIVE.store(live_bytes(), std::sync::atomic::Ordering::Relaxed);
                })
                .physics3d(Vec3::ZERO)
                .run_headless(TICKS, |ctx| {
                    ctx.world.insert_resource(Bodies(Vec::new()));
                });
            let at_end = LAST_LIVE.load(std::sync::atomic::Ordering::Relaxed);
            let after = live_bytes();
            drop(world);
            println!(
                "    physics3d/churn-200: {:.1} MB live heap on the last tick ({:.1} MB once the physics was dropped)",
                at_end as f64 / 1e6,
                after as f64 / 1e6
            );
        },
    );
}

struct Bodies(Vec<ponko::prelude::Entity>);

/// Time spent in the lookups benchmark's loops.
static LOOKUP_NS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// The process's live heap on the churn benchmark's last tick.
static LAST_LIVE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Queries per run of the query benchmarks (the 2D ones run as many).
pub const QUERIES: usize = 10_000;

/// A 3D level of fixed bodies: `n` 1 m boxes and spheres on a 2 m grid on
/// the ground (y = 0), every third one a sphere.
fn level(n: usize) -> Vec<(Vec3, bool)> {
    let per_row = (n as f32).sqrt().ceil() as usize;
    (0..n)
        .map(|i| {
            (
                vec3((i % per_row) as f32 * 2.0, 0.0, (i / per_row) as f32 * 2.0),
                i % 3 == 0,
            )
        })
        .collect()
}

/// Query `i` of a run: from points along the level's -X edge, fanning
/// across it in the ground plane (deterministic, no RNG), as the 2D
/// benchmark's rays do.
fn query(i: usize, size: f32) -> (Vec3, Vec3) {
    let t = i as f32 / QUERIES as f32;
    let origin = vec3(-1.0, 0.0, t * size);
    let angle = (t * 37.0).sin() * 0.6;
    (origin, vec3(angle.cos(), 0.0, angle.sin()))
}

/// The 3D physics of a headless game that stepped once over `level`.
fn stepped(level: &[(Vec3, bool)]) -> Physics3D {
    let taken = Rc::new(RefCell::new(None));
    let slot = taken.clone();
    let level = level.to_vec();
    Game::new("bench", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                *slot.borrow_mut() = Some(std::mem::take(&mut ctx.physics3d));
            }
        })
        .run_headless(2, move |ctx| {
            for &(at, round) in &level {
                let collider = if round {
                    Collider3D::sphere(0.5)
                } else {
                    Collider3D::cuboid(Vec3::ONE)
                };
                ctx.world.spawn((Transform3D::at(at), RigidBody3D::fixed(), collider));
            }
        });
    taken.borrow_mut().take().expect("stepped")
}

/// 10k ray casts (nearest hit), all-hit ray casts, sphere and turned box
/// casts, closest points and penetrations over a 1,000-body level, through
/// Ponko's queries, and ray and box casts through rapier's directly. The
/// same level, queries and counts as the 2D `physics/rays-10k`.
fn queries(b: &mut Bench) {
    let level = level(1_000);
    let size = (level.len() as f32).sqrt().ceil() * 2.0;
    let filter = QueryFilter3D::default();
    let turn = Quat::from_rotation_y(0.3);
    let mut count = |name: &str, op: &mut dyn FnMut(&Physics3D, usize) -> usize| {
        b.run(
            &format!("physics3d/queries-10k/ponko-{name}"),
            5,
            || stepped(&level),
            |p| {
                let found: usize = (0..QUERIES).map(|i| op(p, i)).sum();
                std::hint::black_box(found);
            },
        );
    };
    let range = 2.0 * size;
    count("cast_ray", &mut |p, i| {
        let (o, d) = query(i, size);
        usize::from(p.cast_ray(o, d, range, filter).is_some())
    });
    let mut hits = Vec::<Hit3D>::with_capacity(256);
    count("cast_ray_all", &mut |p, i| {
        let (o, d) = query(i, size);
        p.cast_ray_all(o, d, range, filter, &mut hits);
        hits.len()
    });
    count("cast_sphere", &mut |p, i| {
        let (o, d) = query(i, size);
        usize::from(p.cast_sphere(o, 0.2, d, range, filter).is_some())
    });
    count("cast_box", &mut |p, i| {
        let (o, d) = query(i, size);
        usize::from(p.cast_box(o, Vec3::splat(0.4), turn, d, range, filter).is_some())
    });
    count("closest_point", &mut |p, i| {
        let (o, d) = query(i, size);
        // Points inside the level, between and inside the bodies.
        let at = o + d * (i % 64) as f32 * 0.5 + Vec3::Y * 0.3;
        usize::from(p.closest_point(at, 4.0, filter).is_some())
    });
    let mut pushes = Vec::<Penetration3D>::with_capacity(64);
    let shape = Collider3D::cuboid(Vec3::splat(0.8)).shape;
    count("penetrations", &mut |p, i| {
        let (o, d) = query(i, size);
        let at = o + d * (i % 64) as f32 * 0.5;
        p.penetrations(shape, at, turn, filter, &mut pushes);
        pushes.len()
    });

    let rapier_world = || {
        let mut w = rp::PhysicsWorld::new();
        for &(at, round) in &level {
            let body = w.insert_body(rp::RigidBodyBuilder::fixed().translation(rp::Vector::new(at.x, at.y, at.z)));
            let collider = if round {
                rp::ColliderBuilder::ball(0.5)
            } else {
                rp::ColliderBuilder::cuboid(0.5, 0.5, 0.5)
            };
            w.insert_collider(collider, Some(body));
        }
        w.step();
        w
    };
    let to_rp = |v: Vec3| rp::Vector::new(v.x, v.y, v.z);
    b.run("physics3d/queries-10k/rapier-cast_ray", 5, rapier_world, |w| {
        let filter = rp::QueryFilter::default().exclude_sensors();
        let mut found = 0;
        for i in 0..QUERIES {
            let (o, d) = query(i, size);
            let ray = rp::Ray::new(to_rp(o), to_rp(d));
            found += usize::from(w.cast_ray_and_get_normal(&ray, range, true, filter).is_some());
        }
        std::hint::black_box(found);
    });
    b.run("physics3d/queries-10k/rapier-cast_box", 5, rapier_world, |w| {
        let filter = rp::QueryFilter::default().exclude_sensors();
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: range,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: true,
        };
        let cube = rp::Cuboid::new(rp::Vector::splat(0.2));
        let mut found = 0;
        for i in 0..QUERIES {
            let (o, d) = query(i, size);
            let pose = rp::Pose::from_parts(to_rp(o), rp::Rotation::from_xyzw(turn.x, turn.y, turn.z, turn.w));
            found += usize::from(w.cast_shape(&pose, to_rp(d), &cube, options, filter).is_some());
        }
        std::hint::black_box(found);
    });
}
