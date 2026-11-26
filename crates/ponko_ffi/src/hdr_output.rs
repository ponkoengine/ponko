//! HDR display output (Rust `HdrOutput`, `HdrStatus`): the settings a game
//! opens its window with and changes while it runs, and what the window
//! does with them.

use std::mem::{offset_of, size_of};

use ponko::render::{HdrEncoding, HdrOffReason, HdrOutput, HdrStatus};

use crate::ctx::ctx_access;
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::game::{build, ok};
use crate::{Failure, PonkoCtx, PonkoGame, PonkoStatus, boundary, invalid};

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// The version of `PonkoHdrOutput` this header describes.
pub const PONKO_HDR_OUTPUT_VERSION: u32 = 2;
/// The version of `PonkoHdrStatus` this header describes.
pub const PONKO_HDR_STATUS_VERSION: u32 = 2;

/// HDR display output settings (Rust `HdrOutput`), in a layout that can
/// grow: set `struct_size` and `version`, fill it with
/// `ponko_hdr_output_defaults`, change it, and give it to
/// `ponko_game_hdr_output` (before the window opens) or
/// `ponko_ctx_set_hdr_output` (at any time while the game runs: it turns
/// HDR on and off without a restart).
///
/// Off by default, and then nothing changes. With `enabled` the window
/// draws in an HDR format and color space where the display system offers
/// one; the 3D frame's highlights reach `peak_nits` while UI, sprites and
/// the inspector stay at `paper_white_nits`.
///
/// Version 1: `enabled`, `paper_white_nits`, `peak_nits`.
/// Version 2: `max_content_light_nits`, `max_frame_average_nits` (HDR10
/// metadata).
/// ```
/// use ponko_ffi::*;
/// let mut h: PonkoHdrOutput = unsafe { std::mem::zeroed() };
/// (h.struct_size, h.version) = (size_of::<PonkoHdrOutput>() as u32, PONKO_HDR_OUTPUT_VERSION);
/// assert_eq!(unsafe { ponko_hdr_output_defaults(&mut h) }, PonkoStatus::PONKO_OK);
/// assert_eq!((h.enabled, h.paper_white_nits, h.peak_nits), (false, 200.0, 0.0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoHdrOutput {
    /// `sizeof(PonkoHdrOutput)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_HDR_OUTPUT_VERSION` as the caller compiled it.
    pub version: u32,
    /// Output in HDR where the display system offers it. Default false.
    pub enabled: bool,
    /// How bright SDR white (UI, sprites) is, in nits. Default 200. Where
    /// the system sets SDR white itself (macOS, browsers) it only scales
    /// `peak_nits`. Not finite or not positive: the default.
    pub paper_white_nits: f32,
    /// The brightest highlight in nits; 0 (default) asks the display, or
    /// 1000 where it says nothing. Clamped to paper white and 10 000.
    pub peak_nits: f32,
    /// HDR10 metadata: the brightest pixel (MaxCLL) in nits; 0 (default)
    /// is the peak in use. Sent where the platform takes metadata.
    pub max_content_light_nits: f32,
    /// HDR10 metadata: the brightest frame average (MaxFALL) in nits; 0
    /// (default) is paper white.
    pub max_frame_average_nits: f32,
}

const _: () = assert!(offset_of!(PonkoHdrOutput, struct_size) == 0 && offset_of!(PonkoHdrOutput, version) == 4);

impl Versioned for PonkoHdrOutput {
    const NAME: &'static str = "PonkoHdrOutput";
    const VERSION: u32 = PONKO_HDR_OUTPUT_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, peak_nits: f32)),
            2 => Some(end_of!(Self, max_frame_average_nits: f32)),
            _ => None,
        }
    }
}

fn output_to_c(h: &HdrOutput) -> PonkoHdrOutput {
    PonkoHdrOutput {
        struct_size: size_of::<PonkoHdrOutput>() as u32,
        version: PONKO_HDR_OUTPUT_VERSION,
        enabled: h.enabled,
        paper_white_nits: h.paper_white_nits,
        peak_nits: h.peak_nits,
        max_content_light_nits: h.max_content_light_nits,
        max_frame_average_nits: h.max_frame_average_nits,
    }
}

fn output_from_c(c: &PonkoHdrOutput) -> Result<HdrOutput, Failure> {
    if c.peak_nits.is_nan() || c.peak_nits < 0.0 {
        return Err(invalid("peak_nits must be 0 (the display's) or more"));
    }
    for (value, message) in [
        (
            c.max_content_light_nits,
            "max_content_light_nits must be 0 (the peak) or more",
        ),
        (
            c.max_frame_average_nits,
            "max_frame_average_nits must be 0 (paper white) or more",
        ),
    ] {
        if value.is_nan() || value < 0.0 {
            return Err(invalid(message));
        }
    }
    let mut h = if c.enabled {
        HdrOutput::on()
    } else {
        HdrOutput::default()
    };
    h.paper_white_nits = c.paper_white_nits;
    h.peak_nits = c.peak_nits;
    h.max_content_light_nits = c.max_content_light_nits;
    h.max_frame_average_nits = c.max_frame_average_nits;
    Ok(h)
}

/// How frames reach the display (Rust `HdrEncoding`).
/// ```
/// assert_eq!(ponko_ffi::PonkoHdrEncoding::PONKO_HDR_ENCODING_PQ as u32, 3);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoHdrEncoding {
    /// Standard dynamic range (sRGB).
    PONKO_HDR_ENCODING_SDR = 0,
    /// scRGB: linear, 16-bit float, 1.0 is SDR white (Windows, macOS).
    PONKO_HDR_ENCODING_SCRGB = 1,
    /// Extended-range sRGB, 16-bit float (a browser's HDR canvas).
    PONKO_HDR_ENCODING_EXTENDED_SRGB = 2,
    /// HDR10's PQ signal with BT.2020 primaries, 10-bit.
    PONKO_HDR_ENCODING_PQ = 3,
}

/// Why HDR isn't active (Rust `HdrOffReason`); `NONE` while it is.
/// ```
/// assert_eq!(ponko_ffi::PonkoHdrOffReason::PONKO_HDR_OFF_NOT_REQUESTED as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoHdrOffReason {
    /// HDR is active.
    PONKO_HDR_OFF_NONE = 0,
    /// No window (headless).
    PONKO_HDR_OFF_HEADLESS = 1,
    /// Not reported any more (HDR switches on while the game runs; off is
    /// `PONKO_HDR_OFF_OFF`). Kept so code naming it builds.
    PONKO_HDR_OFF_NOT_REQUESTED = 2,
    /// `enabled` is false.
    PONKO_HDR_OFF_OFF = 3,
    /// The display system offers no HDR color space the renderer can encode.
    PONKO_HDR_OFF_UNSUPPORTED = 4,
}

/// What the window does with the HDR settings (Rust `HdrStatus`), in a
/// layout that can grow: set `struct_size` and `version`, then read it with
/// `ponko_ctx_hdr_status`.
///
/// Version 1: the fields up to `display_headroom`.
/// Version 2: `metadata_sent` and the HDR10 metadata fields.
/// ```
/// use ponko_ffi::*;
/// let mut s: PonkoHdrStatus = unsafe { std::mem::zeroed() };
/// (s.struct_size, s.version) = (size_of::<PonkoHdrStatus>() as u32, PONKO_HDR_STATUS_VERSION);
/// let status = unsafe { ponko_ctx_hdr_status(std::ptr::null_mut(), &mut s) };
/// assert_eq!(status, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoHdrStatus {
    /// `sizeof(PonkoHdrStatus)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_HDR_STATUS_VERSION` as the caller compiled it.
    pub version: u32,
    /// The display system offers an HDR color space the renderer can encode
    /// (Windows and macOS do even on an SDR display).
    pub available: bool,
    /// Frames go out in HDR now.
    pub active: bool,
    /// How frames go out.
    pub encoding: PonkoHdrEncoding,
    /// Why HDR isn't active; `PONKO_HDR_OFF_NONE` while it is.
    pub reason: PonkoHdrOffReason,
    /// Paper white as used, in nits.
    pub paper_white_nits: f32,
    /// The brightest highlight as used, in nits.
    pub peak_nits: f32,
    /// `peak_nits / paper_white_nits`; 1 in SDR.
    pub headroom: f32,
    /// The display's peak as the system reports it (Windows); 0 unknown.
    pub display_peak_nits: f32,
    /// The display's headroom over SDR white now (macOS, Windows); 0 unknown.
    pub display_headroom: f32,
    /// The display was given HDR10 metadata (Windows; Apple with PQ). The
    /// four values below are what it got; all 0 when false.
    pub metadata_sent: bool,
    /// The mastering display's black, in nits.
    pub min_mastering_nits: f32,
    /// The mastering display's peak, in nits.
    pub max_mastering_nits: f32,
    /// MaxCLL, in nits.
    pub max_content_light_nits: f32,
    /// MaxFALL, in nits.
    pub max_frame_average_nits: f32,
}

const _: () = assert!(offset_of!(PonkoHdrStatus, struct_size) == 0 && offset_of!(PonkoHdrStatus, version) == 4);

impl Versioned for PonkoHdrStatus {
    const NAME: &'static str = "PonkoHdrStatus";
    const VERSION: u32 = PONKO_HDR_STATUS_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, display_headroom: f32)),
            2 => Some(end_of!(Self, max_frame_average_nits: f32)),
            _ => None,
        }
    }
}

fn status_to_c(s: &HdrStatus) -> PonkoHdrStatus {
    PonkoHdrStatus {
        struct_size: size_of::<PonkoHdrStatus>() as u32,
        version: PONKO_HDR_STATUS_VERSION,
        available: s.available,
        active: s.active,
        encoding: match s.encoding {
            HdrEncoding::ScRgb => PonkoHdrEncoding::PONKO_HDR_ENCODING_SCRGB,
            HdrEncoding::ExtendedSrgb => PonkoHdrEncoding::PONKO_HDR_ENCODING_EXTENDED_SRGB,
            HdrEncoding::Pq => PonkoHdrEncoding::PONKO_HDR_ENCODING_PQ,
            _ => PonkoHdrEncoding::PONKO_HDR_ENCODING_SDR,
        },
        reason: match s.reason {
            None => PonkoHdrOffReason::PONKO_HDR_OFF_NONE,
            Some(HdrOffReason::Headless) => PonkoHdrOffReason::PONKO_HDR_OFF_HEADLESS,
            Some(HdrOffReason::NotRequested) => PonkoHdrOffReason::PONKO_HDR_OFF_NOT_REQUESTED,
            Some(HdrOffReason::Off) => PonkoHdrOffReason::PONKO_HDR_OFF_OFF,
            Some(_) => PonkoHdrOffReason::PONKO_HDR_OFF_UNSUPPORTED,
        },
        paper_white_nits: s.paper_white_nits,
        peak_nits: s.peak_nits,
        headroom: s.headroom,
        display_peak_nits: s.display_peak_nits.unwrap_or(0.0),
        display_headroom: s.display_headroom.unwrap_or(0.0),
        metadata_sent: s.metadata.is_some(),
        min_mastering_nits: s.metadata.map_or(0.0, |m| m.min_mastering_nits),
        max_mastering_nits: s.metadata.map_or(0.0, |m| m.max_mastering_nits),
        max_content_light_nits: s.metadata.map_or(0.0, |m| m.max_content_light_nits),
        max_frame_average_nits: s.metadata.map_or(0.0, |m| m.max_frame_average_nits),
    }
}

/// Fills the fields of `out_hdr`'s version with the defaults (HDR off,
/// 200-nit paper white, the display's peak). `struct_size` and `version`
/// must be set first.
///
/// Unstable: the guide's "HDR displays" is experimental, so this may change
/// or go in any release (STABILITY.md, "Unstable").
/// cbindgen:prefix=/* Unstable: experimental (book/src/hdr.md) */
///
/// # Safety
/// `out_hdr` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hdr_output_defaults(out_hdr: *mut PonkoHdrOutput) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(out_hdr, &output_to_c(&HdrOutput::default())) })
}

/// Opens the window with these HDR output settings (Rust
/// `Game::hdr_output`); `ponko_ctx_set_hdr_output` changes them later.
/// `PONKO_INVALID_ARGUMENT` for a NaN or negative `peak_nits` or metadata
/// value; the game is then left as it was.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_hdr_output(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
///
/// Unstable: the guide's "HDR displays" is experimental, so this may change
/// or go in any release (STABILITY.md, "Unstable").
/// cbindgen:prefix=/* Unstable: experimental (book/src/hdr.md) */
///
/// # Safety
/// `game` null or a live game; `hdr` null or a readable descriptor (read
/// during the call only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_hdr_output(game: *mut PonkoGame, hdr: *const PonkoHdrOutput) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            match read_desc(hdr, output_to_c(&HdrOutput::default())).and_then(|c| output_from_c(&c)) {
                Ok(h) => ok(g.hdr_output(h)),
                Err(e) => (g, Err(e)),
            }
        })
    }
}

/// Reads the HDR output settings into the fields of `out_hdr`'s version
/// (set its `struct_size` and `version` first).
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_hdr_output(std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
///
/// Unstable: the guide's "HDR displays" is experimental, so this may change
/// or go in any release (STABILITY.md, "Unstable").
/// cbindgen:prefix=/* Unstable: experimental (book/src/hdr.md) */
///
/// # Safety
/// `ctx` null or live; `out_hdr` null or a writable descriptor with its
/// header set (written during the call only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_hdr_output(ctx: *mut PonkoCtx, out_hdr: *mut PonkoHdrOutput) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(out_hdr, &output_to_c(&c.gfx.hdr_output()))) })
}

/// Changes the HDR output settings (Rust `Graphics::set_hdr_output`) from
/// the next frame: `enabled` switches the window between HDR and SDR
/// output while the game runs (no restart), paper white, peak and HDR10
/// metadata follow. Fields newer than the caller's version keep their
/// values. `PONKO_INVALID_ARGUMENT` for a NaN or negative `peak_nits` or
/// metadata value; nothing changes then.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_hdr_output(std::ptr::null_mut(), std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
///
/// Unstable: the guide's "HDR displays" is experimental, so this may change
/// or go in any release (STABILITY.md, "Unstable").
/// cbindgen:prefix=/* Unstable: experimental (book/src/hdr.md) */
///
/// # Safety
/// `ctx` null or live; `hdr` null or a readable descriptor (read during the
/// call only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_hdr_output(ctx: *mut PonkoCtx, hdr: *const PonkoHdrOutput) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let read = read_desc(hdr, output_to_c(&c.gfx.hdr_output()))?;
            c.gfx.set_hdr_output(output_from_c(&read)?);
            Ok(())
        })
    })
}

/// Reads what the window does with the HDR settings into the fields of
/// `out_status`'s version (set its `struct_size` and `version` first).
///
/// Unstable: the guide's "HDR displays" is experimental, so this may change
/// or go in any release (STABILITY.md, "Unstable").
/// cbindgen:prefix=/* Unstable: experimental (book/src/hdr.md) */
///
/// # Safety
/// `ctx` null or live; `out_status` null or a writable descriptor with its
/// header set (written during the call only).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_hdr_status(ctx: *mut PonkoCtx, out_status: *mut PonkoHdrStatus) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(out_status, &status_to_c(&c.gfx.hdr_status()))) })
}
