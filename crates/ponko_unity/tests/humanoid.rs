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

// --- Through the importer ---

/// The human bones of `skeleton`, parents first, with their parent and
/// local position (Unity's names).
fn bone_list() -> Vec<(String, Option<usize>, Vec3)> {
    let skin = unity_names();
    skin.nodes
        .iter()
        .skip(1)
        .map(|n| {
            let parent = n.parent.filter(|&p| p > 0).map(|p| p - 1);
            (
                n.name.clone(),
                parent,
                Mat4::from_cols_array(&n.matrix).w_axis.truncate(),
            )
        })
        .collect()
}

/// The bone names of the imported characters: `bone_<Unity's name in
/// lower case>`.
fn node(human: &str) -> String {
    format!("bone_{}", human.to_lowercase())
}

/// What the importer writes for a prefab of a character whose bones are
/// named by `node` (or Unity's names, `unity_names`), with `clips`. Its
/// Animator uses the avatar of `Rig.fbx` (Rig set to Humanoid, the
/// `.meta` mapping `node` names) if `animator_avatar`. Returns the
/// `.glb`'s bytes and the report's entries.
fn import_character(
    name: &str,
    animator_avatar: bool,
    unity_names: bool,
    configure: impl FnOnce(&mut ponko_unity::Importer),
) -> (Vec<u8>, Vec<String>) {
    use ponko_unity::{Hierarchy, Importer, Project, UnityFile};
    let dir = std::env::temp_dir().join(format!("ponko_unity_humanoid_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let assets = dir.join("Assets");
    std::fs::create_dir_all(&assets).unwrap();
    let bones = bone_list();
    let named = |human: &str| if unity_names { human.to_owned() } else { node(human) };
    // The body: a mesh asset skinned to every bone, all on the first.
    let mut mesh = include_str!("fixtures/quad.asset").to_owned();
    mesh.push_str("\n  m_BindPose:\n");
    for _ in &bones {
        mesh.push_str("  - {e00: 1, e11: 1, e22: 1, e33: 1}\n");
    }
    mesh.push_str("  m_Skin:\n");
    for _ in 0..5 {
        mesh.push_str("  - {weight0: 1, boneIndex0: 0}\n");
    }
    std::fs::write(assets.join("Body.asset"), mesh).unwrap();
    std::fs::write(assets.join("Body.asset.meta"), "guid: body0000\n").unwrap();
    // The model the avatar comes from (its mesh isn't used).
    std::fs::write(assets.join("Rig.fbx"), include_bytes!("fixtures/skinned.fbx")).unwrap();
    let mut meta =
        String::from("guid: rig00000\nModelImporter:\n  animationType: 3\n  humanDescription:\n    human:\n");
    for (human, ..) in &bones {
        meta.push_str(&format!("    - boneName: {}\n      humanName: {human}\n", node(human)));
    }
    std::fs::write(assets.join("Rig.fbx.meta"), &meta).unwrap();
    // Character (Animator) > Body (the renderer), Character > Armature >
    // the bones. Transforms are `id + 1`; bone k is object 100 + 10 k.
    let mut prefab = String::from("%YAML 1.1\n");
    let mut object = |prefab: &mut String, id: usize, name: &str, parent: usize, at: Vec3| {
        prefab.push_str(&format!(
            "--- !u!1 &{id}\nGameObject:\n  m_Name: {name}\n  m_IsActive: 1\n--- !u!4 &{}\nTransform:\n  m_GameObject: {{fileID: {id}}}\n  m_Father: {{fileID: {parent}}}\n  m_LocalRotation: {{x: 0, y: 0, z: 0, w: 1}}\n  m_LocalPosition: {{x: {}, y: {}, z: {}}}\n  m_LocalScale: {{x: 1, y: 1, z: 1}}\n",
            id + 1,
            at.x,
            at.y,
            at.z
        ));
    };
    object(&mut prefab, 1, "Character", 0, Vec3::ZERO);
    object(&mut prefab, 11, "Body", 2, Vec3::ZERO);
    object(&mut prefab, 21, "Armature", 2, Vec3::ZERO);
    for (k, (human, parent, at)) in bones.iter().enumerate() {
        let parent = parent.map_or(22, |p| 100 + 10 * p + 1);
        object(&mut prefab, 100 + 10 * k, &named(human), parent, *at);
    }
    prefab.push_str("--- !u!95 &3\nAnimator:\n  m_GameObject: {fileID: 1}\n  m_Enabled: 1\n");
    if animator_avatar {
        prefab.push_str("  m_Avatar: {fileID: 9000000, guid: rig00000, type: 3}\n");
    }
    let refs: Vec<String> = (0..bones.len())
        .map(|k| format!("{{fileID: {}}}", 100 + 10 * k + 1))
        .collect();
    prefab.push_str(&format!(
        "--- !u!137 &13\nSkinnedMeshRenderer:\n  m_GameObject: {{fileID: 11}}\n  m_Enabled: 1\n  m_Mesh: {{fileID: 4300000, guid: body0000, type: 2}}\n  m_Bones: [{}]\n  m_RootBone: {{fileID: 101}}\n  m_Materials: []\n",
        refs.join(", ")
    ));
    let project = Project::open(&dir).unwrap();
    let hierarchy = Hierarchy::build(&UnityFile::parse(&prefab).unwrap(), &project).unwrap();
    let out = dir.join("out");
    let mut importer = Importer::new(&project, &out, "");
    importer.animation_clips([wave()]);
    configure(&mut importer);
    let scene = importer.scene(&hierarchy).unwrap();
    let report = importer.finish().unwrap();
    let path = scene["entities"][0]["components"]["Model"]["mesh"].as_str().unwrap();
    let bytes = std::fs::read(out.join(path)).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    (bytes, report.skipped.keys().cloned().collect())
}

/// A node's model-space place in a `.glb` at the last sample of animation
/// `clip` (rest where it isn't animated).
fn glb_place(bytes: &[u8], clip: &str, name: &str) -> Vec3 {
    let gltf = gltf::Gltf::from_slice(bytes).unwrap();
    let blob = gltf.blob.as_deref();
    let animation = gltf.animations().find(|a| a.name() == Some(clip)).unwrap();
    let mut local: Vec<(Vec3, Quat, Vec3)> = gltf
        .nodes()
        .map(|n| {
            let (t, r, s) = n.transform().decomposed();
            (Vec3::from(t), Quat::from_array(r), Vec3::from(s))
        })
        .collect();
    for channel in animation.channels() {
        let i = channel.target().node().index();
        match channel.reader(|_| blob).read_outputs().unwrap() {
            gltf::animation::util::ReadOutputs::Translations(v) => local[i].0 = Vec3::from(v.last().unwrap()),
            gltf::animation::util::ReadOutputs::Rotations(v) => {
                local[i].1 = Quat::from_array(v.into_f32().last().unwrap());
            }
            _ => {}
        }
    }
    let mut parent = vec![None; local.len()];
    for n in gltf.nodes() {
        for c in n.children() {
            parent[c.index()] = Some(n.index());
        }
    }
    let target = gltf.nodes().find(|n| n.name() == Some(name)).unwrap().index();
    let mut m = Mat4::IDENTITY;
    let mut at = Some(target);
    while let Some(i) = at {
        let (t, r, s) = local[i];
        m = Mat4::from_scale_rotation_translation(s, r, t) * m;
        at = parent[i];
    }
    m.transform_point3(Vec3::ZERO)
}

fn wave() -> AnimationClip {
    with_t_pose(&[("Left Arm Down-Up", &[(0.0, 0.4), (1.0, 1.0)])])
}

#[test]
fn the_importer_takes_the_avatar_of_the_model_the_animator_names() {
    let (bytes, skipped) = import_character("animator", true, false, |_| {});
    assert!(skipped.is_empty(), "{skipped:?}");
    // In Ponko's space (Z mirrored): the left arm 60 degrees up.
    let up = (glb_place(&bytes, "Wave", "bone_lefthand") - glb_place(&bytes, "Wave", "bone_leftupperarm")).normalize();
    assert!((up.y - 60f32.to_radians().sin()).abs() < 1e-3, "{up}");
    let level = glb_place(&bytes, "Wave", "bone_righthand").y - glb_place(&bytes, "Wave", "bone_rightupperarm").y;
    assert!(level.abs() < 1e-4, "{level}");
    // The engine's play test reads this file: keep it current with
    // `PONKO_UPDATE_UNITY_FIXTURES=1 cargo test -p ponko_unity --test humanoid`.
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/unity/humanoid_wave.glb"
    );
    if std::env::var_os("PONKO_UPDATE_UNITY_FIXTURES").is_some() {
        std::fs::create_dir_all(std::path::Path::new(fixture).parent().unwrap()).unwrap();
        std::fs::write(fixture, &bytes).unwrap();
    }
    let saved = std::fs::read(fixture).expect("tests/fixtures/unity/humanoid_wave.glb");
    for bone in [
        "bone_lefthand",
        "bone_righthand",
        "bone_head",
        "bone_leftfoot",
        "bone_hips",
    ] {
        let (now, then) = (glb_place(&bytes, "Wave", bone), glb_place(&saved, "Wave", bone));
        assert!(
            now.abs_diff_eq(then, 1e-4),
            "{bone}: {now} vs the saved {then}; update the fixture"
        );
    }
}

#[test]
fn without_an_animator_avatar_the_importer_uses_its_own_or_finds_the_bones() {
    // Names it can't guess, and no avatar: reported, nothing turned.
    let (bytes, skipped) = import_character("none", false, false, |_| {});
    assert!(skipped.iter().any(|s| s.contains("need an avatar")), "{skipped:?}");
    let gltf = gltf::Gltf::from_slice(&bytes).unwrap();
    assert!(gltf.animations().next().is_none());
    // The same names through `humanoid_avatar`.
    let mut meta = String::from("ModelImporter:\n  humanDescription:\n    human:\n");
    for (human, ..) in bone_list() {
        meta.push_str(&format!("    - boneName: {}\n      humanName: {human}\n", node(&human)));
    }
    let avatar = HumanoidAvatar::from_meta(&meta).unwrap().unwrap();
    let (bytes, skipped) = import_character("given", false, false, |i| i.humanoid_avatar(avatar));
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(glb_place(&bytes, "Wave", "bone_lefthand").y > 1.7);
    // Unity's own bone names: found without any avatar.
    let (bytes, skipped) = import_character("found", false, true, |_| {});
    assert!(skipped.is_empty(), "{skipped:?}");
    assert!(glb_place(&bytes, "Wave", "LeftHand").y > 1.7);
}
