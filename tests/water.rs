//! The water kit (`ponko::water`), headless: every preset builds shaders
//! that compile, the CPU wave query is the surface's (heights, normals,
//! the flow map), contact ripples and the underwater effect come and go
//! as they should, nothing is made before water is, and floating bodies
//! settle at the depth their density gives, ride the waves and drift
//! with a river, deterministically on the fixed step. The GPU side (the
//! drawn surface against the CPU heights, the reference images) is in
//! `src/render/water_render_test.rs`.
#![cfg(feature = "3d")]

use ponko::prelude::*;
use ponko::render::Model;
use ponko::water::{FlowMap, Water, WaterDesc, WaterPlugin, WaterQuality, WaterReflection, Wave, Waves};

/// Every kind of water, at every quality and with every reflection.
fn every_desc() -> Vec<WaterDesc> {
    let flow = FlowMap::from_fn(8, 8, 2.0, |uv| vec2(0.0, 2.0 * (1.0 - (uv.x * 2.0 - 1.0).powi(2))));
    let mut out = Vec::new();
    for quality in [WaterQuality::Low, WaterQuality::Medium, WaterQuality::High] {
        for reflection in [
            WaterReflection::Sky,
            WaterReflection::ScreenSpace,
            WaterReflection::Planar,
        ] {
            for desc in [
                WaterDesc::lake(Vec2::ZERO, vec2(20.0, 20.0), 0.0),
                WaterDesc::river(Vec2::ZERO, vec2(6.0, 40.0), 0.0, vec2(0.0, 1.0)).with_flow_map(flow.clone()),
                WaterDesc::sea(0.0, vec2(1.0, 0.2), 1.5),
                WaterDesc::lake(Vec2::ZERO, vec2(5.0, 5.0), 0.0).with_waves(Waves::none()),
            ] {
                out.push(desc.with_quality(quality).with_reflection(reflection));
            }
        }
    }
    out
}

#[test]
fn every_preset_builds_shaders_that_compile() {
    Game::new("water", 320, 180).run_headless(0, |ctx| {
        for desc in every_desc() {
            let e = Water::try_spawn(ctx, desc.clone()).unwrap_or_else(|err| panic!("{desc:?}: {err}"));
            assert!(ponko::water::despawn_water(ctx, e));
        }
        let live = ctx.gfx.live_counts();
        assert_eq!(
            (
                live.materials,
                live.shaders,
                live.meshes,
                live.render_targets,
                live.computes,
                live.effects,
                live.texture_arrays
            ),
            (0, 0, 0, 0, 0, 0, 0),
            "despawning frees everything the water made"
        );
        assert!(ctx.scene3d.passes.compute.is_empty() && ctx.scene3d.passes.target_cameras.is_empty());
    });
}

#[test]
fn a_game_with_the_plugin_and_no_water_makes_nothing() {
    let before = Game::new("plain", 320, 180).run_headless(3, |_| {});
    let with = Game::new("kit", 320, 180).plugin(WaterPlugin).run_headless(3, |ctx| {
        let live = ctx.gfx.live_counts();
        assert_eq!(
            (live.materials, live.shaders, live.meshes, live.render_targets),
            (0, 0, 0, 0)
        );
    });
    let _ = (before, with);
    Game::new("kit", 320, 180)
        .plugin(WaterPlugin)
        .system(|ctx: &mut Ctx| {
            let p = &ctx.scene3d.passes;
            assert!(p.compute.is_empty() && p.effects.is_empty() && p.target_cameras.is_empty());
            let live = ctx.gfx.live_counts();
            assert_eq!(
                (
                    live.materials,
                    live.shaders,
                    live.meshes,
                    live.render_targets,
                    live.computes,
                    live.effects
                ),
                (0, 0, 0, 0, 0, 0),
                "no water, nothing made"
            );
        })
        .run_headless(5, |_| {});
}

#[test]
fn a_water_body_adds_only_what_its_desc_asks_for() {
    Game::new("lake", 320, 180).run_headless(0, |ctx| {
        let plain = WaterDesc {
            ripples: false,
            underwater: false,
            reflection: WaterReflection::ScreenSpace,
            ..WaterDesc::lake(Vec2::ZERO, vec2(10.0, 10.0), 0.0)
        };
        Water::spawn(ctx, plain);
        let live = ctx.gfx.live_counts();
        assert_eq!(
            (
                live.materials,
                live.shaders,
                live.meshes,
                live.render_targets,
                live.computes,
                live.effects
            ),
            (1, 1, 1, 0, 0, 0),
            "a lake without ripples, underwater view or a mirror is one mesh, material and shader"
        );
        assert!(ctx.scene3d.passes.compute.is_empty() && ctx.scene3d.passes.target_cameras.is_empty());
        Water::spawn(
            ctx,
            WaterDesc::lake(Vec2::ZERO, vec2(10.0, 10.0), 0.0).with_reflection(WaterReflection::Planar),
        );
        let live = ctx.gfx.live_counts();
        assert_eq!(
            (live.render_targets, live.computes, live.effects),
            (2, 1, 1),
            "mirror and ripple targets"
        );
        assert_eq!(ctx.scene3d.passes.compute.len(), 1);
    });
}

#[test]
fn the_cpu_height_is_where_the_waves_put_the_surface() {
    // Displace a fine grid of rest points the way the vertex shader does,
    // then ask the query for the height at each displaced point.
    let sea = Waves::sea(vec2(0.7, 0.3), 2.0);
    let t = 3.7;
    let mut worst: f32 = 0.0;
    for i in 0..60 {
        for j in 0..60 {
            let rest = vec2(i as f32 * 0.7 - 20.0, j as f32 * 0.55 - 15.0);
            let moved = sea.displace(rest, t);
            let h = sea.height_at(vec2(moved.x, moved.z), t);
            worst = worst.max((h - moved.y).abs());
        }
    }
    assert!(
        worst < 1e-4,
        "the height query misses the displaced surface by {worst} m"
    );
}

#[test]
fn the_wave_normal_is_the_slope_of_the_height() {
    let sea = Waves::sea(vec2(-0.4, 1.0), 1.0);
    let e = 0.01;
    for i in 0..50 {
        let at = vec2(i as f32 * 1.3 - 30.0, (i as f32 * 0.7).sin() * 10.0);
        let t = i as f32 * 0.21;
        let dx = (sea.height_at(at + vec2(e, 0.0), t) - sea.height_at(at - vec2(e, 0.0), t)) / (2.0 * e);
        let dz = (sea.height_at(at + vec2(0.0, e), t) - sea.height_at(at - vec2(0.0, e), t)) / (2.0 * e);
        let from_slope = vec3(-dx, 1.0, -dz).normalize();
        let n = sea.sample(at, t).normal;
        assert!(
            n.dot(from_slope) > 0.9995,
            "at {at}: normal {n}, slope gives {from_slope}"
        );
    }
}

#[test]
fn flat_water_has_no_waves_and_lake_waves_stay_small() {
    assert_eq!(Waves::none().height_at(vec2(3.0, 4.0), 10.0), 0.0);
    let lake = Waves::lake(Vec2::X, 0.1);
    assert!((lake.max_height() - 0.05).abs() < 1e-5);
    for i in 0..100 {
        assert!(lake.height_at(vec2(i as f32, 0.5), i as f32 * 0.1).abs() <= 0.05 + 1e-5);
    }
    // A long swell rolls along its direction at the deep-water speed.
    let swell = Waves::new(&[Wave::new(Vec2::X, 30.0, 0.5).with_steepness(0.0)]);
    let c = swell.waves()[0].phase_speed();
    let a = swell.height_at(vec2(1.0, 0.0), 2.0);
    let b = swell.height_at(vec2(1.0 + c * 0.5, 0.0), 2.5);
    assert!((a - b).abs() < 1e-4, "the crest moved on at {c} m/s: {a} vs {b}");
}

#[test]
fn a_river_current_follows_its_flow_map() {
    Game::new("river", 320, 180).run_headless(0, |ctx| {
        let map = FlowMap::from_fn(16, 16, 3.0, |uv| vec2(0.0, 3.0 * (1.0 - (uv.x * 2.0 - 1.0).powi(2))));
        let desc = WaterDesc::river(vec2(10.0, 0.0), vec2(8.0, 50.0), 2.0, Vec2::ZERO).with_flow_map(map);
        let e = Water::spawn(ctx, desc);
        let water = ctx.world.get::<Water>(e).unwrap();
        let middle = water.sample(vec2(10.0, 0.0), 0.0).velocity;
        let bank = water.sample(vec2(6.2, 0.0), 0.0).velocity;
        assert!((middle.z - 3.0).abs() < 0.1, "fast in the channel: {middle}");
        assert!(bank.z < 0.5, "slow at the bank: {bank}");
        assert_eq!(water.height_at(vec2(10.0, 3.0), 5.0), 2.0, "a river has no waves");
        assert!(!water.contains(vec2(0.0, 0.0)));
    });
}

#[test]
fn flow_maps_round_trip_through_rgba() {
    let a = FlowMap::from_fn(4, 4, 2.0, |uv| vec2(uv.x * 2.0 - 1.0, uv.y - 0.5));
    let bytes: Vec<u8> = (0..16)
        .flat_map(|i| {
            let v = a.at(vec2(((i % 4) as f32 + 0.5) / 4.0, ((i / 4) as f32 + 0.5) / 4.0)) / 2.0;
            [
                (v.x * 127.0 + 128.0).round() as u8,
                (v.y * 127.0 + 128.0).round() as u8,
                0,
                255,
            ]
        })
        .collect();
    let b = FlowMap::from_rgba(4, 4, 2.0, &bytes);
    for i in 0..10 {
        let uv = vec2(i as f32 * 0.1, 1.0 - i as f32 * 0.1);
        assert!((a.at(uv) - b.at(uv)).length() < 1e-5);
    }
}

#[test]
fn contacts_wait_their_turn_and_stay_in_the_ripple_field() {
    Game::new("ripples", 320, 180).run_headless(0, |ctx| {
        let e = Water::spawn(ctx, WaterDesc::lake(Vec2::ZERO, vec2(20.0, 20.0), 0.0));
        let water = ctx.world.get_mut::<Water>(e).unwrap();
        for i in 0..5 {
            water.touch(vec2(i as f32, 0.0), 0.5, 0.1);
        }
        water.touch(vec2(500.0, 0.0), 0.5, 0.1);
        assert_eq!(water.waiting_contacts(), 5, "a contact outside the field is dropped");
        assert_eq!(water.contacts_started(), 5, "only the accepted ones count");
        ponko::water::update_water(ctx);
        assert_eq!(
            ctx.world.get::<Water>(e).unwrap().contacts_started(),
            5,
            "the update drains the waiting ones, not the total"
        );
        assert_eq!(
            ctx.world.get::<Water>(e).unwrap().waiting_contacts(),
            2,
            "three a frame"
        );
        ponko::water::update_water(ctx);
        assert_eq!(ctx.world.get::<Water>(e).unwrap().waiting_contacts(), 0);
    });
}

#[test]
fn the_underwater_effect_runs_only_while_the_camera_is_under() {
    Game::new("dive", 320, 180).plugin(WaterPlugin).run_headless(0, |ctx| {
        let e = Water::spawn(ctx, WaterDesc::sea(1.0, vec2(1.0, 0.0), 1.0));
        ctx.scene3d.camera.position = vec3(0.0, 8.0, 0.0);
        ponko::water::update_water(ctx);
        assert!(ctx.scene3d.passes.effects.is_empty(), "above the water: no effect");
        assert!(!ctx.world.get::<Water>(e).unwrap().shows_underwater());
        ctx.scene3d.camera.position = vec3(0.0, -3.0, 0.0);
        ponko::water::update_water(ctx);
        assert_eq!(ctx.scene3d.passes.effects.len(), 1, "under the water: the effect");
        ponko::water::update_water(ctx);
        assert_eq!(ctx.scene3d.passes.effects.len(), 1, "added once");
        ctx.scene3d.camera.position = vec3(0.0, 8.0, 0.0);
        ponko::water::update_water(ctx);
        assert!(ctx.scene3d.passes.effects.is_empty(), "back up: gone");
    });
}

#[test]
fn the_sea_mesh_follows_the_camera_and_the_mirror_with_it() {
    Game::new("sea", 320, 180).plugin(WaterPlugin).run_headless(0, |ctx| {
        let desc = WaterDesc::sea(0.0, Vec2::X, 1.0).with_reflection(WaterReflection::Planar);
        let e = Water::spawn(ctx, desc);
        ctx.scene3d.camera.position = vec3(120.0, 10.0, -40.0);
        ponko::water::update_water(ctx);
        let at = ctx.world.get::<ponko::d3::Transform>(e).unwrap().position;
        assert!(
            (at - vec3(120.0, 0.0, -40.0)).length() < 5.0,
            "the mesh is under the camera: {at}"
        );
        let mirror = &ctx.scene3d.passes.target_cameras[0];
        assert_eq!(mirror.camera.position, vec3(120.0, -10.0, -40.0));
        assert!(ctx.world.get::<Model>(e).is_some());
    });
}

#[cfg(feature = "physics3d")]
mod floating {
    use super::*;
    use ponko::d3::{Collider, RigidBody, Transform};
    use ponko::water::Buoyancy;

    /// Where a box of `size` and `density` dropped at `start` into a game
    /// with `water` is after each of `ticks` steps, and the simulation's
    /// time then.
    fn float(water: WaterDesc, size: Vec3, density: f32, start: Vec3, ticks: u64) -> Vec<(f32, Vec3)> {
        struct Track(Entity, Vec<(f32, Vec3)>);
        let world = Game::new("float", 64, 64)
            .physics3d(vec3(0.0, -9.81, 0.0))
            .plugin(WaterPlugin)
            .sim_system(|ctx: &mut Ctx| {
                let e = ctx.world.resource::<Track>().0;
                if let Some(p) = ctx.physics3d.position(e) {
                    let t = ctx.time.sim_time() as f32;
                    ctx.world.resource_mut::<Track>().1.push((t, p));
                }
            })
            .run_headless(ticks, move |ctx| {
                Water::spawn(ctx, water.clone());
                let body = ctx.world.spawn((
                    Transform::at(start),
                    RigidBody::dynamic(),
                    Collider::cuboid(size).density(density),
                    Buoyancy::cuboid(size),
                ));
                ctx.world.insert_resource(Track(body, Vec::new()));
            });
        world.resource::<Track>().1.clone()
    }

    #[test]
    fn a_box_settles_at_the_depth_its_density_gives() {
        for density in [300.0, 500.0, 800.0] {
            let still = WaterDesc::lake(Vec2::ZERO, vec2(40.0, 40.0), 2.0).with_waves(Waves::none());
            let size = vec3(1.0, 0.8, 1.0);
            let path = float(still, size, density, vec3(0.0, 4.0, 0.0), 900);
            let end = path.last().unwrap().1;
            // Under by its share of the water's density: the centre sits
            // h * (0.5 - density / 1000) above the surface.
            let expected = 2.0 + size.y * (0.5 - density / 1000.0);
            assert!(
                (end.y - expected).abs() < 0.02,
                "density {density}: settled at {} (expected {expected})",
                end.y
            );
            let last: Vec<f32> = path[path.len() - 60..].iter().map(|(_, p)| p.y).collect();
            let spread = last.iter().cloned().fold(f32::MIN, f32::max) - last.iter().cloned().fold(f32::MAX, f32::min);
            assert!(spread < 0.01, "density {density}: still bobbing by {spread} m");
        }
    }

    #[test]
    fn a_floating_box_rides_the_waves() {
        let sea = WaterDesc::sea(0.0, vec2(1.0, 0.0), 1.2);
        let waves = sea.waves.clone();
        let size = vec3(0.6, 0.6, 0.6);
        let path = float(sea, size, 500.0, vec3(0.0, 0.5, 0.0), 1200);
        // After it settled: its height follows the surface under it.
        let mut errors = Vec::new();
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for &(t, p) in path.iter().skip(600) {
            let surface = waves.height_at(vec2(p.x, p.z), t);
            errors.push((p.y - surface).abs());
            lowest = lowest.min(p.y);
            highest = highest.max(p.y);
        }
        let mean = errors.iter().sum::<f32>() / errors.len() as f32;
        assert!(mean < 0.15, "the box stays with the surface: mean gap {mean} m");
        assert!(
            highest - lowest > 0.5,
            "it rises and falls with the waves: {lowest}..{highest}"
        );
    }

    #[test]
    fn floating_is_deterministic_on_the_fixed_step() {
        let run = || {
            float(
                WaterDesc::sea(0.0, vec2(0.3, 1.0), 1.0),
                Vec3::splat(0.5),
                450.0,
                vec3(0.5, 1.0, -0.5),
                300,
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_floating_box_drifts_downstream() {
        let river = WaterDesc::river(Vec2::ZERO, vec2(20.0, 200.0), 0.0, vec2(0.0, 1.5));
        let path = float(river, Vec3::splat(0.5), 500.0, vec3(0.0, 0.2, 0.0), 600);
        let (t1, end) = *path.last().unwrap();
        assert!(end.z > 8.0, "carried downstream: {end}");
        let (t0, before) = path[path.len() - 61];
        let speed = (end.z - before.z) / (t1 - t0);
        assert!((speed - 1.5).abs() < 0.2, "at the current's speed: {speed} m/s");
    }

    #[test]
    fn a_body_falling_in_starts_ripples_only_with_its_ripple_switch_on() {
        let started = |ripples: bool| {
            let mut lake = None;
            let world = Game::new("splash", 64, 64)
                .physics3d(vec3(0.0, -9.81, 0.0))
                .plugin(WaterPlugin)
                .run_headless(60, |ctx| {
                    lake = Some(Water::spawn(ctx, WaterDesc::lake(Vec2::ZERO, vec2(20.0, 20.0), 0.0)));
                    let mut floats = Buoyancy::cuboid(Vec3::splat(0.4));
                    floats.ripples = ripples;
                    ctx.world.spawn((
                        Transform::at(vec3(1.0, 1.5, 0.0)),
                        RigidBody::dynamic(),
                        Collider::cuboid(Vec3::splat(0.4)).density(500.0),
                        floats,
                    ));
                });
            // The water's update takes the waiting contacts every frame; the
            // running total is what the splash left, read once after the run.
            let water = world.get::<Water>(lake.unwrap()).unwrap();
            water.contacts_started()
        };
        assert!(started(true) > 0, "the splash started contact ripples");
        assert_eq!(started(false), 0, "a body with the switch off starts none");
    }
}

#[cfg(feature = "physics3d")]
#[path = "../examples/water/game.rs"]
#[allow(dead_code)]
mod game;

/// The water example (`examples/water`), run without a window.
#[cfg(feature = "physics3d")]
mod example {
    use super::game::*;
    use ponko::d3::Transform;
    use ponko::prelude::*;
    use ponko::water::{Water, WaterQuality, WaterReflection};

    fn demo() -> Game {
        Game::new("water test", 320, 180)
            .physics3d(vec3(0.0, -9.81, 0.0))
            .plugin(WaterDemo)
    }

    /// The water surface over `at` (the highest body there) at `t`.
    fn surface(world: &World, at: Vec3, t: f32) -> Option<f32> {
        let demo = world.resource::<Demo>();
        [demo.lake, demo.river, demo.sea]
            .into_iter()
            .filter_map(|e| world.get::<Water>(e))
            .filter(|w| w.contains(vec2(at.x, at.z)))
            .map(|w| w.height_at(vec2(at.x, at.z), t))
            .reduce(f32::max)
    }

    #[test]
    fn the_examples_crates_float_and_the_river_carries_its_own() {
        struct Clock(f32);
        let mut start = Vec3::ZERO;
        let world = demo()
            .sim_system(|ctx: &mut Ctx| {
                let t = ctx.time.sim_time() as f32;
                ctx.world.insert_resource(Clock(t));
            })
            .run_headless(600, |ctx: &mut Ctx| {
                let river_crate = ctx.world.resource::<Demo>().crates[2];
                start = ctx.world.get::<Transform>(river_crate).unwrap().position;
            });
        let demo = world.resource::<Demo>();
        assert_eq!(demo.crates.len(), 6);
        let t = world.resource::<Clock>().0;
        // The lake's and the sea's crates sit at the surface (half as
        // dense as water: their centres at the waterline).
        for (i, &e) in demo.crates.iter().enumerate().filter(|&(i, _)| i != 2) {
            let at = world.get::<Transform>(e).unwrap().position;
            let water = surface(&world, at, t).expect("over water");
            assert!(
                (at.y - water).abs() < 0.6,
                "crate {i} at {at} floats on water at {water}"
            );
        }
        let lake = world.get::<Water>(demo.lake).unwrap();
        for &e in &demo.crates[..2] {
            let at = world.get::<Transform>(e).unwrap().position;
            assert!(lake.contains(vec2(at.x, at.z)), "the lake's crates stay in it: {at}");
        }
        // The river's crate went downstream (towards +x), out of the river.
        let at = world.get::<Transform>(demo.crates[2]).unwrap().position;
        assert!(at.x > start.x + 3.0, "carried downstream from {start} to {at}");
    }

    #[test]
    fn switching_quality_remakes_the_water_without_leaking() {
        let world = demo().run_headless(3, |ctx: &mut Ctx| {
            let before = ctx.gfx.live_counts();
            set_quality(ctx, WaterQuality::High, WaterReflection::ScreenSpace);
            set_quality(ctx, WaterQuality::Low, WaterReflection::Sky);
            set_quality(ctx, WaterQuality::Medium, WaterReflection::Planar);
            assert_eq!(ctx.gfx.live_counts(), before, "the same GPU resources as at the start");
        });
        let mut world = world;
        assert_eq!(world.query::<&Water>().count(), 3);
        let demo = world.resource::<Demo>();
        assert_eq!(
            (demo.quality, demo.reflection),
            (WaterQuality::Medium, WaterReflection::Planar)
        );
        let lake = world.get::<Water>(demo.lake).unwrap();
        assert!(lake.reflection_target().is_some());
    }
}
