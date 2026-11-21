//! The built-in physics and navigation steps as scheduled systems: what
//! they declare, when their collision events reach readers (also readers
//! and steps that run every few ticks), how removals and teleports reach
//! the step, and what the order of physics, navigation and character
//! motion means for the tick. The guide's "Physics, navigation, characters
//! and interpolation: who runs when" section describes the same rules.
//!
//! In builds with debug assertions every test here also checks the steps'
//! declarations: a step that used anything it didn't declare would panic.
#![cfg(any(feature = "physics", feature = "physics3d", feature = "navmesh"))]

#[cfg(any(feature = "physics", feature = "physics3d"))]
use std::cell::RefCell;
#[cfg(any(feature = "physics", feature = "physics3d"))]
use std::rc::Rc;

#[cfg(feature = "physics3d")]
use ponko::access::AccessConflict;
#[cfg(any(feature = "physics", feature = "physics3d"))]
use ponko::access::Service;
#[cfg(any(feature = "physics", feature = "physics3d"))]
use ponko::collision::CollisionStarted;
use ponko::prelude::*;
use ponko::{prelude::Systems, systems};

#[cfg(any(feature = "physics", feature = "physics3d"))]
type Log<T> = Rc<RefCell<Vec<T>>>;

/// The tick each run of this system read a `CollisionStarted` in, once per event.
#[cfg(any(feature = "physics", feature = "physics3d"))]
fn starts(log: &Log<u64>) -> impl FnMut(&mut Ctx) + 'static {
    let log = Rc::clone(log);
    move |ctx: &mut Ctx| {
        let tick = ctx.time.sim_tick();
        for _ in ctx.events.read::<CollisionStarted>() {
            log.borrow_mut().push(tick);
        }
    }
}

/// The moving body of a test. Every body has a velocity (a body requires
/// one), so tests find theirs by this.
#[cfg(any(feature = "physics", feature = "physics3d"))]
#[derive(Component)]
struct Ball;

#[cfg(feature = "physics")]
mod physics2d {
    use super::*;
    use ponko::d2::{Collider, RigidBody, Transform as Transform2D, Velocity};

    const HZ: f32 = 60.0;

    fn sensor() -> (Transform2D, RigidBody, Collider) {
        (
            Transform2D::at(vec2(0.0, 0.0)),
            RigidBody::fixed(),
            Collider::circle(10.0).sensor(),
        )
    }

    /// A ball flying left towards the sensor at the origin.
    fn flying_ball() -> (Transform2D, RigidBody, Collider, Velocity, Ball) {
        (
            Transform2D::at(vec2(100.0, 0.0)),
            RigidBody::dynamic(),
            Collider::circle(5.0),
            Velocity::linear(vec2(-600.0, 0.0)),
            Ball,
        )
    }

    #[test]
    fn the_step_declares_its_inputs_and_keeps_its_id_and_phase() {
        #[derive(Component)]
        struct Score;
        let game = Game::new("t", 1, 1).physics(Vec2::ZERO).add(
            Phase::Physics,
            (|_: &mut Ctx| {}).access(ponko::access::SystemAccess::new().write::<Score>()),
        );
        assert_eq!(game.system_count(Systems::PHYSICS), 1);
        let report = game.access_report();
        let step = report
            .systems
            .iter()
            .find(|s| s.name.ends_with("step_physics"))
            .expect("the step is listed");
        assert_eq!(step.phase, Phase::Physics);
        assert!(!step.enforced, "it keeps `&mut Ctx` and stays sequential");
        let access = step.access.as_ref().expect("declared");
        assert_eq!(access, &systems::step_physics_access());
        assert!(!access.is_exclusive());
        assert_eq!(access.service(Service::Physics), Some(true));
        assert_eq!(access.service(Service::Time), Some(false));
        assert_eq!(access.service(Service::Physics3D), None);
        assert!(!access.records_commands());
        // A system in the same phase that touches none of it doesn't conflict.
        assert_eq!(report.compatible_pairs().count(), 1);
        assert_eq!(
            report.parallel_pairs().count(),
            0,
            "a `Ctx` system never shares a batch"
        );
    }

    #[test]
    fn a_reader_after_the_step_sees_a_contact_in_its_tick_and_one_before_it_a_tick_later() {
        let (early, late, slow) = (Log::default(), Log::default(), Log::default());
        Game::new("t", 1, 1)
            .add(Phase::BeforePhysics, starts(&early))
            .physics(Vec2::ZERO)
            .sim_system(starts(&late))
            .add(Phase::AfterPhysics, starts(&slow).every(4))
            .run_headless(40, |ctx| {
                ctx.world.spawn(sensor());
                ctx.world.spawn(flying_ball());
            });
        let (early, late, slow) = (early.borrow(), late.borrow(), slow.borrow());
        assert_eq!(late.len(), 1, "each reader sees the contact once: {late:?}");
        assert_eq!(early[..], [late[0] + 1], "before the step: the next tick");
        assert_eq!(slow.len(), 1, "a slow reader still sees it once: {slow:?}");
        assert!(
            slow[0] >= late[0] && slow[0] < late[0] + 4,
            "on its first run after the step sent it: sent {}, read {}",
            late[0],
            slow[0]
        );
    }

    /// The ball's x after `ticks`, and the ticks a per-tick reader saw contacts in.
    fn flight(every: u32, ticks: u64) -> (f32, Vec<u64>) {
        let seen = Log::default();
        let mut game = Game::new("t", 1, 1).fixed_hz(HZ as u32).physics(Vec2::ZERO);
        if every > 1 {
            // The same step, run every few ticks with the time they cover.
            game = game
                .remove(Systems::PHYSICS)
                .add(Phase::AfterPhysics, systems::step_physics.every(every));
        }
        let mut ball = None;
        let world = game.sim_system(starts(&seen)).run_headless(ticks, |ctx| {
            ctx.world.spawn(sensor());
            ball = Some(ctx.world.spawn((
                Transform2D::at(vec2(60.0, 0.0)),
                RigidBody::dynamic(),
                Collider::circle(5.0),
                Velocity::linear(vec2(-60.0, 0.0)),
            )));
        });
        let x = world.get::<Transform2D>(ball.unwrap()).unwrap().position.x;
        let seen = seen.borrow().clone();
        (x, seen)
    }

    #[test]
    fn a_step_run_every_other_tick_covers_both_ticks_and_its_contacts_are_read_once() {
        let (each_tick, seen_each) = flight(1, 60);
        let (every_other, seen_other) = flight(2, 60);
        assert!((each_tick - 0.0).abs() < 0.01, "one second at 60 px/s: {each_tick}");
        assert!(
            (every_other - each_tick).abs() < 0.01,
            "half the steps, twice the timestep: {every_other} vs {each_tick}"
        );
        assert_eq!(seen_each.len(), 1);
        assert_eq!(seen_other.len(), 1, "read once, not once per covered tick");
        assert!(
            seen_other[0].abs_diff(seen_each[0]) <= 1,
            "the coarser step reports the contact within its interval: {seen_other:?} vs {seen_each:?}"
        );
    }

    #[test]
    fn teleports_and_removals_before_the_step_reach_it_in_the_same_tick() {
        let seen = Log::default();
        let bodies: Log<(u64, usize)> = Log::default();
        let counts = Rc::clone(&bodies);
        let mut ball = None;
        let world = Game::new("t", 1, 1)
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                let ball = ctx.world.query::<Entity>().filter::<With<Ball>>().iter().next();
                match (ctx.time.sim_tick(), ball) {
                    // Into the sensor: a teleport, by writing the transform.
                    (3, Some(ball)) => ctx.world.get_mut::<Transform2D>(ball).unwrap().position = vec2(2.0, 0.0),
                    // Gone before this tick's step.
                    (6, Some(ball)) => ctx.commands.despawn(ball),
                    _ => {}
                }
            })
            .physics(Vec2::ZERO)
            .sim_system(starts(&seen))
            .sim_system(move |ctx: &mut Ctx| {
                counts
                    .borrow_mut()
                    .push((ctx.time.sim_tick(), ctx.physics.body_count()));
            })
            .run_headless(9, |ctx| {
                ctx.world.spawn(sensor());
                ball = Some(ctx.world.spawn((
                    Transform2D::at(vec2(100.0, 0.0)),
                    RigidBody::dynamic(),
                    Collider::circle(5.0),
                    Ball,
                )));
            });
        assert_eq!(seen.borrow()[..], [3], "the teleport's contact, the tick it happened");
        let bodies = bodies.borrow();
        let at = |tick: u64| bodies.iter().find(|b| b.0 == tick).unwrap().1;
        assert_eq!(at(5), 2);
        assert_eq!(at(6), 1, "the despawned entity's body left with that tick's step");
        assert!(!world.is_alive(ball.unwrap()));
    }

    #[test]
    fn a_removal_after_the_step_reaches_it_on_the_next_tick() {
        let bodies: Log<(u64, usize)> = Log::default();
        let counts = Rc::clone(&bodies);
        Game::new("t", 1, 1)
            .physics(Vec2::ZERO)
            .sim_system(move |ctx: &mut Ctx| {
                counts
                    .borrow_mut()
                    .push((ctx.time.sim_tick(), ctx.physics.body_count()));
                if ctx.time.sim_tick() == 4 {
                    let ball = ctx
                        .world
                        .query::<Entity>()
                        .filter::<With<Ball>>()
                        .iter()
                        .next()
                        .unwrap();
                    ctx.commands.despawn(ball);
                }
            })
            .run_headless(7, |ctx| {
                ctx.world.spawn(sensor());
                ctx.world.spawn(flying_ball());
            });
        let bodies = bodies.borrow();
        let at = |tick: u64| bodies.iter().find(|b| b.0 == tick).unwrap().1;
        assert_eq!(at(4), 2, "the step had already run");
        assert_eq!(at(5), 1);
    }
}

#[cfg(feature = "physics3d")]
mod physics3d {
    use super::*;
    use ponko::d3::{
        Character as Character3D, Collider as Collider3D, RigidBody as RigidBody3D, Transform as Transform3D,
    };

    fn sensor() -> (Transform3D, RigidBody3D, Collider3D) {
        (
            Transform3D::default(),
            RigidBody3D::fixed(),
            Collider3D::sphere(1.0).sensor(),
        )
    }

    #[test]
    fn the_step_and_the_blend_declare_their_inputs_and_keep_their_ids_and_phases() {
        let game = Game::new("t", 1, 1).physics3d(Vec3::ZERO);
        assert_eq!(game.system_count(Systems::PHYSICS3D), 1);
        assert_eq!(game.system_count(Systems::INTERPOLATE_PHYSICS3D), 1);
        let report = game.access_report();
        let find = |name: &str| {
            report
                .systems
                .iter()
                .find(|s| s.name.ends_with(name))
                .unwrap_or_else(|| panic!("{name} is listed"))
        };
        let step = find("::step_physics3d");
        assert_eq!(step.phase, Phase::Physics);
        assert_eq!(step.access.as_ref(), Some(&systems::step_physics3d_access()));
        let access = step.access.as_ref().unwrap();
        assert!(!access.is_exclusive() && !access.records_commands());
        assert_eq!(access.service(Service::Physics3D), Some(true));
        assert_eq!(access.service(Service::Time), Some(false));
        let blend = find("::interpolate_physics3d");
        assert_eq!(blend.phase, Phase::Frame);
        assert_eq!(blend.access.as_ref(), Some(&systems::interpolate_physics3d_access()));
        // The blend writes poses the step wrote: they could never swap.
        let conflicts = systems::step_physics3d_access().conflicts_with(&systems::interpolate_physics3d_access());
        assert!(conflicts.contains(&AccessConflict::Service(Service::Physics3D)));
    }

    #[cfg(feature = "physics")]
    #[test]
    fn the_2d_and_3d_steps_only_share_the_collision_events() {
        let conflicts = systems::step_physics_access().conflicts_with(&systems::step_physics3d_access());
        assert!(!conflicts.is_empty());
        assert!(
            conflicts.iter().all(|c| matches!(c, AccessConflict::Event(_))),
            "separate worlds and components; both send the collision events: {conflicts:?}"
        );
    }

    #[test]
    fn contacts_teleports_and_removals_keep_their_timing_through_the_schedule() {
        let (early, late, slow) = (Log::default(), Log::default(), Log::default());
        let bodies: Log<(u64, usize)> = Log::default();
        let counts = Rc::clone(&bodies);
        let mut ball = None;
        let world = Game::new("t", 1, 1)
            .add(Phase::BeforePhysics, starts(&early))
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                let ball = ctx.world.query::<Entity>().filter::<With<Ball>>().iter().next();
                match (ctx.time.sim_tick(), ball) {
                    (3, Some(ball)) => ctx.world.get_mut::<Transform3D>(ball).unwrap().position = vec3(0.2, 0.0, 0.0),
                    (6, Some(ball)) => ctx.commands.despawn(ball),
                    _ => {}
                }
            })
            .physics3d(Vec3::ZERO)
            .sim_system(starts(&late))
            .add(Phase::AfterPhysics, starts(&slow).every(4))
            .sim_system(move |ctx: &mut Ctx| {
                counts
                    .borrow_mut()
                    .push((ctx.time.sim_tick(), ctx.physics3d.body_count()));
            })
            .run_headless(12, |ctx| {
                ctx.world.spawn(sensor());
                ball = Some(ctx.world.spawn((
                    Transform3D::at(vec3(10.0, 0.0, 0.0)),
                    RigidBody3D::dynamic(),
                    Collider3D::sphere(0.5),
                    Ball,
                )));
            });
        assert_eq!(late.borrow()[..], [3], "the teleport's contact, the tick it happened");
        assert_eq!(early.borrow()[..], [4], "a reader before the step: the next tick");
        let slow = slow.borrow();
        assert_eq!(slow.len(), 1, "a slow reader sees it once: {slow:?}");
        assert!((3..7).contains(&slow[0]), "{slow:?}");
        let bodies = bodies.borrow();
        let at = |tick: u64| bodies.iter().find(|b| b.0 == tick).unwrap().1;
        assert_eq!(
            (at(5), at(6)),
            (2, 1),
            "the body left with the step of the despawn's tick"
        );
        assert!(!world.is_alive(ball.unwrap()));
    }

    /// How far a character walked along +X in the tick its speed was set,
    /// as seen by a system after the step, when the speed is set in `phase`.
    fn first_step(phase: Phase) -> f32 {
        let moved: Log<f32> = Log::default();
        let seen = Rc::clone(&moved);
        Game::new("t", 1, 1)
            .add(phase, |ctx: &mut Ctx| {
                if ctx.time.sim_tick() == 30 {
                    ctx.world.query::<&mut Character3D>().for_each(|c| c.velocity.x = 2.0);
                }
            })
            .physics3d(vec3(0.0, -9.81, 0.0))
            .add(Phase::SimLast, move |ctx: &mut Ctx| {
                if ctx.time.sim_tick() == 30 {
                    let x = ctx
                        .world
                        .query::<&Transform3D>()
                        .filter::<With<Character3D>>()
                        .iter()
                        .next()
                        .unwrap()
                        .position
                        .x;
                    seen.borrow_mut().push(x);
                }
            })
            .run_headless(40, |ctx| {
                ctx.world.spawn((
                    Transform3D::at(vec3(0.0, -0.5, 0.0)),
                    RigidBody3D::fixed(),
                    Collider3D::cuboid(vec3(40.0, 1.0, 40.0)),
                ));
                ctx.world
                    .spawn((Transform3D::at(vec3(0.0, 0.05, 0.0)), Character3D::new(1.8, 0.3)));
            });
        let moved = moved.borrow();
        moved[0]
    }

    #[test]
    fn character_input_before_the_step_moves_it_that_tick_and_input_after_it_the_next() {
        let before = first_step(Phase::BeforePhysics);
        let after = first_step(Phase::AfterPhysics);
        assert!((before - 2.0 / 50.0).abs() < 1e-3, "one 50 Hz step at 2 m/s: {before}");
        assert!(after.abs() < 1e-4, "the step had already run: {after}");
    }

    #[cfg(feature = "navmesh")]
    mod navigation {
        use super::*;
        use ponko::d3::{NavAgent as NavAgent3D, NavMesh as NavMesh3D, QueryFilter as QueryFilter3D};
        use ponko::nav::{NavBakeSettings, NavSource};
        use ponko::render::MeshData;

        /// Whether, in each tick from 40 on, a ray straight down at the
        /// walking agent's transform found its (small) kinematic collider:
        /// that is, whether physics queries saw the agent where navigation
        /// put it this tick.
        fn body_follows(navigation: Phase) -> Vec<bool> {
            let found: Log<bool> = Log::default();
            let seen = Rc::clone(&found);
            Game::new("t", 1, 1)
                .fixed_hz(60)
                .add(
                    navigation,
                    systems::step_navmesh3d.access(systems::step_navmesh3d_access()),
                )
                .physics3d(Vec3::ZERO)
                .add(Phase::SimLast, move |ctx: &mut Ctx| {
                    if ctx.time.sim_tick() < 40 {
                        return;
                    }
                    let (walker, at) = ctx
                        .world
                        .query::<(Entity, &Transform3D)>()
                        .filter::<With<NavAgent3D>>()
                        .iter()
                        .map(|(e, t)| (e, t.position))
                        .next()
                        .unwrap();
                    let hit = ctx.physics3d.cast_ray(
                        at + vec3(0.0, 2.0, 0.0),
                        vec3(0.0, -1.0, 0.0),
                        4.0,
                        QueryFilter3D::default(),
                    );
                    seen.borrow_mut().push(hit.is_some_and(|h| h.entity == walker));
                })
                .run_headless(80, |ctx| {
                    let floor = NavSource::from_mesh(&MeshData::plane(20.0), Transform3D::default());
                    ctx.world
                        .insert_resource(NavMesh3D::bake(&[floor], NavBakeSettings::default()).unwrap());
                    let agent = NavAgent3D {
                        destination: Some(vec3(8.0, 0.0, 0.0)),
                        ..Default::default()
                    };
                    ctx.world.spawn((
                        Transform3D::at(vec3(-8.0, 0.0, 0.0)),
                        agent,
                        RigidBody3D::kinematic(),
                        Collider3D::sphere(0.02),
                    ));
                });
            let found = found.borrow().clone();
            assert_eq!(found.len(), 40);
            found
        }

        #[test]
        fn navigation_before_physics_moves_the_body_that_tick_and_after_it_a_tick_late() {
            assert!(
                body_follows(Phase::BeforePhysics).iter().all(|&hit| hit),
                "the step picks the agent's new transform up as a teleport"
            );
            assert!(
                body_follows(Phase::AfterPhysics).iter().all(|&hit| !hit),
                "the body is still where the last step left it: 5 cm behind a 2 cm ball"
            );
        }

        #[test]
        fn navigation_and_physics_conflict_on_the_transform_only() {
            let conflicts = systems::step_navmesh3d_access().conflicts_with(&systems::step_physics3d_access());
            assert_eq!(conflicts.len(), 1, "{conflicts:?}");
            assert!(
                matches!(conflicts[0], AccessConflict::Component(name) if name.ends_with("Transform3D") || name.ends_with("Transform")),
                "{conflicts:?}"
            );
        }
    }
}

#[cfg(feature = "navmesh")]
mod navmesh {
    use super::*;
    use ponko::d3::{NavAgent as NavAgent3D, NavMesh as NavMesh3D, Transform as Transform3D};
    use ponko::nav::{NavBakeSettings, NavSource};
    use ponko::render::MeshData;

    fn baked() -> NavMesh3D {
        let floor = NavSource::from_mesh(&MeshData::plane(20.0), Transform3D::default());
        NavMesh3D::bake(&[floor], NavBakeSettings::default()).unwrap()
    }

    fn walker(to: Vec3) -> (Transform3D, NavAgent3D) {
        let agent = NavAgent3D {
            destination: Some(to),
            ..Default::default()
        };
        (Transform3D::at(vec3(-5.0, 0.0, 0.0)), agent)
    }

    #[test]
    fn the_declared_step_walks_agents_like_the_plain_one() {
        let run = |declared: bool| {
            let game = Game::new("t", 1, 1).fixed_hz(60);
            let game = if declared {
                game.add(
                    Phase::AfterPhysics,
                    systems::step_navmesh3d.access(systems::step_navmesh3d_access()),
                )
            } else {
                game.sim_system(systems::step_navmesh3d)
            };
            assert_eq!(game.system_count(Systems::STEP_NAVMESH3D), 1);
            let mut world = game.run_headless(120, |ctx| {
                ctx.world.insert_resource(baked());
                ctx.world.spawn(walker(vec3(5.0, 0.0, 0.0)));
                // With avoidance: the crowd path of the step.
                let (t, mut agent) = walker(vec3(5.0, 0.0, 2.0));
                agent.avoidance = true;
                ctx.world.spawn((t, agent));
            });
            let mut at: Vec<[u32; 3]> = world
                .query::<&Transform3D>()
                .iter()
                .map(|t| t.position.to_array().map(f32::to_bits))
                .collect();
            at.sort_unstable();
            at
        };
        let declared = run(true);
        assert_eq!(declared, run(false));
        assert_ne!(declared[0], declared[1]);
    }

    #[test]
    fn a_teleported_agent_replans_from_where_it_was_put_and_a_removed_one_is_dropped() {
        let mut ids = (None, None);
        let mut world = Game::new("t", 1, 1)
            .fixed_hz(60)
            .add(Phase::BeforePhysics, |ctx: &mut Ctx| {
                if ctx.time.sim_tick() != 30 {
                    return;
                }
                let mut agents: Vec<Entity> = ctx
                    .world
                    .query::<Entity>()
                    .filter::<With<NavAgent3D>>()
                    .iter()
                    .collect();
                agents.sort_unstable();
                // The first one jumps past its goal; the second is removed.
                ctx.world.get_mut::<Transform3D>(agents[0]).unwrap().position = vec3(8.0, 0.0, 0.0);
                ctx.commands.despawn(agents[1]);
            })
            .add(
                Phase::AfterPhysics,
                systems::step_navmesh3d.access(systems::step_navmesh3d_access()),
            )
            .run_headless(240, |ctx| {
                ctx.world.insert_resource(baked());
                ids.0 = Some(ctx.world.spawn(walker(vec3(5.0, 0.0, 0.0))));
                ids.1 = Some(ctx.world.spawn(walker(vec3(5.0, 0.0, 3.0))));
            });
        let (jumper, removed) = (ids.0.unwrap(), ids.1.unwrap());
        assert!(jumper < removed);
        assert!(!world.is_alive(removed));
        let p = world.get::<Transform3D>(jumper).unwrap().position;
        assert!(
            p.distance(vec3(5.0, p.y, 0.0)) < 0.25,
            "walked back from 8 to its goal at 5: {p:?}"
        );
        assert_eq!(world.query::<&NavAgent3D>().iter().count(), 1);
    }
}
