#![cfg(feature = "navmesh")]
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::Transform as Transform3D;
#[allow(unused_imports)]
#[cfg(feature = "navmesh")]
use ponko::d3::{NavAgent as NavAgent3D, NavMesh as NavMesh3D, NavObstacle as NavObstacle3D};
#[allow(unused_imports)]
#[cfg(feature = "navmesh")]
use ponko::nav::{NavBakeSettings, NavCrowdStats, NavSource};
use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::MeshData;

fn source(mesh: MeshData, position: Vec3) -> NavSource {
    NavSource::from_mesh(&mesh, Transform3D::at(position))
}
fn floor() -> NavSource {
    source(MeshData::plane(20.0), Vec3::ZERO)
}
fn bake(sources: &[NavSource]) -> NavMesh3D {
    NavMesh3D::bake(sources, NavBakeSettings::default()).unwrap()
}
fn carving_bake(sources: &[NavSource]) -> NavMesh3D {
    let settings = NavBakeSettings {
        carving: true,
        ..Default::default()
    };
    NavMesh3D::bake(sources, settings).unwrap()
}
#[test]
fn snaps_and_rejects_distant_points() {
    let nav = bake(&[floor()]);
    let p = nav.sample_position(vec3(1.0, 1.0, 2.0), 2.0).unwrap();
    assert!(p.distance(vec3(1.0, 0.0, 2.0)) < 0.2);
    assert!(nav.sample_position(Vec3::splat(100.0), 1.0).is_none());
}
#[test]
fn paths_around_a_solid_box() {
    let nav = bake(&[floor(), source(MeshData::cube(4.0), vec3(0.0, 2.0, 0.0))]);
    let path = nav.find_path(vec3(-7.0, 0.0, 0.0), vec3(7.0, 0.0, 0.0)).unwrap();
    assert!(path.iter().any(|p| p.z.abs() > 2.0), "{path:?}");
    assert!(path.len() >= 3);
}
#[test]
fn steep_slopes_are_not_walkable() {
    let mesh = MeshData::plane(12.0);
    let slope = NavSource::from_mesh(
        &mesh,
        Transform3D::default().rotated(Quat::from_rotation_x(60_f32.to_radians())),
    );
    let nav = bake(&[slope]);
    assert!(nav.sample_position(Vec3::ZERO, 1.0).is_none());
}
#[test]
fn low_steps_connect_but_tall_steps_do_not() {
    for (height, connected) in [(0.2, true), (1.0, false)] {
        let a = source(MeshData::plane(8.0), vec3(-4.0, 0.0, 0.0));
        let b = source(MeshData::plane(8.0), vec3(4.0, height, 0.0));
        let nav = bake(&[a, b]);
        assert_eq!(
            nav.find_path(vec3(-3.0, 0.0, 0.0), vec3(3.0, height, 0.0)).is_some(),
            connected
        );
    }
}
#[test]
fn obstacles_block_and_removal_restores_paths() {
    let mut nav = bake(&[floor()]);
    let a = vec3(-7.0, 0.0, 0.0);
    let b = vec3(7.0, 0.0, 0.0);
    assert!(nav.find_path(a, b).is_some());
    nav.set_obstacles(&[NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 2.0, 20.0))]);
    assert!(nav.find_path(a, b).is_none());
    nav.set_obstacles(&[]);
    assert!(nav.find_path(a, b).is_some());
}
// Built the pre-struct-update way on purpose: setting fields on a default
// keeps compiling and working.
#[test]
#[allow(clippy::field_reassign_with_default)]
fn agent_reaches_destination_headless() {
    let mut world = Game::new("navigation", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .run_headless(360, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            let mut agent = NavAgent3D::default();
            agent.destination = Some(vec3(5.0, 0.0, 0.0));
            ctx.world.spawn((Transform3D::at(vec3(-5.0, 0.0, 0.0)), agent));
        });
    let p = world.query::<&Transform3D>().iter().next().unwrap().position;
    assert!(p.distance(vec3(5.0, 0.1, 0.0)) < 0.25, "{p:?}");
}
#[test]
fn struct_update_agent_keeps_named_settings_and_defaults_the_rest() {
    let agent = NavAgent3D {
        speed: 2.0,
        radius: 0.25,
        destination: Some(vec3(5.0, 0.0, 0.0)),
        ..Default::default()
    };
    let default = NavAgent3D::default();
    assert_eq!(agent.speed, 2.0);
    assert_eq!(agent.radius, 0.25);
    assert_eq!(agent.destination, Some(vec3(5.0, 0.0, 0.0)));
    assert_eq!(agent.acceleration, default.acceleration);
    assert_eq!(agent.avoidance, default.avoidance);
    assert_eq!(agent.avoidance_weight, default.avoidance_weight);
    assert_eq!(agent.neighbour_distance, default.neighbour_distance);

    // The settings it was built with are the ones the step uses: one second
    // at 2 units per second (after 0.2 s of acceleration) covers about 1.8.
    let start = vec3(-5.0, 0.0, 0.0);
    let mut world = Game::new("navigation", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .run_headless(60, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            ctx.world.spawn((Transform3D::at(start), agent));
        });
    let p = world.query::<&Transform3D>().iter().next().unwrap().position;
    let travelled = (p - start).with_y(0.0).length();
    assert!((1.6..=2.0).contains(&travelled), "{travelled}");
}
#[test]
fn agent_built_from_a_walking_agent_plans_its_own_path() {
    struct Walker {
        entity: Entity,
        tick: u32,
    }
    fn copy_at_tick_30(ctx: &mut Ctx) {
        let w = ctx.world.resource_mut::<Walker>();
        w.tick += 1;
        if w.tick != 30 {
            return;
        }
        let walker = w.entity;
        let walking = ctx.world.get::<NavAgent3D>(walker).unwrap().clone();
        // The copy carries the walker's progress; the step must replan it.
        let copy = NavAgent3D {
            destination: Some(vec3(5.0, 0.0, 4.0)),
            ..walking
        };
        ctx.world.spawn((Transform3D::at(vec3(-5.0, 0.0, 4.0)), copy));
    }
    let mut world = Game::new("navigation", 1, 1)
        .fixed_hz(60)
        .sim_system(copy_at_tick_30)
        .sim_system(systems::step_navmesh3d)
        .run_headless(480, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            let agent = NavAgent3D {
                destination: Some(vec3(5.0, 0.0, -4.0)),
                ..Default::default()
            };
            let walker = ctx.world.spawn((Transform3D::at(vec3(-5.0, 0.0, -4.0)), agent));
            ctx.world.insert_resource(Walker {
                entity: walker,
                tick: 0,
            });
        });
    let mut arrived = 0;
    world.query::<(&Transform3D, &NavAgent3D)>().for_each(|(t, a)| {
        let target = a.destination.unwrap();
        assert!(
            t.position.with_y(0.0).distance(target) < 0.25,
            "{:?} -> {target:?}",
            t.position
        );
        arrived += 1;
    });
    assert_eq!(arrived, 2);
}
#[test]
fn baking_and_paths_are_repeatable() {
    let sources = [floor(), source(MeshData::cube(4.0), vec3(0.0, 2.0, 0.0))];
    let a = bake(&sources);
    let b = bake(&sources);
    let start = vec3(-7.0, 0.0, 0.0);
    let end = vec3(7.0, 0.0, 0.0);
    assert_eq!(a.find_path(start, end), b.find_path(start, end));
}

#[test]
fn invalid_inputs_return_errors_or_no_position() {
    let settings = NavBakeSettings {
        cell_size: 0.0,
        ..Default::default()
    };
    assert!(NavMesh3D::bake(&[floor()], settings).is_err());
    assert!(
        NavMesh3D::bake(
            &[NavSource {
                triangles: vec![[Vec3::NAN; 3]]
            }],
            NavBakeSettings::default()
        )
        .is_err()
    );
    let nav = bake(&[floor()]);
    assert!(nav.sample_position(Vec3::ZERO, -1.0).is_none());
    assert!(nav.find_path(Vec3::NAN, Vec3::ZERO).is_none());
}

#[test]
fn clearance_excludes_low_ceilings() {
    let mut ceiling = floor();
    for t in &mut ceiling.triangles {
        for p in t.iter_mut() {
            p.y = 1.0;
        }
        t.swap(1, 2);
    }
    let nav = bake(&[floor(), ceiling]);
    assert!(nav.sample_position(Vec3::ZERO, 0.3).is_none());
}

#[test]
fn agent_follows_detour_without_entering_solid_box() {
    fn check(ctx: &mut Ctx) {
        ctx.world.query::<&Transform3D>().for_each(|t| {
            assert!(t.position.x.abs() > 2.0 || t.position.z.abs() > 2.0, "{:?}", t.position);
        });
    }
    let mut world = Game::new("navigation", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .sim_system(check)
        .run_headless(600, |ctx| {
            ctx.world
                .insert_resource(bake(&[floor(), source(MeshData::cube(4.0), vec3(0.0, 2.0, 0.0))]));
            let agent = NavAgent3D {
                destination: Some(vec3(7.0, 0.0, 0.0)),
                ..Default::default()
            };
            ctx.world.spawn((Transform3D::at(vec3(-7.0, 0.0, 0.0)), agent));
        });
    let position = world.query::<&Transform3D>().iter().next().unwrap().position;
    assert!(position.distance(vec3(7.0, 0.0, 0.0)) < 0.25, "{position:?}");
}

#[test]
fn mesh_replacement_invalidates_an_existing_agent_path() {
    struct Tick(u32);
    fn replace(ctx: &mut Ctx) {
        ctx.world.resource_mut::<Tick>().0 += 1;
        if ctx.world.resource::<Tick>().0 == 40 {
            let empty = NavSource::from_mesh(
                &MeshData::plane(12.0),
                Transform3D::default().rotated(Quat::from_rotation_x(60_f32.to_radians())),
            );
            ctx.world.insert_resource(bake(&[empty]));
        }
    }
    let mut world = Game::new("navigation", 1, 1)
        .fixed_hz(60)
        .sim_system(replace)
        .sim_system(systems::step_navmesh3d)
        .run_headless(300, |ctx| {
            ctx.world.insert_resource(Tick(0));
            ctx.world.insert_resource(bake(&[floor()]));
            let agent = NavAgent3D {
                destination: Some(vec3(7.0, 0.0, 0.0)),
                ..Default::default()
            };
            ctx.world.spawn((Transform3D::at(vec3(-7.0, 0.0, 0.0)), agent));
        });
    let x = world.query::<&Transform3D>().iter().next().unwrap().position.x;
    assert!(x < -4.0, "{x}");
}

#[test]
fn obstacle_components_stop_agents_then_removal_replans_deterministically() {
    #[derive(Clone, Copy)]
    struct State {
        tick: u32,
        obstacle: Entity,
    }
    fn update(ctx: &mut Ctx) {
        let state = ctx.world.resource_mut::<State>();
        state.tick += 1;
        let state = *state;
        if state.tick == 30 {
            ctx.world.get_mut::<NavObstacle3D>(state.obstacle).unwrap().center.x = 0.0;
        }
        if state.tick == 100 {
            ctx.world
                .query::<&Transform3D>()
                .for_each(|t| assert!(t.position.x < 0.0));
            ctx.world.despawn(state.obstacle);
        }
    }
    let run = || {
        let mut world = Game::new("navigation", 1, 1)
            .fixed_hz(60)
            .sim_system(update)
            .sim_system(systems::step_navmesh3d)
            .run_headless(500, |ctx| {
                ctx.world.insert_resource(bake(&[floor()]));
                let obstacle = ctx
                    .world
                    .spawn((NavObstacle3D::new(vec3(30.0, 1.0, 0.0), vec3(0.5, 2.0, 20.0)),));
                ctx.world.insert_resource(State { tick: 0, obstacle });
                let agent = NavAgent3D {
                    destination: Some(vec3(7.0, 0.0, 0.0)),
                    ..Default::default()
                };
                ctx.world.spawn((Transform3D::at(vec3(-7.0, 0.0, 0.0)), agent));
            });
        world.query::<&Transform3D>().iter().next().unwrap().position
    };
    let a = run();
    let b = run();
    assert_eq!(a, b);
    assert!(a.distance(vec3(7.0, 0.0, 0.0)) < 0.25, "{a:?}");
}

#[test]
fn agents_follow_steps_and_keep_their_floor_in_stacked_geometry() {
    for (upper, avoidance) in [(false, false), (true, false), (false, true), (true, true)] {
        let level = if upper { 4.0 } else { 0.0 };
        let mut world = Game::new("navigation", 1, 1)
            .fixed_hz(60)
            .sim_system(systems::step_navmesh3d)
            .run_headless(360, |ctx| {
                let sources = [
                    source(MeshData::plane(8.0), vec3(-4.0, level, 0.0)),
                    source(MeshData::plane(8.0), vec3(4.0, level + 0.2, 0.0)),
                    source(MeshData::plane(20.0), vec3(0.0, if upper { 0.0 } else { 4.0 }, 0.0)),
                ];
                ctx.world.insert_resource(bake(&sources));
                let agent = NavAgent3D {
                    destination: Some(vec3(3.0, level + 0.2, 0.0)),
                    avoidance,
                    ..Default::default()
                };
                ctx.world.spawn((Transform3D::at(vec3(-3.0, level, 0.0)), agent));
            });
        let position = world.query::<&Transform3D>().iter().next().unwrap().position;
        assert!(position.distance(vec3(3.0, level + 0.2, 0.0)) < 0.25, "{position:?}");
    }
}

fn crowd_agent(goal: Option<Vec3>) -> NavAgent3D {
    NavAgent3D {
        avoidance: true,
        destination: goal,
        ..Default::default()
    }
}
fn check_crowd(ctx: &mut Ctx) {
    let agents: Vec<_> = ctx
        .world
        .query::<(&Transform3D, &NavAgent3D)>()
        .iter()
        .map(|(t, a)| (t.position, a.radius))
        .collect();
    for (i, (p, r)) in agents.iter().enumerate() {
        assert!(
            ctx.world.resource::<NavMesh3D>().sample_position(*p, 0.02).is_some(),
            "off mesh {p:?}"
        );
        for (q, s) in &agents[i + 1..] {
            assert!((p - q).with_y(0.0).length() >= r + s - 0.025, "overlap {p:?} {q:?}");
        }
    }
}
#[test]
fn crowd_head_on_corridor_passes_and_arrives_repeatably() {
    let run = || {
        let mut world = Game::new("crowd", 1, 1)
            .fixed_hz(60)
            .sim_system(systems::step_navmesh3d)
            .sim_system(check_crowd)
            .run_headless(600, |ctx| {
                let floor = NavSource::from_mesh(
                    &MeshData::plane(20.0),
                    Transform3D {
                        scale: vec3(1.0, 1.0, 0.15),
                        ..Default::default()
                    },
                );
                ctx.world.insert_resource(bake(&[floor]));
                for sign in [-1.0, 1.0] {
                    ctx.world.spawn((
                        Transform3D::at(vec3(sign * 7.0, 0.0, 0.0)),
                        crowd_agent(Some(vec3(-sign * 7.0, 0.0, 0.0))),
                    ));
                }
            });
        world
            .query::<(&Transform3D, &NavAgent3D)>()
            .iter()
            .map(|(t, a)| {
                assert!(t.position.distance(a.destination.unwrap()) < 0.25, "{:?}", t.position);
                t.position
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}
#[test]
fn crowd_walks_around_a_stopped_agent() {
    let mut world = Game::new("crowd", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .sim_system(check_crowd)
        .run_headless(600, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            ctx.world.spawn((Transform3D::default(), crowd_agent(None)));
            ctx.world.spawn((
                Transform3D::at(vec3(-7.0, 0.0, 0.0)),
                crowd_agent(Some(vec3(7.0, 0.0, 0.0))),
            ));
        });
    world.query::<(&Transform3D, &NavAgent3D)>().for_each(|(t, a)| {
        let target = a.destination.unwrap_or(Vec3::ZERO);
        assert!(t.position.distance(target) < 0.25, "{:?}", t.position);
    });
}
#[test]
fn fifty_agents_reach_a_shared_goal_region_without_overlaps() {
    let mut world = Game::new("crowd", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .sim_system(check_crowd)
        .run_headless(1800, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            for i in 0..50 {
                let angle = i as f32 * std::f32::consts::TAU / 50.0;
                ctx.world.spawn((
                    Transform3D::at(vec3(angle.cos() * 8.0, 0.0, angle.sin() * 8.0)),
                    crowd_agent(Some(Vec3::ZERO)),
                ));
            }
        });
    // Fifty radius-.3 discs need a region, rather than a single occupied point.
    world
        .query::<&Transform3D>()
        .for_each(|t| assert!(t.position.with_y(0.0).length() < 4.5, "{:?}", t.position));
}

/// `n` agents in rows of 25, 0.72 apart, each walking one unit, for 60
/// ticks at 60 Hz.
fn walking_grid(n: usize) -> World {
    Game::new("crowd", 1, 1)
        .fixed_hz(60)
        .sim_system(systems::step_navmesh3d)
        .run_headless(60, move |ctx| {
            ctx.world
                .insert_resource(bake(&[source(MeshData::plane(20.0), Vec3::ZERO)]));
            for i in 0..n {
                let position = vec3((i % 25) as f32 * 0.72 - 8.64, 0.0, (i / 25) as f32 * 0.72 - 6.84);
                let mut a = crowd_agent(Some(position + Vec3::Z));
                a.radius = 0.2;
                a.avoidance_weight = 0.0;
                ctx.world.spawn((Transform3D::at(position), a));
            }
        })
}

#[test]
fn five_hundred_agents_each_step_every_tick_with_work_that_grows_linearly() {
    let mut world = walking_grid(500);
    assert_eq!(world.query::<&NavAgent3D>().iter().count(), 500);
    world.query::<(&Transform3D, &NavAgent3D)>().for_each(|(t, a)| {
        assert!(
            t.position.with_y(0.0).distance(a.destination.unwrap()) < 0.999,
            "agent did not move: {:?}",
            t.position
        );
    });
    // Counted work, not time: one crowd step per 60 Hz tick, each moving
    // every agent, holds on a machine of any speed and any load.
    let many = world.resource::<NavMesh3D>().crowd_stats();
    assert_eq!((many.steps, many.agent_steps), (60, 60 * 500), "{many:?}");
    // A crowd step that grew with the square of the agents (or worse) would
    // do ten times the work per agent for ten times the agents at the same
    // spacing; a linear one does about the same (rows inside the grid have
    // more neighbours than the edge rows, so a little more).
    let few = walking_grid(50).resource::<NavMesh3D>().crowd_stats();
    assert_eq!((few.steps, few.agent_steps), (60, 60 * 50), "{few:?}");
    let per_agent = |s: NavCrowdStats| s.work as f64 / s.agent_steps as f64;
    let growth = per_agent(many) / per_agent(few);
    eprintln!(
        "work per agent step: {:.0} for 50, {:.0} for 500 ({growth:.2}x)",
        per_agent(few),
        per_agent(many)
    );
    assert!(
        growth < 2.0,
        "work per agent step grew {growth:.2}x: {few:?} -> {many:?}"
    );
}

#[test]
fn crowd_slots_survive_changes_without_reusing_another_agents_state() {
    struct State {
        tick: u32,
        moving: Entity,
        stopped: Entity,
    }
    fn edit(ctx: &mut Ctx) {
        let s = ctx.world.resource_mut::<State>();
        s.tick += 1;
        let (tick, moving, stopped) = (s.tick, s.moving, s.stopped);
        if tick == 60 {
            ctx.world.get_mut::<Transform3D>(moving).unwrap().position = vec3(-6.0, 0.0, 2.0);
            ctx.world.get_mut::<NavAgent3D>(moving).unwrap().destination = Some(vec3(6.0, 0.0, 2.0));
            ctx.world.despawn(stopped);
            // Cloning a live component must not clone ownership of its crowd slot.
            let mut clone = ctx.world.get::<NavAgent3D>(moving).unwrap().clone();
            clone.destination = Some(vec3(6.0, 0.0, -2.0));
            ctx.world.spawn((Transform3D::at(vec3(-6.0, 0.0, -2.0)), clone));
        }
        if tick == 120 {
            ctx.world.get_mut::<NavAgent3D>(moving).unwrap().avoidance = false;
        }
        if tick == 150 {
            ctx.world.get_mut::<NavAgent3D>(moving).unwrap().avoidance = true;
        }
        if tick == 180 {
            // More agents than the initial capacity forces a slot/corridor rebuild.
            for i in 0..20 {
                ctx.world.spawn((
                    Transform3D::at(vec3(-8.0 + (i % 10) as f32, 0.0, 5.0 + (i / 10) as f32)),
                    crowd_agent(None),
                ));
            }
        }
    }
    let mut world = Game::new("crowd", 1, 1)
        .fixed_hz(60)
        .sim_system(edit)
        .sim_system(systems::step_navmesh3d)
        .sim_system(check_crowd)
        .run_headless(600, |ctx| {
            ctx.world.insert_resource(bake(&[floor()]));
            let moving = ctx.world.spawn((
                Transform3D::at(vec3(-7.0, 0.0, 0.0)),
                crowd_agent(Some(vec3(7.0, 0.0, 0.0))),
            ));
            let stopped = ctx.world.spawn((Transform3D::default(), crowd_agent(None)));
            ctx.world.insert_resource(State {
                tick: 0,
                moving,
                stopped,
            });
        });
    world.query::<(&Transform3D, &NavAgent3D)>().for_each(|(t, a)| {
        if let Some(target) = a.destination {
            assert!(t.position.distance(target) < 0.25, "{:?}", t.position);
        }
    });
}

fn drain_carving(nav: &mut NavMesh3D, budget: usize) -> usize {
    let mut steps = 0;
    for _ in 0..200 {
        if !nav.carving_pending() {
            return steps;
        }
        let n = nav.update_carving(budget).unwrap();
        assert!(n <= budget);
        steps += n;
    }
    panic!("carving did not drain");
}
#[test]
fn carving_cuts_edges_and_removal_restores_straight_path() {
    let mut nav = carving_bake(&[floor()]);
    let from = vec3(-7.0, 0.0, 0.0);
    let to = vec3(7.0, 0.0, 0.0);
    let mut o = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 1.0, 2.0));
    o.carve = true;
    nav.set_obstacles(&[o]);
    assert!(nav.carving_pending());
    assert_eq!(nav.update_carving(0).unwrap(), 0);
    let rebuilt = drain_carving(&mut nav, 1);
    assert!(rebuilt > 0 && rebuilt <= 8, "{rebuilt}");
    let path = nav.find_path(from, to).unwrap();
    assert!(path.iter().any(|p| p.z.abs() > 2.0), "{path:?}");
    assert!(nav.sample_position(Vec3::ZERO, 0.2).is_none());
    o.center.x += 0.01;
    nav.set_obstacles(&[o]);
    assert!(!nav.carving_pending());
    assert_eq!(nav.update_carving(1).unwrap(), 0);
    o.center.x += 1.0;
    nav.set_obstacles(&[o]);
    assert!(drain_carving(&mut nav, 1) <= 16);
    nav.set_obstacles(&[]);
    drain_carving(&mut nav, 1);
    let path = nav.find_path(from, to).unwrap();
    assert!(path.iter().all(|p| p.z.abs() < 0.3), "{path:?}");
}
#[test]
fn carving_updates_received_while_pending_are_not_lost() {
    let mut nav = carving_bake(&[floor()]);
    let mut o = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), Vec3::ONE);
    o.carve = true;
    nav.set_obstacles(&[o]);
    nav.update_carving(1).unwrap();
    nav.set_obstacles(&[]);
    drain_carving(&mut nav, 1);
    assert!(nav.sample_position(Vec3::ZERO, 0.3).is_some());
}

#[test]
fn crowd_reroutes_when_a_hole_is_carved_across_its_path() {
    struct Tick(u32);
    fn drop_box(ctx: &mut Ctx) {
        let t = ctx.world.resource_mut::<Tick>();
        t.0 += 1;
        if t.0 == 30 {
            let mut o = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 1.0, 2.0));
            o.carve = true;
            ctx.world.spawn((o,));
        }
    }
    fn run() -> Vec3 {
        let mut w = Game::new("carving", 1, 1)
            .fixed_hz(60)
            .sim_system(drop_box)
            .sim_system(systems::step_navmesh3d)
            .sim_system(check_crowd)
            .run_headless(600, |ctx| {
                ctx.world.insert_resource(Tick(0));
                ctx.world.insert_resource(carving_bake(&[floor()]));
                ctx.world.spawn((
                    Transform3D::at(vec3(-7.0, 0.0, 0.0)),
                    crowd_agent(Some(vec3(7.0, 0.0, 0.0))),
                ));
            });
        w.query::<&Transform3D>().iter().next().unwrap().position
    }
    let a = run();
    assert!(a.distance(vec3(7.0, 0.1, 0.0)) < 0.3, "{a:?}");
    assert_eq!(a, run());
}

/// A smooth hill, 2 m tall at the origin: slopes up to about 20 degrees and
/// enough curvature that flat polygons alone miss the ground by most of a metre.
fn hill_height(x: f32, z: f32) -> f32 {
    2.0 * (-(x * x + z * z) / (2.0 * 3.5 * 3.5)).exp()
}
fn hill() -> NavSource {
    let (step, half) = (0.5, 12.0);
    let n = (2.0 * half / step) as i32;
    let at = |i: i32, j: i32| {
        let (x, z) = (i as f32 * step - half, j as f32 * step - half);
        vec3(x, hill_height(x, z), z)
    };
    let mut triangles = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
            triangles.push([a, b, c]);
            triangles.push([c, b, d]);
        }
    }
    NavSource { triangles }
}
/// The largest gap between the mesh's surface and the hill at points around
/// `center`, between `inner` and `outer` metres from it.
fn surface_error(nav: &NavMesh3D, center: Vec3, inner: f32, outer: f32) -> f32 {
    let mut worst = 0.0f32;
    for i in 0..48 {
        let a = i as f32 * std::f32::consts::TAU / 48.0;
        for r in [inner, (inner + outer) / 2.0, outer] {
            let (x, z) = (center.x + r * a.cos(), center.z + r * a.sin());
            let p = nav.sample_position(vec3(x, hill_height(x, z), z), 0.6).unwrap();
            worst = worst.max((p.y - hill_height(p.x, p.z)).abs());
        }
    }
    worst
}
fn is_carved(nav: &NavMesh3D, p: Vec3) -> bool {
    nav.sample_position(p, 0.2).is_none()
}
fn is_open(nav: &NavMesh3D, p: Vec3) -> bool {
    nav.sample_position(p + Vec3::Y * 0.1, 0.3)
        .is_some_and(|q| (q - p).with_y(0.0).length() < 0.05)
}
fn carve(mut o: NavObstacle3D) -> NavObstacle3D {
    o.carve = true;
    o
}

#[test]
fn carving_a_turned_box_cuts_the_box_not_its_bounds() {
    let mut nav = carving_bake(&[floor()]);
    let mut o = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(3.0, 1.0, 0.3));
    o.yaw = std::f32::consts::FRAC_PI_4;
    nav.set_obstacles(&[carve(o)]);
    drain_carving(&mut nav, 1);
    // Positive yaw turns +X toward -Z: the box runs from (+x, -z) to (-x, +z).
    for t in [-2.0, -1.0, 0.0, 1.0, 2.0] {
        assert!(is_carved(&nav, vec3(t, 0.0, -t)), "{t}");
    }
    // Corners of the box's world bounds stay walkable.
    for p in [vec3(1.8, 0.0, 1.8), vec3(-1.8, 0.0, -1.8)] {
        assert!(is_open(&nav, p), "{p}");
    }
    let path = nav.find_path(vec3(-7.0, 0.0, -7.0), vec3(7.0, 0.0, 7.0)).unwrap();
    assert!(path.iter().all(|p| p.x.abs() < 7.5 && p.z.abs() < 7.5), "{path:?}");
}
#[test]
fn carving_a_cylinder_cuts_a_round_hole() {
    let mut nav = carving_bake(&[floor()]);
    nav.set_obstacles(&[carve(NavObstacle3D::cylinder(vec3(0.0, 1.0, 0.0), 1.5, 1.0))]);
    drain_carving(&mut nav, 1);
    for p in [
        Vec3::ZERO,
        vec3(1.2, 0.0, 0.0),
        vec3(0.0, 0.0, -1.2),
        vec3(0.9, 0.0, 0.9),
    ] {
        assert!(is_carved(&nav, p), "{p}");
    }
    // Inside the cylinder's bounding box (grown by the agent), outside its circle.
    assert!(is_open(&nav, vec3(1.75, 0.0, 1.75)));
    assert!(is_open(&nav, vec3(-1.75, 0.0, -1.75)));
}
#[test]
fn carving_a_convex_polygon_cuts_the_polygon() {
    let mut nav = carving_bake(&[floor()]);
    let wedge = [vec2(-2.0, -1.5), vec2(2.0, -1.5), vec2(0.0, 2.5)];
    nav.set_obstacles(&[carve(NavObstacle3D::convex(vec3(0.0, 1.0, 0.0), &wedge, 1.0).unwrap())]);
    drain_carving(&mut nav, 1);
    for p in [Vec3::ZERO, vec3(0.0, 0.0, 2.0), vec3(-1.5, 0.0, -1.2)] {
        assert!(is_carved(&nav, p), "{p}");
    }
    // Inside the wedge's bounds, beside its slanted sides.
    assert!(is_open(&nav, vec3(1.8, 0.0, 1.8)));
    assert!(is_open(&nav, vec3(-1.8, 0.0, 1.8)));
    // Turning it half a circle points the tip the other way.
    let mut turned = NavObstacle3D::convex(vec3(0.0, 1.0, 0.0), &wedge, 1.0).unwrap();
    turned.yaw = std::f32::consts::PI;
    nav.set_obstacles(&[carve(turned)]);
    drain_carving(&mut nav, 1);
    assert!(is_carved(&nav, vec3(0.0, 0.0, -2.0)));
    assert!(is_open(&nav, vec3(0.0, 0.0, 2.6)));
}
#[test]
fn blocking_shapes_close_paths_and_invalid_turns_block_everything() {
    let mut nav = bake(&[floor()]);
    let (from, to) = (vec3(-7.0, 0.0, 0.0), vec3(7.0, 0.0, 0.0));
    let mut wall = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.3, 1.0, 14.0));
    wall.yaw = 0.3;
    for o in [
        wall,
        NavObstacle3D::cylinder(vec3(0.0, 1.0, 0.0), 12.0, 1.0),
        NavObstacle3D::convex(
            vec3(0.0, 1.0, 0.0),
            &[vec2(-1.0, -12.0), vec2(1.0, -12.0), vec2(0.0, 12.0)],
            1.0,
        )
        .unwrap(),
    ] {
        nav.set_obstacles(&[o]);
        assert!(nav.find_path(from, to).is_none(), "{o:?}");
    }
    let mut small = NavObstacle3D::cylinder(vec3(0.0, 1.0, 6.0), 0.5, 1.0);
    nav.set_obstacles(&[small]);
    assert!(nav.find_path(from, to).is_some());
    small.yaw = f32::NAN;
    nav.set_obstacles(&[small]);
    assert!(nav.find_path(from, to).is_none());
    assert!(nav.sample_position(vec3(0.0, 0.0, -6.0), 0.3).is_none());
    nav.set_obstacles(&[]);
    assert!(nav.find_path(from, to).is_some());
}
#[test]
fn turning_a_carving_obstacle_rebuilds_only_past_half_a_cell() {
    let mut nav = carving_bake(&[floor()]);
    let mut o = carve(NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(2.0, 1.0, 0.3)));
    nav.set_obstacles(&[o]);
    drain_carving(&mut nav, 1);
    // The far corner moves about 0.02 m: not worth a rebuild.
    o.yaw = 0.01;
    nav.set_obstacles(&[o]);
    assert!(!nav.carving_pending());
    o.yaw = 0.5;
    nav.set_obstacles(&[o]);
    assert!(nav.carving_pending());
    assert!(drain_carving(&mut nav, 1) > 0);
}
#[test]
fn carved_tiles_keep_detail_heights_on_a_hill() {
    let center = vec3(2.5, 0.0, 0.0);
    let mut nav = carving_bake(&[hill()]);
    let before = surface_error(&nav, center, 1.6, 3.0);
    let mut o = NavObstacle3D::new(center + Vec3::Y * hill_height(center.x, center.z), vec3(0.8, 1.0, 0.4));
    o.yaw = 0.7;
    nav.set_obstacles(&[carve(o)]);
    assert!(drain_carving(&mut nav, 1) > 0);
    assert!(is_carved(&nav, center + Vec3::Y * hill_height(center.x, center.z)));
    let after = surface_error(&nav, center, 1.6, 3.0);
    // As close to the ground as an ordinary bake's detail mesh (about 0.25 m
    // here: the voxel floor, the sample error and 1.2 m sample spacing).
    let plain = surface_error(&bake(&[hill()]), center, 1.6, 3.0);
    assert!(
        before <= plain + 0.03 && after <= plain + 0.03 && after < 0.3,
        "ordinary {plain}, before {before}, after {after}"
    );
}
#[test]
fn agents_stay_on_a_hill_next_to_carved_shapes_and_repeat_exactly() {
    struct Tick(u32);
    fn edit(ctx: &mut Ctx) {
        let t = ctx.world.resource_mut::<Tick>();
        t.0 += 1;
        let tick = t.0;
        let on_hill = |x: f32, z: f32| vec3(x, hill_height(x, z) + 0.5, z);
        if tick == 5 {
            // Each one stands across an agent's straight line.
            let mut post = NavObstacle3D::new(on_hill(1.0, 0.0), vec3(1.0, 1.0, 0.3));
            post.yaw = 0.6;
            ctx.world.spawn((carve(post),));
            ctx.world
                .spawn((carve(NavObstacle3D::cylinder(on_hill(-2.5, 0.2), 0.6, 1.0)),));
            let wedge = [vec2(-0.6, -0.5), vec2(0.6, -0.5), vec2(0.0, 0.7)];
            ctx.world
                .spawn((carve(NavObstacle3D::convex(on_hill(0.3, -2.5), &wedge, 1.0).unwrap()),));
        }
        if tick == 60 {
            ctx.world.query::<&mut NavObstacle3D>().for_each(|o| {
                o.yaw += if o.shape == ponko::d3::NavObstacleShape::Box {
                    0.5
                } else {
                    0.0
                }
            });
        }
    }
    fn check(ctx: &mut Ctx) {
        let shapes: Vec<NavObstacle3D> = ctx.world.query::<&NavObstacle3D>().iter().copied().collect();
        ctx.world.query::<(&Transform3D, &NavAgent3D)>().for_each(|(t, _)| {
            let p = t.position;
            assert!((p.y - hill_height(p.x, p.z)).abs() < 0.2, "off the surface at {p}");
            for o in &shapes {
                let d = (p - o.center).with_y(0.0);
                let (c, s) = (o.yaw.cos(), o.yaw.sin());
                let local = vec2(d.x * c - d.z * s, d.x * s + d.z * c);
                let inside = match o.shape {
                    ponko::d3::NavObstacleShape::Cylinder => local.length() < o.half_extents.x,
                    // The wedge's inscribed circle.
                    ponko::d3::NavObstacleShape::Convex(_) => local.length() < 0.3,
                    _ => local.x.abs() < o.half_extents.x && local.y.abs() < o.half_extents.z,
                };
                assert!(!inside, "inside {o:?} at {p}");
            }
        });
    }
    fn run() -> Vec<Vec3> {
        let mut w = Game::new("carving", 1, 1)
            .fixed_hz(60)
            .sim_system(edit)
            .sim_system(systems::step_navmesh3d)
            .sim_system(check)
            .run_headless(700, |ctx| {
                ctx.world.insert_resource(Tick(0));
                ctx.world.insert_resource(carving_bake(&[hill()]));
                for (from, to) in [
                    (vec3(-6.0, 0.0, 0.5), vec3(6.0, 0.0, -0.5)),
                    (vec3(0.5, 0.0, -6.0), vec3(-0.5, 0.0, 6.0)),
                ] {
                    let agent = NavAgent3D {
                        destination: Some(to.with_y(hill_height(to.x, to.z))),
                        ..Default::default()
                    };
                    ctx.world
                        .spawn((Transform3D::at(from.with_y(hill_height(from.x, from.z))), agent));
                }
            });
        w.query::<&Transform3D>().iter().map(|t| t.position).collect()
    }
    let a = run();
    for p in &a {
        assert!(p.with_y(0.0).length() > 5.0, "did not arrive: {p}");
    }
    assert_eq!(a, run());
}
