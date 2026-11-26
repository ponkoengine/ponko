//! Humanoid retargeting headless: a walk cycle made for one rig, played on a
//! rig with other names, joint axes and proportions, with its feet planted.
#![cfg(feature = "3d")]

use std::f32::consts::PI;
use std::sync::Arc;

use ponko::anim::{
    AnimationClip, AnimationGraph, AnimationLayer, AnimationParameter, AnimationState, AnimationTrack, Animator,
    HumanBone, HumanoidMap, Interpolation, Joint, JointAnimation, JointMask, Motion, MotionChild, ParamId,
    RetargetError, Retargeter, Skeleton, StateId,
};
use ponko::d3::Transform;
use ponko::prelude::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Counts allocations on the thread that turns tracking on, so other tests
/// running in parallel don't disturb the count.
struct CountingAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
fn record_allocation() {
    if TRACK.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
    }
}
// SAFETY: all allocation and deallocation operations delegate unchanged to System.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocator caller supplies a valid layout.
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record_allocation();
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the allocation and layout come from the allocator caller.
        unsafe { System.dealloc(ptr, layout) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// The largest slide allowed for a planted foot, in metres at the target's size.
const SLIDE_TOLERANCE: f32 = 0.01;
/// How far a planted foot may sit off the target's own ankle height.
const HEIGHT_TOLERANCE: f32 = 0.01;

/// Builds a skeleton from model-space rest positions and rotations, so local
/// joint axes can differ between rigs while the T-pose stays the same.
fn rig(joints: &[(&str, Option<usize>, Vec3, Quat)]) -> Skeleton {
    Skeleton::new(
        joints
            .iter()
            .map(|&(name, parent, at, rotation)| {
                let rest = match parent {
                    None => Transform {
                        position: at,
                        rotation,
                        ..Transform::default()
                    },
                    Some(p) => {
                        let (_, pr, pa) = (joints[p].0, joints[p].3, joints[p].2);
                        Transform {
                            position: pr.inverse() * (at - pa),
                            rotation: pr.inverse() * rotation,
                            ..Transform::default()
                        }
                    }
                };
                Joint::new(name, parent).with_rest(rest)
            })
            .collect(),
    )
}

/// Model-space positions of the source rig: hips at 0.9 m, thigh 0.45 m,
/// shin 0.42 m, ankle 0.08 m above the ground.
fn source() -> Skeleton {
    let q = Quat::IDENTITY;
    let mut joints = vec![
        ("mixamorig:Hips", None, vec3(0.0, 0.9, 0.0), q),
        ("mixamorig:Spine", Some(0), vec3(0.0, 1.0, 0.0), q),
        ("mixamorig:Spine1", Some(1), vec3(0.0, 1.15, 0.0), q),
        ("mixamorig:Spine2", Some(2), vec3(0.0, 1.3, 0.0), q),
        ("mixamorig:Neck", Some(3), vec3(0.0, 1.45, 0.0), q),
        ("mixamorig:Head", Some(4), vec3(0.0, 1.55, 0.0), q),
    ];
    for (side, x) in [("Left", 1.0), ("Right", -1.0)] {
        let s = joints.len();
        let name = |part: &str| -> &'static str { Box::leak(format!("mixamorig:{side}{part}").into_boxed_str()) };
        joints.extend([
            (name("Shoulder"), Some(3), vec3(0.05 * x, 1.4, 0.0), q),
            (name("Arm"), Some(s), vec3(0.15 * x, 1.4, 0.0), q),
            (name("ForeArm"), Some(s + 1), vec3(0.4 * x, 1.4, 0.0), q),
            (name("Hand"), Some(s + 2), vec3(0.65 * x, 1.4, 0.0), q),
            (name("UpLeg"), Some(0), vec3(0.1 * x, 0.95, 0.0), q),
            (name("Leg"), Some(s + 4), vec3(0.1 * x, 0.5, 0.0), q),
            (name("Foot"), Some(s + 5), vec3(0.1 * x, 0.08, 0.0), q),
            (name("ToeBase"), Some(s + 6), vec3(0.1 * x, 0.02, 0.12), q),
        ]);
    }
    rig(&joints)
}

/// A taller rig with Unreal-style names, a root joint, an extra spine joint,
/// twisted joint axes and longer thighs relative to its shins (0.7 m and
/// 0.42 m; ankle 0.1 m up).
fn target() -> Skeleton {
    let twist = |i: u32| {
        Quat::from_rotation_x(0.3 * i as f32)
            * Quat::from_rotation_y(-0.7 + 0.2 * i as f32)
            * Quat::from_rotation_z(0.5)
    };
    let mut joints = vec![
        ("root", None, Vec3::ZERO, Quat::IDENTITY),
        ("pelvis", Some(0), vec3(0.0, 1.24, 0.0), twist(1)),
        ("spine_01", Some(1), vec3(0.0, 1.35, 0.0), twist(2)),
        ("spine_02", Some(2), vec3(0.0, 1.5, 0.0), twist(3)),
        ("spine_03", Some(3), vec3(0.0, 1.65, 0.0), twist(4)),
        ("spine_04", Some(4), vec3(0.0, 1.8, 0.0), twist(5)),
        ("neck_01", Some(5), vec3(0.0, 1.95, 0.0), twist(6)),
        ("head", Some(6), vec3(0.0, 2.05, 0.0), twist(7)),
    ];
    for (side, x) in [("l", 1.0), ("r", -1.0)] {
        let s = joints.len();
        let name = |part: &str| -> &'static str { Box::leak(format!("{part}_{side}").into_boxed_str()) };
        let t = |i: u32| twist(i + if x > 0.0 { 10 } else { 20 });
        joints.extend([
            (name("clavicle"), Some(5), vec3(0.06 * x, 1.85, 0.0), t(0)),
            (name("upperarm"), Some(s), vec3(0.2 * x, 1.85, 0.0), t(1)),
            (name("lowerarm"), Some(s + 1), vec3(0.5 * x, 1.85, 0.0), t(2)),
            (name("hand"), Some(s + 2), vec3(0.8 * x, 1.85, 0.0), t(3)),
            (name("upperarm_twist_01"), Some(s + 1), vec3(0.3 * x, 1.85, 0.0), t(4)),
            (name("thigh"), Some(1), vec3(0.13 * x, 1.22, 0.0), t(5)),
            (name("calf"), Some(s + 5), vec3(0.13 * x, 0.52, 0.0), t(6)),
            (name("foot"), Some(s + 6), vec3(0.13 * x, 0.1, 0.0), t(7)),
            (name("ball"), Some(s + 7), vec3(0.13 * x, 0.03, 0.15), t(8)),
        ]);
    }
    rig(&joints)
}

const SPEED: f32 = 1.0;
const CYCLE: f32 = 1.0;
const HIP_HEIGHT: f32 = 0.85;

fn smoothstep(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

/// Where a foot's ankle is at `t` (model space): planted for the first half
/// of its cycle while the hips walk over it, then lifted and swung ahead.
fn ankle(t: f32, offset: f32, x: f32) -> Vec3 {
    let u = t / CYCLE + offset;
    let (c, p) = (u.floor(), u - u.floor());
    let planted = (c - offset) * SPEED * CYCLE + 0.25;
    if p < 0.5 {
        vec3(x, 0.08, planted)
    } else {
        let s = (p - 0.5) / 0.5;
        vec3(x, 0.08 + 0.1 * (PI * s).sin(), planted + SPEED * CYCLE * smoothstep(s))
    }
}

/// Whether the foot with `offset` is planted (well inside its stance) at `t`.
fn planted(t: f32, offset: f32) -> bool {
    let p = (t / CYCLE + offset).rem_euclid(1.0);
    (0.05..0.45).contains(&p)
}

/// A two-cycle walk for the source rig, solved per key so the planted foot
/// stays put: hips move forward (root motion), legs by planar two-bone IK,
/// arms swing.
fn walk(s: &Skeleton) -> AnimationClip {
    let times: Vec<f32> = (0..=120).map(|i| i as f32 / 60.0).collect();
    let key = |q: Quat| Vec4::from_array(q.to_array());
    let mut tracks = Vec::new();
    let mut hips = Vec::new();
    for &t in &times {
        hips.push(vec3(0.0, HIP_HEIGHT + 0.01 * (4.0 * PI * t).cos(), SPEED * t).extend(0.0));
    }
    tracks.push(
        JointAnimation::new(0, s.joints()[0].name.clone()).with_translation(AnimationTrack::new(
            times.clone(),
            hips.clone(),
            Interpolation::Linear,
        )),
    );
    for (side, offset, x) in [("Left", 0.0, 0.1), ("Right", 0.5, -0.1)] {
        let find = |part: &str| s.find(&format!("mixamorig:{side}{part}")).unwrap();
        let (up, knee, foot, arm) = (find("UpLeg"), find("Leg"), find("Foot"), find("Arm"));
        let (mut ru, mut rk, mut rf, mut ra) = (vec![], vec![], vec![], vec![]);
        for (i, &t) in times.iter().enumerate() {
            let hip = hips[i].truncate() + vec3(x, 0.05, 0.0);
            let goal = ankle(t, offset, x);
            let (l1, l2) = (0.45, 0.42);
            let d = (goal - hip).length().min(l1 + l2 - 1e-4);
            let dir = (goal - hip).normalize();
            let along = (l1 * l1 + d * d - l2 * l2) / (2.0 * d);
            let h = (l1 * l1 - along * along).max(0.0).sqrt();
            let bend = (Vec3::Z - dir * dir.dot(Vec3::Z)).normalize();
            let k = hip + dir * along + bend * h;
            let thigh = Quat::from_rotation_arc(-Vec3::Y, (k - hip).normalize());
            let shin = Quat::from_rotation_arc(-Vec3::Y, (goal - k).normalize());
            ru.push(key(thigh));
            rk.push(key(thigh.inverse() * shin));
            rf.push(key(shin.inverse()));
            ra.push(key(Quat::from_rotation_x(
                0.4 * (2.0 * PI * (t / CYCLE + offset)).sin(),
            )));
        }
        for (joint, values) in [(up, ru), (knee, rk), (foot, rf), (arm, ra)] {
            tracks.push(
                JointAnimation::new(joint, s.joints()[joint].name.clone()).with_rotation(AnimationTrack::new(
                    times.clone(),
                    values,
                    Interpolation::Linear,
                )),
            );
        }
    }
    AnimationClip::new("walk", s, tracks)
}

fn at(s: &Skeleton, joint: usize) -> Vec3 {
    s.joint_matrix(joint).transform_point3(Vec3::ZERO)
}

/// The worst slide of a planted foot (horizontal distance from where it was
/// when the stance began) and the worst height error while planted, with
/// `pose` giving the skeleton and an extra world offset (root motion) at `t`.
fn slide_and_height(feet: [(usize, f32); 2], ground: f32, mut pose: impl FnMut(f32) -> (Vec3, Skeleton)) -> (f32, f32) {
    let (mut slide, mut height) = (0.0f32, 0.0f32);
    let mut start: [Option<Vec3>; 2] = [None, None];
    for i in 0..=480 {
        let t = i as f32 / 240.0;
        let (offset, s) = pose(t);
        for (n, (joint, phase)) in feet.iter().enumerate() {
            if !planted(t, *phase) {
                start[n] = None;
                continue;
            }
            let p = at(&s, *joint) + offset;
            let begin = *start[n].get_or_insert(p);
            slide = slide.max(vec2(p.x - begin.x, p.z - begin.z).length());
            height = height.max((p.y - ground).abs());
        }
    }
    (slide, height)
}

fn feet(s: &Skeleton, left: &str, right: &str) -> [(usize, f32); 2] {
    [(s.find(left).unwrap(), 0.0), (s.find(right).unwrap(), 0.5)]
}

fn retargeter() -> (Skeleton, Skeleton, Retargeter) {
    let (a, b) = (source(), target());
    let r = Retargeter::new(&a, &HumanoidMap::detect(&a), &b, &HumanoidMap::detect(&b)).expect("both rigs map");
    (a, b, r)
}

#[test]
fn source_walk_keeps_its_planted_feet_still() {
    let a = source();
    let clip = walk(&a);
    let (slide, height) = slide_and_height(feet(&a, "mixamorig:LeftFoot", "mixamorig:RightFoot"), 0.08, |t| {
        let mut s = a.clone();
        clip.sample(t, &mut s);
        (Vec3::ZERO, s)
    });
    assert!(slide < 0.002 && height < 0.002, "source slide {slide}, height {height}");
}

#[test]
fn retargeted_walk_plants_feet_on_a_differently_proportioned_rig() {
    let (a, b, mut r) = retargeter();
    let scale = (0.7 + 0.42) / (0.45 + 0.42);
    assert!((r.scale() - scale).abs() < 1e-4, "scale {}", r.scale());
    let walk_b = r.retarget(&walk(&a));
    assert_eq!(walk_b.name(), "walk");
    assert!((walk_b.duration() - 2.0).abs() < 1e-6);
    let (slide, height) = slide_and_height(feet(&b, "foot_l", "foot_r"), 0.1, |t| {
        let mut s = b.clone();
        walk_b.sample(t, &mut s);
        (Vec3::ZERO, s)
    });
    eprintln!("retargeted walk: planted feet slid up to {slide} m, {height} m off the ground");
    assert!(slide < SLIDE_TOLERANCE, "planted foot slid {slide} m");
    assert!(height < HEIGHT_TOLERANCE, "planted foot was {height} m off the ground");
    // The stride grows with the legs: one cycle moves the hips `scale` times as far.
    let pelvis = b.find("pelvis").unwrap();
    let mut s = b.clone();
    walk_b.sample(0.0, &mut s);
    let start = at(&s, pelvis);
    walk_b.sample(1.0, &mut s);
    let moved = at(&s, pelvis) - start;
    assert!(
        (moved.z - scale).abs() < 0.01 && moved.x.abs() < 0.01,
        "pelvis moved {moved}"
    );
}

#[test]
fn without_foot_ik_the_proportion_change_makes_feet_slide() {
    let (a, b, mut r) = retargeter();
    r.foot_ik = false;
    let walk_b = r.retarget(&walk(&a));
    let (slide, height) = slide_and_height(feet(&b, "foot_l", "foot_r"), 0.1, |t| {
        let mut s = b.clone();
        walk_b.sample(t, &mut s);
        (Vec3::ZERO, s)
    });
    eprintln!("without foot IK: planted feet slid up to {slide} m, {height} m off the ground");
    assert!(
        slide > 2.0 * SLIDE_TOLERANCE || height > 2.0 * HEIGHT_TOLERANCE,
        "slide {slide}, height {height}: the test rigs no longer need foot IK"
    );
}

#[test]
fn retargeted_rotations_match_the_source_in_model_space() {
    let (a, b, mut r) = retargeter();
    let clip = walk(&a);
    let walk_b = r.retarget(&clip);
    let rot = |s: &Skeleton, j: &str| s.joint_matrix(s.find(j).unwrap()).to_scale_rotation_translation().1;
    let (mut sa, mut sb) = (a.clone(), b.clone());
    for t in [0.1, 0.37, 0.8, 1.55] {
        clip.sample(t, &mut sa);
        walk_b.sample(t, &mut sb);
        // An arm is posed by rotation alone: its turn from rest matches.
        for (ja, jb) in [("mixamorig:LeftArm", "upperarm_l"), ("mixamorig:Spine2", "spine_04")] {
            let da = rot(&sa, ja) * rot(&a, ja).inverse();
            let db = rot(&sb, jb) * rot(&b, jb).inverse();
            assert!(
                da.angle_between(db) < 0.02,
                "{ja} vs {jb} at {t}: {}",
                da.angle_between(db)
            );
        }
    }
    // Unmapped joints keep no tracks.
    let twist = b.find("upperarm_twist_01_l").unwrap();
    assert!(walk_b.tracks().iter().all(|t| t.joint != twist));
}

fn graph(states: Vec<AnimationState>, parameters: Vec<AnimationParameter>) -> Arc<AnimationGraph> {
    AnimationGraph::new(parameters, states, vec![], StateId(0)).expect("a valid graph")
}

#[test]
fn retargeted_clips_work_in_blend_trees_with_root_motion() {
    let (a, b, mut r) = retargeter();
    let walk_b = r.retarget(&walk(&a));
    let idle_b = r.retarget(&AnimationClip::new("idle", &a, vec![]));
    let child = |clip: &AnimationClip| MotionChild::new(Motion::Clip(clip.clone()));
    let tree = Motion::blend_1d(ParamId(0), vec![(0.0, child(&idle_b)), (1.0, child(&walk_b))]);
    let mut animator = Animator::new(graph(
        vec![AnimationState::new(tree)],
        vec![AnimationParameter::Float(1.0)],
    ));
    animator.manual = true;
    animator.extract_root_motion = true;
    animator.root_joint = b.find("pelvis").unwrap();
    let mut s = b.clone();
    let mut world = Vec3::ZERO;
    let mut i = 0;
    let (slide, height) = slide_and_height(feet(&b, "foot_l", "foot_r"), 0.1, |t| {
        // Step the animator to `t` in 1/240 s frames, moving the entity by its root motion.
        while i < (t * 240.0).round() as i32 {
            animator.update(1.0 / 240.0, &mut s);
            world += animator.root_motion().position;
            i += 1;
        }
        (world, s.clone())
    });
    eprintln!("through a blend tree with root motion: slid up to {slide} m, {height} m off the ground");
    assert!(
        slide < SLIDE_TOLERANCE,
        "planted foot slid {slide} m through the blend tree"
    );
    assert!(height < HEIGHT_TOLERANCE, "planted foot was {height} m off the ground");
    assert!((world.z - 2.0 * r.scale()).abs() < 0.02, "root motion {world}");
    // Halfway to idle, the entity moves half as far.
    animator.set_float(ParamId(0), 0.5);
    let mut half = Vec3::ZERO;
    for _ in 0..240 {
        animator.update(1.0 / 240.0, &mut s);
        half += animator.root_motion().position;
    }
    assert!((half.z - 0.5 * r.scale()).abs() < 0.01, "half-blend root motion {half}");
}

#[test]
fn retargeted_clips_work_in_masked_layers() {
    let (a, b, mut r) = retargeter();
    let walk_b = r.retarget(&walk(&a));
    let wave_a = AnimationClip::new(
        "wave",
        &a,
        vec![
            JointAnimation::new(a.find("mixamorig:LeftArm").unwrap(), "mixamorig:LeftArm").with_rotation(
                AnimationTrack::new(
                    vec![0.0],
                    vec![Vec4::from_array(Quat::from_rotation_z(1.2).to_array())],
                    Interpolation::Step,
                ),
            ),
        ],
    );
    let wave_b = r.retarget(&wave_a);
    let mut base = Animator::new(graph(vec![AnimationState::new(Motion::Clip(walk_b.clone()))], vec![]));
    base.manual = true;
    let mut layer = AnimationLayer::new(graph(vec![AnimationState::new(Motion::Clip(wave_b))], vec![]));
    layer.mask = Some(JointMask::descendants(&b, "clavicle_l").unwrap());
    base.add_layer(layer);
    let mut s = b.clone();
    base.update(0.3, &mut s);
    let mut plain = b.clone();
    walk_b.sample(0.3, &mut plain);
    let calf = b.find("calf_l").unwrap();
    assert!(
        s.pose[calf].rotation.abs_diff_eq(plain.pose[calf].rotation, 1e-5),
        "the leg follows the walk"
    );
    let arm = b.find("upperarm_l").unwrap();
    let mut waving = b.clone();
    r.retarget_pose(
        &{
            let mut src = a.clone();
            wave_a.sample(0.0, &mut src);
            src
        },
        &mut waving,
    );
    assert!(
        s.pose[arm].rotation.abs_diff_eq(waving.pose[arm].rotation, 1e-4),
        "the arm waves"
    );
}

#[test]
fn common_naming_schemes_are_detected() {
    let chain = |names: &[&str]| {
        Skeleton::new(
            names
                .iter()
                .enumerate()
                .map(|(i, n)| Joint::new(*n, i.checked_sub(1)))
                .collect(),
        )
    };
    let cases: &[(&[&str], &[HumanBone])] = &[
        (
            &[
                "Bip01",
                "Bip01 Pelvis",
                "Bip01 Spine",
                "Bip01 L Thigh",
                "Bip01 L Calf",
                "Bip01 L Foot",
                "Bip01 L Toe0",
            ],
            &[
                B::Hips,
                B::Spine,
                B::LeftUpperLeg,
                B::LeftLowerLeg,
                B::LeftFoot,
                B::LeftToes,
            ],
        ),
        (
            &[
                "J_Bip_C_Hips",
                "J_Bip_C_Spine",
                "J_Bip_C_Chest",
                "J_Bip_C_UpperChest",
                "J_Bip_R_Shoulder",
                "J_Bip_R_UpperArm",
                "J_Bip_R_LowerArm",
                "J_Bip_R_Hand",
                "J_Bip_R_Index1",
                "J_Bip_R_Index2",
                "J_Bip_R_Index3",
            ],
            &[
                B::Hips,
                B::Spine,
                B::Chest,
                B::UpperChest,
                B::RightShoulder,
                B::RightUpperArm,
                B::RightLowerArm,
                B::RightHand,
                B::RightIndexProximal,
                B::RightIndexIntermediate,
                B::RightIndexDistal,
            ],
        ),
        (
            &["Armature|hips", "spine", "chest", "neck", "head"],
            &[B::Hips, B::Spine, B::Chest, B::Neck, B::Head],
        ),
        (
            &["DEF-pelvis", "DEF-thigh.L", "DEF-shin.L", "DEF-foot.L", "DEF-toe.L"],
            &[B::Hips, B::LeftUpperLeg, B::LeftLowerLeg, B::LeftFoot, B::LeftToes],
        ),
        (
            &[
                "mixamorig:Hips",
                "mixamorig:Spine",
                "mixamorig:Spine1",
                "mixamorig:Spine2",
                "mixamorig:LeftShoulder",
                "mixamorig:LeftArm",
                "mixamorig:LeftForeArm",
                "mixamorig:LeftHand",
                "mixamorig:LeftHandThumb1",
                "mixamorig:LeftHandThumb2",
                "mixamorig:LeftHandThumb3",
                "mixamorig:LeftHandThumb4",
            ],
            &[
                B::Hips,
                B::Spine,
                B::Chest,
                B::UpperChest,
                B::LeftShoulder,
                B::LeftUpperArm,
                B::LeftLowerArm,
                B::LeftHand,
                B::LeftThumbProximal,
                B::LeftThumbIntermediate,
                B::LeftThumbDistal,
            ],
        ),
    ];
    for (names, bones) in cases {
        let s = chain(names);
        let map = HumanoidMap::detect(&s);
        // Bones name consecutive joints, after an unnamed top joint ("Bip01").
        let first = usize::from(names[0] == "Bip01");
        for (i, bone) in bones.iter().enumerate() {
            assert_eq!(map.get(*bone), Some(first + i), "{bone:?} in {names:?}");
        }
    }
    // The Unreal-style rig: root above the pelvis, five spines, twist bones skipped.
    let b = target();
    let map = HumanoidMap::detect(&b);
    for (bone, name) in [
        (B::Root, "root"),
        (B::Hips, "pelvis"),
        (B::Spine, "spine_01"),
        (B::Chest, "spine_03"),
        (B::UpperChest, "spine_04"),
        (B::LeftUpperArm, "upperarm_l"),
        (B::RightToes, "ball_r"),
    ] {
        assert_eq!(map.get(bone), b.find(name), "{bone:?}");
    }
    assert_eq!(map.missing().count(), 0);
}

use HumanBone as B;

#[test]
fn maps_can_be_edited_and_incomplete_maps_are_refused() {
    let (a, b) = (source(), target());
    let mut map = HumanoidMap::detect(&b);
    map.set(B::Head, None);
    assert_eq!(
        Retargeter::new(&a, &HumanoidMap::detect(&a), &b, &map).unwrap_err(),
        RetargetError::MissingBone {
            source: false,
            bone: B::Head
        }
    );
    map.set(B::Head, Some(999));
    let err = Retargeter::new(&a, &HumanoidMap::detect(&a), &b, &map).unwrap_err();
    assert_eq!(
        err.to_string(),
        "the head is mapped to joint 999, which the target skeleton doesn't have"
    );
    map.set(B::Head, b.find("neck_01"));
    assert!(Retargeter::new(&a, &HumanoidMap::detect(&a), &b, &map).is_ok());
}

#[test]
fn live_pose_retargeting_allocates_nothing_after_the_first_call() {
    let (a, b, mut r) = retargeter();
    let clip = walk(&a);
    let (mut source, mut target) = (a.clone(), b.clone());
    clip.sample(0.0, &mut source);
    r.retarget_pose(&source, &mut target);
    ALLOCS.set(0);
    TRACK.set(true);
    for i in 0..120 {
        clip.sample(i as f32 / 60.0, &mut source);
        r.retarget_pose(&source, &mut target);
    }
    TRACK.set(false);
    assert_eq!(ALLOCS.get(), 0, "allocations while following a live pose");
    let mut expected = b.clone();
    let walk_b = r.retarget(&clip);
    walk_b.sample(119.0 / 60.0, &mut expected);
    let foot = b.find("foot_l").unwrap();
    assert!(
        at(&target, foot).distance(at(&expected, foot)) < 1e-3,
        "live and baked poses agree"
    );
}

/// The source rig turned to face -Z, its left at -X: the way a Unity rig
/// comes out of the Unity importer.
fn source_facing_back() -> Skeleton {
    let s = source();
    let names: Vec<String> = s.joints().iter().map(|j| j.name.clone()).collect();
    let joints: Vec<(&str, Option<usize>, Vec3, Quat)> = s
        .joints()
        .iter()
        .enumerate()
        .map(|(i, j)| {
            let p = at(&s, i);
            (names[i].as_str(), j.parent, vec3(-p.x, p.y, -p.z), Quat::IDENTITY)
        })
        .collect();
    rig(&joints)
}

/// A humanoid clip as `ponko_unity` imports it from Unity's muscle curves
/// (its humanoid test writes `tests/fixtures/unity/humanoid_wave.glb`: a
/// T-posed character whose left arm Down-Up muscle goes from its T-pose
/// value to 1 over a second), played by the engine and retargeted.
#[test]
fn an_imported_unity_humanoid_clip_plays_and_retargets() {
    use ponko::assets::MemorySource;
    let bytes = include_bytes!("fixtures/unity/humanoid_wave.glb").to_vec();
    Game::new("unity", 1, 1).run_headless(0, move |ctx| {
        ctx.assets
            .add_source(MemorySource::new("unity").with("wave.glb", bytes.clone()));
        let rest = ctx.gfx.load_skeleton("wave.glb");
        let wave = ctx.gfx.load_clip("wave.glb", "Wave");
        assert!((wave.duration() - 1.0).abs() < 1e-5);
        let joint = |name: &str| rest.find(name).unwrap_or_else(|| panic!("no joint {name}"));
        let (shoulder, hand) = (joint("bone_leftupperarm"), joint("bone_lefthand"));
        let arm = |s: &Skeleton| (at(s, hand) - at(s, shoulder)).normalize();
        // Played: level at the start, 60 degrees up at the end, and on the
        // way up halfway through an Animator.
        let mut s = rest.clone();
        wave.sample(0.0, &mut s);
        assert!(arm(&s).y.abs() < 1e-3, "{}", arm(&s));
        wave.sample(1.0, &mut s);
        assert!((arm(&s).y - 60f32.to_radians().sin()).abs() < 1e-3, "{}", arm(&s));
        let mut animator = Animator::new(graph(vec![AnimationState::new(Motion::Clip(wave.clone()))], vec![]));
        animator.manual = true;
        let mut played = rest.clone();
        for _ in 0..60 {
            animator.update(1.0 / 120.0, &mut played);
        }
        let halfway = arm(&played).y;
        assert!(halfway > 0.2 && halfway < 0.8, "half a second in: {halfway}");
        // Retargeted onto a rig of other names and proportions.
        let mut map = HumanoidMap::new();
        for (bone, name) in [
            (HumanBone::Hips, "hips"),
            (HumanBone::Spine, "spine"),
            (HumanBone::Chest, "chest"),
            (HumanBone::Neck, "neck"),
            (HumanBone::Head, "head"),
            (HumanBone::LeftShoulder, "leftshoulder"),
            (HumanBone::LeftUpperArm, "leftupperarm"),
            (HumanBone::LeftLowerArm, "leftlowerarm"),
            (HumanBone::LeftHand, "lefthand"),
            (HumanBone::RightShoulder, "rightshoulder"),
            (HumanBone::RightUpperArm, "rightupperarm"),
            (HumanBone::RightLowerArm, "rightlowerarm"),
            (HumanBone::RightHand, "righthand"),
            (HumanBone::LeftUpperLeg, "leftupperleg"),
            (HumanBone::LeftLowerLeg, "leftlowerleg"),
            (HumanBone::LeftFoot, "leftfoot"),
            (HumanBone::LeftToes, "lefttoes"),
            (HumanBone::RightUpperLeg, "rightupperleg"),
            (HumanBone::RightLowerLeg, "rightlowerleg"),
            (HumanBone::RightFoot, "rightfoot"),
            (HumanBone::RightToes, "righttoes"),
        ] {
            map.set(bone, Some(joint(&format!("bone_{name}"))));
        }
        let target = source_facing_back();
        let mut retarget = Retargeter::new(&rest, &map, &target, &HumanoidMap::detect(&target)).expect("both map");
        let theirs = retarget.retarget(&wave);
        let mut t = target.clone();
        theirs.sample(1.0, &mut t);
        let (shoulder, hand) = (
            t.find("mixamorig:LeftArm").unwrap(),
            t.find("mixamorig:LeftHand").unwrap(),
        );
        let up = (at(&t, hand) - at(&t, shoulder)).normalize();
        assert!((up.y - 60f32.to_radians().sin()).abs() < 1e-2, "retargeted: {up}");
    });
}
