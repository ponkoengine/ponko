//! The water example's scene and controls, as a plugin: a lake on a
//! plateau drains by a river past a beach into the sea, with crates
//! floating on all three. `main.rs` opens it in a window; `tests/water.rs`
//! runs it without one.

use ponko::d3::*;
use ponko::prelude::*;
use ponko::render::{MaterialDesc, MeshData};
use ponko::water::{Buoyancy, FlowMap, Water, WaterDesc, WaterPlugin, WaterQuality, WaterReflection, despawn_water};

/// The example's state: its water bodies and crates, and what the keys set.
pub struct Demo {
    pub lake: Entity,
    pub river: Entity,
    pub sea: Entity,
    pub crates: Vec<Entity>,
    pub quality: WaterQuality,
    pub reflection: WaterReflection,
    crate_model: Model,
    yaw: f32,
    pitch: f32,
}

/// The water example: the scene, the water, flying around and the keys.
pub struct WaterDemo;

impl Plugin for WaterDemo {
    fn build(&self, game: Game) -> Game {
        game.plugin(WaterPlugin).system(controls)
    }

    fn setup(&self, ctx: &mut Ctx) {
        build_scene(ctx);
    }
}

// ANCHOR: bodies
/// The lake: on the plateau, 30 m across, its surface 2.5 m up.
pub fn lake_desc(quality: WaterQuality, reflection: WaterReflection) -> WaterDesc {
    WaterDesc::lake(vec2(-45.0, 0.0), vec2(30.0, 30.0), 2.5)
        .with_quality(quality)
        .with_reflection(reflection)
}

/// The river: from the lake to the coast, fastest mid-stream.
pub fn river_desc(quality: WaterQuality) -> WaterDesc {
    let flow = FlowMap::from_fn(8, 4, 2.5, |uv| {
        let across = uv.y * 2.0 - 1.0;
        vec2(2.5 * (1.0 - across * across).max(0.3), 0.0)
    });
    WaterDesc::river(vec2(-25.0, 0.0), vec2(10.0, 6.0), 2.4, Vec2::ZERO)
        .with_flow_map(flow)
        .with_quality(quality)
}

/// The sea: everywhere around, following the camera.
pub fn sea_desc(quality: WaterQuality) -> WaterDesc {
    WaterDesc::sea(0.0, vec2(-0.8, 0.5), 1.2).with_quality(quality)
}
// ANCHOR_END: bodies

/// A fixed box from `lo` to `hi` (drawn and solid), turned about z.
fn block(ctx: &mut Ctx, model: Model, lo: Vec3, hi: Vec3, tilt: f32) {
    let size = hi - lo;
    ctx.world.spawn((
        Transform {
            position: (lo + hi) * 0.5,
            scale: size,
            rotation: Quat::from_rotation_z(tilt),
        },
        model,
        RigidBody::fixed(),
        Collider::cuboid(size),
    ));
}

fn build_scene(ctx: &mut Ctx) {
    let cube = |ctx: &mut Ctx, color: u32| {
        let material = ctx
            .gfx
            .create_material(MaterialDesc::color(Color::hex(color)).with_roughness(0.9));
        Model::new(ctx.gfx.create_mesh(&MeshData::cube(1.0), material))
    };
    let grass = cube(ctx, 0x5A8A3A);
    let sand = cube(ctx, 0xC9B07A);
    // The plateau (top 3 m) around the lake's pit and the river's channel.
    for (lo, hi) in [
        (vec3(-100.0, -6.0, -60.0), vec3(-60.0, 3.0, 60.0)),
        (vec3(-60.0, -6.0, 15.0), vec3(-20.0, 3.0, 60.0)),
        (vec3(-60.0, -6.0, -60.0), vec3(-20.0, 3.0, -15.0)),
        (vec3(-30.0, -6.0, 3.0), vec3(-20.0, 3.0, 15.0)),
        (vec3(-30.0, -6.0, -15.0), vec3(-20.0, 3.0, -3.0)),
    ] {
        block(ctx, grass, lo, hi, 0.0);
    }
    block(ctx, sand, vec3(-60.0, -6.0, -15.0), vec3(-30.0, -0.5, 15.0), 0.0);
    block(ctx, sand, vec3(-30.0, -6.0, -3.0), vec3(-20.0, 0.5, 3.0), 0.0);
    // The beach slopes from the plateau's foot into the sea; the sea bed.
    block(ctx, sand, vec3(-21.0, -3.0, -60.0), vec3(31.0, -1.0, 60.0), -0.17);
    block(ctx, sand, vec3(-20.0, -12.0, -300.0), vec3(300.0, -8.0, 300.0), 0.0);

    let quality = WaterQuality::Medium;
    let reflection = WaterReflection::Planar;
    let lake = Water::spawn(ctx, lake_desc(quality, reflection));
    let river = Water::spawn(ctx, river_desc(quality));
    let sea = Water::spawn(ctx, sea_desc(quality));
    let crate_model = cube(ctx, 0xA0703C);
    let mut demo = Demo {
        lake,
        river,
        sea,
        crates: Vec::new(),
        quality,
        reflection,
        crate_model,
        yaw: -1.2,
        pitch: -0.25,
    };
    for at in [
        vec3(-50.0, 4.0, -4.0),
        vec3(-42.0, 4.5, 5.0),
        vec3(-27.0, 3.5, 0.0),
        vec3(12.0, 2.0, -6.0),
        vec3(20.0, 2.0, 4.0),
        vec3(28.0, 2.5, -1.0),
    ] {
        drop_crate(ctx, &mut demo, at);
    }
    ctx.scene3d.camera.position = vec3(-8.0, 9.0, 22.0);
    ctx.world.insert_resource(demo);
}

// ANCHOR: crate
/// A crate 1 m across, half as dense as water, dropped at `at`.
pub fn drop_crate(ctx: &mut Ctx, demo: &mut Demo, at: Vec3) -> Entity {
    let size = Vec3::ONE;
    let e = ctx.world.spawn((
        Transform::at(at),
        demo.crate_model,
        RigidBody::dynamic(),
        Collider::cuboid(size).density(500.0),
        Buoyancy::cuboid(size),
    ));
    demo.crates.push(e);
    e
}
// ANCHOR_END: crate

/// Makes the three water bodies again at `quality`, the lake reflecting
/// with `reflection`.
pub fn set_quality(ctx: &mut Ctx, quality: WaterQuality, reflection: WaterReflection) {
    let mut demo = ctx.world.remove_resource::<Demo>().expect("the water example's state");
    for e in [demo.lake, demo.river, demo.sea] {
        despawn_water(ctx, e);
    }
    demo.lake = Water::spawn(ctx, lake_desc(quality, reflection));
    demo.river = Water::spawn(ctx, river_desc(quality));
    demo.sea = Water::spawn(ctx, sea_desc(quality));
    demo.quality = quality;
    demo.reflection = reflection;
    ctx.world.insert_resource(demo);
}

/// WASD flies, Q and E go down and up, the arrows look around; 1, 2 and 3
/// pick the quality, R the lake's reflection, Space drops a crate ahead.
fn controls(ctx: &mut Ctx) {
    let dt = ctx.time.dt();
    let key = |k| ctx.input.key_down(k);
    let turn = (f32::from(u8::from(key(Key::Left))) - f32::from(u8::from(key(Key::Right)))) * 1.5 * dt;
    let tilt = (f32::from(u8::from(key(Key::Up))) - f32::from(u8::from(key(Key::Down)))) * 1.0 * dt;
    let axis = |plus, minus| f32::from(u8::from(key(plus))) - f32::from(u8::from(key(minus)));
    let (ahead, side, up) = (axis(Key::W, Key::S), axis(Key::D, Key::A), axis(Key::E, Key::Q));
    let (rotation, forward) = {
        let demo = ctx.world.resource_mut::<Demo>();
        demo.yaw += turn;
        demo.pitch = (demo.pitch + tilt).clamp(-1.4, 1.4);
        let rotation = Quat::from_rotation_y(demo.yaw) * Quat::from_rotation_x(demo.pitch);
        (rotation, rotation * Vec3::NEG_Z)
    };
    let speed = if ctx.input.key_down(Key::LeftShift) { 30.0 } else { 10.0 };
    let camera = &mut ctx.scene3d.camera;
    camera.rotation = rotation;
    camera.position += (forward * ahead + rotation * Vec3::X * side + Vec3::Y * up) * speed * dt;

    let (quality, reflection) = {
        let demo = ctx.world.resource::<Demo>();
        (demo.quality, demo.reflection)
    };
    let picked = [
        (Key::Num1, WaterQuality::Low),
        (Key::Num2, WaterQuality::Medium),
        (Key::Num3, WaterQuality::High),
    ]
    .into_iter()
    .find(|&(k, _)| ctx.input.key_pressed(k))
    .map(|(_, q)| q);
    let reflection_next = ctx.input.key_pressed(Key::R).then_some(match reflection {
        WaterReflection::Sky => WaterReflection::ScreenSpace,
        WaterReflection::ScreenSpace => WaterReflection::Planar,
        WaterReflection::Planar => WaterReflection::Sky,
    });
    if picked.is_some() || reflection_next.is_some() {
        set_quality(ctx, picked.unwrap_or(quality), reflection_next.unwrap_or(reflection));
    }
    if ctx.input.key_pressed(Key::Space) {
        let at = ctx.scene3d.camera.position + forward * 4.0;
        let mut demo = ctx.world.remove_resource::<Demo>().expect("the water example's state");
        drop_crate(ctx, &mut demo, at);
        ctx.world.insert_resource(demo);
    }
}
