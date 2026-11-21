//! Lakes, rivers and seas from C (Rust `ponko::water`): a versioned
//! `PonkoWaterDesc` filled from a preset, spawned as a water body; its CPU
//! wave query (the same waves the shader draws); contact ripples; and
//! buoyancy for 3D physics bodies (the `physics3d` feature; without it the
//! buoyancy calls fail with an error naming it).
//!
//! ```c
//! ponko_game_water(game);
//! // in setup:
//! PonkoWaterDesc sea = {.struct_size = sizeof sea, .version = PONKO_WATER_DESC_VERSION};
//! ponko_water_desc_sea(&sea, 0.0f, (PonkoVec2){-0.8f, 0.5f}, 1.2f);
//! uint64_t water;
//! ponko_ctx_spawn_water(ctx, &sea, &water);
//! ponko_ctx_set_buoyancy_box(ctx, crate_bits, (PonkoVec3){1, 0.6f, 1}, PONKO_BUOYANCY_DRAG);
//! ```

use std::mem::{offset_of, size_of};

use ponko::prelude::{Color, Ctx, Entity, Vec2};
use ponko::water::{
    FlowMap, MAX_WAVES, Water, WaterDesc, WaterPlugin, WaterQuality, WaterReflection, Wave, WaveSpectrum, Waves,
    despawn_water,
};

use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::game::{build, ok};
use crate::three::PonkoVec3;
use crate::types::{PonkoColor, PonkoVec2, to_c};
use crate::{Failure, PonkoCtx, PonkoGame, PonkoStatus, boundary, engine_error, error, invalid};

/// The version of `PonkoWaterDesc` this header describes.
pub const PONKO_WATER_DESC_VERSION: u32 = 2;
/// How many waves a `PonkoWaterDesc` holds.
pub const PONKO_WATER_MAX_WAVES: usize = 8;
const _: () = assert!(PONKO_WATER_MAX_WAVES == MAX_WAVES);
/// The default buoyancy drag (`ponko_ctx_set_buoyancy_box` and `_sphere`).
pub const PONKO_BUOYANCY_DRAG: f32 = 4.0;
/// The version of `PonkoBuoyancyDesc` this header describes.
pub const PONKO_BUOYANCY_DESC_VERSION: u32 = 1;
/// How many sample points a `PonkoBuoyancyDesc` holds: the most a floating
/// body is pushed at.
pub const PONKO_BUOYANCY_MAX_POINTS: usize = 32;
/// The density of fresh water in kg/m³ (`PonkoBuoyancyDesc::water_density`
/// from the presets); sea water is about 1025.
pub const PONKO_FRESH_WATER_DENSITY: f32 = 1000.0;

/// How much work the water's shader and passes do
/// (`PonkoWaterDesc::quality`).
/// ```
/// assert_eq!(ponko_ffi::PonkoWaterQuality::PONKO_WATER_QUALITY_HIGH as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoWaterQuality {
    /// One octave of detail, no refraction, sky reflection only, a coarse mesh.
    PONKO_WATER_QUALITY_LOW,
    /// Refraction, screen-space reflections with 12 steps, two octaves.
    PONKO_WATER_QUALITY_MEDIUM,
    /// Everything at full detail: 24 reflection steps, three octaves.
    PONKO_WATER_QUALITY_HIGH,
}

/// What the water reflects (`PonkoWaterDesc::reflection`).
/// ```
/// assert_eq!(ponko_ffi::PonkoWaterReflection::PONKO_WATER_REFLECTION_PLANAR as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoWaterReflection {
    /// The sky and the sun's glint only. Free.
    PONKO_WATER_REFLECTION_SKY,
    /// What is on screen, marched over the depth buffer; the sky elsewhere.
    PONKO_WATER_REFLECTION_SCREEN_SPACE,
    /// A mirrored camera draws the scene again into a texture each frame.
    PONKO_WATER_REFLECTION_PLANAR,
}

/// One Gerstner wave (Rust `ponko::water::Wave`).
/// ```
/// let w = ponko_ffi::PonkoWave::default();
/// assert_eq!(w.speed, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoWave {
    /// Where the crests roll to, on the ground (x, z).
    pub direction: PonkoVec2,
    /// From one crest to the next, in metres.
    pub wavelength: f32,
    /// Crest height above the rest level, in metres.
    pub amplitude: f32,
    /// 0 (a sine) to 1 (pointed crests).
    pub steepness: f32,
    /// Times the deep-water speed (1 normally).
    pub speed: f32,
    /// Where the wave starts, in radians.
    pub phase: f32,
}

/// A sea's waves from a wind-driven spectrum (Rust `ponko::water::WaveSpectrum`):
/// thousands of waves worked out each frame with an FFT, instead of a
/// handful of Gerstner waves. Start from `ponko_wave_spectrum`, change what
/// you like, and put it in `PonkoWaterDesc::spectrum` with `has_spectrum`.
/// ```
/// let s = ponko_ffi::ponko_wave_spectrum(ponko_ffi::PonkoVec2 { x: 12.0, y: 4.0 }, 2.0);
/// assert_eq!((s.tile, s.resolution, s.seed), (256.0, 128, 1));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoWaveSpectrum {
    /// The wind along x and z, in m/s: the waves run with it, and its
    /// speed sets how long the biggest are (about `0.9 * speed^2` metres).
    pub wind: PonkoVec2,
    /// The significant wave height in metres: trough to crest of the
    /// highest third of the waves.
    pub height: f32,
    /// The side of the square of sea that repeats, in metres.
    pub tile: f32,
    /// Cells across the square: 64, 128 or 256 (rounded to the nearest).
    pub resolution: u32,
    /// How sharp the crests are: 0 rounded, 1 pointed.
    pub choppiness: f32,
    /// Picks the random sea: equal seeds draw equal seas.
    pub seed: u32,
}

/// The spectrum of a sea under `wind` (m/s along x and z) with `height`
/// metres of significant wave height, on a 256 m square of 128 cells,
/// choppiness 1, seed 1 (Rust `WaveSpectrum::new`).
/// ```
/// let s = ponko_ffi::ponko_wave_spectrum(ponko_ffi::PonkoVec2 { x: 10.0, y: 0.0 }, 1.5);
/// assert_eq!((s.height, s.choppiness), (1.5, 1.0));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_wave_spectrum(wind: PonkoVec2, height: f32) -> PonkoWaveSpectrum {
    spectrum_to_c(WaveSpectrum::new(to_c(wind), height))
}

fn spectrum_to_c(s: WaveSpectrum) -> PonkoWaveSpectrum {
    PonkoWaveSpectrum {
        wind: to_c(s.wind),
        height: s.height,
        tile: s.tile,
        resolution: s.resolution,
        choppiness: s.choppiness,
        seed: s.seed,
    }
}

fn spectrum_from_c(s: &PonkoWaveSpectrum) -> Result<WaveSpectrum, Failure> {
    finite(
        &[s.wind.x, s.wind.y, s.height, s.tile, s.choppiness],
        "the wave spectrum's settings must be finite",
    )?;
    if s.height < 0.0 || s.choppiness < 0.0 {
        return Err(invalid(
            "the wave spectrum's height and choppiness must not be negative",
        ));
    }
    if s.tile <= 0.0 {
        return Err(invalid("the wave spectrum's tile must be positive"));
    }
    Ok(WaveSpectrum::new(to_c(s.wind), s.height)
        .with_tile(s.tile, s.resolution)
        .with_choppiness(s.choppiness)
        .with_seed(s.seed))
}

/// Everything about one body of water (Rust `WaterDesc`). A versioned
/// descriptor: set `struct_size` and `version`, fill it with
/// `ponko_water_desc_lake`, `_river` or `_sea`, change what you like, and
/// spawn it with `ponko_ctx_spawn_water`.
///
/// Version 1: every field up to `underwater`. Version 2 adds
/// `has_spectrum` and `spectrum` (a program built against version 1 gets
/// Gerstner waves, as before).
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoWaterDesc = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoWaterDesc>() as u32, PONKO_WATER_DESC_VERSION);
/// let at = PonkoVec2 { x: 0.0, y: 0.0 };
/// assert_eq!(unsafe { ponko_water_desc_lake(&mut d, at, PonkoVec2 { x: 40.0, y: 30.0 }, 1.5) }, PonkoStatus::PONKO_OK);
/// assert_eq!((d.level, d.wave_count), (1.5, 3));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoWaterDesc {
    /// `sizeof(PonkoWaterDesc)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_WATER_DESC_VERSION` as the caller compiled it.
    pub version: u32,
    /// The rest height of the surface, in metres.
    pub level: f32,
    /// The middle of the water's area on the ground (x, z).
    pub center: PonkoVec2,
    /// How far the area reaches along x and z, in metres.
    pub size: PonkoVec2,
    /// The mesh follows the camera and covers everything (a sea); `center`
    /// is then ignored and `size` is how far it reaches.
    pub follow_camera: bool,
    /// The waves; the first `wave_count` are used.
    pub waves: [PonkoWave; PONKO_WATER_MAX_WAVES],
    /// How many of `waves` are used, 0 to `PONKO_WATER_MAX_WAVES`.
    pub wave_count: u32,
    /// A current in m/s along x and z, where there is no flow map.
    pub flow: PonkoVec2,
    /// RGBA8 texels of a flow map over the area (red: speed along x, green
    /// along z, 128 still, 0 / 255 `-max` / `+max`), or null for none. Copied.
    pub flow_map: *const u8,
    /// The flow map's width in texels.
    pub flow_map_width: u32,
    /// The flow map's height in texels.
    pub flow_map_height: u32,
    /// The speed 0 and 255 stand for, in m/s.
    pub flow_map_max_speed: f32,
    /// How much of each colour channel the water swallows per metre.
    pub absorption: PonkoVec3,
    /// The water's own colour where the bottom is out of sight.
    pub scatter: PonkoColor,
    /// How much foam: 0 none, 1 the default.
    pub foam: f32,
    /// How far from the shore foam reaches, as depth in metres.
    pub shore_foam: f32,
    /// How strongly the surface bends what is under it.
    pub refraction: f32,
    /// A `PonkoWaterReflection`.
    pub reflection: u32,
    /// A `PonkoWaterQuality`.
    pub quality: u32,
    /// Contact ripples (`ponko_ctx_water_touch`). Needs compute shaders:
    /// not on WebGL2, where touching does nothing.
    pub ripples: bool,
    /// The side of the ripple square around `center`, in metres.
    pub ripple_area: f32,
    /// The underwater screen effect when the camera dips below.
    pub underwater: bool,
    /// Version 2: the waves come from `spectrum` instead of `waves` (Rust
    /// `Waves::spectrum`); `waves` and `wave_count` are then ignored.
    pub has_spectrum: bool,
    /// Version 2: the sea's spectrum, used when `has_spectrum` is set.
    pub spectrum: PonkoWaveSpectrum,
}

const _: () = assert!(offset_of!(PonkoWaterDesc, struct_size) == 0 && offset_of!(PonkoWaterDesc, version) == 4);
impl Versioned for PonkoWaterDesc {
    const NAME: &'static str = "PonkoWaterDesc";
    const VERSION: u32 = PONKO_WATER_DESC_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, underwater) + size_of::<bool>()),
            2 => Some(offset_of!(Self, spectrum) + size_of::<PonkoWaveSpectrum>()),
            _ => None,
        }
    }
}

/// What the water does at one point (`ponko_ctx_water_sample`).
/// ```
/// assert_eq!(ponko_ffi::PonkoWaterSample::default().height, 0.0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoWaterSample {
    /// The surface's height, the level included, in metres.
    pub height: f32,
    /// The surface's unit normal.
    pub normal: PonkoVec3,
    /// How fast the water at the surface moves (waves and current), m/s.
    pub velocity: PonkoVec3,
    /// 1 on flat water, below 1 near sharp crests, 0 where it would fold.
    pub squeeze: f32,
}

/// How a 3D body floats (Rust `water::Buoyancy`): where it samples the
/// water, the water it displaces, the water's density, the drag and whether
/// it ripples the surface. A versioned descriptor: set `struct_size` and
/// `version`, fill it with `ponko_buoyancy_desc_box`, `_sphere` or
/// `_points`, change what you like, and give it to a body with
/// `ponko_ctx_set_buoyancy`.
///
/// The body's `volume` is split evenly over its points. Each stands for a
/// column `height` metres tall around it: wholly under water when the
/// surface is `height / 2` above it, out of it when that far below.
///
/// Version 1: every field below.
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoBuoyancyDesc = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoBuoyancyDesc>() as u32, PONKO_BUOYANCY_DESC_VERSION);
/// d.water_density = 1025.0; // sea water
/// assert_eq!(d.points.len(), PONKO_BUOYANCY_MAX_POINTS);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoBuoyancyDesc {
    /// `sizeof(PonkoBuoyancyDesc)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_BUOYANCY_DESC_VERSION` as the caller compiled it.
    pub version: u32,
    /// Where the body samples the water, in its own space; the first
    /// `point_count` are used.
    pub points: [PonkoVec3; PONKO_BUOYANCY_MAX_POINTS],
    /// How many of `points` are used, 1 to `PONKO_BUOYANCY_MAX_POINTS`.
    pub point_count: u32,
    /// The water the body displaces when wholly under, in m³.
    pub volume: f32,
    /// The height of the column each point stands for, in metres.
    pub height: f32,
    /// The water's density in kg/m³ (`PONKO_FRESH_WATER_DENSITY`; sea
    /// water about 1025): denser water floats the body higher.
    pub water_density: f32,
    /// How strongly the water drags the body along with it, per second
    /// (`PONKO_BUOYANCY_DRAG`).
    pub drag: f32,
    /// The body starts contact ripples on the water as it crosses the
    /// surface.
    pub ripples: bool,
}

const _: () = assert!(offset_of!(PonkoBuoyancyDesc, struct_size) == 0 && offset_of!(PonkoBuoyancyDesc, version) == 4);
impl Versioned for PonkoBuoyancyDesc {
    const NAME: &'static str = "PonkoBuoyancyDesc";
    const VERSION: u32 = PONKO_BUOYANCY_DESC_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, ripples) + size_of::<bool>()),
            _ => None,
        }
    }
}

fn desc_to_c(d: &WaterDesc) -> PonkoWaterDesc {
    let mut waves = [PonkoWave::default(); PONKO_WATER_MAX_WAVES];
    for (c, w) in waves.iter_mut().zip(d.waves.waves()) {
        *c = PonkoWave {
            direction: to_c(w.direction),
            wavelength: w.wavelength,
            amplitude: w.amplitude,
            steepness: w.steepness,
            speed: w.speed,
            phase: w.phase,
        };
    }
    PonkoWaterDesc {
        struct_size: size_of::<PonkoWaterDesc>() as u32,
        version: PONKO_WATER_DESC_VERSION,
        level: d.level,
        center: to_c(d.center),
        size: to_c(d.size),
        follow_camera: d.follow_camera,
        waves,
        wave_count: d.waves.waves().len() as u32,
        flow: to_c(d.flow),
        // The presets have no map; a desc read back never keeps one.
        flow_map: std::ptr::null(),
        flow_map_width: 0,
        flow_map_height: 0,
        flow_map_max_speed: 0.0,
        absorption: to_c(d.absorption),
        scatter: to_c(d.scatter),
        foam: d.foam,
        shore_foam: d.shore_foam,
        refraction: d.refraction,
        reflection: match d.reflection {
            WaterReflection::Sky => PonkoWaterReflection::PONKO_WATER_REFLECTION_SKY,
            WaterReflection::ScreenSpace => PonkoWaterReflection::PONKO_WATER_REFLECTION_SCREEN_SPACE,
            WaterReflection::Planar => PonkoWaterReflection::PONKO_WATER_REFLECTION_PLANAR,
        } as u32,
        quality: match d.quality {
            WaterQuality::Low => PonkoWaterQuality::PONKO_WATER_QUALITY_LOW,
            WaterQuality::Medium => PonkoWaterQuality::PONKO_WATER_QUALITY_MEDIUM,
            WaterQuality::High => PonkoWaterQuality::PONKO_WATER_QUALITY_HIGH,
        } as u32,
        ripples: d.ripples,
        ripple_area: d.ripple_area,
        underwater: d.underwater,
        has_spectrum: d.waves.is_spectrum(),
        spectrum: d.waves.spectrum_settings().map(spectrum_to_c).unwrap_or_default(),
    }
}

fn finite(values: &[f32], message: &'static str) -> Result<(), Failure> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

/// The Rust desc for a C one, checked.
///
/// # Safety
/// `c.flow_map` null or readable for `width * height * 4` bytes.
unsafe fn desc_from_c(c: &PonkoWaterDesc) -> Result<WaterDesc, Failure> {
    finite(
        &[
            c.level,
            c.center.x,
            c.center.y,
            c.size.x,
            c.size.y,
            c.flow.x,
            c.flow.y,
            c.absorption.x,
            c.absorption.y,
            c.absorption.z,
            c.foam,
            c.shore_foam,
            c.refraction,
            c.ripple_area,
        ],
        "water settings must be finite",
    )?;
    if c.size.x <= 0.0 || c.size.y <= 0.0 {
        return Err(invalid("water size must be positive"));
    }
    let count = c.wave_count as usize;
    if count > PONKO_WATER_MAX_WAVES {
        return Err(invalid("wave_count is more than PONKO_WATER_MAX_WAVES"));
    }
    let mut waves = Vec::with_capacity(count);
    for w in &c.waves[..count] {
        finite(
            &[
                w.direction.x,
                w.direction.y,
                w.wavelength,
                w.amplitude,
                w.steepness,
                w.speed,
                w.phase,
            ],
            "waves must be finite",
        )?;
        if w.wavelength <= 0.0 {
            return Err(invalid("a wave's wavelength must be positive"));
        }
        waves.push(Wave {
            direction: to_c(w.direction),
            wavelength: w.wavelength,
            amplitude: w.amplitude,
            steepness: w.steepness,
            speed: w.speed,
            phase: w.phase,
        });
    }
    let reflection = match c.reflection {
        0 => WaterReflection::Sky,
        1 => WaterReflection::ScreenSpace,
        2 => WaterReflection::Planar,
        _ => return Err(invalid("reflection is not a PonkoWaterReflection")),
    };
    let quality = match c.quality {
        0 => WaterQuality::Low,
        1 => WaterQuality::Medium,
        2 => WaterQuality::High,
        _ => return Err(invalid("quality is not a PonkoWaterQuality")),
    };
    let flow_map = if c.flow_map.is_null() {
        None
    } else {
        let (w, h, max) = (c.flow_map_width, c.flow_map_height, c.flow_map_max_speed);
        if w == 0 || h == 0 || w > 4096 || h > 4096 {
            return Err(invalid("a flow map is 1 to 4096 texels on a side"));
        }
        if !(max.is_finite() && max > 0.0) {
            return Err(invalid("flow_map_max_speed must be positive"));
        }
        // SAFETY: caller contract; at most 4096 * 4096 * 4 bytes.
        let rgba = unsafe { std::slice::from_raw_parts(c.flow_map, w as usize * h as usize * 4) };
        Some(FlowMap::from_rgba(w, h, max, rgba))
    };
    let scatter: Color = to_c(c.scatter);
    Ok(WaterDesc {
        level: c.level,
        center: to_c(c.center),
        size: to_c(c.size),
        follow_camera: c.follow_camera,
        waves: if c.has_spectrum {
            Waves::spectrum(spectrum_from_c(&c.spectrum)?)
        } else {
            Waves::new(&waves)
        },
        flow: to_c(c.flow),
        flow_map,
        absorption: to_c(c.absorption),
        scatter,
        foam: c.foam.max(0.0),
        shore_foam: c.shore_foam.max(0.0),
        refraction: c.refraction.max(0.0),
        reflection,
        quality,
        ripples: c.ripples,
        ripple_area: c.ripple_area.max(1.0),
        underwater: c.underwater,
    })
}

fn water_of(c: &Ctx, bits: u64) -> Result<&Water, Failure> {
    c.world
        .get::<Water>(Entity::from_bits(bits))
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity is not a water body"))
}

/// Fills `desc` (header set) with a still lake over `size` metres around
/// `center` (x, z) at height `level`: light ripples, clear greenish water,
/// screen-space reflections, shore foam and contact ripples.
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_water_desc_lake(
    desc: *mut PonkoWaterDesc,
    center: PonkoVec2,
    size: PonkoVec2,
    level: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(desc, &desc_to_c(&WaterDesc::lake(to_c(center), to_c(size), level))) })
}

/// Fills `desc` with a river over `size` metres around `center` at
/// `level`, flowing at `flow` m/s (x, z); give it a flow map for a current
/// that bends.
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoWaterDesc = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoWaterDesc>() as u32, PONKO_WATER_DESC_VERSION);
/// let v = |x, y| PonkoVec2 { x, y };
/// assert_eq!(unsafe { ponko_water_desc_river(&mut d, v(0.0, 0.0), v(8.0, 60.0), 0.0, v(0.0, 1.5)) }, PonkoStatus::PONKO_OK);
/// assert_eq!(d.flow.y, 1.5);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_water_desc_river(
    desc: *mut PonkoWaterDesc,
    center: PonkoVec2,
    size: PonkoVec2,
    level: f32,
    flow: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let river = WaterDesc::river(to_c(center), to_c(size), level, to_c(flow));
        write_desc(desc, &desc_to_c(&river))
    })
}

/// Fills `desc` with an open sea to the horizon at `level`: six Gerstner
/// waves around `wind` (x, z), about `height` metres from trough to crest.
/// ```
/// use ponko_ffi::*;
/// let mut d: PonkoWaterDesc = unsafe { std::mem::zeroed() };
/// (d.struct_size, d.version) = (size_of::<PonkoWaterDesc>() as u32, PONKO_WATER_DESC_VERSION);
/// assert_eq!(unsafe { ponko_water_desc_sea(&mut d, 0.0, PonkoVec2 { x: 1.0, y: 0.0 }, 1.2) }, PonkoStatus::PONKO_OK);
/// assert!(d.follow_camera);
/// assert_eq!(d.wave_count, 6);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_water_desc_sea(
    desc: *mut PonkoWaterDesc,
    level: f32,
    wind: PonkoVec2,
    height: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(desc, &desc_to_c(&WaterDesc::sea(level, to_c(wind), height))) })
}

/// Adds the water kit's systems (Rust `WaterPlugin`): keeps every water
/// body up to date each frame and, with `physics3d`, floats bodies that
/// have buoyancy. Call it before running the game.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_water(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `game` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_water(game: *mut PonkoGame) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { build(game, |g| ok(g.plugin(WaterPlugin))) }
}

/// Spawns a body of water from `desc` and writes its entity bits to
/// `out_entity`. Fails with the reason for bad settings, or when the GPU
/// can't make its textures. The flow map is copied.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_spawn_water(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `desc` null or readable, its flow map null or
/// readable for `width * height * 4` bytes; `out_entity` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_spawn_water(
    ctx: *mut PonkoCtx,
    desc: *const PonkoWaterDesc,
    out_entity: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if desc.is_null() {
            return Err(invalid("desc is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let c = read_desc(desc, desc_to_c(&WaterDesc::lake(Vec2::ZERO, Vec2::ONE, 0.0)))?;
            let d = desc_from_c(&c)?;
            let e = ctx_access(ctx, |c| Water::try_spawn(c, d).map_err(engine_error))?;
            out(out_entity, e.to_bits())
        }
    })
}

/// Despawns water body `entity` and frees its textures and passes.
/// `PONKO_NOT_FOUND` if it isn't one.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_despawn_water(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_despawn_water(ctx: *mut PonkoCtx, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if despawn_water(c, Entity::from_bits(entity)) {
                Ok(())
            } else {
                Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity is not a water body"))
            }
        })
    })
}

/// The surface of water body `entity` at ground point `at` (x, z) and time
/// `t` in seconds (`ponko_ctx_time` is what the shader draws; buoyancy uses
/// `ponko_ctx_sim_time`): height with the level, normal, velocity and
/// squeeze. The same waves the GPU draws, to within a centimetre.
/// ```
/// let mut s = ponko_ffi::PonkoWaterSample::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_water_sample(std::ptr::null(), 0, Default::default(), 0.0, &mut s) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_sample` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_water_sample(
    ctx: *const PonkoCtx,
    entity: u64,
    at: PonkoVec2,
    t: f32,
    out_sample: *mut PonkoWaterSample,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        finite(&[at.x, at.y, t], "at and t must be finite")?;
        let s = ctx_access(ctx, |c| Ok(water_of(c, entity)?.sample(to_c(at), t)))?;
        out(
            out_sample,
            PonkoWaterSample {
                height: s.height,
                normal: to_c(s.normal),
                velocity: to_c(s.velocity),
                squeeze: s.squeeze,
            },
        )
    })
}

/// Starts a ring of contact ripples on water body `entity` at `at` (x, z):
/// `radius` metres across, `strength` about metres high (negative pulls the
/// surface down). Does nothing outside its ripple area, without ripples,
/// or on WebGL2.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_water_touch(std::ptr::null_mut(), 0, Default::default(), 0.5, 0.1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_water_touch(
    ctx: *mut PonkoCtx,
    entity: u64,
    at: PonkoVec2,
    radius: f32,
    strength: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = Entity::from_bits(entity);
            let water = c
                .world
                .get_mut::<Water>(e)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity is not a water body"))?;
            water.touch(to_c(at), radius, strength);
            Ok(())
        })
    })
}

/// Writes how many contact ripples wait for the water's next update on water
/// body `entity` (Rust `Water::waiting_contacts`): the rings
/// `ponko_ctx_water_touch` and floating bodies crossing the surface started
/// that the ripple simulation hasn't taken yet. Each update takes three, and
/// the oldest are dropped past a limit; a body outside the ripple area, or
/// water without ripples, starts none, so a test sees a floating body's
/// ripple switch (`PonkoBuoyancyDesc::ripples`) by its effect: read it in a
/// system that runs after physics. `PONKO_NOT_FOUND` for an entity that
/// isn't a water body.
///
/// Ownership: `out_count` is written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_water_waiting_contacts(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_water_waiting_contacts(
    ctx: *mut PonkoCtx,
    entity: u64,
    out_count: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let n = ctx_access(ctx, |c| Ok(water_of(c, entity)?.waiting_contacts()))?;
        out(out_count, u32::try_from(n).unwrap_or(u32::MAX))
    })
}

/// Makes 3D body `entity` float: a box of `size` (full width, height and
/// depth) sampling the water at its eight corners' layers. `drag` is how
/// strongly the water pulls it along (`PONKO_BUOYANCY_DRAG` normally).
/// Floats with `ponko_game_water`. Replaces any buoyancy it had.
/// ```
/// let v = ponko_ffi::PonkoVec3 { x: 1.0, y: 1.0, z: 1.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_buoyancy_box(std::ptr::null_mut(), 0, v, 4.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_buoyancy_box(
    ctx: *mut PonkoCtx,
    entity: u64,
    size: PonkoVec3,
    drag: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_box(ctx, entity, size, drag) }
}

/// Makes 3D body `entity` float as a ball of `radius`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_buoyancy_sphere(std::ptr::null_mut(), 0, 0.5, 4.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_buoyancy_sphere(
    ctx: *mut PonkoCtx,
    entity: u64,
    radius: f32,
    drag: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_sphere(ctx, entity, radius, drag) }
}

/// Stops `entity` floating. `PONKO_NOT_FOUND` if it had no buoyancy.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_buoyancy(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_buoyancy(ctx: *mut PonkoCtx, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::remove(ctx, entity) }
}

/// How much of floating body `entity` was under water at the last physics
/// step, 0 to 1.
/// ```
/// let mut s = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_buoyancy_submerged(std::ptr::null(), 0, &mut s) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_share` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_buoyancy_submerged(
    ctx: *const PonkoCtx,
    entity: u64,
    out_share: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::submerged(ctx, entity, out_share) }
}

/// Fills `desc` (header set) with a box of `size` (full width, height and
/// depth) centred on the body: eight points in two layers, fresh water,
/// drag `PONKO_BUOYANCY_DRAG`, ripples on (Rust `Buoyancy::cuboid`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_buoyancy_desc_box(std::ptr::null_mut(), Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_buoyancy_desc_box(desc: *mut PonkoBuoyancyDesc, size: PonkoVec3) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::desc_box(desc, size) }
}

/// Fills `desc` with a ball of `radius`: seven points, fresh water, drag
/// `PONKO_BUOYANCY_DRAG`, ripples on (Rust `Buoyancy::sphere`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_buoyancy_desc_sphere(std::ptr::null_mut(), 0.5) } as u32, 1);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_buoyancy_desc_sphere(desc: *mut PonkoBuoyancyDesc, radius: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::desc_sphere(desc, radius) }
}

/// Fills `desc` with `count` sample `points` (body space) sharing `volume`
/// m³, each standing for a column `height` tall; fresh water, drag
/// `PONKO_BUOYANCY_DRAG`, ripples on (Rust `Buoyancy::points`). `count` is
/// 1 to `PONKO_BUOYANCY_MAX_POINTS`; the points are copied.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_buoyancy_desc_points(std::ptr::null_mut(), std::ptr::null(), 0, 1.0, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `desc` null or a writable descriptor with its header set; `points` null
/// or readable for `count` values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_buoyancy_desc_points(
    desc: *mut PonkoBuoyancyDesc,
    points: *const PonkoVec3,
    count: u32,
    volume: f32,
    height: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::desc_points(desc, points, count, volume, height) }
}

/// Makes 3D body `entity` float as `desc` says: its points, volume, column
/// height, the water's density, drag and the ripple switch. Floats with
/// `ponko_game_water`. Replaces any buoyancy it had. Fails, saying why, for
/// a `point_count` of 0 or over `PONKO_BUOYANCY_MAX_POINTS`, points or
/// numbers that aren't finite, a volume, height or density that isn't
/// positive, or a negative drag.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_buoyancy(std::ptr::null_mut(), 0, std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `desc` null or a readable descriptor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_buoyancy(
    ctx: *mut PonkoCtx,
    entity: u64,
    desc: *const PonkoBuoyancyDesc,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_desc(ctx, entity, desc) }
}

/// Writes how floating body `entity` floats into the fields of
/// `out_desc`'s version: change some and give it back with
/// `ponko_ctx_set_buoyancy`. `PONKO_NOT_FOUND` if it has no buoyancy;
/// `PONKO_INVALID_ARGUMENT` if Rust gave it more than
/// `PONKO_BUOYANCY_MAX_POINTS` points.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_buoyancy(std::ptr::null(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_desc` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_buoyancy(
    ctx: *const PonkoCtx,
    entity: u64,
    out_desc: *mut PonkoBuoyancyDesc,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::get_desc(ctx, entity, out_desc) }
}

#[cfg(feature = "physics3d")]
mod imp {
    use ponko::prelude::Vec3;
    use ponko::water::Buoyancy;

    use super::*;

    unsafe fn set(ctx: *mut PonkoCtx, entity: u64, drag: f32, make: impl FnOnce() -> Buoyancy) -> PonkoStatus {
        boundary(|| {
            if !(drag.is_finite() && drag >= 0.0) {
                return Err(invalid("drag must be finite and not negative"));
            }
            // SAFETY: forwarded caller contract.
            unsafe {
                ctx_access(ctx, |c| {
                    let e = Entity::from_bits(entity);
                    if !c.world.is_alive(e) {
                        return Err(invalid("no such entity"));
                    }
                    c.world.insert(e, make().with_drag(drag));
                    Ok(())
                })
            }
        })
    }
    fn buoyancy_to_c(b: &Buoyancy) -> Result<PonkoBuoyancyDesc, Failure> {
        if b.points.len() > PONKO_BUOYANCY_MAX_POINTS {
            return Err(invalid("the body has more than PONKO_BUOYANCY_MAX_POINTS points"));
        }
        let mut points = [PonkoVec3::default(); PONKO_BUOYANCY_MAX_POINTS];
        for (c, p) in points.iter_mut().zip(&b.points) {
            *c = to_c(*p);
        }
        Ok(PonkoBuoyancyDesc {
            struct_size: size_of::<PonkoBuoyancyDesc>() as u32,
            version: PONKO_BUOYANCY_DESC_VERSION,
            points,
            point_count: b.points.len() as u32,
            volume: b.volume,
            height: b.height,
            water_density: b.water_density,
            drag: b.drag,
            ripples: b.ripples,
        })
    }

    fn buoyancy_from_c(d: &PonkoBuoyancyDesc) -> Result<Buoyancy, Failure> {
        let count = d.point_count as usize;
        if count == 0 || count > PONKO_BUOYANCY_MAX_POINTS {
            return Err(invalid("point_count must be 1 to PONKO_BUOYANCY_MAX_POINTS"));
        }
        let points: Vec<Vec3> = d.points[..count].iter().map(|p| to_c(*p)).collect();
        if !points.iter().all(|p: &Vec3| p.is_finite()) {
            return Err(invalid("buoyancy points must be finite"));
        }
        finite(
            &[d.volume, d.height, d.water_density, d.drag],
            "buoyancy settings must be finite",
        )?;
        if d.volume <= 0.0 || d.height <= 0.0 || d.water_density <= 0.0 {
            return Err(invalid("volume, height and water_density must be positive"));
        }
        if d.drag < 0.0 {
            return Err(invalid("drag must not be negative"));
        }
        let mut b = Buoyancy::points(points, d.volume, d.height).with_drag(d.drag);
        b.water_density = d.water_density;
        b.ripples = d.ripples;
        Ok(b)
    }

    pub(super) unsafe fn desc_box(desc: *mut PonkoBuoyancyDesc, size: PonkoVec3) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        boundary(|| unsafe {
            let size: Vec3 = to_c(size);
            if !(size.is_finite() && size.cmpgt(Vec3::ZERO).all()) {
                return Err(invalid("size must be positive"));
            }
            write_desc(desc, &buoyancy_to_c(&Buoyancy::cuboid(size))?)
        })
    }
    pub(super) unsafe fn desc_sphere(desc: *mut PonkoBuoyancyDesc, radius: f32) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        boundary(|| unsafe {
            if !(radius.is_finite() && radius > 0.0) {
                return Err(invalid("radius must be positive"));
            }
            write_desc(desc, &buoyancy_to_c(&Buoyancy::sphere(radius))?)
        })
    }
    pub(super) unsafe fn desc_points(
        desc: *mut PonkoBuoyancyDesc,
        points: *const PonkoVec3,
        count: u32,
        volume: f32,
        height: f32,
    ) -> PonkoStatus {
        boundary(|| {
            if points.is_null() || count == 0 || count as usize > PONKO_BUOYANCY_MAX_POINTS {
                return Err(invalid("points must hold 1 to PONKO_BUOYANCY_MAX_POINTS values"));
            }
            // SAFETY: caller contract: readable for `count` (at most 32) values.
            let given = unsafe { std::slice::from_raw_parts(points, count as usize) };
            let mut c = buoyancy_to_c(&Buoyancy::points(Vec::new(), volume, height))?;
            c.points[..given.len()].copy_from_slice(given);
            c.point_count = count;
            c.height = height;
            // Checked as `ponko_ctx_set_buoyancy` will check it.
            buoyancy_from_c(&c)?;
            // SAFETY: forwarded caller contract.
            unsafe { write_desc(desc, &c) }
        })
    }
    pub(super) unsafe fn set_desc(ctx: *mut PonkoCtx, entity: u64, desc: *const PonkoBuoyancyDesc) -> PonkoStatus {
        boundary(|| {
            if desc.is_null() {
                return Err(invalid("desc is null"));
            }
            // SAFETY: forwarded caller contract.
            unsafe {
                let defaults = buoyancy_to_c(&Buoyancy::sphere(1.0))?;
                let b = buoyancy_from_c(&read_desc(desc, defaults)?)?;
                ctx_access(ctx, |c| {
                    let e = Entity::from_bits(entity);
                    if !c.world.is_alive(e) {
                        return Err(invalid("no such entity"));
                    }
                    c.world.insert(e, b);
                    Ok(())
                })
            }
        })
    }
    pub(super) unsafe fn get_desc(ctx: *const PonkoCtx, entity: u64, out_desc: *mut PonkoBuoyancyDesc) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        boundary(|| unsafe {
            let d = ctx_access(ctx, |c| {
                let b = c
                    .world
                    .get::<Buoyancy>(Entity::from_bits(entity))
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity has no buoyancy"))?;
                buoyancy_to_c(b)
            })?;
            write_desc(out_desc, &d)
        })
    }
    pub(super) unsafe fn set_box(ctx: *mut PonkoCtx, entity: u64, size: PonkoVec3, drag: f32) -> PonkoStatus {
        let size: Vec3 = to_c(size);
        if !(size.is_finite() && size.cmpgt(Vec3::ZERO).all()) {
            return boundary(|| Err(invalid("size must be positive")));
        }
        // SAFETY: forwarded caller contract.
        unsafe { set(ctx, entity, drag, || Buoyancy::cuboid(size)) }
    }
    pub(super) unsafe fn set_sphere(ctx: *mut PonkoCtx, entity: u64, radius: f32, drag: f32) -> PonkoStatus {
        if !(radius.is_finite() && radius > 0.0) {
            return boundary(|| Err(invalid("radius must be positive")));
        }
        // SAFETY: forwarded caller contract.
        unsafe { set(ctx, entity, drag, || Buoyancy::sphere(radius)) }
    }
    pub(super) unsafe fn remove(ctx: *mut PonkoCtx, entity: u64) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        boundary(|| unsafe {
            ctx_access(ctx, |c| match c.world.remove::<Buoyancy>(Entity::from_bits(entity)) {
                Some(_) => Ok(()),
                None => Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity has no buoyancy")),
            })
        })
    }
    pub(super) unsafe fn submerged(ctx: *const PonkoCtx, entity: u64, share: *mut f32) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        boundary(|| unsafe {
            let s = ctx_access(ctx, |c| {
                c.world
                    .get::<Buoyancy>(Entity::from_bits(entity))
                    .map(|b| b.submerged)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity has no buoyancy"))
            })?;
            out(share, s)
        })
    }
}

#[cfg(not(feature = "physics3d"))]
mod imp {
    //! Without the feature: every buoyancy call fails, saying why.
    use super::*;

    fn off() -> Result<(), Failure> {
        Err(invalid("ponko_ffi was built without the `physics3d` feature"))
    }
    pub(super) unsafe fn set_box(_: *mut PonkoCtx, _: u64, _: PonkoVec3, _: f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_sphere(_: *mut PonkoCtx, _: u64, _: f32, _: f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn remove(_: *mut PonkoCtx, _: u64) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn submerged(_: *const PonkoCtx, _: u64, _: *mut f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn desc_box(_: *mut PonkoBuoyancyDesc, _: PonkoVec3) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn desc_sphere(_: *mut PonkoBuoyancyDesc, _: f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn desc_points(
        _: *mut PonkoBuoyancyDesc,
        _: *const PonkoVec3,
        _: u32,
        _: f32,
        _: f32,
    ) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_desc(_: *mut PonkoCtx, _: u64, _: *const PonkoBuoyancyDesc) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn get_desc(_: *const PonkoCtx, _: u64, _: *mut PonkoBuoyancyDesc) -> PonkoStatus {
        boundary(off)
    }
}
