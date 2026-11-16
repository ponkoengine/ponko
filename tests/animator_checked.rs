//! Checked animator operations for ids that come from outside the code that
//! built the graph (save files, scripts, network, tools), and the builders
//! for the extensible animation structs.
#![cfg(feature = "3d")]

use std::sync::Arc;

use ponko::anim::{
    AnimationClip, AnimationEvent, AnimationGraph, AnimationGraphError, AnimationLayer, AnimationParameter,
    AnimationState, AnimationTrack, Animator, Interpolation, Joint, JointAnimation, LayerId, Motion, MotionChild,
    ParamId, Skeleton, StateId,
};
use ponko::prelude::*;

fn skeleton() -> Skeleton {
    Skeleton::new(vec![Joint::new("root", None)])
}

/// A one-second clip holding the root at `x`.
fn clip(s: &Skeleton, x: f32) -> AnimationClip {
    let at = Vec4::new(x, 0.0, 0.0, 0.0);
    AnimationClip::new(
        format!("x{x}"),
        s,
        vec![JointAnimation::new(0, "root").with_translation(AnimationTrack::new(
            vec![0.0, 1.0],
            vec![at, at],
            Interpolation::Step,
        ))],
    )
}

/// States at x = 0 and x = 1; a float, a bool and a trigger parameter.
fn graph(s: &Skeleton) -> Arc<AnimationGraph> {
    AnimationGraph::new(
        vec![
            AnimationParameter::Float(0.5),
            AnimationParameter::Bool(false),
            AnimationParameter::Trigger(false),
        ],
        vec![
            AnimationState::new(Motion::Clip(clip(s, 0.0))),
            AnimationState::new(Motion::Clip(clip(s, 1.0))),
        ],
        vec![],
        StateId(0),
    )
    .expect("a valid graph")
}

const FLOAT: ParamId = ParamId(0);
const BOOL: ParamId = ParamId(1);
const TRIGGER: ParamId = ParamId(2);
const MISSING: ParamId = ParamId(9);

fn wrong(parameter: ParamId, expected: &'static str) -> Result<(), AnimationGraphError> {
    Err(AnimationGraphError::WrongParameterKind { parameter, expected })
}

#[test]
fn checked_setters_change_parameters_of_the_right_kind() {
    let mut a = Animator::new(graph(&skeleton()));
    assert_eq!(a.try_set_float(FLOAT, 2.5), Ok(()));
    assert_eq!(a.try_set_bool(BOOL, true), Ok(()));
    assert_eq!(a.try_set_trigger(TRIGGER, true), Ok(()));
    assert!(matches!(a.parameter(FLOAT), Some(AnimationParameter::Float(v)) if v == 2.5));
    assert!(matches!(a.parameter(BOOL), Some(AnimationParameter::Bool(true))));
    assert!(matches!(a.parameter(TRIGGER), Some(AnimationParameter::Trigger(true))));
}

#[test]
fn checked_setters_refuse_unknown_ids_and_wrong_kinds_without_changing_anything() {
    let mut a = Animator::new(graph(&skeleton()));
    assert_eq!(a.try_set_float(MISSING, 1.0), wrong(MISSING, "float"));
    assert_eq!(a.try_set_float(BOOL, 1.0), wrong(BOOL, "float"));
    assert_eq!(a.try_set_bool(MISSING, true), wrong(MISSING, "bool"));
    assert_eq!(a.try_set_bool(TRIGGER, true), wrong(TRIGGER, "bool"));
    assert_eq!(a.try_set_trigger(MISSING, true), wrong(MISSING, "trigger"));
    assert_eq!(a.try_set_trigger(FLOAT, true), wrong(FLOAT, "trigger"));
    assert!(matches!(a.parameter(FLOAT), Some(AnimationParameter::Float(v)) if v == 0.5));
    assert!(matches!(a.parameter(BOOL), Some(AnimationParameter::Bool(false))));
    assert!(matches!(a.parameter(TRIGGER), Some(AnimationParameter::Trigger(false))));
}

#[test]
fn checked_float_setter_refuses_non_finite_values_and_keeps_the_old_one() {
    let mut a = Animator::new(graph(&skeleton()));
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            a.try_set_float(FLOAT, bad),
            Err(AnimationGraphError::NonFiniteValue(FLOAT))
        );
    }
    assert!(matches!(a.parameter(FLOAT), Some(AnimationParameter::Float(v)) if v == 0.5));
}

#[test]
fn checked_play_queues_a_valid_state_change() {
    let mut s = skeleton();
    let mut a = Animator::new(graph(&s));
    a.manual = true;
    assert_eq!(a.try_play(StateId(1), 0.25), Ok(()));
    a.update(0.0, &mut s);
    assert_eq!(a.state(), StateId(1));
    assert_eq!(a.normalized_time(), 0.25);
    assert_eq!(s.pose[0].position.x, 1.0);
}

#[test]
fn checked_play_refuses_bad_commands_and_queues_nothing() {
    let mut s = skeleton();
    let mut a = Animator::new(graph(&s));
    assert_eq!(
        a.try_play(StateId(2), 0.0),
        Err(AnimationGraphError::UnknownState(StateId(2)))
    );
    for bad in [-0.5, f32::NAN, f32::INFINITY] {
        assert_eq!(
            a.try_play(StateId(1), bad),
            Err(AnimationGraphError::InvalidPlayback(StateId(1)))
        );
    }
    a.update(0.0, &mut s);
    assert_eq!(a.state(), StateId(0), "a refused command must not be queued");
    assert_eq!(s.pose[0].position.x, 0.0);
}

#[test]
fn checked_crossfade_refuses_bad_durations_and_offsets() {
    let mut s = skeleton();
    let mut a = Animator::new(graph(&s));
    assert_eq!(
        a.try_crossfade(StateId(5), 0.1, 0.0),
        Err(AnimationGraphError::UnknownState(StateId(5)))
    );
    assert_eq!(
        a.try_crossfade(StateId(1), -0.1, 0.0),
        Err(AnimationGraphError::InvalidPlayback(StateId(1)))
    );
    assert_eq!(
        a.try_crossfade(StateId(1), 0.1, f32::NAN),
        Err(AnimationGraphError::InvalidPlayback(StateId(1)))
    );
    a.update(0.0, &mut s);
    assert_eq!(a.state(), StateId(0));
    assert_eq!(a.try_crossfade(StateId(1), 1.0, 0.0), Ok(()));
    a.update(0.5, &mut s);
    assert_eq!(a.state(), StateId(1));
    assert_eq!(a.fade_weight(), 0.5);
}

#[test]
fn get_layer_mut_finds_added_layers_and_none_otherwise() {
    let s = skeleton();
    let g = graph(&s);
    let mut a = Animator::new(g.clone());
    assert!(a.get_layer_mut(LayerId(0)).is_none());
    let id = a.add_layer(AnimationLayer::new(g));
    a.get_layer_mut(id).expect("just added").weight = 0.25;
    assert_eq!(a.layer_mut(id).weight, 0.25);
    assert!(a.get_layer_mut(LayerId(id.0 + 1)).is_none());
    let layer = a.get_layer_mut(id).unwrap();
    assert_eq!(layer.animator.try_set_float(MISSING, 1.0), wrong(MISSING, "float"));
}

#[test]
fn checked_errors_explain_themselves() {
    let messages = [
        AnimationGraphError::UnknownState(StateId(3)).to_string(),
        AnimationGraphError::NonFiniteValue(ParamId(1)).to_string(),
        AnimationGraphError::InvalidPlayback(StateId(2)).to_string(),
        wrong(ParamId(4), "bool").unwrap_err().to_string(),
    ];
    assert_eq!(messages[0], "no such animation state 3");
    assert!(messages[1].contains("parameter 1") && messages[1].contains("not finite"));
    assert!(messages[2].contains("state 2"));
    assert_eq!(messages[3], "parameter 4 is not a bool parameter");
}

#[test]
#[should_panic(expected = "Animator::set_float: parameter 9 is not a float parameter")]
fn fast_float_setter_panics_on_an_unknown_id() {
    Animator::new(graph(&skeleton())).set_float(MISSING, 1.0);
}

#[test]
#[should_panic(expected = "Animator::set_float: value for parameter 0 is not finite")]
fn fast_float_setter_panics_on_a_non_finite_value() {
    Animator::new(graph(&skeleton())).set_float(FLOAT, f32::NAN);
}

#[test]
#[should_panic(expected = "Animator::set_bool: parameter 0 is not a bool parameter")]
fn fast_bool_setter_panics_on_a_wrong_kind() {
    Animator::new(graph(&skeleton())).set_bool(FLOAT, true);
}

#[test]
#[should_panic(expected = "Animator::set_trigger: parameter 1 is not a trigger parameter")]
fn fast_trigger_setter_panics_on_a_wrong_kind() {
    Animator::new(graph(&skeleton())).set_trigger(BOOL, true);
}

#[test]
#[should_panic(expected = "Animator: invalid playback command: no such animation state 7")]
fn fast_play_panics_on_an_unknown_state() {
    Animator::new(graph(&skeleton())).play(StateId(7), 0.0);
}

#[test]
#[should_panic(expected = "Animator::layer_mut: no layer 0")]
fn fast_layer_access_panics_on_an_unknown_layer() {
    let _ = Animator::new(graph(&skeleton())).layer_mut(LayerId(0));
}

#[test]
fn state_builder_matches_the_documented_defaults_and_sets_each_field() {
    let s = skeleton();
    let plain = AnimationState::new(Motion::Clip(clip(&s, 0.0)));
    assert_eq!(
        (plain.speed, plain.speed_parameter, plain.looping, plain.events.len()),
        (1.0, None, true, 0)
    );
    let built = AnimationState::new(Motion::Clip(clip(&s, 0.0)))
        .with_speed(2.0)
        .with_speed_parameter(FLOAT)
        .with_looping(false)
        .with_events(vec![AnimationEvent::new(0.5, "mid")])
        .with_event(AnimationEvent::new(0.25, "early"));
    assert_eq!(
        (built.speed, built.speed_parameter, built.looping),
        (2.0, Some(FLOAT), false)
    );
    let names: Vec<&str> = built.events.iter().map(|e| &*e.name).collect();
    assert_eq!(names, ["mid", "early"]);
    assert_eq!(built.with_speed_parameter(None).speed_parameter, None);
}

#[test]
fn built_events_fire_in_time_order() {
    let mut s = skeleton();
    let state = AnimationState::new(Motion::Clip(clip(&s, 0.0)))
        .with_event(AnimationEvent::new(0.75, "late"))
        .with_event(AnimationEvent::new(0.25, "early"));
    let g = AnimationGraph::new(vec![], vec![state], vec![], StateId(0)).expect("a valid graph");
    let mut a = Animator::new(g);
    a.update(0.9, &mut s);
    let fired: Vec<String> = a.drain_events().map(|e| e.to_string()).collect();
    assert_eq!(fired, ["early", "late"]);
}

#[test]
fn built_non_looping_states_hold_their_last_pose() {
    let mut s = skeleton();
    let state = AnimationState::new(Motion::Clip(clip(&s, 1.0))).with_looping(false);
    let mut a = Animator::new(AnimationGraph::new(vec![], vec![state], vec![], StateId(0)).unwrap());
    a.update(5.0, &mut s);
    assert_eq!(a.normalized_time(), 1.0);
}

#[test]
fn motion_child_builder_scales_the_child_clock() {
    let s = skeleton();
    let child = MotionChild::new(Motion::Clip(clip(&s, 0.0)));
    assert_eq!(child.time_scale, 1.0);
    let fast = child.with_time_scale(2.0);
    let tree = Motion::blend_1d(FLOAT, vec![(0.0, fast)]);
    let g = AnimationGraph::new(
        vec![AnimationParameter::Float(0.0)],
        vec![AnimationState::new(tree)],
        vec![],
        StateId(0),
    )
    .expect("a valid graph");
    let mut sk = skeleton();
    let mut a = Animator::new(g);
    a.update(0.25, &mut sk);
    // A one-second clip at twice the speed lasts half a second.
    assert_eq!(a.normalized_time(), 0.5);
    let zero = MotionChild::new(Motion::Clip(clip(&s, 0.0))).with_time_scale(0.0);
    let bad = AnimationGraph::new(
        vec![AnimationParameter::Float(0.0)],
        vec![AnimationState::new(Motion::blend_1d(FLOAT, vec![(0.0, zero)]))],
        vec![],
        StateId(0),
    );
    assert_eq!(bad.unwrap_err(), AnimationGraphError::InvalidTimeScale(StateId(0)));
}

#[test]
fn event_builder_takes_any_string_like_name() {
    let owned = AnimationEvent::new(0.1, String::from("owned"));
    let shared: Arc<str> = "shared".into();
    let reused = AnimationEvent::new(0.2, shared.clone());
    assert_eq!(&*owned.name, "owned");
    assert!(Arc::ptr_eq(&reused.name, &shared), "an Arc name is kept, not copied");
}
