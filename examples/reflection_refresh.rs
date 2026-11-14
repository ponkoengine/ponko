//! Reflection probes that refresh by themselves through a day. Mirror balls
//! and a polished floor stand in a courtyard; the sun crosses the sky in a
//! minute. The courtyard's probe has a `ReflectionRefresh` that captures it
//! again whenever the sun or sky has changed by about a degree, a step a
//! frame, and eases each new capture in: the reflections follow the sky
//! from dawn to night with no pop.
//!
//! Space switches the refresh off (the probe keeps its last capture, as a
//! probe baked once would) and on again.
//!
//!   `cargo run --release --example reflection_refresh --features 3d`
//!
//! `SHOT=path.png` saves frame 120 and quits.

#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData, Sky};

fn main() {
    Game::new("Ponko: reflection refresh", 1280, 720)
        .system(|ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            // A day in a minute: from an hour before sunrise to an hour
            // after sunset, then again.
            let hours = 5.0 + (ctx.time.elapsed() as f32 * 0.25) % 15.0;
            ctx.scene3d.lighting.sun_direction = Sky::sun_direction_at(hours);
            if ctx.input.key_pressed(Key::Space) {
                ctx.world.query::<&mut ReflectionRefresh>().for_each(|refresh| {
                    refresh.when = match refresh.when {
                        ProbeRefresh::Never => ProbeRefresh::LightingChanged(0.02),
                        _ => ProbeRefresh::Never,
                    };
                    println!("refresh: {:?}", refresh.when);
                });
            }
            if ctx.time.frame() == 120
                && let Ok(path) = std::env::var("SHOT")
            {
                ctx.gfx.save_screenshot(path);
            }
            if ctx.time.frame() == 122 && std::env::var("SHOT").is_ok() {
                ctx.quit();
            }
        })
        .setup(|ctx| {
            ctx.scene3d.camera.position = vec3(0.0, 2.5, 9.0);
            ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
            ctx.scene3d.sky.atmosphere = true;

            let floor = ctx.gfx.create_material(
                MaterialDesc::color(Color::hex(0x404850))
                    .with_metallic(0.0)
                    .with_roughness(0.15),
            );
            let wall = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::hex(0xC0A080)).with_roughness(0.8));
            let chrome = ctx.gfx.create_material(
                MaterialDesc::color(Color::WHITE)
                    .with_metallic(1.0)
                    .with_roughness(0.05),
            );
            let brushed = ctx.gfx.create_material(
                MaterialDesc::color(Color::hex(0xE0B080))
                    .with_metallic(1.0)
                    .with_roughness(0.35),
            );
            let plane = ctx.gfx.create_mesh(&MeshData::plane(20.0), floor);
            ctx.world.spawn((Transform::default(), Model::new(plane)));
            // Low walls on three sides: the sky stays in view above them.
            let block = ctx.gfx.create_mesh(&MeshData::cube(1.0), wall);
            for (at, scale) in [
                (vec3(0.0, 1.0, -6.0), vec3(12.0, 2.0, 0.4)),
                (vec3(-6.0, 1.0, 0.0), vec3(0.4, 2.0, 12.0)),
                (vec3(6.0, 1.0, 0.0), vec3(0.4, 2.0, 12.0)),
            ] {
                ctx.world.spawn((
                    Transform {
                        scale,
                        ..Transform::at(at)
                    },
                    Model::new(block),
                ));
            }
            let ball = ctx.gfx.create_mesh(&MeshData::sphere(1.0, 48), chrome);
            let rough_ball = ctx.gfx.create_mesh(&MeshData::sphere(1.0, 48), brushed);
            for (x, mesh) in [(-2.5, ball), (0.0, rough_ball), (2.5, ball)] {
                ctx.world.spawn((Transform::at(vec3(x, 1.0, 0.0)), Model::new(mesh)));
            }

            // The probe covers the courtyard, captured from above the balls.
            // No bake: the refresh fills it on the first frames, then keeps
            // it up to date.
            ctx.world.spawn((
                Transform::at(vec3(0.0, 3.5, 0.0)),
                ReflectionProbe {
                    size: vec3(12.0, 8.0, 12.0),
                    ..Default::default()
                },
                ReflectionRefresh {
                    when: ProbeRefresh::LightingChanged(0.02),
                    blend_seconds: 0.5,
                },
            ));
        })
        .run();
}
