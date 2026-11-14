//! Offscreen transparency and particles: see-through batch instances
//! drawn far to near, alpha particles of several emitters sorted against
//! each other, soft particles fading where they meet the scene, and
//! particles in target cameras (planar reflections).
use super::render_test_support as support;
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::d3::{
    Camera as Camera3D, ParticleBlend as ParticleBlend3D, ParticleEmitter as ParticleEmitter3D,
    ParticleSettings as ParticleSettings3D, Transform as Transform3D,
};
use crate::particles::ParticleGradient;
use crate::prelude::*;
use crate::render::{Blend, InstanceBatchOptions, InstanceLevel, MaterialDesc, MeshData, Model, TargetCamera};

/// A see-through material showing each model's tint, unlit, at `alpha`.
fn tinted(rig: &mut BatchRig, alpha: f32) -> MaterialDesc {
    let source = format!(
        "fn surface(s: Surface) -> Surface {{ var o = s; o.unlit = true; o.albedo = s.color.rgb; \
         o.emissive = vec3<f32>(0.0); o.alpha = {alpha:.3}; return o; }}"
    );
    let name = format!("tinted {alpha}");
    let shader = rig.ctx.gfx.create_shader(&name, &source);
    rig.renderer.create_shader(&rig.gpu.device, &name, &source).unwrap();
    MaterialDesc {
        shader: Some(shader),
        blend: Blend::Transparent,
        ..Default::default()
    }
}

/// The image's center pixel.
fn center(image: &support::Image) -> [u8; 4] {
    image.pixel(image.width / 2, image.height / 2)
}

/// A rig with no sky, sun or effects: what is drawn is what was put there.
fn rig() -> Option<BatchRig> {
    let mut rig = BatchRig::new(RigSetup::default())?;
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.lighting.sun_intensity = 0.0;
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(Vec3::NEG_Z);
    Some(rig)
}

#[test]
fn see_through_batch_instances_blend_far_to_near_from_either_side() {
    let Some(mut rig) = rig() else { return };
    rig.keep_order = true;
    let material = tinted(&mut rig, 0.7);
    let mesh = rig.mesh(&MeshData::cube(1.0), material);
    let level = InstanceLevel {
        mesh,
        min_distance: 0.0,
        max_distance: f32::INFINITY,
        camera: true,
        shadows: false,
    };
    // In the batch's order the red one comes first, then the green one,
    // whichever side the camera is on.
    let transforms = [
        Mat4::from_translation(vec3(0.0, 0.0, -4.0)),
        Mat4::from_translation(vec3(0.0, 0.0, -8.0)),
    ];
    let options = InstanceBatchOptions {
        cast_shadows: false,
        ..Default::default()
    };
    rig.batch_levels(&[level], &transforms, &[Color::RED, Color::GREEN], options);
    let front = center(&rig.image());
    assert!(
        front[0] > front[1] + 60,
        "seen from the red side, red is nearest and must cover green: {front:?}"
    );
    rig.ctx.scene3d.camera.position = vec3(0.0, 0.0, -12.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let back = center(&rig.image());
    assert!(
        back[1] > back[0] + 60,
        "seen from the green side, green is nearest and must cover red: {back:?}"
    );
}

/// An emitter whose particles of `size` stand at `points` (world space),
/// tinted `color`.
fn emitter(points: &[Vec3], size: f32, color: Color, blend: ParticleBlend3D, softness: f32) -> ParticleEmitter3D {
    let settings = ParticleSettings3D {
        rate: 0.0,
        speed: (0.0, 0.0),
        gravity: Vec3::ZERO,
        size: (size, size),
        lifetime: (100.0, 100.0),
        blend,
        softness,
        color_over_life: ParticleGradient::new(vec![(0.0, color)]),
        ..Default::default()
    };
    ParticleEmitter3D::at_points_for_test(settings, points)
}

/// One particle of `size` at `at`.
fn puff(at: Vec3, size: f32, color: Color, blend: ParticleBlend3D, softness: f32) -> (Transform3D, ParticleEmitter3D) {
    (Transform3D::default(), emitter(&[at], size, color, blend, softness))
}

#[test]
fn alpha_particles_of_two_emitters_interleave_far_to_near() {
    let Some(mut rig) = rig() else { return };
    // Red has a near and a far particle, green one between them, its
    // centre nearer than red's: sorting whole emitters would draw green
    // over both. Particle by particle, the near red one covers green.
    let red = |points: &[Vec3]| {
        emitter(
            points,
            2.0,
            Color::rgba(1.0, 0.0, 0.0, 0.8),
            ParticleBlend3D::Alpha,
            0.0,
        )
    };
    let green = emitter(
        &[vec3(0.0, 0.0, -5.5)],
        2.0,
        Color::rgba(0.0, 1.0, 0.0, 0.8),
        ParticleBlend3D::Alpha,
        0.0,
    );
    let both = rig.ctx.world.spawn((
        Transform3D::default(),
        red(&[vec3(0.0, 0.0, -3.0), vec3(0.0, 0.0, -9.0)]),
    ));
    rig.ctx.world.spawn((Transform3D::default(), green));
    let c = center(&rig.image());
    assert!(
        c[0] > c[1] + 40,
        "the near red particle must cover the green one behind it: {c:?}"
    );
    // Negative control: without the near red one, green covers the far red one.
    rig.ctx.world.despawn(both);
    rig.ctx
        .world
        .spawn((Transform3D::default(), red(&[vec3(0.0, 0.0, -9.0)])));
    let c = center(&rig.image());
    assert!(c[1] > c[0] + 40, "green must cover the far red particle: {c:?}");
}

/// Removes every particle emitter.
fn clear_emitters(rig: &mut BatchRig) {
    let ids: Vec<Entity> = rig
        .ctx
        .world
        .query::<(Entity, &ParticleEmitter3D)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in ids {
        rig.ctx.world.despawn(e);
    }
}

#[test]
fn soft_particles_fade_where_they_meet_a_wall() {
    let Some(mut rig) = rig() else { return };
    let wall = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::BLACK));
    // A black wall whose front is at z = -5, facing the camera.
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 0.0, -5.5),
            scale: vec3(20.0, 20.0, 1.0),
            ..Default::default()
        },
        Model::new(wall),
    ));
    // A big white puff half a unit in front of the wall, seen face on.
    let color = Color::rgba(1.0, 1.0, 1.0, 1.0);
    let mut measure = |at: Vec3, size: f32, softness: f32| -> [u8; 4] {
        clear_emitters(&mut rig);
        rig.ctx
            .world
            .spawn(puff(at, size, color, ParticleBlend3D::Alpha, softness));
        center(&rig.image())
    };
    let hard = measure(vec3(0.0, 0.0, -4.5), 6.0, 0.0);
    let soft = measure(vec3(0.0, 0.0, -4.5), 6.0, 4.0);
    assert!(
        hard[0] > 200,
        "a hard particle in front of the wall is fully white: {hard:?}"
    );
    assert!(
        soft[0] + 60 < hard[0],
        "half a unit in front of the wall, a particle 4 units soft is faint: {soft:?} vs {hard:?}"
    );
    // Far from the wall, the soft one is as bright as the hard one.
    let far = measure(vec3(0.0, 0.0, -0.8), 0.6, 4.0);
    assert!(far[0] > 200, "a soft particle far from anything is whole: {far:?}");
}

/// The largest step in red between two neighbouring rows of column `x`,
/// from row `top` down.
fn sharpest_step(image: &support::Image, x: u32, top: u32) -> u8 {
    (top..image.height - 1)
        .map(|y| image.pixel(x, y)[0].abs_diff(image.pixel(x, y + 1)[0]))
        .max()
        .unwrap()
}

/// The capture for "smoke meets the scene without a hard edge": a puff of
/// smoke resting on dark ground before a dark wall, drawn hard and soft
/// (`PONKO_RENDER_TEST_IMAGES=1` keeps both images). Below the puff's
/// centre its only edge is where the ground cuts it.
#[test]
fn soft_smoke_meets_the_ground_without_a_hard_edge() {
    let Some(mut rig) = rig() else { return };
    let dark = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::BLACK));
    // Ground with its top at y = 0, and a wall behind.
    for (position, scale) in [
        (vec3(0.0, -0.5, -5.0), vec3(40.0, 1.0, 40.0)),
        (vec3(0.0, 5.0, -8.5), vec3(40.0, 10.0, 1.0)),
    ] {
        rig.ctx.world.spawn((
            Transform3D {
                position,
                scale,
                ..Default::default()
            },
            Model::new(dark),
        ));
    }
    rig.ctx.scene3d.camera.position = vec3(0.0, 2.0, 2.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.3, -4.0));
    let mut draw = |softness: f32| {
        clear_emitters(&mut rig);
        rig.ctx.world.spawn(puff(
            vec3(0.0, 0.3, -4.0),
            2.5,
            Color::rgba(0.9, 0.9, 0.9, 0.9),
            ParticleBlend3D::Alpha,
            softness,
        ));
        let image = rig.image();
        support::keep_image(&image);
        image
    };
    let hard = draw(0.0);
    let soft = draw(3.0);
    let (x, top) = (hard.width / 2, hard.height / 2);
    let (hard_step, soft_step) = (sharpest_step(&hard, x, top), sharpest_step(&soft, x, top));
    assert!(
        hard_step > 100,
        "the hard puff is cut by the ground: a step of {hard_step}"
    );
    assert!(
        soft_step < 40,
        "the soft puff fades into the ground: its sharpest step is {soft_step} (hard: {hard_step})"
    );
    // It still shows whole where the scene is far behind it (its top,
    // before the wall), and fades towards the ground.
    let brightest = |image: &support::Image| (0..image.height).map(|y| image.pixel(x, y)[0]).max().unwrap();
    let (h, s) = (brightest(&hard), brightest(&soft));
    assert!(s + 30 > h, "the soft puff's top shows whole: {s} (hard: {h})");
}

#[test]
fn particles_show_in_a_target_camera_unless_turned_off() {
    let Some(mut rig) = rig() else { return };
    rig.ctx.world.spawn(puff(
        vec3(0.0, 0.0, -5.0),
        3.0,
        Color::RED,
        ParticleBlend3D::Additive,
        0.0,
    ));
    let mut camera: Camera3D = rig.ctx.scene3d.camera;
    camera.position = vec3(0.0, 0.0, 0.0);
    camera.look_at(Vec3::NEG_Z);
    let texture = rig.target_camera(camera);
    rig.frame();
    let red = target_center(&rig, &texture);
    assert!(
        red[0] > 0.5 && red[1] < 0.1,
        "the target camera must see the particle: {red:?}"
    );
    rig.ctx.scene3d.passes.target_cameras[0].camera.particles = false;
    rig.frame();
    let off = target_center(&rig, &texture);
    assert!(off[0] < 0.05, "with particles off, the target shows none: {off:?}");
    // The main camera has the same switch.
    let main = center(&rig.image());
    assert!(main[0] > 100, "the main view still shows the particle: {main:?}");
    rig.ctx.scene3d.camera.particles = false;
    let hidden = center(&rig.image());
    assert!(
        hidden[0] < 10,
        "with the main camera's particles off, it shows none: {hidden:?}"
    );
}

#[test]
fn particles_show_in_a_planar_reflection() {
    let Some(mut rig) = rig() else { return };
    // A red puff above a mirror at height 0, the camera above looking
    // down past it at the mirror.
    rig.ctx.world.spawn(puff(
        vec3(0.0, 1.0, -6.0),
        1.5,
        Color::RED,
        ParticleBlend3D::Additive,
        0.0,
    ));
    let glow = MaterialDesc {
        color: Color::BLACK,
        emissive: Color::rgb(0.1, 0.2, 1.0),
        ..Default::default()
    };
    let block = rig.mesh(&MeshData::cube(1.0), glow);
    rig.ctx
        .world
        .spawn((Transform3D::at(vec3(-2.0, 0.5, -6.0)), Model::new(block)));
    rig.ctx.scene3d.camera.position = vec3(0.0, 3.0, 0.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, -1.0, -6.0));
    let main = rig.ctx.scene3d.camera;
    let texture = rig.target_camera(main);
    let reflection = TargetCamera::reflection(main, rig.ctx.scene3d.passes.target_cameras[0].target, 0.0);
    rig.ctx.scene3d.passes.target_cameras[0] = reflection;
    rig.frame();
    let image = read_hdr(&rig, &texture);
    support::keep_image(&support::Image {
        width: texture.width(),
        height: texture.height(),
        rgba: image
            .iter()
            .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8))
            .collect(),
    });
    // The reflection holds the puff, mirrored below the mirror plane, and
    // the blue block beside it.
    let lit = image.iter().filter(|p| p[0] > 0.3 && p[1] < 0.1 && p[2] < 0.1).count();
    assert!(lit > 20, "the reflection must show the red particle ({lit} red pixels)");
    let block = image.iter().filter(|p| p[2] > 0.3 && p[0] < 0.1).count();
    assert!(block > 20, "the reflection shows the scene too ({block} blue pixels)");
}

/// A render target's pixels as linear RGBA.
fn read_hdr(rig: &BatchRig, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
    let bytes = support::read_texture(&rig.gpu.device, &rig.gpu.queue, texture);
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|p| std::array::from_fn(|i| half::f16::from_le_bytes([p[2 * i], p[2 * i + 1]]).to_f32()))
        .collect()
}

fn target_center(rig: &BatchRig, texture: &wgpu::Texture) -> [f32; 4] {
    let pixels = read_hdr(rig, texture);
    let (w, h) = (texture.width(), texture.height());
    pixels[(h / 2 * w + w / 2) as usize]
}
