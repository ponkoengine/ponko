//! Joint-track clips built in code from C (Rust `AnimationClip::try_new`
//! with `JointAnimation`s), for crowds and skinned meshes, and the live
//! counts in their versioned form, crowds included (Rust `LiveCounts`).
use std::ffi::c_char;
use std::mem::{offset_of, size_of};

use ponko::anim::{AnimationClip, AnimationTrack, Interpolation, JointAnimation, Skeleton};
use ponko::prelude::{Entity, Vec4};

use crate::ctx::{ctx_access, out, text};
use crate::descriptors::{Versioned, write_desc};
use crate::{PonkoClip, PonkoCtx, PonkoStatus, boundary, invalid, invalid_owned};

macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// Which part of a joint's local pose a `PonkoJointTrack` animates.
/// ```
/// assert_eq!(ponko_ffi::PonkoJointChannel::PONKO_JOINT_ROTATION as u32, 1);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoJointChannel {
    /// The local position (x, y, z; w unused).
    PONKO_JOINT_TRANSLATION = 0,
    /// The local rotation, a quaternion (x, y, z, w).
    PONKO_JOINT_ROTATION = 1,
    /// The local scale (x, y, z; w unused).
    PONKO_JOINT_SCALE = 2,
}

/// One joint's keys for one channel, for `ponko_ctx_create_clip`.
/// ```
/// let t = ponko_ffi::PonkoJointTrack { joint: 0, channel: 1, interpolation: 1, times: std::ptr::null(), values: std::ptr::null(), count: 0 };
/// assert_eq!(t.channel, ponko_ffi::PonkoJointChannel::PONKO_JOINT_ROTATION as u32);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoJointTrack {
    /// The joint's index in the skeleton the clip is made for.
    pub joint: u32,
    /// A `PonkoJointChannel`.
    pub channel: u32,
    /// 0 STEP, 1 LINEAR, 2 CUBICSPLINE (as `ponko_clip_set_morph_track`).
    pub interpolation: u32,
    /// `count` key times in seconds, rising.
    pub times: *const f32,
    /// Four floats per key (x, y, z, w); for CUBICSPLINE three such values
    /// per key: in-tangent, value, out-tangent.
    pub values: *const f32,
    /// How many keys.
    pub count: usize,
}

fn track_from_c(t: &PonkoJointTrack) -> Result<AnimationTrack, crate::Failure> {
    let interpolation = match t.interpolation {
        0 => Interpolation::Step,
        1 => Interpolation::Linear,
        2 => Interpolation::CubicSpline,
        _ => return Err(invalid("PonkoJointTrack: interpolation is 0, 1 or 2")),
    };
    if t.count == 0 || t.count > 1_000_000 || t.times.is_null() || t.values.is_null() {
        return Err(invalid("PonkoJointTrack: give 1 to 1,000,000 keys and both arrays"));
    }
    let per_key = if interpolation == Interpolation::CubicSpline {
        3
    } else {
        1
    };
    // SAFETY: caller contract: `times` holds `count` floats, `values` four
    // per value.
    let (times, values) = unsafe {
        (
            std::slice::from_raw_parts(t.times, t.count),
            std::slice::from_raw_parts(t.values, t.count * per_key * 4),
        )
    };
    let values = values.as_chunks::<4>().0.iter().map(|&v| Vec4::from_array(v)).collect();
    AnimationTrack::try_new(times.to_vec(), values, interpolation).map_err(invalid_owned)
}

/// Builds the joint animations of `tracks` for `skeleton`: tracks of one
/// joint merge, a channel given twice for a joint is refused.
fn joints_from_c(skeleton: &Skeleton, tracks: &[PonkoJointTrack]) -> Result<Vec<JointAnimation>, crate::Failure> {
    let mut joints: Vec<JointAnimation> = Vec::new();
    for t in tracks {
        let index = t.joint as usize;
        let Some(joint) = skeleton.joints().get(index) else {
            return Err(invalid("PonkoJointTrack: joint is not in the skeleton"));
        };
        let track = track_from_c(t)?;
        let at = match joints.iter().position(|j| j.joint == index) {
            Some(at) => at,
            None => {
                joints.push(JointAnimation::new(index, joint.name.clone()));
                joints.len() - 1
            }
        };
        let slot = match t.channel {
            0 => &mut joints[at].translation,
            1 => &mut joints[at].rotation,
            2 => &mut joints[at].scale,
            _ => return Err(invalid("PonkoJointTrack: channel is not a PonkoJointChannel")),
        };
        if slot.replace(track).is_some() {
            return Err(invalid("PonkoJointTrack: a joint's channel is given twice"));
        }
    }
    Ok(joints)
}

/// Makes a clip called `name` from joint tracks, for the skeleton of
/// `skeleton_entity` (Rust `AnimationClip::try_new` with
/// `JointAnimation`s): keys for each joint's position, rotation and scale,
/// in any order. Joints are named after that skeleton's, so the clip also
/// plays on skeletons with the same joint names. Bake it into a crowd with
/// `ponko_ctx_bake_crowd_animation`, or pose a skeleton with
/// `ponko_ctx_sample_clip`; add morph tracks with
/// `ponko_clip_set_morph_track`. A joint outside the skeleton, a channel
/// given twice, or bad keys are `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `name` and `tracks` (with their arrays) are read during the
/// call only. The caller owns the clip written to `out_clip` and frees it
/// with `ponko_clip_free`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_create_clip(std::ptr::null_mut(), 0, c"walk".as_ptr(), std::ptr::null(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` a C string; `tracks` holds `track_count`
/// entries whose arrays are as `PonkoJointTrack` says (or is null with
/// `track_count` 0); `out_clip` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_clip(
    ctx: *mut PonkoCtx,
    skeleton_entity: u64,
    name: *const c_char,
    tracks: *const PonkoJointTrack,
    track_count: usize,
    out_clip: *mut *mut PonkoClip,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_clip.is_null() {
                return Err(invalid("output is null"));
            }
            let tracks = match (tracks.is_null(), track_count) {
                (_, 0) => &[][..],
                (true, _) => return Err(invalid("tracks is null")),
                (false, n) => std::slice::from_raw_parts(tracks, n),
            };
            let e = Entity::from_bits(skeleton_entity);
            let skeleton = c.world.get::<Skeleton>(e).ok_or_else(|| {
                invalid("that entity has no skeleton (ponko_ctx_set_skeleton or ponko_ctx_load_skeleton)")
            })?;
            let joints = joints_from_c(skeleton, tracks)?;
            let clip = AnimationClip::try_new(name, skeleton, joints).map_err(invalid_owned)?;
            out(out_clip, Box::into_raw(Box::new(PonkoClip(clip))))
        })
    })
}

/// The version of `PonkoLiveCountsEx` this header describes.
pub const PONKO_LIVE_COUNTS_EX_VERSION: u32 = 1;

/// How many handles of each kind are live (Rust `LiveCounts`), in the
/// versioned form that grows with new kinds: everything `PonkoLiveCounts`
/// has, plus crowds, instance cells, collision meshes, sprite shaders,
/// sprite materials and sky shaders. Set `struct_size` and `version`, then
/// call `ponko_ctx_live_counts_ex`.
///
/// Version 1: every field below.
/// ```
/// let c: ponko_ffi::PonkoLiveCountsEx = unsafe { std::mem::zeroed() };
/// assert_eq!(c.crowds, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoLiveCountsEx {
    /// `sizeof(PonkoLiveCountsEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_LIVE_COUNTS_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// The counts `ponko_ctx_live_counts` gives.
    pub counts: crate::PonkoLiveCounts,
    /// Crowds of skinned characters (`ponko_ctx_create_crowd`).
    pub crowds: usize,
    /// Instance cells (parts of instance batches).
    pub instance_cells: usize,
    /// Collision meshes (3D physics).
    pub collision_meshes: usize,
    /// Sprite shaders.
    pub sprite_shaders: usize,
    /// Sprite materials.
    pub sprite_materials: usize,
    /// Sky shaders.
    pub sky_shaders: usize,
}

impl Versioned for PonkoLiveCountsEx {
    const NAME: &'static str = "PonkoLiveCountsEx";
    const VERSION: u32 = PONKO_LIVE_COUNTS_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, sky_shaders: usize)),
            _ => None,
        }
    }
}

/// Counts live handles of every kind (Rust `Ctx::live_counts`) into the
/// fields of `out_counts`'s version: a leak check that also sees crowds,
/// instance cells, collision meshes, sprite shaders and materials and sky
/// shaders.
///
/// Ownership: `out_counts` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_live_counts_ex(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_counts` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_live_counts_ex(
    ctx: *mut PonkoCtx,
    out_counts: *mut PonkoLiveCountsEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let n = c.live_counts();
            let ex = PonkoLiveCountsEx {
                struct_size: size_of::<PonkoLiveCountsEx>() as u32,
                version: PONKO_LIVE_COUNTS_EX_VERSION,
                counts: n.into(),
                crowds: n.crowds,
                instance_cells: n.instance_cells,
                collision_meshes: n.collision_meshes,
                sprite_shaders: n.sprite_shaders,
                sprite_materials: n.sprite_materials,
                sky_shaders: n.sky_shaders,
            };
            write_desc(out_counts, &ex)
        })
    })
}

const _: () = assert!(offset_of!(PonkoLiveCountsEx, struct_size) == 0 && offset_of!(PonkoLiveCountsEx, version) == 4);
