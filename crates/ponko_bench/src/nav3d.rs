//! 3D navigation: agents walking across a baked navmesh (`step_navmesh3d`
//! registered as a sim system), path following without crowd avoidance, and
//! the cost of rebuilding one carved tile.
use ponko::d3::NavAgent as NavAgent3D;
use ponko::d3::NavMesh as NavMesh3D;
use ponko::d3::NavObstacle;
use ponko::d3::Transform as Transform3D;
use ponko::nav::NavBakeSettings;
use ponko::nav::NavSource;
use ponko::prelude::{Game, vec3};
use ponko::render::MeshData;
use ponko::systems::step_navmesh3d;

use crate::Bench;

pub const TICKS: u64 = 250;

pub fn run(b: &mut Bench) {
    for n in [1_000, 10_000] {
        b.run(
            &format!("nav3d/agents-{n}"),
            3,
            || (),
            |_| {
                let world = Game::new("bench", 1, 1)
                    .sim_system(step_navmesh3d)
                    .run_headless(TICKS, |ctx| {
                        let source = NavSource::from_mesh(&MeshData::plane(100.0), Transform3D::default());
                        let nav = NavMesh3D::bake(&[source], NavBakeSettings::default()).expect("bake");
                        ctx.world.insert_resource(nav);
                        let side = (n as f32).sqrt().ceil() as usize;
                        for i in 0..n {
                            let (x, z) = ((i % side) as f32, (i / side) as f32);
                            let at = vec3(x * 0.8 - 40.0, 0.0, z * 0.8 - 40.0);
                            let mut agent = NavAgent3D::default();
                            agent.destination = Some(vec3(-at.x, 0.0, -at.z));
                            ctx.world.spawn((Transform3D::at(at), agent));
                        }
                    });
                std::hint::black_box(world);
            },
        );
    }
}

/// A 100 m square: flat, or with 2 m bumps every 8 m (curved ground makes the
/// detail mesh sample more heights).
fn ground(bumps: bool) -> NavSource {
    let height = |x: f32, z: f32| {
        if bumps {
            (x * 0.785).sin() * (z * 0.785).cos()
        } else {
            0.0
        }
    };
    let at = |i: i32, j: i32| {
        let (x, z) = (i as f32 * 0.5 - 50.0, j as f32 * 0.5 - 50.0);
        vec3(x, height(x, z), z)
    };
    let mut triangles = Vec::new();
    for i in 0..200 {
        for j in 0..200 {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
            triangles.extend([[a, b, c], [c, b, d]]);
        }
    }
    NavSource { triangles }
}

/// One carved layer rebuild per run: a turned box moves between two places
/// inside one 6.4 m tile, so each move queues exactly that tile, and
/// `update_carving(1)` rebuilds it (marking, meshing and detail heights).
pub fn carve(b: &mut Bench) {
    for (name, bumps) in [("nav3d/carve-tile-flat", false), ("nav3d/carve-tile-bumpy", true)] {
        b.steady(
            name,
            200,
            || {
                let settings = NavBakeSettings {
                    carving: true,
                    ..Default::default()
                };
                let nav = NavMesh3D::bake(&[ground(bumps)], settings).expect("bake");
                let mut obstacle = NavObstacle::new(vec3(-2.0, 1.0, -2.0), vec3(0.6, 1.0, 0.3));
                obstacle.yaw = 0.5;
                obstacle.carve = true;
                (nav, obstacle, false)
            },
            |(nav, obstacle, moved)| {
                *moved = !*moved;
                obstacle.center = if *moved {
                    vec3(-3.0, 1.0, -3.0)
                } else {
                    vec3(-2.0, 1.0, -2.0)
                };
                nav.set_obstacles(&[*obstacle]);
                let rebuilt = nav.update_carving(1).expect("carve");
                assert!(rebuilt == 1 && !nav.carving_pending(), "one tile per move");
            },
        );
    }
}
