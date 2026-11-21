//! The sky kit and the day-night driver from C (Rust `SkyKit`, `DayNight`):
//! clouds, a moon with phases and stars over the built-in atmosphere, and a
//! clock that moves the sun and the moon. Both structs are versioned
//! descriptors (STABILITY.md, "Versioned descriptors"): set `struct_size`
//! and `version`, then fill them with `ponko_sky_kit_defaults` or
//! `ponko_day_night_new`.
use std::mem::{offset_of, size_of};

use ponko::render::{CloudQuality, DayNight, SkyKit};

use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::three::PonkoVec3;
use crate::types::{PonkoColor, PonkoVec2, to_c};
use crate::{PonkoCtx, PonkoStatus, boundary, invalid};

macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// How finely the sky kit draws its clouds (`PonkoSkyKit::quality`, Rust
/// `CloudQuality`); measured costs per preset are in the guide's "The sky kit".
/// ```
/// assert_eq!(ponko_ffi::PonkoCloudQuality::PONKO_CLOUDS_MEDIUM as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoCloudQuality {
    /// A flat 2D cloud layer in the sky shader (what WebGL2 always draws).
    PONKO_CLOUDS_FLAT = 0,
    /// Volumetric, 24 steps per ray.
    PONKO_CLOUDS_LOW = 1,
    /// Volumetric, 48 steps per ray (the default).
    PONKO_CLOUDS_MEDIUM = 2,
    /// Volumetric, 96 steps per ray.
    PONKO_CLOUDS_HIGH = 3,
}

/// The version of `PonkoSkyKit` this header describes.
pub const PONKO_SKY_KIT_VERSION: u32 = 1;

/// The sky kit (Rust `SkyKit`): `ponko_ctx_set_sky_kit` turns it on with
/// these settings (or off, with `enabled` false), `ponko_ctx_sky_kit` reads
/// them. Distances are world units, taken to be meters.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut k: PonkoSkyKit = unsafe { std::mem::zeroed() };
/// (k.struct_size, k.version) = (size_of::<PonkoSkyKit>() as u32, PONKO_SKY_KIT_VERSION);
/// assert_eq!(unsafe { ponko_sky_kit_defaults(&mut k) }, PonkoStatus::PONKO_OK);
/// assert_eq!(k.coverage, 0.5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoSkyKit {
    /// `sizeof(PonkoSkyKit)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_SKY_KIT_VERSION` as the caller compiled it.
    pub version: u32,
    /// Whether the kit is on (`ponko_ctx_sky_kit`), or should be
    /// (`ponko_ctx_set_sky_kit`: false turns it off and keeps nothing).
    pub enabled: bool,
    /// A `PonkoCloudQuality`. Default `PONKO_CLOUDS_MEDIUM`.
    pub quality: u32,
    /// How much of the sky is clouded, 0 (clear) to 1 (overcast). Default 0.5.
    pub coverage: f32,
    /// 0 flat sheets (stratus) to 1 tall heaps (cumulus). Default 0.5.
    pub kind: f32,
    /// Which way and how fast the clouds drift, units per second along X
    /// and Z (`x`, `y` here). Default (6, 2).
    pub wind: PonkoVec2,
    /// How far the clouds have drifted on top of the wind (the same on
    /// every machine of a networked game). Default zero.
    pub offset: PonkoVec2,
    /// The height of the layer's base. Default 1500.
    pub altitude: f32,
    /// How deep the layer is. Default 1500.
    pub thickness: f32,
    /// How thick the clouds are inside. Default 1.
    pub density: f32,
    /// How much they shadow themselves (darker undersides). Default 1.
    pub self_shadow: f32,
    /// How their edges glow toward the sun or moon, 0 to 1. Default 0.6.
    pub silver_lining: f32,
    /// How dark their shadows on the ground are, 0 (none, no pass) to 1.
    /// Default 0.8.
    pub shadows: f32,
    /// Whether `moon_direction` places the moon; false (the default): it
    /// follows the sun as far behind as its phase says.
    pub moon_has_direction: bool,
    /// Which way moonlight travels (like the sun's direction).
    pub moon_direction: PonkoVec3,
    /// The moon's phase: 0 new, 0.25 first quarter, 0.5 full (default),
    /// 0.75 last quarter.
    pub moon_phase: f32,
    /// Its size in the sky, degrees across. Default 2.
    pub moon_size: f32,
    /// How bright it is, 0 for none. Default 1.
    pub moon_brightness: f32,
}

impl Versioned for PonkoSkyKit {
    const NAME: &'static str = "PonkoSkyKit";
    const VERSION: u32 = PONKO_SKY_KIT_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, moon_brightness: f32)),
            _ => None,
        }
    }
}

fn quality_to_c(q: CloudQuality) -> u32 {
    match q {
        CloudQuality::Flat => 0,
        CloudQuality::Low => 1,
        CloudQuality::High => 3,
        _ => 2,
    }
}

fn glam_neg_y() -> ponko::prelude::Vec3 {
    ponko::prelude::Vec3::new(0.0, -1.0, 0.0)
}

fn kit_to_c(kit: Option<SkyKit>) -> PonkoSkyKit {
    let enabled = kit.is_some();
    let k = kit.unwrap_or_default();
    let c = &k.clouds;
    let m = &k.moon;
    PonkoSkyKit {
        struct_size: size_of::<PonkoSkyKit>() as u32,
        version: PONKO_SKY_KIT_VERSION,
        enabled,
        quality: quality_to_c(k.quality),
        coverage: c.coverage,
        kind: c.kind,
        wind: to_c(c.wind),
        offset: to_c(c.offset),
        altitude: c.altitude,
        thickness: c.thickness,
        density: c.density,
        self_shadow: c.self_shadow,
        silver_lining: c.silver_lining,
        shadows: c.shadows,
        moon_has_direction: m.direction.is_some(),
        moon_direction: to_c(m.direction.unwrap_or(glam_neg_y())),
        moon_phase: m.phase,
        moon_size: m.size,
        moon_brightness: m.brightness,
    }
}

fn kit_from_c(k: &PonkoSkyKit) -> Result<Option<SkyKit>, crate::Failure> {
    if !k.enabled {
        return Ok(None);
    }
    let quality = match k.quality {
        0 => CloudQuality::Flat,
        1 => CloudQuality::Low,
        2 => CloudQuality::Medium,
        3 => CloudQuality::High,
        _ => return Err(invalid("PonkoSkyKit: quality is not a PonkoCloudQuality")),
    };
    let finite = [
        k.coverage,
        k.kind,
        k.wind.x,
        k.wind.y,
        k.offset.x,
        k.offset.y,
        k.altitude,
        k.thickness,
        k.density,
        k.self_shadow,
        k.silver_lining,
        k.shadows,
        k.moon_phase,
        k.moon_size,
        k.moon_brightness,
    ];
    if finite.iter().any(|v| !v.is_finite()) {
        return Err(invalid("PonkoSkyKit: every number must be finite"));
    }
    let mut kit = SkyKit::default().with_quality(quality);
    let c = &mut kit.clouds;
    c.coverage = k.coverage;
    c.kind = k.kind;
    c.wind = to_c(k.wind);
    c.offset = to_c(k.offset);
    c.altitude = k.altitude;
    c.thickness = k.thickness;
    c.density = k.density;
    c.self_shadow = k.self_shadow;
    c.silver_lining = k.silver_lining;
    c.shadows = k.shadows;
    let m = &mut kit.moon;
    m.direction = k
        .moon_has_direction
        .then(|| to_c::<_, ponko::prelude::Vec3>(k.moon_direction));
    m.phase = k.moon_phase;
    m.size = k.moon_size;
    m.brightness = k.moon_brightness;
    Ok(Some(kit))
}

/// Fills the fields of `kit`'s version with the kit's defaults, `enabled`
/// true (Rust `SkyKit::default()`). Set `struct_size` and `version` first.
///
/// Ownership: `kit` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_sky_kit_defaults(std::ptr::null_mut()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `kit` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_sky_kit_defaults(kit: *mut PonkoSkyKit) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(kit, &kit_to_c(Some(SkyKit::default()))) })
}

/// Reads the sky kit into the fields of `out_kit`'s version: `enabled`
/// false (and the defaults) while the game draws the plain atmosphere.
///
/// Ownership: `out_kit` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_sky_kit(std::ptr::null_mut(), std::ptr::null_mut()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `ctx` null or live; `out_kit` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sky_kit(ctx: *mut PonkoCtx, out_kit: *mut PonkoSkyKit) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(out_kit, &kit_to_c(c.scene3d.sky.kit))) })
}

/// Turns the sky kit on with `kit`'s settings (Rust `sky.kit = Some(kit)`),
/// or off when `kit->enabled` is false: clouds drifting in the wind,
/// casting shadows and tinting the fog, a moon with phases, stars turning
/// through the night. Fields newer than the caller's version keep their
/// defaults. A game that never turns it on pays nothing for it. Non-finite
/// numbers or an unknown quality are `PONKO_INVALID_ARGUMENT` and change
/// nothing.
///
/// Ownership: `kit` is read during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_sky_kit(std::ptr::null_mut(), std::ptr::null()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `ctx` null or live; `kit` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_sky_kit(ctx: *mut PonkoCtx, kit: *const PonkoSkyKit) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let k = read_desc(kit, kit_to_c(Some(SkyKit::default())))?;
            c.scene3d.sky.kit = kit_from_c(&k)?;
            Ok(())
        })
    })
}

/// The share of the moon's face that is lit at `phase`: 0 new, 1 full
/// (Rust `Moon::illumination`).
/// ```
/// assert!((ponko_ffi::ponko_moon_illumination(0.5) - 1.0).abs() < 1e-6);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_moon_illumination(phase: f32) -> f32 {
    ponko::render::Moon {
        phase,
        ..ponko::render::Moon::default()
    }
    .illumination()
}

/// The version of `PonkoDayNight` this header describes.
pub const PONKO_DAY_NIGHT_VERSION: u32 = 1;

/// A day and night cycle (Rust `DayNight`): a clock the game keeps (in its
/// own memory) that moves the sun through the day and the moon through its
/// phases. `ponko_day_night_new` starts one; call
/// `ponko_ctx_update_day_night` once a frame.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoDayNight = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoDayNight>() as u32, PONKO_DAY_NIGHT_VERSION);
/// assert_eq!(unsafe { ponko_day_night_new(7.0, &mut d) }, PonkoStatus::PONKO_OK);
/// assert_eq!(d.hour, 7.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoDayNight {
    /// `sizeof(PonkoDayNight)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_DAY_NIGHT_VERSION` as the caller compiled it.
    pub version: u32,
    /// The time of day, 0 to 24.
    pub hour: f32,
    /// Whole days gone by: the moon's phase moves on each day.
    pub day: u32,
    /// Game hours per real second. Default 0.1 (a day in four minutes); 0
    /// holds the time.
    pub hours_per_second: f32,
    /// Days from one new moon to the next. Default 29.5.
    pub days_per_moon: f32,
    /// The moon's phase on day 0 at midnight. Default 0.5 (full).
    pub first_phase: f32,
    /// The sun's color (before the atmosphere reddens it). Default as the
    /// lighting's.
    pub sun_color: PonkoColor,
    /// The sun's brightness. Default as the lighting's.
    pub sun_intensity: f32,
    /// The night's light under a full moon; a new moon leaves a fifth.
    pub night_light: PonkoColor,
}

impl Versioned for PonkoDayNight {
    const NAME: &'static str = "PonkoDayNight";
    const VERSION: u32 = PONKO_DAY_NIGHT_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, night_light: PonkoColor)),
            _ => None,
        }
    }
}

fn day_to_c(d: &DayNight) -> PonkoDayNight {
    PonkoDayNight {
        struct_size: size_of::<PonkoDayNight>() as u32,
        version: PONKO_DAY_NIGHT_VERSION,
        hour: d.hour,
        day: d.day,
        hours_per_second: d.hours_per_second,
        days_per_moon: d.days_per_moon,
        first_phase: d.first_phase,
        sun_color: to_c(d.sun_color),
        sun_intensity: d.sun_intensity,
        night_light: to_c(d.night_light),
    }
}

fn day_from_c(c: &PonkoDayNight) -> DayNight {
    let mut d = DayNight::new(0.0);
    d.hour = c.hour;
    d.day = c.day;
    d.hours_per_second = c.hours_per_second;
    d.days_per_moon = c.days_per_moon;
    d.first_phase = c.first_phase;
    d.sun_color = to_c(c.sun_color);
    d.sun_intensity = c.sun_intensity;
    d.night_light = to_c(c.night_light);
    d
}

/// Reads a caller's cycle, its version's fields over the defaults.
unsafe fn read_day(day: *const PonkoDayNight) -> Result<DayNight, crate::Failure> {
    // SAFETY: forwarded caller contract.
    let c = unsafe { read_desc(day, day_to_c(&DayNight::new(0.0)))? };
    Ok(day_from_c(&c))
}

/// Starts a cycle at `hour` o'clock on day 0 with the default speed, moon
/// and light (Rust `DayNight::new`): fills the fields of `out_day`'s
/// version. Set `struct_size` and `version` first.
///
/// Ownership: `out_day` is the caller's, written during the call only; the
/// caller keeps the cycle.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_day_night_new(7.0, std::ptr::null_mut()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `out_day` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_day_night_new(hour: f32, out_day: *mut PonkoDayNight) -> PonkoStatus {
    boundary(|| {
        if !hour.is_finite() {
            return Err(invalid("hour must be finite"));
        }
        // SAFETY: forwarded caller contract.
        unsafe { write_desc(out_day, &day_to_c(&DayNight::new(hour))) }
    })
}

/// Moves the clock on by `seconds` of real time (`hours_per_second` each),
/// counting the days (Rust `DayNight::advance`).
///
/// Ownership: `day` is the caller's, read and written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_day_night_advance(std::ptr::null_mut(), 1.0) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `day` null or a readable and writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_day_night_advance(day: *mut PonkoDayNight, seconds: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let mut d = read_day(day)?;
        d.advance(seconds);
        write_desc(day, &day_to_c(&d))
    })
}

/// The moon's phase at the cycle's time (Rust `DayNight::moon_phase`).
///
/// Ownership: `day` is read and `out_phase` written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_day_night_moon_phase(std::ptr::null(), std::ptr::null_mut()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `day` null or readable; `out_phase` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_day_night_moon_phase(day: *const PonkoDayNight, out_phase: *mut f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let d = read_day(day)?;
        out(out_phase, d.moon_phase())
    })
}

/// Sets the game's sun (direction, color, brightness), the moon's phase
/// (turning the sky kit on, with its defaults, if it was off) and the night
/// light to the cycle's time (Rust `DayNight::apply`).
///
/// Ownership: `day` is read during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_apply_day_night(std::ptr::null_mut(), std::ptr::null()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `ctx` null or live; `day` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_apply_day_night(ctx: *mut PonkoCtx, day: *const PonkoDayNight) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            read_day(day)?.apply(&mut c.scene3d);
            Ok(())
        })
    })
}

/// The whole cycle for one frame (Rust `DayNight::update`): advances `day`
/// by this frame's time, then applies it as `ponko_ctx_apply_day_night`.
/// Call it once a frame from a system.
///
/// Ownership: `day` is the caller's, read and written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_update_day_night(std::ptr::null_mut(), std::ptr::null_mut()) }, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `ctx` null or live; `day` null or a readable and writable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_update_day_night(ctx: *mut PonkoCtx, day: *mut PonkoDayNight) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let mut d = read_day(day)?;
            d.update(c);
            write_desc(day, &day_to_c(&d))
        })
    })
}

const _: () = assert!(offset_of!(PonkoSkyKit, struct_size) == 0 && offset_of!(PonkoSkyKit, version) == 4);
const _: () = assert!(offset_of!(PonkoDayNight, struct_size) == 0 && offset_of!(PonkoDayNight, version) == 4);
