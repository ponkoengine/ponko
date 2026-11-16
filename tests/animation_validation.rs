//! Fallible animation-data constructors: content from files or tools is
//! checked with `try_*` calls that return an `Error` instead of panicking,
//! and the panicking constructors keep their behaviour and messages.
#![cfg(feature = "3d")]

use ponko::anim::{AnimationClip, AnimationTrack, Interpolation, Joint, JointAnimation, MorphAnimation, Skeleton};
use ponko::prelude::*;

fn joint(name: &str, parent: Option<usize>) -> Joint {
    Joint::new(name, parent)
}

fn step(times: Vec<f32>, values: Vec<Vec4>) -> Result<AnimationTrack, Error> {
    AnimationTrack::try_new(times, values, Interpolation::Step)
}

fn joint_track(joint: usize, name: &str) -> JointAnimation {
    JointAnimation::new(joint, name).with_translation(AnimationTrack::new(
        vec![0.0, 2.0],
        vec![Vec4::ZERO, Vec4::ONE],
        Interpolation::Linear,
    ))
}

fn morph(name: &str, end: f32) -> MorphAnimation {
    MorphAnimation::new(
        name,
        AnimationTrack::new(vec![0.0, end], vec![Vec4::ZERO, Vec4::ONE], Interpolation::Linear),
    )
}

#[test]
fn valid_tracks_are_accepted_with_their_keys() {
    let linear = AnimationTrack::try_new(vec![0.0, 0.5], vec![Vec4::ZERO, Vec4::ONE], Interpolation::Linear).unwrap();
    assert_eq!(linear.times(), &[0.0, 0.5]);
    assert_eq!(linear.values(), &[Vec4::ZERO, Vec4::ONE]);
    let cubic = AnimationTrack::try_new(
        vec![0.0],
        vec![Vec4::ZERO, Vec4::ONE, Vec4::ZERO],
        Interpolation::CubicSpline,
    );
    assert_eq!(cubic.unwrap().interpolation(), Interpolation::CubicSpline);
}

#[test]
fn bad_key_times_are_invalid_errors() {
    for times in [
        vec![],
        vec![1.0, 0.0],
        vec![0.0, 0.0],
        vec![-1.0],
        vec![f32::NAN],
        vec![0.0, f32::INFINITY],
    ] {
        let values = vec![Vec4::ZERO; times.len()];
        let err = step(times.clone(), values).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid, "{times:?}");
        assert!(err.message().contains("key times must"), "{err}");
    }
}

#[test]
fn bad_value_counts_and_non_finite_values_are_invalid_errors() {
    let wrong_count = step(vec![0.0, 1.0], vec![Vec4::ZERO]).unwrap_err();
    assert_eq!(wrong_count.kind(), ErrorKind::Invalid);
    let not_finite = step(vec![0.0], vec![Vec4::new(f32::NAN, 0.0, 0.0, 0.0)]).unwrap_err();
    assert_eq!(not_finite.kind(), ErrorKind::Invalid);
    // Cubic tracks need an in-tangent, value and out-tangent per key.
    let cubic = AnimationTrack::try_new(vec![0.0], vec![Vec4::ZERO], Interpolation::CubicSpline).unwrap_err();
    assert_eq!(cubic.kind(), ErrorKind::Invalid);
    assert!(cubic.to_string().starts_with("ponko: "), "{cubic}");
}

#[test]
#[should_panic(expected = "AnimationTrack::new: key times must")]
fn panicking_track_constructor_still_panics_with_the_reason() {
    let _ = AnimationTrack::new(vec![1.0, 0.0], vec![Vec4::ZERO; 2], Interpolation::Step);
}

#[test]
fn valid_skeletons_are_accepted_at_rest() {
    let s = Skeleton::try_new(vec![joint("hip", None), joint("knee", Some(0))]).unwrap();
    assert_eq!(s.find("knee"), Some(1));
    assert_eq!(s.pose.len(), 2);
    assert!(Skeleton::try_new(Vec::new()).unwrap().joints().is_empty());
}

#[test]
fn a_parent_after_its_child_is_an_invalid_error_naming_the_joint() {
    for parent in [1, 2, 7] {
        let err = Skeleton::try_new(vec![joint("hip", None), joint("knee", Some(parent))]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(err.message().contains("joint 1 (knee)"), "{err}");
    }
}

#[test]
fn more_than_65536_joints_is_a_limit_error() {
    let joints = (0..=1 << 16).map(|i| joint(&i.to_string(), None)).collect();
    let err = Skeleton::try_new(joints).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Limit);
    let joints: Vec<Joint> = (0..1 << 16).map(|_| joint("j", None)).collect();
    assert!(Skeleton::try_new(joints).is_ok());
}

#[test]
#[should_panic(expected = "Skeleton::new: joint 0 (root) hangs from joint 0")]
fn panicking_skeleton_constructor_still_panics_with_the_reason() {
    let _ = Skeleton::new(vec![joint("root", Some(0))]);
}

#[test]
fn valid_clips_are_accepted_with_their_duration() {
    let s = Skeleton::new(vec![joint("hip", None), joint("knee", Some(0))]);
    let clip = AnimationClip::try_new("walk", &s, vec![joint_track(1, "knee")]).unwrap();
    assert_eq!(clip.name(), "walk");
    assert_eq!(clip.duration(), 2.0);
    assert_eq!(clip.tracks().len(), 1);
}

#[test]
fn clip_tracks_must_name_their_source_joint() {
    let s = Skeleton::new(vec![joint("hip", None)]);
    for track in [joint_track(0, "knee"), joint_track(1, "hip"), joint_track(5, "tail")] {
        let err = AnimationClip::try_new("walk", &s, vec![track]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Invalid);
        assert!(err.message().contains("joint index and name"), "{err}");
    }
}

#[test]
#[should_panic(expected = "AnimationClip::new: joint index and name must match")]
fn panicking_clip_constructor_still_panics_with_the_reason() {
    let _ = AnimationClip::new("walk", &Skeleton::new(vec![]), vec![joint_track(0, "hip")]);
}

#[test]
fn morph_tracks_extend_the_duration_and_keep_shared_clips_intact() {
    let s = Skeleton::new(vec![joint("hip", None)]);
    let clip = AnimationClip::new("talk", &s, vec![joint_track(0, "hip")]);
    let shared = clip.clone();
    let talk = clip
        .try_with_morph_tracks(vec![morph("open", 3.0), morph("smile", 1.0)])
        .unwrap();
    assert_eq!(talk.duration(), 3.0);
    assert_eq!(talk.morph_tracks().len(), 2);
    assert!(shared.morph_tracks().is_empty(), "the original clip is untouched");
    assert_eq!(shared.duration(), 2.0);
}

#[test]
fn duplicate_morph_track_names_are_invalid_errors_naming_the_track() {
    let clip = AnimationClip::new("talk", &Skeleton::new(vec![]), vec![]);
    let err = clip
        .try_with_morph_tracks(vec![morph("open", 1.0), morph("smile", 1.0), morph("open", 2.0)])
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Invalid);
    assert!(err.message().contains("duplicate morph track name \"open\""), "{err}");
}

#[test]
#[should_panic(expected = "duplicate morph track name")]
fn panicking_morph_track_setter_still_panics_with_the_reason() {
    let _ = AnimationClip::new("talk", &Skeleton::new(vec![]), vec![])
        .with_morph_tracks(vec![morph("open", 1.0), morph("open", 1.0)]);
}

#[test]
fn errors_work_with_the_question_mark_operator() {
    fn build(times: Vec<f32>) -> Result<AnimationClip, Box<dyn std::error::Error>> {
        let s = Skeleton::try_new(vec![joint("hip", None)])?;
        let track = AnimationTrack::try_new(times.clone(), vec![Vec4::ONE; times.len()], Interpolation::Step)?;
        let hip = JointAnimation::new(0, "hip").with_translation(track);
        Ok(AnimationClip::try_new("hop", &s, vec![hip])?.try_with_morph_tracks(vec![])?)
    }
    assert_eq!(build(vec![0.0, 0.25]).unwrap().duration(), 0.25);
    assert!(
        build(vec![0.25, 0.0])
            .unwrap_err()
            .to_string()
            .contains("key times must")
    );
}

#[test]
fn joint_builder_defaults_to_rest_at_the_parent_and_takes_an_index_or_none() {
    let root = Joint::new("hip", None);
    assert_eq!((root.name.as_str(), root.parent), ("hip", None));
    assert_eq!(root.rest, ponko::d3::Transform::default());
    let knee = Joint::new(String::from("knee"), 0).with_rest(ponko::d3::Transform::at(Vec3::NEG_Y));
    assert_eq!(knee.parent, Some(0));
    assert_eq!(knee.rest.position, Vec3::NEG_Y);
    assert_eq!(Joint::new("knee", Some(0)), Joint::new("knee", 0));
}

#[test]
fn joint_animation_builder_sets_only_the_tracks_given() {
    let key = || AnimationTrack::new(vec![0.0], vec![Vec4::ONE], Interpolation::Step);
    let bare = JointAnimation::new(2, "wrist");
    assert_eq!((bare.joint, bare.name.as_str()), (2, "wrist"));
    assert!(bare.translation.is_none() && bare.rotation.is_none() && bare.scale.is_none());
    let full = JointAnimation::new(2, "wrist")
        .with_translation(key())
        .with_rotation(key())
        .with_scale(key());
    assert!(full.translation.is_some() && full.rotation.is_some() && full.scale.is_some());
    assert!(full.with_rotation(None).rotation.is_none());
}

#[test]
fn morph_builders_keep_names_tracks_and_offsets() {
    let channel = MorphAnimation::new(
        "blink",
        AnimationTrack::new(vec![0.0, 0.5], vec![Vec4::ZERO, Vec4::X], Interpolation::Linear),
    );
    assert_eq!(channel.name, "blink");
    assert_eq!(channel.track.times(), &[0.0, 0.5]);
    let target = ponko::anim::MorphTarget::new("blink", vec![Vec3::Y; 3]);
    assert_eq!(target.positions.len(), 3);
    assert!(target.normals.is_empty());
    assert_eq!(target.with_normals(vec![Vec3::Z; 3]).normals, vec![Vec3::Z; 3]);
}

#[test]
fn attach_to_joint_builder_sits_on_the_joint_unless_offset() {
    let person = World::new().spawn((ponko::d3::Transform::default(),));
    let held = ponko::anim::AttachToJoint::new(person, 4);
    assert_eq!((held.skeleton, held.joint), (person, 4));
    assert_eq!(held.offset, ponko::d3::Transform::default());
    let moved = held.with_offset(ponko::d3::Transform::at(Vec3::X));
    assert_eq!(moved.offset.position, Vec3::X);
}

#[test]
fn blend_tree_constructors_build_the_matching_variants() {
    use ponko::anim::{Motion, MotionChild, ParamId};
    let s = Skeleton::new(vec![]);
    let child = || MotionChild::new(Motion::Clip(AnimationClip::new("c", &s, vec![])));
    match Motion::blend_1d(ParamId(1), vec![(0.0, child()), (1.0, child())]) {
        Motion::Blend1D {
            parameter, children, ..
        } => assert_eq!((parameter, children.len()), (ParamId(1), 2)),
        _ => panic!("expected a 1D blend"),
    }
    match Motion::blend_2d(ParamId(0), ParamId(2), vec![(Vec2::ZERO, child())]) {
        Motion::Blend2D { x, y, children, .. } => assert_eq!((x, y, children.len()), (ParamId(0), ParamId(2), 1)),
        _ => panic!("expected a 2D blend"),
    }
}
