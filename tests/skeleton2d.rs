//! 2D skeletons: pose math, clips, blending, the state machine, events,
//! slots, IK, the file formats, hot reload and reflection.
use std::f32::consts::{FRAC_PI_2, PI};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ponko::anim::{AnimationCondition, AnimationParameter, AnimationTransition, ParamId, StateId};
use ponko::prelude::*;
use ponko::skeleton2d::{
    BonePose, BoneTrack, Clip, Curve, IkConstraint, Key, MeshVertex, Skeleton, SkeletonData, SkeletonGraph,
    SkeletonMotion, SkeletonState, SkinnedMesh, SlotTrack, SpritePart,
};

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

fn near2(a: Vec2, b: Vec2) {
    assert!(a.distance(b) < 1e-3, "{a} != {b}");
}

fn pixel() -> Texture {
    // Texture handles are only made by `Graphics`; headless gives real ones.
    let mut tex = None;
    Game::new("t", 1, 1).run_headless(0, |ctx| tex = Some(ctx.gfx.create_texture(4, 4, &[255; 64])));
    tex.unwrap()
}

/// root -> arm (10, 0) -> hand (20, 0), each 20 long; one slot per bone.
fn arm() -> SkeletonData {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let arm = data.add_bone("arm", Some(root), BonePose::at(vec2(10.0, 0.0)), 20.0);
    let hand = data.add_bone("hand", Some(arm), BonePose::at(vec2(20.0, 0.0)), 20.0);
    let tex = pixel();
    for (name, bone) in [("arm", arm), ("hand", hand)] {
        let slot = data.add_slot(name, bone);
        data.add_attachment(slot, name, SpritePart::new(tex));
    }
    data
}

fn rotate(bone: usize, keys: Vec<Key<f32>>) -> BoneTrack {
    let mut t = BoneTrack::new(bone);
    t.rotate = keys;
    t
}

fn translate(bone: usize, keys: Vec<Key<Vec2>>) -> BoneTrack {
    let mut t = BoneTrack::new(bone);
    t.translate = keys;
    t
}

#[test]
fn bones_compose_translation_rotation_and_scale_down_the_hierarchy() {
    let mut data = SkeletonData::new();
    let root = data.add_bone(
        "root",
        None,
        BonePose::at(vec2(10.0, 0.0)).with_rotation(FRAC_PI_2),
        0.0,
    );
    let child = data.add_bone(
        "child",
        Some(root),
        BonePose::at(vec2(5.0, 0.0)).with_scale(vec2(2.0, 2.0)),
        0.0,
    );
    let s = Skeleton::new(Arc::new(data));
    // A quarter turn clockwise on screen (y down) takes +x to +y.
    near2(s.bone_position(child), vec2(10.0, 5.0));
    near(s.bone_rotation(child), FRAC_PI_2);
    // The child's scale doubles distances along its own axes.
    near2(s.bone_point(child, vec2(1.0, 0.0)), vec2(10.0, 7.0));
}

#[test]
fn clip_keys_are_offsets_from_the_rest_pose_and_interpolate_linearly() {
    let mut data = arm();
    let clip = Clip::new("move", 1.0).with_bone_track(translate(
        1,
        vec![Key::new(0.0, Vec2::ZERO), Key::new(1.0, vec2(0.0, 10.0))],
    ));
    data.add_clip(clip);
    let mut s = Skeleton::new(Arc::new(data));
    s.play("move");
    s.update(0.25);
    near2(s.bone_pose(1).translation, vec2(10.0, 2.5));
    near2(s.bone_position(2), vec2(30.0, 2.5));
}

#[test]
fn stepped_keys_hold_until_the_next_key() {
    let mut data = arm();
    let keys = vec![Key::new(0.0, 0.0).with_curve(Curve::Stepped), Key::new(1.0, 1.0)];
    data.add_clip(Clip::new("snap", 2.0).with_bone_track(rotate(1, keys)));
    let mut s = Skeleton::new(Arc::new(data));
    s.play_once("snap");
    s.update(0.9);
    near(s.bone_pose(1).rotation, 0.0);
    s.update(0.2);
    near(s.bone_pose(1).rotation, 1.0);
}

#[test]
fn rotation_keys_go_straight_through_their_values() {
    let mut data = arm();
    let keys = vec![Key::new(0.0, 3.0), Key::new(1.0, -3.0), Key::new(2.0, -3.0 + 2.0 * PI)];
    data.add_clip(Clip::new("spin", 2.0).with_bone_track(rotate(1, keys)));
    let mut s = Skeleton::new(Arc::new(data));
    s.play_once("spin");
    s.update(0.5);
    // Half way from 3 to -3 is 0: values aren't wrapped the shorter way.
    near(s.bone_pose(1).rotation, 0.0);
    // A key a full turn on turns all the way round.
    s.update(1.0);
    near(s.bone_pose(1).rotation, -3.0 + PI);
}

#[test]
fn looping_clips_wrap_and_once_clips_hold_their_last_pose() {
    let mut data = arm();
    let keys = vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0)];
    data.add_clip(Clip::new("swing", 1.0).with_bone_track(rotate(1, keys)));
    let data = Arc::new(data);
    let mut looping = Skeleton::new(data.clone());
    looping.play("swing");
    looping.update(1.25);
    near(looping.bone_pose(1).rotation, 0.25);
    near(looping.normalized_time(), 1.25);
    assert!(!looping.finished());
    let mut once = Skeleton::new(data);
    once.play_once("swing");
    once.update(1.25);
    near(once.bone_pose(1).rotation, 1.0);
    assert!(once.finished());
}

/// Ten hours of 60 Hz updates in one looping clip: the pose still turns
/// one step's worth each update, the event fires once a loop, and the
/// clock reads ten hours' worth of loops.
#[test]
fn a_looping_clip_keeps_its_speed_and_events_after_ten_hours() {
    let mut data = arm();
    let keys = vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0)];
    data.add_clip(
        Clip::new("swing", 1.0)
            .with_bone_track(rotate(1, keys))
            .with_event(0.5, "half"),
    );
    let mut s = Skeleton::new(Arc::new(data));
    s.play("swing");
    let dt = 1.0f32 / 60.0;
    let steps = 10 * 3600 * 60;
    for _ in 0..steps {
        s.update(dt);
    }
    s.drain_events().for_each(drop);
    let expected = steps as f64 * f64::from(dt);
    let loops = s.normalized_time_f64();
    assert!((loops - expected).abs() < 1e-6, "{loops} loops, expected {expected}");
    let mut angle = s.bone_pose(1).rotation;
    for _ in 0..120 {
        s.update(dt);
        let now = s.bone_pose(1).rotation;
        let turned = (now - angle).rem_euclid(1.0);
        assert!((turned - dt).abs() < 1e-4, "turned {turned} in one step");
        angle = now;
    }
    assert_eq!(s.drain_events().count(), 2, "once a loop");
}

#[test]
fn speed_scales_the_clock() {
    let mut data = arm();
    data.add_clip(Clip::new("swing", 2.0).with_bone_track(rotate(1, vec![Key::new(0.0, 0.0), Key::new(2.0, 2.0)])));
    let mut s = Skeleton::new(Arc::new(data));
    s.speed = 2.0;
    s.play("swing");
    s.update(0.25);
    near(s.bone_pose(1).rotation, 0.5);
}

#[test]
fn crossfades_blend_from_the_old_pose_to_the_new_over_their_duration() {
    let mut data = arm();
    data.add_clip(Clip::new("left", 1.0).with_bone_track(rotate(1, vec![Key::new(0.0, -1.0)])));
    data.add_clip(Clip::new("right", 1.0).with_bone_track(rotate(1, vec![Key::new(0.0, 1.0)])));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("left");
    s.update(0.1);
    near(s.bone_pose(1).rotation, -1.0);
    assert!(s.crossfade("right", 0.4));
    s.update(0.2);
    assert!(s.fading());
    near(s.bone_pose(1).rotation, 0.0);
    s.update(0.2);
    assert!(!s.fading());
    near(s.bone_pose(1).rotation, 1.0);
    assert_eq!(s.clip(), Some("right"));
    assert!(!s.crossfade("missing", 0.1), "unknown clips change nothing");
}

fn blend_graph(data: &mut SkeletonData) -> Arc<SkeletonGraph> {
    let walk = data.add_clip(Clip::new("walk", 1.0).with_bone_track(rotate(1, vec![Key::new(0.0, 0.0)])));
    let run = data.add_clip(Clip::new("run", 0.5).with_bone_track(rotate(1, vec![Key::new(0.0, 1.0)])));
    SkeletonGraph::new(
        vec![("speed", AnimationParameter::Float(0.0))],
        vec![SkeletonState::new(
            "move",
            SkeletonMotion::Blend1D {
                parameter: ParamId(0),
                children: vec![(100.0, walk), (200.0, run)],
            },
        )],
        vec![],
        StateId(0),
    )
    .unwrap()
}

#[test]
fn blend1d_mixes_the_two_clips_around_the_parameter_and_clamps_at_the_ends() {
    let mut data = arm();
    let graph = blend_graph(&mut data);
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    let speed = s.parameter_id("speed").unwrap();
    assert_eq!(s.state(), Some(StateId(0)));
    for (value, rotation) in [
        (0.0, 0.0),
        (100.0, 0.0),
        (125.0, 0.25),
        (150.0, 0.5),
        (200.0, 1.0),
        (900.0, 1.0),
    ] {
        s.set_float(speed, value);
        s.update(0.0);
        near(s.bone_pose(1).rotation, rotation);
    }
}

#[test]
fn blend1d_children_keep_their_phase_and_the_state_lasts_their_mixed_length() {
    let mut data = arm();
    let graph = blend_graph(&mut data);
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    let speed = s.parameter_id("speed").unwrap();
    // Half walk (1 s), half run (0.5 s): one loop takes 0.75 s.
    s.set_float(speed, 150.0);
    s.update(0.375);
    near(s.normalized_time(), 0.5);
}

#[test]
fn graph_transitions_use_the_shared_conditions_and_consume_triggers() {
    let mut data = arm();
    let idle = data.add_clip(Clip::new("idle", 1.0));
    let jump = data.add_clip(Clip::new("jump", 0.5));
    let graph = SkeletonGraph::new(
        vec![
            ("jump", AnimationParameter::Trigger(false)),
            ("height", AnimationParameter::Float(0.0)),
        ],
        vec![
            SkeletonState::new("idle", SkeletonMotion::Clip(idle)),
            SkeletonState::new("jump", SkeletonMotion::Clip(jump)).once(),
        ],
        vec![
            AnimationTransition::new(StateId(0), StateId(1)).with_condition(AnimationCondition::Trigger(ParamId(0))),
            AnimationTransition::new(StateId(1), StateId(0))
                .with_exit_time(1.0)
                .with_condition(AnimationCondition::Less(ParamId(1), 1.0)),
        ],
        StateId(0),
    )
    .unwrap();
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    let trigger = s.parameter_id("jump").unwrap();
    s.update(0.1);
    assert_eq!(s.state(), Some(StateId(0)));
    s.set_trigger(trigger, true);
    s.update(0.1);
    assert_eq!(s.state(), Some(StateId(1)));
    assert!(
        matches!(s.parameter(trigger), Some(AnimationParameter::Trigger(false))),
        "consumed"
    );
    s.update(0.2);
    assert_eq!(s.state(), Some(StateId(1)), "exit time not reached");
    s.update(0.4);
    assert_eq!(s.state(), Some(StateId(0)));
    assert_eq!(s.clip(), Some("idle"));
}

#[test]
fn checked_calls_say_why_instead_of_panicking_and_change_nothing() {
    use ponko::anim::AnimationGraphError as E;
    let mut data = arm();
    let idle = data.add_clip(Clip::new("idle", 1.0));
    let graph = SkeletonGraph::new(
        vec![
            ("speed", AnimationParameter::Float(0.0)),
            ("grounded", AnimationParameter::Bool(true)),
            ("jump", AnimationParameter::Trigger(false)),
        ],
        vec![
            SkeletonState::new("idle", SkeletonMotion::Clip(idle)),
            SkeletonState::new("hold", SkeletonMotion::Clip(idle)).once(),
        ],
        vec![],
        StateId(0),
    )
    .unwrap();
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    let (speed, grounded, jump) = (ParamId(0), ParamId(1), ParamId(2));
    assert_eq!(s.try_set_float(speed, 2.0), Ok(()));
    assert_eq!(s.try_set_float(speed, f32::NAN), Err(E::NonFiniteValue(speed)));
    assert!(
        matches!(s.parameter(speed), Some(AnimationParameter::Float(2.0))),
        "kept"
    );
    let not = |parameter, expected| E::WrongParameterKind { parameter, expected };
    assert_eq!(s.try_set_float(grounded, 1.0), Err(not(grounded, "float")));
    assert_eq!(s.try_set_float(ParamId(9), 1.0), Err(not(ParamId(9), "float")));
    assert_eq!(s.try_set_bool(grounded, false), Ok(()));
    assert_eq!(s.try_set_bool(jump, true), Err(not(jump, "bool")));
    assert_eq!(s.try_set_trigger(jump, true), Ok(()));
    assert_eq!(s.try_set_trigger(speed, true), Err(not(speed, "trigger")));
    assert!(matches!(s.parameter(jump), Some(AnimationParameter::Trigger(true))));

    assert_eq!(
        s.try_crossfade_state(StateId(2), 0.1, 0.0),
        Err(E::UnknownState(StateId(2)))
    );
    assert_eq!(
        s.try_crossfade_state(StateId(1), -0.1, 0.0),
        Err(E::InvalidPlayback(StateId(1)))
    );
    assert_eq!(
        s.try_play_state(StateId(1), f32::INFINITY),
        Err(E::InvalidPlayback(StateId(1)))
    );
    s.update(0.0);
    assert_eq!(s.state(), Some(StateId(0)), "nothing was queued");
    assert_eq!(s.try_play_state(StateId(1), 0.5), Ok(()));
    s.update(0.0);
    assert_eq!(s.state(), Some(StateId(1)));
    near(s.normalized_time(), 0.5);
    assert!(!s.looping());

    // Without a graph there are no states.
    let mut plain = Skeleton::new(Arc::new(arm()));
    assert_eq!(
        plain.try_crossfade_state(StateId(0), 0.0, 0.0),
        Err(E::UnknownState(StateId(0)))
    );
    assert_eq!(plain.try_set_bool(ParamId(0), true), Err(not(ParamId(0), "bool")));
}

#[test]
#[should_panic(expected = "parameter 1 is not a float parameter")]
fn setting_a_parameter_as_the_wrong_kind_panics_with_the_reason() {
    let mut data = arm();
    let graph = blend_graph(&mut data);
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    s.set_float(ParamId(1), 1.0);
}

#[test]
fn graphs_refuse_what_the_3d_graph_refuses() {
    use ponko::anim::AnimationGraphError;
    let state = || SkeletonState::new("a", SkeletonMotion::Clip(0));
    assert_eq!(
        SkeletonGraph::new(vec![], vec![state()], vec![], StateId(1)).unwrap_err(),
        AnimationGraphError::InvalidInitialState(StateId(1))
    );
    let blend = SkeletonState::new(
        "b",
        SkeletonMotion::Blend1D {
            parameter: ParamId(0),
            children: vec![(1.0, 0), (1.0, 0)],
        },
    );
    assert_eq!(
        SkeletonGraph::new(
            vec![("x", AnimationParameter::Float(0.0))],
            vec![blend],
            vec![],
            StateId(0)
        )
        .unwrap_err(),
        AnimationGraphError::InvalidThresholds(StateId(0))
    );
    let wrong_kind = AnimationTransition::from_any(StateId(0)).with_condition(AnimationCondition::Trigger(ParamId(0)));
    assert!(matches!(
        SkeletonGraph::new(
            vec![("x", AnimationParameter::Float(0.0))],
            vec![state()],
            vec![wrong_kind],
            StateId(0)
        ),
        Err(AnimationGraphError::WrongParameterKind { .. })
    ));
    // A state playing a clip the skeleton doesn't have.
    let mut data = arm();
    let graph = SkeletonGraph::new(vec![], vec![state()], vec![], StateId(0)).unwrap();
    assert!(data.set_graph(graph).is_err());
}

#[test]
fn events_fire_once_per_crossing_including_loop_wraps() {
    let mut data = arm();
    data.add_clip(
        Clip::new("step", 1.0)
            .with_event(0.25, "left")
            .with_event(0.75, "right"),
    );
    let mut s = Skeleton::new(Arc::new(data));
    s.play("step");
    s.update(0.5);
    assert_eq!(s.drain_events().map(|e| e.to_string()).collect::<Vec<_>>(), ["left"]);
    s.update(0.0);
    assert_eq!(s.drain_events().count(), 0, "no time, no events");
    s.update(1.0);
    assert_eq!(
        s.drain_events().map(|e| e.to_string()).collect::<Vec<_>>(),
        ["right", "left"]
    );
}

#[test]
fn slot_tracks_swap_attachments_hide_slots_and_reorder_drawing() {
    let mut data = arm();
    let tex = pixel();
    data.add_attachment(0, "fist", SpritePart::new(tex));
    let mut slot = SlotTrack::new(0);
    slot.attachment = vec![Key::new(0.5, Some("fist".into()))];
    let mut hand = SlotTrack::new(1);
    hand.visible = vec![Key::new(0.0, true), Key::new(0.5, false)];
    let mut clip = Clip::new("punch", 1.0).with_slot_track(slot).with_slot_track(hand);
    clip.draw_order = vec![Key::new(0.5, Some(Arc::from([1u16, 0].as_slice())))];
    data.add_clip(clip);
    let mut s = Skeleton::new(Arc::new(data));
    assert_eq!(s.attachment(0), Some("arm"), "the first attachment is the rest one");
    s.play_once("punch");
    s.update(0.25);
    assert_eq!((s.attachment(0), s.attachment(1)), (Some("arm"), Some("hand")));
    assert_eq!(s.draw_order(), [0, 1]);
    s.update(0.5);
    assert_eq!((s.attachment(0), s.attachment(1)), (Some("fist"), None));
    assert_eq!(s.draw_order(), [1, 0]);
    // The game's choice wins over keys until it lets go.
    s.set_attachment(0, Some(Some("arm")));
    s.update(0.0);
    assert_eq!(s.attachment(0), Some("arm"));
    s.set_attachment(0, None);
    assert_eq!(s.attachment(0), Some("fist"));
}

#[test]
fn skins_replace_default_attachments_by_name() {
    let mut data = arm();
    let red = data.add_skin("red");
    let tex = pixel();
    data.add_skin_attachment(red, 0, "arm", SpritePart::new(tex).with_size(vec2(9.0, 9.0)));
    assert!(
        matches!(data.attachment(red, 0, "arm"), Some(ponko::skeleton2d::Attachment::Sprite(p)) if p.size == vec2(9.0, 9.0))
    );
    assert!(
        data.attachment(red, 1, "hand").is_some(),
        "falls back to the default skin"
    );
    let mut s = Skeleton::new(Arc::new(data));
    assert!(s.set_skin("red"));
    assert_eq!(s.skin(), "red");
    assert!(!s.set_skin("blue"));
}

#[test]
fn two_bone_ik_reaches_targets_in_range_and_bends_the_chosen_way() {
    let data = Arc::new(arm());
    for (target, positive) in [
        (vec2(30.0, 10.0), true),
        (vec2(30.0, 10.0), false),
        (vec2(10.0, 25.0), true),
    ] {
        let mut s = Skeleton::new(data.clone());
        s.two_bone_ik(1, 2, target, positive, 1.0);
        near2(s.bone_point(2, vec2(20.0, 0.0)), target);
        // Which side of the line from the shoulder to the target the elbow is on.
        let shoulder = s.bone_position(1);
        let elbow = s.bone_position(2);
        let side = (target - shoulder).perp_dot(elbow - shoulder);
        assert_eq!(side < 0.0, positive, "target {target}, bend_positive {positive}");
    }
}

#[test]
fn two_bone_ik_points_straight_at_targets_out_of_reach_and_mix_zero_does_nothing() {
    let mut s = Skeleton::new(Arc::new(arm()));
    s.two_bone_ik(1, 2, vec2(10.0, 100.0), true, 1.0);
    near2(s.bone_point(2, vec2(20.0, 0.0)), vec2(10.0, 40.0));
    let mut still = Skeleton::new(Arc::new(arm()));
    still.two_bone_ik(1, 2, vec2(10.0, 100.0), true, 0.0);
    near(still.bone_rotation(1), 0.0);
}

#[test]
#[should_panic(expected = "child of the first")]
fn two_bone_ik_needs_a_parent_and_child() {
    let mut s = Skeleton::new(Arc::new(arm()));
    s.two_bone_ik(2, 1, Vec2::ZERO, true, 1.0);
}

#[test]
fn look_at_turns_a_bone_toward_its_target_by_its_mix() {
    let mut s = Skeleton::new(Arc::new(arm()));
    s.look_at(1, vec2(10.0, -50.0), 1.0);
    near(s.bone_rotation(1), -FRAC_PI_2);
    let mut half = Skeleton::new(Arc::new(arm()));
    half.look_at(1, vec2(10.0, 50.0), 0.5);
    near(half.bone_rotation(1), FRAC_PI_2 / 2.0);
}

#[test]
fn ik_constraints_follow_their_target_bone_or_the_games_target() {
    let mut data = arm();
    let goal = data.add_bone("goal", Some(0), BonePose::at(vec2(25.0, 20.0)), 0.0);
    data.add_ik(IkConstraint::new("reach", &[1, 2], goal));
    let mut s = Skeleton::new(Arc::new(data));
    s.update(0.0);
    near2(s.bone_point(2, vec2(20.0, 0.0)), vec2(25.0, 20.0));
    s.set_ik_target(0, Some(vec2(10.0, -30.0)));
    s.update(0.0);
    near2(s.bone_point(2, vec2(20.0, 0.0)), vec2(10.0, -30.0));
    s.set_ik_mix(0, 0.0);
    s.update(0.0);
    near(s.bone_rotation(1), 0.0);
}

#[test]
#[should_panic(expected = "named \"arm\" already")]
fn bone_names_are_unique() {
    let mut data = arm();
    data.add_bone("arm", None, BonePose::IDENTITY, 0.0);
}

#[test]
#[should_panic(expected = "at least one bone")]
fn mesh_vertices_need_a_weighted_bone() {
    let _ = MeshVertex::new(Vec2::ZERO, Vec2::ZERO, &[(0, 0.0)]);
}

#[test]
fn mesh_vertices_keep_their_four_strongest_bones() {
    let v = MeshVertex::new(
        Vec2::ZERO,
        Vec2::ZERO,
        &[(0, 1.0), (1, 2.0), (2, 3.0), (3, 4.0), (4, 10.0)],
    );
    assert_eq!(v.bones, [4, 3, 2, 1]);
    near(v.weights.iter().sum(), 1.0);
}

const FILE: &str = r##"{
  "format": "ponko-skeleton2d", "version": 1,
  "bones": [
    { "name": "root" },
    { "name": "thigh", "parent": "root", "y": -10, "rotation": 90, "length": 10 },
    { "name": "shin", "parent": "thigh", "x": 10, "length": 10 },
    { "name": "foot-goal", "parent": "root", "x": 5, "y": 8 }
  ],
  "slots": [
    { "name": "leg", "bone": "thigh", "attachment": "leg" },
    { "name": "body", "bone": "root", "attachment": "body", "color": "#ff000080" }
  ],
  "skins": [
    { "name": "default", "attachments": [
      { "slot": "body", "name": "body", "image": "body.png", "size": [8, 8] },
      { "slot": "leg", "name": "leg", "type": "mesh", "image": "leg.png",
        "vertices": [
          { "position": [-1, -10], "uv": [0, 0], "bones": ["thigh"] },
          { "position": [1, -10], "uv": [1, 0], "bones": ["thigh"] },
          { "position": [0, 10], "uv": [0.5, 1], "bones": ["thigh", "shin"], "weights": [1, 3] }
        ],
        "triangles": [0, 1, 2] }
    ] },
    { "name": "gold", "attachments": [
      { "slot": "body", "name": "body", "image": "gold.png", "region": [0, 0, 2, 2] }
    ] }
  ],
  "ik": [{ "name": "leg", "bones": ["thigh", "shin"], "target": "foot-goal", "mix": 0 }],
  "clips": [
    { "name": "idle", "duration": 1, "events": [{ "time": 0.5, "name": "blink" }] },
    { "name": "walk", "bones": [{ "bone": "thigh", "rotate": [
        { "time": 0, "value": 0 }, { "time": 0.5, "value": 30, "curve": "stepped" }, { "time": 1, "value": 0 } ] }],
      "slots": [{ "slot": "body", "visible": [{ "time": 0.5, "value": false }] }],
      "draw_order": [{ "time": 0.5, "order": ["body", "leg"] }] }
  ],
  "graph": {
    "parameters": [{ "name": "speed", "float": 0 }, { "name": "hit", "trigger": false }],
    "states": [
      { "name": "move", "blend1d": { "parameter": "speed", "clips": [[0, "idle"], [1, "walk"]] } },
      { "name": "hurt", "clip": "idle", "loop": false }
    ],
    "transitions": [{ "to": "hurt", "when": [{ "trigger": "hit" }], "duration": 0.1 }],
    "initial": "move"
  }
}"##;

#[test]
fn the_json_format_reads_bones_slots_skins_meshes_ik_clips_and_the_graph() {
    let data = SkeletonData::from_json(FILE, "hero/hero.json").unwrap();
    assert_eq!(data.bones().len(), 4);
    let thigh = &data.bones()[1];
    near(thigh.rest.rotation, FRAC_PI_2);
    assert_eq!(thigh.rest.translation, vec2(0.0, -10.0));
    assert_eq!(data.slots()[1].color, Color::hex(0xff0000).with_alpha(128.0 / 255.0));
    assert_eq!(data.skins().len(), 2);
    assert_eq!(data.images(), ["hero/body.png", "hero/leg.png", "hero/gold.png"]);
    let Some(ponko::skeleton2d::Attachment::Mesh(leg)) = data.attachment(0, 0, "leg") else {
        panic!("a mesh");
    };
    assert_eq!(leg.vertices[2].weights, [0.75, 0.25, 0.0, 0.0]);
    assert_eq!(data.ik_constraints()[0].bones, [1, 2]);
    let walk = &data.clips()[data.clip("walk").unwrap()];
    assert_eq!(walk.duration, 1.0, "the last key");
    near(walk.bones[0].rotate[1].value, 30f32.to_radians());
    assert_eq!(walk.bones[0].rotate[1].curve, Curve::Stepped);
    assert_eq!(data.clips()[0].events[0].time, 0.5);
    let graph = data.graph().unwrap();
    assert_eq!(graph.state("hurt"), Some(StateId(1)));
    assert_eq!(graph.parameter("hit"), Some(ParamId(1)));
}

#[test]
fn bad_skeleton_files_are_errors_that_say_what_is_wrong() {
    let base: serde_json::Value = serde_json::from_str(FILE).unwrap();
    type Patch = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<(&str, Patch)> = vec![
        ("version", Box::new(|v| v["version"] = 2.into())),
        ("named \"hip\"", Box::new(|v| v["bones"][1]["parent"] = "hip".into())),
        ("isn't a color", Box::new(|v| v["slots"][1]["color"] = "red".into())),
        (
            "exactly one of",
            Box::new(|v| v["graph"]["states"][1]["blend1d"] = serde_json::json!({ "parameter": "speed", "clips": [] })),
        ),
        (
            "no clip named",
            Box::new(|v| v["graph"]["states"][1]["clip"] = "fly".into()),
        ),
        ("unknown field", Box::new(|v| v["bones"][0]["skew"] = 1.into())),
        (
            "child of its first",
            Box::new(|v| v["ik"][0]["bones"] = serde_json::json!(["shin", "thigh"])),
        ),
        (
            "triangles",
            Box::new(|v| v["skins"][0]["attachments"][1]["triangles"] = serde_json::json!([0, 1, 7])),
        ),
        (
            "every slot once",
            Box::new(|v| v["clips"][1]["draw_order"][0]["order"] = serde_json::json!(["body"])),
        ),
    ];
    for (message, patch) in cases {
        let mut file = base.clone();
        patch(&mut file);
        let err = SkeletonData::from_json(&file.to_string(), "x.json").unwrap_err();
        assert!(err.to_string().contains(message), "{message}: {err}");
    }
    assert!(SkeletonData::from_json("not json", "x.json").is_err());
}

const SPINE: &str = r#"{
  "skeleton": { "spine": "4.2.33", "images": "./images/" },
  "bones": [
    { "name": "root" },
    { "name": "torso", "parent": "root", "y": 20, "rotation": 90, "length": 30 },
    { "name": "head", "parent": "torso", "x": 30, "length": 10 }
  ],
  "slots": [
    { "name": "torso", "bone": "torso", "attachment": "torso" },
    { "name": "head", "bone": "head", "attachment": "head", "color": "ffffff80" }
  ],
  "ik": [{ "name": "aim", "bones": ["head"], "target": "root", "mix": 0 }],
  "skins": [{ "name": "default", "attachments": {
    "torso": { "torso": { "type": "mesh", "uvs": [0, 0, 1, 0, 0, 1],
      "triangles": [0, 1, 2],
      "vertices": [1, 1, 0, 0, 1,  2, 1, 30, 0, 0.5, 2, 0, 0, 0.5,  1, 2, 5, 0, 1],
      "hull": 3 } },
    "head": { "head": { "x": 5, "y": 1, "rotation": -90, "width": 12, "height": 10 },
              "head-hurt": { "path": "faces/hurt", "width": 12, "height": 10 } }
  } }],
  "events": { "step": {} },
  "animations": {
    "nod": {
      "bones": { "head": {
        "rotate": [ { "value": 10 }, { "time": 0.5, "value": -10, "curve": [0.625, -10, 0.875, 10] }, { "time": 1, "value": 10, "curve": "stepped" } ],
        "translate": [ { "time": 0, "x": 2, "y": 3 } ] } },
      "slots": { "head": { "attachment": [ { "time": 0.5, "name": "head-hurt" } ] } },
      "drawOrder": [ { "time": 0.25, "offsets": [ { "slot": "torso", "offset": 1 } ] } ],
      "events": [ { "time": 0.75, "name": "step" } ]
    }
  }
}"#;

#[test]
fn spine_exports_import_with_y_mirrored_and_angles_reversed() {
    let data = SkeletonData::from_spine_json(SPINE, "spine/hero.json").unwrap();
    let torso = &data.bones()[1];
    assert_eq!(torso.rest.translation, vec2(0.0, -20.0));
    near(torso.rest.rotation, -FRAC_PI_2);
    let mut images = data.images().to_vec();
    images.sort();
    assert_eq!(
        images,
        [
            "spine/images/faces/hurt.png",
            "spine/images/head.png",
            "spine/images/torso.png"
        ]
    );
    assert_eq!(data.slots()[1].color.a, 128.0 / 255.0);
    // The weighted mesh: vertex 1 is half on the torso, half on the head,
    // both at their own (30, 0) and (0, 0): the head's origin.
    let Some(ponko::skeleton2d::Attachment::Mesh(mesh)) = data.attachment(0, 0, "torso") else {
        panic!("a mesh");
    };
    let head_origin = vec2(0.0, -50.0);
    near2(mesh.vertices[1].position, head_origin);
    assert_eq!(mesh.vertices[1].weights[..2], [0.5, 0.5]);
    let Some(ponko::skeleton2d::Attachment::Sprite(head)) = data.attachment(0, 1, "head") else {
        panic!("a region");
    };
    assert_eq!(head.size, vec2(12.0, 10.0));
    assert_eq!(head.offset.translation, vec2(5.0, -1.0));
    near(head.offset.rotation, FRAC_PI_2);
    assert!(
        !data.ik_constraints()[0].bend_positive,
        "Spine's positive bend is our negative one"
    );
    let nod = &data.clips()[0];
    assert_eq!(nod.duration, 1.0);
    near(nod.bones[0].rotate[0].value, -10f32.to_radians());
    assert_eq!(
        nod.bones[0].rotate[1].curve,
        Curve::Bezier(ponko::skeleton2d::Bezier::new(0.25, 0.0, 0.75, 1.0)),
        "4.x handles in seconds and degrees, normalized"
    );
    assert_eq!(nod.bones[0].rotate[2].curve, Curve::Stepped);
    assert_eq!(nod.bones[0].translate[0].value, vec2(2.0, -3.0));
    assert_eq!(&**nod.draw_order[0].value.as_ref().unwrap(), &[1, 0]);
    near(nod.events[0].time, 0.75);
    let mut s = Skeleton::new(Arc::new(data));
    s.play("nod");
    s.update(0.6);
    assert_eq!(s.attachment(1), Some("head-hurt"));
}

fn png(rgba: [u8; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn source() -> ponko::assets::MemorySource {
    ponko::assets::MemorySource::new("skeletons")
        .with("hero/hero.json", FILE)
        .with("hero/body.png", png([255; 4]))
        .with("hero/leg.png", png([255; 4]))
        .with("hero/gold.png", png([255; 4]))
}

#[test]
fn skeleton_files_load_through_the_asset_system_once_with_their_images() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(source());
        let data = ctx.load_skeleton2d("hero/hero.json");
        assert_eq!(data.name(), Some("hero/hero.json"));
        assert!(
            Arc::ptr_eq(&data, &ctx.load_skeleton2d("hero/hero.json")),
            "loaded once"
        );
        let body = data.attachment(0, 1, "body").unwrap().texture();
        assert_eq!((body.width(), body.height()), (2, 2), "its image is loaded");
        let err = ctx.try_load_skeleton2d("missing.json").unwrap_err();
        assert_eq!(err.kind(), ponko::error::ErrorKind::NotFound);
    });
}

#[test]
fn skeleton_images_that_are_missing_are_load_errors() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets
            .add_source(ponko::assets::MemorySource::new("s").with("hero/hero.json", FILE));
        let err = ctx.try_load_skeleton2d("hero/hero.json").unwrap_err();
        assert!(err.to_string().contains("body.png"), "{err}");
    });
}

#[test]
fn skeletons_round_trip_through_reflection_with_their_clip_clock_parameters_and_skin() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(source());
        let data = ctx.load_skeleton2d("hero/hero.json");
        let mut s = Skeleton::new(data);
        let speed = s.parameter_id("speed").unwrap();
        s.set_float(speed, 0.75);
        s.set_skin("gold");
        s.z = 3.0;
        s.update(0.25);
        let e = ctx.world.spawn((s,));
        let json = ctx.world.get_json(e, "Skeleton2D").unwrap();
        assert_eq!(json["skeleton"], "hero/hero.json");
        assert_eq!(json["state"], "move");
        assert_eq!(json["skin"], "gold");
        assert_eq!(json["parameters"]["speed"], 0.75);
        let copy = ctx.world.spawn_json(serde_json::json!({ "Skeleton2D": json })).unwrap();
        let (a, b) = (
            ctx.world.get::<Skeleton>(e).unwrap(),
            ctx.world.get::<Skeleton>(copy).unwrap(),
        );
        assert_eq!(a.state(), b.state());
        assert_eq!(a.normalized_time(), b.normalized_time());
        assert_eq!(a.skin(), b.skin());
        assert_eq!(a.z, b.z);
        assert_eq!(a.bone_pose(1), b.bone_pose(1));
        // A file needs only the skeleton.
        let rest = ctx
            .world
            .spawn_json(serde_json::json!({ "Skeleton2D": { "skeleton": "hero/hero.json" } }))
            .unwrap();
        assert_eq!(ctx.world.get::<Skeleton>(rest).unwrap().state(), Some(StateId(0)));
        for bad in [
            serde_json::json!({ "skeleton": "nope.json" }),
            serde_json::json!({ "skeleton": "hero/hero.json", "clip": "fly" }),
            serde_json::json!({ "skeleton": "hero/hero.json", "skin": "silver" }),
            serde_json::json!({ "skeleton": "hero/hero.json", "parameters": { "speed": true } }),
            serde_json::json!({ "skeleton": "hero/hero.json", "clip": "idle", "state": "move" }),
        ] {
            assert!(
                ctx.world
                    .spawn_json(serde_json::json!({ "Skeleton2D": bad.clone() }))
                    .is_err(),
                "{bad}"
            );
        }
    });
}

#[test]
fn saves_and_scenes_keep_an_animated_skeleton() {
    let saved = std::cell::RefCell::new(serde_json::Value::Null);
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ctx.assets.add_source(source());
        let mut s = Skeleton::new(ctx.load_skeleton2d("hero/hero.json"));
        s.play("walk");
        s.update(0.6);
        ctx.world.spawn((ponko::d2::Transform::at(vec2(5.0, 6.0)), s));
        *saved.borrow_mut() = ctx.world.save();
    });
    let save = saved.into_inner();
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        // Loading the scene loads the skeleton file it names first.
        ctx.assets.add_source(source().with("save.json", save.to_string()));
        ctx.load_scene("save.json").unwrap();
        let s = ctx.world.query::<&Skeleton>().iter().next().cloned().expect("loaded");
        assert_eq!(s.clip(), Some("walk"));
        near(s.normalized_time(), 0.6);
        assert_eq!(s.attachment(1), None, "the walk hides the body after 0.5 s");
    });
}

#[test]
fn skeletons_built_in_code_are_not_saved_but_the_entity_is() {
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let e = ctx.world.spawn((Skeleton::new(Arc::new(arm())),));
        assert!(ctx.world.get_json(e, "Skeleton2D").is_err());
    });
}

#[test]
fn the_built_in_update_advances_skeletons_and_skips_manual_ones() {
    let mut data = arm();
    data.add_clip(Clip::new("swing", 1.0).with_bone_track(rotate(1, vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0)])));
    let data = Arc::new(data);
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        let mut auto = Skeleton::new(data.clone());
        auto.play("swing");
        let mut manual = auto.clone();
        manual.manual = true;
        let a = ctx.world.spawn((auto,));
        let m = ctx.world.spawn((manual,));
        let dt = ctx.time.dt();
        ponko::systems::update_skeletons2d(ctx);
        near(ctx.world.get::<Skeleton>(a).unwrap().normalized_time(), dt);
        near(ctx.world.get::<Skeleton>(m).unwrap().normalized_time(), 0.0);
    });
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[test]
fn editing_a_skeleton_file_reloads_every_skeleton_playing_it() {
    let dir = std::env::temp_dir().join(format!("ponko-skeleton2d-hot-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("rig.json");
    let text = |x: f32| {
        format!(
            r#"{{ "format": "ponko-skeleton2d", "version": 1,
                 "bones": [{{ "name": "root" }}, {{ "name": "arm", "parent": "root", "x": {x} }}],
                 "clips": [{{ "name": "idle", "duration": 1 }}] }}"#
        )
    };
    write(&file, &text(1.0));
    let name = file.to_str().unwrap().to_string();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let edited = file.clone();
    Game::new("hot", 1, 1)
        .hot_reload(true)
        .sim_system(move |ctx: &mut Ctx| {
            ponko::systems::update_skeletons2d(ctx);
            for s in ctx.world.query::<&Skeleton>().iter() {
                log.lock()
                    .unwrap()
                    .push((s.bone_position(1).x, s.clip().map(str::to_string)));
            }
            if ctx.time.sim_tick() == 2 {
                std::thread::sleep(Duration::from_millis(50));
                write(&edited, &text(7.0));
                std::thread::sleep(Duration::from_millis(400));
            }
        })
        .run_headless(5, move |ctx| {
            let data = ctx.load_skeleton2d(&name);
            let mut s = Skeleton::new(data);
            s.play("idle");
            ctx.world.spawn((s,));
        });
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.first(), Some(&(1.0, Some("idle".to_string()))));
    assert_eq!(seen.last(), Some(&(7.0, Some("idle".to_string()))), "{seen:?}");
}

#[test]
fn rest_poses_need_no_update_and_cost_nothing_without_skeletons() {
    // A world with no skeleton: the built-in update and drawing are no-ops.
    Game::new("t", 1, 1).run_headless(0, |ctx| {
        ponko::systems::update_skeletons2d(ctx);
        assert_eq!(ctx.gfx.sprite_count(), 0);
    });
    let s = Skeleton::new(Arc::new(arm()));
    near2(s.bone_position(2), vec2(30.0, 0.0));
}

#[test]
fn skinned_meshes_validate_their_triangles() {
    let tex = pixel();
    let v = MeshVertex::new(Vec2::ZERO, Vec2::ZERO, &[(0, 1.0)]);
    let ok = SkinnedMesh::new(tex, vec![v; 3], vec![0, 1, 2]);
    assert_eq!(ok.triangles.len(), 3);
    let bad = std::panic::catch_unwind(|| SkinnedMesh::new(tex, vec![v; 3], vec![0, 1, 3]));
    assert!(bad.is_err());
}

#[test]
#[should_panic(expected = "isn't a mesh or clipping mask there")]
fn deform_tracks_need_a_mesh_to_move() {
    use ponko::skeleton2d::DeformTrack;
    let mut data = arm();
    let mut bend = DeformTrack::new(0, 0, "arm");
    bend.keys = vec![Key::new(0.0, Arc::from([Vec2::ZERO; 4]))];
    data.add_clip(Clip::new("bend", 1.0).with_deform_track(bend));
}

#[test]
#[should_panic(expected = "needs 8 offsets (four per vertex)")]
fn deform_keys_carry_four_offsets_per_vertex() {
    use ponko::skeleton2d::{ClippingMask, DeformTrack};
    let mut data = arm();
    let corner = |p: Vec2| MeshVertex::new(p, Vec2::ZERO, &[(1, 1.0)]);
    data.add_attachment(
        0,
        "mask",
        ClippingMask::new(vec![corner(Vec2::ZERO), corner(Vec2::X)], None),
    );
    let mut bend = DeformTrack::new(0, 0, "mask");
    bend.keys = vec![Key::new(0.0, Arc::from([Vec2::ZERO; 2]))];
    data.add_clip(Clip::new("bend", 1.0).with_deform_track(bend));
}

#[test]
#[should_panic(expected = "ends at a slot that isn't in the skeleton")]
fn clipping_masks_end_at_a_slot_of_the_skeleton() {
    use ponko::skeleton2d::ClippingMask;
    let mut data = arm();
    let corner = |p: Vec2| MeshVertex::new(p, Vec2::ZERO, &[(1, 1.0)]);
    data.add_attachment(
        0,
        "mask",
        ClippingMask::new(vec![corner(Vec2::ZERO), corner(Vec2::X), corner(Vec2::Y)], Some(5)),
    );
}

#[test]
#[should_panic(expected = "can't move its own target")]
fn transform_constraints_cant_follow_their_own_bone() {
    use ponko::skeleton2d::TransformConstraint;
    let mut data = arm();
    data.add_transform_constraint(TransformConstraint::new("self", &[1], 1));
}

#[test]
fn clip_ids_play_like_names_and_a_typo_lists_the_clips() {
    let mut data = arm();
    let keys = vec![Key::new(0.0, 0.0), Key::new(1.0, 1.0)];
    data.add_clip(Clip::new("idle", 1.0));
    data.add_clip(Clip::new("swing", 1.0).with_bone_track(rotate(1, keys)));
    let data = Arc::new(data);
    let mut s = Skeleton::new(data.clone());
    let swing = s.clip_id("swing").unwrap();
    assert_eq!(swing, data.clip_id("swing").unwrap());
    assert_eq!(swing, ponko::skeleton2d::ClipId(1));
    assert!(s.play(swing));
    s.update(0.25);
    near(s.bone_pose(1).rotation, 0.25);
    assert_eq!(s.clip(), Some("swing"));
    assert!(s.play_once(swing) && s.crossfade(swing, 0.1));
    assert!(
        !s.play(ponko::skeleton2d::ClipId(2)),
        "an index past the clips changes nothing"
    );
    let name = String::from("idle");
    assert!(s.play(&name), "a String's name plays as well");
    let err = s.clip_id("swnig").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    assert_eq!(err.message(), r#"unknown animation clip "swnig" (known: idle, swing)"#);
}

#[test]
fn checked_parameter_and_state_lookups_list_the_names_there_are() {
    let mut data = arm();
    let graph = blend_graph(&mut data);
    assert_eq!(graph.try_state("move").unwrap(), StateId(0));
    assert_eq!(graph.try_parameter("speed").unwrap(), ParamId(0));
    let err = graph.try_state("mvoe").unwrap_err();
    assert_eq!(err.message(), r#"unknown animation state "mvoe" (known: move)"#);
    data.set_graph(graph).unwrap();
    let s = Skeleton::new(Arc::new(data));
    assert_eq!(s.try_parameter_id("speed").unwrap(), ParamId(0));
    let err = s.try_parameter_id("sped").unwrap_err();
    assert_eq!(err.kind(), ErrorKind::NotFound);
    assert_eq!(err.message(), r#"unknown animation parameter "sped" (known: speed)"#);
}

#[test]
fn a_rig_built_in_code_sets_its_rest_slots_and_shears() {
    let mut data = arm();
    // The hand shows nothing at rest until an animation keys it on.
    let hand = data.slot("hand").unwrap();
    data.set_slot_rest(hand, None, Color::WHITE);
    // A cape whose +y axis leans an eighth of a turn; +x stays along the bone.
    let lean = BonePose::at(vec2(0.0, 5.0)).with_shear(vec2(0.0, std::f32::consts::FRAC_PI_4));
    let cape = data.add_bone("cape", Some(0), lean, 10.0);
    data.add_clip(Clip::new("still", 1.0));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("still");
    s.update(0.0);
    assert_eq!(s.attachment(hand), None);
    assert_eq!(s.attachment(s.data().slot("arm").unwrap()), Some("arm"));
    near2(s.bone_point(cape, vec2(1.0, 0.0)), vec2(1.0, 5.0));
    let up = s.bone_point(cape, vec2(0.0, 1.0)) - vec2(0.0, 5.0);
    near(up.length(), 1.0);
    near(up.x.abs(), std::f32::consts::FRAC_1_SQRT_2);
}

#[test]
fn a_graph_lists_its_parameters_and_states_and_can_be_switched_by_hand() {
    let mut data = arm();
    let idle = data.add_clip(Clip::new("idle", 1.0));
    let wave = data.add_clip(Clip::new("wave", 1.0).with_bone_track(rotate(1, vec![Key::new(0.0, 1.0)])));
    let graph = SkeletonGraph::new(
        vec![("happy", AnimationParameter::Bool(false))],
        vec![
            SkeletonState::new("idle", SkeletonMotion::Clip(idle)),
            SkeletonState::new("wave", SkeletonMotion::Clip(wave)),
        ],
        vec![],
        StateId(0),
    )
    .unwrap();
    // A debug panel names the parameters and the starting state.
    assert_eq!(
        graph.parameter_names().iter().map(|n| &**n).collect::<Vec<_>>(),
        ["happy"]
    );
    assert_eq!(graph.initial(), StateId(0));
    data.set_graph(graph).unwrap();
    let mut s = Skeleton::new(Arc::new(data));
    s.update(0.0);
    // A cutscene switches state directly, halfway into the wave.
    s.crossfade_state(StateId(1), 0.2, 0.5);
    s.update(0.2);
    assert_eq!(s.state(), Some(StateId(1)));
    assert_eq!(s.clip(), Some("wave"));
    near(s.bone_pose(1).rotation, 1.0);
}
