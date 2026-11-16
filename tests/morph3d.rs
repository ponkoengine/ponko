//! Morph targets headless: a dent-shaped blend shape found by name and
//! shown by weight.
#![cfg(feature = "3d")]

#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::anim::{
    AnimationGraph, AnimationLayer, AnimationParameter, AnimationState, AnimationTrack, Animator, Interpolation,
    LayerBlend, MorphAnimation, MorphFrame, MorphTarget, Motion, MotionChild, Skeleton, SkinOf,
};
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::d3::Transform;
use ponko::prelude::*;
#[allow(unused_imports)]
#[cfg(feature = "3d")]
use ponko::render::{MeshData, Model};

// ANCHOR: morph
// System or setup: everything here runs headless too.
fn morph(ctx: &mut Ctx) {
    // One target, named to find it again: at full weight its third corner
    // sinks 0.4 into the sheet. Normals are left out, so the shading keeps
    // its smooth look; give one offset per corner to bend the light along.
    let mut sheet = MeshData::plane(1.0);
    let mut dent = vec![Vec3::ZERO; sheet.positions.len()];
    dent[2] = vec3(0.0, 0.0, -0.4);
    sheet.morph_targets.push(MorphTarget::new("dent", dent));

    let mut halfway = sheet.morph_targets[0].clone();
    halfway.name = "dent_half".into();
    halfway.positions[2] = vec3(0.1, 0.0, -0.15);
    sheet.morph_targets.push(halfway);
    sheet.morph_frames = vec![
        MorphFrame {
            channel: 0,
            target: 1,
            weight: 0.5,
        },
        MorphFrame {
            channel: 0,
            target: 0,
            weight: 1.0,
        },
    ];
    let material = ctx.gfx.default_material();
    let mesh = ctx.gfx.create_mesh(&sheet, material);
    let dent = ctx.gfx.morph_target(mesh, "dent").unwrap();
    let mut weights = ponko::anim::MorphWeights::default();
    weights.set(dent, 0.5); // halfway dented
    let mut skeleton = Skeleton::new(vec![]);
    let expression =
        ponko::anim::AnimationClip::new("expression", &skeleton, vec![]).with_morph_tracks(vec![MorphAnimation::new(
            "dent",
            AnimationTrack::new(vec![0.0, 1.0], vec![Vec4::ZERO, Vec4::X], Interpolation::Linear),
        )]);
    expression.sample(0.5, &mut skeleton);
    ctx.world
        .spawn((Transform::default(), Model::new(mesh), weights, skeleton));
    ctx.gfx.write_animated_morphs(&mut ctx.world);
}
// ANCHOR_END: morph

#[test]
fn a_named_morph_target_is_weighted_on_a_model() {
    let mut world = Game::new("morph", 1, 1).run_headless(0, morph);
    let mut panels = 0;
    world
        .query::<(&Model, &ponko::anim::MorphWeights)>()
        .for_each(|(_model, weights)| {
            panels += 1;
            assert_eq!(weights.0, [0.5], "halfway toward the target's offsets");
        });
    assert_eq!(panels, 1, "the sheet is the only weighted model");
}

fn morph_clip(s: &Skeleton, name: &str, value: f32) -> ponko::anim::AnimationClip {
    ponko::anim::AnimationClip::new("face", s, vec![]).with_morph_tracks(vec![MorphAnimation::new(
        name,
        AnimationTrack::new(vec![0., 2.], vec![Vec4::splat(value); 2], Interpolation::Linear),
    )])
}
fn graph(motions: Vec<Motion>) -> std::sync::Arc<AnimationGraph> {
    AnimationGraph::new(
        vec![AnimationParameter::Float(0.5)],
        motions.into_iter().map(AnimationState::new).collect(),
        vec![],
        ponko::anim::StateId(0),
    )
    .expect("a valid graph")
}
#[test]
fn scalar_sampling_modes_preserve_unanimated_weights() {
    let names = ["smile".into(), "untouched".into()];
    for (mode, values, expected) in [
        (Interpolation::Step, vec![Vec4::ZERO, Vec4::X], 0.),
        (Interpolation::Linear, vec![Vec4::ZERO, Vec4::X], 0.5),
        (
            Interpolation::CubicSpline,
            vec![Vec4::ZERO, Vec4::ZERO, Vec4::X * 4., Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
            1.,
        ),
    ] {
        let mut s = Skeleton::new(vec![]);
        let clip = ponko::anim::AnimationClip::new("face", &s, vec![]).with_morph_tracks(vec![MorphAnimation::new(
            "smile",
            AnimationTrack::new(vec![0., 2.], values, mode),
        )]);
        assert_eq!(clip.duration(), 2.);
        clip.sample(1., &mut s);
        let mut w = ponko::anim::MorphWeights(vec![0.7, 0.3]);
        s.write_morph_weights(&names, &mut w);
        assert_eq!(w.0, [expected, 0.3]);
    }
}
#[test]
fn morphs_blend_in_trees_crossfades_and_masked_additive_layers() {
    let mut s = Skeleton::new(vec![]);
    let a = morph_clip(&s, "smile", 0.2);
    let b = morph_clip(&s, "smile", 0.8);
    let tree = Motion::blend_1d(
        ponko::anim::ParamId(0),
        vec![
            (0., MotionChild::new(Motion::Clip(a.clone()))),
            (1., MotionChild::new(Motion::Clip(b.clone()))),
        ],
    );
    let mut animator = Animator::new(graph(vec![tree, Motion::Clip(b.clone()), Motion::Clip(a)]));
    let names = ["smile".into(), "untouched".into()];
    let read = |s: &Skeleton| {
        let mut w = ponko::anim::MorphWeights(vec![0., 0.3]);
        s.write_morph_weights(&names, &mut w);
        w.0
    };
    animator.update(0., &mut s);
    assert_eq!(read(&s), [0.5, 0.3]);
    animator.crossfade(ponko::anim::StateId(1), 1., 0.);
    animator.update(0.5, &mut s);
    assert!((read(&s)[0] - 0.65).abs() < 1e-6);
    // Interrupting a fade uses the sampled morph snapshot.
    animator.crossfade(ponko::anim::StateId(2), 1., 0.);
    animator.update(0.5, &mut s);
    assert!((read(&s)[0] - 0.425).abs() < 1e-6);
    let mut layer = AnimationLayer::new(graph(vec![Motion::Clip(b)]));
    layer.mode = LayerBlend::Additive;
    layer.weight = 0.5;
    layer.mask = Some(ponko::anim::JointMask::new(vec![]));
    animator.add_layer(layer);
    animator.play(ponko::anim::StateId(2), 0.);
    animator.update(0., &mut s);
    assert!((read(&s)[0] - 0.6).abs() < 1e-6);
}
#[test]
fn automatic_morph_only_animation_writes_components_and_attachments_without_drift() {
    Game::new("morph", 1, 1).run_headless(0, |ctx| {
        let mut mesh = MeshData::plane(1.);
        mesh.morph_targets.push(MorphTarget::new("smile", vec![Vec3::Y; 4]));
        mesh.morph_targets.push(MorphTarget::new("other", vec![Vec3::X; 4]));
        let material = ctx.gfx.default_material();
        let mesh = ctx.gfx.create_mesh(&mesh, material);
        let skeleton = Skeleton::new(vec![]);
        let base = ponko::anim::AnimationClip::new("idle", &skeleton, vec![]);
        let mut animator = Animator::new(graph(vec![Motion::Clip(base)]));
        let mut layer = AnimationLayer::new(graph(vec![Motion::Clip(morph_clip(&skeleton, "smile", 0.4))]));
        layer.mode = LayerBlend::Additive;
        layer.weight = 0.5;
        animator.add_layer(layer);
        let owner = ctx.world.spawn((skeleton, animator));
        let model = ctx.world.spawn((
            Model::new(mesh),
            SkinOf(owner),
            ponko::anim::MorphWeights(vec![0.2, 0.7]),
        ));
        for _ in 0..3 {
            let mut q = ctx.world.query::<(&mut Animator, &mut Skeleton)>();
            let (a, s) = q.get(owner).unwrap();
            a.update(0., s);
            drop(q);
            ctx.gfx.write_animated_morphs(&mut ctx.world);
            assert_eq!(ctx.world.get::<ponko::anim::MorphWeights>(model).unwrap().0, [0.4, 0.7]);
        }
    });
}

#[test]
fn morphs_blend_in_2d_and_override_layers_without_touching_other_targets() {
    let mut skeleton = Skeleton::new(vec![]);
    let child = |v| MotionChild::new(Motion::Clip(morph_clip(&skeleton, "smile", v)));
    let tree = Motion::blend_2d(
        ponko::anim::ParamId(0),
        ponko::anim::ParamId(1),
        vec![(Vec2::ZERO, child(0.2)), (Vec2::ONE, child(0.8))],
    );
    let graph = AnimationGraph::new(
        vec![AnimationParameter::Float(0.5), AnimationParameter::Float(0.5)],
        vec![AnimationState::new(tree)],
        vec![],
        ponko::anim::StateId(0),
    )
    .expect("a valid graph");
    let mut animator = Animator::new(graph);
    let mut layer = AnimationLayer::new(crate::graph(vec![Motion::Clip(morph_clip(&skeleton, "smile", 1.))]));
    layer.weight = 0.5;
    layer.mask = Some(ponko::anim::JointMask::new(vec![]));
    animator.add_layer(layer);
    animator.update(0., &mut skeleton);
    let mut weights = ponko::anim::MorphWeights(vec![0., 0.7]);
    skeleton.write_morph_weights(&["smile".into(), "other".into()], &mut weights);
    assert_eq!(weights.0, [0.75, 0.7]);
}

#[test]
fn invalid_frame_mappings_are_rejected_and_replacing_tracks_updates_duration() {
    let mut mesh = MeshData::plane(1.);
    mesh.morph_targets.push(MorphTarget::new("shape", vec![Vec3::ZERO; 4]));
    for frame in [
        MorphFrame {
            channel: 1,
            target: 0,
            weight: 1.,
        },
        MorphFrame {
            channel: 0,
            target: 0,
            weight: 0.,
        },
        MorphFrame {
            channel: 0,
            target: 0,
            weight: f32::NAN,
        },
    ] {
        mesh.morph_frames = vec![frame];
        assert!(mesh.check().is_err());
    }
    let skeleton = Skeleton::new(vec![]);
    let clip = morph_clip(&skeleton, "smile", 1.);
    let shortened = clip.clone().with_morph_tracks(vec![]);
    assert_eq!(shortened.duration(), 0.);
    assert_eq!(clip.duration(), 2.);
}
