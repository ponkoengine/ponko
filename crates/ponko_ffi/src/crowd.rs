//! Crowds (Rust `Graphics::create_crowd`): thousands of skinned characters
//! in a few draws. Their clips are baked once from a skeleton
//! (`ponko_ctx_bake_crowd_animation`); each character is a
//! `PonkoCrowdInstance` (where it stands, its tint, the clip it plays and
//! how far in, a second clip it blends to), and its clips play on by
//! themselves between updates.
use std::ffi::c_char;

use ponko::__ffi;
use ponko::anim::Skeleton;
use ponko::prelude::{Color, Entity, Mat4};
use ponko::render::{Crowd, CrowdAnimation, CrowdClip, CrowdInstance, CrowdLevel, CrowdOptions};

use crate::ctx::{ctx_access, out, text};
use crate::three::{PonkoMesh, PonkoVec3};
use crate::types::{PonkoColor, to_c};
use crate::{PonkoClip, PonkoCtx, PonkoStatus, boundary, engine_error, error, invalid};

/// Clips baked for crowds (Rust `CrowdAnimation`): made with
/// `ponko_ctx_bake_crowd_animation`, freed with
/// `ponko_crowd_animation_free`. A crowd keeps what it needs, so the bake
/// can be freed once its crowds are made.
pub struct PonkoCrowdAnimation(CrowdAnimation);

/// A crowd (Rust `Crowd`), freed with `ponko_ctx_free_crowd`. Ids are not
/// reused.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoCrowd {
    /// Engine-internal id.
    pub id: u32,
}

/// A clip to bake (Rust `CrowdClip`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrowdClip {
    /// A live clip (from `ponko_ctx_load_clip`); the bake copies it.
    pub clip: *const PonkoClip,
    /// Whether it starts over at its end (a walk) or holds its last frame
    /// (a fall).
    pub looping: bool,
}

/// One level of detail of a crowd (Rust `CrowdLevel`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrowdLevel {
    /// A live skinned mesh (`ponko_ctx_create_skinned_mesh` or a skinned
    /// glTF mesh). While it is freed, the whole crowd draws nothing.
    pub mesh: PonkoMesh,
    /// Inclusive, finite, nonnegative lower distance from each view's eye.
    pub min_distance: f32,
    /// Exclusive upper distance; infinity is allowed.
    pub max_distance: f32,
    /// Drawn in the main camera and target cameras.
    pub camera: bool,
    /// Casts shadows (also needs `PonkoCrowdOptions::cast_shadows`).
    pub shadows: bool,
    /// Joints moving each corner, 1 to 4: the strongest ones.
    pub influences: u8,
}

/// One character of a crowd (Rust `CrowdInstance`). Start from
/// `ponko_crowd_instance_at`: a zero `speed` holds the pose.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrowdInstance {
    /// Model to world, column-major; finite and affine.
    pub transform: [f32; 16],
    /// Tint, in sRGB.
    pub color: PonkoColor,
    /// The clip it plays: an index into the bake
    /// (`ponko_crowd_animation_clip`).
    pub clip: u16,
    /// The clip it blends to.
    pub blend_clip: u16,
    /// Seconds into `clip` at this update.
    pub time: f32,
    /// Seconds into `blend_clip` at this update.
    pub blend_time: f32,
    /// How much of `blend_clip` it shows, 0 to 1.
    pub blend: f32,
    /// How fast both clips play on until the next update: 1 for their own
    /// pace, 0 to hold (-127 to 127, kept to 1/256).
    pub speed: f32,
}

/// Settings for a crowd (Rust `CrowdOptions`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoCrowdOptions {
    /// Casts sun, point-light and spot-light shadows from the levels that
    /// cast them.
    pub cast_shadows: bool,
}

fn skeleton(c: &ponko::prelude::Ctx, bits: u64) -> Result<&Skeleton, crate::Failure> {
    let e = Entity::from_bits(bits);
    if !c.world.is_alive(e) {
        return Err(invalid("no such entity"));
    }
    c.world
        .get::<Skeleton>(e)
        .ok_or_else(|| invalid("that entity has no skeleton (ponko_ctx_set_skeleton or ponko_ctx_load_skeleton)"))
}

fn instance_from_c(i: &PonkoCrowdInstance) -> CrowdInstance {
    CrowdInstance::new(Mat4::from_cols_array(&i.transform))
        .with_color(Color::rgba(i.color.r, i.color.g, i.color.b, i.color.a))
        .playing(i.clip, i.time)
        .blending(i.blend_clip, i.blend_time, i.blend)
        .with_speed(i.speed)
}

/// The characters behind `instances`; the engine checks their values.
///
/// # Safety
/// A non-null `instances` holds `count` entries.
unsafe fn instances_from_c(
    instances: *const PonkoCrowdInstance,
    count: usize,
) -> Result<Vec<CrowdInstance>, crate::Failure> {
    if count > ponko::render::MAX_CROWD_INSTANCES {
        return Err(invalid(
            "more characters than one crowd holds; split them into several crowds",
        ));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    if instances.is_null() {
        return Err(invalid("instances is null"));
    }
    // SAFETY: caller contract: `count` entries.
    let instances = unsafe { std::slice::from_raw_parts(instances, count) };
    Ok(instances.iter().map(instance_from_c).collect())
}

/// A character at `position`, white, playing clip 0 from its start at its
/// own pace (Rust `CrowdInstance::new`).
/// ```
/// let p = ponko_ffi::ponko_crowd_instance_at(ponko_ffi::PonkoVec3 { x: 2.0, y: 0.0, z: 0.0 });
/// assert_eq!((p.transform[12], p.speed, p.clip), (2.0, 1.0, 0));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_crowd_instance_at(position: PonkoVec3) -> PonkoCrowdInstance {
    let at = Mat4::from_translation(ponko::prelude::vec3(position.x, position.y, position.z));
    PonkoCrowdInstance {
        transform: at.to_cols_array(),
        color: PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        clip: 0,
        blend_clip: 0,
        time: 0.0,
        blend_time: 0.0,
        blend: 0.0,
        speed: 1.0,
    }
}

/// Bakes `clips` on the skeleton of `skeleton_entity` at `fps` frames a
/// second (Rust `CrowdAnimation::bake`); free the result with
/// `ponko_crowd_animation_free`. Clip `k` of the array is clip index `k`.
/// # Safety
/// `ctx` null or live; `clips` holds `clip_count` entries whose clips are
/// live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_bake_crowd_animation(std::ptr::null_mut(), 0, std::ptr::null(), 0, 30.0, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_bake_crowd_animation(
    ctx: *mut PonkoCtx,
    skeleton_entity: u64,
    clips: *const PonkoCrowdClip,
    clip_count: usize,
    fps: f32,
    output: *mut *mut PonkoCrowdAnimation,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if output.is_null() {
                return Err(invalid("output is null"));
            }
            if !fps.is_finite() || fps <= 0.0 {
                return Err(invalid("fps must be finite and > 0; 30 suits most clips"));
            }
            if clips.is_null() || clip_count == 0 || clip_count > usize::from(u16::MAX) + 1 {
                return Err(invalid("give 1 to 65,536 clips"));
            }
            let mut list = Vec::with_capacity(clip_count);
            for clip in std::slice::from_raw_parts(clips, clip_count) {
                let Some(live) = clip.clip.as_ref() else {
                    return Err(invalid("a clip is null"));
                };
                if !live.0.duration().is_finite() {
                    return Err(invalid("a clip's duration isn't finite"));
                }
                list.push(if clip.looping {
                    CrowdClip::looping(live.0.clone())
                } else {
                    CrowdClip::once(live.0.clone())
                });
            }
            let baked = CrowdAnimation::bake(skeleton(c, skeleton_entity)?, &list, fps);
            out(output, Box::into_raw(Box::new(PonkoCrowdAnimation(baked))))
        })
    })
}

/// Frees a bake; null does nothing. Crowds made from it keep drawing.
/// # Safety
/// `animation` null or from `ponko_ctx_bake_crowd_animation`, freed once.
/// ```
/// unsafe { ponko_ffi::ponko_crowd_animation_free(std::ptr::null_mut()) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_crowd_animation_free(animation: *mut PonkoCrowdAnimation) {
    if !animation.is_null() {
        // SAFETY: caller contract: from Box::into_raw, freed once.
        drop(unsafe { Box::from_raw(animation) });
    }
}

/// The index of the clip called `name` in a bake (Rust
/// `CrowdAnimation::clip`); `PONKO_NOT_FOUND` when it has none.
/// # Safety
/// `animation` null or live; `name` a C string; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_crowd_animation_clip(std::ptr::null(), c"walk".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_crowd_animation_clip(
    animation: *const PonkoCrowdAnimation,
    name: *const c_char,
    out_index: *mut u16,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let Some(animation) = animation.as_ref() else {
            return Err(invalid("animation is null"));
        };
        let name = text(name)?;
        let index = animation
            .0
            .clip(name)
            .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no clip of that name in the bake"))?;
        out(out_index, index)
    })
}

/// Makes a crowd of `count` characters drawn by `levels` (Rust
/// `Graphics::try_create_crowd`). A level whose mesh is freed, from
/// another game or not skinned, a bad distance band or influence count,
/// or a character the engine rejects is an invalid argument.
/// # Safety
/// `ctx` null or live; `animation` live; `levels` holds `level_count`
/// entries and a non-null `instances` `count`; output writable.
/// ```
/// let o = ponko_ffi::PonkoCrowdOptions { cast_shadows: true };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_create_crowd(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), 0, std::ptr::null(), 0, o, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_create_crowd(
    ctx: *mut PonkoCtx,
    animation: *const PonkoCrowdAnimation,
    levels: *const PonkoCrowdLevel,
    level_count: usize,
    instances: *const PonkoCrowdInstance,
    count: usize,
    options: PonkoCrowdOptions,
    output: *mut PonkoCrowd,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if output.is_null() {
                return Err(invalid("output is null"));
            }
            let Some(animation) = animation.as_ref() else {
                return Err(invalid("animation is null"));
            };
            if levels.is_null() || level_count == 0 || level_count > ponko::render::MAX_CROWD_LEVELS {
                return Err(invalid("give 1 to 8 crowd levels"));
            }
            let levels: Vec<CrowdLevel> = std::slice::from_raw_parts(levels, level_count)
                .iter()
                .map(|l| {
                    CrowdLevel::new(to_c(l.mesh), l.min_distance, l.max_distance)
                        .with_camera(l.camera)
                        .with_shadows(l.shadows)
                        .with_influences(l.influences)
                })
                .collect();
            let people = instances_from_c(instances, count)?;
            let options = CrowdOptions::default().with_cast_shadows(options.cast_shadows);
            let crowd = c
                .gfx
                .try_create_crowd(&animation.0, &levels, &people, options)
                .map_err(engine_error)?;
            out(output, PonkoCrowd { id: __ffi::id(crowd) })
        })
    })
}

/// Replaces a crowd's characters (Rust `Graphics::try_update_crowd`), as
/// often as every frame. A freed crowd or a character the engine rejects
/// is an invalid argument; the crowd then keeps its characters.
/// # Safety
/// `ctx` null or live; a non-null `instances` holds `count` entries.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_update_crowd(std::ptr::null_mut(), Default::default(), std::ptr::null(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_update_crowd(
    ctx: *mut PonkoCtx,
    crowd: PonkoCrowd,
    instances: *const PonkoCrowdInstance,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let people = instances_from_c(instances, count)?;
            let crowd = __ffi::from_id::<Crowd>(crowd.id);
            if !c.gfx.try_update_crowd(crowd, &people).map_err(engine_error)? {
                return Err(invalid("that crowd is freed"));
            }
            Ok(())
        })
    })
}

/// How many characters a crowd has (Rust `Graphics::crowd_len`); an
/// invalid argument for a freed crowd.
/// # Safety
/// `ctx` null or live; output writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_crowd_len(std::ptr::null_mut(), Default::default(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_crowd_len(
    ctx: *mut PonkoCtx,
    crowd: PonkoCrowd,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let len = c
                .gfx
                .crowd_len(__ffi::from_id::<Crowd>(crowd.id))
                .ok_or_else(|| invalid("that crowd is freed"))?;
            out(out_count, len)
        })
    })
}

/// Frees a crowd (Rust `Ctx::free`); repeated frees are harmless.
/// # Safety
/// `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_free_crowd(std::ptr::null_mut(), Default::default()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_free_crowd(ctx: *mut PonkoCtx, crowd: PonkoCrowd) -> PonkoStatus {
    // SAFETY: forwarded context contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.gfx.free(__ffi::from_id::<Crowd>(crowd.id));
            Ok(())
        })
    })
}
