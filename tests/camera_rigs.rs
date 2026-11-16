//! Camera rigs in running games: the same camera at 30, 60 and 144 frames a
//! second, and the third-person camera kept out of walls by the 3D physics.

use std::cell::RefCell;
use std::rc::Rc;

use ponko::camera::CameraShake;
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
}
