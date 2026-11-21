//! Camera rigs and screen/world conversion (Rust `ponko::camera`, the
//! `d2` and `d3` rigs, and the cameras' conversion methods).
//!
//! Each rig is a versioned descriptor (STABILITY.md, "Versioned
//! descriptors"; the reading rules are in `descriptors.rs`): its settings,
//! then its state, which the rig's calls keep. The caller owns the struct
//! (on the stack, in a component, anywhere), sets its header, fills it with
//! the rig's `_defaults` call, changes the settings it wants, and passes it
//! to the rig's `_update` call once a frame. Nothing is allocated and
//! nothing is kept between calls: the state travels in the struct.
//!
//! ```c
//! PonkoFollowCamera2D follow = {.struct_size = sizeof follow, .version = PONKO_FOLLOW_CAMERA2D_VERSION};
//! ponko_follow_camera2d_defaults(&follow);
//! follow.look_ahead = (PonkoVec2){48, 0};
//! // every frame:
//! ponko_follow_camera2d_update(&follow, ctx, hero_position, ponko_ctx_dt(ctx));
//! ```

use std::mem::{offset_of, size_of};

use ponko::camera::{CameraShake, CameraTarget, damp, damp_angle, smooth_damp};
use ponko::d2::{FollowCamera, PixelPerfect};
use ponko::d3::{FirstPersonCamera, OrbitCamera, ThirdPersonCamera};
use ponko::prelude::{Rect, Vec2, Vec3};

use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::types::{PonkoRect, PonkoVec2, to_c};
use crate::*;

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// Implements `Versioned` for a version-1 descriptor ending at `$last`.
macro_rules! versioned_v1 {
    ($t:ty, $name:literal, $version:ident, $last:ident: $lt:ty) => {
        const _: () = assert!(offset_of!($t, struct_size) == 0 && offset_of!($t, version) == 4);
        impl Versioned for $t {
            const NAME: &'static str = $name;
            const VERSION: u32 = $version;
            fn len_of(version: u32) -> Option<usize> {
                match version {
                    1 => Some(end_of!(Self, $last: $lt)),
                    _ => None,
                }
            }
        }
    };
}

fn v2(v: PonkoVec2) -> Vec2 {
    to_c(v)
}

fn v3(v: PonkoVec3) -> Vec3 {
    to_c(v)
}

// ---- smoothing ----------------------------------------------------------------

/// Moves `current` toward `target` like a critically damped spring (Rust
/// `ponko::camera::smooth_damp`): eases in and settles without overshoot.
/// `velocity` is the spring's speed: keep it between calls, starting at 0
/// (null: from rest, not written back). `smooth_time` is roughly how long
/// it takes to get most of the way; 0 or less jumps. Exact, so the same at
/// every frame rate.
/// # Safety
/// `velocity` null or a valid, writable float.
/// ```
/// let mut v = 0.0;
/// let at = unsafe { ponko_ffi::ponko_smooth_damp(0.0, 10.0, &mut v, 0.2, 10.0) };
/// assert!((at - 10.0).abs() < 1e-3);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_smooth_damp(
    current: f32,
    target: f32,
    velocity: *mut f32,
    smooth_time: f32,
    dt: f32,
) -> f32 {
    // SAFETY: caller contract.
    let mut v = if velocity.is_null() { 0.0 } else { unsafe { *velocity } };
    let at = smooth_damp(current, target, &mut v, smooth_time, dt);
    if !velocity.is_null() {
        // SAFETY: caller contract.
        unsafe { *velocity = v };
    }
    at
}

/// `ponko_smooth_damp` for 2D points.
/// # Safety
/// `velocity` null or a valid, writable `PonkoVec2`.
/// ```
/// use ponko_ffi::*;
/// let mut v = PonkoVec2::default();
/// let at = unsafe { ponko_smooth_damp_vec2(PonkoVec2 { x: 0.0, y: 0.0 }, PonkoVec2 { x: 4.0, y: 2.0 }, &mut v, 0.1, 5.0) };
/// assert!((at.x - 4.0).abs() < 1e-3);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_smooth_damp_vec2(
    current: PonkoVec2,
    target: PonkoVec2,
    velocity: *mut PonkoVec2,
    smooth_time: f32,
    dt: f32,
) -> PonkoVec2 {
    // SAFETY: caller contract.
    let mut v = if velocity.is_null() {
        Vec2::ZERO
    } else {
        v2(unsafe { *velocity })
    };
    let at = smooth_damp(v2(current), v2(target), &mut v, smooth_time, dt);
    if !velocity.is_null() {
        // SAFETY: caller contract.
        unsafe { *velocity = to_c(v) };
    }
    to_c(at)
}

/// `ponko_smooth_damp` for 3D points.
/// # Safety
/// `velocity` null or a valid, writable `PonkoVec3`.
/// ```
/// use ponko_ffi::*;
/// let at = unsafe { ponko_smooth_damp_vec3(PonkoVec3::default(), PonkoVec3 { x: 1.0, y: 2.0, z: 3.0 }, std::ptr::null_mut(), 0.0, 0.016) };
/// assert_eq!(at.z, 3.0); // no smooth time: jumps
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_smooth_damp_vec3(
    current: PonkoVec3,
    target: PonkoVec3,
    velocity: *mut PonkoVec3,
    smooth_time: f32,
    dt: f32,
) -> PonkoVec3 {
    // SAFETY: caller contract.
    let mut v = if velocity.is_null() {
        Vec3::ZERO
    } else {
        v3(unsafe { *velocity })
    };
    let at = smooth_damp(v3(current), v3(target), &mut v, smooth_time, dt);
    if !velocity.is_null() {
        // SAFETY: caller contract.
        unsafe { *velocity = to_c(v) };
    }
    to_c(at)
}

/// Moves `current` toward `target`, covering half the remaining way every
/// `half_life` seconds (Rust `ponko::camera::damp`; 0 or less jumps).
/// ```
/// assert!((ponko_ffi::ponko_damp(0.0, 10.0, 0.5, 0.5) - 5.0).abs() < 1e-5);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_damp(current: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    damp(current, target, half_life, dt)
}

/// `ponko_damp` for 2D points.
/// ```
/// use ponko_ffi::*;
/// let at = ponko_damp_vec2(PonkoVec2 { x: 0.0, y: 0.0 }, PonkoVec2 { x: 2.0, y: 4.0 }, 1.0, 1.0);
/// assert!((at.y - 2.0).abs() < 1e-5);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_damp_vec2(current: PonkoVec2, target: PonkoVec2, half_life: f32, dt: f32) -> PonkoVec2 {
    to_c(damp(v2(current), v2(target), half_life, dt))
}

/// `ponko_damp` for 3D points.
/// ```
/// use ponko_ffi::*;
/// let at = ponko_damp_vec3(PonkoVec3::default(), PonkoVec3 { x: 8.0, y: 0.0, z: 0.0 }, 1.0, 2.0);
/// assert!((at.x - 6.0).abs() < 1e-4);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_damp_vec3(current: PonkoVec3, target: PonkoVec3, half_life: f32, dt: f32) -> PonkoVec3 {
    to_c(damp(v3(current), v3(target), half_life, dt))
}

/// `ponko_damp` for angles in radians, turning the short way round (Rust
/// `ponko::camera::damp_angle`). The result stays near `current`.
/// ```
/// let a = ponko_ffi::ponko_damp_angle(3.0, -3.0, 0.1, 10.0);
/// assert!((a - (-3.0 + std::f32::consts::TAU)).abs() < 1e-3);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_damp_angle(current: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    damp_angle(current, target, half_life, dt)
}

// ---- shake --------------------------------------------------------------------

/// The version of `PonkoCameraShake` this header describes.
pub const PONKO_CAMERA_SHAKE_VERSION: u32 = 1;

/// Trauma-based camera shake (Rust `ponko::camera::CameraShake`): add
/// trauma when something hits, update once a frame, then apply after the
/// camera is placed for the frame. A versioned descriptor: set the header,
/// fill it with `ponko_camera_shake_defaults`.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut s: PonkoCameraShake = unsafe { std::mem::zeroed() };
/// (s.struct_size, s.version) = (size_of::<PonkoCameraShake>() as u32, PONKO_CAMERA_SHAKE_VERSION);
/// unsafe {
///     assert_eq!(ponko_camera_shake_defaults(&mut s), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_camera_shake_add_trauma(&mut s, 0.5), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_camera_shake_update(&mut s, 0.25), PonkoStatus::PONKO_OK);
/// }
/// assert_eq!(s.trauma, 0.25);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoCameraShake {
    /// `sizeof(PonkoCameraShake)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_CAMERA_SHAKE_VERSION` as the caller compiled it.
    pub version: u32,
    /// How shaken, 0 to 1.
    pub trauma: f32,
    /// Trauma lost per second. Default 1.
    pub decay: f32,
    /// The shake is trauma to this power. Default 2.
    pub exponent: f32,
    /// Wiggles per second. Default 15.
    pub frequency: f32,
    /// Farthest a full 2D shake moves the camera, in window pixels. Default 10 x 10.
    pub max_offset: PonkoVec2,
    /// Most a full 3D shake turns the camera (yaw, pitch, roll), radians.
    /// Default 3°, 3°, 5°.
    pub max_angles: PonkoVec3,
    /// Picks the noise.
    pub seed: u32,
    /// State: seconds of shake played.
    pub time: f32,
}
versioned_v1!(PonkoCameraShake, "PonkoCameraShake", PONKO_CAMERA_SHAKE_VERSION, time: f32);

fn shake_to_c(s: &CameraShake) -> PonkoCameraShake {
    PonkoCameraShake {
        struct_size: size_of::<PonkoCameraShake>() as u32,
        version: PONKO_CAMERA_SHAKE_VERSION,
        trauma: s.trauma,
        decay: s.decay,
        exponent: s.exponent,
        frequency: s.frequency,
        max_offset: to_c(s.max_offset),
        max_angles: to_c(s.max_angles),
        seed: s.seed,
        time: s.time,
    }
}

fn shake_from_c(c: &PonkoCameraShake) -> CameraShake {
    let mut s = CameraShake::default();
    s.trauma = c.trauma;
    s.decay = c.decay;
    s.exponent = c.exponent;
    s.frequency = c.frequency;
    s.max_offset = v2(c.max_offset);
    s.max_angles = v3(c.max_angles);
    s.seed = c.seed;
    s.time = c.time;
    s
}

/// Reads a shake, lets `f` change it, writes it back.
unsafe fn with_shake(shake: *mut PonkoCameraShake, f: impl FnOnce(&mut CameraShake)) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut s = shake_from_c(&read_desc(shake, shake_to_c(&CameraShake::default()))?);
        f(&mut s);
        write_desc(shake, &shake_to_c(&s))
    })
}

/// Fills the fields of `shake`'s version with the defaults (no trauma).
/// `struct_size` and `version` must be set first.
/// # Safety
/// `shake` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera_shake_defaults(shake: *mut PonkoCameraShake) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(shake, &shake_to_c(&CameraShake::default())) })
}

/// Adds `amount` of trauma, kept within 0 to 1.
/// # Safety
/// `shake` null or a readable, writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera_shake_add_trauma(shake: *mut PonkoCameraShake, amount: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_shake(shake, |s| s.add_trauma(amount)) }
}

/// Advances the shake by `dt` seconds and wears trauma off; once a frame.
/// # Safety
/// `shake` null or a readable, writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera_shake_update(shake: *mut PonkoCameraShake, dt: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_shake(shake, |s| s.update(dt)) }
}

/// Moves the 2D camera by the shake's offset now. Call it after the camera
/// is placed for the frame.
/// # Safety
/// `shake` null or a readable descriptor; `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_camera_shake_apply2d(std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera_shake_apply2d(shake: *const PonkoCameraShake, ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let s = shake_from_c(&read_desc(shake, shake_to_c(&CameraShake::default()))?);
        ctx_access(ctx, |c| {
            s.apply_2d(&mut c.camera);
            Ok(())
        })
    })
}

/// Turns the 3D camera by the shake's angles now. Call it after the camera
/// is placed for the frame.
/// # Safety
/// `shake` null or a readable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_camera_shake_apply3d(shake: *const PonkoCameraShake, ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let s = shake_from_c(&read_desc(shake, shake_to_c(&CameraShake::default()))?);
        ctx_access(ctx, |c| {
            s.apply_3d(&mut c.scene3d.camera);
            Ok(())
        })
    })
}

// ---- 2D follow ----------------------------------------------------------------

/// The version of `PonkoFollowCamera2D` this header describes.
pub const PONKO_FOLLOW_CAMERA2D_VERSION: u32 = 1;

/// A 2D camera following a target (Rust `d2::FollowCamera`): a dead zone,
/// a look-ahead, an offset and bounds. A versioned descriptor: set the
/// header, fill it with `ponko_follow_camera2d_defaults`, change the
/// settings, then `ponko_follow_camera2d_update` once a frame. The fields
/// after `bounds` are its state.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoFollowCamera2D {
    /// `sizeof(PonkoFollowCamera2D)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_FOLLOW_CAMERA2D_VERSION` as the caller compiled it.
    pub version: u32,
    /// The box the target moves in freely, world units. Default zero.
    pub dead_zone: PonkoVec2,
    /// How far ahead of the target to look, per axis. Default zero.
    pub look_ahead: PonkoVec2,
    /// Speed (world units per second) that turns the look-ahead. Default 20.
    pub look_ahead_speed: f32,
    /// Always added to where the camera looks. Default zero.
    pub offset: PonkoVec2,
    /// Roughly how long the camera takes to catch up, seconds; 0 sticks.
    /// Default 0.15.
    pub smooth_time: f32,
    /// Whether `bounds` applies. Default false.
    pub has_bounds: bool,
    /// The part of the world the camera may show, with `has_bounds`.
    pub bounds: PonkoRect,
    /// State: the world point the camera centers on.
    pub center: PonkoVec2,
    /// State: how fast `center` moves.
    pub velocity: PonkoVec2,
    /// State: the point the dead zone is centered on.
    pub focus: PonkoVec2,
    /// State: the look-ahead direction, -1, 0 or 1 per axis (a game may set it).
    pub facing: PonkoVec2,
    /// State: where the camera was heading at the last update.
    pub goal: PonkoVec2,
    /// State: the target at the last update.
    pub last_target: PonkoVec2,
    /// State: whether the camera has been put on a target yet.
    pub started: bool,
}
versioned_v1!(PonkoFollowCamera2D, "PonkoFollowCamera2D", PONKO_FOLLOW_CAMERA2D_VERSION, started: bool);

fn follow_to_c(f: &FollowCamera) -> PonkoFollowCamera2D {
    PonkoFollowCamera2D {
        struct_size: size_of::<PonkoFollowCamera2D>() as u32,
        version: PONKO_FOLLOW_CAMERA2D_VERSION,
        dead_zone: to_c(f.dead_zone),
        look_ahead: to_c(f.look_ahead),
        look_ahead_speed: f.look_ahead_speed,
        offset: to_c(f.offset),
        smooth_time: f.smooth_time,
        has_bounds: f.bounds.is_some(),
        bounds: to_c(f.bounds.unwrap_or_default()),
        center: to_c(f.center),
        velocity: to_c(f.velocity),
        focus: to_c(f.focus),
        facing: to_c(f.facing),
        goal: to_c(f.goal),
        last_target: to_c(f.last_target),
        started: f.started,
    }
}

fn follow_from_c(c: &PonkoFollowCamera2D) -> FollowCamera {
    let mut f = FollowCamera::default();
    f.dead_zone = v2(c.dead_zone);
    f.look_ahead = v2(c.look_ahead);
    f.look_ahead_speed = c.look_ahead_speed;
    f.offset = v2(c.offset);
    f.smooth_time = c.smooth_time;
    f.bounds = c.has_bounds.then(|| to_c::<PonkoRect, Rect>(c.bounds));
    f.center = v2(c.center);
    f.velocity = v2(c.velocity);
    f.focus = v2(c.focus);
    f.facing = v2(c.facing);
    f.goal = v2(c.goal);
    f.last_target = v2(c.last_target);
    f.started = c.started;
    f
}

/// Reads a follow rig, runs `f` on it with the context, writes it back.
unsafe fn with_follow(
    rig: *mut PonkoFollowCamera2D,
    ctx: *mut PonkoCtx,
    f: impl FnOnce(&mut FollowCamera, &mut ponko::prelude::Ctx),
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = follow_from_c(&read_desc(rig, follow_to_c(&FollowCamera::default()))?);
        ctx_access(ctx, |c| {
            f(&mut r, c);
            Ok(())
        })?;
        write_desc(rig, &follow_to_c(&r))
    })
}

/// Fills the fields of `rig`'s version with the defaults. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `rig` null or a writable descriptor with its header set.
/// ```
/// use ponko_ffi::*;
/// let mut f: PonkoFollowCamera2D = unsafe { std::mem::zeroed() };
/// (f.struct_size, f.version) = (size_of::<PonkoFollowCamera2D>() as u32, PONKO_FOLLOW_CAMERA2D_VERSION);
/// assert_eq!(unsafe { ponko_follow_camera2d_defaults(&mut f) }, PonkoStatus::PONKO_OK);
/// assert_eq!((f.smooth_time, f.has_bounds, f.started), (0.15, false, false));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_follow_camera2d_defaults(rig: *mut PonkoFollowCamera2D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(rig, &follow_to_c(&FollowCamera::default())) })
}

/// Moves the 2D camera toward `target` by `dt` seconds (once a frame, with
/// the frame's dt); the first call puts it there at once.
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_follow_camera2d_update(std::ptr::null_mut(), std::ptr::null_mut(), Default::default(), 0.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_follow_camera2d_update(
    rig: *mut PonkoFollowCamera2D,
    ctx: *mut PonkoCtx,
    target: PonkoVec2,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_follow(rig, ctx, |r, c| r.update(&mut c.camera, v2(target), dt)) }
}

/// Puts the 2D camera on `target` at once (a respawn, a new level).
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_follow_camera2d_snap(
    rig: *mut PonkoFollowCamera2D,
    ctx: *mut PonkoCtx,
    target: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_follow(rig, ctx, |r, c| r.snap(&mut c.camera, v2(target))) }
}

// ---- pixel perfect ------------------------------------------------------------

/// The version of `PonkoPixelPerfect` this header describes.
pub const PONKO_PIXEL_PERFECT_VERSION: u32 = 1;

/// Pixel-perfect 2D (Rust `d2::PixelPerfect`): whole-number zoom for the
/// window and a camera on the art pixel grid. A versioned descriptor: set
/// the header, fill it with `ponko_pixel_perfect_defaults`.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoPixelPerfect {
    /// `sizeof(PonkoPixelPerfect)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_PIXEL_PERFECT_VERSION` as the caller compiled it.
    pub version: u32,
    /// The size in art pixels the game is drawn for. Default 320 x 180.
    pub resolution: PonkoVec2,
    /// Art pixels per world unit. Default 1.
    pub pixels_per_unit: f32,
    /// Move the camera in whole art pixels (off: whole window pixels).
    /// Default true.
    pub snap: bool,
}
versioned_v1!(PonkoPixelPerfect, "PonkoPixelPerfect", PONKO_PIXEL_PERFECT_VERSION, snap: bool);

fn pixels_to_c(p: &PixelPerfect) -> PonkoPixelPerfect {
    PonkoPixelPerfect {
        struct_size: size_of::<PonkoPixelPerfect>() as u32,
        version: PONKO_PIXEL_PERFECT_VERSION,
        resolution: to_c(p.resolution),
        pixels_per_unit: p.pixels_per_unit,
        snap: p.snap,
    }
}

unsafe fn pixels_from(ptr: *const PonkoPixelPerfect) -> Result<PixelPerfect, Failure> {
    // SAFETY: forwarded caller contract.
    let c = unsafe { read_desc(ptr, pixels_to_c(&PixelPerfect::default()))? };
    let mut p = PixelPerfect::new(c.resolution.x, c.resolution.y);
    p.pixels_per_unit = c.pixels_per_unit;
    p.snap = c.snap;
    Ok(p)
}

/// Fills the fields of `pixels`' version with the defaults. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `pixels` null or a writable descriptor with its header set.
/// ```
/// use ponko_ffi::*;
/// let mut p: PonkoPixelPerfect = unsafe { std::mem::zeroed() };
/// (p.struct_size, p.version) = (size_of::<PonkoPixelPerfect>() as u32, PONKO_PIXEL_PERFECT_VERSION);
/// assert_eq!(unsafe { ponko_pixel_perfect_defaults(&mut p) }, PonkoStatus::PONKO_OK);
/// assert_eq!((p.resolution.x, p.snap), (320.0, true));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_pixel_perfect_defaults(pixels: *mut PonkoPixelPerfect) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(pixels, &pixels_to_c(&PixelPerfect::default())) })
}

/// Sets the 2D camera's zoom to the whole scale for the window, keeping
/// its center, and puts it on the pixel grid. Once a frame, after whatever
/// placed the camera.
/// # Safety
/// `pixels` null or a readable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_pixel_perfect_apply(
    pixels: *const PonkoPixelPerfect,
    ctx: *mut PonkoCtx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = pixels_from(pixels)?;
        ctx_access(ctx, |c| {
            p.apply(&mut c.camera);
            Ok(())
        })
    })
}

/// Where in the window exactly the resolution shows at the whole scale,
/// centered, in window pixels: draw bars around it to show nothing more.
/// # Safety
/// `pixels` null or a readable descriptor; `ctx` null or live; `out_rect` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_pixel_perfect_screen_rect(
    pixels: *const PonkoPixelPerfect,
    ctx: *mut PonkoCtx,
    out_rect: *mut PonkoRect,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = pixels_from(pixels)?;
        let r = ctx_access(ctx, |c| Ok(p.screen_rect(c.camera.viewport())))?;
        out(out_rect, to_c(r))
    })
}

// ---- 3D orbit -----------------------------------------------------------------

/// The version of `PonkoOrbitCamera` this header describes.
pub const PONKO_ORBIT_CAMERA_VERSION: u32 = 1;

/// A 3D camera circling a point (Rust `d3::OrbitCamera`). Set `target`,
/// `yaw`, `pitch` and `distance` (where it should be; the camera eases
/// there), then `ponko_orbit_camera_update` once a frame. The fields after
/// `smoothing` are its state.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoOrbitCamera {
    /// `sizeof(PonkoOrbitCamera)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_ORBIT_CAMERA_VERSION` as the caller compiled it.
    pub version: u32,
    /// The point it circles and looks at.
    pub target: PonkoVec3,
    /// Turn about +Y, radians (0: looking along -Z).
    pub yaw: f32,
    /// Up and down, radians; negative looks down. Default -0.5.
    pub pitch: f32,
    /// Distance from the target. Default 10.
    pub distance: f32,
    /// Lowest pitch. Default -1.5.
    pub min_pitch: f32,
    /// Highest pitch. Default 1.5.
    pub max_pitch: f32,
    /// Closest distance. Default 1.
    pub min_distance: f32,
    /// Farthest distance. Default 500.
    pub max_distance: f32,
    /// Radians per pixel dragged. Default 0.005.
    pub sensitivity: f32,
    /// Half-life of the easing, seconds; 0 moves at once. Default 0.05.
    pub smoothing: f32,
    /// State: the target the camera is at now.
    pub current_target: PonkoVec3,
    /// State: the yaw, pitch and distance the camera is at now.
    pub current: PonkoVec3,
    /// State: the target at the last update.
    pub last_target: PonkoVec3,
    /// State: the yaw, pitch and distance wanted at the last update.
    pub last_goal: PonkoVec3,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}
versioned_v1!(PonkoOrbitCamera, "PonkoOrbitCamera", PONKO_ORBIT_CAMERA_VERSION, started: bool);

fn orbit_to_c(o: &OrbitCamera) -> PonkoOrbitCamera {
    PonkoOrbitCamera {
        struct_size: size_of::<PonkoOrbitCamera>() as u32,
        version: PONKO_ORBIT_CAMERA_VERSION,
        target: to_c(o.target),
        yaw: o.yaw,
        pitch: o.pitch,
        distance: o.distance,
        min_pitch: o.min_pitch,
        max_pitch: o.max_pitch,
        min_distance: o.min_distance,
        max_distance: o.max_distance,
        sensitivity: o.sensitivity,
        smoothing: o.smoothing,
        current_target: to_c(o.current_target),
        current: to_c(o.current),
        last_target: to_c(o.last_target),
        last_goal: to_c(o.last_goal),
        started: o.started,
    }
}

fn orbit_from_c(c: &PonkoOrbitCamera) -> OrbitCamera {
    let mut o = OrbitCamera::default();
    o.target = v3(c.target);
    o.yaw = c.yaw;
    o.pitch = c.pitch;
    o.distance = c.distance;
    o.min_pitch = c.min_pitch;
    o.max_pitch = c.max_pitch;
    o.min_distance = c.min_distance;
    o.max_distance = c.max_distance;
    o.sensitivity = c.sensitivity;
    o.smoothing = c.smoothing;
    o.current_target = v3(c.current_target);
    o.current = v3(c.current);
    o.last_target = v3(c.last_target);
    o.last_goal = v3(c.last_goal);
    o.started = c.started;
    o
}

/// Reads an orbit rig, runs `f` on it with the context, writes it back.
unsafe fn with_orbit(
    rig: *mut PonkoOrbitCamera,
    ctx: *mut PonkoCtx,
    f: impl FnOnce(&mut OrbitCamera, &mut ponko::prelude::Ctx),
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = orbit_from_c(&read_desc(rig, orbit_to_c(&OrbitCamera::default()))?);
        ctx_access(ctx, |c| {
            f(&mut r, c);
            Ok(())
        })?;
        write_desc(rig, &orbit_to_c(&r))
    })
}

/// Fills the fields of `rig`'s version with the defaults. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `rig` null or a writable descriptor with its header set.
/// ```
/// use ponko_ffi::*;
/// let mut o: PonkoOrbitCamera = unsafe { std::mem::zeroed() };
/// (o.struct_size, o.version) = (size_of::<PonkoOrbitCamera>() as u32, PONKO_ORBIT_CAMERA_VERSION);
/// assert_eq!(unsafe { ponko_orbit_camera_defaults(&mut o) }, PonkoStatus::PONKO_OK);
/// assert_eq!((o.pitch, o.distance), (-0.5, 10.0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_orbit_camera_defaults(rig: *mut PonkoOrbitCamera) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(rig, &orbit_to_c(&OrbitCamera::default())) })
}

/// The usual mouse controls from this frame's input: drag with the left
/// button to turn, the right or middle button to pan, scroll to zoom. Call
/// it before `ponko_orbit_camera_update`.
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_orbit_camera_mouse_controls(
    rig: *mut PonkoOrbitCamera,
    ctx: *mut PonkoCtx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_orbit(rig, ctx, |r, c| r.mouse_controls(&c.input)) }
}

/// Eases the 3D camera toward the orbit's goal by `dt` seconds and places
/// it (keeping the goal within its limits).
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_orbit_camera_update(
    rig: *mut PonkoOrbitCamera,
    ctx: *mut PonkoCtx,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_orbit(rig, ctx, |r, c| r.update(&mut c.scene3d.camera, dt)) }
}

// ---- 3D first person ------------------------------------------------------------

/// The version of `PonkoFirstPersonCamera` this header describes.
pub const PONKO_FIRST_PERSON_CAMERA_VERSION: u32 = 1;

/// A first-person camera (Rust `d3::FirstPersonCamera`): turn it with
/// `ponko_first_person_camera_look`, place it at the eye with
/// `ponko_first_person_camera_update`. Walk along
/// `ponko_first_person_camera_flat_forward` and strafe along
/// `ponko_first_person_camera_flat_right`. The fields after `smoothing`
/// are its state.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoFirstPersonCamera {
    /// `sizeof(PonkoFirstPersonCamera)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_FIRST_PERSON_CAMERA_VERSION` as the caller compiled it.
    pub version: u32,
    /// Turn about +Y, radians (0: looking along -Z, positive turns left).
    pub yaw: f32,
    /// Up and down, radians; positive looks up.
    pub pitch: f32,
    /// Lowest pitch. Default -1.5.
    pub min_pitch: f32,
    /// Highest pitch. Default 1.5.
    pub max_pitch: f32,
    /// Radians per unit of mouse motion. Default 0.0025.
    pub sensitivity: f32,
    /// Half-life of the turning, seconds. Default 0 (exactly with the mouse).
    pub smoothing: f32,
    /// State: the yaw and pitch the camera is at now.
    pub current: PonkoVec2,
    /// State: the yaw and pitch wanted at the last update.
    pub last_goal: PonkoVec2,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}
versioned_v1!(
    PonkoFirstPersonCamera,
    "PonkoFirstPersonCamera",
    PONKO_FIRST_PERSON_CAMERA_VERSION,
    started: bool
);

fn first_to_c(f: &FirstPersonCamera) -> PonkoFirstPersonCamera {
    PonkoFirstPersonCamera {
        struct_size: size_of::<PonkoFirstPersonCamera>() as u32,
        version: PONKO_FIRST_PERSON_CAMERA_VERSION,
        yaw: f.yaw,
        pitch: f.pitch,
        min_pitch: f.min_pitch,
        max_pitch: f.max_pitch,
        sensitivity: f.sensitivity,
        smoothing: f.smoothing,
        current: to_c(f.current),
        last_goal: to_c(f.last_goal),
        started: f.started,
    }
}

fn first_from_c(c: &PonkoFirstPersonCamera) -> FirstPersonCamera {
    let mut f = FirstPersonCamera::default();
    f.yaw = c.yaw;
    f.pitch = c.pitch;
    f.min_pitch = c.min_pitch;
    f.max_pitch = c.max_pitch;
    f.sensitivity = c.sensitivity;
    f.smoothing = c.smoothing;
    f.current = v2(c.current);
    f.last_goal = v2(c.last_goal);
    f.started = c.started;
    f
}

unsafe fn read_first(rig: *const PonkoFirstPersonCamera) -> Result<FirstPersonCamera, Failure> {
    // SAFETY: forwarded caller contract.
    Ok(first_from_c(&unsafe {
        read_desc(rig, first_to_c(&FirstPersonCamera::default()))
    }?))
}

/// Fills the fields of `rig`'s version with the defaults. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `rig` null or a writable descriptor with its header set.
/// ```
/// use ponko_ffi::*;
/// let mut f: PonkoFirstPersonCamera = unsafe { std::mem::zeroed() };
/// (f.struct_size, f.version) = (size_of::<PonkoFirstPersonCamera>() as u32, PONKO_FIRST_PERSON_CAMERA_VERSION);
/// unsafe {
///     assert_eq!(ponko_first_person_camera_defaults(&mut f), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_first_person_camera_look(&mut f, PonkoVec2 { x: 0.0, y: -1e6 }), PonkoStatus::PONKO_OK);
/// }
/// assert_eq!(f.pitch, f.max_pitch); // looked up as far as it goes
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_first_person_camera_defaults(rig: *mut PonkoFirstPersonCamera) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(rig, &first_to_c(&FirstPersonCamera::default())) })
}

/// Turns by mouse motion `delta` (right and down positive, as
/// `ponko_ctx_mouse_motion`) times the sensitivity, within the pitch limits.
/// # Safety
/// `rig` null or a readable, writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_first_person_camera_look(
    rig: *mut PonkoFirstPersonCamera,
    delta: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = read_first(rig)?;
        r.look(v2(delta));
        write_desc(rig, &first_to_c(&r))
    })
}

/// Level forward (the way the view faces, along the ground) for `rig`'s
/// yaw, into `out_forward`: move the character this way for "forward".
/// The same as `(-sin yaw, 0, -cos yaw)`.
/// # Safety
/// `rig` null or a readable descriptor; `out_forward` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut r: PonkoFirstPersonCamera = unsafe { std::mem::zeroed() };
/// (r.struct_size, r.version) = (size_of::<PonkoFirstPersonCamera>() as u32, PONKO_FIRST_PERSON_CAMERA_VERSION);
/// let mut forward = PonkoVec3::default();
/// unsafe {
///     ponko_first_person_camera_defaults(&mut r);
///     assert_eq!(ponko_first_person_camera_flat_forward(&r, &mut forward), PonkoStatus::PONKO_OK);
/// }
/// assert_eq!((forward.x, forward.y, forward.z), (0.0, 0.0, -1.0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_first_person_camera_flat_forward(
    rig: *const PonkoFirstPersonCamera,
    out_forward: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_forward, to_c(read_first(rig)?.flat_forward())) })
}

/// Level right for `rig`'s yaw, into `out_right`: for strafing. The same
/// as `(cos yaw, 0, -sin yaw)`.
/// # Safety
/// `rig` null or a readable descriptor; `out_right` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut r: PonkoFirstPersonCamera = unsafe { std::mem::zeroed() };
/// (r.struct_size, r.version) = (size_of::<PonkoFirstPersonCamera>() as u32, PONKO_FIRST_PERSON_CAMERA_VERSION);
/// let mut right = PonkoVec3::default();
/// unsafe {
///     ponko_first_person_camera_defaults(&mut r);
///     assert_eq!(ponko_first_person_camera_flat_right(&r, &mut right), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_first_person_camera_flat_right(&r, std::ptr::null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
/// }
/// assert_eq!((right.x, right.y, right.z), (1.0, 0.0, 0.0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_first_person_camera_flat_right(
    rig: *const PonkoFirstPersonCamera,
    out_right: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_right, to_c(read_first(rig)?.flat_right())) })
}

/// Places the 3D camera at `eye`, turned to the view (eased by `smoothing`).
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_first_person_camera_update(
    rig: *mut PonkoFirstPersonCamera,
    ctx: *mut PonkoCtx,
    eye: PonkoVec3,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = read_first(rig)?;
        ctx_access(ctx, |c| {
            r.update(&mut c.scene3d.camera, v3(eye), dt);
            Ok(())
        })?;
        write_desc(rig, &first_to_c(&r))
    })
}

// ---- 3D third person ------------------------------------------------------------

/// The version of `PonkoThirdPersonCamera` this header describes.
pub const PONKO_THIRD_PERSON_CAMERA_VERSION: u32 = 1;

/// A third-person camera behind a character (Rust `d3::ThirdPersonCamera`):
/// orbits a pivot above it, follows it smoothly, and comes in front of
/// walls with `ponko_third_person_camera_update_physics`. Move the
/// character along `ponko_third_person_camera_flat_forward` for "forward"
/// (and `_flat_right` for right). The fields
/// after `recover_smoothing` are its state.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoThirdPersonCamera {
    /// `sizeof(PonkoThirdPersonCamera)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_THIRD_PERSON_CAMERA_VERSION` as the caller compiled it.
    pub version: u32,
    /// Turn about +Y, radians (0: behind a character facing -Z).
    pub yaw: f32,
    /// Up and down, radians; negative looks down. Default -0.3.
    pub pitch: f32,
    /// How far behind the pivot. Default 5.
    pub distance: f32,
    /// How high above the target the pivot is. Default 1.5.
    pub height: f32,
    /// How far right of the pivot (over the shoulder). Default 0.
    pub shoulder: f32,
    /// Lowest pitch. Default -1.2.
    pub min_pitch: f32,
    /// Highest pitch. Default 0.8.
    pub max_pitch: f32,
    /// Radians per unit of mouse motion. Default 0.0025.
    pub sensitivity: f32,
    /// Half-life of following the target, seconds; 0 sticks. Default 0.05.
    pub follow_smoothing: f32,
    /// The camera's size against walls. Default 0.2.
    pub collision_radius: f32,
    /// Half-life of moving back out from a wall, seconds. Default 0.2.
    pub recover_smoothing: f32,
    /// State: the pivot the camera turns around now.
    pub current_pivot: PonkoVec3,
    /// State: how far from the pivot the camera is now.
    pub current_distance: f32,
    /// State: the pivot wanted at the last update.
    pub last_pivot: PonkoVec3,
    /// State: whether the camera has been placed yet.
    pub started: bool,
}
versioned_v1!(
    PonkoThirdPersonCamera,
    "PonkoThirdPersonCamera",
    PONKO_THIRD_PERSON_CAMERA_VERSION,
    started: bool
);

fn third_to_c(t: &ThirdPersonCamera) -> PonkoThirdPersonCamera {
    PonkoThirdPersonCamera {
        struct_size: size_of::<PonkoThirdPersonCamera>() as u32,
        version: PONKO_THIRD_PERSON_CAMERA_VERSION,
        yaw: t.yaw,
        pitch: t.pitch,
        distance: t.distance,
        height: t.height,
        shoulder: t.shoulder,
        min_pitch: t.min_pitch,
        max_pitch: t.max_pitch,
        sensitivity: t.sensitivity,
        follow_smoothing: t.follow_smoothing,
        collision_radius: t.collision_radius,
        recover_smoothing: t.recover_smoothing,
        current_pivot: to_c(t.current_pivot),
        current_distance: t.current_distance,
        last_pivot: to_c(t.last_pivot),
        started: t.started,
    }
}

fn third_from_c(c: &PonkoThirdPersonCamera) -> ThirdPersonCamera {
    let mut t = ThirdPersonCamera::default();
    t.yaw = c.yaw;
    t.pitch = c.pitch;
    t.distance = c.distance;
    t.height = c.height;
    t.shoulder = c.shoulder;
    t.min_pitch = c.min_pitch;
    t.max_pitch = c.max_pitch;
    t.sensitivity = c.sensitivity;
    t.follow_smoothing = c.follow_smoothing;
    t.collision_radius = c.collision_radius;
    t.recover_smoothing = c.recover_smoothing;
    t.current_pivot = v3(c.current_pivot);
    t.current_distance = c.current_distance;
    t.last_pivot = v3(c.last_pivot);
    t.started = c.started;
    t
}

unsafe fn read_third(rig: *const PonkoThirdPersonCamera) -> Result<ThirdPersonCamera, Failure> {
    // SAFETY: forwarded caller contract.
    Ok(third_from_c(&unsafe {
        read_desc(rig, third_to_c(&ThirdPersonCamera::default()))
    }?))
}

/// Fills the fields of `rig`'s version with the defaults. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `rig` null or a writable descriptor with its header set.
/// ```
/// use ponko_ffi::*;
/// let mut t: PonkoThirdPersonCamera = unsafe { std::mem::zeroed() };
/// (t.struct_size, t.version) = (size_of::<PonkoThirdPersonCamera>() as u32, PONKO_THIRD_PERSON_CAMERA_VERSION);
/// assert_eq!(unsafe { ponko_third_person_camera_defaults(&mut t) }, PonkoStatus::PONKO_OK);
/// assert_eq!((t.distance, t.height), (5.0, 1.5));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_defaults(rig: *mut PonkoThirdPersonCamera) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(rig, &third_to_c(&ThirdPersonCamera::default())) })
}

/// Turns by mouse motion `delta` (right and down positive) times the
/// sensitivity, within the pitch limits.
/// # Safety
/// `rig` null or a readable, writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_look(
    rig: *mut PonkoThirdPersonCamera,
    delta: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = read_third(rig)?;
        r.look(v2(delta));
        write_desc(rig, &third_to_c(&r))
    })
}

/// Level forward (the way the view faces, along the ground) for `rig`'s
/// yaw, into `out_forward`: move the character this way for "forward".
/// The same as `(-sin yaw, 0, -cos yaw)`.
/// # Safety
/// `rig` null or a readable descriptor; `out_forward` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut r: PonkoThirdPersonCamera = unsafe { std::mem::zeroed() };
/// (r.struct_size, r.version) = (size_of::<PonkoThirdPersonCamera>() as u32, PONKO_THIRD_PERSON_CAMERA_VERSION);
/// let mut forward = PonkoVec3::default();
/// unsafe {
///     ponko_third_person_camera_defaults(&mut r);
///     assert_eq!(ponko_third_person_camera_flat_forward(&r, &mut forward), PonkoStatus::PONKO_OK);
/// }
/// assert_eq!((forward.x, forward.y, forward.z), (0.0, 0.0, -1.0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_flat_forward(
    rig: *const PonkoThirdPersonCamera,
    out_forward: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_forward, to_c(read_third(rig)?.flat_forward())) })
}

/// Level right for `rig`'s yaw, into `out_right`: for strafing. The same
/// as `(cos yaw, 0, -sin yaw)`.
/// # Safety
/// `rig` null or a readable descriptor; `out_right` null or writable.
/// ```
/// use ponko_ffi::*;
/// let mut r: PonkoThirdPersonCamera = unsafe { std::mem::zeroed() };
/// (r.struct_size, r.version) = (size_of::<PonkoThirdPersonCamera>() as u32, PONKO_THIRD_PERSON_CAMERA_VERSION);
/// let mut right = PonkoVec3::default();
/// unsafe {
///     ponko_third_person_camera_defaults(&mut r);
///     assert_eq!(ponko_third_person_camera_flat_right(&r, &mut right), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_third_person_camera_flat_right(&r, std::ptr::null_mut()), PonkoStatus::PONKO_INVALID_ARGUMENT);
/// }
/// assert_eq!((right.x, right.y, right.z), (1.0, 0.0, 0.0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_flat_right(
    rig: *const PonkoThirdPersonCamera,
    out_right: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { out(out_right, to_c(read_third(rig)?.flat_right())) })
}

/// Follows `target` (the character's position) by `dt` seconds and places
/// the 3D camera, without checking for walls.
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_update(
    rig: *mut PonkoThirdPersonCamera,
    ctx: *mut PonkoCtx,
    target: PonkoVec3,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = read_third(rig)?;
        ctx_access(ctx, |c| {
            r.update(&mut c.scene3d.camera, v3(target), dt);
            Ok(())
        })?;
        write_desc(rig, &third_to_c(&r))
    })
}

/// `ponko_third_person_camera_update`, keeping the camera out of the 3D
/// physics colliders (a sphere cast of `collision_radius` from the pivot).
/// `filter` null: the defaults; exclude the character's entity in it so
/// its own collider doesn't count. Needs the `physics3d` feature. Works
/// inside a `ponko_query` callback too (it moves only the 3D camera and
/// reads physics), so a loop over the characters can follow one; not in
/// `ponko_query_par`.
/// # Safety
/// `rig` null or a readable, writable descriptor; `ctx` null or live;
/// `filter` null or a readable descriptor.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_third_person_camera_update_physics(std::ptr::null_mut(), std::ptr::null_mut(), Default::default(), std::ptr::null(), 0.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_third_person_camera_update_physics(
    rig: *mut PonkoThirdPersonCamera,
    ctx: *mut PonkoCtx,
    target: PonkoVec3,
    filter: *const PonkoQueryFilterEx,
    dt: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut r = read_third(rig)?;
        let filter = filter_3d(filter)?;
        physics_update(&mut r, ctx, v3(target), filter, dt)?;
        write_desc(rig, &third_to_c(&r))
    })
}

/// A caller's versioned filter (or the defaults, for null).
unsafe fn filter_3d(filter: *const PonkoQueryFilterEx) -> Result<PonkoQueryFilterEx, Failure> {
    let mut defaults: PonkoQueryFilterEx = PonkoQueryFilterEx {
        struct_size: size_of::<PonkoQueryFilterEx>() as u32,
        version: PONKO_QUERY_FILTER_EX_VERSION,
        exclude: false,
        exclude_entity: 0,
        groups: ponko_collision_groups_all(),
        sensors: false,
    };
    // SAFETY: a local with its header set.
    let status = unsafe { ponko_query_filter_ex_defaults(&mut defaults) };
    if status != PonkoStatus::PONKO_OK {
        return Err(invalid("the default query filter"));
    }
    if filter.is_null() {
        return Ok(defaults);
    }
    // SAFETY: forwarded caller contract.
    unsafe { read_desc(filter, defaults) }
}

#[cfg(feature = "physics3d")]
unsafe fn physics_update(
    r: &mut ThirdPersonCamera,
    ctx: *mut PonkoCtx,
    target: Vec3,
    f: PonkoQueryFilterEx,
    dt: f32,
) -> Result<(), Failure> {
    use ponko::d3::{CollisionGroups, QueryFilter};
    let mut filter = QueryFilter::default();
    filter.exclude = f.exclude.then(|| ponko_ecs::Entity::from_bits(f.exclude_entity));
    filter.groups = CollisionGroups::new(f.groups.memberships, f.groups.filter);
    filter.sensors = f.sensors;
    // SAFETY: forwarded caller contract.
    unsafe {
        crate::ctx::camera3d_with_physics(ctx, |camera, physics| {
            r.update_with_physics(camera, target, physics, filter, dt);
            Ok(())
        })
    }
}

#[cfg(not(feature = "physics3d"))]
unsafe fn physics_update(
    _: &mut ThirdPersonCamera,
    _: *mut PonkoCtx,
    _: Vec3,
    _: PonkoQueryFilterEx,
    _: f32,
) -> Result<(), Failure> {
    Err(invalid(
        "the third-person camera's wall checks need the physics3d feature",
    ))
}

// ---- rigs the engine updates: CameraTarget ------------------------------------

/// Marks entity `entity` as the one the camera rigs follow (Rust
/// `ponko::camera::CameraTarget`), or takes the mark off (`on` false). Every
/// frame, at the end of the animate phase, the engine moves each rig kept
/// with a `ponko_ctx_set_*_rig` call to the marked entity's transform (2D
/// rigs read `PonkoTransform2D`, 3D rigs `PonkoTransform3D`): no camera code.
/// The first frame with a target, and whenever another entity is marked,
/// the camera cuts to it. With no marked entity the rigs leave the camera
/// alone. With several, the first one found is followed. The mark is saved
/// with the world. A dead entity: `PONKO_NOT_FOUND`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_camera_target(std::ptr::null_mut(), 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_camera_target(ctx: *mut PonkoCtx, entity: u64, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = ponko::prelude::Entity::from_bits(entity);
            if !c.world.is_alive(e) {
                return Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity is not alive"));
            }
            if on {
                c.world.insert(e, CameraTarget);
            } else {
                c.world.remove::<CameraTarget>(e);
            }
            Ok(())
        })
    })
}

/// The entity the rigs follow: the first one marked with
/// `ponko_ctx_set_camera_target`. `*out_found` says whether there is one;
/// `*out_entity` is written only then.
/// ```
/// let (mut e, mut found) = (0, false);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_camera_target(std::ptr::null_mut(), &mut e, &mut found) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_camera_target(
    ctx: *mut PonkoCtx,
    out_entity: *mut u64,
    out_found: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let target = c
                .world
                .query::<ponko::prelude::Entity>()
                .filter::<ponko::prelude::With<CameraTarget>>()
                .iter()
                .next();
            if let Some(e) = target {
                out(out_entity, e.to_bits())?;
            }
            out(out_found, target.is_some())
        })
    })
}

/// Keeps `rig` (read with `to`/`from`) as the world's resource, or takes it away.
///
/// # Safety
/// `ctx` null or live; `rig` null or a readable descriptor with its header set.
unsafe fn set_kept_rig<C: Versioned, R: Default + Send + Sync + 'static>(
    ctx: *mut PonkoCtx,
    rig: *const C,
    to: fn(&R) -> C,
    from: fn(&C) -> R,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let rig = if rig.is_null() {
            None
        } else {
            Some(from(&read_desc(rig, to(&R::default()))?))
        };
        ctx_access(ctx, |c| {
            match rig {
                Some(r) => c.world.insert_resource(r),
                None => drop(c.world.remove_resource::<R>()),
            }
            Ok(())
        })
    })
}

/// Reads the world's rig resource back into `rig`.
///
/// # Safety
/// `ctx` null or live; `rig` writable with its header set; `out_kept` writable.
unsafe fn kept_rig<C: Versioned, R: Send + Sync + 'static>(
    ctx: *mut PonkoCtx,
    rig: *mut C,
    out_kept: *mut bool,
    to: fn(&R) -> C,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let kept = ctx_access(ctx, |c| Ok(c.world.get_resource::<R>().map(to)))?;
        if let Some(k) = &kept {
            write_desc(rig, k)?;
        }
        out(out_kept, kept.is_some())
    })
}

/// Keeps a 2D follow rig (Rust `d2::FollowCamera` as a resource): it moves the 2D camera, in the world for the engine to update every frame
/// toward the entity marked with `ponko_ctx_set_camera_target`, replacing
/// the one kept before; null takes it away. The rig is copied, its state
/// included: read it back with `ponko_ctx_follow_camera2d_rig` to change a setting,
/// then set it again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_follow_camera2d_rig(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` null or a readable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_follow_camera2d_rig(
    ctx: *mut PonkoCtx,
    rig: *const PonkoFollowCamera2D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set_kept_rig::<PonkoFollowCamera2D, FollowCamera>(ctx, rig, follow_to_c, follow_from_c) }
}

/// Reads the rig `ponko_ctx_set_follow_camera2d_rig` keeps (its settings and
/// state, as the engine last updated it) into `rig`, whose header must be
/// set, and sets `*out_kept`; with none kept, `*out_kept` is false and
/// `rig` is left alone.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_follow_camera2d_rig(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` writable with its header set; `out_kept` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_follow_camera2d_rig(
    ctx: *mut PonkoCtx,
    rig: *mut PonkoFollowCamera2D,
    out_kept: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { kept_rig::<PonkoFollowCamera2D, FollowCamera>(ctx, rig, out_kept, follow_to_c) }
}

/// Keeps an orbit rig (Rust `d3::OrbitCamera` as a resource): it turns the 3D camera around the target, in the world for the engine to update every frame
/// toward the entity marked with `ponko_ctx_set_camera_target`, replacing
/// the one kept before; null takes it away. The rig is copied, its state
/// included: read it back with `ponko_ctx_orbit_camera_rig` to change a setting,
/// then set it again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_orbit_camera_rig(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` null or a readable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_orbit_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *const PonkoOrbitCamera,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set_kept_rig::<PonkoOrbitCamera, OrbitCamera>(ctx, rig, orbit_to_c, orbit_from_c) }
}

/// Reads the rig `ponko_ctx_set_orbit_camera_rig` keeps (its settings and
/// state, as the engine last updated it) into `rig`, whose header must be
/// set, and sets `*out_kept`; with none kept, `*out_kept` is false and
/// `rig` is left alone.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_orbit_camera_rig(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` writable with its header set; `out_kept` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_orbit_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *mut PonkoOrbitCamera,
    out_kept: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { kept_rig::<PonkoOrbitCamera, OrbitCamera>(ctx, rig, out_kept, orbit_to_c) }
}

/// Keeps a first-person rig (Rust `d3::FirstPersonCamera` as a resource): the 3D camera at the target's eye height, in the world for the engine to update every frame
/// toward the entity marked with `ponko_ctx_set_camera_target`, replacing
/// the one kept before; null takes it away. The rig is copied, its state
/// included: read it back with `ponko_ctx_first_person_camera_rig` to change a setting,
/// then set it again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_first_person_camera_rig(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` null or a readable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_first_person_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *const PonkoFirstPersonCamera,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set_kept_rig::<PonkoFirstPersonCamera, FirstPersonCamera>(ctx, rig, first_to_c, first_from_c) }
}

/// Reads the rig `ponko_ctx_set_first_person_camera_rig` keeps (its settings and
/// state, as the engine last updated it) into `rig`, whose header must be
/// set, and sets `*out_kept`; with none kept, `*out_kept` is false and
/// `rig` is left alone.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_first_person_camera_rig(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` writable with its header set; `out_kept` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_first_person_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *mut PonkoFirstPersonCamera,
    out_kept: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { kept_rig::<PonkoFirstPersonCamera, FirstPersonCamera>(ctx, rig, out_kept, first_to_c) }
}

/// Keeps a third-person rig (Rust `d3::ThirdPersonCamera` as a resource): the 3D camera behind the target, kept out of walls with physics3d, in the world for the engine to update every frame
/// toward the entity marked with `ponko_ctx_set_camera_target`, replacing
/// the one kept before; null takes it away. The rig is copied, its state
/// included: read it back with `ponko_ctx_third_person_camera_rig` to change a setting,
/// then set it again.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_third_person_camera_rig(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` null or a readable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_third_person_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *const PonkoThirdPersonCamera,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { set_kept_rig::<PonkoThirdPersonCamera, ThirdPersonCamera>(ctx, rig, third_to_c, third_from_c) }
}

/// Reads the rig `ponko_ctx_set_third_person_camera_rig` keeps (its settings and
/// state, as the engine last updated it) into `rig`, whose header must be
/// set, and sets `*out_kept`; with none kept, `*out_kept` is false and
/// `rig` is left alone.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_third_person_camera_rig(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `rig` writable with its header set; `out_kept` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_third_person_camera_rig(
    ctx: *mut PonkoCtx,
    rig: *mut PonkoThirdPersonCamera,
    out_kept: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { kept_rig::<PonkoThirdPersonCamera, ThirdPersonCamera>(ctx, rig, out_kept, third_to_c) }
}

// ---- screen and world -----------------------------------------------------------

/// The window point where 2D world point `world` appears, through the 2D
/// camera (Rust `ctx.camera.world_to_screen`).
/// # Safety
/// `ctx` null or live; `out_screen` writable.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_world_to_screen(std::ptr::null_mut(), Default::default(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_world_to_screen(
    ctx: *mut PonkoCtx,
    world: PonkoVec2,
    out_screen: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = ctx_access(ctx, |c| Ok(c.camera.world_to_screen(v2(world))))?;
        out(out_screen, to_c(p))
    })
}

/// The 2D world point under window point `screen` (Rust
/// `ctx.camera.screen_to_world`; `ponko_ctx_mouse_world` for the mouse).
/// # Safety
/// `ctx` null or live; `out_world` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_screen_to_world(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    out_world: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = ctx_access(ctx, |c| Ok(c.camera.screen_to_world(v2(screen))))?;
        out(out_world, to_c(p))
    })
}

/// The window point where 3D world point `world` appears through the 3D
/// camera (Rust `ctx.scene3d.camera.world_to_screen`). Behind the camera:
/// `*out_visible` false and `*out_screen` untouched.
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_world_to_screen3d(
    ctx: *mut PonkoCtx,
    world: PonkoVec3,
    out_visible: *mut bool,
    out_screen: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_visible.is_null() || out_screen.is_null() {
            return Err(invalid("output is null"));
        }
        let p = ctx_access(ctx, |c| Ok(c.scene3d.camera.world_to_screen(v3(world))))?;
        out_visible.write(p.is_some());
        if let Some(p) = p {
            out_screen.write(to_c(p));
        }
        Ok(())
    })
}

/// The 3D world point under window point `screen`, `depth` units in front
/// of the camera along its view direction (Rust
/// `ctx.scene3d.camera.screen_to_world`).
/// # Safety
/// `ctx` null or live; `out_world` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_screen_to_world3d(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    depth: f32,
    out_world: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let p = ctx_access(ctx, |c| Ok(c.scene3d.camera.screen_to_world(v2(screen), depth)))?;
        out(out_world, to_c(p))
    })
}

/// Where the ray under window point `screen` meets the level plane
/// `y = height` (Rust `ctx.scene3d.camera.screen_to_ground`). When it
/// points away from it: `*out_found` false and `*out_point` untouched.
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_screen_to_ground(
    ctx: *mut PonkoCtx,
    screen: PonkoVec2,
    height: f32,
    out_found: *mut bool,
    out_point: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_found.is_null() || out_point.is_null() {
            return Err(invalid("output is null"));
        }
        let p = ctx_access(ctx, |c| Ok(c.scene3d.camera.screen_to_ground(v2(screen), height)))?;
        out_found.write(p.is_some());
        if let Some(p) = p {
            out_point.write(to_c(p));
        }
        Ok(())
    })
}
