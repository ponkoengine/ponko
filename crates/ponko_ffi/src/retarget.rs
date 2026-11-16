//! Humanoid retargeting from C (Rust `HumanoidMap`, `Retargeter`): play a
//! clip made for one humanoid skeleton on another with different
//! proportions, joint names and joint axes.
//!
//! Skeletons are the ones on entities (`ponko_ctx_set_skeleton`,
//! `ponko_ctx_load_skeleton`). A `PonkoHumanoidMap` says which joint plays
//! each `PonkoHumanBone`; `ponko_ctx_detect_humanoid_map` reads the usual
//! naming schemes. A `PonkoRetargeter` copies both skeletons' rest data when
//! it is made, so it keeps no borrow of the world: it outlives the entities,
//! and later changes to their skeletons don't reach it.

use std::ffi::{CStr, c_char};

use ponko::anim::{HumanBone, HumanoidMap, Retargeter, Skeleton};
use ponko::prelude::Entity;

use crate::ctx::{ctx_access, out};
use crate::skin::PonkoClip;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid, invalid_owned};

/// A bone of a humanoid skeleton (Rust `HumanBone`), passed as `uint32_t`.
/// Fingers have three segments each, from the hand outwards. New bones get
/// new values at the end.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoHumanBone {
    /// The root.
    PONKO_HUMAN_BONE_ROOT = 0,
    /// The hips.
    PONKO_HUMAN_BONE_HIPS = 1,
    /// The spine.
    PONKO_HUMAN_BONE_SPINE = 2,
    /// The chest.
    PONKO_HUMAN_BONE_CHEST = 3,
    /// The upper chest.
    PONKO_HUMAN_BONE_UPPER_CHEST = 4,
    /// The neck.
    PONKO_HUMAN_BONE_NECK = 5,
    /// The head.
    PONKO_HUMAN_BONE_HEAD = 6,
    /// The left shoulder.
    PONKO_HUMAN_BONE_LEFT_SHOULDER = 7,
    /// The left upper arm.
    PONKO_HUMAN_BONE_LEFT_UPPER_ARM = 8,
    /// The left lower arm.
    PONKO_HUMAN_BONE_LEFT_LOWER_ARM = 9,
    /// The left hand.
    PONKO_HUMAN_BONE_LEFT_HAND = 10,
    /// The right shoulder.
    PONKO_HUMAN_BONE_RIGHT_SHOULDER = 11,
    /// The right upper arm.
    PONKO_HUMAN_BONE_RIGHT_UPPER_ARM = 12,
    /// The right lower arm.
    PONKO_HUMAN_BONE_RIGHT_LOWER_ARM = 13,
    /// The right hand.
    PONKO_HUMAN_BONE_RIGHT_HAND = 14,
    /// The left upper leg.
    PONKO_HUMAN_BONE_LEFT_UPPER_LEG = 15,
    /// The left lower leg.
    PONKO_HUMAN_BONE_LEFT_LOWER_LEG = 16,
    /// The left foot.
    PONKO_HUMAN_BONE_LEFT_FOOT = 17,
    /// The left toes.
    PONKO_HUMAN_BONE_LEFT_TOES = 18,
    /// The right upper leg.
    PONKO_HUMAN_BONE_RIGHT_UPPER_LEG = 19,
    /// The right lower leg.
    PONKO_HUMAN_BONE_RIGHT_LOWER_LEG = 20,
    /// The right foot.
    PONKO_HUMAN_BONE_RIGHT_FOOT = 21,
    /// The right toes.
    PONKO_HUMAN_BONE_RIGHT_TOES = 22,
    /// The left thumb proximal.
    PONKO_HUMAN_BONE_LEFT_THUMB_PROXIMAL = 23,
    /// The left thumb intermediate.
    PONKO_HUMAN_BONE_LEFT_THUMB_INTERMEDIATE = 24,
    /// The left thumb distal.
    PONKO_HUMAN_BONE_LEFT_THUMB_DISTAL = 25,
    /// The left index proximal.
    PONKO_HUMAN_BONE_LEFT_INDEX_PROXIMAL = 26,
    /// The left index intermediate.
    PONKO_HUMAN_BONE_LEFT_INDEX_INTERMEDIATE = 27,
    /// The left index distal.
    PONKO_HUMAN_BONE_LEFT_INDEX_DISTAL = 28,
    /// The left middle proximal.
    PONKO_HUMAN_BONE_LEFT_MIDDLE_PROXIMAL = 29,
    /// The left middle intermediate.
    PONKO_HUMAN_BONE_LEFT_MIDDLE_INTERMEDIATE = 30,
    /// The left middle distal.
    PONKO_HUMAN_BONE_LEFT_MIDDLE_DISTAL = 31,
    /// The left ring proximal.
    PONKO_HUMAN_BONE_LEFT_RING_PROXIMAL = 32,
    /// The left ring intermediate.
    PONKO_HUMAN_BONE_LEFT_RING_INTERMEDIATE = 33,
    /// The left ring distal.
    PONKO_HUMAN_BONE_LEFT_RING_DISTAL = 34,
    /// The left little proximal.
    PONKO_HUMAN_BONE_LEFT_LITTLE_PROXIMAL = 35,
    /// The left little intermediate.
    PONKO_HUMAN_BONE_LEFT_LITTLE_INTERMEDIATE = 36,
    /// The left little distal.
    PONKO_HUMAN_BONE_LEFT_LITTLE_DISTAL = 37,
    /// The right thumb proximal.
    PONKO_HUMAN_BONE_RIGHT_THUMB_PROXIMAL = 38,
    /// The right thumb intermediate.
    PONKO_HUMAN_BONE_RIGHT_THUMB_INTERMEDIATE = 39,
    /// The right thumb distal.
    PONKO_HUMAN_BONE_RIGHT_THUMB_DISTAL = 40,
    /// The right index proximal.
    PONKO_HUMAN_BONE_RIGHT_INDEX_PROXIMAL = 41,
    /// The right index intermediate.
    PONKO_HUMAN_BONE_RIGHT_INDEX_INTERMEDIATE = 42,
    /// The right index distal.
    PONKO_HUMAN_BONE_RIGHT_INDEX_DISTAL = 43,
    /// The right middle proximal.
    PONKO_HUMAN_BONE_RIGHT_MIDDLE_PROXIMAL = 44,
    /// The right middle intermediate.
    PONKO_HUMAN_BONE_RIGHT_MIDDLE_INTERMEDIATE = 45,
    /// The right middle distal.
    PONKO_HUMAN_BONE_RIGHT_MIDDLE_DISTAL = 46,
    /// The right ring proximal.
    PONKO_HUMAN_BONE_RIGHT_RING_PROXIMAL = 47,
    /// The right ring intermediate.
    PONKO_HUMAN_BONE_RIGHT_RING_INTERMEDIATE = 48,
    /// The right ring distal.
    PONKO_HUMAN_BONE_RIGHT_RING_DISTAL = 49,
    /// The right little proximal.
    PONKO_HUMAN_BONE_RIGHT_LITTLE_PROXIMAL = 50,
    /// The right little intermediate.
    PONKO_HUMAN_BONE_RIGHT_LITTLE_INTERMEDIATE = 51,
    /// The right little distal.
    PONKO_HUMAN_BONE_RIGHT_LITTLE_DISTAL = 52,
}

/// The Rust bone of each `PonkoHumanBone` value, in value order.
const BONES: [HumanBone; 53] = [
    HumanBone::Root,
    HumanBone::Hips,
    HumanBone::Spine,
    HumanBone::Chest,
    HumanBone::UpperChest,
    HumanBone::Neck,
    HumanBone::Head,
    HumanBone::LeftShoulder,
    HumanBone::LeftUpperArm,
    HumanBone::LeftLowerArm,
    HumanBone::LeftHand,
    HumanBone::RightShoulder,
    HumanBone::RightUpperArm,
    HumanBone::RightLowerArm,
    HumanBone::RightHand,
    HumanBone::LeftUpperLeg,
    HumanBone::LeftLowerLeg,
    HumanBone::LeftFoot,
    HumanBone::LeftToes,
    HumanBone::RightUpperLeg,
    HumanBone::RightLowerLeg,
    HumanBone::RightFoot,
    HumanBone::RightToes,
    HumanBone::LeftThumbProximal,
    HumanBone::LeftThumbIntermediate,
    HumanBone::LeftThumbDistal,
    HumanBone::LeftIndexProximal,
    HumanBone::LeftIndexIntermediate,
    HumanBone::LeftIndexDistal,
    HumanBone::LeftMiddleProximal,
    HumanBone::LeftMiddleIntermediate,
    HumanBone::LeftMiddleDistal,
    HumanBone::LeftRingProximal,
    HumanBone::LeftRingIntermediate,
    HumanBone::LeftRingDistal,
    HumanBone::LeftLittleProximal,
    HumanBone::LeftLittleIntermediate,
    HumanBone::LeftLittleDistal,
    HumanBone::RightThumbProximal,
    HumanBone::RightThumbIntermediate,
    HumanBone::RightThumbDistal,
    HumanBone::RightIndexProximal,
    HumanBone::RightIndexIntermediate,
    HumanBone::RightIndexDistal,
    HumanBone::RightMiddleProximal,
    HumanBone::RightMiddleIntermediate,
    HumanBone::RightMiddleDistal,
    HumanBone::RightRingProximal,
    HumanBone::RightRingIntermediate,
    HumanBone::RightRingDistal,
    HumanBone::RightLittleProximal,
    HumanBone::RightLittleIntermediate,
    HumanBone::RightLittleDistal,
];

/// Each bone's name, NUL-terminated, in value order.
const NAMES: [&CStr; 53] = [
    c"root",
    c"hips",
    c"spine",
    c"chest",
    c"upper chest",
    c"neck",
    c"head",
    c"left shoulder",
    c"left upper arm",
    c"left lower arm",
    c"left hand",
    c"right shoulder",
    c"right upper arm",
    c"right lower arm",
    c"right hand",
    c"left upper leg",
    c"left lower leg",
    c"left foot",
    c"left toes",
    c"right upper leg",
    c"right lower leg",
    c"right foot",
    c"right toes",
    c"left thumb proximal",
    c"left thumb intermediate",
    c"left thumb distal",
    c"left index proximal",
    c"left index intermediate",
    c"left index distal",
    c"left middle proximal",
    c"left middle intermediate",
    c"left middle distal",
    c"left ring proximal",
    c"left ring intermediate",
    c"left ring distal",
    c"left little proximal",
    c"left little intermediate",
    c"left little distal",
    c"right thumb proximal",
    c"right thumb intermediate",
    c"right thumb distal",
    c"right index proximal",
    c"right index intermediate",
    c"right index distal",
    c"right middle proximal",
    c"right middle intermediate",
    c"right middle distal",
    c"right ring proximal",
    c"right ring intermediate",
    c"right ring distal",
    c"right little proximal",
    c"right little intermediate",
    c"right little distal",
];

fn bone(value: u32) -> Result<HumanBone, Failure> {
    BONES
        .get(value as usize)
        .copied()
        .ok_or_else(|| invalid_owned(format!("{value} is not a PonkoHumanBone")))
}

fn bone_to_c(bone: HumanBone) -> u32 {
    BONES
        .iter()
        .position(|b| *b == bone)
        .expect("every HumanBone has a PonkoHumanBone (checked by a test)") as u32
}

/// The name of a `PonkoHumanBone` in words, such as `"left upper leg"`
/// (static), or null for an unknown value.
/// ```
/// let name = unsafe { std::ffi::CStr::from_ptr(ponko_ffi::ponko_human_bone_name(ponko_ffi::PonkoHumanBone::PONKO_HUMAN_BONE_LEFT_UPPER_LEG as u32)) };
/// assert_eq!(name.to_str(), Ok("left upper leg"));
/// assert!(ponko_ffi::ponko_human_bone_name(9999).is_null());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_human_bone_name(bone: u32) -> *const c_char {
    NAMES.get(bone as usize).map_or(std::ptr::null(), |n| n.as_ptr())
}

/// Whether retargeting needs `bone` mapped on both skeletons: the hips,
/// spine, head, and each arm and leg down to the hand and foot (Rust
/// `HumanBone::is_required`). False for unknown values.
/// ```
/// assert!(ponko_ffi::ponko_human_bone_is_required(ponko_ffi::PonkoHumanBone::PONKO_HUMAN_BONE_HIPS as u32));
/// assert!(!ponko_ffi::ponko_human_bone_is_required(ponko_ffi::PonkoHumanBone::PONKO_HUMAN_BONE_CHEST as u32));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_human_bone_is_required(bone: u32) -> bool {
    BONES.get(bone as usize).is_some_and(|b| b.is_required())
}

/// Which joint of a skeleton plays each `PonkoHumanBone` (Rust
/// `HumanoidMap`): from `ponko_humanoid_map_new` or
/// `ponko_ctx_detect_humanoid_map`, freed with `ponko_humanoid_map_free`.
pub struct PonkoHumanoidMap(HumanoidMap);

/// Prepared retargeting from one skeleton to another (Rust `Retargeter`):
/// from `ponko_ctx_create_retargeter`, freed with `ponko_retargeter_free`.
pub struct PonkoRetargeter {
    inner: Retargeter,
    /// Joints of the skeletons it was made for: `retarget_pose` needs at least as many.
    source_joints: usize,
    target_joints: usize,
}

unsafe fn put<T>(out_ptr: *mut *mut T, value: T) -> Result<(), Failure> {
    // SAFETY: caller contract: null or writable.
    unsafe { out(out_ptr, Box::into_raw(Box::new(value))) }
}

/// An empty map: no bone mapped. Free it with `ponko_humanoid_map_free`.
/// # Safety
/// `out_map` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut m = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_humanoid_map_new(&mut m) }, PonkoStatus::PONKO_OK);
/// unsafe { ponko_humanoid_map_free(m) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_humanoid_map_new(out_map: *mut *mut PonkoHumanoidMap) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { put(out_map, PonkoHumanoidMap(HumanoidMap::new())) })
}

fn skeleton(c: &ponko::prelude::Ctx, bits: u64) -> Result<&Skeleton, Failure> {
    let e = Entity::from_bits(bits);
    if !c.world.is_alive(e) {
        return Err(invalid("no such entity"));
    }
    c.world
        .get::<Skeleton>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no skeleton"))
}

/// Maps the bones of the skeleton on `skeleton_entity` by joint name (Rust
/// `HumanoidMap::detect`): words such as hips/pelvis, thigh/upper leg/up
/// leg, sides as left/right or a separate l/r, prefixes such as `name:`.
/// Check `ponko_humanoid_map_missing` afterwards and fix the rest with
/// `ponko_humanoid_map_set`. `PONKO_NOT_FOUND` if the entity has no skeleton.
/// # Safety
/// `ctx` null or live; `out_map` null or writable.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_detect_humanoid_map(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_detect_humanoid_map(
    ctx: *mut PonkoCtx,
    skeleton_entity: u64,
    out_map: *mut *mut PonkoHumanoidMap,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_map.is_null() {
                return Err(invalid("output is null"));
            }
            let map = HumanoidMap::detect(skeleton(c, skeleton_entity)?);
            put(out_map, PonkoHumanoidMap(map))
        })
    })
}

/// Frees a map; null does nothing.
/// # Safety
/// `map` null or from this library, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_humanoid_map_free(map: *mut PonkoHumanoidMap) {
    if !map.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(map) });
    }
}

fn map_ref<'a>(map: *const PonkoHumanoidMap) -> Result<&'a HumanoidMap, Failure> {
    // SAFETY: callers pass null or a live map.
    unsafe { map.as_ref() }
        .map(|m| &m.0)
        .ok_or_else(|| invalid("humanoid map is null"))
}

/// The joint mapped to `bone` into `*out_joint`, or -1 when none is.
/// # Safety
/// `map` null or live; `out_joint` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut m = std::ptr::null_mut();
/// let mut j = 0;
/// unsafe {
///     ponko_humanoid_map_new(&mut m);
///     assert_eq!(ponko_humanoid_map_set(m, PonkoHumanBone::PONKO_HUMAN_BONE_HEAD as u32, 4), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_humanoid_map_get(m, PonkoHumanBone::PONKO_HUMAN_BONE_HEAD as u32, &mut j), PonkoStatus::PONKO_OK);
///     assert_eq!(j, 4);
///     ponko_humanoid_map_get(m, PonkoHumanBone::PONKO_HUMAN_BONE_NECK as u32, &mut j);
///     assert_eq!(j, -1);
///     ponko_humanoid_map_free(m);
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_humanoid_map_get(
    map: *const PonkoHumanoidMap,
    bone: u32,
    out_joint: *mut i32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let joint = map_ref(map)?.get(self::bone(bone)?);
        out(out_joint, joint.map_or(-1, |j| i32::try_from(j).unwrap_or(i32::MAX)))
    })
}

/// Maps `bone` to `joint` (an index into the skeleton's joints), or unmaps
/// it with -1. The joint is checked against the skeleton when a
/// retargeter is made.
/// # Safety
/// `map` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_humanoid_map_set(map: *mut PonkoHumanoidMap, bone: u32, joint: i32) -> PonkoStatus {
    boundary(|| {
        let bone = self::bone(bone)?;
        if joint < -1 {
            return Err(invalid("joint must be an index, or -1 to unmap the bone"));
        }
        // SAFETY: caller contract: null or live.
        let map = unsafe { map.as_mut() }.ok_or_else(|| invalid("humanoid map is null"))?;
        map.0.set(bone, usize::try_from(joint).ok());
        Ok(())
    })
}

/// The required bones (`ponko_human_bone_is_required`) the map doesn't
/// map, as `PonkoHumanBone` values: writes up to `capacity` to `out_bones`
/// and how many there are to `*out_count` (0: retargeting can use it).
/// # Safety
/// `map` null or live; `out_bones` writable for `capacity` values (or null
/// with 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_humanoid_map_missing(
    map: *const PonkoHumanoidMap,
    out_bones: *mut u32,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let map = map_ref(map)?;
        if out_count.is_null() || (out_bones.is_null() && capacity > 0) {
            return Err(invalid("output is null"));
        }
        let mut n = 0;
        for b in map.missing() {
            if n < capacity {
                out_bones.add(n).write(bone_to_c(b));
            }
            n += 1;
        }
        out(out_count, n)
    })
}

/// Prepares retargeting from the skeleton on `source_entity` to the one on
/// `target_entity`, each with its map (Rust `Retargeter::new`). It copies
/// both skeletons' rest data, so it stays valid when the entities change or
/// go. Bones mapped on only one side are left alone. Fails with
/// `PONKO_INVALID_ARGUMENT` (and a message naming the bone) if a required
/// bone isn't mapped on either skeleton or a map names a joint its skeleton
/// doesn't have; `PONKO_NOT_FOUND` if an entity has no skeleton.
/// # Safety
/// `ctx` null or live; maps null or live; `out_retargeter` null or writable.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_create_retargeter(std::ptr::null_mut(), 0, std::ptr::null(), 0, std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_retargeter(
    ctx: *mut PonkoCtx,
    source_entity: u64,
    source_map: *const PonkoHumanoidMap,
    target_entity: u64,
    target_map: *const PonkoHumanoidMap,
    out_retargeter: *mut *mut PonkoRetargeter,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_retargeter.is_null() {
                return Err(invalid("output is null"));
            }
            let (source_map, target_map) = (map_ref(source_map)?, map_ref(target_map)?);
            let (source, target) = (skeleton(c, source_entity)?, skeleton(c, target_entity)?);
            let inner =
                Retargeter::new(source, source_map, target, target_map).map_err(|e| invalid_owned(e.to_string()))?;
            put(
                out_retargeter,
                PonkoRetargeter {
                    inner,
                    source_joints: source.joints().len(),
                    target_joints: target.joints().len(),
                },
            )
        })
    })
}

/// Frees a retargeter; null does nothing.
/// # Safety
/// `retargeter` null or from this library, freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_retargeter_free(retargeter: *mut PonkoRetargeter) {
    if !retargeter.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(retargeter) });
    }
}

fn retargeter_mut<'a>(r: *mut PonkoRetargeter) -> Result<&'a mut PonkoRetargeter, Failure> {
    // SAFETY: callers pass null or a live retargeter.
    unsafe { r.as_mut() }.ok_or_else(|| invalid("retargeter is null"))
}

/// How much further the target moves than the source: its leg length over
/// the source's, from upper leg to foot at rest (Rust `Retargeter::scale`).
/// # Safety
/// `retargeter` null or live; `out_scale` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_retargeter_scale(
    retargeter: *const PonkoRetargeter,
    out_scale: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_scale, retargeter_mut(retargeter.cast_mut())?.inner.scale()) })
}

/// Whether the feet are placed with two-bone IK where the scaled source
/// feet are (Rust `Retargeter::foot_ik`). On by default.
/// # Safety
/// `retargeter` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_retargeter_set_foot_ik(retargeter: *mut PonkoRetargeter, on: bool) -> PonkoStatus {
    boundary(|| {
        retargeter_mut(retargeter)?.inner.foot_ik = on;
        Ok(())
    })
}

/// Keys per second `ponko_retarget_clip` adds where the source clip's keys
/// are further apart (Rust `Retargeter::sample_rate`): above 0. Default 30.
/// # Safety
/// `retargeter` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_retargeter_set_sample_rate(retargeter: *mut PonkoRetargeter, rate: f32) -> PonkoStatus {
    boundary(|| {
        if !(rate.is_finite() && rate > 0.0) {
            return Err(invalid("sample rate must be above 0"));
        }
        retargeter_mut(retargeter)?.inner.sample_rate = rate;
        Ok(())
    })
}

/// `clip` (made for the source skeleton) as a new clip for the target
/// skeleton, under the same name (Rust `Retargeter::retarget`): it keys every
/// mapped target joint's rotation (and the hips' and root's position) at the
/// source's key times, adding keys at the sample rate, with linear
/// interpolation. It samples the whole clip: do it once per clip, at load.
/// Free the new clip with `ponko_clip_free`.
/// # Safety
/// `retargeter` and `clip` null or live; `out_clip` null or writable.
/// ```
/// let s = unsafe { ponko_ffi::ponko_retarget_clip(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_retarget_clip(
    retargeter: *mut PonkoRetargeter,
    clip: *const PonkoClip,
    out_clip: *mut *mut PonkoClip,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let r = retargeter_mut(retargeter)?;
        let clip = clip.as_ref().ok_or_else(|| invalid("clip is null"))?;
        if out_clip.is_null() {
            return Err(invalid("output is null"));
        }
        put(out_clip, PonkoClip(r.inner.retarget(&clip.0)))
    })
}

/// Poses the mapped joints of `target_entity`'s skeleton like
/// `source_entity`'s is posed now, leaving its other joints alone (Rust
/// `Retargeter::retarget_pose`): every frame, to follow a live skeleton.
/// Allocates nothing after the first call. Each skeleton needs at least the
/// joints of the one the retargeter was made for, and the two entities must
/// differ.
/// # Safety
/// `ctx` null or live; `retargeter` null or live.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_retarget_pose(std::ptr::null_mut(), std::ptr::null_mut(), 0, 1) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_retarget_pose(
    ctx: *mut PonkoCtx,
    retargeter: *mut PonkoRetargeter,
    source_entity: u64,
    target_entity: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let r = retargeter_mut(retargeter)?;
            if source_entity == target_entity {
                return Err(invalid("the source and target must be different entities"));
            }
            let (s, t) = (
                skeleton(c, source_entity)?.joints().len(),
                skeleton(c, target_entity)?.joints().len(),
            );
            if s < r.source_joints || t < r.target_joints {
                return Err(invalid(
                    "a skeleton has fewer joints than the one this retargeter was made for",
                ));
            }
            let [source, target] = c
                .world
                .get_many_mut::<Skeleton, 2>([Entity::from_bits(source_entity), Entity::from_bits(target_entity)])
                .ok_or_else(|| invalid("no such skeletons"))?;
            r.inner.retarget_pose(source, target);
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_human_bone_has_a_c_value_in_rust_order() {
        assert_eq!(
            BONES.as_slice(),
            HumanBone::ALL,
            "append new bones at the end of PonkoHumanBone"
        );
        for (i, b) in BONES.iter().enumerate() {
            assert_eq!(NAMES[i].to_str(), Ok(b.name()));
        }
    }
}
