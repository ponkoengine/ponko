//! Large levels: many tiles with the default settings, helpful limit errors,
//! opt-in carving layers, and carving that blocks a corridor completely.
#![cfg(feature = "navmesh")]
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::Transform as Transform3D;
#[allow(unused_imports)]
#[cfg(feature = "navmesh")]
use ponko::d3::{NavAgent as NavAgent3D, NavMesh as NavMesh3D, NavObstacle as NavObstacle3D};
#[allow(unused_imports)]
#[cfg(feature = "navmesh")]
use ponko::nav::{NavBakeSettings, NavSource};
use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::MeshData;
use std::time::Instant;

fn plane(size: f32) -> NavSource {
    NavSource::from_mesh(&MeshData::plane(size), Transform3D::default())
}
/// A square grid of `cells` x `cells` quads, `size` metres across, whose height
/// is a gentle rolling surface (slopes well under the 45 degree default).
fn rolling_terrain(size: f32, cells: usize) -> NavSource {
    let step = size / cells as f32;
    let height = |i: usize, j: usize| {
        let (x, z) = (i as f32 * step, j as f32 * step);
        0.6 * (x * 0.21).sin() * (z * 0.17).cos() + 0.3 * (x * 0.07 + z * 0.05).sin()
    };
    let at = |i: usize, j: usize| vec3(i as f32 * step - size / 2.0, height(i, j), j as f32 * step - size / 2.0);
    let mut triangles = Vec::with_capacity(cells * cells * 2);
    for i in 0..cells {
        for j in 0..cells {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            triangles.push([a, c, b]);
            triangles.push([a, d, c]);
        }
    }
    NavSource { triangles }
}
fn bake(sources: &[NavSource], settings: NavBakeSettings) -> NavMesh3D {
    let start = Instant::now();
    let nav = NavMesh3D::bake(sources, settings).unwrap();
    eprintln!("baked in {:?}", start.elapsed());
    nav
}
fn assert_corner_to_corner(nav: &NavMesh3D, half: f32, slack: f32) {
    let (a, b) = (vec3(-half + 1.0, 0.0, -half + 1.0), vec3(half - 1.0, 0.0, half - 1.0));
    let from = nav.sample_position(a, 3.0).expect("start on the mesh");
    let to = nav.sample_position(b, 3.0).expect("goal on the mesh");
    let path = nav.find_path(from, to).expect("corner to corner");
    assert!(path.first().unwrap().distance(from) < 0.5 && path.last().unwrap().distance(to) < 0.5);
    let length: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!(
        length >= from.distance(to) - 0.1 && length < from.distance(to) * slack,
        "{length}"
    );
}

#[test]
fn a_flat_40_m_plane_bakes_with_default_settings() {
    let nav = bake(&[plane(40.0)], NavBakeSettings::default());
    assert_corner_to_corner(&nav, 20.0, 1.05);
}

#[test]
fn a_flat_500_m_plane_bakes_with_default_settings() {
    let nav = bake(&[plane(500.0)], NavBakeSettings::default());
    assert_corner_to_corner(&nav, 500.0 / 2.0, 1.05);
}

#[test]
fn a_one_kilometre_plane_bakes_with_a_coarser_cell_size() {
    let settings = NavBakeSettings {
        cell_size: 0.5,
        cell_height: 0.25,
        ..Default::default()
    };
    let nav = bake(&[plane(1000.0)], settings);
    assert_corner_to_corner(&nav, 500.0, 1.05);
}

#[test]
fn rolling_terrain_bakes_and_paths_across_many_tiles() {
    for carving in [false, true] {
        let settings = NavBakeSettings {
            carving,
            ..Default::default()
        };
        let nav = bake(&[rolling_terrain(120.0, 240)], settings);
        let a = nav.sample_position(vec3(-55.0, 0.0, -55.0), 5.0).expect("start");
        let b = nav.sample_position(vec3(55.0, 0.0, 55.0), 5.0).expect("goal");
        let path = nav.find_path(a, b).expect("across the terrain");
        let length: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!(
            length > a.distance(b) * 0.99 && length < a.distance(b) * 1.3,
            "{length}"
        );
        assert!(path.iter().any(|p| p.y.abs() > 0.05), "terrain is not flat");
    }
}

#[test]
fn too_many_tiles_names_the_limit_and_the_setting() {
    let settings = NavBakeSettings {
        cell_size: 0.05,
        ..Default::default()
    };
    let message = NavMesh3D::bake(&[plane(2000.0)], settings).err().unwrap().to_string();
    assert!(
        message.contains("16384") && message.contains("tile_size") && message.contains("cell_size"),
        "{message}"
    );
    assert!(message.contains("255"), "{message}");
}

/// Noise with a fixed seed: every vertex 0..0.15 m off a flat floor, so there
/// are many tiny polygons and the per-tile size caps get real work.
fn rough_floor(size: f32, cells: usize) -> NavSource {
    let step = size / cells as f32;
    let h = |i: usize, j: usize| {
        let n = (i as u32).wrapping_mul(73_856_093) ^ (j as u32).wrapping_mul(19_349_663);
        (n.wrapping_mul(2_654_435_761) >> 16) as f32 / 65_536.0 * 0.15
    };
    let at = |i: usize, j: usize| vec3(i as f32 * step - size / 2.0, h(i, j), j as f32 * step - size / 2.0);
    let mut triangles = Vec::new();
    for i in 0..cells {
        for j in 0..cells {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            triangles.push([a, c, b]);
            triangles.push([a, d, c]);
        }
    }
    NavSource { triangles }
}

#[test]
fn rough_floors_fit_the_tile_caps_at_every_tile_size() {
    for tile_size in [8, 32, 64, 128, 255] {
        let settings = NavBakeSettings {
            tile_size,
            ..Default::default()
        };
        let nav = NavMesh3D::bake(&[rough_floor(80.0, 320)], settings).unwrap_or_else(|e| panic!("{tile_size}: {e}"));
        let from = nav.sample_position(vec3(-35.0, 0.0, -35.0), 3.0).unwrap();
        let to = nav.sample_position(vec3(35.0, 0.0, 35.0), 3.0).unwrap();
        assert!(nav.find_path(from, to).is_some(), "{tile_size}");
    }
}

#[test]
fn carving_layers_are_built_only_on_request() {
    let sources = [plane(60.0)];
    let plain = Instant::now();
    let a = NavMesh3D::bake(&sources, NavBakeSettings::default()).unwrap();
    let plain = plain.elapsed();
    let carving = Instant::now();
    let b = NavMesh3D::bake(
        &sources,
        NavBakeSettings {
            carving: true,
            ..Default::default()
        },
    )
    .unwrap();
    eprintln!("60 m plane: plain bake {plain:?}, carving bake {:?}", carving.elapsed());
    let mut o = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 1.0, 2.0));
    o.carve = true;
    let (mut a, mut b) = (a, b);
    a.set_obstacles(&[o]);
    b.set_obstacles(&[o]);
    // Without layers nothing is queued and the obstacle blocks like a plain one.
    assert!(!a.carving_pending() && b.carving_pending());
    assert_eq!(a.update_carving(4).unwrap(), 0);
    assert!(a.sample_position(Vec3::ZERO, 0.3).is_none());
}

/// A 3 m wide, 40 m long corridor along X.
fn corridor() -> NavSource {
    NavSource::from_mesh(
        &MeshData::plane(1.0),
        Transform3D {
            scale: vec3(40.0, 1.0, 3.0),
            ..Default::default()
        },
    )
}

#[test]
fn a_carving_obstacle_across_a_corridor_leaves_no_path_and_removal_restores_it() {
    let mut nav = NavMesh3D::bake(
        &[corridor()],
        NavBakeSettings {
            carving: true,
            ..Default::default()
        },
    )
    .unwrap();
    let (a, b) = (vec3(-15.0, 0.0, 0.0), vec3(15.0, 0.0, 0.0));
    assert!(nav.find_path(a, b).is_some());
    let mut wall = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 1.0, 3.0));
    wall.carve = true;
    nav.set_obstacles(&[wall]);
    for _ in 0..100 {
        if !nav.carving_pending() {
            break;
        }
        nav.update_carving(1).unwrap();
    }
    assert!(!nav.carving_pending());
    assert!(nav.find_path(a, b).is_none(), "both ends exist but are cut apart");
    assert!(nav.sample_position(a, 0.5).is_some() && nav.sample_position(b, 0.5).is_some());
    nav.set_obstacles(&[]);
    for _ in 0..100 {
        if !nav.carving_pending() {
            break;
        }
        nav.update_carving(1).unwrap();
    }
    assert!(nav.find_path(a, b).is_some());
}

#[test]
fn an_agent_stands_still_when_a_carving_obstacle_cuts_its_corridor() {
    fn drop_wall(ctx: &mut Ctx) {
        if ctx.world.query::<&NavObstacle3D>().iter().next().is_none() {
            let mut wall = NavObstacle3D::new(vec3(0.0, 1.0, 0.0), vec3(0.5, 1.0, 3.0));
            wall.carve = true;
            ctx.world.spawn((wall,));
        }
    }
    let mut world = Game::new("blocked", 1, 1)
        .fixed_hz(60)
        .sim_system(drop_wall)
        .sim_system(systems::step_navmesh3d)
        .run_headless(600, |ctx| {
            let settings = NavBakeSettings {
                carving: true,
                ..Default::default()
            };
            ctx.world
                .insert_resource(NavMesh3D::bake(&[corridor()], settings).unwrap());
            let agent = NavAgent3D {
                destination: Some(vec3(15.0, 0.0, 0.0)),
                ..Default::default()
            };
            ctx.world.spawn((Transform3D::at(vec3(-15.0, 0.0, 0.0)), agent));
        });
    let p = world.query::<&Transform3D>().iter().next().unwrap().position;
    assert!(p.x < -0.5 && p.is_finite(), "agent must stop before the wall: {p:?}");
}

#[test]
fn a_two_kilometre_strip_bakes_at_the_default_cell_size() {
    // A 2 km square at 0.2 m cells is 100 million cells, so use a thin strip:
    // 626 tiles of 6.4 m.
    let strip = NavSource::from_mesh(
        &MeshData::plane(1.0),
        Transform3D {
            scale: vec3(2000.0, 1.0, 8.0),
            ..Default::default()
        },
    );
    let nav = bake(&[strip], NavBakeSettings::default());
    let path = nav.find_path(vec3(-990.0, 0.0, 0.0), vec3(990.0, 0.0, 0.0)).unwrap();
    assert!(path.len() >= 2);
}

#[test]
fn tile_size_grows_when_a_level_would_need_too_many_tiles() {
    // 600 m at 0.5 m cells with 8-cell tiles would be 150 x 150 = 22,500 tiles.
    let settings = NavBakeSettings {
        cell_size: 0.5,
        cell_height: 0.25,
        tile_size: 8,
        ..Default::default()
    };
    for carving in [false, true] {
        let nav = bake(&[plane(600.0)], NavBakeSettings { carving, ..settings });
        assert_corner_to_corner(&nav, 300.0, 1.05);
    }
}
