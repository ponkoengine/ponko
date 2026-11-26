//! Camera rigs in running games: the same camera at 30, 60 and 144 frames a
//! second, and the third-person camera kept out of walls by the 3D physics.

use std::cell::RefCell;
use std::rc::Rc;

use ponko::camera::{CameraShake, CameraTarget};
use ponko::d2::{FollowCamera, PixelPerfect};
use ponko::prelude::*;

/// Where the 2D follow camera is after one second of a hero running right
/// and up at a steady speed, with frames at `hz`.
fn follow_2d_at(hz: u32) -> Vec2 {
    let out = Rc::new(RefCell::new(Vec2::ZERO));
    let seen = out.clone();
    let mut follow = FollowCamera::default()
        .with_dead_zone(vec2(24.0, 16.0))
        .with_look_ahead(vec2(32.0, 0.0))
        .with_bounds(Rect::new(0.0, 0.0, 2000.0, 1000.0));
    let mut frames = 0u32;
    Game::new("follow", 640, 360)
        .fixed_hz(hz)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let t = frames as f32 / hz as f32; // the first frame is at 0 s
            frames += 1;
            let hero = vec2(300.0, 600.0) + vec2(150.0, -60.0) * t;
            follow.update(&mut ctx.camera, hero, ctx.time.frame_dt());
            *seen.borrow_mut() = ctx.camera.center();
        })
        .run_headless(u64::from(hz) + 1, |_| {});
    out.take()
}

#[test]
fn the_2d_follow_camera_is_where_it_was_at_30_60_and_144_hz() {
    let ends = [30, 60, 144].map(follow_2d_at);
    for e in &ends[1..] {
        assert!(e.distance(ends[0]) < 0.01, "{ends:?}");
    }
    // The hero is at (450, 540). The camera: the dead zone's edge (12 and
    // 8 units), 32 ahead on x, minus the steady lag of speed x smooth time.
    let expect = vec2(450.0 - 12.0 + 32.0 - 150.0 * 0.15, 540.0 + 8.0 + 60.0 * 0.15);
    assert!(ends[0].distance(expect) < 0.01, "{ends:?}");
}

#[test]
fn pixel_perfect_follows_the_window_size() {
    Game::new("retro", 1366, 768).run_headless(1, |ctx| {
        let pixels = PixelPerfect::new(320.0, 180.0);
        ctx.camera.look_at(vec2(160.3, 90.7));
        pixels.apply(&mut ctx.camera);
        assert_eq!(ctx.camera.zoom, 4.0, "the largest whole scale that fits");
        assert_eq!(
            ctx.camera.position,
            ctx.camera.position.round(),
            "on the art pixel grid"
        );
        let bars = pixels.screen_rect(ctx.camera.viewport());
        assert_eq!(bars.size(), vec2(1280.0, 720.0));
        // A sprite placed by hand sits on the same grid.
        let coin = pixels.snap_point(vec2(10.4, 3.6), ctx.camera.zoom);
        assert_eq!(coin, vec2(10.0, 4.0));
    });
}

#[test]
fn a_shaken_camera_returns_to_rest_when_the_trauma_is_gone() {
    let mut shake = CameraShake::default();
    shake.add_trauma(1.0);
    let rest = vec2(100.0, 100.0);
    let mut moved = 0.0f32;
    Game::new("shake", 320, 180)
        .fixed_hz(60)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            shake.update(ctx.time.frame_dt());
            ctx.camera.look_at(rest);
            shake.apply_2d(&mut ctx.camera);
            moved = moved.max(ctx.camera.center().distance(rest));
            if ctx.time.frame() > 70 {
                assert_eq!(ctx.camera.center(), rest, "a second of decay ends the shake");
                assert!(moved > 1.0, "it shook first");
            }
        })
        .run_headless(80, |_| {});
}

#[test]
fn a_shake_can_drive_a_camera_of_the_games_own() {
    // A game with its own camera code reads the offsets instead of `apply_2d`.
    let mut shake = CameraShake::default();
    assert_eq!(shake.offset_2d(), Vec2::ZERO);
    shake.add_trauma(1.0);
    let (mut moved, mut turned) = (0.0f32, 0.0f32);
    for _ in 0..30 {
        shake.update(1.0 / 60.0);
        let (offset, angles) = (shake.offset_2d(), shake.angles());
        assert!(offset.abs().cmple(shake.max_offset).all(), "{offset}");
        assert!(angles.abs().cmple(shake.max_angles).all(), "{angles}");
        moved = moved.max(offset.length());
        turned = turned.max(angles.length());
    }
    assert!(moved > 0.0 && turned > 0.0);
    for _ in 0..60 {
        shake.update(1.0 / 60.0);
    }
    assert_eq!((shake.offset_2d(), shake.angles()), (Vec2::ZERO, Vec3::ZERO));
}

/// The hero: moved 2 units right every tick.
#[derive(Clone, Copy, Component)]
struct Runner;

fn run_right(ctx: &mut Ctx) {
    ctx.world
        .query::<(&mut d2::Transform, &Runner)>()
        .for_each(|(t, _)| t.position.x += 2.0);
}

#[test]
fn a_follow_camera_resource_follows_the_camera_target_with_no_camera_system() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    Game::new("fed", 640, 360)
        .headless_frames(true)
        .sim_system(run_right)
        // Late systems see the camera the engine placed this frame.
        .add(Phase::Late, move |ctx: &mut Ctx| {
            let hero = ctx
                .world
                .query::<(&d2::Transform, &Runner)>()
                .iter()
                .next()
                .unwrap()
                .0
                .position;
            seen.borrow_mut().push((hero, ctx.camera.center()));
        })
        .run_headless(30, |ctx| {
            ctx.world.insert_resource(FollowCamera::default().with_smooth_time(0.0));
            ctx.world
                .spawn((d2::Transform::at(vec2(100.0, 50.0)), Runner, CameraTarget));
        });
    let log = log.borrow();
    assert_eq!(log.len(), 30);
    for (hero, center) in log.iter() {
        assert!(hero.distance(*center) < 1e-3, "on the hero: {center} for {hero}");
    }
    assert!(log.last().unwrap().0.x > 150.0, "the hero ran");
}

#[test]
fn a_new_camera_target_is_cut_to_and_no_target_leaves_the_camera_alone() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let seen = log.clone();
    let mut tick = 0;
    Game::new("cut", 640, 360)
        .headless_frames(true)
        .sim_system(run_right)
        .sim_system(move |ctx: &mut Ctx| {
            tick += 1;
            let heroes: Vec<Entity> = ctx.world.query::<(Entity, &Runner)>().iter().map(|(e, _)| e).collect();
            if tick == 20 {
                // Lost: no target for a while; the game shows a spot of its own.
                for e in heroes {
                    ctx.world.despawn(e);
                }
                ctx.camera.look_at(vec2(-500.0, -500.0));
            } else if tick == 30 {
                // Respawned far away.
                ctx.world
                    .spawn((d2::Transform::at(vec2(3000.0, 50.0)), Runner, CameraTarget));
            }
        })
        .add(Phase::Late, move |ctx: &mut Ctx| {
            let hero = ctx
                .world
                .query::<(&d2::Transform, &Runner)>()
                .iter()
                .next()
                .map(|(t, _)| t.position);
            seen.borrow_mut().push((hero, ctx.camera.center()));
        })
        .run_headless(40, |ctx| {
            ctx.world.insert_resource(FollowCamera::default().with_smooth_time(0.5));
            ctx.world
                .spawn((d2::Transform::at(vec2(100.0, 50.0)), Runner, CameraTarget));
        });
    let log = log.borrow();
    // The first frame cuts to the hero; then the camera eases after it, behind.
    let (hero, center) = log[0];
    assert!(hero.unwrap().distance(center) < 1e-3, "{log:?}");
    let (hero, center) = log[15];
    assert!(center.x < hero.unwrap().x - 1.0, "easing behind the runner: {log:?}");
    // No target: the game's own camera position stays.
    for (hero, center) in &log[20..29] {
        assert!(
            hero.is_none() && center.distance(vec2(-500.0, -500.0)) < 1e-3,
            "{log:?}"
        );
    }
    // The new target is cut to, not eased to from across the level.
    let (hero, center) = log[29];
    assert!(hero.unwrap().distance(center) < 1e-3, "{log:?}");
}

#[test]
fn the_follow_camera_keeps_a_pixel_perfect_grid() {
    Game::new("retro", 1280, 720).run_headless(1, |ctx| {
        let mut follow = FollowCamera::default()
            .with_smooth_time(0.2)
            .with_pixel_perfect(PixelPerfect::new(320.0, 180.0))
            .with_bounds(Rect::new(0.0, 0.0, 1000.0, 180.0));
        follow.update(&mut ctx.camera, vec2(10.3, 90.0), 0.0);
        assert_eq!(ctx.camera.zoom, 4.0);
        // The bounds were applied at the zoom: the left edge of the level.
        assert_eq!(ctx.camera.visible_rect().min.x, 0.0);
        for i in 1..20 {
            follow.update(&mut ctx.camera, vec2(10.3 + 7.7 * i as f32, 90.0), 1.0 / 60.0);
            assert_eq!(ctx.camera.position, ctx.camera.position.round());
        }
    });
}

#[cfg(feature = "3d")]
mod three {
    use super::*;
    use ponko::d3::{OrbitCamera, ThirdPersonCamera};

    /// Where the third-person camera is after half a second of a character
    /// running at a steady speed while the player turns the view.
    fn chase_at(hz: u32) -> (Vec3, Quat) {
        let out = Rc::new(RefCell::new((Vec3::ZERO, Quat::IDENTITY)));
        let seen = out.clone();
        let mut rig = ThirdPersonCamera::default();
        rig.follow_smoothing = 0.1;
        let mut orbit = OrbitCamera::new(Vec3::ZERO, 8.0);
        orbit.smoothing = 0.1;
        let mut frames = 0u32;
        Game::new("chase", 800, 600)
            .fixed_hz(hz)
            .headless_frames(true)
            .system(move |ctx: &mut Ctx| {
                let t = frames as f32 / hz as f32; // the first frame is at 0 s
                frames += 1;
                let dt = ctx.time.frame_dt();
                // Half a radian a second of mouse motion, spread over the
                // frames (none before the first).
                let moving = if t > 0.0 { dt } else { 0.0 };
                rig.look(vec2(0.5 / rig.sensitivity * moving, 0.0));
                rig.update(&mut ctx.scene3d.camera, vec3(3.0, 0.0, -5.0) * t, dt);
                let chase = (ctx.scene3d.camera.position, ctx.scene3d.camera.rotation);
                // And an orbit panning steadily, on another camera.
                let mut other = ctx.scene3d.camera;
                orbit.target += vec3(1.0, 0.0, 0.5) * moving;
                orbit.update(&mut other, dt);
                *seen.borrow_mut() = (chase.0 + other.position, chase.1);
            })
            .run_headless(u64::from(hz / 2) + 1, |_| {});
        out.take()
    }

    #[test]
    fn the_3d_rigs_are_where_they_were_at_30_60_and_144_hz() {
        let ends = [30, 60, 144].map(chase_at);
        for (p, r) in &ends[1..] {
            assert!(p.distance(ends[0].0) < 1e-3, "{ends:?}");
            assert!(r.abs_diff_eq(ends[0].1, 1e-5), "{ends:?}");
        }
    }

    #[test]
    fn world_and_screen_points_convert_both_ways_on_the_game_camera() {
        Game::new("pick", 1280, 720).run_headless(1, |ctx| {
            let cam = &mut ctx.scene3d.camera;
            cam.position = vec3(4.0, 8.0, 12.0);
            cam.look_at(Vec3::ZERO);
            let ground = cam.screen_to_ground(vec2(640.0, 360.0), 0.0).unwrap();
            assert!(ground.abs_diff_eq(Vec3::ZERO, 1e-3), "{ground}");
            let back = cam.world_to_screen(ground).unwrap();
            assert!(back.abs_diff_eq(vec2(640.0, 360.0), 1e-2));
            let p = cam.screen_to_world(vec2(100.0, 650.0), 5.0);
            assert!(cam.world_to_screen(p).unwrap().abs_diff_eq(vec2(100.0, 650.0), 1e-2));
        });
    }

    #[test]
    fn rigs_take_obstacles_and_panning_from_the_game() {
        // An editor view: panned by the keyboard rather than the mouse.
        let mut orbit = OrbitCamera::new(Vec3::ZERO, 8.0);
        orbit.pan(2.0, 1.0);
        // At yaw 0 right is +X; up is the view's, tilted by the pitch.
        assert!((orbit.target.x - 2.0).abs() < 1e-5, "{}", orbit.target);
        assert!((vec2(orbit.target.y, orbit.target.z).length() - 1.0).abs() < 1e-5 && orbit.target.y > 0.0);
        // A game without 3D physics: the room's back wall, at z = 2, is the only obstacle.
        let mut rig = ThirdPersonCamera::default();
        rig.pitch = 0.0;
        rig.follow_smoothing = 0.0;
        Game::new("room", 800, 600).run_headless(0, move |ctx| {
            let camera = &mut ctx.scene3d.camera;
            for _ in 0..240 {
                rig.update_with(camera, Vec3::ZERO, 1.0 / 60.0, |from, dir, max| {
                    (dir.z > 0.0).then(|| (2.0 - from.z) / dir.z).filter(|&d| d < max)
                });
            }
            assert!(
                (camera.position.z - 2.0).abs() < 1e-3,
                "stops at the wall: {}",
                camera.position
            );
        });
    }

    #[cfg(feature = "physics3d")]
    #[test]
    fn the_third_person_camera_stays_in_front_of_walls() {
        use ponko::d3::{Character, Collider, QueryFilter, RigidBody, Transform};
        let log = Rc::new(RefCell::new(Vec::new()));
        let seen = log.clone();
        let mut rig = ThirdPersonCamera::default();
        rig.pitch = 0.0;
        Game::new("cave", 800, 600)
            .physics3d(Vec3::ZERO)
            .headless_frames(true)
            .system(move |ctx: &mut Ctx| {
                let hero = ctx
                    .world
                    .query::<(Entity, &Transform, &Character)>()
                    .iter()
                    .next()
                    .map(|(e, t, _)| (e, t.position));
                if let Some((who, at)) = hero {
                    let filter = QueryFilter::default().exclude(who);
                    let dt = ctx.time.frame_dt();
                    rig.update_with_physics(&mut ctx.scene3d.camera, at, &ctx.physics3d, filter, dt);
                    seen.borrow_mut().push((ctx.time.frame(), ctx.scene3d.camera.position));
                }
            })
            .sim_system(|ctx: &mut Ctx| {
                if ctx.time.sim_tick() == 30 {
                    // The wall goes: the camera eases back out.
                    let walls: Vec<Entity> = ctx
                        .world
                        .query::<(Entity, &Collider)>()
                        .iter()
                        .map(|(e, _)| e)
                        .collect();
                    let walls: Vec<Entity> = walls
                        .into_iter()
                        .filter(|&e| ctx.world.get::<Character>(e).is_none())
                        .collect();
                    for w in walls {
                        ctx.world.despawn(w);
                    }
                }
            })
            .run_headless(120, |ctx| {
                ctx.world.spawn((Transform::default(), Character::new(1.8, 0.35)));
                // A wall whose near face is 2 units behind the hero's head.
                let wall = Transform::at(vec3(0.0, 1.5, 2.5));
                ctx.world
                    .spawn((wall, RigidBody::fixed(), Collider::cuboid(vec3(10.0, 10.0, 1.0))));
            });
        let log = log.borrow();
        let near = log.iter().find(|(f, _)| *f == 20).unwrap().1;
        assert!(
            near.z <= 2.0 - rig.collision_radius + 1e-2 && near.z > 1.0,
            "in front of the wall: {near}"
        );
        let far = log.last().unwrap().1;
        // 1.5 s after the wall went: 7.5 half-lives of easing out.
        assert!((far.z - rig.distance).abs() < 0.02, "back out to its distance: {far}");
    }
    #[test]
    fn three_d_rig_resources_follow_the_camera_target() {
        use ponko::d3::{FirstPersonCamera, Transform};
        let at = vec3(4.0, 0.0, -2.0);
        // Third person, sticking to the target: where the rig puts the camera itself.
        let mut rig = ThirdPersonCamera::default();
        rig.follow_smoothing = 0.0;
        let seen = Rc::new(RefCell::new(None));
        let out = seen.clone();
        Game::new("chase", 800, 600)
            .headless_frames(true)
            .add(Phase::Late, move |ctx: &mut Ctx| {
                let mut expected = ctx.scene3d.camera;
                let mut fresh = rig;
                fresh.update(&mut expected, at, 0.0);
                *out.borrow_mut() = Some((ctx.scene3d.camera, expected));
            })
            .run_headless(3, move |ctx| {
                ctx.world.insert_resource(rig);
                ctx.world.spawn((Transform::at(at), CameraTarget));
            });
        let (camera, expected) = seen.borrow().unwrap();
        assert!(
            camera.position.abs_diff_eq(expected.position, 1e-4),
            "{} vs {}",
            camera.position,
            expected.position
        );
        assert!(camera.rotation.abs_diff_eq(expected.rotation, 1e-5));
        assert!(camera.position.z > at.z + 1.0, "behind the target");
        // First person: at the eye, `height` above the target.
        let seen = Rc::new(RefCell::new(None));
        let out = seen.clone();
        Game::new("walk", 800, 600)
            .headless_frames(true)
            .add(Phase::Late, move |ctx: &mut Ctx| {
                *out.borrow_mut() = Some(ctx.scene3d.camera)
            })
            .run_headless(2, move |ctx| {
                let mut eyes = FirstPersonCamera::default();
                eyes.yaw = 0.5;
                ctx.world.insert_resource(eyes);
                ctx.world.spawn((Transform::at(at), CameraTarget));
            });
        let camera = seen.borrow().unwrap();
        assert!(
            camera.position.abs_diff_eq(at + Vec3::Y * 1.6, 1e-5),
            "{}",
            camera.position
        );
        assert!(
            camera.forward().x < -0.4,
            "turned left by the yaw: {}",
            camera.forward()
        );
    }

    #[cfg(feature = "physics3d")]
    #[test]
    fn a_fed_third_person_camera_stays_in_front_of_walls_but_not_its_targets_own_collider() {
        use ponko::d3::{Character, Collider, RigidBody, Transform};
        let seen = Rc::new(RefCell::new(None));
        let out = seen.clone();
        let mut rig = ThirdPersonCamera::default();
        rig.pitch = 0.0;
        Game::new("cave", 800, 600)
            .physics3d(Vec3::ZERO)
            .headless_frames(true)
            .add(Phase::Late, move |ctx: &mut Ctx| {
                *out.borrow_mut() = Some(ctx.scene3d.camera.position)
            })
            .run_headless(10, move |ctx| {
                ctx.world.insert_resource(rig);
                // The character's own capsule is around the pivot: not a wall.
                ctx.world
                    .spawn((Transform::default(), Character::new(1.8, 0.35), CameraTarget));
                let wall = Transform::at(vec3(0.0, 1.5, 2.5));
                ctx.world
                    .spawn((wall, RigidBody::fixed(), Collider::cuboid(vec3(10.0, 10.0, 1.0))));
            });
        let at = seen.borrow().unwrap();
        assert!(
            at.z <= 2.0 - rig.collision_radius + 1e-2 && at.z > 1.0,
            "in front of the wall: {at}"
        );
    }
}
