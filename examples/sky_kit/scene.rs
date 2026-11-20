//! The `sky_kit` example's scene and controls, apart from its window so
//! `tests/sky_kit_example.rs` can run them headless: a field of blocks
//! under the engine's sky kit, a day on fast forward, and keys for the
//! clouds.

#[allow(unused_imports)]
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{CloudQuality, DayNight, MaterialDesc, MeshData, SkyKit};

/// The cloud covers C steps through, clear to overcast.
pub const COVERS: [f32; 4] = [0.2, 0.45, 0.7, 1.0];

/// The example's state: the clock, and whether it is held.
pub struct SkyDemo {
    pub day: DayNight,
    pub paused: bool,
}

/// Builds the field and turns the sky kit on, at `hour` o'clock.
pub fn setup(ctx: &mut Ctx, hour: f32) -> SkyDemo {
    let grass = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::hex(0x5A7A3A)).with_roughness(0.95));
    let stone = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::hex(0xC8C0B0)).with_roughness(0.8));
    let brick = ctx
        .gfx
        .create_material(MaterialDesc::color(Color::hex(0xA0402C)).with_roughness(0.7));
    let field = ctx.gfx.create_mesh(&MeshData::plane(20000.0), grass);
    ctx.world.spawn((Transform::default(), Model::new(field)));
    // Blocks in rings around the camera: something casts a shadow whichever
    // way the sun stands, and the far ones show the cloud shadows passing.
    let (stone, brick) = (
        ctx.gfx.create_mesh(&MeshData::cube(1.0), stone),
        ctx.gfx.create_mesh(&MeshData::cube(1.0), brick),
    );
    for i in 0..40 {
        let ring = (i % 4) as f32;
        let a = i as f32 * 2.399; // the golden angle: evenly spread
        let r = 12.0 + ring * 30.0 + (i as f32 * 7.0) % 9.0;
        let size = vec3(2.0 + ring, 3.0 + (i % 5) as f32 * 2.0 + ring * 3.0, 2.0 + ring);
        let mesh = if i % 3 == 0 { brick } else { stone };
        ctx.world.spawn((
            Transform {
                scale: size,
                ..Transform::at(vec3(a.cos() * r, size.y * 0.5, a.sin() * r))
            },
            Model::new(mesh),
        ));
    }
    let mut kit = SkyKit::default();
    kit.clouds.coverage = COVERS[1];
    kit.clouds.wind = vec2(15.0, 4.0); // a brisk wind, so the clouds visibly move
    ctx.scene3d.sky.kit = Some(kit);
    ctx.scene3d.sky.fog_density = 0.002; // haze in the distance, in the clouds' colors
    ctx.scene3d.sky.fog_sun_glow = 0.1;
    ctx.scene3d.lighting.shadows = true;
    ctx.scene3d.lighting.shadow_sun_step = 0.01; // shadows hold still as the sun moves
    ctx.scene3d.camera.position = vec3(0.0, 3.0, 0.0);
    ctx.scene3d.camera.look_at(vec3(20.0, 6.0, -10.0));
    let mut day = DayNight::new(hour);
    day.hours_per_second = 0.5; // a day in 48 seconds
    day.days_per_moon = 8.0; // the moon's phases show within a few nights
    day.apply(&mut ctx.scene3d);
    SkyDemo { day, paused: false }
}

/// The next cloud cover in [`COVERS`] after the current one.
pub fn next_cover(ctx: &mut Ctx) {
    if let Some(kit) = &mut ctx.scene3d.sky.kit {
        let at = COVERS
            .iter()
            .position(|&c| c >= kit.clouds.coverage - 1e-3)
            .unwrap_or(0);
        kit.clouds.coverage = COVERS[(at + 1) % COVERS.len()];
    }
}

/// The next [`CloudQuality`], cheapest after the finest.
pub fn next_quality(ctx: &mut Ctx) {
    if let Some(kit) = &mut ctx.scene3d.sky.kit {
        let all = CloudQuality::ALL;
        let at = all.iter().position(|&q| q == kit.quality).unwrap_or(0);
        kit.quality = all[(at + 1) % all.len()];
    }
}

/// One frame: the keys, then the clock (space holds it; C and Q step the
/// cloud cover and the quality).
pub fn update(ctx: &mut Ctx, demo: &mut SkyDemo) {
    if ctx.input.key_pressed(Key::Space) {
        demo.paused = !demo.paused;
    }
    if ctx.input.key_pressed(Key::C) {
        next_cover(ctx);
    }
    if ctx.input.key_pressed(Key::Q) {
        next_quality(ctx);
    }
    if demo.paused {
        demo.day.apply(&mut ctx.scene3d);
    } else {
        demo.day.update(ctx);
    }
}
