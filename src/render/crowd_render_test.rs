//! Offscreen crowds (`Graphics::create_crowd`). A character drawn by a
//! crowd lands on the same pixels as the same mesh drawn as a skinned model
//! in the same pose, on the GPU path (compute culling, skinning in the
//! vertex shader) and on the CPU path (WebGL2's limits: frames posed on the
//! CPU); far levels and fewer influences draw what they should; sun shadows
//! fall (a reference image); the motion vectors follow the animation; ten
//! thousand characters take a handful of draws; and a scene without crowds
//! makes no crowd buffers, uploads or passes. A day into the game the clock
//! is as precise as at its start; global illumination's probes see crowds;
//! window views draw them; occlusion culling leaves out a crowd behind a
//! wall. Skips without a GPU.
use super::render_test_support::{self as support, Image, Tolerance};
use super::static_batch_render_test::{BatchRig, RigSetup, glow};
use crate::anim::{AnimationClip, AnimationTrack, Interpolation, Joint, JointAnimation, Skeleton};
use crate::d3::Transform as Transform3D;
use crate::prelude::*;
use crate::render::traffic;
use crate::render::{
    AntiAliasing, Crowd, CrowdAnimation, CrowdClip, CrowdInstance, CrowdLevel, CrowdOptions, MaterialDesc, Mesh,
    MeshData, Model,
};
use crate::three::GpuMaterialDesc;

/// Where the chest turns on the hips.
pub(super) const PIVOT: Vec3 = Vec3::new(0.0, 1.0, 0.0);

/// A figure 2.15 tall standing on the origin, made of three boxes: hips
/// (on joint 0), chest and head (on joint 1, which turns at [`PIVOT`]). The
/// chest's corners are weighted to both joints: its lower ring 0.4 chest,
/// its upper ring 0.75 (so fewer influences move them differently).
pub(super) fn figure() -> (Skeleton, MeshData) {
    figure_with(1)
}

/// [`figure`] with every box face cut into `cuts` x `cuts` quads (36
/// triangles a cut squared), for a character of a realistic size.
pub(super) fn figure_with(cuts: u32) -> (Skeleton, MeshData) {
    let skeleton = Skeleton::new(vec![
        Joint::new("hips", None),
        Joint::new("chest", 0).with_rest(Transform3D::at(PIVOT)),
    ]);
    let mut mesh = MeshData::default();
    let mut cuboid = |min: Vec3, max: Vec3, weigh: &dyn Fn(Vec3) -> ([u16; 4], [f32; 4])| {
        let faces: [(Vec3, Vec3, Vec3); 6] = [
            (Vec3::X, Vec3::Y, Vec3::Z),
            (Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z),
            (Vec3::Y, Vec3::Z, Vec3::X),
            (Vec3::NEG_Y, Vec3::NEG_Z, Vec3::X),
            (Vec3::Z, Vec3::NEG_Y, Vec3::NEG_X),
            (Vec3::NEG_Z, Vec3::Y, Vec3::NEG_X),
        ];
        let center = (min + max) * 0.5;
        let half = (max - min) * 0.5;
        let row = cuts + 1;
        for (normal, u, v) in faces {
            let base = mesh.positions.len() as u32;
            for j in 0..row {
                for i in 0..row {
                    let (a, b) = (i as f32 / cuts as f32 * 2.0 - 1.0, j as f32 / cuts as f32 * 2.0 - 1.0);
                    let p = center + (normal + u * a + v * b) * half;
                    let (joints, weights) = weigh(p);
                    mesh.positions.push(p);
                    mesh.normals.push(normal);
                    mesh.joints.push(joints);
                    mesh.weights.push(weights);
                }
            }
            // Counter-clockwise seen from outside.
            let quad = if u.cross(v).dot(normal) > 0.0 {
                [0, 1, 2, 0, 2, 3]
            } else {
                [0, 2, 1, 0, 3, 2]
            };
            for j in 0..cuts {
                for i in 0..cuts {
                    let at = base + j * row + i;
                    let corners = [at, at + 1, at + row + 1, at + row];
                    mesh.indices.extend(quad.map(|k| corners[k]));
                }
            }
        }
    };
    let hips = |_: Vec3| ([0, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    let chest = |p: Vec3| {
        let w = if p.y < 1.4 { 0.4 } else { 0.75 };
        ([1, 0, 0, 0], [w, 1.0 - w, 0.0, 0.0])
    };
    let head = |_: Vec3| ([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
    cuboid(vec3(-0.25, 0.0, -0.15), vec3(0.25, 1.0, 0.15), &hips);
    cuboid(vec3(-0.3, 1.0, -0.18), vec3(0.3, 1.8, 0.18), &chest);
    cuboid(vec3(-0.15, 1.85, -0.15), vec3(0.15, 2.15, 0.15), &head);
    (skeleton, mesh)
}

/// The chest turning a quarter turn about z (to the figure's left) in one
/// second.
pub(super) fn bend(skeleton: &Skeleton) -> AnimationClip {
    turn(
        skeleton,
        "bend",
        Vec3::Z,
        &[(0.0, 0.0), (1.0, std::f32::consts::FRAC_PI_2)],
    )
}

/// The chest swaying forward and back about x, once a second.
pub(super) fn sway(skeleton: &Skeleton) -> AnimationClip {
    let a = 0.5;
    turn(skeleton, "sway", Vec3::X, &[(0.0, -a), (0.5, a), (1.0, -a)])
}

fn turn(skeleton: &Skeleton, name: &str, axis: Vec3, keys: &[(f32, f32)]) -> AnimationClip {
    let mut chest = JointAnimation::new(1, "chest");
    chest.rotation = Some(AnimationTrack::new(
        keys.iter().map(|k| k.0).collect(),
        keys.iter()
            .map(|k| Vec4::from(Quat::from_axis_angle(axis, k.1)))
            .collect(),
        Interpolation::Linear,
    ));
    AnimationClip::new(name, skeleton, vec![chest])
}

/// A skinned mesh of `data` with `desc`, in the rig's game and renderer.
pub(super) fn skinned_mesh(rig: &mut BatchRig, data: &MeshData, desc: MaterialDesc, skeleton: &Skeleton) -> Mesh {
    let material = rig.ctx.gfx.create_material(desc);
    let mesh = rig.ctx.gfx.create_skinned_mesh(data, material, skeleton);
    rig.renderer
        .create_material(&rig.gpu.device, &GpuMaterialDesc::new(&desc), &rig.textures);
    let mut weighted = data.clone();
    for w in &mut weighted.weights {
        *w = crate::three::normalized_weights(*w);
    }
    rig.renderer.create_mesh(
        &rig.gpu.device,
        &weighted,
        &[(0..data.indices.len() as u32, material)],
        true,
    );
    mesh
}

/// A crowd made in the rig's game and handed to its renderer, as `Graphics` does.
pub(super) fn crowd(
    rig: &mut BatchRig,
    animation: &CrowdAnimation,
    levels: &[CrowdLevel],
    people: &[CrowdInstance],
    options: CrowdOptions,
) -> Crowd {
    let crowd = rig.ctx.gfx.create_crowd(animation, levels, people, options);
    upload(rig, crowd);
    crowd
}

/// Hands crowd `crowd` as the game has it now to the renderer.
pub(super) fn upload(rig: &mut BatchRig, crowd: Crowd) {
    let data = rig.ctx.gfx.assets3d.crowds[crowd.id as usize].as_ref();
    rig.renderer.set_crowd(&rig.gpu.device, &rig.gpu.queue, crowd.id, data);
}

/// The rig's camera 6 m in front of the figure, looking at its middle.
fn front(rig: &mut BatchRig) {
    rig.ctx.scene3d.camera.position = vec3(0.0, 1.1, 5.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 1.1, 0.0));
}

/// The figure drawn as a skinned model posed by `clip` at `time` (its
/// mesh's weights cut to `influences`, as a crowd level does), against
/// the same figure drawn by a crowd playing `clip` held at `time`
/// (`blend` of a second clip held at 0.25 s, the model showing the same
/// mix). Returns (model, crowd, crowd at rest) images, or `None` without a GPU.
fn model_and_crowd(setup: RigSetup, time: f32, influences: u8, blend: f32) -> Option<(Image, Image, Image)> {
    let mut rig = BatchRig::new(setup)?;
    rig.skins = true;
    front(&mut rig);
    let (skeleton, data) = figure();
    let (bend, sway) = (bend(&skeleton), sway(&skeleton));
    let white = glow(Color::WHITE);
    // The model: the figure's own weights cut as the crowd's level cuts them.
    let mut cut = data.clone();
    for (j, w) in cut.joints.iter_mut().zip(&mut cut.weights) {
        (*j, *w) = crate::three::crowd::top_influences(*j, crate::three::normalized_weights(*w), influences);
    }
    let model_mesh = skinned_mesh(&mut rig, &cut, white, &skeleton);
    let mut posed = skeleton.clone();
    posed.reset_pose();
    bend.sample(time, &mut posed);
    if blend > 0.0 {
        sway.sample_blended(0.25, blend, &mut posed);
    }
    let model = rig
        .ctx
        .world
        .spawn((Transform3D::default(), Model::new(model_mesh), posed));
    let model_image = rig.image();
    rig.ctx.world.despawn(model);
    // The crowd.
    let mesh = skinned_mesh(&mut rig, &data, white, &skeleton);
    let animation = CrowdAnimation::bake(&skeleton, &[CrowdClip::once(bend), sway.into()], 30.0);
    let level = CrowdLevel::new(mesh, 0.0, f32::INFINITY).with_influences(influences);
    let held = |t: f32| {
        CrowdInstance::new(Mat4::IDENTITY)
            .playing(0, t)
            .blending(1, 0.25, blend)
            .with_speed(0.0)
    };
    let c = crowd(&mut rig, &animation, &[level], &[held(time)], CrowdOptions::default());
    let crowd_image = rig.image();
    assert!(rig.ctx.gfx.update_crowd(c, &[held(0.0)]));
    upload(&mut rig, c);
    let rest = rig.image();
    Some((model_image, crowd_image, rest))
}

/// How close a crowd's character must come to the skinned model: blending
/// matrices between baked frames (a crowd) and blending rotations (a
/// model) part by a fraction of a pixel at the bend's edges.
const SAME_POSE: Tolerance = Tolerance {
    channel: 48,
    outliers: 0.01,
    edge: 1,
};

fn check_like_the_model(setup: RigSetup, time: f32, influences: u8, blend: f32, what: &str) {
    let Some((model, crowd, rest)) = model_and_crowd(setup, time, influences, blend) else {
        return;
    };
    let lit = |i: &Image| i.fraction(0..i.width, 0..i.height, |p| p[0] > 128);
    assert!(lit(&model) > 0.02, "{what}: the model draws ({})", lit(&model));
    let d = support::diff_within(&crowd, &model, SAME_POSE);
    eprintln!("{what}: crowd against model {d}");
    support::keep_image(&crowd);
    assert!(
        d.within(SAME_POSE),
        "{what}: the crowd's character isn't where the model is: {d}"
    );
    // Negative control: the crowd at rest is beyond the same tolerance.
    let control = support::diff_within(&rest, &model, SAME_POSE);
    assert!(
        !control.within(SAME_POSE),
        "{what}: the pose at rest passes as the bent one ({control}): the check compares nothing"
    );
}

#[test]
fn a_crowd_character_draws_where_the_skinned_model_does_on_the_gpu_path() {
    // On a baked frame (0.5 s at 30 frames a second), between two (0.61 s),
    // with one influence a corner, and blended half way with a second clip.
    check_like_the_model(RigSetup::default(), 0.5, 4, 0.0, "on a baked frame");
    check_like_the_model(RigSetup::default(), 0.61, 4, 0.0, "between baked frames");
    check_like_the_model(RigSetup::default(), 0.8, 1, 0.0, "one influence");
    check_like_the_model(RigSetup::default(), 0.8, 2, 0.0, "two influences");
    check_like_the_model(RigSetup::default(), 0.7, 4, 0.5, "blended with a second clip");
}

#[test]
fn a_crowd_character_draws_where_the_skinned_model_does_on_the_cpu_path() {
    let cpu = RigSetup {
        compute: false,
        ..Default::default()
    };
    // The CPU path shows the nearest baked frame: on one it is the pose.
    check_like_the_model(cpu, 0.5, 4, 0.0, "CPU path, on a baked frame");
    check_like_the_model(cpu, 0.8, 1, 0.0, "CPU path, one influence");
}

#[test]
fn the_device_picks_the_crowd_path() {
    for compute in [true, false] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            compute,
            ..Default::default()
        }) else {
            return;
        };
        let (skeleton, data) = figure();
        let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
        let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into()], 30.0);
        let c = crowd(
            &mut rig,
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &[CrowdInstance::new(Mat4::IDENTITY)],
            CrowdOptions::default(),
        );
        let gpu = rig.compute_culling();
        assert_eq!(rig.renderer.crowd_on_gpu(c.id), Some(gpu), "compute culling: {gpu}");
    }
}

#[test]
fn far_characters_draw_their_far_level() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    rig.ctx.scene3d.camera.position = vec3(0.0, 1.0, 10.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
    let (skeleton, data) = figure();
    let near = skinned_mesh(&mut rig, &data, glow(Color::rgb(1.0, 0.0, 0.0)), &skeleton);
    let far = skinned_mesh(&mut rig, &data, glow(Color::rgb(0.0, 1.0, 0.0)), &skeleton);
    let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into()], 30.0);
    // The band splits at 11 m: the left figure is nearer, the right farther.
    let levels = [
        CrowdLevel::new(near, 0.0, 11.0),
        CrowdLevel::new(far, 11.0, f32::INFINITY),
    ];
    let people = [
        CrowdInstance::new(Mat4::from_translation(vec3(-2.0, 0.0, 0.0))),
        CrowdInstance::new(Mat4::from_translation(vec3(4.0, 0.0, -6.0))),
    ];
    crowd(&mut rig, &animation, &levels, &people, CrowdOptions::default());
    let image = rig.image();
    support::keep_image(&image);
    let (w, h) = (image.width, image.height);
    let red = |p: [u8; 4]| p[0] > 100 && p[1] < 50;
    let green = |p: [u8; 4]| p[1] > 100 && p[0] < 50;
    assert!(
        image.fraction(0..w / 2, 0..h, red) > 0.01,
        "the near figure draws its near level"
    );
    assert_eq!(image.fraction(0..w / 2, 0..h, green), 0.0, "and not its far one");
    assert!(
        image.fraction(w / 2..w, 0..h, green) > 0.002,
        "the far figure draws its far level"
    );
    assert_eq!(image.fraction(w / 2..w, 0..h, red), 0.0, "and not its near one");
}

/// A field of figures on a lit ground under the sun, seen from above and
/// to the side.
fn shadow_scene(rig: &mut BatchRig, cast: bool) {
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.camera.position = vec3(0.0, 9.0, 12.0);
    rig.ctx.scene3d.camera.look_at(vec3(0.0, 0.5, 0.0));
    let ground = MaterialDesc {
        color: Color::rgb(0.7, 0.7, 0.7),
        ..Default::default()
    };
    let plane = rig.mesh(&MeshData::plane(20.0), ground);
    rig.ctx.world.spawn((Transform3D::default(), Model::new(plane)));
    let (skeleton, data) = figure();
    let body = MaterialDesc {
        color: Color::rgb(0.9, 0.5, 0.2),
        ..Default::default()
    };
    let mesh = skinned_mesh(rig, &data, body, &skeleton);
    let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into(), sway(&skeleton).into()], 30.0);
    let mut people = Vec::new();
    for z in -2..=2_i32 {
        for x in -2..=2_i32 {
            let at = Mat4::from_translation(vec3(x as f32 * 2.5, 0.0, z as f32 * 2.5));
            let clip = (x + z).rem_euclid(2) as u16;
            people.push(
                CrowdInstance::new(at)
                    .playing(clip, 0.1 * (x + 2) as f32)
                    .with_speed(0.0),
            );
        }
    }
    let options = CrowdOptions::default().with_cast_shadows(cast);
    crowd(
        rig,
        &animation,
        &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
        &people,
        options,
    );
}

const SHADOW_TOLERANCE: Tolerance = Tolerance {
    channel: 24,
    outliers: 0.01,
    edge: 1,
};

#[test]
fn a_crowd_casts_sun_shadows() {
    let Some(mut rig) = BatchRig::new(RigSetup {
        size: (192, 128),
        ..Default::default()
    }) else {
        return;
    };
    shadow_scene(&mut rig, true);
    rig.frame();
    let image = rig.image();
    let Some(mut control) = BatchRig::new(RigSetup {
        size: (192, 128),
        ..Default::default()
    }) else {
        return;
    };
    shadow_scene(&mut control, false);
    control.frame();
    let shadowless = control.image();
    // The ground is darker somewhere with shadows than without: count the
    // ground pixels the shadows darken.
    let mut darker = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            let (a, b) = (image.pixel(x, y), shadowless.pixel(x, y));
            let grey = |p: [u8; 4]| p[0].abs_diff(p[2]) < 8;
            if grey(b) && b[0] > 40 && a[0] + 20 < b[0] {
                darker += 1;
            }
        }
    }
    eprintln!("ground pixels in shadow: {darker}");
    assert!(darker > 150, "the figures shadow the ground ({darker} pixels darkened)");
    support::check_reference(
        "crowd_shadows",
        &image,
        SHADOW_TOLERANCE,
        "25 skinned figures of a crowd (two clips held at different times) on a grey ground under the sun, \
         192x128, sun shadows on",
        &rig.gpu.info,
    );
    support::reference_rejects(
        "crowd_shadows",
        &shadowless,
        SHADOW_TOLERANCE,
        "the crowd casting no shadows",
    );
}

/// The motion vector the last frame wrote where the camera sees `at`,
/// `None` where it wrote none.
fn motion_at(rig: &BatchRig, at: Vec3) -> Option<Vec2> {
    let texture = rig.renderer.velocity_texture()?;
    let bytes = support::read_texture(&rig.gpu.device, &rig.gpu.queue, texture);
    let size = texture.width();
    let (x, y) = pixel(rig, at);
    let i = ((y * size + x) * 4) as usize;
    let half = |k: usize| half::f16::from_le_bytes([bytes[i + k * 2], bytes[i + k * 2 + 1]]).to_f32();
    let v = vec2(half(0), half(1));
    (v != Vec2::ZERO).then_some(v)
}

fn pixel(rig: &BatchRig, at: Vec3) -> (u32, u32) {
    let p = rig.ctx.scene3d.camera.view_proj().project_point3(at);
    let size = 128.0;
    (((p.x * 0.5 + 0.5) * size) as u32, ((0.5 - p.y * 0.5) * size) as u32)
}

/// Where a point of the chest's front at `rest` is with the bend at `t`.
fn bent(rest: Vec3, t: f32) -> Vec3 {
    PIVOT + Quat::from_rotation_z(std::f32::consts::FRAC_PI_2 * t.clamp(0.0, 1.0)) * (rest - PIVOT)
}

#[test]
fn crowd_motion_vectors_follow_the_animation() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    if !rig.compute_culling() {
        return support::skip("motion vectors need the GPU path");
    }
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    front(&mut rig);
    let (skeleton, data) = figure();
    let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
    let animation = CrowdAnimation::bake(&skeleton, &[CrowdClip::once(bend(&skeleton))], 60.0);
    let start = 0.2;
    let playing = CrowdInstance::new(Mat4::IDENTITY).playing(0, start);
    let c = crowd(
        &mut rig,
        &animation,
        &[CrowdLevel::new(mesh, 0.0, f32::INFINITY).with_influences(1)],
        &[playing],
        CrowdOptions::default(),
    );
    // Three frames playing: the third's motion is one frame of the bend.
    for _ in 0..3 {
        rig.frame();
    }
    let dt = 1.0 / 60.0;
    let (head, hips) = (vec3(0.0, 2.0, 0.15), vec3(0.0, 0.5, 0.15));
    let now = start + 2.0 * dt;
    let (before, at) = (bent(head, now - dt), bent(head, now));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let (b, a) = (view_proj.project_point3(before), view_proj.project_point3(at));
    let expected = vec2((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
    let got = motion_at(&rig, at);
    eprintln!("head: motion {got:?}, expected {expected:?}");
    let got = got.expect("the bending figure writes motion");
    assert!(
        (got - expected).abs().max_element() < 0.002,
        "the head moves as the bend: {got:?} against {expected:?}"
    );
    assert_eq!(motion_at(&rig, hips), None, "the hips hold still");
    // Held (speed 0): no motion at all.
    assert!(rig.ctx.gfx.update_crowd(c, &[playing.with_speed(0.0)]));
    upload(&mut rig, c);
    rig.frame();
    rig.frame();
    let held = bent(head, start);
    assert_eq!(motion_at(&rig, held), None, "a held pose writes no motion");
    // An update that moves the figure: the motion runs from its last record.
    let moved = Mat4::from_translation(vec3(0.05, 0.0, 0.0));
    assert!(
        rig.ctx
            .gfx
            .update_crowd(c, &[CrowdInstance::new(moved).playing(0, start).with_speed(0.0)])
    );
    upload(&mut rig, c);
    rig.frame();
    let (b, a) = (
        view_proj.project_point3(held),
        view_proj.project_point3(held + vec3(0.05, 0.0, 0.0)),
    );
    let expected = vec2((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
    let got = motion_at(&rig, held + vec3(0.05, 0.0, 0.0)).expect("the moved figure writes motion");
    assert!(
        (got - expected).abs().max_element() < 0.002,
        "it moved by its records: {got:?} against {expected:?}"
    );
    // Gone, then back elsewhere: a character an update appends starts
    // still, not from the record that last stood in its place.
    assert!(rig.ctx.gfx.update_crowd(c, &[]));
    upload(&mut rig, c);
    rig.frame();
    let elsewhere = vec3(-0.05, 0.0, 0.0);
    let back = CrowdInstance::new(Mat4::from_translation(elsewhere))
        .playing(0, start)
        .with_speed(0.0);
    assert!(rig.ctx.gfx.update_crowd(c, &[back]));
    upload(&mut rig, c);
    rig.frame();
    assert_eq!(
        motion_at(&rig, held + elsewhere),
        None,
        "an appended character has no motion of its own yet"
    );
}

#[test]
fn ten_thousand_characters_take_a_few_draws() {
    for compute in [true, false] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            compute,
            ..Default::default()
        }) else {
            return;
        };
        rig.ctx.scene3d.camera.position = vec3(0.0, 40.0, 80.0);
        rig.ctx.scene3d.camera.look_at(Vec3::ZERO);
        let (skeleton, data) = figure();
        let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
        let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into(), sway(&skeleton).into()], 30.0);
        let people: Vec<CrowdInstance> = (0..10_000)
            .map(|i| {
                let at = vec3((i % 100) as f32 - 50.0, 0.0, (i / 100) as f32 - 50.0);
                // All on one baked frame: the CPU path groups by frame.
                CrowdInstance::new(Mat4::from_translation(at))
                    .playing(0, 0.5)
                    .with_speed(0.0)
            })
            .collect();
        crowd(
            &mut rig,
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &people,
            CrowdOptions::default(),
        );
        rig.frame();
        let cost = rig.frame();
        eprintln!("compute {compute}: {} draws", cost.changes.draws);
        // The depth prepass and the color pass, one draw each.
        assert!(
            cost.changes.draws <= 4,
            "10,000 characters in {} draws",
            cost.changes.draws
        );
        let image = rig.image();
        assert!(image.fraction(0..128, 0..128, |p| p[0] > 128) > 0.1, "the crowd draws");
    }
}

#[test]
fn scenes_without_crowds_make_no_crowd_buffers_uploads_or_passes() {
    for compute in [true, false] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            compute,
            ..Default::default()
        }) else {
            return;
        };
        front(&mut rig);
        let cube = rig.mesh(&MeshData::cube(1.0), glow(Color::WHITE));
        rig.ctx.world.spawn((Transform3D::default(), Model::new(cube)));
        rig.frame();
        traffic::take();
        rig.frame();
        let quiet = traffic::take();
        assert_eq!(rig.renderer.crowd_gpu_objects(), 0, "no crowd objects");
        assert!(!rig.renderer.crowds_cull(), "no cull pass");
        assert_eq!(quiet.static_batches, 0, "no crowd uploads");
        // With a crowd: its buffers, uploads and (GPU path) cull pass.
        let (skeleton, data) = figure();
        let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
        let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into()], 30.0);
        let c = crowd(
            &mut rig,
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &[CrowdInstance::new(Mat4::from_translation(vec3(1.5, 0.0, 0.0)))],
            CrowdOptions::default(),
        );
        rig.frame();
        rig.frame();
        traffic::take();
        rig.frame();
        let busy = traffic::take();
        assert!(rig.renderer.crowd_gpu_objects() > 0);
        assert!(busy.static_batches > 0, "a crowd uploads its frame's views and clock");
        assert_eq!(rig.renderer.crowds_cull(), rig.compute_culling());
        // Its steady frames: the crowd code allocates nothing itself.
        assert_eq!(
            rig.prepared_allocations[2], 0,
            "a crowd's steady frame allocates nothing"
        );
        assert_eq!(busy.buffers_created, 0, "nor makes buffers");
        // Freed: no more uploads or passes.
        rig.ctx.free(c);
        rig.renderer.set_crowd(&rig.gpu.device, &rig.gpu.queue, c.id, None);
        rig.frame();
        traffic::take();
        rig.frame();
        assert_eq!(traffic::take().static_batches, 0, "a freed crowd uploads nothing");
        assert!(!rig.renderer.crowds_cull());
    }
}

/// A day of game time, in seconds.
const DAY: f64 = 86_400.0;

#[test]
fn crowd_motion_stays_one_frame_of_the_clip_a_day_into_the_game() {
    // The game clock 24 hours on: a frame's motion is still a 60th of a
    // second of the bend, not the 32-bit clock's steps (at 86,400 s a
    // 32-bit float moves in steps of 1/128 s).
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    if !rig.compute_culling() {
        return support::skip("motion vectors need the GPU path");
    }
    rig.time = DAY;
    rig.ctx.scene3d.post.anti_aliasing = AntiAliasing::Taa;
    front(&mut rig);
    let (skeleton, data) = figure();
    let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
    let animation = CrowdAnimation::bake(&skeleton, &[CrowdClip::once(bend(&skeleton))], 60.0);
    let start = 0.2;
    let c = crowd(
        &mut rig,
        &animation,
        &[CrowdLevel::new(mesh, 0.0, f32::INFINITY).with_influences(1)],
        &[CrowdInstance::new(Mat4::IDENTITY).playing(0, start)],
        CrowdOptions::default(),
    );
    for _ in 0..3 {
        rig.frame();
    }
    let dt = 1.0 / 60.0;
    let (elapsed, past) = rig.renderer.crowd_clock(c.id).expect("the crowd");
    assert!(
        (elapsed - 2.0 * dt).abs() < 1e-5 && (elapsed - past - dt).abs() < 1e-5,
        "the shaders get two frames and one frame since the epoch: {elapsed}, {past}"
    );
    let head = vec3(0.0, 2.0, 0.15);
    let now = start + 2.0 * dt;
    let (before, at) = (bent(head, now - dt), bent(head, now));
    let view_proj = rig.ctx.scene3d.camera.view_proj();
    let (b, a) = (view_proj.project_point3(before), view_proj.project_point3(at));
    let expected = vec2((b.x - a.x) * 0.5, (a.y - b.y) * 0.5);
    let got = motion_at(&rig, at).expect("the bending figure writes motion");
    eprintln!("a day in: head motion {got:?}, expected {expected:?}");
    assert!(
        (got - expected).length() < 0.1 * expected.length(),
        "the head moves one frame of the bend a day into the game: {got:?} against {expected:?}"
    );
}

#[test]
fn a_crowd_left_alone_for_a_day_shows_the_pose_its_clip_does() {
    // A looping one-second bend played from 0.37 s and never updated, the
    // game clock run to a day and 0.24 s in steps of ten minutes: the
    // figure shows the bend at 0.61 s, as one held there does, on both
    // paths. Every step moves the crowd's clock (see `REBASE_SECONDS`).
    for compute in [true, false] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            compute,
            ..Default::default()
        }) else {
            return;
        };
        front(&mut rig);
        let (skeleton, data) = figure();
        let mesh = skinned_mesh(&mut rig, &data, glow(Color::WHITE), &skeleton);
        let animation = CrowdAnimation::bake(&skeleton, &[CrowdClip::looping(bend(&skeleton))], 30.0);
        let level = [CrowdLevel::new(mesh, 0.0, f32::INFINITY)];
        let c = crowd(
            &mut rig,
            &animation,
            &level,
            &[CrowdInstance::new(Mat4::IDENTITY).playing(0, 0.37)],
            CrowdOptions::default(),
        );
        rig.dt = 600.0;
        while rig.time < DAY {
            rig.frame();
        }
        rig.time = DAY + 0.24;
        let day = rig.image();
        let (elapsed, _) = rig.renderer.crowd_clock(c.id).expect("the crowd");
        assert!(
            elapsed.abs() < 16.0, // the clock moves every 16 s (`REBASE_SECONDS`)
            "the shaders' seconds stay small: {elapsed}"
        );
        let held = |t: f32| CrowdInstance::new(Mat4::IDENTITY).playing(0, t).with_speed(0.0);
        assert!(rig.ctx.gfx.update_crowd(c, &[held(0.61)]));
        upload(&mut rig, c);
        let expected = rig.image();
        assert!(rig.ctx.gfx.update_crowd(c, &[held(0.0)]));
        upload(&mut rig, c);
        let rest = rig.image();
        let d = support::diff_within(&day, &expected, SAME_POSE);
        eprintln!("compute {compute}: a day on against held at 0.61 s: {d}");
        support::keep_image(&day);
        assert!(
            d.within(SAME_POSE),
            "compute {compute}: a day on, the crowd shows the bend at 0.61 s: {d}"
        );
        let control = support::diff_within(&rest, &expected, SAME_POSE);
        assert!(
            !control.within(SAME_POSE),
            "compute {compute}: the pose at rest passes as the bent one ({control})"
        );
    }
}

/// The depth grid a readback of the camera's depth would give with a wall
/// in front of everything left of the middle of the picture, at `depth`
/// (reverse-Z), and nothing (the far plane) elsewhere.
fn left_wall_grid(rig: &BatchRig, depth: f32) -> crate::three::occlusion::OcclusionGrid {
    let camera = &rig.ctx.scene3d.camera;
    let (w, h) = (32, 32);
    let depths = (0..w * h).map(|i| if i % w < w / 2 { depth } else { 0.0 }).collect();
    crate::three::occlusion::OcclusionGrid::new(camera.view_proj(), camera.position, w, h, depths)
}

#[test]
fn a_crowd_behind_a_wall_draws_none_of_it() {
    for compute in [true, false] {
        let Some(mut rig) = BatchRig::new(RigSetup {
            compute,
            ..Default::default()
        }) else {
            return;
        };
        // The camera looks down -z at a wall 6 m ahead covering the left
        // half of the picture; the figures stand 22 m behind it, their
        // culling spheres (around every pose) inside its half of the view.
        rig.ctx.scene3d.camera.position = vec3(0.0, 1.0, 8.0);
        rig.ctx.scene3d.camera.look_at(vec3(0.0, 1.0, 0.0));
        let wall = rig.mesh(&MeshData::cube(1.0), glow(Color::rgb(0.0, 0.0, 1.0)));
        rig.ctx.world.spawn((
            Transform3D {
                position: vec3(-20.0, 1.0, 2.0),
                scale: vec3(40.0, 40.0, 0.2),
                ..Default::default()
            },
            Model::new(wall),
        ));
        let face = rig.ctx.scene3d.camera.view_proj().project_point3(vec3(-1.0, 1.0, 2.1)).z;
        let (skeleton, data) = figure();
        let mesh = skinned_mesh(&mut rig, &data, glow(Color::rgb(1.0, 0.0, 0.0)), &skeleton);
        let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into()], 30.0);
        let at = |x: f32| CrowdInstance::new(Mat4::from_translation(vec3(x, 0.0, -20.0)));
        let behind: Vec<CrowdInstance> = [-4.0, -6.0, -8.0].map(at).to_vec();
        let c = crowd(
            &mut rig,
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &behind,
            CrowdOptions::default(),
        );
        let drawn = |rig: &BatchRig| {
            rig.renderer
                .crowd_drawn(&rig.gpu.device, &rig.gpu.queue, c.id, 0)
                .expect("the crowd")
        };
        // Negative control: without occlusion culling the camera's view
        // has every figure (the wall still hides them in the picture).
        rig.frame();
        let all = rig.image();
        assert_eq!(drawn(&rig), 3, "compute {compute}: no occlusion culling, all drawn");
        rig.ctx.scene3d.camera.occlusion_culling = true;
        rig.grid = Some(left_wall_grid(&rig, face));
        let culled = rig.image();
        assert_eq!(drawn(&rig), 0, "compute {compute}: the crowd behind the wall draws none of it");
        let red = |p: [u8; 4]| p[0] > 100 && p[2] < 50;
        assert_eq!(all.fraction(0..128, 0..128, red), 0.0, "the wall hides them anyway");
        let d = support::diff(&culled, &all, 2);
        assert!(d.within(super::static_batch_render_test::SAME_DEVICE), "and nothing pops: {d}");
        // Figures beside the wall are drawn.
        let mut people = behind.clone();
        people.extend([4.0, 7.0].map(at));
        assert!(rig.ctx.gfx.update_crowd(c, &people));
        upload(&mut rig, c);
        let image = rig.image();
        assert_eq!(drawn(&rig), 2, "compute {compute}: the two beside the wall are drawn");
        assert!(image.fraction(64..128, 0..128, red) > 0.001, "and seen");
    }
}

/// A skinned unit cube (every corner on the one joint of `skeleton`).
fn skinned_cube() -> (Skeleton, MeshData) {
    let skeleton = Skeleton::new(vec![Joint::new("root", None)]);
    let mut cube = MeshData::cube(1.0);
    cube.joints = vec![[0; 4]; cube.positions.len()];
    cube.weights = vec![[1.0, 0.0, 0.0, 0.0]; cube.positions.len()];
    (skeleton, cube)
}

/// The box from `lo` to `hi` as a model matrix of the unit cube.
fn box_at(lo: Vec3, hi: Vec3) -> Mat4 {
    Mat4::from_scale_rotation_translation(hi - lo, Quat::IDENTITY, (lo + hi) * 0.5)
}

/// Global illumination's closed white room (`gi_render_test`'s red wall
/// room) whose sunlit west wall is a red crowd character (a skinned cube
/// stretched over the wall), or a red model with `crowd` false.
fn red_crowd_room(crowd: bool) -> Option<super::gi_render_test::Rig> {
    let mut rig = super::gi_render_test::Rig::new(true)?;
    let white = rig.mesh(&MeshData::cube(1.0), MaterialDesc::color(Color::WHITE));
    let red = MaterialDesc::color(Color::rgb(0.9, 0.04, 0.04));
    let (inner, outer) = (3.5, 3.9);
    let walls = [
        (vec3(-outer, -0.9, -outer), vec3(outer, -0.5, outer)), // floor
        (vec3(inner, -0.5, -outer), vec3(outer, 3.5, outer)),   // east
        (vec3(-inner, -0.5, -outer), vec3(inner, 3.5, -inner)), // north
        (vec3(-inner, -0.5, inner), vec3(inner, 3.5, outer)),   // south
        // The roof, with an opening from x -2 to 0 and z -1.5 to 1.5.
        (vec3(-outer, 3.5, -outer), vec3(-2.0, 3.9, outer)),
        (vec3(0.0, 3.5, -outer), vec3(outer, 3.9, outer)),
        (vec3(-2.0, 3.5, -outer), vec3(0.0, 3.9, -1.5)),
        (vec3(-2.0, 3.5, 1.5), vec3(0.0, 3.9, outer)),
    ];
    for (lo, hi) in walls {
        let (scale, _, position) = box_at(lo, hi).to_scale_rotation_translation();
        rig.ctx.world.spawn((
            Transform3D {
                position,
                scale,
                ..Default::default()
            },
            Model::new(white),
        ));
    }
    let west = box_at(vec3(-outer, -0.5, -outer), vec3(-inner, 3.5, outer));
    if crowd {
        let (skeleton, cube) = skinned_cube();
        let material = rig.ctx.gfx.create_material(red);
        let mesh = rig.ctx.gfx.create_skinned_mesh(&cube, material, &skeleton);
        rig.renderer
            .create_material(&rig.device, &GpuMaterialDesc::new(&red), &rig.textures);
        rig.renderer
            .create_mesh(&rig.device, &cube, &[(0..cube.indices.len() as u32, material)], true);
        let idle = AnimationClip::new("idle", &skeleton, vec![]);
        let animation = CrowdAnimation::bake(&skeleton, &[idle.into()], 30.0);
        let c = rig.ctx.gfx.create_crowd(
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &[CrowdInstance::new(west)],
            CrowdOptions::default(),
        );
        let data = rig.ctx.gfx.assets3d.crowds[c.id as usize].as_ref();
        rig.renderer.set_crowd(&rig.device, &rig.queue, c.id, data);
    } else {
        let mesh = rig.mesh(&MeshData::cube(1.0), red);
        let (scale, _, position) = west.to_scale_rotation_translation();
        rig.ctx.world.spawn((
            Transform3D {
                position,
                scale,
                ..Default::default()
            },
            Model::new(mesh),
        ));
    }
    let lighting = &mut rig.ctx.scene3d.lighting;
    lighting.sun_direction = vec3(-1.0, -1.0, 0.0);
    lighting.sun_color = Color::WHITE;
    lighting.sun_intensity = 4.0;
    rig.ctx.scene3d.sky.sky_color = Color::rgb(0.5, 0.5, 0.5);
    rig.ctx.scene3d.sky.ground_color = Color::rgb(0.5, 0.5, 0.5);
    lighting.ambient_intensity = 0.3;
    lighting.shadow_distance = 30.0;
    lighting.gi = crate::render::GlobalIllumination {
        enabled: true,
        extent: vec3(12.0, 4.0, 8.0),
        spacing: 1.0,
        probes_per_frame: crate::render::GlobalIllumination::MAX_PROBES_PER_FRAME,
        intensity: 1.0,
    };
    rig.ctx.scene3d.camera.position = vec3(2.0, 1.3, 0.3);
    rig.ctx.scene3d.camera.look_at(vec3(-3.5, -0.2, 0.0));
    Some(rig)
}

#[test]
fn global_illumination_probes_see_crowds_as_they_see_models() {
    // The sunlit red wall a crowd character: the probes see it, so the
    // floor beside it takes red bounce light, as beside a red model wall.
    let Some(mut models) = red_crowd_room(false) else {
        return;
    };
    let Some(mut crowds) = red_crowd_room(true) else {
        return;
    };
    let (a, b) = (models.draw(80), crowds.draw(80));
    let near_red = vec3(-3.0, -0.5, -0.8);
    let far_side = vec3(-0.5, -0.5, -0.8);
    let sunlit_wall = vec3(-3.5, 1.0, 0.0);
    let (m, n) = (models.at(&a, near_red), crowds.at(&b, near_red));
    let (mf, nf) = (models.at(&a, far_side), crowds.at(&b, far_side));
    eprintln!("floor by the red wall: {m:?} model, {n:?} crowd; further away {mf:?}, {nf:?}");
    assert!(crowds.at(&b, sunlit_wall)[0] > 150, "the crowd wall is drawn, in the sun");
    assert!(n[0] > n[1] + 20, "red bounce light from a crowd wall: {n:?}");
    for (x, y) in [(m, n), (mf, nf)] {
        assert!(
            (0..3).all(|k| (x[k] - y[k]).abs() <= 12),
            "a crowd lights the floor as a model does: {x:?} vs {y:?}"
        );
    }
}

#[test]
fn window_views_draw_crowds_for_their_own_camera() {
    use super::regression_stage::{Setup, Stage};
    use crate::camera::Viewport;
    // Both paths: the desktop tier culls on the GPU, WebGL2's on the CPU.
    for tier in [None, Some(crate::render::GpuTier::WebGl2)] {
        let Some(mut stage) = Stage::with(Setup {
            tier,
            ..Default::default()
        }) else {
            return;
        };
        let (skeleton, data) = figure();
        let red = glow(Color::rgb(1.0, 0.0, 0.0));
        let material = stage.ctx.gfx.create_material(red);
        let mesh = stage.ctx.gfx.create_skinned_mesh(&data, material, &skeleton);
        stage
            .renderer
            .create_material(&stage.device, &GpuMaterialDesc::new(&red), &stage.textures);
        let mut weighted = data.clone();
        for w in &mut weighted.weights {
            *w = crate::three::normalized_weights(*w);
        }
        stage
            .renderer
            .create_mesh(&stage.device, &weighted, &[(0..data.indices.len() as u32, material)], true);
        let animation = CrowdAnimation::bake(&skeleton, &[bend(&skeleton).into()], 30.0);
        // A figure at x = -6 and one at x = +6.
        let people = [-6.0, 6.0].map(|x| CrowdInstance::new(Mat4::from_translation(vec3(x, 0.0, 0.0))));
        let c = stage.ctx.gfx.create_crowd(
            &animation,
            &[CrowdLevel::new(mesh, 0.0, f32::INFINITY)],
            &people,
            CrowdOptions::default(),
        );
        let crowd_data = stage.ctx.gfx.assets3d.crowds[c.id as usize].as_ref();
        stage
            .renderer
            .set_crowd(&stage.device, &stage.queue, c.id, crowd_data);
        // Player one looks at the left figure, player two at the right one.
        let camera_at = |stage: &Stage, x: f32, rect: Viewport| {
            let mut camera = stage.ctx.scene3d.camera;
            camera.position = vec3(x, 1.5, 6.0);
            camera.look_at(vec3(x, 1.0, 0.0));
            camera.rect = rect;
            camera
        };
        stage.ctx.scene3d.camera = camera_at(&stage, -6.0, Viewport::split(2, 0));
        let p2 = camera_at(&stage, 6.0, Viewport::split(2, 1));
        stage.ctx.scene3d.views.push(crate::d3::View::new(p2));
        let mut windows = super::views::WindowViews::default();
        let red = |p: [u8; 4]| p[0] > 100 && p[1] < 50 && p[2] < 50;
        // Twice: the main frame after a view's draws its crowd still.
        for k in 0..2 {
            let image = super::views_render_test::frame(&mut stage, &mut windows, true);
            let (left, right) = (image.fraction(0..64, 0..128, red), image.fraction(64..128, 0..128, red));
            eprintln!("tier {tier:?}, frame {k}: red {left} left, {right} right");
            assert!(left > 0.02, "{tier:?}: player one sees the left figure");
            assert!(right > 0.02, "{tier:?}: player two's view draws the right figure");
        }
        // Negative control: player two turned away from both sees none.
        stage.ctx.scene3d.views[0].camera = camera_at(&stage, 30.0, Viewport::split(2, 1));
        let image = super::views_render_test::frame(&mut stage, &mut windows, true);
        assert_eq!(image.fraction(64..128, 0..128, red), 0.0, "{tier:?}: nothing there");
        assert!(image.fraction(0..64, 0..128, red) > 0.02, "{tier:?}: player one still does");
        stage.finish();
    }
}
