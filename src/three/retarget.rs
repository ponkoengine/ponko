//! Humanoid retargeting: a clip made for one humanoid skeleton, played on
//! another with different proportions, joint names and joint axes.
use std::fmt;

use glam::{Mat4, Quat, Vec3, Vec4};

use super::{AnimationClip, AnimationTrack, Interpolation, JointAnimation, Skeleton, Transform3D, two_bone_ik};

macro_rules! human_bones {
    ($($bone:ident = $name:literal,)*) => {
        /// A bone of a humanoid skeleton, for [`HumanoidMap`]. Fingers have
        /// three segments each, from the hand outwards.
        ///
        /// ```
        /// use ponko::anim::HumanBone;
        /// assert_eq!(HumanBone::LeftUpperLeg.name(), "left upper leg");
        /// assert!(HumanBone::Hips.is_required());
        /// assert!(!HumanBone::Chest.is_required());
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum HumanBone {
            $(#[doc = concat!("The ", $name, ".")] $bone,)*
        }
        impl HumanBone {
            /// Every bone, in hierarchy order (parents before children).
            pub const ALL: &'static [HumanBone] = &[$(HumanBone::$bone,)*];
            /// Its name in words, such as `"left upper leg"`.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $(HumanBone::$bone => $name,)*
                }
            }
        }
    };
}

human_bones! {
    Root = "root",
    Hips = "hips",
    Spine = "spine",
    Chest = "chest",
    UpperChest = "upper chest",
    Neck = "neck",
    Head = "head",
    LeftShoulder = "left shoulder",
    LeftUpperArm = "left upper arm",
    LeftLowerArm = "left lower arm",
    LeftHand = "left hand",
    RightShoulder = "right shoulder",
    RightUpperArm = "right upper arm",
    RightLowerArm = "right lower arm",
    RightHand = "right hand",
    LeftUpperLeg = "left upper leg",
    LeftLowerLeg = "left lower leg",
    LeftFoot = "left foot",
    LeftToes = "left toes",
    RightUpperLeg = "right upper leg",
    RightLowerLeg = "right lower leg",
    RightFoot = "right foot",
    RightToes = "right toes",
    LeftThumbProximal = "left thumb proximal",
    LeftThumbIntermediate = "left thumb intermediate",
    LeftThumbDistal = "left thumb distal",
    LeftIndexProximal = "left index proximal",
    LeftIndexIntermediate = "left index intermediate",
    LeftIndexDistal = "left index distal",
    LeftMiddleProximal = "left middle proximal",
    LeftMiddleIntermediate = "left middle intermediate",
    LeftMiddleDistal = "left middle distal",
    LeftRingProximal = "left ring proximal",
    LeftRingIntermediate = "left ring intermediate",
    LeftRingDistal = "left ring distal",
    LeftLittleProximal = "left little proximal",
    LeftLittleIntermediate = "left little intermediate",
    LeftLittleDistal = "left little distal",
    RightThumbProximal = "right thumb proximal",
    RightThumbIntermediate = "right thumb intermediate",
    RightThumbDistal = "right thumb distal",
    RightIndexProximal = "right index proximal",
    RightIndexIntermediate = "right index intermediate",
    RightIndexDistal = "right index distal",
    RightMiddleProximal = "right middle proximal",
    RightMiddleIntermediate = "right middle intermediate",
    RightMiddleDistal = "right middle distal",
    RightRingProximal = "right ring proximal",
    RightRingIntermediate = "right ring intermediate",
    RightRingDistal = "right ring distal",
    RightLittleProximal = "right little proximal",
    RightLittleIntermediate = "right little intermediate",
    RightLittleDistal = "right little distal",
}

use HumanBone as B;

/// Per side: shoulder, upper arm, lower arm, hand, upper leg, lower leg, foot, toes.
const LIMBS: [[HumanBone; 8]; 2] = [
    [
        B::LeftShoulder,
        B::LeftUpperArm,
        B::LeftLowerArm,
        B::LeftHand,
        B::LeftUpperLeg,
        B::LeftLowerLeg,
        B::LeftFoot,
        B::LeftToes,
    ],
    [
        B::RightShoulder,
        B::RightUpperArm,
        B::RightLowerArm,
        B::RightHand,
        B::RightUpperLeg,
        B::RightLowerLeg,
        B::RightFoot,
        B::RightToes,
    ],
];
/// Per side, per finger (thumb, index, middle, ring, little), three segments.
const FINGERS: [[[HumanBone; 3]; 5]; 2] = [
    [
        [B::LeftThumbProximal, B::LeftThumbIntermediate, B::LeftThumbDistal],
        [B::LeftIndexProximal, B::LeftIndexIntermediate, B::LeftIndexDistal],
        [B::LeftMiddleProximal, B::LeftMiddleIntermediate, B::LeftMiddleDistal],
        [B::LeftRingProximal, B::LeftRingIntermediate, B::LeftRingDistal],
        [B::LeftLittleProximal, B::LeftLittleIntermediate, B::LeftLittleDistal],
    ],
    [
        [B::RightThumbProximal, B::RightThumbIntermediate, B::RightThumbDistal],
        [B::RightIndexProximal, B::RightIndexIntermediate, B::RightIndexDistal],
        [B::RightMiddleProximal, B::RightMiddleIntermediate, B::RightMiddleDistal],
        [B::RightRingProximal, B::RightRingIntermediate, B::RightRingDistal],
        [B::RightLittleProximal, B::RightLittleIntermediate, B::RightLittleDistal],
    ],
];
/// Hips, spine, head and the arms and legs down to hands and feet.
const REQUIRED: [HumanBone; 15] = [
    B::Hips,
    B::Spine,
    B::Head,
    B::LeftUpperArm,
    B::LeftLowerArm,
    B::LeftHand,
    B::RightUpperArm,
    B::RightLowerArm,
    B::RightHand,
    B::LeftUpperLeg,
    B::LeftLowerLeg,
    B::LeftFoot,
    B::RightUpperLeg,
    B::RightLowerLeg,
    B::RightFoot,
];

impl HumanBone {
    /// Whether retargeting needs this bone mapped on both skeletons: the
    /// hips, spine, head, and each arm and leg down to the hand and foot.
    #[must_use]
    pub fn is_required(self) -> bool {
        REQUIRED.contains(&self)
    }
}

/// Which joint of a skeleton plays each [`HumanBone`]. Start from
/// [`detect`](Self::detect), which reads common joint naming schemes, then
/// fix what it got wrong with [`set`](Self::set).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{HumanBone, HumanoidMap, Joint, Skeleton};
/// let joint = |name: &str, parent| Joint { name: name.into(), parent, rest: ponko::d3::Transform::default() };
/// let s = Skeleton::new(vec![joint("mixamorig:Hips", None), joint("mixamorig:LeftUpLeg", Some(0)), joint("tail", Some(0))]);
/// let mut map = HumanoidMap::detect(&s);
/// assert_eq!(map.get(HumanBone::Hips), Some(0));
/// assert_eq!(map.get(HumanBone::LeftUpperLeg), Some(1));
/// map.set(HumanBone::Spine, s.find("tail"));
/// assert_eq!(map.get(HumanBone::Spine), Some(2));
/// assert!(map.missing().any(|b| b == HumanBone::Head));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanoidMap {
    joints: Vec<Option<usize>>,
}

impl Default for HumanoidMap {
    fn default() -> Self {
        Self {
            joints: vec![None; HumanBone::ALL.len()],
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Root,
    Hips,
    Spine,
    Chest,
    UpperChest,
    Neck,
    Head,
    Limb(usize, usize),
    Finger(usize, usize),
}

/// Splits a joint name into lowercase words at separators, case changes and
/// digits, after any `namespace:` or `parent|` prefix.
fn words(name: &str) -> Vec<String> {
    let name = name.rsplit([':', '|']).next().unwrap_or(name);
    let mut out: Vec<String> = Vec::new();
    let mut prev: Option<char> = None;
    for c in name.chars() {
        if !c.is_alphanumeric() {
            prev = None;
            continue;
        }
        let split = match prev {
            None => true,
            Some(p) => (p.is_lowercase() && c.is_uppercase()) || (p.is_ascii_digit() != c.is_ascii_digit()),
        };
        if split {
            out.push(String::new());
        }
        out.last_mut().unwrap().extend(c.to_lowercase());
        prev = Some(c);
    }
    out
}

fn classify(name: &str) -> Option<Class> {
    const NOISE: &[&str] = &[
        "mixamorig",
        "bip",
        "j",
        "def",
        "jnt",
        "joint",
        "bn",
        "bone",
        "b",
        "c",
        "center",
        "armature",
        "sk",
        "org",
    ];
    let mut side = None;
    let mut key = String::new();
    for w in words(name) {
        match w.as_str() {
            "l" | "left" => side = Some(0),
            "r" | "right" => side = Some(1),
            w if NOISE.contains(&w) || w.chars().all(|c| c.is_ascii_digit()) => {}
            w => key.push_str(w),
        }
    }
    let Some(side) = side else {
        return match key.as_str() {
            "root" => Some(Class::Root),
            "hips" | "hip" | "pelvis" => Some(Class::Hips),
            "spine" => Some(Class::Spine),
            "chest" => Some(Class::Chest),
            "upperchest" => Some(Class::UpperChest),
            "neck" => Some(Class::Neck),
            "head" => Some(Class::Head),
            _ => None,
        };
    };
    let limb = match key.as_str() {
        "shoulder" | "clavicle" | "clav" | "collar" | "collarbone" => Some(0),
        "upperarm" | "uparm" | "arm" => Some(1),
        "lowerarm" | "lowarm" | "forearm" | "elbow" => Some(2),
        "hand" | "wrist" => Some(3),
        "upperleg" | "upleg" | "thigh" | "hip" => Some(4),
        "lowerleg" | "lowleg" | "leg" | "calf" | "shin" | "knee" => Some(5),
        "foot" | "ankle" => Some(6),
        "toes" | "toe" | "toebase" | "ball" => Some(7),
        _ => None,
    };
    if let Some(limb) = limb {
        return Some(Class::Limb(side, limb));
    }
    if key.contains("metacarpal") {
        return None;
    }
    let mut finger = key.strip_prefix("hand").unwrap_or(&key).replace("finger", "");
    for segment in ["proximal", "intermediate", "medial", "distal"] {
        if let Some(f) = finger.strip_suffix(segment) {
            finger = f.to_string();
        }
    }
    let finger = match finger.as_str() {
        "thumb" => 0,
        "index" => 1,
        "middle" => 2,
        "ring" => 3,
        "little" | "pinky" | "pinkie" => 4,
        _ => return None,
    };
    Some(Class::Finger(side, finger))
}

fn is_ancestor(s: &Skeleton, ancestor: usize, mut joint: usize) -> bool {
    while let Some(p) = s.joints()[joint].parent {
        if p == ancestor {
            return true;
        }
        joint = p;
    }
    false
}

impl HumanoidMap {
    /// An empty map: no bone mapped.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Maps bones by joint name. It understands the usual schemes: words
    /// such as `hips`/`pelvis`, `thigh`/`upper leg`/`up leg`, `calf`/`shin`/`leg`,
    /// `clavicle`/`shoulder`, `forearm`/`lower arm`, finger names with
    /// segment numbers; sides as `left`/`right` or a separate `l`/`r`
    /// (`LeftUpLeg`, `thigh_l`, `UpperLeg.L`, `L Thigh`); and prefixes such
    /// as `name:` or `armature|`. When several joints match, the one
    /// nearest the top of the hierarchy wins. A chain of plain `spine`
    /// joints maps its first to [`Spine`](HumanBone::Spine), and with three
    /// or more, its last to [`UpperChest`](HumanBone::UpperChest) and its
    /// middle one to [`Chest`](HumanBone::Chest). Check
    /// [`missing`](Self::missing) afterwards.
    #[must_use]
    pub fn detect(skeleton: &Skeleton) -> Self {
        let joints = skeleton.joints();
        let mut depth = vec![0usize; joints.len()];
        for (i, j) in joints.iter().enumerate() {
            if let Some(p) = j.parent {
                depth[i] = depth[p] + 1;
            }
        }
        let mut found: Vec<(usize, usize, Class)> = joints
            .iter()
            .enumerate()
            .filter_map(|(i, j)| classify(&j.name).map(|c| (depth[i], i, c)))
            .collect();
        found.sort_by_key(|&(d, i, _)| (d, i));
        let first = |class: Class| found.iter().find(|f| f.2 == class).map(|f| f.1);
        let mut map = Self::default();
        let hips = first(Class::Hips);
        map.set(B::Hips, hips);
        if let (Some(root), Some(hips)) = (first(Class::Root), hips)
            && is_ancestor(skeleton, root, hips)
        {
            map.set(B::Root, Some(root));
        }
        let below_hips = |j: usize| hips.is_none_or(|h| is_ancestor(skeleton, h, j));
        let pick = |class: Class| found.iter().find(|f| f.2 == class && below_hips(f.1)).map(|f| f.1);
        let spines: Vec<usize> = found
            .iter()
            .filter(|f| f.2 == Class::Spine && below_hips(f.1))
            .map(|f| f.1)
            .collect();
        map.set(B::Spine, spines.first().copied());
        let (chest, upper) = (pick(Class::Chest), pick(Class::UpperChest));
        if chest.is_some() || upper.is_some() {
            map.set(B::Chest, chest);
            map.set(B::UpperChest, upper);
        } else if spines.len() == 2 {
            map.set(B::Chest, Some(spines[1]));
        } else if spines.len() >= 3 {
            map.set(B::Chest, Some(spines[spines.len() / 2]));
            map.set(B::UpperChest, spines.last().copied());
        }
        map.set(B::Neck, pick(Class::Neck));
        map.set(B::Head, pick(Class::Head));
        for side in 0..2 {
            for (limb, bone) in LIMBS[side].iter().enumerate() {
                map.set(*bone, pick(Class::Limb(side, limb)));
            }
            for (finger, segments) in FINGERS[side].iter().enumerate() {
                let chain = found.iter().filter(|f| f.2 == Class::Finger(side, finger)).map(|f| f.1);
                for (bone, joint) in segments.iter().zip(chain) {
                    map.set(*bone, Some(joint));
                }
            }
        }
        map
    }

    /// The joint mapped to `bone`, if any.
    #[must_use]
    pub fn get(&self, bone: HumanBone) -> Option<usize> {
        self.joints[bone as usize]
    }

    /// Maps `bone` to `joint`, or unmaps it with `None`.
    pub fn set(&mut self, bone: HumanBone, joint: Option<usize>) {
        self.joints[bone as usize] = joint;
    }

    /// The [required](HumanBone::is_required) bones that aren't mapped.
    pub fn missing(&self) -> impl Iterator<Item = HumanBone> + '_ {
        REQUIRED.iter().copied().filter(|b| self.get(*b).is_none())
    }

    fn check(&self, s: &Skeleton, source: bool) -> Result<(), RetargetError> {
        if let Some(bone) = self.missing().next() {
            return Err(RetargetError::MissingBone { source, bone });
        }
        for &bone in HumanBone::ALL {
            if let Some(joint) = self.get(bone)
                && joint >= s.joints().len()
            {
                return Err(RetargetError::InvalidJoint { source, bone, joint });
            }
        }
        Ok(())
    }
}

/// Why a [`Retargeter`] couldn't be made.
///
/// ```
/// use ponko::anim::{HumanBone, HumanoidMap, Retargeter, RetargetError, Skeleton};
/// let s = Skeleton::new(vec![]);
/// let err = Retargeter::new(&s, &HumanoidMap::new(), &s, &HumanoidMap::new()).unwrap_err();
/// assert_eq!(err, RetargetError::MissingBone { source: true, bone: HumanBone::Hips });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RetargetError {
    /// A [required](HumanBone::is_required) bone isn't mapped.
    MissingBone {
        /// On the source skeleton (otherwise the target).
        source: bool,
        /// The bone.
        bone: HumanBone,
    },
    /// A bone is mapped to a joint the skeleton doesn't have.
    InvalidJoint {
        /// On the source skeleton (otherwise the target).
        source: bool,
        /// The bone.
        bone: HumanBone,
        /// The joint index it names.
        joint: usize,
    },
}

impl fmt::Display for RetargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let side = |source: bool| if source { "source" } else { "target" };
        match self {
            Self::MissingBone { source, bone } => {
                write!(
                    f,
                    "the {} skeleton has no joint mapped to the {}",
                    side(*source),
                    bone.name()
                )
            }
            Self::InvalidJoint { source, bone, joint } => write!(
                f,
                "the {} is mapped to joint {joint}, which the {} skeleton doesn't have",
                bone.name(),
                side(*source)
            ),
        }
    }
}

impl std::error::Error for RetargetError {}

#[derive(Debug, Clone)]
struct Pair {
    source: usize,
    target: usize,
    /// Inverse of the source joint's rest rotation in model space.
    source_rest_inverse: Quat,
    /// The target joint's rest rotation in model space.
    target_rest: Quat,
    /// Hips and root also follow the source's (scaled) movement.
    translate: bool,
    source_rest_position: Vec3,
    target_rest_position: Vec3,
}

#[derive(Debug, Clone)]
struct Leg {
    /// Target upper leg, lower leg and foot.
    chain: [usize; 3],
    /// Index of the foot's pair.
    foot_pair: usize,
    source_foot: usize,
    target_toes: Option<usize>,
    source_rest: Vec3,
    target_rest: Vec3,
}

/// Plays clips made for one humanoid skeleton on another: different
/// proportions, joint names and joint axes. Make one per pair of
/// skeletons, then [`retarget`](Self::retarget) each clip once (at load)
/// into an ordinary [`AnimationClip`] for the target, which works
/// everywhere a clip does (blend trees, layers, root motion), or call
/// [`retarget_pose`](Self::retarget_pose) every frame to copy a live pose.
///
/// Each mapped bone turns, in model space, by as much as the source bone
/// turned from its rest pose; both skeletons' rest poses should therefore
/// be the same reference pose (both T-poses, or the same A-pose), with the
/// same up and forward axes in model space. The hips (and the root, if both
/// have one) move by the source's movement times [`scale`](Self::scale),
/// the ratio of the leg lengths, so strides and root motion fit the target.
/// With [`foot_ik`](Self::foot_ik), each foot is then placed by two-bone IK
/// where the scaled source foot is, so planted feet stay planted and feet
/// keep their height above the ground even when the thigh-to-shin ratio
/// differs.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::anim::{HumanoidMap, Joint, Retargeter, Skeleton};
/// // Two tiny humanoids: one half the size of the other, with other names.
/// fn humanoid(names: [&str; 15], size: f32) -> Skeleton {
///     let at = [(0., 1., 0.), (0., 0.1, 0.), (0., 0.5, 0.), (0.2, 0.4, 0.), (0.3, 0., 0.), (0.3, 0., 0.),
///         (-0.2, 0.4, 0.), (-0.3, 0., 0.), (-0.3, 0., 0.), (0.1, -0.05, 0.), (0., -0.45, 0.), (0., -0.45, 0.),
///         (-0.1, -0.05, 0.), (0., -0.45, 0.), (0., -0.45, 0.)];
///     let parents = [None, Some(0), Some(1), Some(1), Some(3), Some(4), Some(1), Some(6), Some(7), Some(0), Some(9), Some(10), Some(0), Some(12), Some(13)];
///     Skeleton::new((0..15).map(|i| Joint { name: names[i].into(), parent: parents[i],
///         rest: ponko::d3::Transform::at(Vec3::from(at[i]) * size) }).collect())
/// }
/// let a = humanoid(["Hips", "Spine", "Head", "LeftArm", "LeftForeArm", "LeftHand", "RightArm", "RightForeArm", "RightHand",
///     "LeftUpLeg", "LeftLeg", "LeftFoot", "RightUpLeg", "RightLeg", "RightFoot"], 1.0);
/// let b = humanoid(["pelvis", "spine_01", "head", "upperarm_l", "lowerarm_l", "hand_l", "upperarm_r", "lowerarm_r", "hand_r",
///     "thigh_l", "calf_l", "foot_l", "thigh_r", "calf_r", "foot_r"], 0.5);
/// let mut retarget = Retargeter::new(&a, &HumanoidMap::detect(&a), &b, &HumanoidMap::detect(&b)).unwrap();
/// assert!((retarget.scale() - 0.5).abs() < 1e-5);
/// let idle = ponko::anim::AnimationClip::new("idle", &a, vec![]);
/// let idle_b = retarget.retarget(&idle);
/// assert_eq!(idle_b.name(), "idle");
/// ```
#[derive(Debug, Clone)]
pub struct Retargeter {
    source: Skeleton,
    target: Skeleton,
    pairs: Vec<Pair>,
    /// Target joint index to pair index.
    target_pair: Vec<Option<usize>>,
    legs: Vec<Leg>,
    scale: f32,
    /// Place the feet with two-bone IK where the scaled source feet are.
    /// On by default.
    pub foot_ik: bool,
    /// Keys per second that [`retarget`](Self::retarget) adds where the
    /// source clip's keys are further apart. Default 30.
    pub sample_rate: f32,
    source_globals: Vec<Mat4>,
    target_globals: Vec<Mat4>,
    desired: Vec<Quat>,
}

fn rest_globals(s: &Skeleton) -> Vec<Mat4> {
    let mut out = Vec::new();
    globals(s.joints().iter().map(|j| &j.rest), s, &mut out);
    out
}

fn globals<'a>(locals: impl Iterator<Item = &'a Transform3D>, s: &Skeleton, out: &mut Vec<Mat4>) {
    out.clear();
    for (local, joint) in locals.zip(s.joints()) {
        let m = local.matrix();
        let m = joint.parent.map_or(m, |p| out[p] * m);
        out.push(m);
    }
}

fn rotation(m: &Mat4) -> Quat {
    m.to_scale_rotation_translation().1.normalize()
}

fn position(m: &Mat4) -> Vec3 {
    m.w_axis.truncate()
}

fn leg_length(rest: &[Mat4], map: &HumanoidMap) -> f32 {
    let at = |b: HumanBone| position(&rest[map.get(b).unwrap()]);
    let leg = |u, l, f| at(u).distance(at(l)) + at(l).distance(at(f));
    0.5 * (leg(B::LeftUpperLeg, B::LeftLowerLeg, B::LeftFoot) + leg(B::RightUpperLeg, B::RightLowerLeg, B::RightFoot))
}

impl Retargeter {
    /// Prepares retargeting from `source` to `target`, each with its map.
    /// Bones mapped on only one side are left alone.
    ///
    /// # Errors
    ///
    /// If a [required](HumanBone::is_required) bone isn't mapped on either
    /// skeleton, or a map names a joint its skeleton doesn't have.
    pub fn new(
        source: &Skeleton,
        source_map: &HumanoidMap,
        target: &Skeleton,
        target_map: &HumanoidMap,
    ) -> Result<Self, RetargetError> {
        source_map.check(source, true)?;
        target_map.check(target, false)?;
        let source_rest = rest_globals(source);
        let target_rest = rest_globals(target);
        let mut pairs: Vec<(HumanBone, Pair)> = HumanBone::ALL
            .iter()
            .filter_map(|&bone| {
                let (s, t) = (source_map.get(bone)?, target_map.get(bone)?);
                Some((
                    bone,
                    Pair {
                        source: s,
                        target: t,
                        source_rest_inverse: rotation(&source_rest[s]).inverse(),
                        target_rest: rotation(&target_rest[t]),
                        translate: matches!(bone, B::Root | B::Hips),
                        source_rest_position: position(&source_rest[s]),
                        target_rest_position: position(&target_rest[t]),
                    },
                ))
            })
            .collect();
        pairs.sort_by_key(|(_, p)| p.target);
        pairs.dedup_by_key(|(_, p)| p.target);
        let mut target_pair = vec![None; target.joints().len()];
        for (i, (_, p)) in pairs.iter().enumerate() {
            target_pair[p.target] = Some(i);
        }
        let pair_of = |bone: HumanBone| pairs.iter().position(|(b, _)| *b == bone);
        let legs = [
            [B::LeftUpperLeg, B::LeftLowerLeg, B::LeftFoot, B::LeftToes],
            [B::RightUpperLeg, B::RightLowerLeg, B::RightFoot, B::RightToes],
        ]
        .iter()
        .filter_map(|&[u, l, f, toes]| {
            let foot_pair = pair_of(f)?;
            let chain = [target_map.get(u)?, target_map.get(l)?, target_map.get(f)?];
            let source_foot = source_map.get(f)?;
            Some(Leg {
                chain,
                foot_pair,
                source_foot,
                target_toes: target_map.get(toes),
                source_rest: position(&source_rest[source_foot]),
                target_rest: position(&target_rest[chain[2]]),
            })
        })
        .collect();
        let (from, to) = (
            leg_length(&source_rest, source_map),
            leg_length(&target_rest, target_map),
        );
        let scale = if from > 1e-6 && to > 1e-6 { to / from } else { 1.0 };
        let count = pairs.len();
        Ok(Self {
            source: source.clone(),
            target: target.clone(),
            pairs: pairs.into_iter().map(|(_, p)| p).collect(),
            target_pair,
            legs,
            scale,
            foot_ik: true,
            sample_rate: 30.0,
            source_globals: Vec::new(),
            target_globals: Vec::new(),
            desired: vec![Quat::IDENTITY; count],
        })
    }

    /// How much further the target moves than the source: its leg length
    /// over the source's, from upper leg to foot at rest.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// Poses the mapped joints of `target` like `source` is posed now,
    /// leaving its other joints as they are. Allocates nothing after the
    /// first call. Use it every frame to follow a live skeleton; for clips,
    /// [`retarget`](Self::retarget) once instead.
    ///
    /// # Panics
    ///
    /// If either skeleton has fewer joints than the one this retargeter was
    /// made for.
    pub fn retarget_pose(&mut self, source: &Skeleton, target: &mut Skeleton) {
        assert!(
            source.joints().len() >= self.source.joints().len() && target.joints().len() >= self.target.joints().len(),
            "Retargeter::retarget_pose: skeletons have fewer joints than the ones it was made for"
        );
        let rest = source.joints().iter().map(|j| &j.rest);
        globals(
            source.pose.iter().chain(rest.skip(source.pose.len())),
            source,
            &mut self.source_globals,
        );
        let n = self.target.joints().len();
        if target.pose.len() < n {
            let have = target.pose.len();
            target.pose.extend(self.target.joints()[have..].iter().map(|j| j.rest));
        }
        self.target_globals.clear();
        for i in 0..n {
            let parent = self.target.joints()[i]
                .parent
                .map_or(Mat4::IDENTITY, |p| self.target_globals[p]);
            if let Some(k) = self.target_pair[i] {
                let pair = &self.pairs[k];
                let from = &self.source_globals[pair.source];
                let desired = (rotation(from) * pair.source_rest_inverse * pair.target_rest).normalize();
                self.desired[k] = desired;
                target.pose[i].rotation = (rotation(&parent).inverse() * desired).normalize();
                if pair.translate && parent.determinant().abs() > 1e-12 {
                    let at = pair.target_rest_position + (position(from) - pair.source_rest_position) * self.scale;
                    target.pose[i].position = parent.inverse().transform_point3(at);
                }
            }
            self.target_globals.push(parent * target.pose[i].matrix());
        }
        if !self.foot_ik {
            return;
        }
        for leg in &self.legs {
            let [upper, lower, foot] = leg.chain;
            let goal =
                leg.target_rest + (position(&self.source_globals[leg.source_foot]) - leg.source_rest) * self.scale;
            let hip = position(&target.joint_matrix(upper));
            let knee = position(&target.joint_matrix(lower));
            let reach = (goal - hip).normalize_or_zero();
            let bend = (knee - hip) - reach * (knee - hip).dot(reach);
            let pole = if bend.length() > 1e-4 {
                bend
            } else {
                // A straight leg: bend the knee the way the toes point.
                leg.target_toes
                    .map(|t| position(&target.joint_matrix(t)) - position(&target.joint_matrix(foot)))
                    .unwrap_or(Vec3::Z)
            };
            two_bone_ik(target, &Transform3D::default(), leg.chain, goal, pole, 1.0);
            let parent = rotation(&target.joint_matrix(lower));
            target.pose[foot].rotation = (parent.inverse() * self.desired[leg.foot_pair]).normalize();
        }
    }

    /// `clip` (made for the source skeleton) as a clip for the target
    /// skeleton, under the same name. It keys every mapped target joint's
    /// rotation (and the hips' and root's position) at the source's key
    /// times, adding keys so none are more than `1 / sample_rate` seconds
    /// apart, with linear interpolation. Morph tracks are copied as they
    /// are. Do this once per clip, at load: it samples the whole clip.
    pub fn retarget(&mut self, clip: &AnimationClip) -> AnimationClip {
        let times = self.sample_times(clip);
        let mut source = self.source.clone();
        let mut target = self.target.clone();
        let mut rotations: Vec<Vec<Vec4>> = vec![Vec::with_capacity(times.len()); self.pairs.len()];
        let mut positions: Vec<Vec<Vec4>> = vec![Vec::new(); self.pairs.len()];
        for &t in &times {
            source.reset_pose();
            clip.sample(t, &mut source);
            target.reset_pose();
            self.retarget_pose(&source, &mut target);
            for (k, pair) in self.pairs.iter().enumerate() {
                let pose = target.pose[pair.target];
                let mut q = Vec4::from_array(pose.rotation.to_array());
                if rotations[k].last().is_some_and(|last| last.dot(q) < 0.0) {
                    q = -q;
                }
                rotations[k].push(q);
                if pair.translate {
                    positions[k].push(pose.position.extend(0.0));
                }
            }
        }
        let tracks = self
            .pairs
            .iter()
            .zip(rotations.into_iter().zip(positions))
            .map(|(pair, (rotation, position))| JointAnimation {
                joint: pair.target,
                name: self.target.joints()[pair.target].name.clone(),
                translation: pair
                    .translate
                    .then(|| AnimationTrack::new(times.clone(), position, Interpolation::Linear)),
                rotation: Some(AnimationTrack::new(times.clone(), rotation, Interpolation::Linear)),
                scale: None,
            })
            .collect();
        let out = AnimationClip::new(clip.name(), &self.target, tracks);
        if clip.morph_tracks().is_empty() {
            out
        } else {
            out.with_morph_tracks(clip.morph_tracks().to_vec())
        }
    }

    fn sample_times(&self, clip: &AnimationClip) -> Vec<f32> {
        let mut keys: Vec<f32> = clip
            .tracks()
            .iter()
            .flat_map(|t| [&t.translation, &t.rotation, &t.scale])
            .flatten()
            .flat_map(|t| t.times().iter().copied())
            .collect();
        keys.push(0.0);
        keys.sort_by(f32::total_cmp);
        keys.dedup_by(|a, b| *a - *b < 1e-5);
        let step = if self.sample_rate.is_finite() && self.sample_rate > 0.0 {
            1.0 / self.sample_rate
        } else {
            f32::INFINITY
        };
        let mut times = Vec::with_capacity(keys.len());
        for pair in keys.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let extra = ((b - a) / step).ceil().max(1.0) as usize;
            times.extend((0..extra).map(|i| a + (b - a) * i as f32 / extra as f32));
        }
        times.push(*keys.last().unwrap());
        times
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_split_case_digits_and_namespaces() {
        assert_eq!(words("mixamorig:LeftUpLeg"), ["left", "up", "leg"]);
        assert_eq!(words("spine_01"), ["spine", "01"]);
        assert_eq!(words("Bip01 L Thigh"), ["bip", "01", "l", "thigh"]);
        assert_eq!(words("Armature|UpperArm.R"), ["upper", "arm", "r"]);
        assert_eq!(words("J_Bip_L_Index1"), ["j", "bip", "l", "index", "1"]);
    }

    #[test]
    fn twist_end_and_metacarpal_joints_are_not_bones() {
        assert!(classify("upperarm_twist_01_l").is_none());
        assert!(classify("mixamorig:HeadTop_End").is_none());
        assert!(classify("index_metacarpal_l").is_none());
        assert!(classify("tail_01").is_none());
    }
}
