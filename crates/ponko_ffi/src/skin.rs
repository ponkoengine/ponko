//! Skinned meshes from C: skeletons on entities, posed joint by joint
//! (Rust `Skeleton`, `SkinOf`, `AttachToJoint`, `Graphics::create_skinned_mesh`,
//! `Graphics::load_skeleton`).

use std::ffi::{CStr, c_char};

use ponko::anim::AttachToJoint;
use ponko::anim::Joint;
use ponko::anim::Skeleton;
use ponko::anim::SkinOf;
use ponko::d3::Transform as Transform3D;
use ponko::prelude::{Ctx, Entity};

use crate::ctx::{ctx_access, out, text};
use crate::three::{
    PonkoMaterial, PonkoMesh, PonkoMeshData, PonkoTransform3D, PonkoVec3, transform_from_c, transform_to_c,
};
use crate::types::PonkoColor;
use crate::types::to_c;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid, invalid_owned};

/// One joint of a skeleton (Rust `Joint`): its name, the index of the
/// joint it hangs from (earlier in the list; -1 for a root) and where it
/// sits relative to that joint at rest.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PonkoJoint {
    /// Its name (UTF-8, copied).
    pub name: *const c_char,
    /// The joint it hangs from, or -1.
    pub parent: i32,
    /// Where it sits relative to its parent at rest.
    pub rest: PonkoTransform3D,
}

fn entity(c: &Ctx, bits: u64) -> Result<Entity, Failure> {
    let e = Entity::from_bits(bits);
    if c.world.is_alive(e) {
        Ok(e)
    } else {
        Err(invalid("no such entity"))
    }
}

fn skeleton(c: &Ctx, bits: u64) -> Result<&Skeleton, Failure> {
    c.world
        .get::<Skeleton>(entity(c, bits)?)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no skeleton"))
}

fn skeleton_mut(c: &mut Ctx, bits: u64) -> Result<&mut Skeleton, Failure> {
    let e = entity(c, bits)?;
    c.world
        .get_mut::<Skeleton>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "that entity has no skeleton"))
}

fn joint_index(s: &Skeleton, joint: usize) -> Result<usize, Failure> {
    if joint < s.joints().len() {
        Ok(joint)
    } else {
        Err(invalid_owned(format!(
            "joint {joint} is past the skeleton's {}",
            s.joints().len()
        )))
    }
}

/// Gives mesh data up to four joints per corner and how much each moves it
/// (Rust `MeshData::joints` and `weights`): `joints` and `weights` hold 4
/// each per corner; `count` is the number of corners, or 0 to remove them.
/// # Safety
/// `data` null or live; `joints` and `weights` hold `4 * count` values (null when 0).
/// ```
/// use ponko_ffi::{PonkoStatus::*, *};
/// let d = ponko_mesh_data_plane(1.0);
/// let (j, w) = ([0u16; 16], [1.0f32, 0.0, 0.0, 0.0].repeat(4));
/// assert_eq!(unsafe { ponko_mesh_data_set_skin(d, j.as_ptr(), w.as_ptr(), 4) }, PONKO_OK);
/// assert_eq!(unsafe { ponko_mesh_data_set_skin(d, j.as_ptr(), w.as_ptr(), 3) }, PONKO_INVALID_ARGUMENT);
/// unsafe { ponko_mesh_data_free(d) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_set_skin(
    data: *mut PonkoMeshData,
    joints: *const u16,
    weights: *const f32,
    count: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller contract: null or live.
        let Some(data) = (unsafe { data.as_mut() }) else {
            return Err(invalid("mesh data is null"));
        };
        if count > 0 && (joints.is_null() || weights.is_null()) {
            return Err(invalid("joints or weights is null"));
        }
        let corners = data.0.positions.len();
        if count != 0 && count != corners {
            return Err(invalid_owned(format!("{count} skinned corners for {corners} corners")));
        }
        if count == 0 {
            data.0.joints = Vec::new();
            data.0.weights = Vec::new();
            return Ok(());
        }
        // SAFETY: caller passes 4 * count of each.
        let (j, w) = unsafe {
            (
                std::slice::from_raw_parts(joints, count * 4),
                std::slice::from_raw_parts(weights, count * 4),
            )
        };
        data.0.joints = j.as_chunks::<4>().0.to_vec();
        data.0.weights = w.as_chunks::<4>().0.to_vec();
        Ok(())
    })
}

/// Puts a skeleton of `count` joints, at rest, on `entity` (Rust
/// `Skeleton::new`), replacing any it had. Each joint's parent must come
/// before it.
/// # Safety
/// `ctx` null or live; `joints` holds `count` joints with valid names.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_skeleton(std::ptr::null_mut(), 0, std::ptr::null(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_skeleton(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    joints: *const PonkoJoint,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            if joints.is_null() && count > 0 {
                return Err(invalid("joints is null"));
            }
            if count > 1 << 16 {
                return Err(invalid("more than 65536 joints"));
            }
            let raw = if count == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(joints, count)
            };
            let mut list = Vec::with_capacity(count);
            for (i, j) in raw.iter().enumerate() {
                if j.name.is_null() {
                    return Err(invalid("a joint's name is null"));
                }
                let name = CStr::from_ptr(j.name)
                    .to_str()
                    .map_err(|_| invalid("a joint's name isn't UTF-8"))?;
                let parent = match j.parent {
                    -1 => None,
                    p if p >= 0 && (p as usize) < i => Some(p as usize),
                    p => {
                        return Err(invalid_owned(format!(
                            "joint {i} hangs from {p}, which doesn't come before it"
                        )));
                    }
                };
                list.push(Joint::new(name.to_string(), parent).with_rest(transform_from_c(j.rest)));
            }
            c.world.insert(e, Skeleton::try_new(list).map_err(invalid_owned)?);
            Ok(())
        })
    })
}

/// Puts the skeleton of a glTF file with skins on `entity`, at rest (Rust
/// `Graphics::load_skeleton`), loading the file's mesh if it isn't yet.
/// # Safety
/// `ctx` null or live; `path` a C string.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_skeleton(std::ptr::null_mut(), 0, c"a.glb".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_skeleton(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    path: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let path = text(path)?;
            let s = c
                .gfx
                .try_load_skeleton(path)
                .map_err(|e| invalid_owned(e.to_string()))?;
            c.world.insert(e, s);
            Ok(())
        })
    })
}

/// Makes a mesh that bends with the skeleton on `skeleton_entity` (Rust
/// `Graphics::create_skinned_mesh`): `data`'s joints index that skeleton's
/// joints, and it's modelled with the skeleton at rest.
/// # Safety
/// `ctx` null or live; `data` null or live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_create_skinned_mesh(std::ptr::null_mut(), std::ptr::null(), Default::default(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_skinned_mesh(
    ctx: *mut PonkoCtx,
    data: *const PonkoMeshData,
    material: PonkoMaterial,
    skeleton_entity: u64,
    out_mesh: *mut PonkoMesh,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(data) = data.as_ref() else {
                return Err(invalid("mesh data is null"));
            };
            if out_mesh.is_null() {
                return Err(invalid("output is null"));
            }
            // Checked here so bad input is an error, not a panic.
            data.0.check().map_err(invalid_owned)?;
            if data.0.joints.is_empty() {
                return Err(invalid("the mesh data has no joints (use ponko_ctx_create_mesh)"));
            }
            if !c.gfx.is_live(to_c(material)) {
                return Err(invalid("that material wasn't made by this game"));
            }
            let s = skeleton(c, skeleton_entity)?.clone();
            let n = s.joints().len();
            if let Some(bad) = data.0.joints.iter().flatten().find(|&&j| usize::from(j) >= n) {
                return Err(invalid_owned(format!("joint {bad} is past the skeleton's {n} joints")));
            }
            out(out_mesh, to_c(c.gfx.create_skinned_mesh(&data.0, to_c(material), &s)))
        })
    })
}

/// How many joints the skeleton on `entity` has.
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_joint_count(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_joint_count(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_count, skeleton(c, entity_bits)?.joints().len())) })
}

/// The first joint called `name` on `entity`'s skeleton (Rust
/// `Skeleton::find`): `out_found` says whether there is one, and
/// `out_joint` gets its index if so.
/// # Safety
/// `ctx` null or live; `name` a C string; outputs writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_find_joint(std::ptr::null_mut(), 0, c"hip".as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_find_joint(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    name: *const c_char,
    out_found: *mut bool,
    out_joint: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_joint.is_null() {
                return Err(invalid("output is null"));
            }
            let found = skeleton(c, entity_bits)?.find(name);
            out(out_found, found.is_some())?;
            if let Some(j) = found {
                *out_joint = j;
            }
            Ok(())
        })
    })
}

/// Where `joint` of `entity`'s skeleton sits relative to its parent this
/// frame (Rust `skeleton.pose[joint]`).
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_joint_pose(std::ptr::null_mut(), 0, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_joint_pose(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    joint: usize,
    out_pose: *mut PonkoTransform3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = skeleton(c, entity_bits)?;
            let j = joint_index(s, joint)?;
            let pose = s.pose.get(j).copied().unwrap_or(s.joints()[j].rest);
            out(out_pose, transform_to_c(pose))
        })
    })
}

/// Moves `joint` of `entity`'s skeleton: where it sits relative to its
/// parent from now on (Rust `skeleton.pose[joint] = pose`).
/// # Safety
/// `ctx` null or live; `pose` null or readable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_joint_pose(std::ptr::null_mut(), 0, 0, std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_joint_pose(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    joint: usize,
    pose: *const PonkoTransform3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(pose) = pose.as_ref() else {
                return Err(invalid("pose is null"));
            };
            let s = skeleton_mut(c, entity_bits)?;
            let j = joint_index(s, joint)?;
            if s.pose.len() <= j {
                let rest: Vec<Transform3D> = s.joints()[s.pose.len()..=j].iter().map(|x| x.rest).collect();
                s.pose.extend(rest);
            }
            s.pose[j] = transform_from_c(*pose);
            Ok(())
        })
    })
}

/// Puts every joint of `entity`'s skeleton back at rest (Rust
/// `Skeleton::reset_pose`).
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_reset_pose(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_reset_pose(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            skeleton_mut(c, entity_bits)?.reset_pose();
            Ok(())
        })
    })
}

/// `joint`'s matrix in `entity`'s own space from its pose now (Rust
/// `Skeleton::joint_matrix`), as 16 floats, column after column.
/// # Safety
/// `ctx` null or live; `out_matrix` holds 16 floats.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_joint_matrix(std::ptr::null_mut(), 0, 0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_joint_matrix(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    joint: usize,
    out_matrix: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_matrix.is_null() {
                return Err(invalid("output is null"));
            }
            let s = skeleton(c, entity_bits)?;
            let m = s.joint_matrix(joint_index(s, joint)?).to_cols_array();
            std::ptr::copy_nonoverlapping(m.as_ptr(), out_matrix, 16);
            Ok(())
        })
    })
}

/// Poses `entity`'s skinned model with `skeleton_entity`'s skeleton and
/// draws it there (Rust `SkinOf`): clothes and parts, bound by joint name.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_skin_of(std::ptr::null_mut(), 0, 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_skin_of(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    skeleton_entity: u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (e, of) = (entity(c, entity_bits)?, entity(c, skeleton_entity)?);
            c.world.insert(e, SkinOf(of));
            Ok(())
        })
    })
}

/// Keeps `entity`'s transform on `joint` of `skeleton_entity`'s skeleton,
/// moved by `offset` in the joint's space (Rust `AttachToJoint`); set just
/// before drawing.
/// # Safety
/// `ctx` null or live; `offset` null or readable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_attach_to_joint(std::ptr::null_mut(), 0, 0, 0, std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_attach_to_joint(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    skeleton_entity: u64,
    joint: usize,
    offset: *const PonkoTransform3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(offset) = offset.as_ref() else {
                return Err(invalid("offset is null"));
            };
            let (e, of) = (entity(c, entity_bits)?, entity(c, skeleton_entity)?);
            c.world
                .insert(e, AttachToJoint::new(of, joint).with_offset(transform_from_c(*offset)));
            Ok(())
        })
    })
}

/// An animation clip (Rust `AnimationClip`): made with
/// `ponko_ctx_load_clip`, freed with `ponko_clip_free`.
pub struct PonkoClip(pub(crate) ponko::anim::AnimationClip);

/// Loads the clip called `name` of a glTF file with skins (Rust
/// `Graphics::load_clip`); free it with `ponko_clip_free`.
/// `PONKO_NOT_FOUND` when the file has no clip of that name.
///
/// Ownership: on success `out_clip` receives an owned clip, the caller's; free
/// it with ponko_clip_free.
///
/// # Safety
/// `ctx` null or live; `path` and `name` C strings; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_clip(std::ptr::null_mut(), c"a.glb".as_ptr(), c"walk".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_clip(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    name: *const c_char,
    out_clip: *mut *mut PonkoClip,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (path, name) = (text(path)?, text(name)?);
            if out_clip.is_null() {
                return Err(invalid("output is null"));
            }
            let clips = c.gfx.try_load_clips(path).map_err(invalid_owned)?;
            let clip = clips
                .into_iter()
                .find(|clip| clip.name() == name)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no clip of that name in the file"))?;
            *out_clip = Box::into_raw(Box::new(PonkoClip(clip)));
            Ok(())
        })
    })
}

/// Frees a clip; null does nothing.
/// # Safety
/// `clip` null or from `ponko_ctx_load_clip`, freed once.
/// ```
/// unsafe { ponko_ffi::ponko_clip_free(std::ptr::null_mut()) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_clip_free(clip: *mut PonkoClip) {
    if !clip.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(clip) });
    }
}

/// A clip's length in seconds (0 for null).
/// # Safety
/// `clip` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_clip_duration(std::ptr::null()) }, 0.0);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_clip_duration(clip: *const PonkoClip) -> f32 {
    // SAFETY: caller contract.
    unsafe { clip.as_ref() }.map_or(0.0, |c| c.0.duration())
}

/// Poses `entity`'s skeleton with `clip` at `time` (clamped to the clip),
/// blending from its current pose by `weight` (1: the clip's pose; Rust
/// `AnimationClip::sample_blended`). Joints are matched by name when the
/// skeleton isn't the clip's own.
/// # Safety
/// `ctx` null or live; `clip` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_sample_clip(std::ptr::null_mut(), std::ptr::null(), 0, 0.0, 1.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sample_clip(
    ctx: *mut PonkoCtx,
    clip: *const PonkoClip,
    entity_bits: u64,
    time: f32,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(clip) = clip.as_ref() else {
                return Err(invalid("clip is null"));
            };
            let s = skeleton_mut(c, entity_bits)?;
            clip.0.sample_blended(time, weight, s);
            c.gfx.write_animated_morphs(&mut c.world);
            Ok(())
        })
    })
}

/// Bends a two-bone chain of `entity`'s skeleton (`upper`, its child
/// `middle`, and that one's child `end`, e.g. thigh, shin, foot) so `end`
/// reaches `target` (world space), with the middle joint bending toward
/// `pole` (a world direction). `weight` 0 to 1 blends with the current pose.
/// Out-of-reach targets get a straight chain pointing at them (Rust
/// `two_bone_ik`). Use it in a late system, after animation.
/// ```
/// let v = ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_two_bone_ik(std::ptr::null_mut(), 0, 0, 1, 2, v, v, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_two_bone_ik(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    upper: usize,
    middle: usize,
    end: usize,
    target: PonkoVec3,
    pole: PonkoVec3,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let model = c.world.get::<Transform3D>(e).copied().unwrap_or_default();
            let s = skeleton_mut(c, entity_bits)?;
            for j in [upper, middle, end] {
                joint_index(s, j)?;
            }
            ponko::anim::two_bone_ik(s, &model, [upper, middle, end], to_c(target), to_c(pole), weight);
            Ok(())
        })
    })
}

/// Turns `joint` of `entity`'s skeleton so its local `axis` points at
/// `target` (world space), by at most `max_angle` radians, blended by
/// `weight` 0 to 1 (Rust `look_at_joint`). For a spine-neck-head chain, call
/// it per joint from the root out with smaller weights.
/// ```
/// let v = ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 1.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_look_at_joint(std::ptr::null_mut(), 0, 0, v, v, 1.0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_look_at_joint(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    joint: usize,
    axis: PonkoVec3,
    target: PonkoVec3,
    max_angle: f32,
    weight: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let model = c.world.get::<Transform3D>(e).copied().unwrap_or_default();
            let s = skeleton_mut(c, entity_bits)?;
            joint_index(s, joint)?;
            ponko::anim::look_at_joint(s, &model, joint, to_c(axis), to_c(target), max_angle, weight);
            Ok(())
        })
    })
}

/// Draws `entity`'s bones this frame as lines from each joint to its
/// parent, over the scene (no depth test).
/// ```
/// let white = ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_draw_skeleton(std::ptr::null_mut(), 0, white) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_draw_skeleton(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    color: PonkoColor,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            skeleton(c, entity_bits)?;
            c.draw_skeleton(e, to_c(color));
            Ok(())
        })
    })
}
