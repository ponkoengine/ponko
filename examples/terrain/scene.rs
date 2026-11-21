//! The terrain example's scene and controls (`main.rs` opens it in a
//! window; the engine's offscreen GPU tests draw it without one): 5 x 5
//! tiles of 256 m (129 x 129 heights, 2 m apart), drawn coarser with
//! distance, with grass, dirt and rock blended by a weight map, and a lake
//! from the water kit filling the valleys.

#[cfg(feature = "3d")]
use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{Material, Sky};
use ponko::terrain::{TerrainMaterialDesc, TextureKind};
use ponko::water::{Water, WaterDesc, WaterReflection, despawn_water};

pub const TILES: i32 = 5;
pub const N: usize = 129;
pub const TILE: f32 = 256.0;
pub const WATER: f32 = 2.0;

/// Smooth value noise, 0 to 1.
fn noise(x: f32, z: f32) -> f32 {
    tiling_noise(x, z, i32::MAX)
}

/// Value noise that repeats every `period` cells, for textures that tile.
fn tiling_noise(x: f32, z: f32, period: i32) -> f32 {
    let hash = |i: i32, j: i32| {
        let (i, j) = (i.rem_euclid(period), j.rem_euclid(period));
        let h = (i.wrapping_mul(374_761_393) ^ j.wrapping_mul(668_265_263)) as u32;
        let h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        (h >> 8) as f32 / (1u32 << 24) as f32
    };
    let (i, j) = (x.floor() as i32, z.floor() as i32);
    let (fx, fz) = (x - x.floor(), z - z.floor());
    let (sx, sz) = (fx * fx * (3.0 - 2.0 * fx), fz * fz * (3.0 - 2.0 * fz));
    let top = hash(i, j) + (hash(i + 1, j) - hash(i, j)) * sx;
    let bottom = hash(i, j + 1) + (hash(i + 1, j + 1) - hash(i, j + 1)) * sx;
    top + (bottom - top) * sz
}

/// Hills: a few octaves of noise, flattened in the valleys.
pub fn height(x: f32, z: f32) -> f32 {
    let mut h = 0.0;
    let (mut scale, mut amp) = (1.0 / 300.0, 40.0);
    for _ in 0..5 {
        h += (noise(x * scale, z * scale) - 0.5) * amp;
        scale *= 2.1;
        amp *= 0.45;
    }
    h + 6.0
}

/// A ground layer: `base` color with two scales of noise in it.
fn layer(base: [f32; 3], grain: f32) -> Vec<u8> {
    let size = 256;
    (0..size * size)
        .flat_map(|i| {
            let (x, y) = ((i % size) as f32, (i / size) as f32);
            // Periods that divide the 256 pixels, so the layer tiles.
            let n = tiling_noise(x / 16.0, y / 16.0, 16) * 0.6 + tiling_noise(x / 4.0, y / 4.0, 64) * 0.4;
            let shade = 1.0 + (n - 0.5) * grain;
            let c = base.map(|v| ((v * shade).clamp(0.0, 1.0) * 255.0) as u8);
            [c[0], c[1], c[2], 255]
        })
        .collect()
}

fn ground(ctx: &mut Ctx) -> Material {
    let layers = [
        layer([0.24, 0.42, 0.16], 0.6),
        layer([0.40, 0.31, 0.22], 0.7),
        layer([0.46, 0.45, 0.42], 0.9),
        layer([0.70, 0.62, 0.45], 0.4),
    ];
    let refs: Vec<&[u8]> = layers.iter().map(Vec::as_slice).collect();
    let layers = ctx.gfx.create_texture_array(256, 256, &refs, TextureKind::Color);
    // One weight map over the whole world, 2 m a pixel.
    let cells = (TILES as usize * (N - 1)) / 2;
    let area = TILES as f32 * TILE;
    let origin = -area / 2.0;
    let weights: Vec<u8> = (0..cells * cells)
        .flat_map(|i| {
            let x = origin + (i % cells) as f32 * area / cells as f32;
            let z = origin + (i / cells) as f32 * area / cells as f32;
            let h = height(x, z);
            let slope = (height(x + 2.0, z) - height(x - 2.0, z)).hypot(height(x, z + 2.0) - height(x, z - 2.0)) / 4.0;
            let sand = (1.0 - (h - WATER).abs() / 1.5).clamp(0.0, 1.0);
            let rock = ((slope - 0.45) * 4.0).clamp(0.0, 1.0);
            let dirt = ((h - 18.0) / 8.0).clamp(0.0, 1.0) * (1.0 - rock);
            let grass = (1.0 - sand - rock - dirt).max(0.0);
            [grass, dirt, rock, sand].map(|w| (w * 255.0) as u8)
        })
        .collect();
    let weights = ctx
        .gfx
        .create_texture_array(cells as u32, cells as u32, &[&weights], TextureKind::Data);
    let desc = TerrainMaterialDesc::new(layers, weights, vec2(origin, origin), vec2(area, area))
        .with_layer_size([5.0, 4.0, 7.0, 3.0, 4.0, 4.0, 4.0, 4.0]);
    ctx.gfx.create_terrain_material(&desc)
}

/// The lake: one level of water over the whole map, its edges hidden by
/// the hills. It mirrors them (a planar reflection), foams where it is
/// shallow and tints the view when the camera dives.
pub fn lake_desc(underwater: bool) -> WaterDesc {
    let area = TILES as f32 * TILE;
    WaterDesc {
        underwater,
        ..WaterDesc::lake(Vec2::ZERO, vec2(area, area), WATER).with_reflection(WaterReflection::Planar)
    }
}

/// Where the camera starts for `view` (`high`, `low`, `under`, `cliff`),
/// and its yaw and pitch.
pub fn start(view: &str) -> (Vec3, f32, f32) {
    // The first spot nearest the middle where the ground fits.
    let spot = |fits: &dyn Fn(f32) -> bool| {
        (0..200 * 200)
            .map(|i| vec2((i % 200) as f32 * 4.0 - 400.0, (i / 200) as f32 * 4.0 - 400.0))
            .filter(|p| fits(height(p.x, p.y)))
            .min_by(|a, b| a.length().total_cmp(&b.length()))
            .unwrap_or(Vec2::ZERO)
    };
    match view {
        "low" => {
            let p = spot(&|h| h > WATER + 4.0);
            (vec3(p.x, height(p.x, p.y) + 1.8, p.y), 0.6, -0.08)
        }
        "under" => {
            let p = spot(&|h| h < WATER - 2.0 && h > WATER - 5.0);
            (vec3(p.x, WATER - 1.0, p.y), 0.6, -0.6)
        }
        "cliff" => {
            // The steepest ground near the middle, seen from 30 m off its foot.
            let slope = |p: Vec2| {
                (height(p.x + 2.0, p.y) - height(p.x - 2.0, p.y)).hypot(height(p.x, p.y + 2.0) - height(p.x, p.y - 2.0))
            };
            let steep = (0..200 * 200)
                .map(|i| vec2((i % 200) as f32 * 4.0 - 400.0, (i / 200) as f32 * 4.0 - 400.0))
                .filter(|p| height(p.x, p.y) > WATER + 2.0)
                .max_by(|a, b| slope(*a).total_cmp(&slope(*b)))
                .unwrap_or(Vec2::ZERO);
            // Downhill, away from the face.
            let down = -vec2(
                height(steep.x + 2.0, steep.y) - height(steep.x - 2.0, steep.y),
                height(steep.x, steep.y + 2.0) - height(steep.x, steep.y - 2.0),
            )
            .normalize_or_zero();
            let eye = steep + down * 30.0;
            let at = vec3(eye.x, height(eye.x, eye.y).max(WATER) + 6.0, eye.y);
            let to = vec3(steep.x, height(steep.x, steep.y), steep.y) - at;
            (at, f32::atan2(-to.x, -to.z), (to.y / to.length()).asin())
        }
        _ => (vec3(-300.0, 90.0, 300.0), -0.8, -0.25),
    }
}

/// The example's state: the camera's turn and the lake.
#[derive(Debug, Clone, Copy)]
pub struct Terrain {
    pub yaw: f32,
    pub pitch: f32,
    pub lake: Entity,
    /// Whether the lake tints the view under water (U switches it).
    pub underwater: bool,
}

/// Builds the scene: the sun at `hour`, the tiles, the lake, the camera
/// where `view` says.
pub fn setup(ctx: &mut Ctx, view: &str, hour: f32) -> Terrain {
    ctx.scene3d.post.bloom = 0.1;
    ctx.scene3d.lighting.sun_direction = Sky::sun_direction_at(hour);
    ctx.scene3d.lighting.shadow_distance = 400.0;
    ctx.scene3d.camera.far = 3000.0;
    ctx.scene3d.sky.fog_density = 0.0015;
    ctx.scene3d.sky.fog_height = 0.0;
    ctx.scene3d.sky.fog_falloff = 60.0;

    let ground = ground(ctx);
    let half = TILES / 2;
    for tz in -half..=half {
        for tx in -half..=half {
            let corner = vec3(tx as f32 * TILE - TILE / 2.0, 0.0, tz as f32 * TILE - TILE / 2.0);
            let heights: Vec<f32> = (0..N * N)
                .map(|i| height(corner.x + (i % N) as f32 * 2.0, corner.z + (i / N) as f32 * 2.0))
                .collect();
            let tile = ctx.gfx.create_terrain_tile(&heights, TILE, ground);
            ctx.world.spawn((Transform::at(corner), tile));
        }
    }

    let (start, yaw, pitch) = start(view);
    ctx.scene3d.camera.position = start;
    ctx.scene3d.camera.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
    let lake = Water::spawn(ctx, lake_desc(true));
    Terrain {
        yaw,
        pitch,
        lake,
        underwater: true,
    }
}

/// One frame: the arrows turn, WASD, Q and E fly, U switches the
/// underwater view. (`WaterPlugin` keeps the lake's mirror and underwater
/// view up to date.)
pub fn update(ctx: &mut Ctx, t: &mut Terrain) {
    let dt = ctx.time.dt();
    if ctx.input.key_down(Key::Left) {
        t.yaw += dt;
    }
    if ctx.input.key_down(Key::Right) {
        t.yaw -= dt;
    }
    if ctx.input.key_down(Key::Up) {
        t.pitch = (t.pitch + dt).min(1.4);
    }
    if ctx.input.key_down(Key::Down) {
        t.pitch = (t.pitch - dt).max(-1.4);
    }
    let rotation = Quat::from_rotation_y(t.yaw) * Quat::from_rotation_x(t.pitch);
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;
    let speed = 60.0 * dt;
    let mut go = Vec3::ZERO;
    for (key, dir) in [
        (Key::W, forward),
        (Key::S, -forward),
        (Key::D, right),
        (Key::A, -right),
        (Key::E, Vec3::Y),
        (Key::Q, Vec3::NEG_Y),
    ] {
        if ctx.input.key_down(key) {
            go += dir;
        }
    }
    ctx.scene3d.camera.position += go * speed;
    ctx.scene3d.camera.rotation = rotation;
    if ctx.input.key_pressed(Key::U) {
        switch_underwater(ctx, t);
    }
}

/// Makes the lake again with the underwater view switched.
pub fn switch_underwater(ctx: &mut Ctx, t: &mut Terrain) {
    despawn_water(ctx, t.lake);
    t.underwater = !t.underwater;
    t.lake = Water::spawn(ctx, lake_desc(t.underwater));
}
