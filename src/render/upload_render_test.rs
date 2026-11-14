//! Offscreen: what the 3D renderer uploads and creates per frame, once a
//! scene has settled (`RenderStats::uploads`).
use super::buffer_render_test::Rig;
use super::traffic;
#[allow(unused_imports)]
use crate::d3::{
    Line as Line3D, ParticleEmitter as ParticleEmitter3D, ParticleSettings as ParticleSettings3D,
    ParticleShape as ParticleShape3D, PointLight as PointLight3D, Transform as Transform3D,
};
use crate::prelude::*;
#[allow(unused_imports)]
use crate::render::Blend;
#[allow(unused_imports)]
use crate::render::{
    AntiAliasing, GpuUploads, Material, MaterialDesc, Mesh, MeshData, Model, ModelMaterial, RenderStats,
};

/// A rig with `n` × `n` cubes on the ground, seen from above, with sun
/// shadows; `None` without a GPU.
fn scene(n: i32) -> Option<(Rig, Vec<Entity>)> {
    let mut rig = Rig::new()?;
    let mesh = rig.mesh(
        &MeshData::cube(0.4),
        MaterialDesc {
            color: Color::WHITE,
            ..Default::default()
        },
    );
    let mut cubes = Vec::new();
    for x in 0..n {
        for z in 0..n {
            let at = vec3(x as f32 - n as f32 / 2.0, 0.0, z as f32 - n as f32 / 2.0);
            cubes.push(rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh))));
        }
    }
    rig.ctx.scene3d.camera.position = vec3(0.0, n as f32, n as f32);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    Some((rig, cubes))
}

/// Draws `frames` frames after `settle` warm-up ones, calling `change`
/// before each; the most of each count over the measured frames.
fn steady(rig: &mut Rig, settle: usize, frames: usize, mut change: impl FnMut(&mut Rig, usize)) -> GpuUploads {
    for i in 0..settle {
        change(rig, i);
        rig.draw();
    }
    traffic::take();
    let mut most = GpuUploads::default();
    for i in settle..settle + frames {
        change(rig, i);
        rig.draw();
        let c = traffic::take();
        most.instances = most.instances.max(c.instances);
        most.uniforms = most.uniforms.max(c.uniforms);
        most.lights = most.lights.max(c.lights);
        most.static_batches = most.static_batches.max(c.static_batches);
        most.other = most.other.max(c.other);
        most.materials = most.materials.max(c.materials);
        most.meshes = most.meshes.max(c.meshes);
        most.skins = most.skins.max(c.skins);
        most.textures = most.textures.max(c.textures);
        most.buffers_created = most.buffers_created.max(c.buffers_created);
        most.reallocations = most.reallocations.max(c.reallocations);
        most.buffer_bytes_created = most.buffer_bytes_created.max(c.buffer_bytes_created);
    }
    most
}

#[test]
fn a_still_scene_creates_no_buffers_and_uploads_nothing() {
    let Some((mut rig, _)) = scene(16) else { return };
    let c = steady(&mut rig, 4, 6, |_, _| {});
    eprintln!("still scene, 256 models: {c:?} ({} bytes)", c.bytes());
    assert_eq!((c.buffers_created, c.reallocations), (0, 0), "{c:?}");
    // Not the models, nor the frame uniforms of the camera and of every
    // shadow view (which were 15 KiB a frame when uploaded whole).
    assert_eq!(c.bytes(), 0, "{c:?}");
}

#[test]
fn a_still_scene_with_a_running_clock_uploads_only_the_clock() {
    let Some((mut rig, _)) = scene(16) else { return };
    let c = steady(&mut rig, 4, 6, |rig, i| rig.time = (i as f32 / 60.0, 1.0 / 60.0));
    eprintln!("still scene, clock running: {c:?} ({} bytes)", c.bytes());
    // One vec4 in each frame uniform: the camera's and the shadow views'.
    assert!(c.uniforms > 0 && c.uniforms <= 16 * 8, "{c:?}");
    assert_eq!(c.bytes(), c.uniforms, "{c:?}");
    rig.check_mirrors();
}

#[test]
fn a_moving_model_uploads_only_its_part_of_the_instances() {
    let Some((mut rig, cubes)) = scene(16) else { return };
    let moving = cubes[100];
    let c = steady(&mut rig, 4, 6, |rig, i| {
        rig.ctx.world.get_mut::<Transform3D>(moving).unwrap().position.y = (i % 2) as f32 * 0.3;
    });
    eprintln!("one model moving of 256: {c:?} ({} bytes)", c.bytes());
    assert_eq!(c.buffers_created, 0, "{c:?}");
    // Every model's record is 80 bytes; all of them would be 20 KiB per view.
    // The model keeps its place in each view that draws it (the camera and
    // the shadow maps), so only its own records are written.
    assert!(c.instances > 0 && c.instances <= 8 * 80, "{c:?}");
}

/// Step `step` of a run of scene edits: a model moved, two despawned, then
/// enough spawned to outgrow the instance buffer, then one moved back.
fn edit(rig: &mut Rig, cubes: &mut Vec<Entity>, step: usize) {
    match step {
        0 => rig.ctx.world.get_mut::<Transform3D>(cubes[3]).unwrap().position.y = 1.5,
        1 => {
            for e in cubes.drain(10..12) {
                rig.ctx.world.despawn(e);
            }
        }
        2 => {
            let model = *rig.ctx.world.get::<Model>(cubes[0]).unwrap();
            for i in 0..200 {
                let at = vec3((i % 20) as f32 - 10.0, 1.0 + (i / 20) as f32 * 0.5, -10.0);
                cubes.push(rig.ctx.world.spawn((Transform3D::at(at), model)));
            }
        }
        _ => rig.ctx.world.get_mut::<Transform3D>(cubes[3]).unwrap().position.y = 0.0,
    }
}

#[test]
fn edited_scenes_draw_the_same_as_ones_uploaded_whole() {
    let Some((mut rig, mut cubes)) = scene(16) else { return };
    let before = rig.draw();
    rig.draw();
    let mut grew = 0;
    for step in 0..4 {
        edit(&mut rig, &mut cubes, step);
        rig.draw();
        grew += traffic::take().buffers_created;
    }
    let edited = rig.draw();
    assert!(grew > 0, "the spawns outgrew the instance buffer");
    assert_ne!(edited, before, "the edits show");
    // The same scene, drawn once from scratch.
    let Some((mut fresh, mut fresh_cubes)) = scene(16) else {
        return;
    };
    for step in 0..4 {
        edit(&mut fresh, &mut fresh_cubes, step);
    }
    let whole = fresh.draw();
    let differ = edited.iter().zip(&whole).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} bytes differ");
}

#[test]
fn moved_and_removed_lights_draw_the_same_as_ones_uploaded_whole() {
    let light = |color| PointLight3D {
        color,
        intensity: 6.0,
        range: 6.0,
        ..Default::default()
    };
    let lit = |rig: &mut Rig| {
        rig.ctx.scene3d.lighting.sun_intensity = 0.0;
        let red = rig
            .ctx
            .world
            .spawn((Transform3D::at(vec3(-3.0, 1.0, 0.0)), light(Color::RED)));
        let blue = rig
            .ctx
            .world
            .spawn((Transform3D::at(vec3(3.0, 1.0, 0.0)), light(Color::BLUE)));
        (red, blue)
    };
    let edit = |rig: &mut Rig, (red, blue): (Entity, Entity), step: usize| match step {
        0 => rig.ctx.world.get_mut::<Transform3D>(red).unwrap().position.x = 0.0,
        _ => {
            rig.ctx.world.despawn(blue);
        }
    };
    let Some((mut rig, _)) = scene(8) else { return };
    let lights = lit(&mut rig);
    let before = rig.draw();
    rig.draw();
    traffic::take();
    rig.draw();
    assert_eq!(traffic::take().lights, 0, "unchanged lights aren't uploaded again");
    for step in 0..2 {
        edit(&mut rig, lights, step);
        rig.draw();
        assert!(traffic::take().lights > 0, "changed lights are");
    }
    let edited = rig.draw();
    assert_ne!(edited, before, "the edits show");
    let Some((mut fresh, _)) = scene(8) else { return };
    let fresh_lights = lit(&mut fresh);
    for step in 0..2 {
        edit(&mut fresh, fresh_lights, step);
    }
    let whole = fresh.draw();
    let differ = edited.iter().zip(&whole).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} bytes differ");
}

/// Flies the camera along the cubes: every frame some leave its view and
/// others enter.
fn pan(rig: &mut Rig, step: usize) {
    let x = step as f32 * 0.15 - 4.0;
    rig.ctx.scene3d.camera.position = vec3(x, 6.0, 9.0);
    rig.ctx.scene3d.camera.look_at(vec3(x, 0.0, 0.0));
}

#[test]
fn a_moving_camera_uploads_the_models_that_entered_or_left_not_the_scene() {
    let Some((mut rig, _)) = scene(40) else { return };
    // Every cascade is drawn every frame, so its models are uploaded too.
    rig.ctx.scene3d.lighting.shadow_refresh = 1;
    rig.ctx.scene3d.camera.far = 30.0;
    let mut whole = usize::MAX;
    let c = steady(&mut rig, 12, 24, |rig, i| {
        pan(rig, i);
        if i > 0 {
            whole = whole.min(rig.models_drawn() * 80);
        }
    });
    eprintln!("camera panning over 1600 models: {c:?}; the views draw at least {whole} bytes of instances");
    assert!(whole >= 400 * 80, "a few hundred models are in view: {whole}");
    assert_eq!(c.buffers_created, 0, "{c:?}");
    // The models that stay in view keep their records' places, so the
    // frame that uploads the most still uploads a small part of what the
    // views draw. (A frame that sorted its models shifted most of them, and
    // uploaded nearly all.)
    assert!(c.instances > 0 && c.instances * 8 <= whole as u64, "{c:?} of {whole}");
}

/// A rig with cubes on the ground under a low camera, a second mesh (a
/// bigger blue cube, or a ball) and a material to edit them with; `None`
/// without a GPU.
fn flight(ball: bool) -> Option<(Rig, Vec<Entity>, Mesh, Material)> {
    let (mut rig, cubes) = scene(24)?;
    let data = if ball {
        MeshData::sphere(0.3, 12)
    } else {
        MeshData::cube(0.6)
    };
    let other = rig.mesh(
        &data,
        MaterialDesc {
            color: Color::rgb(0.2, 0.5, 1.0),
            ..Default::default()
        },
    );
    let red = rig.ctx.gfx.create_material(MaterialDesc {
        color: Color::RED,
        ..Default::default()
    });
    // Every cascade is drawn every frame, as in a renderer's first frame.
    rig.ctx.scene3d.lighting.shadow_refresh = 1;
    rig.ctx.scene3d.camera.far = 40.0;
    Some((rig, cubes, other, red))
}

/// Step `step` of a flight over the cubes while the scene is edited: the
/// camera moves every step; then models move, are despawned, lose and get
/// components, change mesh and material, and new ones are spawned.
fn fly(rig: &mut Rig, cubes: &mut Vec<Entity>, (other, red): (Mesh, Material), step: usize) {
    pan(rig, step * 4);
    let world = &mut rig.ctx.world;
    match step {
        // Some models move, then all of them.
        2 => {
            for &e in cubes.iter().step_by(3) {
                world.get_mut::<Transform3D>(e).unwrap().position.y = 0.3;
            }
        }
        3 => {
            for (i, &e) in cubes.iter().enumerate() {
                let t = world.get_mut::<Transform3D>(e).unwrap();
                t.position.y = (i % 5) as f32 * 0.1;
                t.rotation = Quat::from_rotation_y(i as f32 * 0.2);
            }
        }
        4 => {
            for e in cubes.drain(280..300) {
                world.despawn(e);
            }
        }
        // Components removed and added.
        5 => {
            for &e in &cubes[300..310] {
                world.remove::<Model>(e);
            }
            for &e in &cubes[310..330] {
                world.insert(e, crate::render::ShadowMesh(None));
            }
        }
        // Another mesh, another material.
        6 => {
            for &e in &cubes[250..280] {
                world.get_mut::<Model>(e).unwrap().mesh = other;
            }
        }
        7 => {
            for &e in &cubes[200..240] {
                world.insert(e, ModelMaterial(red));
            }
        }
        8 => {
            for i in 0..60 {
                let at = vec3((i % 20) as f32 * 0.9 - 9.0, 1.2, (i / 20) as f32 - 4.0);
                cubes.push(world.spawn((Transform3D::at(at), Model::new(other))));
            }
        }
        9 => {
            for &e in &cubes[200..210] {
                world.remove::<ModelMaterial>(e);
            }
            for &e in &cubes[300..305] {
                world.insert(e, Model::new(other).with_color(Color::GREEN));
            }
            for &e in &cubes[310..320] {
                world.remove::<crate::render::ShadowMesh>(e);
            }
        }
        _ => {}
    }
}

/// Flies and edits (see [`fly`]), drawing every step, and after each step
/// draws the same scene in a renderer that has drawn nothing yet: the most
/// pixels that differ between the two, over the steps.
fn flown_against_drawn_once(ball: bool) -> Option<usize> {
    let (mut rig, mut cubes, other, red) = flight(ball)?;
    let mut last = rig.draw();
    let mut most = 0;
    for step in 0..12 {
        fly(&mut rig, &mut cubes, (other, red), step);
        let kept = rig.draw();
        assert_ne!(kept, last, "step {step} shows");
        let (mut fresh, mut fresh_cubes, other, red) = flight(ball)?;
        for earlier in 0..=step {
            fly(&mut fresh, &mut fresh_cubes, (other, red), earlier);
        }
        let whole = fresh.draw();
        let differ = (kept.chunks(4).zip(whole.chunks(4))).filter(|(a, b)| a != b).count();
        most = most.max(differ);
        last = kept;
    }
    Some(most)
}

#[test]
fn a_flight_with_edits_draws_the_same_as_the_scene_drawn_once() {
    let Some(differ) = flown_against_drawn_once(false) else {
        return;
    };
    assert_eq!(differ, 0, "{differ} pixels differ");
}

#[test]
fn models_drawn_in_another_order_differ_by_no_more_than_edge_samples() {
    // A view that keeps its models' places draws the models of one mesh in
    // another order than a first frame does. With opaque balls a few
    // pixels wide, some pixels then differ, by little. Why hasn't been
    // established. (Measured when this test was written, and not asserted
    // here: a first frame of the same balls spawned in another order
    // differed as much, so it comes with the order, not with what is
    // kept.) This bounds how many pixels do.
    let Some(differ) = flown_against_drawn_once(true) else {
        return;
    };
    eprintln!("balls, flown against drawn once: at most {differ} of 16384 pixels differ");
    assert!(differ <= 16384 / 100, "{differ} pixels differ");
}

/// A rig with see-through panes of one mesh: a row of them to the left,
/// then a stack of overlapping ones in other tints in the middle (spawned
/// back to front, or front to back when `reversed`); `None` without a GPU.
fn panes(reversed: bool) -> Option<Rig> {
    let mut rig = Rig::new()?;
    let pane = rig.mesh(
        &MeshData::cube(0.5),
        MaterialDesc {
            color: Color::rgba(1.0, 1.0, 1.0, 0.5),
            blend: Blend::Transparent,
            ..Default::default()
        },
    );
    for i in 0..9 {
        let at = vec3(-0.8 - i as f32 * 0.65, 0.5, 0.5);
        rig.ctx.world.spawn((Transform3D::at(at), Model::new(pane)));
    }
    let tints = [
        Color::RED,
        Color::GREEN,
        Color::BLUE,
        Color::rgb(1.0, 1.0, 0.0),
        Color::WHITE,
    ];
    let mut stack: Vec<usize> = (0..tints.len()).collect();
    if reversed {
        stack.reverse();
    }
    for k in stack {
        let at = vec3(k as f32 * 0.1, 0.5, -(k as f32) * 0.4);
        rig.ctx
            .world
            .spawn((Transform3D::at(at), Model::new(pane).with_color(tints[k])));
    }
    Some(rig)
}

/// Step `step` of a camera flying to the right past the panes: the row
/// leaves its view pane by pane, the stack stays in it.
fn pass_panes(rig: &mut Rig, step: usize) {
    let x = step as f32 * 0.7;
    rig.ctx.scene3d.camera.position = vec3(x, 1.5, 7.0);
    rig.ctx.scene3d.camera.look_at(vec3(x, 0.5, 0.0));
}

#[test]
fn overlapping_see_through_models_blend_the_same_while_others_leave_the_view() {
    let Some(mut rig) = panes(false) else { return };
    pass_panes(&mut rig, 0);
    let mut last = rig.draw();
    let first = rig.models_drawn();
    for step in 1..=5 {
        // The view keeps each pane's place among the mesh's instances; a
        // pane of the row that leaves hands its place to another. The
        // stack must blend as before all the same: in the order of the
        // panes' distance, which a renderer that has drawn nothing yet
        // draws them in too.
        pass_panes(&mut rig, step);
        let kept = rig.draw();
        assert_ne!(kept, last, "step {step} shows");
        let mut fresh = panes(false).unwrap();
        pass_panes(&mut fresh, step);
        let once = fresh.draw();
        let differ = (kept.chunks(4).zip(once.chunks(4))).filter(|(a, b)| a != b).count();
        assert_eq!(
            differ, 0,
            "step {step}: {differ} pixels differ from the scene drawn once"
        );
        last = kept;
    }
    assert!(
        rig.models_drawn() + 3 <= first && rig.models_drawn() >= 5,
        "panes of the row left the view ({first} drawn, then {}), the stack stayed",
        rig.models_drawn()
    );
    // Nor does the order the panes were spawned in show.
    let mut other = panes(true).unwrap();
    pass_panes(&mut other, 5);
    let differ = (last.chunks(4).zip(other.draw().chunks(4)))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differ, 0, "{differ} pixels differ from the stack spawned in reverse");
    // And the stack does blend: its panes' tints show through each other.
    let mixed = last
        .chunks(4)
        .filter(|p| p[0] > 40 && p[1] > 40 && p[2] > 40 && p[0] != p[2])
        .count();
    assert!(mixed > 50, "{mixed} pixels show several tints");
}

/// Still particles, a line and a model: once drawn, none of them is
/// uploaded again; moving particles are.
#[test]
fn still_particles_and_lines_are_not_uploaded_again() {
    let Some((mut rig, _)) = scene(4) else { return };
    let settings = ParticleSettings3D {
        rate: 0.0,
        burst: 200,
        max_particles: 400,
        lifetime: (100.0, 100.0),
        ..Default::default()
    };
    let mut emitter = ParticleEmitter3D::new(settings);
    let at = Transform3D::at(vec3(0.0, 1.0, 0.0));
    emitter.update(1.0 / 60.0, &at);
    assert!(emitter.live_count() > 100);
    let fountain = rig.ctx.world.spawn((at, emitter));
    let line = Line3D::new(
        vec![vec3(-2.0, 0.5, 0.0), vec3(0.0, 1.5, 0.0), vec3(2.0, 0.5, 0.0)],
        0.1,
        Color::RED,
    );
    rig.ctx.world.spawn((Transform3D::default(), line));
    let c = steady(&mut rig, 3, 4, |_, _| {});
    eprintln!("still particles and a line: {c:?}");
    assert_eq!((c.other, c.bytes(), c.buffers_created), (0, 0, 0), "{c:?}");
    rig.check_mirrors();
    // The particles move: they are uploaded, the line isn't.
    let c = steady(&mut rig, 1, 3, |rig, _| {
        let e = rig.ctx.world.get_mut::<ParticleEmitter3D>(fountain).unwrap();
        e.update(1.0 / 60.0, &at);
    });
    eprintln!("moving particles and a still line: {c:?}");
    let particles = rig.ctx.world.get::<ParticleEmitter3D>(fountain).unwrap().live_count() as u64;
    // A particle's record is four vec4s.
    assert!(c.other > 0 && c.other <= particles * 64, "{c:?}");
    rig.check_mirrors();
}

/// Moves `count` of the cubes along x (no turning), to `step`'s place.
fn slide(rig: &mut Rig, cubes: &[Entity], count: usize, step: usize) {
    for &e in cubes.iter().step_by(cubes.len() / count).take(count) {
        rig.ctx.world.get_mut::<Transform3D>(e).unwrap().position.x +=
            if step.is_multiple_of(2) { 0.05 } else { -0.04 };
    }
}

#[test]
fn moving_models_upload_only_the_changed_parts_of_their_motion() {
    // The same scene and moves, with temporal antialiasing (which draws
    // the moved models' motion) and without: the difference is the motion.
    // Motion records exist only for temporal antialiasing: a tier without
    // it (webgl2) draws no motion, so there is nothing to measure.
    let uploads = |taa: bool| -> Option<GpuUploads> {
        let (mut rig, cubes) = scene(16)?;
        if taa {
            if !crate::render::render_test_support::needs(&rig.device, crate::render::GpuFeature::TemporalAntialiasing)
            {
                return None;
            }
            rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
        }
        let c = steady(&mut rig, 4, 6, |rig, i| slide(rig, &cubes, 8, i));
        if taa {
            rig.check_mirrors();
        }
        Some(c)
    };
    let Some(with) = uploads(true) else { return };
    let Some(without) = uploads(false) else { return };
    let motion = with.instances - without.instances;
    eprintln!("8 of 256 models sliding: {motion} bytes of motion records ({with:?})");
    // A record is 144 bytes: the instance and last frame's matrix. A model
    // that slid changes their translations, 16 bytes each.
    assert!(motion > 0 && motion <= 8 * 32, "{motion}");
}

#[test]
fn growing_scenes_reallocate_a_logarithmic_number_of_times() {
    let Some((mut rig, _)) = scene(4) else { return };
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let mesh = rig.ctx.world.query::<&Model>().iter().next().unwrap().mesh;
    rig.draw();
    traffic::take();
    let (mut reallocations, mut grew) = (0, 0);
    let mut models = 16u32;
    for step in 0..60 {
        // 40 more models, some of which move.
        for i in 0..40 {
            let at = vec3(
                (i % 8) as f32 - 4.0,
                0.5 + step as f32 * 0.02,
                (i / 8) as f32 - 6.0 + step as f32 * 0.1,
            );
            rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh)));
        }
        models += 40;
        for (_, t) in rig.ctx.world.query::<(&Model, &mut Transform3D)>().iter().step_by(7) {
            t.position.y += 0.01;
        }
        rig.draw();
        let c = traffic::take();
        reallocations += c.reallocations;
        grew += u32::from(c.reallocations > 0);
        if c.reallocations > 0 {
            // Every buffer replaced starts out unknown, and is written whole.
            rig.check_mirrors();
        }
    }
    eprintln!("{models} models spawned 40 a frame: {reallocations} reallocations in {grew} of 60 frames");
    assert!(reallocations > 0, "the buffers grew");
    // Each buffer doubles: the instances (camera and shadow views, up to
    // five copies of every model) and the motion records some ten times
    // or fewer between them, not once a frame.
    assert!(reallocations <= 2 * (models * 5).ilog2(), "{reallocations}");
    assert!(grew <= 12, "{grew} frames reallocated");
    rig.check_mirrors();
}

#[test]
fn churning_scenes_stop_allocating() {
    // A world as big from frame to frame, whose models come and go: once
    // its buffers fit, it neither makes buffers nor grows them.
    let Some((mut rig, mut cubes)) = scene(20) else { return };
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    let mesh = rig.ctx.world.get::<Model>(cubes[0]).unwrap().mesh;
    let mut churn = |rig: &mut Rig, step: usize| {
        for e in cubes.drain(..80) {
            rig.ctx.world.despawn(e);
        }
        for i in 0..80 {
            let at = vec3(
                (i % 20) as f32 - 10.0,
                0.3 * (step % 3) as f32,
                (i / 20) as f32 - 10.0 + (step % 5) as f32 * 4.0,
            );
            cubes.push(rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh))));
        }
        rig.time = (step as f32 / 60.0, 1.0 / 60.0);
    };
    let mut made = Vec::new();
    for step in 0..40 {
        churn(&mut rig, step);
        rig.draw();
        let c = traffic::take();
        made.push((c.buffers_created, c.reallocations, c.buffer_bytes_created));
        if step % 10 == 9 {
            rig.check_mirrors();
        }
    }
    eprintln!("400 models, 80 replaced a frame: buffers made (count, reallocations, bytes) {made:?}");
    for (step, &m) in made.iter().enumerate().skip(10) {
        assert_eq!(m, (0, 0, 0), "frame {step} allocated");
    }
}

/// A streamed world under load: rows of models are spawned ahead of a
/// camera that flies along them, and despawned behind it; a part of
/// them moves every frame, some with particles. Every few steps, the
/// frame must draw what a renderer that has drawn nothing yet draws, and
/// the renderer's buffers hold what was written.
#[test]
fn a_streamed_world_under_load_draws_what_a_fresh_renderer_draws() {
    const ROWS: i32 = 12;
    const ROW: i32 = 60;
    struct World {
        rig: Rig,
        rows: std::collections::VecDeque<Vec<Entity>>,
        cube: Mesh,
        big: Mesh,
    }
    fn row(w: &mut World, z: i32) {
        let mut row = Vec::new();
        for x in 0..ROW {
            let at = vec3(x as f32 * 0.5 - ROW as f32 / 4.0, 0.0, -(z as f32) * 0.8);
            let mesh = if (x + z) % 3 == 0 { w.big } else { w.cube };
            let mut model = Model::new(mesh);
            if (x * 7 + z) % 11 == 0 {
                model = model.with_color(Color::rgb(0.9, 0.3, 0.2));
            }
            row.push(w.rig.ctx.world.spawn((Transform3D::at(at), model)));
        }
        let fire = ParticleEmitter3D::new(ParticleSettings3D {
            rate: 0.0,
            burst: 30,
            seed: z as u64,
            lifetime: (100.0, 100.0),
            ..Default::default()
        });
        let at = Transform3D::at(vec3(0.0, 0.5, -(z as f32) * 0.8));
        let mut fire = fire;
        fire.update(1.0 / 60.0, &at);
        row.push(w.rig.ctx.world.spawn((at, fire)));
        w.rows.push_back(row);
    }
    fn world() -> Option<World> {
        let mut rig = Rig::new()?;
        let white = MaterialDesc {
            color: Color::WHITE,
            ..Default::default()
        };
        let cube = rig.mesh(&MeshData::cube(0.3), white);
        // Another cube, not a ball: balls a few pixels wide drawn in another
        // order differ at a few edge samples (see
        // `models_drawn_in_another_order_differ_by_no_more_than_edge_samples`).
        let big = rig.mesh(&MeshData::cube(0.45), white);
        rig.ctx.scene3d.lighting.shadow_refresh = 1;
        rig.ctx.scene3d.camera.far = 12.0;
        let mut w = World {
            rig,
            rows: Default::default(),
            cube,
            big,
        };
        for z in 0..ROWS {
            row(&mut w, z);
        }
        Some(w)
    }
    /// Step `step`: a row behind goes, a row ahead comes, every third
    /// model of the rows in view moves, and the camera flies on.
    fn step(w: &mut World, step: i32) {
        for e in w.rows.pop_front().unwrap() {
            w.rig.ctx.world.despawn(e);
        }
        row(w, ROWS + step);
        for (i, row) in w.rows.iter().enumerate() {
            for &e in row.iter().skip(i % 3).step_by(3) {
                if let Some(t) = w.rig.ctx.world.get_mut::<Transform3D>(e) {
                    t.position.y = ((step + i as i32) % 4) as f32 * 0.1;
                    t.rotation = Quat::from_rotation_y(step as f32 * 0.3);
                }
            }
        }
        let z = -(step as f32 + 1.0) * 0.8;
        w.rig.ctx.scene3d.camera.position = vec3(0.0, 2.5, z + 3.0);
        w.rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.0, z - 3.0));
        w.rig.time = (step as f32 / 60.0, 1.0 / 60.0);
    }
    let Some(mut w) = world() else { return };
    w.rig.draw();
    traffic::take();
    let mut made = Vec::new();
    for s in 0..24 {
        step(&mut w, s);
        let kept = w.rig.draw();
        let c = traffic::take();
        made.push((c.buffers_created, c.reallocations));
        w.rig.check_mirrors();
        if s % 6 == 5 {
            let mut fresh = world().unwrap();
            for earlier in 0..=s {
                step(&mut fresh, earlier);
            }
            let whole = fresh.rig.draw();
            let differ = kept.chunks(4).zip(whole.chunks(4)).filter(|(a, b)| a != b).count();
            assert_eq!(differ, 0, "step {s}: {differ} pixels differ from the world drawn once");
            // Not the streamed renderer's (the counts are this thread's).
            traffic::take();
        }
    }
    eprintln!("streamed world: buffers made (count, reallocations) per step {made:?}");
    for (s, &m) in made.iter().enumerate().skip(4) {
        assert_eq!(m, (0, 0), "step {s} allocated though the load stayed the same");
    }
}

#[test]
fn outgrown_particle_buffers_draw_what_buffers_made_that_size_draw() {
    // Particles, then many more of them: their buffer is replaced, and
    // nothing still refers to the old one, nor to what it held.
    let fire = |rig: &mut Rig| {
        let at = Transform3D::at(vec3(0.0, 0.5, 0.0));
        let mut e = ParticleEmitter3D::new(ParticleSettings3D {
            rate: 0.0,
            burst: 20,
            max_particles: 4000,
            lifetime: (100.0, 100.0),
            shape: ParticleShape3D::Sphere { radius: 2.0 },
            ..Default::default()
        });
        e.update(1.0 / 60.0, &at);
        rig.ctx.world.spawn((at, e))
    };
    let more = |rig: &mut Rig, e: Entity| {
        let e = rig.ctx.world.get_mut::<ParticleEmitter3D>(e).unwrap();
        e.burst(3000);
        e.update(0.0, &Transform3D::at(vec3(0.0, 0.5, 0.0)));
    };
    let Some((mut rig, _)) = scene(4) else { return };
    let e = fire(&mut rig);
    let few = rig.draw();
    traffic::take();
    more(&mut rig, e);
    let grown = rig.draw();
    assert_ne!(grown, few, "the particles show");
    assert!(traffic::take().reallocations > 0, "the particle buffer grew");
    rig.check_mirrors();
    let Some((mut fresh, _)) = scene(4) else { return };
    let e = fire(&mut fresh);
    more(&mut fresh, e);
    let once = fresh.draw();
    let differ = grown.chunks(4).zip(once.chunks(4)).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "{differ} pixels differ");
}

#[test]
fn scattered_changes_take_a_bounded_number_of_writes() {
    use super::upload_bench::{scatter, scattered};
    use traffic::{MAX_WRITES, Upload};
    // Every 17th model and every other emitter's particles change: more
    // runs of changes than writes allowed, in both buffers.
    let Some((mut rig, cubes, emitters)) = scattered() else {
        return;
    };
    traffic::take();
    rig.draw();
    // The first frame uploads everything whole.
    let whole = traffic::take();
    for i in 0..4 {
        scatter(&mut rig, &cubes, &emitters, i);
        rig.draw();
    }
    traffic::take_writes();
    traffic::take();
    for i in 4..8 {
        scatter(&mut rig, &cubes, &emitters, i);
        rig.draw();
        let (writes, uploads) = (traffic::take_writes(), traffic::take());
        let instances = writes[Upload::Instances as usize] as usize;
        let particles = writes[Upload::Other as usize] as usize;
        assert!(instances <= MAX_WRITES, "{instances} instance writes");
        assert!(particles <= MAX_WRITES, "{particles} particle writes");
        // Closing gaps uploads unchanged records along, but never more
        // than writing all of them.
        assert!(uploads.instances <= whole.instances, "{uploads:?} {whole:?}");
        assert!(uploads.other <= whole.other, "{uploads:?} {whole:?}");
    }
    // And the buffers hold what was meant.
    rig.check_mirrors();
}

#[test]
fn buffers_stop_at_the_device_limit_and_report_what_does_not_fit() {
    // A device whose buffers hold at most 256 KB: about 3,000 models'
    // instances, or 5,000 particles.
    let Some(mut rig) = Rig::with_limits(true, |l| {
        l.max_buffer_size = 256 << 10;
        l.max_storage_buffer_binding_size = 256 << 10;
    }) else {
        return;
    };
    let mesh = rig.mesh(
        &MeshData::cube(0.4),
        MaterialDesc {
            color: Color::WHITE,
            ..Default::default()
        },
    );
    for x in 0..4 {
        rig.ctx
            .world
            .spawn((Transform3D::at(vec3(x as f32 - 2.0, 0.0, 0.0)), Model::new(mesh)));
    }
    rig.ctx.scene3d.camera.position = vec3(0.0, 4.0, 6.0);
    rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
    let few = rig.draw();
    traffic::take_too_big();
    // Far more models than fit, behind the camera's near ones: reported
    // once, never past the device's limit, and the frame isn't drawn.
    let crowd: Vec<Entity> = (0..80 * 80)
        .map(|k| {
            let at = vec3((k % 80) as f32 * 0.5 - 20.0, 0.0, -(k / 80) as f32 * 0.5 - 4.0);
            rig.ctx.world.spawn((Transform3D::at(at), Model::new(mesh)))
        })
        .collect();
    for _ in 0..3 {
        rig.draw();
    }
    assert_eq!(traffic::take_too_big(), 1, "reported once");
    for e in crowd {
        rig.ctx.world.despawn(e);
    }
    // Back under the limit: drawn as before.
    assert_eq!(rig.draw(), few);
    // And particles past the limit: the ones that fit are drawn.
    let at = Transform3D::at(vec3(0.0, 1.0, 0.0));
    let mut e = ParticleEmitter3D::new(ParticleSettings3D {
        rate: 0.0,
        burst: 8000,
        max_particles: 8000,
        lifetime: (1e4, 1e4),
        ..Default::default()
    });
    e.update(1.0 / 60.0, &at);
    rig.ctx.world.spawn((at, e));
    for _ in 0..3 {
        rig.draw();
    }
    assert_eq!(traffic::take_too_big(), 1, "reported once");
    rig.check_mirrors();
}
