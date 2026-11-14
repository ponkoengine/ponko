//! A sky of the game's own: a painted sky shader (`assets/shaders/
//! stylized_sky.wgsl`) with banded colours and flat clouds, through a day
//! in a minute. The fog, the mirror balls, the polished floor and the
//! ambient light all take the painted sky's colours, not the built-in
//! atmosphere's. The sun keeps the colours set here (warm at dusk).
//!
//! Space switches between the painted sky and the built-in atmosphere; A
//! switches the ambient light between the sky's own and authored colours.
//! The shader reloads when its file is saved.
//!
//!   `cargo run --release --example stylized_sky --features 3d`
//!
//! `SHOT=path.png` saves frame 120 and quits; `HOUR=19` holds the time.

#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData, Sky, SkyShader};

/// The painted sky's colours at one time of day: horizon, overhead,
/// ground and clouds (linear), cloud cover, and the sun's colour and
/// intensity.
struct Palette {
    horizon: Vec3,
    overhead: Vec3,
    ground: Vec3,
    clouds: Vec3,
    cover: f32,
    sun: Vec3,
    sun_intensity: f32,
}

const NIGHT: Palette = Palette {
    horizon: Vec3::new(0.03, 0.04, 0.10),
    overhead: Vec3::new(0.005, 0.008, 0.03),
    ground: Vec3::new(0.01, 0.01, 0.02),
    clouds: Vec3::new(0.04, 0.05, 0.09),
    cover: 0.4,
    sun: Vec3::new(0.4, 0.5, 1.0),
    sun_intensity: 0.0,
};

const DUSK: Palette = Palette {
    horizon: Vec3::new(1.2, 0.45, 0.15),
    overhead: Vec3::new(0.25, 0.12, 0.45),
    ground: Vec3::new(0.12, 0.06, 0.08),
    clouds: Vec3::new(1.4, 0.6, 0.5),
    cover: 0.6,
    sun: Vec3::new(1.0, 0.55, 0.3),
    sun_intensity: 2.0,
};

const DAY: Palette = Palette {
    horizon: Vec3::new(0.85, 0.95, 0.9),
    overhead: Vec3::new(0.15, 0.45, 0.85),
    ground: Vec3::new(0.2, 0.25, 0.15),
    clouds: Vec3::new(1.3, 1.3, 1.25),
    cover: 0.5,
    sun: Vec3::new(1.0, 0.95, 0.85),
    sun_intensity: 3.0,
};

/// The palette with the sun `height` (its direction's y, towards the sun)
/// above the horizon: night below it, dusk around it, day above.
fn palette(height: f32) -> Palette {
    let mix = |a: &Palette, b: &Palette, t: f32| Palette {
        horizon: a.horizon.lerp(b.horizon, t),
        overhead: a.overhead.lerp(b.overhead, t),
        ground: a.ground.lerp(b.ground, t),
        clouds: a.clouds.lerp(b.clouds, t),
        cover: a.cover + (b.cover - a.cover) * t,
        sun: a.sun.lerp(b.sun, t),
        sun_intensity: a.sun_intensity + (b.sun_intensity - a.sun_intensity) * t,
    };
    if height < 0.0 {
        mix(&DUSK, &NIGHT, (-height / 0.2).min(1.0))
    } else {
        mix(&DUSK, &DAY, (height / 0.4).min(1.0))
    }
}

/// Sets the painted sky and the sun for `hours` o'clock.
fn set_time(ctx: &mut Ctx, hours: f32) {
    let sun = Sky::sun_direction_at(hours);
    ctx.scene3d.lighting.sun_direction = sun;
    let p = palette(-sun.y);
    let color = |c: Vec3| Color::rgb(c.x, c.y, c.z);
    ctx.scene3d.lighting.sun_color = color(p.sun);
    ctx.scene3d.lighting.sun_intensity = p.sun_intensity;
    let sky = &mut ctx.scene3d.sky;
    sky.shader_params = [
        p.horizon.extend(0.0),
        p.overhead.extend(p.cover),
        p.ground.extend(0.0),
        p.clouds.extend(0.0),
    ];
    // The authored ambient colours, for A: the sky's overhead and ground.
    sky.sky_color = color(p.overhead.lerp(p.horizon, 0.5));
    sky.ground_color = color(p.ground);
}

/// The painted sky shader (loaded once: loading it again returns the same).
fn painted_sky(ctx: &mut Ctx) -> SkyShader {
    ctx.gfx.load_sky_shader("shaders/stylized_sky.wgsl")
}

fn main() {
    Game::new("Ponko: stylized sky", 1280, 720)
        .system(|ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            // A day in a minute, from before sunrise to after sunset.
            let hours = match std::env::var("HOUR").ok().and_then(|h| h.parse().ok()) {
                Some(hour) => hour,
                None => 5.0 + (ctx.time.elapsed() as f32 * 0.25) % 15.0,
            };
            set_time(ctx, hours);
            if ctx.input.key_pressed(Key::Space) {
                let painted = painted_sky(ctx);
                let sky = &mut ctx.scene3d.sky;
                sky.shader = match sky.shader {
                    Some(_) => None,
                    None => Some(painted),
                };
                println!("sky: {}", if sky.shader.is_some() { "painted" } else { "atmosphere" });
            }
            if ctx.input.key_pressed(Key::A) {
                let sky = &mut ctx.scene3d.sky;
                sky.authored_ambient = !sky.authored_ambient;
                println!("authored ambient: {}", sky.authored_ambient);
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
            ctx.scene3d.sky.shader = Some(painted_sky(ctx));
            ctx.scene3d.camera.position = vec3(0.0, 2.0, 9.0);
            ctx.scene3d.camera.look_at(vec3(0.0, 1.5, 0.0));
            // Fog that takes the painted sky's colour behind it, thickest
            // near the ground.
            ctx.scene3d.sky.fog_density = 0.03;
            ctx.scene3d.sky.fog_falloff = 8.0;
            ctx.scene3d.sky.fog_sun_glow = 0.05;

            let floor = ctx.gfx.create_material(
                MaterialDesc::color(Color::hex(0x506070))
                    .with_metallic(0.0)
                    .with_roughness(0.1),
            );
            let chrome = ctx.gfx.create_material(
                MaterialDesc::color(Color::WHITE)
                    .with_metallic(1.0)
                    .with_roughness(0.05),
            );
            let matte = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::hex(0xE8E0D0)).with_roughness(0.9));
            let stone = ctx
                .gfx
                .create_material(MaterialDesc::color(Color::hex(0x8A8078)).with_roughness(0.8));
            let plane = ctx.gfx.create_mesh(&MeshData::plane(200.0), floor);
            ctx.world.spawn((Transform::default(), Model::new(plane)));
            let ball = ctx.gfx.create_mesh(&MeshData::sphere(1.0, 48), chrome);
            let rough_ball = ctx.gfx.create_mesh(&MeshData::sphere(1.0, 48), matte);
            for (x, mesh) in [(-2.5, ball), (0.0, rough_ball), (2.5, ball)] {
                ctx.world.spawn((Transform::at(vec3(x, 1.0, 0.0)), Model::new(mesh)));
            }
            // Pillars walking off into the fog.
            let pillar = ctx.gfx.create_mesh(&MeshData::cube(1.0), stone);
            for i in 0..12 {
                let z = -6.0 - i as f32 * 8.0;
                for x in [-7.0, 7.0] {
                    ctx.world.spawn((
                        Transform {
                            scale: vec3(1.2, 6.0, 1.2),
                            ..Transform::at(vec3(x, 3.0, z))
                        },
                        Model::new(pillar),
                    ));
                }
            }
        })
        .run();
}
