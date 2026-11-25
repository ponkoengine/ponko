//! Humanoid clips: muscle and body curves turned into bone rotations
//! through an avatar, on a small synthetic T-posed skeleton built here.

use glam::{Mat4, Quat, Vec3};
use ponko_unity::{AnimationClip, HumanoidAvatar, MeshSkin, SkinNode};

/// A T-posed humanoid facing +Z, its feet at y = 0, named with `names`
/// (Unity's human bone name to the node's): `Armature` > `Hips` > ...
fn skeleton(names: impl Fn(&str) -> String) -> MeshSkin {
    // (human bone, parent, local position)
    let mut bones: Vec<(&str, Option<&str>, [f32; 3])> = vec![
        ("Hips", None, [0.0, 1.0, 0.0]),
        ("Spine", Some("Hips"), [0.0, 0.1, 0.0]),
        ("Chest", Some("Spine"), [0.0, 0.2, 0.0]),
        ("Neck", Some("Chest"), [0.0, 0.25, 0.0]),
        ("Head", Some("Neck"), [0.0, 0.1, 0.0]),
    ];
    for (side, x) in [("Left", -1.0), ("Right", 1.0)] {
        let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
        let b = |n: &str| leak(format!("{side}{n}"));
        bones.extend([
            (b("UpperLeg"), Some("Hips"), [0.1 * x, 0.0, 0.0]),
            (b("LowerLeg"), Some(b("UpperLeg")), [0.0, -0.45, 0.0]),
            (b("Foot"), Some(b("LowerLeg")), [0.0, -0.45, 0.0]),
            (b("Toes"), Some(b("Foot")), [0.0, -0.05, 0.1]),
            (b("Shoulder"), Some("Chest"), [0.05 * x, 0.2, 0.0]),
            (b("UpperArm"), Some(b("Shoulder")), [0.1 * x, 0.0, 0.0]),
            (b("LowerArm"), Some(b("UpperArm")), [0.3 * x, 0.0, 0.0]),
            (b("Hand"), Some(b("LowerArm")), [0.25 * x, 0.0, 0.0]),
        ]);
    }
    let mut nodes = vec![SkinNode {
        name: "Armature".into(),
        parent: None,
        matrix: Mat4::IDENTITY.to_cols_array(),
    }];
    for (human, parent, at) in &bones {
        let parent = parent.map_or(0, |p| 1 + bones.iter().position(|b| b.0 == p).unwrap());
        nodes.push(SkinNode {
            name: names(human),
            parent: Some(parent),
            matrix: Mat4::from_translation(Vec3::from(*at)).to_cols_array(),
        });
    }
    let n = nodes.len();
    MeshSkin {
        root: Some(0),
        nodes,
        joints: (0..n).collect(),
        inverse_binds: vec![Mat4::IDENTITY.to_cols_array(); n],
        ..Default::default()
    }
}

fn unity_names() -> MeshSkin {
    skeleton(str::to_owned)
}

/// A clip with humanoid float curves: `(attribute, [(time, value)])`,
/// linear between keys.
fn clip(curves: &[(&str, &[(f32, f32)])]) -> AnimationClip {
    let mut text = String::from(
        "--- !u!74 &1\nAnimationClip:\n  m_Name: Wave\n  m_SampleRate: 2\n  m_AnimationClipSettings: {m_StartTime: 0, m_StopTime: 1, m_LoopTime: 1}\n  m_FloatCurves:\n",
    );
    for (attribute, keys) in curves {
        text.push_str("  - curve:\n      m_Curve:\n");
        for (k, &(time, value)) in keys.iter().enumerate() {
            let slope = keys
                .get(k + 1)
                .or(k.checked_sub(1).map(|p| &keys[p]))
                .map_or(0.0, |&(t, v)| (v - value) / (t - time));
            text.push_str(&format!(
                "      - {{time: {time}, value: {value}, inSlope: {slope}, outSlope: {slope}}}\n"
            ));
        }
        text.push_str(&format!("    attribute: {attribute}\n    path: \n    classID: 95\n"));
    }
    AnimationClip::parse(&text).unwrap()
}

/// The muscles a T-pose has that aren't 0, at their T-pose values, for
/// both sides, except `except`.
fn t_pose_except(except: &[&str]) -> Vec<(String, Vec<(f32, f32)>)> {
    let mut curves = Vec::new();
    for side in ["Left", "Right"] {
        for (muscle, value) in [
            ("Arm Down-Up", 0.4),
            ("Arm Front-Back", 0.3),
            ("Forearm Stretch", 1.0),
            ("Upper Leg Front-Back", 0.6),
            ("Lower Leg Stretch", 1.0),
        ] {
            let name = format!("{side} {muscle}");
            if !except.contains(&name.as_str()) {
                curves.push((name, vec![(0.0, value), (1.0, value)]));
            }
        }
    }
    curves
}

fn with_t_pose(extra: &[(&str, &[(f32, f32)])]) -> AnimationClip {
    let except: Vec<&str> = extra.iter().map(|c| c.0).collect();
    let rest = t_pose_except(&except);
    let mut all: Vec<(&str, &[(f32, f32)])> = rest.iter().map(|(n, k)| (n.as_str(), k.as_slice())).collect();
    all.extend_from_slice(extra);
    clip(&all)
}

/// Every node's model-space matrix at sample `k` (rest where not animated).
fn model_at(skin: &MeshSkin, k: usize) -> Vec<Mat4> {
    let animation = &skin.animations[0];
    let mut out: Vec<Mat4> = Vec::new();
    for (i, n) in skin.nodes.iter().enumerate() {
        let local = animation.nodes[i]
            .get(k)
            .map_or(Mat4::from_cols_array(&n.matrix), Mat4::from_cols_array);
        out.push(n.parent.map_or(local, |p| out[p] * local));
    }
    out
}

fn place(skin: &MeshSkin, k: usize, name: &str) -> Vec3 {
    let i = skin.nodes.iter().position(|n| n.name == name).unwrap();
    model_at(skin, k)[i].transform_point3(Vec3::ZERO)
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.abs_diff_eq(b, 1e-4)
}

#[test]
fn a_t_pose_clip_leaves_the_rest_pose_and_raising_an_arm_turns_it_up() {
    let clip = with_t_pose(&[("Left Arm Down-Up", &[(0.0, 0.4), (1.0, 1.0)])]);
    assert!(clip.is_humanoid());
    let mut skin = unity_names();
    let avatar = HumanoidAvatar::detect(&skin);
    assert!(avatar.missing().is_empty(), "{:?}", avatar.missing());
    let report = clip.append_to_skin_with_avatar(&mut skin, 0, &avatar);
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(skin.animations[0].times, [0.0, 0.5, 1.0]);
    // At 0 every bone is at rest: the T-pose's own muscle values.
    let rest = MeshSkin {
        animations: Vec::new(),
        ..unity_names()
    };
    for name in ["LeftHand", "RightHand", "Head", "LeftToes", "RightFoot"] {
        let i = rest.nodes.iter().position(|n| n.name == name).unwrap();
        let at_rest = {
            let mut m = Mat4::from_cols_array(&rest.nodes[i].matrix);
            let mut p = rest.nodes[i].parent;
            while let Some(j) = p {
                m = Mat4::from_cols_array(&rest.nodes[j].matrix) * m;
                p = rest.nodes[j].parent;
            }
            m.transform_point3(Vec3::ZERO)
        };
        assert!(
            near(place(&skin, 0, name), at_rest),
            "{name}: {}",
            place(&skin, 0, name)
        );
    }
    // At 1 the arm is 100 * (1 - 0.4) = 60 degrees up from level, about
    // the shoulder's end; the right arm stays level.
    let shoulder = place(&skin, 2, "LeftUpperArm");
    let hand = place(&skin, 2, "LeftHand");
    let up = (hand - shoulder).normalize();
    assert!((up.y - 60f32.to_radians().sin()).abs() < 1e-3, "{up}");
    assert!(
        up.x < 0.0 && up.z.abs() < 1e-4,
        "out to the left, in the frontal plane: {up}"
    );
    assert!((place(&skin, 2, "RightHand").y - place(&skin, 0, "RightHand").y).abs() < 1e-4);
}

#[test]
fn each_muscle_turns_its_bone_the_way_its_name_says() {
    // (muscle, from, to, bone that moves, its tip, what must hold)
    type Check = fn(Vec3, Vec3) -> bool;
    let cases: [(&str, f32, f32, &str, Check); 8] = [
        // Spine Front-Back at -1: bends 40 degrees to the front (+Z).
        ("Spine Front-Back", 0.0, -1.0, "Head", |before, after| {
            after.z > before.z + 0.2
        }),
        // Spine Left-Right at 1: bends to the right (+X).
        ("Spine Left-Right", 0.0, 1.0, "Head", |before, after| {
            after.x > before.x + 0.2
        }),
        // Spine Twist Left-Right at 1: the chest turns right, so the left
        // hand comes forward.
        ("Spine Twist Left-Right", 0.0, 1.0, "LeftHand", |before, after| {
            after.z > before.z + 0.2
        }),
        // Upper leg Front-Back at -1: the leg swings forward.
        ("Left Upper Leg Front-Back", 0.6, -1.0, "LeftFoot", |before, after| {
            after.z > before.z + 0.5
        }),
        // Lower leg Stretch at 0: the knee bends 80 degrees, the foot goes back.
        ("Left Lower Leg Stretch", 1.0, 0.0, "LeftFoot", |before, after| {
            after.z < before.z - 0.3 && after.y > before.y
        }),
        // Forearm Stretch at 0: the elbow bends forward.
        ("Left Forearm Stretch", 1.0, 0.0, "LeftHand", |before, after| {
            after.z > before.z + 0.2
        }),
        // Arm Front-Back at -1: the arm swings forward.
        ("Left Arm Front-Back", 0.3, -1.0, "LeftHand", |before, after| {
            after.z > before.z + 0.3
        }),
        // Right Arm Down-Up mirrors the left: up.
        ("Right Arm Down-Up", 0.4, 1.0, "RightHand", |before, after| {
            after.y > before.y + 0.3
        }),
    ];
    for (muscle, from, to, tip, holds) in cases {
        let clip = with_t_pose(&[(muscle, &[(0.0, from), (1.0, to)])]);
        let mut skin = unity_names();
        let avatar = HumanoidAvatar::detect(&skin);
        assert!(clip.append_to_skin_with_avatar(&mut skin, 0, &avatar).is_empty());
        let (before, after) = (place(&skin, 0, tip), place(&skin, 2, tip));
        assert!(holds(before, after), "{muscle}: {before} -> {after}");
    }
}

#[test]
fn body_position_and_rotation_move_the_hips() {
    let clip = with_t_pose(&[
        ("RootT.x", &[(0.0, 0.0), (1.0, 0.0)]),
        ("RootT.y", &[(0.0, 1.0), (1.0, 0.9)]),
        ("RootT.z", &[(0.0, 0.0), (1.0, 2.0)]),
        ("RootQ.x", &[(0.0, 0.0), (1.0, 0.0)]),
        ("RootQ.y", &[(0.0, 0.0), (1.0, std::f32::consts::FRAC_1_SQRT_2)]),
        ("RootQ.z", &[(0.0, 0.0), (1.0, 0.0)]),
        ("RootQ.w", &[(0.0, 1.0), (1.0, std::f32::consts::FRAC_1_SQRT_2)]),
    ]);
    let mut skin = unity_names();
    let avatar = HumanoidAvatar::detect(&skin);
    assert!(clip.append_to_skin_with_avatar(&mut skin, 0, &avatar).is_empty());
    // In human scale: the hips' rest height, 1 here.
    assert!(near(place(&skin, 0, "Hips"), Vec3::new(0.0, 1.0, 0.0)));
    assert!(near(place(&skin, 2, "Hips"), Vec3::new(0.0, 0.9, 2.0)));
    // Turned a quarter about +Y: the left hand, out at -X, comes round to +Z.
    let hand = place(&skin, 2, "LeftHand") - place(&skin, 2, "Hips");
    assert!(hand.z > 0.5 && hand.x.abs() < 1e-3, "{hand}");
    let hips = model_at(&skin, 2)[1].to_scale_rotation_translation().1;
    assert!(hips.dot(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)).abs() > 0.9999);
}

#[test]
fn an_avatar_from_a_meta_maps_other_names_and_changed_limits() {
    let names = |human: &str| format!("bone_{}", human.to_lowercase());
    let mut meta =
        String::from("guid: rig00000\nModelImporter:\n  humanDescription:\n    serializedVersion: 3\n    human:\n");
    for human in [
        "Hips",
        "Spine",
        "Chest",
        "Neck",
        "Head",
        "LeftUpperLeg",
        "LeftLowerLeg",
        "LeftFoot",
        "LeftToes",
        "RightUpperLeg",
        "RightLowerLeg",
        "RightFoot",
        "LeftShoulder",
        "LeftUpperArm",
        "LeftLowerArm",
        "LeftHand",
        "RightShoulder",
        "RightUpperArm",
        "RightLowerArm",
        "RightHand",
    ] {
        meta.push_str(&format!("    - boneName: {}\n      humanName: {human}\n", names(human)));
        let limit = if human == "LeftUpperArm" {
            // Arm Down-Up is the bone's z: up to 50 degrees, not 100.
            "      limit:\n        min: {x: -90, y: -100, z: -60}\n        max: {x: 90, y: 100, z: 50}\n        value: {x: 0, y: 0, z: 0}\n        length: 0\n        modified: 1\n"
        } else {
            "      limit:\n        min: {x: 0, y: 0, z: 0}\n        max: {x: 0, y: 0, z: 0}\n        modified: 0\n"
        };
        meta.push_str(limit);
    }
    meta.push_str("    - boneName: tail\n      humanName: Tail\n");
    let avatar = HumanoidAvatar::from_meta(&meta).unwrap().unwrap();
    assert_eq!(avatar.bone("LeftUpperArm"), Some("bone_leftupperarm"));
    assert_eq!(avatar.bone("Tail"), None, "not a human bone");
    assert!(avatar.missing().is_empty());
    let mut skin = skeleton(names);
    let clip = with_t_pose(&[("Left Arm Down-Up", &[(0.0, 0.4), (1.0, 1.0)])]);
    assert!(clip.append_to_skin_with_avatar(&mut skin, 0, &avatar).is_empty());
    // (1 - 0.4) of 50 degrees: 30 up.
    let up = (place(&skin, 2, "bone_lefthand") - place(&skin, 2, "bone_leftupperarm")).normalize();
    assert!((up.y - 0.5).abs() < 1e-3, "{up}");
    // No human bones: no avatar.
    assert!(
        HumanoidAvatar::from_meta("ModelImporter:\n  humanDescription: {}\n")
            .unwrap()
            .is_none()
    );
}

#[test]
fn detection_reads_common_rig_names() {
    let common = |human: &str| -> String {
        let name = match human {
            "Chest" => "Spine1".to_owned(),
            h => h
                .replace("UpperLeg", "UpLeg")
                .replace("LowerLeg", "Leg")
                .replace("UpperArm", "Arm")
                .replace("LowerArm", "ForeArm")
                .replace("Toes", "ToeBase"),
        };
        format!("rig:{name}")
    };
    let skin = skeleton(common);
    let avatar = HumanoidAvatar::detect(&skin);
    assert!(avatar.missing().is_empty(), "{:?}", avatar.missing());
    assert_eq!(avatar.bone("LeftLowerArm"), Some("rig:LeftForeArm"));
    assert_eq!(avatar.bone("Chest"), Some("rig:Spine1"));
    assert_eq!(avatar.bone("LeftToes"), Some("rig:LeftToeBase"));
}

#[test]
fn avatar_settings_are_checked() {
    let mut avatar = HumanoidAvatar::new();
    assert!(avatar.set_bone("Left Index Proximal", "index_01_l").is_ok());
    assert!(avatar.set_bone("Wing", "wing").is_err());
    assert!(avatar.set_limits("Left Arm Down-Up", -10.0, 10.0).is_ok());
    assert!(avatar.set_limits("Left Arm Down-Up", 10.0, -10.0).is_err());
    assert!(avatar.set_limits("Left Wing Flap", -10.0, 10.0).is_err());
    assert!(avatar.set_rest_muscle("RightHand.Index.Spread", 0.2).is_ok());
    assert!(avatar.set_rest_muscle("Spine Wobble", 0.2).is_err());
    assert_eq!(avatar.missing().len(), 15);
}

#[test]
fn a_rest_muscle_setting_moves_the_pose_a_clip_leaves_alone() {
    // A rig resting with its arms down (an A-pose): its rest's Arm Down-Up
    // is 0, so a clip at 0.4 lifts the arms 40 degrees.
    let clip = with_t_pose(&[]);
    let mut skin = unity_names();
    let mut avatar = HumanoidAvatar::detect(&skin);
    avatar.set_rest_muscle("Left Arm Down-Up", 0.0).unwrap();
    assert!(clip.append_to_skin_with_avatar(&mut skin, 0, &avatar).is_empty());
    let up = (place(&skin, 0, "LeftHand") - place(&skin, 0, "LeftUpperArm")).normalize();
    assert!((up.y - 40f32.to_radians().sin()).abs() < 1e-3, "{up}");
}

#[test]
fn humanoid_curves_without_an_avatar_or_bones_are_reported() {
    let clip = with_t_pose(&[("LeftFootT.x", &[(0.0, 0.0), (1.0, 1.0)])]);
    let mut skin = unity_names();
    let report = clip.append_to_skin(&mut skin, 0);
    assert!(report.iter().any(|d| d.contains("need an avatar")), "{report:?}");
    assert!(report.iter().any(|d| d.contains("IK goal")), "{report:?}");
    assert!(skin.animations.is_empty(), "nothing else to animate");
    // An avatar missing required bones, or naming nodes the skin lacks.
    let mut avatar = HumanoidAvatar::detect(&skin);
    avatar.set_bone("Head", "NoSuchNode").unwrap();
    let report = clip.append_to_skin_with_avatar(&mut skin, 0, &avatar);
    assert!(report.iter().any(|d| d.contains("Head")), "{report:?}");
    assert!(skin.animations.is_empty());
}

#[test]
fn transform_curves_win_over_muscles_on_the_same_node() {
    let mut text = String::from(
        "--- !u!74 &1\nAnimationClip:\n  m_Name: Mixed\n  m_SampleRate: 1\n  m_AnimationClipSettings: {m_StartTime: 0, m_StopTime: 1}\n  m_PositionCurves:\n  - path: Hips/Spine\n    curve:\n      m_Curve:\n      - {time: 0, value: {x: 0, y: 7, z: 0}}\n  m_FloatCurves:\n",
    );
    text.push_str("  - curve:\n      m_Curve:\n      - {time: 0, value: -1}\n    attribute: Spine Front-Back\n    path: \n    classID: 95\n");
    let clip = AnimationClip::parse(&text).unwrap();
    let mut skin = unity_names();
    let avatar = HumanoidAvatar::detect(&skin);
    assert!(clip.append_to_skin_with_avatar(&mut skin, 0, &avatar).is_empty());
    let spine = skin.nodes.iter().position(|n| n.name == "Spine").unwrap();
    let (_, r, t) = Mat4::from_cols_array(&skin.animations[0].nodes[spine][0]).to_scale_rotation_translation();
    assert_eq!(t.y, 7.0);
    assert!(r.dot(Quat::IDENTITY).abs() > 0.9999);
    // The chest still follows its own (absent: zero) muscles.
    let chest = skin.nodes.iter().position(|n| n.name == "Chest").unwrap();
    assert!(!skin.animations[0].nodes[chest].is_empty());
}
