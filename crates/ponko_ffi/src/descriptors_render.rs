//! Versioned descriptors for the 3D picture: post-processing, particle
//! emitters and cameras (STABILITY.md, "Versioned descriptors"; the header
//! rules are in `descriptors.rs`).
//!
//! `PonkoPost`, `PonkoParticleSettings3D`, `PonkoCamera3D` and
//! `PonkoTargetCamera` have fixed layouts, so the fields their Rust structs
//! gained later (split toning, depth of field, soft particles, whether a
//! camera draws particles) live in the `...Ex` forms here. Each holds its
//! ABI 2 struct first, then the fields added since, and has its own entry
//! points next to the ABI 2 ones.

use std::mem::{offset_of, size_of};

use ponko::prelude::{Color, Vec3};
use ponko::render::{DepthOfField, Post, PostVolume, SplitToning};

use crate::ctx::ctx_access;
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::particles::{entity, settings_from_c};
use crate::three::{
    PonkoCamera3D, PonkoPost, PonkoTargetCamera, PonkoVec3, camera_from_c, camera_to_c, ponko_ctx_set_target_cameras,
    post_from_c, post_to_c, target_camera_to_c,
};
use crate::types::{PonkoColor, to_c};
use crate::{Failure, PonkoCtx, PonkoParticleSettings3D, PonkoStatus, boundary, invalid};

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// The header both fields of which every descriptor starts with.
macro_rules! assert_header {
    ($t:ty) => {
        const _: () = assert!(offset_of!($t, struct_size) == 0 && offset_of!($t, version) == 4);
    };
}

// ---- post-processing --------------------------------------------------------

/// The version of `PonkoPostEx` this header describes.
pub const PONKO_POST_EX_VERSION: u32 = 2;

/// What happens to the lit 3D frame on its way to the screen, in a layout
/// that can grow (Rust `ctx.scene3d.post`): `PonkoPost` plus the fields it
/// couldn't hold, split toning, depth of field, the sample count and thin
/// objects in reflections. Read it
/// with `ponko_ctx_post_ex` (set `struct_size` and `version` first), change
/// it, and give it back with `ponko_ctx_set_post_ex`.
///
/// Version 1: `post`, `split_toning_*`, `dof_*`, `msaa`. Version 2 adds
/// `ssr_thin_objects`; a version 1 caller leaves it as it is (off, unless
/// Rust or a newer caller turned it on).
/// ```
/// use ponko_ffi::*;
/// let mut p: PonkoPostEx = unsafe { std::mem::zeroed() };
/// (p.struct_size, p.version) = (size_of::<PonkoPostEx>() as u32, PONKO_POST_EX_VERSION);
/// assert_eq!(unsafe { ponko_post_ex_defaults(&mut p) }, PonkoStatus::PONKO_OK);
/// assert_eq!((p.dof_enabled, p.dof_focus_distance, p.msaa), (false, 10.0, 4));
/// assert!(!p.ssr_thin_objects);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoPostEx {
    /// `sizeof(PonkoPostEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_POST_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoPost`).
    pub post: PonkoPost,
    /// Split toning: the tint of the dark parts, blended in with soft light:
    /// grey (0.5, 0.5, 0.5) changes nothing, a brighter or more colorful one
    /// tints more. Alpha is ignored. Default grey.
    pub split_toning_shadows: PonkoColor,
    /// The tint of the bright parts, the same way. Default grey.
    pub split_toning_highlights: PonkoColor,
    /// Where the shadows end and the highlights begin, -1 to 1: above 0 more
    /// of the picture counts as highlights, below 0 as shadows. Default 0.
    pub split_toning_balance: f32,
    /// Depth of field: blur what is out of focus. Default false.
    pub dof_enabled: bool,
    /// How far from the camera, in world units along its view direction,
    /// things are sharp (0 or more). Default 10.
    pub dof_focus_distance: f32,
    /// How far in front of and behind the focus distance things stay fully
    /// sharp, in world units (0 or more). Default 0.
    pub dof_focus_range: f32,
    /// The largest blur: the radius of a blurred spot as a fraction of the
    /// scene's height, 0 to 0.05. Default 0.01.
    pub dof_blur: f32,
    /// Samples per pixel of the 3D frame (multisampling): 2 or 4. Default 4.
    /// `ponko_game_msaa` sets it at startup; this changes it while the game
    /// runs (the 3D targets and pipelines are made again).
    pub msaa: u32,
    /// Version 2. Screen-space reflections keep thin things (rails, cables,
    /// poles) whole without temporal antialiasing: the trace walks a coarse
    /// depth instead of its long steps, for about 0.2 ms more at 2560x1440
    /// on a fast GPU (Rust `Post::ssr_thin_objects`). Default false; the
    /// high and ultra presets turn it on. Only matters where
    /// `post.ssr` is above 0.
    pub ssr_thin_objects: bool,
}
assert_header!(PonkoPostEx);

impl Versioned for PonkoPostEx {
    const NAME: &'static str = "PonkoPostEx";
    const VERSION: u32 = PONKO_POST_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, msaa: u32)),
            2 => Some(end_of!(Self, ssr_thin_objects: bool)),
            _ => None,
        }
    }
}

/// The versioned form of Rust post settings.
fn post_ex_of(p: &Post) -> PonkoPostEx {
    PonkoPostEx {
        struct_size: size_of::<PonkoPostEx>() as u32,
        version: PONKO_POST_EX_VERSION,
        post: post_to_c(p),
        split_toning_shadows: to_c(p.split_toning.shadows),
        split_toning_highlights: to_c(p.split_toning.highlights),
        split_toning_balance: p.split_toning.balance,
        dof_enabled: p.depth_of_field.enabled,
        dof_focus_distance: p.depth_of_field.focus_distance,
        dof_focus_range: p.depth_of_field.focus_range,
        dof_blur: p.depth_of_field.blur,
        msaa: p.msaa,
        ssr_thin_objects: p.ssr_thin_objects,
    }
}

/// `base` with the settings of `ex` over it, checked: the checks of
/// `ponko_ctx_set_post`, plus the ranges of the fields added since.
fn post_of_ex(base: Post, ex: &PonkoPostEx) -> Result<Post, Failure> {
    let mut post = base;
    post_from_c(&mut post, &ex.post)?;
    let (shadows, highlights): (Color, Color) = (to_c(ex.split_toning_shadows), to_c(ex.split_toning_highlights));
    let tints = [
        shadows.r,
        shadows.g,
        shadows.b,
        highlights.r,
        highlights.g,
        highlights.b,
    ];
    if !tints.iter().all(|v| v.is_finite()) || !(-1.0..=1.0).contains(&ex.split_toning_balance) {
        return Err(invalid(
            "split toning tints must be finite and split_toning_balance -1 to 1",
        ));
    }
    let non_negative = |v: f32| v.is_finite() && v >= 0.0;
    if !non_negative(ex.dof_focus_distance)
        || !non_negative(ex.dof_focus_range)
        || !(0.0..=DepthOfField::MAX_BLUR).contains(&ex.dof_blur)
    {
        return Err(invalid(
            "dof_focus_distance and dof_focus_range can't be negative, dof_blur is 0 to 0.05",
        ));
    }
    if ex.msaa != 2 && ex.msaa != 4 {
        return Err(invalid("msaa takes 2 or 4 samples"));
    }
    post.split_toning = SplitToning {
        shadows,
        highlights,
        balance: ex.split_toning_balance,
    };
    post.depth_of_field = DepthOfField {
        enabled: ex.dof_enabled,
        focus_distance: ex.dof_focus_distance,
        focus_range: ex.dof_focus_range,
        blur: ex.dof_blur,
    };
    post.msaa = ex.msaa;
    post.ssr_thin_objects = ex.ssr_thin_objects;
    Ok(post)
}

/// Fills the fields of `post`'s version with the engine's default settings
/// (what a new game starts with). `struct_size` and `version` must be set
/// first.
/// # Safety
/// `post` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_post_ex_defaults(post: *mut PonkoPostEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(post, &post_ex_of(&Post::default())) })
}

/// Reads the 3D post-processing settings into the fields of `out_post`'s
/// version (set its `struct_size` and `version` first).
/// # Safety
/// `ctx` null or live; `out_post` null or a writable descriptor with its
/// header set.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_post_ex(std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_post_ex(ctx: *mut PonkoCtx, out_post: *mut PonkoPostEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(out_post, &post_ex_of(&c.scene3d.post))) })
}

/// Changes the 3D post-processing settings from their versioned form (the
/// checks of `ponko_ctx_set_post`, plus the split toning tints finite and its
/// balance -1 to 1, `dof_focus_distance` and `dof_focus_range` not negative,
/// `dof_blur` 0 to 0.05 and `msaa` 2 or 4). Fields newer than the caller's
/// version keep their current values. Nothing changes when it fails.
/// # Safety
/// `ctx` null or live; `post` null or a readable descriptor.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_post_ex(std::ptr::null_mut(), std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_post_ex(ctx: *mut PonkoCtx, post: *const PonkoPostEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            // Fields past the caller's version keep the current values.
            let read = read_desc(post, post_ex_of(&c.scene3d.post))?;
            c.scene3d.post = post_of_ex(c.scene3d.post, &read)?;
            Ok(())
        })
    })
}

/// The settings `t` of the way from `a` to `b` (Rust `Post::lerp`): `a` at
/// 0, `b` at 1, `t` clamped to 0 to 1 (a NaN counts as 0). Amounts and
/// colors blend; an effect on at one end and off at the other fades its
/// strength to nothing at the end where it's off; choices that can't mix
/// (the tone map, the lookup table, antialiasing, the sample count, the
/// render scale, ambient occlusion's quality) switch halfway. Both inputs are
/// checked like `ponko_ctx_set_post_ex`'s. The result is written to
/// `out_post` (set its `struct_size` and `version` first), which may be `a`
/// or `b`.
/// # Safety
/// `a` and `b` null or readable descriptors; `out_post` null or a writable
/// descriptor with its header set.
/// ```
/// let s = unsafe { ponko_ffi::ponko_post_ex_lerp(std::ptr::null(), std::ptr::null(), 0.5, std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_post_ex_lerp(
    a: *const PonkoPostEx,
    b: *const PonkoPostEx,
    t: f32,
    out_post: *mut PonkoPostEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let defaults = post_ex_of(&Post::default());
        let (from, to) = (read_desc(a, defaults)?, read_desc(b, defaults)?);
        let (from, to) = (post_of_ex(Post::default(), &from)?, post_of_ex(Post::default(), &to)?);
        write_desc(out_post, &post_ex_of(&from.lerp(&to, t)))
    })
}

/// A box in the world with post settings of its own (Rust `PostVolume`): the
/// camera inside it sees them, and over `blend_distance` outside it they fade
/// back into what is around it. A plain box that points at its settings;
/// nothing keeps the pointer past the call.
/// ```
/// let v = ponko_ffi::PonkoPostVolume {
///     min: Default::default(),
///     max: Default::default(),
///     blend_distance: 2.0,
///     post: std::ptr::null(),
/// };
/// assert_eq!(v.blend_distance, 2.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoPostVolume {
    /// The box's corner with the smallest coordinates.
    pub min: PonkoVec3,
    /// The box's corner with the largest coordinates.
    pub max: PonkoVec3,
    /// How far outside the box, in world units, its settings fade out,
    /// smoothly (0 or more). 0 switches at the box's faces.
    pub blend_distance: f32,
    /// The settings inside the box; not null.
    pub post: *const PonkoPostEx,
}

/// `base` with `count` volumes blended over it by where `at` (the camera's
/// position) is, in order: where volumes overlap, the later one wins (Rust
/// `PostVolume::blend`). The result is written to `out_post` (set its
/// `struct_size` and `version` first); give it to `ponko_ctx_set_post_ex`
/// every frame after the camera moved. A volume's `min` must not pass its
/// `max`, and its `blend_distance` is 0 or more; every setting is checked
/// like `ponko_ctx_set_post_ex`'s.
/// # Safety
/// `base` null or a readable descriptor; `volumes` holds `count` values (may
/// be null when 0), each pointing at a readable descriptor; `out_post` null or
/// a writable descriptor with its header set.
/// ```
/// let at = ponko_ffi::PonkoVec3::default();
/// let s = unsafe { ponko_ffi::ponko_post_blend_volumes(std::ptr::null(), std::ptr::null(), 0, at, std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_post_blend_volumes(
    base: *const PonkoPostEx,
    volumes: *const PonkoPostVolume,
    count: usize,
    at: PonkoVec3,
    out_post: *mut PonkoPostEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let defaults = post_ex_of(&Post::default());
        let base = post_of_ex(Post::default(), &read_desc(base, defaults)?)?;
        if volumes.is_null() && count > 0 {
            return Err(invalid("volumes is null"));
        }
        let given = if count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(volumes, count)
        };
        let at: Vec3 = to_c(at);
        if !at.is_finite() {
            return Err(invalid("at must be finite"));
        }
        let mut list = Vec::with_capacity(count);
        for v in given {
            let (min, max): (Vec3, Vec3) = (to_c(v.min), to_c(v.max));
            if !(min.is_finite() && max.is_finite() && min.cmple(max).all()) {
                return Err(invalid("a post volume needs finite corners with min not past max"));
            }
            if !(v.blend_distance.is_finite() && v.blend_distance >= 0.0) {
                return Err(invalid("a post volume's blend_distance must be 0 or more"));
            }
            list.push(PostVolume {
                min,
                max,
                blend_distance: v.blend_distance,
                post: post_of_ex(Post::default(), &read_desc(v.post, defaults)?)?,
            });
        }
        write_desc(out_post, &post_ex_of(&PostVolume::blend(&base, &list, at)))
    })
}

// ---- particles --------------------------------------------------------------

/// The version of `PonkoParticleSettings3DEx` this header describes.
pub const PONKO_PARTICLE_SETTINGS3D_EX_VERSION: u32 = 1;

/// A 3D emitter's settings, in a layout that can grow (Rust
/// `ParticleSettings3D`): `PonkoParticleSettings3D` plus the fields added
/// since ABI 2. Set `struct_size` and `version`, then
/// `ponko_particle_settings3d_ex_defaults`, and give it to an entity with
/// `ponko_ctx_set_particle_emitter3d_ex`. The key pointers inside `settings`
/// are only read during that call.
///
/// Version 1: `settings`, `softness`.
/// ```
/// use ponko_ffi::*;
/// let mut s: PonkoParticleSettings3DEx = unsafe { std::mem::zeroed() };
/// (s.struct_size, s.version) = (size_of::<PonkoParticleSettings3DEx>() as u32, PONKO_PARTICLE_SETTINGS3D_EX_VERSION);
/// assert_eq!(unsafe { ponko_particle_settings3d_ex_defaults(&mut s) }, PonkoStatus::PONKO_OK);
/// assert_eq!((s.settings.max_particles, s.softness), (1024, 0.0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoParticleSettings3DEx {
    /// `sizeof(PonkoParticleSettings3DEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_PARTICLE_SETTINGS3D_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoParticleSettings3D`).
    pub settings: PonkoParticleSettings3D,
    /// Soft particles: over this many world units in front of the scene
    /// behind them, particles fade out, so smoke meets a wall or the ground
    /// without a hard edge (0 or more). Default 0: off, drawn hard. It reads
    /// the scene's depth, which costs a copy of the frame. Target cameras
    /// draw particles without it.
    pub softness: f32,
}
assert_header!(PonkoParticleSettings3DEx);

impl Versioned for PonkoParticleSettings3DEx {
    const NAME: &'static str = "PonkoParticleSettings3DEx";
    const VERSION: u32 = PONKO_PARTICLE_SETTINGS3D_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, softness: f32)),
            _ => None,
        }
    }
}

fn particles_defaults() -> PonkoParticleSettings3DEx {
    PonkoParticleSettings3DEx {
        struct_size: size_of::<PonkoParticleSettings3DEx>() as u32,
        version: PONKO_PARTICLE_SETTINGS3D_EX_VERSION,
        settings: crate::ponko_particle_settings3d_default(),
        softness: 0.0,
    }
}

/// Fills the fields of `settings`' version with the default emitter (as
/// `ponko_particle_settings3d_default`, soft particles off). `struct_size`
/// and `version` must be set first.
/// # Safety
/// `settings` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_particle_settings3d_ex_defaults(
    settings: *mut PonkoParticleSettings3DEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(settings, &particles_defaults()) })
}

/// Gives `entity` (which needs a 3D transform to draw) a new particle
/// emitter from versioned settings, replacing any it had along with its
/// living particles (as `ponko_ctx_set_particle_emitter3d`, plus
/// `softness`, 0 or more).
/// # Safety
/// `ctx` null or live; `settings` null or a readable descriptor, its key
/// pointers as documented.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_particle_emitter3d_ex(std::ptr::null_mut(), 0, std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_particle_emitter3d_ex(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    settings: *const PonkoParticleSettings3DEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let d = read_desc(settings, particles_defaults())?;
            if !(d.softness.is_finite() && d.softness >= 0.0) {
                return Err(invalid("particle softness must be 0 or more"));
            }
            let s = settings_from_c(&d.settings)?.with_softness(d.softness);
            c.world.insert(e, ponko::d3::ParticleEmitter::new(s));
            Ok(())
        })
    })
}

// ---- cameras ----------------------------------------------------------------

/// Whether a camera draws `ctx.debug`'s 3D lines and labels (the
/// `debug_draw` of `PonkoCamera3DEx` and `PonkoTargetCameraEx`, Rust
/// `Camera::debug_draw`). Passed as a `uint32_t`: another value fails with
/// `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_eq!(ponko_ffi::PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT as u32, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoDebugDraw {
    /// What cameras drew before the flag: the window's camera draws them, a
    /// target camera while it draws particles.
    PONKO_DEBUG_DRAW_DEFAULT,
    /// Always (on the default layer).
    PONKO_DEBUG_DRAW_ON,
    /// Never.
    PONKO_DEBUG_DRAW_OFF,
}

fn debug_draw_to_c(d: Option<bool>) -> u32 {
    (match d {
        None => PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT,
        Some(true) => PonkoDebugDraw::PONKO_DEBUG_DRAW_ON,
        Some(false) => PonkoDebugDraw::PONKO_DEBUG_DRAW_OFF,
    }) as u32
}

fn debug_draw_from_c(d: u32) -> Result<Option<bool>, Failure> {
    match d {
        d if d == PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT as u32 => Ok(None),
        d if d == PonkoDebugDraw::PONKO_DEBUG_DRAW_ON as u32 => Ok(Some(true)),
        d if d == PonkoDebugDraw::PONKO_DEBUG_DRAW_OFF as u32 => Ok(Some(false)),
        _ => Err(invalid("debug_draw is not a PonkoDebugDraw")),
    }
}

/// The version of `PonkoCamera3DEx` this header describes.
pub const PONKO_CAMERA3D_EX_VERSION: u32 = 2;

/// The 3D camera, in a layout that can grow (Rust `ctx.scene3d.camera`):
/// `PonkoCamera3D` plus the fields added since ABI 2. Read it with
/// `ponko_ctx_camera3d_ex` (set `struct_size` and `version` first), change
/// it, and give it back with `ponko_ctx_set_camera3d_ex`.
///
/// Version 1: `camera`, `occlusion_culling`, `particles`. Version 2:
/// `debug_draw`.
/// ```
/// use ponko_ffi::*;
/// let c = PonkoCamera3D { position: Default::default(), rotation: ponko_quat_identity(), fov_y: 1.0, near: 0.1, far: 100.0 };
/// let mut e: PonkoCamera3DEx = unsafe { std::mem::zeroed() };
/// (e.struct_size, e.version) = (size_of::<PonkoCamera3DEx>() as u32, PONKO_CAMERA3D_EX_VERSION);
/// assert_eq!(unsafe { ponko_camera3d_ex_from(&c, &mut e) }, PonkoStatus::PONKO_OK);
/// assert_eq!((e.camera.far, e.occlusion_culling, e.particles), (100.0, false, true));
/// assert_eq!(e.debug_draw, PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT as u32);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoCamera3DEx {
    /// `sizeof(PonkoCamera3DEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_CAMERA3D_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoCamera3D`).
    pub camera: PonkoCamera3D,
    /// Skip models hidden behind what was drawn a frame or two before (as
    /// `ponko_ctx_set_occlusion_culling`). Default false.
    pub occlusion_culling: bool,
    /// Draw particles, trails and lines. Default true. Off for a camera that
    /// should show the scene without them.
    pub particles: bool,
    /// Whether it draws `ctx.debug`'s 3D lines and labels, a
    /// `PonkoDebugDraw`. Default `PONKO_DEBUG_DRAW_DEFAULT` (drawn).
    pub debug_draw: u32,
}
assert_header!(PonkoCamera3DEx);

impl Versioned for PonkoCamera3DEx {
    const NAME: &'static str = "PonkoCamera3DEx";
    const VERSION: u32 = PONKO_CAMERA3D_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, particles: bool)),
            2 => Some(end_of!(Self, debug_draw: u32)),
            _ => None,
        }
    }
}

fn camera_ex_of(c: &ponko::d3::Camera) -> PonkoCamera3DEx {
    PonkoCamera3DEx {
        struct_size: size_of::<PonkoCamera3DEx>() as u32,
        version: PONKO_CAMERA3D_EX_VERSION,
        camera: camera_to_c(c),
        occlusion_culling: c.occlusion_culling,
        particles: c.particles,
        debug_draw: debug_draw_to_c(c.debug_draw),
    }
}

/// Fills the fields of `out_camera`'s version from an ABI 2 camera, with the
/// defaults for the rest (no occlusion culling, particles drawn, debug
/// drawing `PONKO_DEBUG_DRAW_DEFAULT`).
/// `struct_size` and `version` must be set first.
/// # Safety
/// `camera` null or readable; `out_camera` null or a writable descriptor with
/// its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera3d_ex_from(
    camera: *const PonkoCamera3D,
    out_camera: *mut PonkoCamera3DEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let camera = camera.as_ref().ok_or_else(|| invalid("camera is null"))?;
        let value = PonkoCamera3DEx {
            struct_size: size_of::<PonkoCamera3DEx>() as u32,
            version: PONKO_CAMERA3D_EX_VERSION,
            camera: *camera,
            occlusion_culling: false,
            particles: true,
            debug_draw: debug_draw_to_c(None),
        };
        write_desc(out_camera, &value)
    })
}

/// Reads the 3D camera into the fields of `out_camera`'s version (set its
/// `struct_size` and `version` first).
/// # Safety
/// `ctx` null or live; `out_camera` null or a writable descriptor with its
/// header set.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_camera3d_ex(std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera3d_ex(ctx: *mut PonkoCtx, out_camera: *mut PonkoCamera3DEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(out_camera, &camera_ex_of(&c.scene3d.camera))) })
}

/// Changes the 3D camera from its versioned form (the checks of
/// `ponko_ctx_set_camera3d`). Fields newer than the caller's version keep
/// their current values. Nothing changes when it fails.
/// # Safety
/// `ctx` null or live; `camera` null or a readable descriptor.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_camera3d_ex(std::ptr::null_mut(), std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_camera3d_ex(ctx: *mut PonkoCtx, camera: *const PonkoCamera3DEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let read = read_desc(camera, camera_ex_of(&c.scene3d.camera))?;
            let mut next = camera_from_c(c.scene3d.camera, &read.camera)?;
            next.occlusion_culling = read.occlusion_culling;
            next.particles = read.particles;
            next.debug_draw = debug_draw_from_c(read.debug_draw)?;
            c.scene3d.camera = next;
            Ok(())
        })
    })
}

/// The version of `PonkoTargetCameraEx` this header describes.
pub const PONKO_TARGET_CAMERA_EX_VERSION: u32 = 2;

/// A camera drawing the 3D world into a render target each frame, in a
/// layout that can grow (Rust `TargetCamera`): `PonkoTargetCamera` plus the
/// fields added since ABI 2. Make one from an ABI 2 camera (for example the
/// result of `ponko_ctx_reflection_camera`) with `ponko_target_camera_ex_from`,
/// change `particles` for a camera that shouldn't show particles and
/// `debug_draw` for one that should show `ctx.debug`'s drawing or not
/// whatever `particles` says, and give the list to
/// `ponko_ctx_set_target_cameras_ex`.
///
/// Version 1: `target_camera`, `particles`. Version 2: `debug_draw`.
/// ```
/// use ponko_ffi::*;
/// let t = PonkoTargetCamera {
///     camera: PonkoCamera3D { position: Default::default(), rotation: ponko_quat_identity(), fov_y: 1.0, near: 0.1, far: 100.0 },
///     target: PonkoTexture { id: 0, width: 64, height: 64 },
///     has_clip_plane: false,
///     clip_plane: [0.0; 4],
/// };
/// let mut e: PonkoTargetCameraEx = unsafe { std::mem::zeroed() };
/// (e.struct_size, e.version) = (size_of::<PonkoTargetCameraEx>() as u32, PONKO_TARGET_CAMERA_EX_VERSION);
/// assert_eq!(unsafe { ponko_target_camera_ex_from(&t, &mut e) }, PonkoStatus::PONKO_OK);
/// assert!(e.particles);
/// assert_eq!(e.debug_draw, PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT as u32);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoTargetCameraEx {
    /// `sizeof(PonkoTargetCameraEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_TARGET_CAMERA_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoTargetCamera`).
    pub target_camera: PonkoTargetCamera,
    /// Draw particles, trails and lines. Default true. Off for a monitor that
    /// should show the room but not the smoke.
    pub particles: bool,
    /// Whether it draws `ctx.debug`'s 3D lines and labels, a
    /// `PonkoDebugDraw`. Default `PONKO_DEBUG_DRAW_DEFAULT`: while it draws
    /// particles (a version 1 caller gets that too).
    pub debug_draw: u32,
}
assert_header!(PonkoTargetCameraEx);

impl Versioned for PonkoTargetCameraEx {
    const NAME: &'static str = "PonkoTargetCameraEx";
    const VERSION: u32 = PONKO_TARGET_CAMERA_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, particles: bool)),
            2 => Some(end_of!(Self, debug_draw: u32)),
            _ => None,
        }
    }
}

fn target_ex_of(t: &ponko::render::TargetCamera) -> PonkoTargetCameraEx {
    PonkoTargetCameraEx {
        struct_size: size_of::<PonkoTargetCameraEx>() as u32,
        version: PONKO_TARGET_CAMERA_EX_VERSION,
        target_camera: target_camera_to_c(t),
        particles: t.camera.particles,
        debug_draw: debug_draw_to_c(t.camera.debug_draw),
    }
}

/// Fills the fields of `out_camera`'s version from an ABI 2 target camera,
/// with the defaults for the rest (particles drawn, debug drawing
/// `PONKO_DEBUG_DRAW_DEFAULT`). `struct_size` and `version` must be set first.
/// # Safety
/// `camera` null or readable; `out_camera` null or a writable descriptor with
/// its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_target_camera_ex_from(
    camera: *const PonkoTargetCamera,
    out_camera: *mut PonkoTargetCameraEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let camera = camera.as_ref().ok_or_else(|| invalid("camera is null"))?;
        let value = PonkoTargetCameraEx {
            struct_size: size_of::<PonkoTargetCameraEx>() as u32,
            version: PONKO_TARGET_CAMERA_EX_VERSION,
            target_camera: *camera,
            particles: true,
            debug_draw: debug_draw_to_c(None),
        };
        write_desc(out_camera, &value)
    })
}

/// Replaces the target cameras drawn each frame with `count` of them, from
/// their versioned forms (as `ponko_ctx_set_target_cameras`, each with its
/// `particles` and `debug_draw`); 0 draws none. `cameras` is an array of `count` pointers, so
/// each descriptor can be of its own version. Nothing changes when it fails.
/// # Safety
/// `ctx` null or live; `cameras` holds `count` pointers (may be null when 0),
/// each to a readable descriptor.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_target_cameras_ex(std::ptr::null_mut(), std::ptr::null(), 0) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_target_cameras_ex(
    ctx: *mut PonkoCtx,
    cameras: *const *const PonkoTargetCameraEx,
    count: usize,
) -> PonkoStatus {
    let mut list = Vec::with_capacity(count);
    let mut particles = Vec::with_capacity(count);
    // SAFETY: forwarded caller contract.
    let status = boundary(|| unsafe {
        if cameras.is_null() && count > 0 {
            return Err(invalid("cameras is null"));
        }
        let given = if count == 0 {
            &[][..]
        } else {
            std::slice::from_raw_parts(cameras, count)
        };
        // Fields past a caller's version keep their defaults.
        let defaults = PonkoTargetCameraEx {
            struct_size: size_of::<PonkoTargetCameraEx>() as u32,
            version: PONKO_TARGET_CAMERA_EX_VERSION,
            // SAFETY: plain data, all-zero valid.
            target_camera: std::mem::zeroed(),
            particles: true,
            debug_draw: debug_draw_to_c(None),
        };
        for &camera in given {
            let d = read_desc(camera, defaults)?;
            list.push(d.target_camera);
            particles.push((d.particles, debug_draw_from_c(d.debug_draw)?));
        }
        Ok(())
    });
    if status != PonkoStatus::PONKO_OK {
        return status;
    }
    // SAFETY: forwarded caller contract; `list` is a local.
    let status = unsafe { ponko_ctx_set_target_cameras(ctx, list.as_ptr(), list.len()) };
    if status != PonkoStatus::PONKO_OK {
        return status;
    }
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            for (t, &(on, debug_draw)) in c.scene3d.passes.target_cameras.iter_mut().zip(&particles) {
                t.camera.particles = on;
                t.camera.debug_draw = debug_draw;
            }
            Ok(())
        })
    })
}

/// Reads target camera number `index` (counting from 0, up to the count
/// `ponko_ctx_target_cameras` gives) into the fields of `out_camera`'s
/// version (set its `struct_size` and `version` first).
/// # Safety
/// `ctx` null or live; `out_camera` null or a writable descriptor with its
/// header set.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_target_camera_ex(std::ptr::null_mut(), 0, std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_target_camera_ex(
    ctx: *mut PonkoCtx,
    index: usize,
    out_camera: *mut PonkoTargetCameraEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let t = c
                .scene3d
                .passes
                .target_cameras
                .get(index)
                .ok_or_else(|| invalid("no target camera with that index"))?;
            write_desc(out_camera, &target_ex_of(t))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_one_ends_at_its_last_field() {
        assert_eq!(PonkoPostEx::len_of(1), Some(offset_of!(PonkoPostEx, msaa) + 4));
        assert_eq!(
            PonkoParticleSettings3DEx::len_of(1),
            Some(offset_of!(PonkoParticleSettings3DEx, softness) + 4)
        );
        assert_eq!(
            PonkoCamera3DEx::len_of(1),
            Some(offset_of!(PonkoCamera3DEx, particles) + 1)
        );
        assert_eq!(
            PonkoTargetCameraEx::len_of(1),
            Some(offset_of!(PonkoTargetCameraEx, particles) + 1)
        );
        assert_eq!(
            PonkoCamera3DEx::len_of(2),
            Some(offset_of!(PonkoCamera3DEx, debug_draw) + 4)
        );
        assert_eq!(
            PonkoTargetCameraEx::len_of(2),
            Some(offset_of!(PonkoTargetCameraEx, debug_draw) + 4)
        );
        assert_eq!(
            PonkoPostEx::len_of(2),
            Some(offset_of!(PonkoPostEx, ssr_thin_objects) + 1)
        );
        assert_eq!(PonkoPostEx::len_of(3), None);
    }

    /// A descriptor with its header set for version 1.
    fn header<T>(version: u32) -> T {
        // SAFETY: every descriptor is plain data, all-zero valid.
        let mut d: T = unsafe { std::mem::zeroed() };
        let words = (&raw mut d).cast::<u32>();
        // SAFETY: the first two words of a descriptor are its header.
        unsafe {
            words.write(size_of::<T>() as u32);
            words.add(1).write(version);
        }
        d
    }

    #[test]
    fn softness_and_camera_flags_reach_the_engine() {
        use crate::PonkoStatus::{PONKO_INVALID_ARGUMENT as BAD, PONKO_OK as OK};
        use crate::ctx::with_ctx;
        use crate::{
            PonkoTexture, ponko_ctx_camera3d, ponko_ctx_create_render_target, ponko_particle_settings3d_default,
        };
        use ponko::d3::{ParticleEmitter, Transform};
        use ponko::prelude::Game;

        Game::new("render descriptors", 8, 8).run_headless(0, |ctx| {
            let (soft, plain) = (
                ctx.world.spawn((Transform::default(),)),
                ctx.world.spawn((Transform::default(),)),
            );
            with_ctx(ctx, |c| {
                // SAFETY: `c` is live for this closure; every descriptor is a local.
                unsafe {
                    let mut d: PonkoParticleSettings3DEx = header(PONKO_PARTICLE_SETTINGS3D_EX_VERSION);
                    assert_eq!(ponko_particle_settings3d_ex_defaults(&mut d), OK);
                    d.softness = 1.5;
                    assert_eq!(ponko_ctx_set_particle_emitter3d_ex(c, soft.to_bits(), &d), OK);
                    d.softness = -1.0;
                    assert_eq!(
                        ponko_ctx_set_particle_emitter3d_ex(c, plain.to_bits(), &d),
                        BAD,
                        "negative"
                    );
                    d.softness = f32::NAN;
                    assert_eq!(ponko_ctx_set_particle_emitter3d_ex(c, plain.to_bits(), &d), BAD, "NaN");
                    d.softness = 0.0;
                    d.settings.lifetime_min = 0.0;
                    assert_eq!(
                        ponko_ctx_set_particle_emitter3d_ex(c, plain.to_bits(), &d),
                        BAD,
                        "the ABI 2 checks"
                    );
                    d.settings = ponko_particle_settings3d_default();
                    assert_eq!(ponko_ctx_set_particle_emitter3d_ex(c, plain.to_bits(), &d), OK);

                    // The main camera: both flags come back and change.
                    let mut cam: PonkoCamera3DEx = header(PONKO_CAMERA3D_EX_VERSION);
                    assert_eq!(ponko_ctx_camera3d_ex(c, &mut cam), OK);
                    assert!(cam.particles && !cam.occlusion_culling, "the defaults");
                    assert_eq!(cam.debug_draw, PonkoDebugDraw::PONKO_DEBUG_DRAW_DEFAULT as u32);
                    cam.particles = false;
                    cam.occlusion_culling = true;
                    cam.debug_draw = PonkoDebugDraw::PONKO_DEBUG_DRAW_OFF as u32;
                    cam.camera.fov_y = 1.0;
                    assert_eq!(ponko_ctx_set_camera3d_ex(c, &cam), OK);
                    let mut bad = cam;
                    bad.camera.near = 0.0;
                    bad.particles = true;
                    assert_eq!(ponko_ctx_set_camera3d_ex(c, &bad), BAD);
                    let mut bad = cam;
                    bad.debug_draw = 3;
                    bad.particles = true;
                    assert_eq!(ponko_ctx_set_camera3d_ex(c, &bad), BAD, "not a PonkoDebugDraw");
                    // A version 1 caller leaves the flag as it is.
                    let mut v1 = cam;
                    v1.version = 1;
                    v1.debug_draw = 3;
                    assert_eq!(ponko_ctx_set_camera3d_ex(c, &v1), OK);
                    let mut old = std::mem::zeroed::<PonkoCamera3D>();
                    assert_eq!(ponko_ctx_camera3d(c, &mut old), OK);
                    assert_eq!(old.fov_y, 1.0, "the ABI 2 call reads the same camera");

                    // Target cameras: one without particles, one with.
                    let mut target = PonkoTexture {
                        id: 0,
                        width: 0,
                        height: 0,
                    };
                    assert_eq!(ponko_ctx_create_render_target(c, 16, 16, &mut target), OK);
                    let mut abi2 = PonkoTargetCamera {
                        camera: old,
                        target,
                        has_clip_plane: false,
                        clip_plane: [0.0; 4],
                    };
                    abi2.camera.near = 0.2;
                    let mut monitor: PonkoTargetCameraEx = header(PONKO_TARGET_CAMERA_EX_VERSION);
                    assert_eq!(ponko_target_camera_ex_from(&abi2, &mut monitor), OK);
                    monitor.particles = false;
                    monitor.debug_draw = PonkoDebugDraw::PONKO_DEBUG_DRAW_ON as u32;
                    let mut mirror = monitor;
                    mirror.particles = true;
                    // A version 1 caller gets the default debug drawing.
                    mirror.version = 1;
                    mirror.debug_draw = 3;
                    let list = [&raw const monitor, &raw const mirror];
                    assert_eq!(ponko_ctx_set_target_cameras_ex(c, list.as_ptr(), 2), OK);
                    let mut read: PonkoTargetCameraEx = header(PONKO_TARGET_CAMERA_EX_VERSION);
                    assert_eq!(ponko_ctx_target_camera_ex(c, 0, &mut read), OK);
                    assert!(!read.particles && read.target_camera.camera.near == 0.2);
                    assert_eq!(read.debug_draw, PonkoDebugDraw::PONKO_DEBUG_DRAW_ON as u32);
                    let mut bad = monitor;
                    bad.debug_draw = 7;
                    let list = [&raw const bad];
                    assert_eq!(
                        ponko_ctx_set_target_cameras_ex(c, list.as_ptr(), 1),
                        BAD,
                        "not a PonkoDebugDraw"
                    );
                    assert_eq!(ponko_ctx_target_camera_ex(c, 2, &mut read), BAD, "past the end");
                    let none = [std::ptr::null::<PonkoTargetCameraEx>()];
                    assert_eq!(
                        ponko_ctx_set_target_cameras_ex(c, none.as_ptr(), 1),
                        BAD,
                        "null descriptor"
                    );
                    assert_eq!(ponko_ctx_set_target_cameras_ex(c, std::ptr::null(), 1), BAD);
                }
            });
            let softness = |e| ctx.world.get::<ParticleEmitter>(e).map(|e| e.settings().softness);
            assert_eq!((softness(soft), softness(plain)), (Some(1.5), Some(0.0)));
            let camera = ctx.scene3d.camera;
            assert!(
                !camera.particles && camera.occlusion_culling && camera.debug_draw == Some(false),
                "the refused set changed nothing"
            );
            let flags: Vec<(bool, Option<bool>)> = ctx
                .scene3d
                .passes
                .target_cameras
                .iter()
                .map(|t| (t.camera.particles, t.camera.debug_draw))
                .collect();
            assert_eq!(
                flags,
                [(false, Some(true)), (true, None)],
                "the refused calls changed nothing"
            );
        });
    }

    #[test]
    fn the_post_round_trips_through_its_versioned_form() {
        let mut p = Post::default();
        p.split_toning.shadows = Color::rgb(0.35, 0.5, 0.6);
        p.depth_of_field.enabled = true;
        p.msaa = 2;
        p.exposure = 0.5;
        let back = post_of_ex(Post::default(), &post_ex_of(&p)).ok().expect("valid");
        assert_eq!(back, p);
    }
}
