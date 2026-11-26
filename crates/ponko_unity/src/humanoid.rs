//! Humanoid ("Mecanim") clips: muscle and body curves turned into bone
//! rotations for a skeleton, through a [`HumanoidAvatar`] that says which
//! skeleton node plays each of Unity's human bones.
//!
//! Each human bone turns by up to three muscles: a twist about the bone
//! (towards its child), and two swings across it. A muscle's value, from
//! -1 to 1, scales its limit: `value * max` for positive values, `-value *
//! min` for negative ones, in degrees. The angle is measured from Unity's
//! zero pose, which isn't the T-pose (arms lowered, elbows and knees
//! bent): the importer takes the skeleton's rest pose as a T-pose, whose
//! muscle values ([`HumanoidAvatar::set_rest_muscle`]) are subtracted
//! first. The axes come from the rest pose, the character facing +Z with
//! +Y up (as Unity's avatars face), so names and joint axes may differ
//! from Unity's own rigs.

use crate::{Error, MeshSkin, Value};
use glam::{Mat4, Quat, Vec3};
use std::collections::{BTreeMap, HashMap};

/// How a muscle turns its bone, in the model space of the rest pose:
/// `Swing(d)` moves the bone's tip towards `d` for positive values;
/// `Twist(r, d)` turns the bone about itself, moving the side facing `r`
/// towards `d`.
#[derive(Clone, Copy)]
enum Motion {
    Swing([f32; 3]),
    Twist([f32; 3], [f32; 3]),
}
use Motion::{Swing, Twist};

const BACK: [f32; 3] = [0.0, 0.0, -1.0];
const FRONT: [f32; 3] = [0.0, 0.0, 1.0];
const UP: [f32; 3] = [0.0, 1.0, 0.0];
const DOWN: [f32; 3] = [0.0, -1.0, 0.0];
const RIGHT: [f32; 3] = [1.0, 0.0, 0.0];
/// Away from the body's middle, on the left side (mirrored for the right).
const OUT: [f32; 3] = [-1.0, 0.0, 0.0];
/// Out and forward: a thumb's direction without its next bone.
const THUMB: [f32; 3] = [-1.0, 0.0, 1.0];

/// One of Unity's muscles: its name in clips, the human bone it turns,
/// how, its default limits (degrees) and its value in a T-pose.
struct Muscle {
    name: &'static str,
    bone: &'static str,
    motion: Motion,
    min: f32,
    max: f32,
    rest: f32,
}

const fn m(name: &'static str, bone: &'static str, motion: Motion, min: f32, max: f32) -> Muscle {
    Muscle {
        name,
        bone,
        motion,
        min,
        max,
        rest: 0.0,
    }
}
const fn at_rest(muscle: Muscle, rest: f32) -> Muscle {
    Muscle { rest, ..muscle }
}

/// Unity's muscles, the left side's (the right side's mirror them).
#[rustfmt::skip]
const MUSCLES: &[Muscle] = &[
    m("Spine Front-Back", "Spine", Swing(BACK), -40.0, 40.0),
    m("Spine Left-Right", "Spine", Swing(RIGHT), -40.0, 40.0),
    m("Spine Twist Left-Right", "Spine", Twist(FRONT, RIGHT), -40.0, 40.0),
    m("Chest Front-Back", "Chest", Swing(BACK), -40.0, 40.0),
    m("Chest Left-Right", "Chest", Swing(RIGHT), -40.0, 40.0),
    m("Chest Twist Left-Right", "Chest", Twist(FRONT, RIGHT), -40.0, 40.0),
    m("UpperChest Front-Back", "UpperChest", Swing(BACK), -20.0, 20.0),
    m("UpperChest Left-Right", "UpperChest", Swing(RIGHT), -20.0, 20.0),
    m("UpperChest Twist Left-Right", "UpperChest", Twist(FRONT, RIGHT), -20.0, 20.0),
    m("Neck Nod Down-Up", "Neck", Swing(BACK), -40.0, 40.0),
    m("Neck Tilt Left-Right", "Neck", Swing(RIGHT), -40.0, 40.0),
    m("Neck Turn Left-Right", "Neck", Twist(FRONT, RIGHT), -40.0, 40.0),
    m("Head Nod Down-Up", "Head", Swing(BACK), -40.0, 40.0),
    m("Head Tilt Left-Right", "Head", Swing(RIGHT), -40.0, 40.0),
    m("Head Turn Left-Right", "Head", Twist(FRONT, RIGHT), -40.0, 40.0),
    m("Left Eye Down-Up", "LeftEye", Swing(UP), -10.0, 15.0),
    m("Left Eye In-Out", "LeftEye", Swing(OUT), -20.0, 20.0),
    m("Jaw Close", "Jaw", Swing(UP), -10.0, 10.0),
    m("Jaw Left-Right", "Jaw", Swing(RIGHT), -10.0, 10.0),
    at_rest(m("Left Upper Leg Front-Back", "LeftUpperLeg", Swing(BACK), -90.0, 50.0), 0.6),
    m("Left Upper Leg In-Out", "LeftUpperLeg", Swing(OUT), -60.0, 60.0),
    m("Left Upper Leg Twist In-Out", "LeftUpperLeg", Twist(FRONT, OUT), -60.0, 60.0),
    at_rest(m("Left Lower Leg Stretch", "LeftLowerLeg", Swing(FRONT), -80.0, 80.0), 1.0),
    m("Left Lower Leg Twist In-Out", "LeftLowerLeg", Twist(FRONT, OUT), -90.0, 90.0),
    m("Left Foot Up-Down", "LeftFoot", Swing(DOWN), -50.0, 50.0),
    m("Left Foot Twist In-Out", "LeftFoot", Swing(OUT), -30.0, 30.0),
    m("Left Toes Up-Down", "LeftToes", Swing(DOWN), -50.0, 50.0),
    m("Left Shoulder Down-Up", "LeftShoulder", Swing(UP), -15.0, 30.0),
    m("Left Shoulder Front-Back", "LeftShoulder", Swing(BACK), -15.0, 15.0),
    at_rest(m("Left Arm Down-Up", "LeftUpperArm", Swing(UP), -60.0, 100.0), 0.4),
    at_rest(m("Left Arm Front-Back", "LeftUpperArm", Swing(BACK), -100.0, 100.0), 0.3),
    m("Left Arm Twist In-Out", "LeftUpperArm", Twist(FRONT, UP), -90.0, 90.0),
    at_rest(m("Left Forearm Stretch", "LeftLowerArm", Swing(BACK), -80.0, 80.0), 1.0),
    m("Left Forearm Twist In-Out", "LeftLowerArm", Twist(FRONT, UP), -90.0, 90.0),
    m("Left Hand Down-Up", "LeftHand", Swing(UP), -80.0, 80.0),
    m("Left Hand In-Out", "LeftHand", Swing(BACK), -40.0, 40.0),
    m("LeftHand.Thumb.1 Stretched", "Left Thumb Proximal", Swing(UP), -20.0, 20.0),
    m("LeftHand.Thumb.Spread", "Left Thumb Proximal", Swing(FRONT), -25.0, 25.0),
    m("LeftHand.Thumb.2 Stretched", "Left Thumb Intermediate", Swing(UP), -40.0, 35.0),
    m("LeftHand.Thumb.3 Stretched", "Left Thumb Distal", Swing(UP), -40.0, 35.0),
    m("LeftHand.Index.1 Stretched", "Left Index Proximal", Swing(UP), -50.0, 50.0),
    m("LeftHand.Index.Spread", "Left Index Proximal", Swing(FRONT), -20.0, 20.0),
    m("LeftHand.Index.2 Stretched", "Left Index Intermediate", Swing(UP), -45.0, 45.0),
    m("LeftHand.Index.3 Stretched", "Left Index Distal", Swing(UP), -45.0, 45.0),
    m("LeftHand.Middle.1 Stretched", "Left Middle Proximal", Swing(UP), -50.0, 50.0),
    m("LeftHand.Middle.Spread", "Left Middle Proximal", Swing(FRONT), -7.5, 7.5),
    m("LeftHand.Middle.2 Stretched", "Left Middle Intermediate", Swing(UP), -45.0, 45.0),
    m("LeftHand.Middle.3 Stretched", "Left Middle Distal", Swing(UP), -45.0, 45.0),
    m("LeftHand.Ring.1 Stretched", "Left Ring Proximal", Swing(UP), -50.0, 50.0),
    m("LeftHand.Ring.Spread", "Left Ring Proximal", Swing(BACK), -7.5, 7.5),
    m("LeftHand.Ring.2 Stretched", "Left Ring Intermediate", Swing(UP), -45.0, 45.0),
    m("LeftHand.Ring.3 Stretched", "Left Ring Distal", Swing(UP), -45.0, 45.0),
    m("LeftHand.Little.1 Stretched", "Left Little Proximal", Swing(UP), -50.0, 50.0),
    m("LeftHand.Little.Spread", "Left Little Proximal", Swing(BACK), -20.0, 20.0),
    m("LeftHand.Little.2 Stretched", "Left Little Intermediate", Swing(UP), -45.0, 45.0),
    m("LeftHand.Little.3 Stretched", "Left Little Distal", Swing(UP), -45.0, 45.0),
];

/// Unity's human bones (the left side's), with the bones that lie along
/// them (the first one the avatar has gives the bone's direction) and the
/// direction to take without one.
#[rustfmt::skip]
const BONES: &[(&str, &[&str], [f32; 3])] = &[
    ("Hips", &["Spine"], UP),
    ("Spine", &["Chest", "UpperChest", "Neck", "Head"], UP),
    ("Chest", &["UpperChest", "Neck", "Head"], UP),
    ("UpperChest", &["Neck", "Head"], UP),
    ("Neck", &["Head"], UP),
    ("Head", &[], UP),
    ("Jaw", &[], FRONT),
    ("LeftEye", &[], FRONT),
    ("LeftUpperLeg", &["LeftLowerLeg"], DOWN),
    ("LeftLowerLeg", &["LeftFoot"], DOWN),
    ("LeftFoot", &["LeftToes"], FRONT),
    ("LeftToes", &[], FRONT),
    ("LeftShoulder", &["LeftUpperArm"], OUT),
    ("LeftUpperArm", &["LeftLowerArm"], OUT),
    ("LeftLowerArm", &["LeftHand"], OUT),
    ("LeftHand", &["Left Middle Proximal", "Left Index Proximal"], OUT),
    ("Left Thumb Proximal", &["Left Thumb Intermediate"], THUMB),
    ("Left Thumb Intermediate", &["Left Thumb Distal"], THUMB),
    ("Left Thumb Distal", &[], THUMB),
    ("Left Index Proximal", &["Left Index Intermediate"], OUT),
    ("Left Index Intermediate", &["Left Index Distal"], OUT),
    ("Left Index Distal", &[], OUT),
    ("Left Middle Proximal", &["Left Middle Intermediate"], OUT),
    ("Left Middle Intermediate", &["Left Middle Distal"], OUT),
    ("Left Middle Distal", &[], OUT),
    ("Left Ring Proximal", &["Left Ring Intermediate"], OUT),
    ("Left Ring Intermediate", &["Left Ring Distal"], OUT),
    ("Left Ring Distal", &[], OUT),
    ("Left Little Proximal", &["Left Little Intermediate"], OUT),
    ("Left Little Intermediate", &["Left Little Distal"], OUT),
    ("Left Little Distal", &[], OUT),
];

/// The human bones every humanoid avatar has (Unity requires them).
pub(crate) const REQUIRED: [&str; 15] = [
    "Hips",
    "Spine",
    "Head",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
];

fn mirrored(v: [f32; 3]) -> [f32; 3] {
    [-v[0], v[1], v[2]]
}

/// The right side's name of a left side's (`Left...` to `Right...`).
fn right_of(name: &str) -> Option<String> {
    name.contains("Left").then(|| name.replacen("Left", "Right", 1))
}

/// Every muscle, both sides, with its motion for that side.
fn muscles() -> impl Iterator<Item = (String, String, Motion, f32, f32, f32)> {
    MUSCLES.iter().flat_map(|mu| {
        let sided = mu.bone.contains("Left");
        let left = (
            mu.name.to_owned(),
            mu.bone.to_owned(),
            mu.motion,
            mu.min,
            mu.max,
            mu.rest,
        );
        let right = sided.then(|| {
            let motion = match mu.motion {
                Swing(d) => Swing(mirrored(d)),
                Twist(r, d) => Twist(mirrored(r), mirrored(d)),
            };
            (
                mu.name.replacen("Left", "Right", 1),
                mu.bone.replacen("Left", "Right", 1),
                motion,
                mu.min,
                mu.max,
                mu.rest,
            )
        });
        std::iter::once(left).chain(right)
    })
}

/// Every human bone, both sides: its name, the bones along it and its
/// direction without them.
fn bones() -> impl Iterator<Item = (String, Vec<String>, [f32; 3])> {
    BONES.iter().flat_map(|(name, along, dir)| {
        let left = (
            (*name).to_owned(),
            along.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            *dir,
        );
        let right = right_of(name).map(|r| {
            (
                r,
                along.iter().map(|s| s.replacen("Left", "Right", 1)).collect::<Vec<_>>(),
                mirrored(*dir),
            )
        });
        std::iter::once(left).chain(right)
    })
}

/// Whether `name` is one of Unity's human bones.
fn is_bone(name: &str) -> bool {
    bones().any(|(b, _, _)| b == name)
}

/// Whether `name` is one of Unity's muscles.
pub(crate) fn is_muscle(name: &str) -> bool {
    muscles().any(|(m, ..)| m == name)
}

/// Which skeleton node plays each of Unity's human bones, for turning a
/// humanoid clip's muscle curves into bone rotations: what a Unity Avatar
/// holds. A model's `.meta` has one (its Rig set to Humanoid):
/// [`from_meta`](Self::from_meta). [`detect`](Self::detect) finds the
/// bones by the usual names, and [`set_bone`](Self::set_bone) fixes or
/// adds one.
///
/// Muscles keep Unity's default limits unless the avatar changed them
/// ([`set_limits`](Self::set_limits)).
///
/// ```
/// let mut avatar = ponko_unity::HumanoidAvatar::new();
/// avatar.set_bone("Hips", "pelvis")?;
/// assert_eq!(avatar.bone("Hips"), Some("pelvis"));
/// assert!(avatar.set_bone("Tail", "tail_01").is_err(), "not a human bone");
/// # Ok::<(), ponko_unity::Error>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HumanoidAvatar {
    /// Human bone name to skeleton node name.
    bones: BTreeMap<String, String>,
    /// Muscle name to (min, max) degrees, where they differ from Unity's.
    limits: HashMap<String, (f32, f32)>,
    /// Muscle name to its value in the rest pose, where it differs.
    rest: HashMap<String, f32>,
    /// Node name to its rest rotation (Unity's space) in the avatar's
    /// reference pose, from the `.meta`.
    skeleton: HashMap<String, Quat>,
}

impl HumanoidAvatar {
    /// An avatar with no bones yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The avatar a model's `.meta` describes (`humanDescription`, written
    /// when its Rig is Humanoid): its bone names, any limits changed in
    /// its Muscles & Settings, and its reference pose. `None` when the
    /// `.meta` maps no human bones.
    ///
    /// ```
    /// let meta = "ModelImporter:\n  humanDescription:\n    human:\n    - boneName: pelvis\n      humanName: Hips\n";
    /// let avatar = ponko_unity::HumanoidAvatar::from_meta(meta)?.unwrap();
    /// assert_eq!(avatar.bone("Hips"), Some("pelvis"));
    /// # Ok::<(), ponko_unity::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// When the text isn't YAML.
    pub fn from_meta(text: &str) -> Result<Option<Self>, Error> {
        let meta = crate::value::parse(text).map_err(|e| Error::decode(e.message).at_line(e.line))?;
        Ok(Self::from_description(&meta["ModelImporter"]["humanDescription"]))
    }

    pub(crate) fn from_description(description: &Value) -> Option<Self> {
        let mut avatar = Self::new();
        for h in description["human"].as_seq().unwrap_or_default() {
            let (Some(node), Some(human)) = (h["boneName"].as_str(), h["humanName"].as_str()) else {
                continue;
            };
            if !is_bone(human) {
                continue;
            }
            avatar.bones.insert(human.to_owned(), node.to_owned());
            let limit = &h["limit"];
            if limit["modified"].as_i64() == Some(1) {
                // x, y and z are the bone's twist and its two swings.
                let axes = |v: &Value| ["x", "y", "z"].map(|k| v[k].as_f32());
                let (min, max) = (axes(&limit["min"]), axes(&limit["max"]));
                for (name, bone, motion, ..) in muscles() {
                    if bone != human {
                        continue;
                    }
                    // x is the twist; the table lists each bone's main
                    // swing (z) before its second one (y).
                    let k = match motion {
                        Twist(..) => 0,
                        Swing(_) => {
                            let first = muscles()
                                .find(|(_, b, m, ..)| *b == bone && matches!(m, Swing(_)))
                                .is_some_and(|(n, ..)| n == name);
                            if first { 2 } else { 1 }
                        }
                    };
                    if let (Some(lo), Some(hi)) = (min[k], max[k])
                        && lo <= hi
                    {
                        avatar.limits.insert(name, (lo, hi));
                    }
                }
            }
        }
        for s in description["skeleton"].as_seq().unwrap_or_default() {
            if let Some(name) = s["name"].as_str() {
                let r = &s["rotation"];
                let q = Quat::from_array(["x", "y", "z", "w"].map(|k| r[k].as_f32().unwrap_or(0.0)));
                if q.length_squared() > 1e-6 {
                    avatar.skeleton.insert(name.to_owned(), q.normalize());
                }
            }
        }
        (!avatar.bones.is_empty()).then_some(avatar)
    }

    /// An avatar for `skin` from its node names: Unity's own bone names
    /// (`Hips`, `LeftUpperArm`, `Left Index Proximal`, with or without
    /// spaces), and the common `LeftUpLeg`, `LeftLeg`, `LeftArm`,
    /// `LeftForeArm`, `LeftToeBase`, `Spine1`/`Spine2` and
    /// `LeftHandIndex1` style, after any `prefix:` or `prefix|`.
    ///
    /// ```
    /// # use ponko_unity::{HumanoidAvatar, MeshSkin, SkinNode};
    /// let node = |name: &str| SkinNode { name: name.into(), parent: None, matrix: glam::Mat4::IDENTITY.to_cols_array() };
    /// let skin = MeshSkin { nodes: vec![node("rig:Hips"), node("rig:LeftUpLeg")], ..Default::default() };
    /// let avatar = HumanoidAvatar::detect(&skin);
    /// assert_eq!(avatar.bone("LeftUpperLeg"), Some("rig:LeftUpLeg"));
    /// ```
    #[must_use]
    pub fn detect(skin: &MeshSkin) -> Self {
        let mut avatar = Self::new();
        let bare = |name: &str| -> String {
            let name = name.rsplit([':', '|']).next().unwrap_or(name);
            name.chars()
                .filter(|c| !c.is_whitespace() && *c != '_')
                .collect::<String>()
        };
        for (human, ..) in bones() {
            let mut names = vec![human.replace(' ', "")];
            let side = ["Left", "Right"]
                .into_iter()
                .find(|s| human.starts_with(s))
                .unwrap_or("");
            let rest = human.trim_start_matches(side).trim();
            let alias: &[&str] = match (human.as_str(), rest) {
                ("Chest", _) => &["Spine1"],
                ("UpperChest", _) => &["Spine2"],
                (_, "UpperLeg") => &["UpLeg"],
                (_, "LowerLeg") => &["Leg"],
                (_, "UpperArm") => &["Arm"],
                (_, "LowerArm") => &["ForeArm"],
                (_, "Toes") => &["ToeBase"],
                _ => &[],
            };
            names.extend(alias.iter().map(|a| format!("{side}{a}")));
            // `Left Index Proximal`: `LeftHandIndex1`.
            for (k, joint) in ["Proximal", "Intermediate", "Distal"].iter().enumerate() {
                if let Some(finger) = rest.strip_suffix(joint) {
                    names.push(format!("{side}Hand{}{}", finger.trim(), k + 1));
                }
            }
            let found = skin
                .nodes
                .iter()
                .find(|n| names.iter().any(|w| bare(&n.name).eq_ignore_ascii_case(w)));
            if let Some(n) = found {
                avatar.bones.insert(human, n.name.clone());
            }
        }
        avatar
    }

    /// The skeleton node playing human bone `human` (Unity's name for it:
    /// `Hips`, `LeftUpperArm`, `Left Index Proximal`, ...).
    #[must_use]
    pub fn bone(&self, human: &str) -> Option<&str> {
        self.bones.get(human).map(String::as_str)
    }

    /// Says skeleton node `node` plays human bone `human`.
    ///
    /// # Errors
    ///
    /// When `human` isn't one of Unity's human bone names.
    pub fn set_bone(&mut self, human: &str, node: &str) -> Result<(), Error> {
        if !is_bone(human) {
            return Err(Error::invalid(format!("{human:?} is not a Unity human bone")));
        }
        self.bones.insert(human.to_owned(), node.to_owned());
        Ok(())
    }

    /// The required human bones (hips, spine, head, and both arms and legs
    /// down to the hands and feet) the avatar has no node for.
    ///
    /// ```
    /// let avatar = ponko_unity::HumanoidAvatar::new();
    /// assert_eq!(avatar.missing().len(), 15);
    /// ```
    #[must_use]
    pub fn missing(&self) -> Vec<&'static str> {
        REQUIRED.into_iter().filter(|b| !self.bones.contains_key(*b)).collect()
    }

    /// Changes a muscle's limits, in degrees: the angle at -1 (`min`) and at
    /// 1 (`max`), as an avatar's Muscles & Settings do.
    ///
    /// # Errors
    ///
    /// When `muscle` isn't one of Unity's muscle names, or `min > max`.
    pub fn set_limits(&mut self, muscle: &str, min: f32, max: f32) -> Result<(), Error> {
        if !is_muscle(muscle) {
            return Err(Error::invalid(format!("{muscle:?} is not a Unity muscle")));
        }
        if min.is_nan() || max.is_nan() || min > max {
            return Err(Error::invalid(format!("{muscle}: limits {min} to {max}")));
        }
        self.limits.insert(muscle.to_owned(), (min, max));
        Ok(())
    }

    /// Says what value muscle `muscle` has in the skeleton's rest pose.
    /// The importer takes rest poses as T-poses and knows Unity's values
    /// for those: arm down-up 0.4, arm front-back 0.3, forearm stretch 1,
    /// upper leg front-back 0.6, lower leg stretch 1, the rest 0. A rig
    /// that rests in another pose (an A-pose) sets its own here.
    ///
    /// ```
    /// let mut avatar = ponko_unity::HumanoidAvatar::new();
    /// avatar.set_rest_muscle("Left Arm Down-Up", 0.0)?; // arms rest lower
    /// # Ok::<(), ponko_unity::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// When `muscle` isn't one of Unity's muscle names.
    pub fn set_rest_muscle(&mut self, muscle: &str, value: f32) -> Result<(), Error> {
        if !is_muscle(muscle) {
            return Err(Error::invalid(format!("{muscle:?} is not a Unity muscle")));
        }
        self.rest.insert(muscle.to_owned(), value);
        Ok(())
    }

    /// The avatar applied to `skin`: each mapped bone's node and how its
    /// muscles turn it. Bones whose node `skin` lacks are left out.
    pub(crate) fn bind(&self, skin: &MeshSkin) -> Bound {
        let index: HashMap<&str, usize> = skin
            .nodes
            .iter()
            .enumerate()
            .rev()
            .map(|(i, n)| (n.name.as_str(), i))
            .collect();
        let node_of = |human: &str| self.bones.get(human).and_then(|n| index.get(n.as_str()).copied());
        // The rest pose: the avatar's reference rotations where it has them.
        let rest: Vec<Mat4> = skin
            .nodes
            .iter()
            .map(|n| {
                let (s, r, t) = Mat4::from_cols_array(&n.matrix).to_scale_rotation_translation();
                let r = self.skeleton.get(&n.name).copied().unwrap_or(r);
                Mat4::from_scale_rotation_translation(s, r, t)
            })
            .collect();
        let mut model = vec![Mat4::IDENTITY; rest.len()];
        let mut done = vec![false; rest.len()];
        fn model_of(
            i: usize,
            skin: &MeshSkin,
            rest: &[Mat4],
            model: &mut [Mat4],
            done: &mut [bool],
            depth: usize,
        ) -> Mat4 {
            if done[i] {
                return model[i];
            }
            let parent = match skin.nodes[i].parent {
                Some(p) if p < rest.len() && p != i && depth < rest.len() => {
                    model_of(p, skin, rest, model, done, depth + 1)
                }
                _ => Mat4::IDENTITY,
            };
            model[i] = parent * rest[i];
            done[i] = true;
            model[i]
        }
        for i in 0..rest.len() {
            model_of(i, skin, &rest, &mut model, &mut done, 0);
        }
        let place = |i: usize| model[i].transform_point3(Vec3::ZERO);
        let mut bound = Bound {
            rest,
            joints: Vec::new(),
            muscles: Vec::new(),
            hips: None,
        };
        for (human, along, fallback) in bones() {
            let Some(node) = node_of(&human) else {
                continue;
            };
            // The bone's direction: towards the next bone along it, or the
            // node's own child.
            let to = along
                .iter()
                .find_map(|b| node_of(b))
                .or_else(|| skin.nodes.iter().position(|n| n.parent == Some(node)))
                .map(|c| place(c) - place(node))
                .filter(|d| d.length_squared() > 1e-10)
                .map_or(Vec3::from(fallback).normalize(), Vec3::normalize);
            let rotation = model[node].to_scale_rotation_translation().1;
            let joint = bound.joints.len();
            bound.joints.push(Joint { node, rotation });
            if human == "Hips" {
                let parent = skin.nodes[node].parent.filter(|&p| p < model.len());
                let height = place(node).y;
                bound.hips = Some(Hips {
                    joint,
                    parent: parent.map_or(Mat4::IDENTITY, |p| model[p]),
                    scale: if height > 1e-4 { height } else { 1.0 },
                });
            }
            for (name, bone, motion, min, max, rest) in muscles() {
                if bone != human {
                    continue;
                }
                let axis = match motion {
                    Swing(d) => to.cross(Vec3::from(d)),
                    Twist(r, d) => to * to.dot(Vec3::from(r).cross(Vec3::from(d))).signum(),
                };
                if axis.length_squared() < 1e-8 {
                    continue;
                }
                let (min, max) = self.limits.get(&name).copied().unwrap_or((min, max));
                let rest = self.rest.get(&name).copied().unwrap_or(rest);
                let twist = matches!(motion, Twist(..));
                bound.muscles.push(BoundMuscle {
                    name,
                    joint,
                    twist,
                    axis: axis.normalize(),
                    min,
                    max,
                    rest,
                });
            }
        }
        // Each bone's twist first, then its swings, in the table's order.
        bound.muscles.sort_by_key(|m| (m.joint, !m.twist));
        bound
    }
}

/// A human bone's node and its rest rotation in model space.
pub(crate) struct Joint {
    pub node: usize,
    rotation: Quat,
}

/// A muscle bound to a joint: the axis it turns about (model space, at
/// rest) and its limits.
pub(crate) struct BoundMuscle {
    pub name: String,
    joint: usize,
    twist: bool,
    axis: Vec3,
    min: f32,
    max: f32,
    rest: f32,
}

/// Where the body's position and rotation go: the hips.
pub(crate) struct Hips {
    joint: usize,
    /// The hips' parent in model space, at rest.
    parent: Mat4,
    /// Unity's human scale: the hips' height at rest.
    scale: f32,
}

/// A [`HumanoidAvatar`] bound to a skin.
pub(crate) struct Bound {
    /// Every node's rest matrix (local, Unity space).
    pub rest: Vec<Mat4>,
    pub joints: Vec<Joint>,
    pub muscles: Vec<BoundMuscle>,
    pub hips: Option<Hips>,
}

impl BoundMuscle {
    /// The angle, in radians, at `value`, from the rest pose.
    fn angle(&self, value: f32) -> f32 {
        let degrees = |v: f32| if v >= 0.0 { v * self.max } else { -v * self.min };
        (degrees(value) - degrees(self.rest)).to_radians()
    }
}

impl Bound {
    /// Each joint's local matrix for a pose: `muscle(name)` is the value of
    /// a muscle (0 without a curve); `body` the body's position (Unity's
    /// `RootT`, in human scale) and rotation (`RootQ`), when the clip has
    /// them.
    pub(crate) fn pose(
        &self,
        muscle: impl Fn(&str) -> f32,
        body: (Option<Vec3>, Option<Quat>),
        out: &mut Vec<(usize, Mat4)>,
    ) {
        out.clear();
        // Each joint's turn, in model space at rest.
        let mut turn = vec![Quat::IDENTITY; self.joints.len()];
        for m in &self.muscles {
            let angle = m.angle(muscle(&m.name));
            if angle != 0.0 {
                turn[m.joint] = Quat::from_axis_angle(m.axis, angle) * turn[m.joint];
            }
        }
        for (k, j) in self.joints.iter().enumerate() {
            let (s, r, mut t) = self.rest[j.node].to_scale_rotation_translation();
            // Turned in model space, seen in the bone's own rest frame.
            let mut local = r * (j.rotation.inverse() * turn[k] * j.rotation);
            if let Some(h) = self.hips.as_ref().filter(|h| h.joint == k) {
                let parent = h.parent.to_scale_rotation_translation().1;
                if let Some(q) = body.1 {
                    // The body's rotation turns the hips from their rest.
                    local = parent.inverse() * q.normalize() * j.rotation;
                }
                if let Some(p) = body.0 {
                    t = h.parent.inverse().transform_point3(p * h.scale);
                }
            }
            out.push((j.node, Mat4::from_scale_rotation_translation(s, local.normalize(), t)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_muscle_has_a_bone_and_both_sides_are_there() {
        let all: Vec<_> = muscles().collect();
        // Unity's 95 muscles: 17 down the body's middle and 39 on each side.
        assert_eq!(all.len(), 95);
        for (name, bone, ..) in &all {
            assert!(is_bone(bone), "{name}: {bone}");
        }
        assert!(is_muscle("RightHand.Little.3 Stretched"));
        assert!(is_muscle("Right Upper Leg Twist In-Out"));
        assert!(is_bone("Right Thumb Distal"));
        assert_eq!(bones().count(), 55);
    }

    #[test]
    fn limits_scale_each_side_of_zero() {
        let m = BoundMuscle {
            name: String::new(),
            joint: 0,
            twist: false,
            axis: Vec3::X,
            min: -60.0,
            max: 100.0,
            rest: 0.4,
        };
        assert!((m.angle(0.4)).abs() < 1e-6, "the rest value is the rest pose");
        assert!((m.angle(1.0) - 60f32.to_radians()).abs() < 1e-5);
        assert!((m.angle(-1.0) - (-100f32).to_radians()).abs() < 1e-5);
    }
}
