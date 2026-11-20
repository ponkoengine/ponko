//! 2D skeleton constraints beyond IK and transform: constraint mixes keyed
//! by clips (and mixed by crossfades), path constraints and physics
//! constraints, each checked against values worked out by hand; and the
//! engine's own file format holding everything a Spine import reads.
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use ponko::prelude::*;
use ponko::skeleton2d::{
    BonePose, BoneTrack, Clip, IkConstraint, IkTrack, Key, MeshVertex, PathConstraint, PathPosition, PathRotate,
    PathShape, PathSpacing, PathTrack, PhysicsConstraint, PhysicsTrack, Skeleton, SkeletonData, TransformConstraint,
    TransformTrack,
};

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} != {b}");
}

fn near2(a: Vec2, b: Vec2) {
    assert!(a.distance(b) < 1e-3, "{a} != {b}");
}

// ---- Constraint mixes keyed by clips -------------------------------------

/// A bone at the origin pointing +x that looks at a goal 10 below it (a
/// quarter turn, clockwise on screen) by a mix of 0.5; clip "on" keys the
/// mix to 1, "off" to 0, "plain" doesn't key it.
fn looker() -> Skeleton {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let head = data.add_bone("head", Some(root), BonePose::IDENTITY, 5.0);
    let goal = data.add_bone("goal", Some(root), BonePose::at(vec2(0.0, 10.0)), 0.0);
    data.add_ik(IkConstraint::new("look", &[head], goal).with_mix(0.5));
    let keyed = |mix: f32| {
        let mut t = IkTrack::new(0);
        t.mix = vec![Key::new(0.0, mix)];
        t
    };
    data.add_clip(Clip::new("on", 1.0).with_ik_track(keyed(1.0)));
    data.add_clip(Clip::new("off", 1.0).with_ik_track(keyed(0.0)));
    data.add_clip(Clip::new("plain", 1.0));
    Skeleton::new(Arc::new(data))
}

#[test]
fn ik_mix_keys_replace_the_constraints_mix_and_blend_in_crossfades() {
    let mut s = looker();
    let head = s.bone("head").unwrap();
    s.play("plain");
    s.update(0.0);
    near(s.bone_rotation(head), 0.5 * FRAC_PI_2);
    s.set_ik_mix(0, 0.2);
    s.update(0.0);
    near(s.bone_rotation(head), 0.2 * FRAC_PI_2);
    s.play("on");
    s.update(0.0);
    near(s.bone_rotation(head), FRAC_PI_2);
    s.crossfade("off", 1.0);
    s.update(0.25);
    // A quarter of the way from mix 1 to mix 0.
    near(s.bone_rotation(head), 0.75 * FRAC_PI_2);
    s.update(1.0);
    near(s.bone_rotation(head), 0.0);
    // To a clip that doesn't key it: toward the game's mix.
    s.crossfade("plain", 1.0);
    s.update(0.5);
    near(s.bone_rotation(head), 0.1 * FRAC_PI_2);
}

#[test]
fn ik_bend_keys_flip_the_joint() {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let upper = data.add_bone("upper", Some(root), BonePose::IDENTITY, 10.0);
    let lower = data.add_bone("lower", Some(upper), BonePose::at(vec2(10.0, 0.0)), 10.0);
    let goal = data.add_bone("goal", Some(root), BonePose::at(vec2(12.0, 0.0)), 0.0);
    data.add_ik(IkConstraint::new("reach", &[upper, lower], goal));
    let mut flip = IkTrack::new(0);
    flip.bend_positive = vec![Key::new(0.0, true), Key::new(0.5, false)];
    data.add_clip(Clip::new("flip", 1.0).with_ik_track(flip));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("flip");
    s.update(0.0);
    // Reaching 12 with two 10s: the elbow sits 8 off the line at x 6, on
    // one side, then on the other once the key says bend the other way.
    let elbow = s.bone_position(lower);
    near(elbow.x, 6.0);
    near(elbow.y.abs(), 8.0);
    s.update(0.6);
    near2(s.bone_position(lower), vec2(6.0, -elbow.y));
}

#[test]
fn transform_mix_keys_move_the_follower_part_way() {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let leader = data.add_bone("leader", Some(root), BonePose::at(vec2(10.0, 4.0)), 0.0);
    let follower = data.add_bone("follower", Some(root), BonePose::IDENTITY, 0.0);
    data.add_transform_constraint(TransformConstraint::new("follow", &[follower], leader));
    let mut slide = TransformTrack::new(0);
    slide.mix_translate = vec![Key::new(0.0, vec2(0.0, 0.0)), Key::new(1.0, vec2(1.0, 0.5))];
    data.add_clip(Clip::new("slide", 1.0).with_transform_track(slide));
    let mut s = Skeleton::new(Arc::new(data));
    near2(s.bone_position(follower), vec2(10.0, 4.0));
    s.play("slide");
    s.update(0.5);
    // x mix 0.5, y mix 0.25 of the way to (10, 4).
    near2(s.bone_position(follower), vec2(5.0, 1.0));
}

// ---- Path constraints ----------------------------------------------------

/// An L-shaped path on the root: right 30 from (0, 0), then down 30 (y
/// down) to (30, 30), each leg a straight cubic with its handles at the
/// thirds (so its parameter runs evenly along it); and a chain of three
/// bones 10 long, at rest along +x.
fn snake(c: impl FnOnce(&mut PathConstraint)) -> (Skeleton, [usize; 3]) {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let a = data.add_bone("a", Some(root), BonePose::IDENTITY, 10.0);
    let b = data.add_bone("b", Some(a), BonePose::at(vec2(10.0, 0.0)), 10.0);
    let d = data.add_bone("c", Some(b), BonePose::at(vec2(10.0, 0.0)), 10.0);
    let slot = data.add_slot("track", root);
    let on = |x: f32, y: f32| MeshVertex::new(vec2(x, y), Vec2::ZERO, &[(root, 1.0)]);
    let points = [
        (-10.0, 0.0),
        (0.0, 0.0),
        (10.0, 0.0),
        (20.0, 0.0),
        (30.0, 0.0),
        (30.0, 10.0),
        (30.0, 20.0),
        (30.0, 30.0),
        (30.0, 40.0),
    ];
    let path = PathShape::new(points.iter().map(|&(x, y)| on(x, y)).collect());
    data.add_attachment(slot, "track", path);
    let mut constraint = PathConstraint::new("follow", &[a, b, d], slot);
    constraint.position_mode = PathPosition::Fixed;
    c(&mut constraint);
    data.add_path_constraint(constraint);
    data.add_clip(Clip::new("idle", 1.0));
    let s = Skeleton::new(Arc::new(data));
    (s, [a, b, d])
}

#[test]
fn path_tangents_place_bones_by_distance_and_turn_them_along_it() {
    let (mut s, [a, b, c]) = snake(|p| p.position = 18.0);
    s.update(0.0);
    // By their lengths: 18, 28 and 38 along. The first two are on the
    // first leg; 38 is 8 down the second, which heads +y.
    near2(s.bone_position(a), vec2(18.0, 0.0));
    near2(s.bone_position(b), vec2(28.0, 0.0));
    near2(s.bone_position(c), vec2(30.0, 8.0));
    near(s.bone_rotation(a), 0.0);
    near(s.bone_rotation(b), 0.0);
    near(s.bone_rotation(c), FRAC_PI_2);
}

#[test]
fn path_chains_keep_their_lengths_and_point_at_the_next_place() {
    let (mut s, [a, b, c]) = snake(|p| {
        p.position = 18.0;
        p.rotate_mode = PathRotate::Chain;
    });
    s.update(0.0);
    // Places at 18, 28, 38 and 48 along: (18, 0), (28, 0), (30, 8), (30, 18).
    // Each bone starts where the one before ends and points at the next
    // place.
    near2(s.bone_position(a), vec2(18.0, 0.0));
    near(s.bone_rotation(a), 0.0);
    near2(s.bone_position(b), vec2(28.0, 0.0));
    let r1 = 8f32.atan2(2.0);
    near(s.bone_rotation(b), r1);
    let tip = vec2(28.0, 0.0) + Vec2::from_angle(r1) * 10.0;
    near2(s.bone_position(c), tip);
    let to = vec2(30.0, 18.0) - tip;
    near(s.bone_rotation(c), to.y.atan2(to.x));
}

#[test]
fn path_spacing_and_mixes_follow_their_modes_and_keys() {
    // Fixed spacing of 5 from 2: 2, 7 and 12, all on the first leg; x mix
    // 0.5 moves each half way from where its parent puts it.
    let (mut s, [a, b, c]) = snake(|p| {
        p.position = 2.0;
        p.spacing_mode = PathSpacing::Fixed;
        p.spacing = 5.0;
        p.mix_rotate = 0.0;
    });
    s.update(0.0);
    near2(s.bone_position(a), vec2(2.0, 0.0));
    near2(s.bone_position(b), vec2(7.0, 0.0));
    near2(s.bone_position(c), vec2(12.0, 0.0));
    // Percent position: half of the 60-long path is the corner.
    let (mut s, [a, ..]) = snake(|p| {
        p.position_mode = PathPosition::Percent;
        p.position = 0.25;
    });
    s.update(0.0);
    near2(s.bone_position(a), vec2(15.0, 0.0));
    // A clip keys the position: 0 to 20 over a second, so 10 half way.
    let (s, [a, ..]) = snake(|_| {});
    let mut data = (**s.data()).clone();
    let mut track = PathTrack::new(0);
    track.position = vec![Key::new(0.0, 0.0), Key::new(1.0, 20.0)];
    data.add_clip(Clip::new("slide", 1.0).with_path_track(track));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("slide");
    s.update(0.5);
    near2(s.bone_position(a), vec2(10.0, 0.0));
}

#[test]
fn path_constraints_import_from_spine_mirrored_to_y_down() {
    // Spine (y up): a path straight up from (0, 0) to (0, 30); a bone on
    // it at 12 points up the screen, which is -y here.
    let text = r#"{ "skeleton": { "spine": "4.2.0" },
        "bones": [{ "name": "root" }, { "name": "rider", "parent": "root", "length": 4 }],
        "slots": [{ "name": "track", "bone": "root", "attachment": "track" }],
        "skins": [{ "name": "default", "attachments": { "track": { "track": {
            "type": "path", "vertexCount": 6, "lengths": [30, 0],
            "vertices": [0, -10, 0, 0, 0, 10, 0, 20, 0, 30, 0, 40] } } } }],
        "path": [{ "name": "ride", "bones": ["rider"], "target": "track",
            "positionMode": "fixed", "position": 12 }] }"#;
    let data = SkeletonData::from_spine_json(text, "ride.json").unwrap();
    assert_eq!(data.warnings(), [] as [String; 0]);
    assert_eq!(data.path_constraints()[0].position, 12.0);
    let mut s = Skeleton::new(Arc::new(data));
    s.update(0.0);
    let rider = s.bone("rider").unwrap();
    near2(s.bone_position(rider), vec2(0.0, -12.0));
    near(s.bone_rotation(rider), -FRAC_PI_2);
}

// ---- Physics constraints -------------------------------------------------

/// A root that a clip moves 10 right after 0.01 s, with a child bone 10
/// long whose x lags by physics constraint `c`.
fn jumper(c: impl FnOnce(&mut PhysicsConstraint)) -> (Skeleton, usize) {
    let mut data = SkeletonData::new();
    let root = data.add_bone("root", None, BonePose::IDENTITY, 0.0);
    let tip = data.add_bone("tip", Some(root), BonePose::IDENTITY, 10.0);
    let mut physics = PhysicsConstraint::new("lag", tip);
    c(&mut physics);
    data.add_physics_constraint(physics);
    let mut jump = BoneTrack::new(root);
    jump.translate = vec![
        Key::new(0.0, vec2(0.0, 0.0)).with_curve(ponko::skeleton2d::Curve::Stepped),
        Key::new(0.01, vec2(10.0, 0.0)),
    ];
    let mut reset = PhysicsTrack::new(None);
    reset.reset = vec![0.5];
    data.add_clip(Clip::new("jump", 1.0).with_bone_track(jump).with_physics_track(reset));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("jump");
    s.update(0.0);
    (s, tip)
}

#[test]
fn a_bone_with_physics_lags_behind_a_jump_by_one_spring_step() {
    let (mut s, tip) = jumper(|p| p.x = 1.0);
    near2(s.bone_position(tip), Vec2::ZERO);
    s.update(1.0 / 60.0);
    // Inertia 1: the bone stays behind, offset -10. One step of 1/60 s:
    // velocity = (0 - (-10) * strength 100) * (1/60 / mass 1) = 16.667,
    // offset = -10 + 16.667 / 60 = -9.7222, so it shows at 0.2778.
    let velocity = 10.0 * 100.0 / 60.0;
    near(s.bone_position(tip).x, 10.0 - 10.0 + velocity / 60.0);
    // A reset key starts the springs over: the bone is where the pose is.
    s.update(0.5);
    near2(s.bone_position(tip), vec2(10.0, 0.0));
}

#[test]
fn physics_gravity_turns_a_bone_down_the_screen() {
    let (mut s, tip) = jumper(|p| {
        p.rotate = 1.0;
        p.gravity = 3600.0;
    });
    let mut data = (**s.data()).clone();
    data.add_clip(Clip::new("still", 1.0));
    s = Skeleton::new(Arc::new(data));
    s.play("still");
    s.update(0.0);
    s.update(1.0 / 60.0);
    // Not moving, so only gravity turns it. One step: velocity -=
    // ((wind 0 - gravity * cos 0) * length 10 / reference 100) * 1/60 =
    // 3600 * 0.1 / 60 = 6; the turn is 6 / 60 = 0.1 radians, clockwise on
    // screen: the tip drops toward +y.
    near(s.bone_rotation(tip), 0.1);
    assert!(s.bone_point(tip, vec2(10.0, 0.0)).y > 0.0);
}

#[test]
fn spine_physics_constraints_import_with_their_settings() {
    let text = r#"{ "skeleton": { "spine": "4.2.0", "referenceScale": 50 },
        "bones": [{ "name": "root" }, { "name": "hair", "parent": "root", "length": 10 }],
        "physics": [{ "name": "sway", "bone": "hair", "rotate": 1, "fps": 30, "strength": 80,
            "damping": 0.5, "mass": 2, "wind": 3, "gravity": -4, "limit": 100, "inertia": 0.25 }],
        "animations": { "blow": { "physics": { "sway": { "wind": [{ "value": 1 }, { "time": 1, "value": 5 }] },
            "": { "reset": [{ "time": 0.5 }] } } } } }"#;
    let data = SkeletonData::from_spine_json(text, "hair.json").unwrap();
    assert_eq!(data.warnings(), [] as [String; 0]);
    let p = &data.physics_constraints()[0];
    assert_eq!((p.rotate, p.strength, p.damping, p.mass), (1.0, 80.0, 0.5, 2.0));
    assert_eq!((p.wind, p.gravity, p.limit, p.inertia), (3.0, -4.0, 100.0, 0.25));
    near(p.step, 1.0 / 30.0);
    assert_eq!(p.reference_scale, 50.0);
    let clip = &data.clips()[0];
    assert_eq!(clip.physics.len(), 2);
    let sway = clip.physics.iter().find(|t| t.constraint == Some(0)).unwrap();
    assert_eq!(sway.wind[1].value, 5.0);
    let every = clip.physics.iter().find(|t| t.constraint.is_none()).unwrap();
    assert_eq!(every.reset, [0.5]);
}

// ---- The engine's own format holds everything Spine import reads ---------

/// A Spine 4.2 export using shear, IK, transform, path and physics
/// constraints (in their own order), a weighted mesh, a clipping mask, a
/// path, a second skin, and clips with bezier, stepped and per-channel
/// curves, one-axis, shear, color, attachment, deform, draw order, event
/// and every constraint timeline.
const RIG: &str = r#"{
  "skeleton": { "spine": "4.2.22", "referenceScale": 80 },
  "bones": [
    { "name": "root" },
    { "name": "hip", "parent": "root", "y": 40, "rotation": 12.5, "shearX": 4, "shearY": -7, "scaleX": 1.2 },
    { "name": "upper", "parent": "hip", "length": 30, "rotation": -80 },
    { "name": "lower", "parent": "upper", "x": 30, "length": 25 },
    { "name": "goal", "parent": "root", "x": 20, "y": 5 },
    { "name": "follower", "parent": "root", "x": -10 },
    { "name": "tail1", "parent": "root", "length": 10 },
    { "name": "tail2", "parent": "tail1", "x": 10, "length": 10 },
    { "name": "hair", "parent": "hip", "y": 20, "length": 15 }
  ],
  "slots": [
    { "name": "mask", "bone": "root", "attachment": "mask" },
    { "name": "leg", "bone": "upper", "attachment": "leg", "color": "ff8040c0" },
    { "name": "body", "bone": "hip", "attachment": "body" },
    { "name": "track", "bone": "root", "attachment": "track" }
  ],
  "ik": [{ "name": "reach", "order": 2, "bones": ["upper", "lower"], "target": "goal", "mix": 0.75, "bendPositive": false }],
  "transform": [{ "name": "follow", "order": 0, "bones": ["follower"], "target": "hip", "rotation": 15, "x": 3, "y": 4,
    "shearY": 5, "mixRotate": 0.5, "mixX": 0.25, "mixY": 0.75, "mixScaleX": 0.3, "mixShearY": 0.6 }],
  "path": [{ "name": "along", "order": 1, "bones": ["tail1", "tail2"], "target": "track", "rotateMode": "chain",
    "spacingMode": "fixed", "positionMode": "fixed", "position": 5, "spacing": 12, "rotation": 3 }],
  "physics": [{ "name": "sway", "order": 3, "bone": "hair", "rotate": 1, "x": 0.5, "fps": 30, "inertia": 0.5,
    "strength": 80, "damping": 0.9, "mass": 2, "wind": 1, "gravity": 3, "limit": 400 }],
  "skins": [
    { "name": "default", "attachments": {
      "mask": { "mask": { "type": "clipping", "end": "leg", "vertexCount": 3, "vertices": [0, 0, 40, 0, 20, 40] } },
      "leg": { "leg": { "type": "mesh", "uvs": [0, 0, 1, 0, 0.5, 1], "triangles": [0, 1, 2], "hull": 3,
        "vertices": [1, 2, 0, 0, 1, 2, 2, 30, 0, 0.6, 3, 0, 0, 0.4, 1, 3, 25, 0, 1] } },
      "body": { "body": { "width": 20, "height": 30, "x": 1, "y": 2, "rotation": 7.5, "scaleX": 1.5 } },
      "track": { "track": { "type": "path", "vertexCount": 9, "lengths": [40, 80, 0],
        "vertices": [-10, 0, 0, 0, 10, 0, 20, 0, 30, 0, 30, 10, 30, 20, 30, 30, 30, 40] } }
    } },
    { "name": "alt", "attachments": { "body": { "body": { "width": 10, "height": 10, "color": "336699ff" } } } }
  ],
  "events": { "step": {} },
  "animations": {
    "walk": {
      "bones": { "hip": {
        "rotate": [{ "value": 0, "curve": [0.1, 0, 0.2, 30] }, { "time": 0.5, "value": 30 }, { "time": 1, "value": 0 }],
        "shear": [{ "x": 0, "y": 0, "curve": [0.2, 0, 0.4, 3, 0.2, 0, 0.6, -2] }, { "time": 1, "x": 10, "y": -5 }],
        "translatex": [{ "value": 0 }, { "time": 0.75, "value": 6, "curve": "stepped" }, { "time": 1, "value": 0 }]
      } },
      "slots": { "leg": {
        "rgba": [{ "color": "ffffffff" }, { "time": 1, "color": "ff000080" }],
        "attachment": [{ "time": 0.9, "name": null }]
      } },
      "attachments": { "default": { "leg": { "leg": { "deform": [
        { "time": 0 }, { "time": 0.5, "offset": 2, "vertices": [1, 2, 3, 4] }, { "time": 1 } ] } } } },
      "ik": { "reach": [{ "mix": 0, "curve": [0.25, 0, 0.5, 1, 0.25, 0, 0.5, 0] }, { "time": 0.5, "mix": 1, "bendPositive": true }] },
      "transform": { "follow": [{ "mixRotate": 0 }, { "time": 1, "mixRotate": 1, "mixX": 0.5 }] },
      "path": { "along": {
        "position": [{ "value": 0 }, { "time": 1, "value": 20 }],
        "spacing": [{ "value": 12 }],
        "mix": [{ "mixRotate": 1 }, { "time": 1, "mixRotate": 0.5, "mixX": 0, "mixY": 0 }]
      } },
      "physics": { "sway": { "wind": [{ "value": 1 }, { "time": 1, "value": 4 }], "reset": [{ "time": 0.5 }] },
        "": { "mix": [{ "value": 1 }] } },
      "drawOrder": [{ "time": 0.25, "offsets": [{ "slot": "body", "offset": -1 }] }],
      "events": [{ "time": 0.3, "name": "step" }]
    },
    "idle": { "bones": { "lower": { "scale": [{ "x": 1, "y": 1 }, { "time": 0.4, "x": 1.1, "y": 0.9 }] } } }
  }
}"#;

/// Every bone's skeleton-space matrix, frame after frame of a walk that
/// crossfades into idle.
fn poses(data: SkeletonData) -> Vec<Vec<(Vec2, f32)>> {
    let mut s = Skeleton::new(Arc::new(data));
    s.play("walk");
    let mut out = Vec::new();
    for frame in 0..90 {
        if frame == 40 {
            s.crossfade("idle", 0.5);
        }
        s.update(1.0 / 60.0);
        out.push(
            (0..s.data().bones().len())
                .map(|b| (s.bone_position(b), s.bone_rotation(b)))
                .collect(),
        );
    }
    out
}

#[test]
fn a_spine_export_kept_in_the_engines_format_loses_nothing() {
    let path = "rig/rig.json";
    let spine = SkeletonData::from_spine_json(RIG, path).unwrap();
    assert_eq!(spine.warnings(), [] as [String; 0]);
    let text = spine.to_json(path).unwrap();
    let ours = SkeletonData::from_json(&text, path).unwrap();
    assert_eq!(ours.warnings(), [] as [String; 0]);
    assert_eq!(ours.bones(), spine.bones());
    assert!(spine.bones()[1].rest.shear != Vec2::ZERO, "shear is in the rig");
    assert_eq!(ours.slots(), spine.slots());
    assert_eq!(ours.skins(), spine.skins());
    assert_eq!(ours.ik_constraints(), spine.ik_constraints());
    assert_eq!(ours.transform_constraints(), spine.transform_constraints());
    assert_eq!(ours.path_constraints(), spine.path_constraints());
    assert_eq!(ours.physics_constraints(), spine.physics_constraints());
    assert_eq!(ours.clips(), spine.clips());
    let walk = &spine.clips()[spine.clip("walk").unwrap()];
    assert!(!walk.deform.is_empty() && !walk.ik.is_empty() && !walk.transforms.is_empty());
    assert!(!walk.paths.is_empty() && !walk.physics.is_empty());
    for (skin, slot, name) in [
        (0, 0, "mask"),
        (0, 1, "leg"),
        (0, 2, "body"),
        (0, 3, "track"),
        (1, 2, "body"),
    ] {
        let a = spine.attachment(skin, slot, name);
        assert!(a.is_some(), "{name}");
        assert_eq!(ours.attachment(skin, slot, name), a, "{name}");
    }
    assert_eq!(ours.images(), spine.images());
    // Posed and played the same, number for number, constraint order
    // (transform, path, IK, physics) included.
    assert_eq!(poses(ours.clone()), poses(spine));
    // And writing it again gives the same text.
    assert_eq!(ours.to_json(path).unwrap(), text);
}

#[test]
fn the_engines_format_reads_the_new_fields_as_documented() {
    // Hand written: shear in degrees, a clipping mask, a path with a path
    // constraint, a physics constraint, deform offsets per vertex bone as
    // listed (weights unsorted), and constraint keys.
    let text = r#"{ "format": "ponko-skeleton2d", "version": 1,
      "bones": [
        { "name": "root" },
        { "name": "a", "parent": "root", "length": 10, "shear": [0, 45] },
        { "name": "b", "parent": "root", "x": 10 }
      ],
      "slots": [
        { "name": "mask", "bone": "root", "attachment": "mask" },
        { "name": "skin", "bone": "root", "attachment": "skin" },
        { "name": "track", "bone": "root", "attachment": "track" }
      ],
      "skins": [{ "name": "default", "attachments": [
        { "slot": "mask", "name": "mask", "type": "clipping", "end": "skin", "vertices": [
          { "position": [0, 0], "bones": ["root"] }, { "position": [9, 0], "bones": ["root"] },
          { "position": [0, 9], "bones": ["root"] } ] },
        { "slot": "skin", "name": "skin", "type": "mesh", "image": "skin.png", "triangles": [0, 1, 2], "vertices": [
          { "position": [0, 0], "uv": [0, 0], "bones": ["a"] },
          { "position": [10, 0], "uv": [1, 0], "bones": ["a", "b"], "weights": [0.25, 0.75] },
          { "position": [0, 10], "uv": [0, 1], "bones": ["b"] } ] },
        { "slot": "track", "name": "track", "type": "path", "vertices": [
          { "position": [-10, 0], "bones": ["root"] }, { "position": [0, 0], "bones": ["root"] },
          { "position": [10, 0], "bones": ["root"] }, { "position": [20, 0], "bones": ["root"] },
          { "position": [30, 0], "bones": ["root"] }, { "position": [40, 0], "bones": ["root"] } ] }
      ] }],
      "path": [{ "name": "ride", "bones": ["b"], "target": "track", "position_mode": "fixed", "position": 6 }],
      "physics": [{ "name": "sway", "bone": "a", "rotate": 1, "gravity": 10, "order": -1 }],
      "clips": [{ "name": "c", "duration": 1,
        "deform": [{ "slot": "skin", "attachment": "skin", "keys": [
          { "time": 0, "value": [[1, 1], [2, 2], [3, 3], [4, 4]] } ] }],
        "path": [{ "constraint": "ride", "position": [{ "time": 0, "value": 6 }, { "time": 1, "value": 16 }] }],
        "physics": [{ "reset": [0.5] }] }]
    }"#;
    let data = SkeletonData::from_json(text, "x/x.json").unwrap();
    near(data.bones()[1].rest.shear.y, PI / 4.0);
    assert_eq!(data.path_constraints()[0].position, 6.0);
    assert_eq!(data.physics_constraints()[0].gravity, 10.0);
    let deform = &data.clips()[0].deform[0].keys[0].value;
    // Vertex 1 lists a (0.25) then b (0.75); the stronger, b, comes first
    // in the engine, so its offset [3, 3] is influence 0.
    assert_eq!(deform[4], vec2(3.0, 3.0));
    assert_eq!(deform[5], vec2(2.0, 2.0));
    assert_eq!(deform[8], vec2(4.0, 4.0));
    let mut s = Skeleton::new(Arc::new(data));
    s.play("c");
    s.update(0.5);
    let b = s.bone("b").unwrap();
    near2(s.bone_position(b), vec2(11.0, 0.0));
}
