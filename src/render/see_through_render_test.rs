//! Offscreen tests of the main view's see-through list (see
//! `gpu3d/draw_order.rs`): ribbons, lines and particles drawn far to near
//! among see-through models and instance batches, and see-through models
//! of one mesh drawn together where that gives the same picture.
use super::gpu3d::SeeThrough;
use super::regression_stage::REFERENCE;
use super::render_test_support as support;
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::d3::{
    Line as Line3D, ParticleBlend as ParticleBlend3D, ParticleEmitter as ParticleEmitter3D,
    ParticleSettings as ParticleSettings3D, Transform as Transform3D,
};
use crate::particles::ParticleGradient;
use crate::prelude::*;
use crate::render::{Blend, InstanceBatchOptions, InstanceLevel, MaterialDesc, Mesh, MeshData, Model};

/// A see-through material showing each model's tint, unlit, at `alpha`.
fn tinted(rig: &mut BatchRig, alpha: f32) -> MaterialDesc {
    let source = format!(
        "fn surface(s: Surface) -> Surface {{ var o = s; o.unlit = true; o.albedo = s.color.rgb; \
         o.emissive = vec3<f32>(0.0); o.alpha = {alpha:.3}; return o; }}"
    );
    let name = format!("see-through tinted {alpha}");
    let shader = rig.ctx.gfx.create_shader(&name, &source);
    rig.renderer.create_shader(&rig.gpu.device, &name, &source).unwrap();
    MaterialDesc {
        shader: Some(shader),
        blend: Blend::Transparent,
        ..Default::default()
    }
}

/// A rig with no sky, sun or effects, looking down -z from the origin.
fn rig() -> Option<BatchRig> {
    let mut rig = BatchRig::new(RigSetup::default())?;
    rig.ctx.scene3d.post.bloom = 0.0;
    rig.ctx.scene3d.lighting.sun_intensity = 0.0;
    rig.ctx.scene3d.camera.position = Vec3::ZERO;
    rig.ctx.scene3d.camera.look_at(Vec3::NEG_Z);
    Some(rig)
}

/// The mean colour of the 8 x 8 pixels around (`x`, `y`).
fn around(image: &support::Image, x: u32, y: u32) -> [f32; 3] {
    let mut sum = [0.0; 3];
    for dy in 0..8 {
        for dx in 0..8 {
            let p = image.pixel(x + dx - 4, y + dy - 4);
            for c in 0..3 {
                sum[c] += f32::from(p[c]) / 64.0;
            }
        }
    }
    sum
}

/// A pane across the whole view at depth `z`, as a model.
fn pane(rig: &mut BatchRig, mesh: Mesh, z: f32, color: Color) {
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(0.0, 0.0, -z),
            scale: vec3(30.0, 30.0, 0.05),
            ..Default::default()
        },
        Model {
            color,
            ..Model::new(mesh)
        },
    ));
}

/// A red line across the view at depth 7, half a metre wide, between a
/// blue pane at depth 10 and a green one at depth 4 (both 0.6 opaque):
/// joined to the far-to-near order, the line covers the blue pane and the
/// green one covers it; drawn after both (the order before), it covers
/// the green pane too.
#[test]
fn a_line_between_two_panes_is_seen_through_the_near_one_and_covers_the_far_one() {
    let Some(mut rig) = rig() else { return };
    let glass = tinted(&mut rig, 0.6);
    let mesh = rig.mesh(&MeshData::cube(1.0), glass);
    pane(&mut rig, mesh, 10.0, Color::rgb(0.0, 0.0, 1.0));
    pane(&mut rig, mesh, 4.0, Color::rgb(0.0, 1.0, 0.0));
    let line = Line3D::new(vec![vec3(-20.0, 0.0, -7.0), vec3(20.0, 0.0, -7.0)], 0.5, Color::RED);
    rig.ctx.world.spawn((Transform3D::default(), line));
    let image = rig.image();
    let (on, off) = (around(&image, 64, 64), around(&image, 64, 20));
    eprintln!("on the line {on:?}, beside it {off:?}");
    // Through the green glass: 0.4 of the red, under 0.6 of green.
    assert!(
        on[0] > 70.0 && on[0] < 200.0 && on[1] > 150.0 && on[2] < 20.0,
        "the line shows through the green pane and hides the blue one: {on:?}"
    );
    assert!(
        off[2] > 70.0 && off[1] > 150.0 && off[0] < 20.0,
        "beside it, the blue pane shows through the green one: {off:?}"
    );
    check_reference(&rig, "see_through_line_between_panes", &image, "line");
    // Negative control: the order before draws the line over both.
    rig.renderer.sort_strips_and_batches = false;
    let before = around(&rig.image(), 64, 64);
    rig.renderer.sort_strips_and_batches = true;
    assert!(
        before[0] > 200.0 && before[1] < 20.0,
        "drawn after the panes, the line covers the green one: {before:?}"
    );
}

/// The same for a ribbon left by a trail.
#[test]
fn a_ribbon_between_two_panes_is_drawn_between_them() {
    let Some(mut rig) = rig() else { return };
    let glass = tinted(&mut rig, 0.6);
    let mesh = rig.mesh(&MeshData::cube(1.0), glass);
    pane(&mut rig, mesh, 10.0, Color::rgb(0.0, 0.0, 1.0));
    pane(&mut rig, mesh, 4.0, Color::rgb(0.0, 1.0, 0.0));
    let mut trail = crate::d3::Trail::new(10.0, 0.5);
    trail.color = ParticleGradient::new(vec![(0.0, Color::RED), (1.0, Color::RED)]);
    trail.width = crate::particles::ParticleCurve::new(vec![(0.0, 0.5), (1.0, 0.5)]);
    for x in [-20.0, 0.0, 20.0] {
        trail.update(0.1, vec3(x, 0.0, -7.0));
    }
    rig.ctx.world.spawn((
        Transform3D {
            position: vec3(20.0, 0.0, -7.0),
            ..Default::default()
        },
        trail,
    ));
    let on = around(&rig.image(), 64, 64);
    assert!(
        on[0] > 70.0 && on[0] < 200.0 && on[1] > 150.0 && on[2] < 20.0,
        "the ribbon shows through the green pane and hides the blue one: {on:?}"
    );
    rig.renderer.sort_strips_and_batches = false;
    let before = around(&rig.image(), 64, 64);
    assert!(
        before[0] > 200.0 && before[1] < 20.0,
        "the order before drew it over both: {before:?}"
    );
}

/// One particle of `size` at `at`, tinted `color`.
fn puff(at: Vec3, size: f32, color: Color) -> (Transform3D, ParticleEmitter3D) {
    let settings = ParticleSettings3D {
        rate: 0.0,
        speed: (0.0, 0.0),
        gravity: Vec3::ZERO,
        size: (size, size),
        lifetime: (100.0, 100.0),
        blend: ParticleBlend3D::Alpha,
        softness: 0.0,
        color_over_life: ParticleGradient::new(vec![(0.0, color)]),
        ..Default::default()
    };
    (
        Transform3D::default(),
        ParticleEmitter3D::at_points_for_test(settings, &[at]),
    )
}

/// A see-through instance batch of two panes, blue at depth 10 and green
/// at depth 4 (0.6 opaque), with a red particle at depth 7 between them
/// and a smaller one at depth 2 in front of both: the far one is drawn
/// between the batch's instances (the near pane tints it, it covers the
/// far one), the near one over everything. Before, particles were drawn
/// after every batch.
#[test]
fn particles_are_drawn_between_the_instances_of_a_see_through_batch() {
    let Some(mut rig) = rig() else { return };
    rig.keep_order = true;
    let glass = tinted(&mut rig, 0.6);
    let mesh = rig.mesh(&MeshData::cube(1.0), glass);
    let level = InstanceLevel {
        mesh,
        min_distance: 0.0,
        max_distance: f32::INFINITY,
        camera: true,
        shadows: false,
    };
    let at = |z: f32| Mat4::from_scale_rotation_translation(vec3(30.0, 30.0, 0.05), Quat::IDENTITY, vec3(0.0, 0.0, -z));
    let options = InstanceBatchOptions {
        cast_shadows: false,
        ..Default::default()
    };
    // Near first: the batch's own order is not the drawn one.
    rig.batch_levels(
        &[level],
        &[at(4.0), at(10.0)],
        &[Color::rgb(0.0, 1.0, 0.0), Color::rgb(0.0, 0.0, 1.0)],
        options,
    );
    rig.ctx.world.spawn(puff(vec3(0.0, 0.0, -7.0), 4.0, Color::RED));
    rig.ctx.world.spawn(puff(vec3(-0.5, 0.0, -2.0), 0.2, Color::WHITE));
    let image = rig.image();
    let list = rig.renderer.see_through_for_test();
    eprintln!("see-through list {list:?}");
    assert!(
        matches!(
            list.as_slice(),
            [
                SeeThrough::Batch { count: 1, .. },
                SeeThrough::Particles(0, 1),
                SeeThrough::Batch { count: 1, .. },
                SeeThrough::Unsorted,
                SeeThrough::Particles(1, 2)
            ]
        ),
        "far pane, far particle, near pane, the rest, near particle: {list:?}"
    );
    let (on, off) = (around(&image, 64, 64), around(&image, 64, 20));
    eprintln!("on the particle {on:?}, beside it {off:?}");
    assert!(
        on[0] > 70.0 && on[0] < 200.0 && on[1] > 150.0 && on[2] < 20.0,
        "the particle shows through the green pane and hides the blue one: {on:?}"
    );
    assert!(
        off[2] > 70.0 && off[1] > 150.0 && off[0] < 20.0,
        "beside it, the blue pane shows through the green one: {off:?}"
    );
    check_reference(&rig, "see_through_particles_among_batch_instances", &image, "batch");
    // Negative control: after the batch, the particle covers both panes.
    rig.renderer.sort_strips_and_batches = false;
    let before = around(&rig.image(), 64, 64);
    assert!(
        before[0] > 200.0 && before[1] < 20.0,
        "drawn after the batch, the particle covers the green pane: {before:?}"
    );
}

fn check_reference(rig: &BatchRig, name: &str, image: &support::Image, scene: &str) {
    let what = match scene {
        "line" => "a red line (Line3D, 0.5 wide) across the view at depth 7",
        _ => "a big red alpha particle (4 across) at depth 7 and a small white one at depth 2, left of the middle",
    };
    let holder = if scene == "line" {
        "two see-through models"
    } else {
        "one see-through instance batch of two instances"
    };
    support::check_reference(
        name,
        image,
        REFERENCE,
        &format!(
            "Between two unlit see-through panes filling the view ({holder}, alpha 0.6), blue at depth 10 \
             and green at depth 4: {what}; camera at the origin looking down -z; black background, no sun, \
             no bloom; 128x128."
        ),
        &rig.gpu.info,
    );
}

/// Small see-through cubes of three meshes in a grid facing the camera,
/// none covering another, at depths that interleave the meshes far to
/// near: strictly far to near takes about a draw (and a material) per
/// cube; drawn together by mesh, a few draws. The picture is the same to the
/// byte, because no two of them share a pixel.
#[test]
fn see_through_models_of_one_mesh_are_drawn_together_where_none_covers_another() {
    let Some(mut rig) = rig() else { return };
    let colors = [Color::RED, Color::GREEN, Color::rgb(0.2, 0.4, 1.0)];
    let glass = tinted(&mut rig, 0.5);
    // A material each (told apart by their colour, which the shader
    // multiplies the model's by).
    let meshes: Vec<Mesh> = [0.9, 0.95, 1.0]
        .iter()
        .map(|&v| {
            let desc = MaterialDesc {
                color: Color::rgb(v, v, v),
                ..glass
            };
            rig.mesh(&MeshData::cube(0.3), desc)
        })
        .collect();
    let mut k = 0;
    for y in -3..=3 {
        for x in -3..=3 {
            let mesh = meshes[k % 3];
            rig.ctx.world.spawn((
                Transform3D {
                    position: vec3(x as f32 * 1.5, y as f32 * 1.5, -10.0 - k as f32 * 0.01),
                    ..Default::default()
                },
                Model {
                    color: colors[k % 3],
                    ..Model::new(mesh)
                },
            ));
            k += 1;
        }
    }
    let mut drawn = |group: bool| {
        rig.renderer.group_see_through = group;
        rig.frame();
        let image = rig.image();
        let changes = rig.renderer.see_through_changes();
        (image, changes)
    };
    let (strict, strict_changes) = drawn(false);
    let (grouped, grouped_changes) = drawn(true);
    support::record(
        &support::output_dir(),
        &format!(
            "see-through list, 49 cubes of 3 meshes: [pipelines, bind groups, vertex buffers, index buffers, draws] \
             far to near {strict_changes:?}, grouped by mesh {grouped_changes:?}"
        ),
    );
    eprintln!("far to near {strict_changes:?}, grouped {grouped_changes:?}");
    // Sorted by distance, a few neighbours share a mesh already.
    assert!(
        strict_changes[4] >= 30,
        "far to near: about a draw per cube: {strict_changes:?}"
    );
    // Each model looks a window of 32 models ahead for others of its mesh.
    assert!(
        grouped_changes[4] <= 6,
        "grouped: a few draws per mesh: {grouped_changes:?}"
    );
    assert!(
        grouped_changes[1] < strict_changes[1] / 4,
        "grouped, the materials are bound once each: {grouped_changes:?} vs {strict_changes:?}"
    );
    assert!(
        strict.rgba == grouped.rgba,
        "the same picture to the byte, grouped or not"
    );
    // Negative control: the cubes are seen, so a wrong order would show.
    let lit = strict.fraction(0..128, 0..128, |p| p[0] > 40 || p[1] > 40 || p[2] > 40);
    assert!(lit > 0.05, "the cubes cover part of the view ({lit})");
}

/// Three panes that overlap on screen, of two meshes, the far and the
/// near one of the same: the near one is never drawn before the middle
/// one, so the picture is that of far-to-near blending (three draws).
#[test]
fn see_through_models_that_overlap_keep_their_far_to_near_order() {
    let Some(mut rig) = rig() else { return };
    let a = tinted(&mut rig, 0.5);
    let b = tinted(&mut rig, 0.6);
    let (first, second) = (rig.mesh(&MeshData::cube(1.0), a), rig.mesh(&MeshData::cube(1.0), b));
    pane(&mut rig, first, 10.0, Color::rgb(0.0, 0.0, 1.0));
    pane(&mut rig, second, 7.0, Color::RED);
    pane(&mut rig, first, 4.0, Color::rgb(0.0, 1.0, 0.0));
    let mut drawn = |group: bool| {
        rig.renderer.group_see_through = group;
        rig.frame();
        (rig.image(), rig.renderer.see_through_changes())
    };
    let (strict, _) = drawn(false);
    let (grouped, changes) = drawn(true);
    assert_eq!(changes[4], 3, "three draws: nothing moved past the red pane");
    assert!(strict.rgba == grouped.rgba, "the same picture");
    let c = around(&grouped, 64, 64);
    // Far to near: green over red over blue; red shows under the green.
    assert!(
        c[0] > 30.0 && c[1] > 100.0,
        "the red pane shows under the green one: {c:?}"
    );
}
