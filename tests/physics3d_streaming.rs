//! 3D physics for big worlds: terrain colliders that match their tiles,
//! collision meshes built on worker threads, and static colliders kept only
//! near focus points.
#![cfg(feature = "physics3d")]

use std::cell::RefCell;
use std::rc::Rc;

#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::Transform;
#[allow(unused_imports)]
#[cfg(feature = "physics3d")]
use ponko::d3::{Character, Collider, QueryFilter, RigidBody};
#[allow(unused_imports)]
#[cfg(feature = "physics3d")]
use ponko::d3::{ColliderFocus, CollisionMeshData, TerrainCollider};
use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::Material;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::terrain::{Heightfield, TerrainTileData};

const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

fn position(world: &mut World, e: Entity) -> Vec3 {
    world.get::<Transform>(e).unwrap().position
}

#[test]
fn a_ball_rests_on_a_turned_and_scaled_terrain_tile_at_the_drawn_height_and_falls_through_its_hole() {
    // A slope rising 0.15 per meter along the tile's X, stretched twice as
    // high and turned 30 degrees: 0.3 per meter along the turned X.
    let n = 9;
    let size = 16.0;
    let mut heights: Vec<f32> = (0..n * n).map(|i| (i % n) as f32 * 2.0 * 0.15).collect();
    // A hole at grid point (6, 6): the cells from 10 m to 14 m are open.
    heights[6 * n + 6] = f32::NAN;
    let corner = vec3(40.0, -3.0, 20.0);
    let tile_transform = Transform {
        position: corner,
        rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_6),
        scale: vec3(1.0, 2.0, 1.0),
    };
    let ground = Heightfield::new(&heights, size);
    // Where a point of the tile (from its corner) is in the world.
    let world_at = move |x: f32, z: f32| tile_transform.position + tile_transform.rotation * vec3(x, 0.0, z);
    let radius = 0.3;
    let (mut ball, mut over_hole) = (None, None);
    let mut world = Game::new("t", 1, 1).physics3d(GRAVITY).run_headless(150, |ctx| {
        let material = ctx.gfx.default_material();
        let data = TerrainTileData::new(&heights, size);
        let tile = ctx.gfx.upload_terrain_tile(&data, material);
        let mesh = ctx.physics3d.heightfield(&heights, size);
        ctx.world
            .spawn((tile_transform, tile, TerrainCollider::new(mesh).friction(1.0)));
        // A ball that can't roll and grips: it stays where it lands.
        let start = world_at(4.0, 4.0) + vec3(0.0, 8.0, 0.0);
        let grippy = RigidBody {
            lock_rotation: true,
            ..RigidBody::dynamic()
        };
        ball = Some(
            ctx.world
                .spawn((Transform::at(start), grippy, Collider::sphere(radius).friction(1.0))),
        );
        over_hole = Some(ctx.world.spawn((
            Transform::at(world_at(12.0, 12.0) + vec3(0.0, 8.0, 0.0)),
            RigidBody::dynamic(),
            Collider::sphere(radius),
        )));
    });
    // The drawn ground under the ball's center, from the tile's heights.
    let p = position(&mut world, ball.unwrap());
    let local = tile_transform.rotation.inverse() * (p - corner);
    let drawn = corner.y + 2.0 * ground.height_at(local.x, local.z).unwrap();
    // On a plane of slope 0.3, the center sits r / cos(slope) above it.
    let want = drawn + radius * (1.0f32 + 0.3 * 0.3).sqrt();
    assert!(
        (p.y - want).abs() < 0.02,
        "rests at {}, the drawn ground puts it at {want}",
        p.y
    );
    let y = position(&mut world, over_hole.unwrap()).y;
    assert!(y < corner.y - 5.0, "fell through the hole: {y}");
}

#[test]
fn a_mirrored_and_stretched_terrain_mesh_piece_is_solid_where_it_is_drawn_and_faces_up() {
    // A ramp of two triangles rising 0.5 per meter along +X, from (0, 0) to
    // (4, 4) on the ground, wound to face up. Mirrored along X and twice
    // as long, it rises 0.25 per meter along -X, over x from -8 to 0.
    let corners = [
        vec3(0.0, 0.0, 0.0),
        vec3(4.0, 2.0, 0.0),
        vec3(4.0, 2.0, 4.0),
        vec3(0.0, 0.0, 4.0),
    ];
    let triangles = [[0, 3, 2], [0, 2, 1]];
    let mirrored = Transform {
        scale: vec3(-2.0, 1.0, 1.0),
        ..Transform::default()
    };
    let hit = Rc::new(RefCell::new(None));
    let out = hit.clone();
    let radius = 0.3;
    let mut ball = None;
    let mut world = Game::new("t", 1, 1)
        .physics3d(GRAVITY)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                let down = ctx
                    .physics3d
                    .cast_ray(vec3(-4.0, 10.0, 1.0), Vec3::NEG_Y, 20.0, QueryFilter::default());
                *out.borrow_mut() = down.map(|h| (h.point, h.normal));
            }
        })
        .run_headless(150, |ctx| {
            let mesh = ctx.physics3d.trimesh(&corners, &triangles).expect("a ramp");
            ctx.world.spawn((mirrored, TerrainCollider::new(mesh).friction(1.0)));
            let grippy = RigidBody {
                lock_rotation: true,
                ..RigidBody::dynamic()
            };
            let start = vec3(-4.0, 6.0, 3.0);
            ball = Some(
                ctx.world
                    .spawn((Transform::at(start), grippy, Collider::sphere(radius).friction(1.0))),
            );
        });
    let (point, normal) = hit.take().expect("the ray hit the mirrored ramp, over x from -8 to 0");
    assert!(
        (point.y - 1.0).abs() < 1e-3,
        "drawn at height 1 at x = -4, hit at {point}"
    );
    let up_the_slope = vec3(0.25, 1.0, 0.0).normalize();
    assert!(
        normal.distance(up_the_slope) < 1e-3,
        "the stretched slope's normal is {up_the_slope}, not {normal}"
    );
    // The triangles still face up: a ball lands on the ramp and stays,
    // its center r / cos(slope) above the drawn surface.
    let p = position(&mut world, ball.unwrap());
    let want = -p.x * 0.25 + radius * (1.0f32 + 0.25 * 0.25).sqrt();
    assert!(
        (p.y - want).abs() < 0.02 && (-8.0..0.0).contains(&p.x),
        "rests at {p}, the drawn ramp puts it at y = {want}"
    );
}

/// Bumpy ground for the mirrored tiles: 9 × 9 heights over 16 m, no cell
/// flat (so a cell split along the wrong diagonal shows), and a hole at
/// grid point (2, 5).
fn bumpy_heights() -> Vec<f32> {
    let n = 9;
    (0..n * n)
        .map(|i| {
            if i == 5 * n + 2 {
                f32::NAN
            } else {
                (i * 37 % 11) as f32 * 0.4
            }
        })
        .collect()
}

/// What a ray straight down at a point of a tile hit: the point of the
/// tile (from its corner, before its scale) and the height and normal.
type Probe = (Vec2, Option<(f32, Vec3)>);

/// Rays straight down at 13 × 13 points of a 16 m tile at `corner` and
/// `scale` (placed by the signed scale, off the cells' edges).
fn probe_tile(ctx: &Ctx, corner: Vec3, scale: Vec3) -> Vec<Probe> {
    let mut out = Vec::new();
    for a in 0..13 {
        for b in 0..13 {
            let local = vec2((a as f32 + 0.31) / 13.0 * 16.0, (b as f32 + 0.57) / 13.0 * 16.0);
            let at = corner + vec3(local.x * scale.x, 50.0, local.y * scale.z);
            let hit = ctx.physics3d.cast_ray(at, Vec3::NEG_Y, 100.0, QueryFilter::default());
            out.push((local, hit.map(|h| (h.point.y, h.normal))));
        }
    }
    out
}

/// Each probe found the ground drawn there by the tile at `corner` and
/// `scale` (its height and the way it faces), and nothing over the hole.
fn assert_drawn(probes: &[Probe], corner: Vec3, scale: Vec3) {
    let ground = Heightfield::new(&bumpy_heights(), 16.0);
    let mut holes = 0;
    for &(local, hit) in probes {
        let drawn = ground.height_at(local.x, local.y).map(|h| {
            let n = ground.normal_at(local.x, local.y).unwrap();
            // A normal goes through a scale's inverse.
            (corner.y + h * scale.y, (n / scale).normalize())
        });
        match (hit, drawn) {
            (None, None) => holes += 1,
            (Some((y, normal)), Some((want_y, want_normal))) => {
                assert!(
                    (y - want_y).abs() < 1e-3 && normal.distance(want_normal) < 1e-3,
                    "at {local} of the tile at scale {scale}: hit ({y}, {normal}), drawn ({want_y}, {want_normal})"
                );
            }
            _ => panic!("at {local} of the tile at scale {scale}: hit {hit:?}, drawn {drawn:?}"),
        }
    }
    assert!(holes > 0, "some rays went through the hole");
}

#[test]
fn a_mirrored_heightfield_tile_is_solid_where_it_is_drawn_with_its_slopes_and_holes() {
    let corner = vec3(3.0, -2.0, 5.0);
    for scale in [vec3(-1.5, 1.0, 1.0), vec3(1.0, 2.0, -0.75), vec3(-1.0, 0.5, -1.25)] {
        let out = Rc::new(RefCell::new(None));
        let probes = out.clone();
        Game::new("t", 1, 1)
            .physics3d(Vec3::ZERO)
            .sim_system(move |ctx: &mut Ctx| {
                if ctx.time.sim_tick() == 1 {
                    // Where the tile is drawn, and where it would be unmirrored.
                    let unmirrored = probe_tile(ctx, corner, scale.abs());
                    *probes.borrow_mut() = Some((probe_tile(ctx, corner, scale), unmirrored));
                }
            })
            .run_headless(2, |ctx| {
                let mesh = ctx.physics3d.heightfield(&bumpy_heights(), 16.0);
                let tile = Transform {
                    position: corner,
                    scale,
                    ..Transform::default()
                };
                ctx.world.spawn((tile, TerrainCollider::new(mesh)));
            });
        let (drawn, unmirrored) = out.take().expect("probed");
        assert_drawn(&drawn, corner, scale);
        // Negative control: nothing is solid where the unmirrored tile
        // would be (where it was before mirrors counted).
        assert!(
            unmirrored.iter().all(|(_, hit)| hit.is_none()),
            "solid on the unmirrored side at scale {scale}"
        );
    }
    // The probes see the diagonals: the heights mirrored with the cells
    // split the unmirrored way are another ground.
    let ground = Heightfield::new(&bumpy_heights(), 16.0);
    let n = 9;
    let flipped: Vec<f32> = (0..n * n)
        .map(|i| bumpy_heights()[i / n * n + (n - 1 - i % n)])
        .collect();
    let split_wrong = Heightfield::new(&flipped, 16.0);
    let worst = (0..169)
        .filter_map(|k| {
            let (x, z) = ((k / 13) as f32 + 0.31, (k % 13) as f32 + 0.57);
            let (x, z) = (x / 13.0 * 16.0, z / 13.0 * 16.0);
            Some((ground.height_at(x, z)? - split_wrong.height_at(16.0 - x, z)?).abs())
        })
        .fold(0.0, f32::max);
    assert!(worst > 0.2, "a wrong diagonal would be off by {worst} at most");
}

#[test]
fn balls_rest_on_mirrored_heightfield_ramps_at_the_drawn_height_and_fall_where_they_are_not_drawn() {
    // Two ramps of 5 × 5 heights over 4 m rising 0.5 per meter, one along
    // X mirrored along X, one along Z mirrored along Z, each twice as
    // long: they rise 0.25 per meter towards -X and -Z, over 8 m.
    let n = 5;
    let along_x: Vec<f32> = (0..n * n).map(|i| (i % n) as f32 * 0.5).collect();
    let along_z: Vec<f32> = (0..n * n).map(|i| (i / n) as f32 * 0.5).collect();
    let x_corner = vec3(0.0, 0.0, 0.0);
    let z_corner = vec3(50.0, 0.0, 0.0);
    let radius = 0.3;
    let grippy = RigidBody {
        lock_rotation: true,
        ..RigidBody::dynamic()
    };
    let mut balls = Vec::new();
    let mut world = Game::new("t", 1, 1).physics3d(GRAVITY).run_headless(150, |ctx| {
        for (heights, corner, scale) in [
            (&along_x, x_corner, vec3(-2.0, 1.0, 1.0)),
            (&along_z, z_corner, vec3(1.0, 1.0, -2.0)),
        ] {
            let mesh = ctx.physics3d.heightfield(heights, 4.0);
            let tile = Transform {
                position: corner,
                scale,
                ..Transform::default()
            };
            ctx.world.spawn((tile, TerrainCollider::new(mesh).friction(1.0)));
        }
        // On each ramp, 4 m up it, and where its unmirrored tile would be.
        for start in [
            x_corner + vec3(-4.0, 6.0, 2.0),
            z_corner + vec3(2.0, 6.0, -4.0),
            x_corner + vec3(4.0, 6.0, 2.0),
            z_corner + vec3(2.0, 6.0, 4.0),
        ] {
            let ball = (Transform::at(start), grippy, Collider::sphere(radius).friction(1.0));
            balls.push(ctx.world.spawn(ball));
        }
    });
    // On a slope of 0.25, the center sits r / cos(slope) above the ground.
    let want = 4.0 * 0.25 + radius * (1.0f32 + 0.25 * 0.25).sqrt();
    for (k, &ball) in balls.iter().enumerate() {
        let p = position(&mut world, ball);
        if k < 2 {
            assert!(
                (p.y - want).abs() < 0.02,
                "ball {k} rests at {p}, the drawn ramp puts it at y = {want}"
            );
        } else {
            assert!(p.y < -5.0, "ball {k} fell where nothing is drawn, at {p}");
        }
    }
}

#[test]
fn a_mirrored_heightfield_tile_that_loads_later_is_streamed_in_where_it_is_drawn() {
    // Built on a worker; placed 20 ticks in, mirrored along X and Z, while
    // the focus is far away; then the focus comes to the drawn tile, 25 m
    // from where the unmirrored one would be.
    let corner = vec3(200.0, 1.0, 0.0);
    let scale = vec3(-2.0, 1.0, -1.0);
    let out = Rc::new(RefCell::new(None));
    let report = out.clone();
    let mut job = Some(ponko::streaming::Job::spawn(|| {
        CollisionMeshData::heightfield(&bumpy_heights(), 16.0)
    }));
    let tile = Rc::new(RefCell::new(None));
    let focus = Rc::new(RefCell::new(None));
    let focus_in = focus.clone();
    Game::new("t", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| match ctx.time.sim_tick() {
            20 => {
                let data = job.take().unwrap().wait();
                let mesh = ctx.physics3d.add_mesh(&data);
                let at = Transform {
                    position: corner,
                    scale,
                    ..Transform::default()
                };
                *tile.borrow_mut() = Some(ctx.world.spawn((at, TerrainCollider::new(mesh))));
            }
            22 => {
                let e = tile.borrow().unwrap();
                let far = (ctx.physics3d.is_collider_active(e), probe_tile(ctx, corner, scale));
                *report.borrow_mut() = Some((far, None));
                let f = focus_in.borrow().unwrap();
                ctx.world.get_mut::<Transform>(f).unwrap().position = vec3(175.0, 0.0, -8.0);
            }
            25 => {
                let e = tile.borrow().unwrap();
                let near = (ctx.physics3d.is_collider_active(e), probe_tile(ctx, corner, scale));
                report.borrow_mut().as_mut().unwrap().1 = Some(near);
            }
            _ => {}
        })
        .run_headless(26, |ctx| {
            ctx.physics3d.stream_colliders(Some(4));
            *focus.borrow_mut() = Some(ctx.world.spawn((Transform::default(), ColliderFocus::new(10.0, 12.0))));
        });
    let ((far_active, far_probes), near) = out.take().expect("ran");
    assert!(!far_active, "streamed out while the focus is far");
    assert!(
        far_probes.iter().all(|(_, hit)| hit.is_none()),
        "nothing to hit while out"
    );
    let (near_active, near_probes) = near.expect("ran to tick 25");
    assert!(
        near_active,
        "streamed in by a focus over the drawn tile, out of reach of the unmirrored one"
    );
    assert_drawn(&near_probes, corner, scale);
}

#[test]
fn a_mirrored_heightfield_tile_is_solid_where_it_is_drawn_after_a_save_and_load() {
    // A saved world keeps the tile's transform, mirror included; a game
    // without a terrain rebuild gives the loaded tile a new collider.
    let dir = std::env::temp_dir().join(format!("ponko-test-mirrored-terrain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let corner = vec3(-4.0, 0.5, 2.0);
    let scale = vec3(1.0, 1.5, -2.0);
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .run_headless(1, move |ctx| {
            let mesh = ctx.physics3d.heightfield(&bumpy_heights(), 16.0);
            let tile = Transform {
                position: corner,
                scale,
                ..Transform::default()
            };
            ctx.world.spawn((tile, TerrainCollider::new(mesh)));
            ctx.storage.save_world(&ctx.world, "mirrored").unwrap();
        });
    let out = Rc::new(RefCell::new(None));
    let probes = out.clone();
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| match ctx.time.sim_tick() {
            0 => {
                // The loaded tile: the one entity with a mirrored transform.
                let tiles: Vec<Entity> = ctx
                    .world
                    .query::<(Entity, &Transform)>()
                    .iter()
                    .filter(|(_, t)| t.scale.z < 0.0)
                    .map(|(e, _)| e)
                    .collect();
                assert_eq!(tiles.len(), 1, "the tile's transform was saved");
                let mesh = ctx.physics3d.heightfield(&bumpy_heights(), 16.0);
                ctx.world.insert(tiles[0], TerrainCollider::new(mesh));
            }
            2 => *probes.borrow_mut() = Some(probe_tile(ctx, corner, scale)),
            _ => {}
        })
        .run_headless(3, |ctx| ctx.load_world("mirrored").unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    assert_drawn(&out.take().expect("probed"), corner, scale);
}

#[test]
fn a_mirrored_heightfield_tile_keeps_its_collider_through_a_save_and_load() {
    // The game sets how a tile's ground is made again once; after loading,
    // it doesn't make the collider itself.
    let dir = std::env::temp_dir().join(format!("ponko-test-kept-terrain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let corner = vec3(-4.0, 0.5, 2.0);
    let scale = vec3(1.0, 1.5, -2.0);
    fn rebuild(ctx: &mut Ctx) {
        ctx.physics3d.set_terrain_rebuild(|world, tile| {
            // The tile's saved transform says which heights it had.
            let mirrored = world.get::<Transform>(tile)?.scale.z < 0.0;
            mirrored.then(|| CollisionMeshData::heightfield(&bumpy_heights(), 16.0))
        });
    }
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .run_headless(1, move |ctx| {
            rebuild(ctx);
            let mesh = ctx.physics3d.heightfield(&bumpy_heights(), 16.0);
            let tile = Transform {
                position: corner,
                scale,
                ..Transform::default()
            };
            ctx.world.spawn((tile, TerrainCollider::new(mesh).friction(0.2)));
            // Not the rebuild's: left without ground after the load.
            ctx.world.spawn((Transform::default(), TerrainCollider::new(mesh)));
            ctx.storage.save_world(&ctx.world, "kept").unwrap();
        });
    let out = Rc::new(RefCell::new(None));
    let probes = out.clone();
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 2 {
                *probes.borrow_mut() = Some(probe_tile(ctx, corner, scale));
            }
        })
        .run_headless(3, |ctx| {
            rebuild(ctx);
            ctx.load_world("kept").unwrap();
            let mut tiles: Vec<(Vec3, TerrainCollider)> = ctx
                .world
                .query::<(&Transform, &TerrainCollider)>()
                .iter()
                .map(|(t, c)| (t.scale, *c))
                .collect();
            tiles.sort_by(|a, b| a.0.z.total_cmp(&b.0.z));
            assert_eq!(tiles.len(), 2, "both colliders were saved");
            let (mirrored, plain) = (tiles[0].1, tiles[1].1);
            assert!(ctx.physics3d.is_live(mirrored.mesh), "the mirrored tile is solid again");
            assert_eq!(mirrored.friction, 0.2, "with its surface");
            assert!(!ctx.physics3d.is_live(plain.mesh), "the other was left without ground");
        });
    let _ = std::fs::remove_dir_all(&dir);
    assert_drawn(&out.take().expect("probed"), corner, scale);
}

#[test]
fn a_loaded_terrain_collider_without_a_rebuild_keeps_its_surface_and_has_no_ground() {
    let dir = std::env::temp_dir().join(format!("ponko-test-unbuilt-terrain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let hit = Rc::new(RefCell::new(None));
    let out = hit.clone();
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .run_headless(1, |ctx| {
            let mesh = ctx.physics3d.heightfield(&[1.0; 5 * 5], 10.0);
            let solid = TerrainCollider::new(mesh).restitution(0.4);
            ctx.world.spawn((Transform::default(), solid));
            ctx.storage.save_world(&ctx.world, "unbuilt").unwrap();
        });
    Game::new("t", 1, 1)
        .data_dir(&dir)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                let ray = ctx
                    .physics3d
                    .cast_ray(vec3(5.0, 10.0, 5.0), Vec3::NEG_Y, 20.0, QueryFilter::default());
                *out.borrow_mut() = Some(ray.is_some());
            }
        })
        .run_headless(2, |ctx| {
            ctx.load_world("unbuilt").unwrap();
            let c = *ctx
                .world
                .query::<(Entity, &TerrainCollider)>()
                .iter()
                .next()
                .expect("saved")
                .1;
            assert_eq!(c.restitution, 0.4);
            assert!(!ctx.physics3d.is_live(c.mesh));
        });
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        hit.take(),
        Some(false),
        "nothing to stand on until the game gives it a mesh"
    );
}

#[test]
fn collision_meshes_build_on_a_worker_thread() {
    let ray_hits = Rc::new(RefCell::new(None));
    let out = ray_hits.clone();
    Game::new("t", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 1 {
                let hit = ctx
                    .physics3d
                    .cast_ray(vec3(5.0, 10.0, 5.0), Vec3::NEG_Y, 20.0, QueryFilter::default());
                *out.borrow_mut() = hit.map(|h| h.point.y);
            }
        })
        .run_headless(2, |ctx| {
            let job = ponko::streaming::Job::spawn(|| CollisionMeshData::heightfield(&[2.0; 5 * 5], 10.0));
            let mesh = ctx.physics3d.add_mesh(&job.wait());
            ctx.world.spawn((Transform::default(), TerrainCollider::new(mesh)));
        });
    let y = ray_hits.take().expect("the ray hit the ground");
    assert!((y - 2.0).abs() < 1e-4, "{y}");
}

#[test]
fn editing_a_tile_swaps_its_collider_and_despawning_removes_it() {
    let hits = Rc::new(RefCell::new(Vec::new()));
    let out = hits.clone();
    let tile = Rc::new(RefCell::new(None));
    let tile_in = tile.clone();
    Game::new("t", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let ray = ctx
                .physics3d
                .cast_ray(vec3(3.0, 50.0, 3.0), Vec3::NEG_Y, 100.0, QueryFilter::default());
            out.borrow_mut().push(ray.map(|h| h.point.y));
            let e = tile_in.borrow().unwrap();
            match ctx.time.sim_tick() {
                0 => {
                    // Dig the ground down to 1 m: a new mesh, the old one freed.
                    let lower = ctx.physics3d.heightfield(&[1.0; 4 * 4], 6.0);
                    let terrain = ctx.world.get_mut::<TerrainCollider>(e).unwrap();
                    let old = std::mem::replace(&mut terrain.mesh, lower);
                    ctx.free(old);
                }
                1 => {
                    ctx.world.despawn(e);
                }
                _ => {}
            }
        })
        .run_headless(4, |ctx| {
            let mesh = ctx.physics3d.heightfield(&[4.0; 4 * 4], 6.0);
            *tile.borrow_mut() = Some(ctx.world.spawn((Transform::default(), TerrainCollider::new(mesh))));
        });
    let hits = hits.take();
    let near = |got: Option<f32>, want: f32| got.is_some_and(|y| (y - want).abs() < 1e-4);
    assert!(near(hits[0], 4.0), "{hits:?}");
    assert!(near(hits[1], 1.0), "edited: {hits:?}");
    assert_eq!(hits[2], None, "despawned: {hits:?}");
}

/// Fixed 1 m boxes along +X every 20 m, from x = 0, and a focus that walks
/// along X: reports, after each step, which boxes are in.
fn boxes_in_as_the_focus_moves(path: &'static [f32], max_new: usize) -> Vec<Vec<bool>> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let boxes = Rc::new(RefCell::new(Vec::new()));
    let boxes_in = boxes.clone();
    Game::new("t", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let tick = ctx.time.sim_tick() as usize;
            let row: Vec<bool> = boxes_in
                .borrow()
                .iter()
                .map(|&b| ctx.physics3d.is_collider_active(b))
                .collect();
            out.borrow_mut().push(row);
            if let Some(&x) = path.get(tick + 1) {
                ctx.world
                    .query::<&mut Transform>()
                    .filter::<With<ColliderFocus>>()
                    .for_each(|t| t.position.x = x);
            }
        })
        .run_headless(path.len() as u64, |ctx| {
            ctx.physics3d.stream_colliders(Some(max_new));
            ctx.world
                .spawn((Transform::at(vec3(path[0], 0.0, 0.0)), ColliderFocus::new(15.0, 25.0)));
            for i in 0..6 {
                let b = ctx.world.spawn((
                    Transform::at(vec3(i as f32 * 20.0, 0.0, 0.0)),
                    RigidBody::fixed(),
                    Collider::cuboid(Vec3::ONE),
                ));
                boxes.borrow_mut().push(b);
            }
        });
    seen.take()
}

#[test]
fn static_colliders_come_and_go_with_the_focus_and_the_gap_between_radii_keeps_them() {
    let rows = boxes_in_as_the_focus_moves(&[0.0, 20.0, 40.0, 40.0, 100.0, 100.0], 100);
    // Within 15 m in, beyond 25 m out; in between as they were.
    assert_eq!(rows[0], [true, false, false, false, false, false], "at 0");
    assert_eq!(
        rows[1],
        [true, true, false, false, false, false],
        "at 20: box 0 is 19.5 away, kept"
    );
    assert_eq!(rows[2], [false, true, true, false, false, false], "at 40");
    assert_eq!(rows[4], [false, false, false, false, false, true], "at 100");
}

#[test]
fn colliders_come_in_nearest_first_within_the_budget() {
    let rows = boxes_in_as_the_focus_moves(&[50.0, 50.0, 50.0, 50.0], 1);
    // Boxes 2 (at 40) and 3 (at 60) are both 9.5 away, 1 and 4 are 29.5.
    assert_eq!(rows[0].iter().filter(|&&b| b).count(), 1, "one per step: {rows:?}");
    assert!(rows[0][2], "the nearest first, ties in a fixed order: {rows:?}");
    assert_eq!(rows[1], [false, false, true, true, false, false]);
    assert_eq!(rows[3], rows[1], "nothing else is in reach");
}

#[test]
fn ground_stays_under_a_body_left_behind_and_queries_only_see_colliders_that_are_in() {
    let report = Rc::new(RefCell::new(None));
    let out = report.clone();
    let ids = Rc::new(RefCell::new(None));
    let ids_in = ids.clone();
    Game::new("t", 1, 1)
        .physics3d(GRAVITY)
        .sim_system(move |ctx: &mut Ctx| {
            let (focus, under_crate, empty, crate_body) = ids_in.borrow().unwrap();
            match ctx.time.sim_tick() {
                // Settled: walk away, 1 km.
                60 => ctx.world.get_mut::<Transform>(focus).unwrap().position.x = 1000.0,
                150 => {
                    let ray = |ctx: &Ctx, x: f32| {
                        ctx.physics3d
                            .cast_ray(vec3(x, 20.0, 5.0), Vec3::NEG_Y, 40.0, QueryFilter::default())
                            .map(|h| h.entity)
                    };
                    *out.borrow_mut() = Some((
                        ctx.physics3d.is_collider_active(under_crate),
                        ctx.physics3d.is_collider_active(empty),
                        ctx.world.get::<Transform>(crate_body).unwrap().position.y,
                        ray(ctx, 2.0),
                        ray(ctx, 25.0),
                    ));
                }
                _ => {}
            }
        })
        .run_headless(151, |ctx| {
            ctx.physics3d.stream_colliders(Some(8));
            let focus = ctx
                .world
                .spawn((Transform::at(vec3(10.0, 0.0, 5.0)), ColliderFocus::new(50.0, 60.0)));
            let flat = ctx.physics3d.heightfield(&[0.0; 3 * 3], 10.0);
            let under_crate = ctx.world.spawn((Transform::default(), TerrainCollider::new(flat)));
            let empty = ctx
                .world
                .spawn((Transform::at(vec3(20.0, 0.0, 0.0)), TerrainCollider::new(flat)));
            let crate_body = ctx.world.spawn((
                Transform::at(vec3(5.0, 2.0, 5.0)),
                RigidBody::dynamic(),
                Collider::cuboid(Vec3::ONE),
            ));
            *ids.borrow_mut() = Some((focus, under_crate, empty, crate_body));
        });
    let (crate_ground, empty_ground, crate_y, ray_crate, ray_empty) = report.take().unwrap();
    assert!(crate_ground, "the crate's ground stays");
    assert!(!empty_ground, "the empty tile went");
    assert!((crate_y - 0.5).abs() < 0.05, "the crate still stands: {crate_y}");
    assert!(ray_crate.is_some(), "the crate (or its ground) is hit");
    assert_eq!(ray_empty, None, "a ray passes where the ground went");
}

#[test]
fn turning_streaming_off_brings_every_collider_back() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let out = seen.clone();
    let far = Rc::new(RefCell::new(None));
    let far_in = far.clone();
    Game::new("t", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let e = far_in.borrow().unwrap();
            out.borrow_mut().push(ctx.physics3d.is_collider_active(e));
            if ctx.time.sim_tick() == 1 {
                ctx.physics3d.stream_colliders(None);
            }
        })
        .run_headless(4, |ctx| {
            ctx.physics3d.stream_colliders(Some(4));
            *far.borrow_mut() = Some(ctx.world.spawn((
                Transform::at(vec3(500.0, 0.0, 0.0)),
                RigidBody::fixed(),
                Collider::sphere(1.0),
            )));
        });
    assert_eq!(seen.take(), [false, false, true, true]);
}

// ANCHOR: collider_streaming
/// A streamed tile, built on a worker: its meshes and its collider, from
/// the same heights.
fn build_tile(heights: Vec<f32>) -> ponko::streaming::Job<(TerrainTileData, CollisionMeshData)> {
    ponko::streaming::Job::spawn(move || {
        let drawn = TerrainTileData::new(&heights, 128.0);
        let solid = CollisionMeshData::heightfield(&heights, 128.0);
        (drawn, solid)
    })
}

/// Once built, on the game's thread: drawn and solid. Free both when the
/// tile goes (`ctx.free(tile)` and `ctx.free(mesh)`).
fn place_tile(ctx: &mut Ctx, corner: Vec3, built: (TerrainTileData, CollisionMeshData), ground: Material) -> Entity {
    let (drawn, solid) = built;
    let tile = ctx.gfx.upload_terrain_tile(&drawn, ground);
    let mesh = ctx.physics3d.add_mesh(&solid);
    ctx.world
        .spawn((Transform::at(corner), tile, TerrainCollider::new(mesh)))
}

/// Drawn out to the streamer's 600 m, but solid only near the player and
/// the car (further ahead of the car: it's faster), 4 new colliders a step.
fn solid_near(ctx: &mut Ctx, player: Entity, car: Entity) {
    ctx.physics3d.stream_colliders(Some(4));
    ctx.world.insert(player, ColliderFocus::new(150.0, 200.0));
    ctx.world.insert(car, ColliderFocus::new(300.0, 350.0));
}
// ANCHOR_END: collider_streaming

#[test]
fn the_guide_tiles_are_solid_only_near_the_player_and_the_car() {
    let seen = Rc::new(RefCell::new(None));
    let out = seen.clone();
    let tiles = Rc::new(RefCell::new(Vec::new()));
    let tiles_in = tiles.clone();
    Game::new("t", 1, 1)
        .physics3d(GRAVITY)
        .sim_system(move |ctx: &mut Ctx| {
            if ctx.time.sim_tick() == 5 {
                let active: Vec<bool> = tiles_in
                    .borrow()
                    .iter()
                    .map(|&t| ctx.physics3d.is_collider_active(t))
                    .collect();
                *out.borrow_mut() = Some(active);
            }
        })
        .run_headless(6, |ctx| {
            let ground = ctx.gfx.default_material();
            // Tiles every 128 m along X, out to 1 km.
            for k in 0..8 {
                let built = build_tile(vec![0.0; 33 * 33]).wait();
                let tile = place_tile(ctx, vec3(k as f32 * 128.0, 0.0, 0.0), built, ground);
                tiles.borrow_mut().push(tile);
            }
            let player = ctx
                .world
                .spawn((Transform::at(vec3(10.0, 0.0, 10.0)), Character::new(1.8, 0.3)));
            let car = ctx.world.spawn((
                Transform::at(vec3(900.0, 1.0, 10.0)),
                RigidBody::dynamic(),
                Collider::cuboid(vec3(2.0, 1.0, 4.0)),
            ));
            solid_near(ctx, player, car);
            assert_eq!(ctx.physics3d.collider_streaming(), Some(4));
        });
    let active = seen.take().unwrap();
    // The player's 150 m reaches tiles 0 and 1; the car's 300 m tiles 4 to 7.
    assert_eq!(active, [true, true, false, false, true, true, true, true]);
}

#[test]
fn square_focus_loads_and_keeps_diagonal_colliders_by_axis_distance() {
    use ponko::d3::SquareColliderFocus;

    let readings = Rc::new(RefCell::new(Vec::new()));
    let out = readings.clone();
    Game::new("square focus", 1, 1)
        .physics3d(Vec3::ZERO)
        .sim_system(move |ctx: &mut Ctx| {
            let [focus, inside_corner, keep_corner, outside] = ctx.world.resource::<FocusTestEntities>().0;
            let active = |entity| ctx.physics3d.is_collider_active(entity);
            out.borrow_mut()
                .push([active(inside_corner), active(keep_corner), active(outside)]);
            match ctx.time.sim_tick() {
                0 => ctx.world.get_mut::<Transform>(focus).unwrap().position = vec3(2.0, 0.0, 2.0),
                1 => ctx.world.get_mut::<Transform>(focus).unwrap().position = Vec3::ZERO,
                2 => ctx.world.get_mut::<Transform>(focus).unwrap().position = Vec3::splat(-20.0),
                _ => {}
            }
        })
        .run_headless(4, |ctx| {
            ctx.physics3d.stream_colliders(Some(8));
            let focus = ctx
                .world
                .spawn((Transform::default(), SquareColliderFocus::new(5.0, 7.0)));
            let mut tile = |at| ctx.world.spawn((Transform::at(at), Collider::cuboid(Vec3::splat(0.2))));
            let inside_corner = tile(vec3(4.0, 0.0, 4.0));
            let keep_corner = tile(vec3(6.0, 0.0, 6.0));
            let outside = tile(vec3(12.0, 0.0, 3.0));
            ctx.world
                .insert_resource(FocusTestEntities([focus, inside_corner, keep_corner, outside]));
        });
    let seen = readings.borrow();
    assert_eq!(seen[0], [true, false, false]);
    assert_eq!(seen[1], [true, true, false]);
    assert_eq!(seen[2], [true, true, false]);
    assert_eq!(seen[3], [false, false, false]);
}

struct FocusTestEntities([Entity; 4]);
