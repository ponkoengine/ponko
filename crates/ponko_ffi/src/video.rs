//! Video playback from C (Rust `ponko::video::Video`, feature `video`): a
//! WebM file with AV1 pictures and Vorbis sound played into a texture,
//! its sound through the mixer. Built without the feature, loading reports
//! `PONKO_UNSUPPORTED` and no video is ever made.
use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};

use crate::audio3d::{PonkoAudioBus, bus_from_c, bus_to_c};
use crate::ctx::{ctx_access, out, text};
use crate::descriptors::{Versioned, write_desc};
use crate::media::{PonkoSound, sound_to_c};
use crate::types::{PonkoTexture, texture_to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, invalid};

#[cfg(feature = "video")]
type Inner = ponko::video::Video;

/// Builds without the `video` feature: no video is ever made, so the
/// methods are never called.
#[cfg(not(feature = "video"))]
enum Inner {}

#[cfg(not(feature = "video"))]
#[allow(clippy::unused_self, clippy::needless_pass_by_ref_mut)]
impl Inner {
    fn from_bytes(_: &mut ponko::prelude::Ctx, _: Vec<u8>) -> Result<Self, ponko::prelude::Error> {
        Err(ponko::prelude::Error::new(
            ponko::prelude::ErrorKind::Unsupported,
            "ponko_ffi was built without the `video` feature",
        ))
    }
    fn load(c: &mut ponko::prelude::Ctx, _: &str) -> Result<Self, ponko::prelude::Error> {
        Self::from_bytes(c, Vec::new())
    }
    fn texture(&self) -> ponko::prelude::Texture {
        match *self {}
    }
    fn size(&self) -> ponko::prelude::UVec2 {
        match *self {}
    }
    fn seconds(&self) -> f32 {
        match *self {}
    }
    fn time(&self) -> f32 {
        match *self {}
    }
    fn frame_time(&self) -> f32 {
        match *self {}
    }
    fn frame_rgba(&self) -> &[u8] {
        match *self {}
    }
    fn sound(&self) -> Option<ponko::prelude::Sound> {
        match *self {}
    }
    fn playing(&self) -> bool {
        match *self {}
    }
    fn finished(&self) -> bool {
        match *self {}
    }
    fn looping(&self) -> bool {
        match *self {}
    }
    fn set_looping(&mut self, _: bool) {
        match *self {}
    }
    fn bus(&self) -> ponko::prelude::AudioBus {
        match *self {}
    }
    fn set_bus(&mut self, _: &mut ponko::prelude::Ctx, _: ponko::prelude::AudioBus) {
        match *self {}
    }
    fn volume(&self) -> f32 {
        match *self {}
    }
    fn set_volume(&mut self, _: &mut ponko::prelude::Ctx, _: f32) {
        match *self {}
    }
    fn play(&mut self, _: &mut ponko::prelude::Ctx) {
        match *self {}
    }
    fn pause(&mut self, _: &mut ponko::prelude::Ctx) {
        match *self {}
    }
    fn seek(&mut self, _: &mut ponko::prelude::Ctx, _: f32) -> Result<(), ponko::prelude::Error> {
        match *self {}
    }
    fn update(&mut self, _: &mut ponko::prelude::Ctx) -> Result<(), ponko::prelude::Error> {
        match *self {}
    }
    fn advance(&mut self, _: &mut ponko::prelude::Ctx, _: f32) -> Result<(), ponko::prelude::Error> {
        match *self {}
    }
    fn free(self, _: &mut ponko::prelude::Ctx) {
        match self {}
    }
}

/// A video being played into a texture (Rust `Video`): from
/// `ponko_ctx_load_video` or `ponko_ctx_video_from_bytes`, freed with
/// `ponko_ctx_free_video`.
pub struct PonkoVideo(Inner);

/// The video's errors: a missing file is `PONKO_NOT_FOUND`, a build without
/// the feature or a codec it doesn't play `PONKO_UNSUPPORTED`, a damaged
/// file `PONKO_INVALID_ARGUMENT`.
fn video_error(e: ponko::prelude::Error) -> Failure {
    let unsupported = e.kind() == ponko::prelude::ErrorKind::Unsupported;
    let mut failure = engine_error(e);
    if unsupported {
        failure.0 = PonkoStatus::PONKO_UNSUPPORTED;
    }
    failure
}

fn video_mut<'a>(video: *mut PonkoVideo) -> Result<&'a mut Inner, Failure> {
    // SAFETY: the caller's contract: null or a live video.
    unsafe { video.as_mut() }
        .map(|v| &mut v.0)
        .ok_or_else(|| invalid("video is null"))
}

fn made(video: Inner, out_video: *mut *mut PonkoVideo) -> Result<(), Failure> {
    let raw = Box::into_raw(Box::new(PonkoVideo(video)));
    // SAFETY: the caller's contract: `out_video` is writable (checked non-null).
    unsafe { out(out_video, raw) }
}

/// Loads a video from an added asset source or the `assets/` folder and
/// shows its first frame, paused (Rust `Video::load`): a WebM file with
/// AV1 pictures (8-bit) and, if it has sound, Vorbis. A missing file is
/// `PONKO_NOT_FOUND`; other codecs, the web, or a build without the
/// `video` feature are `PONKO_UNSUPPORTED` (`ponko_last_error` says how to
/// encode instead); a damaged file is `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `path` is read during the call only. The caller owns the
/// video written to `out_video` and frees it with `ponko_ctx_free_video`,
/// which also frees its texture and sound.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_video(std::ptr::null_mut(), c"intro.webm".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` a C string; `out_video` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_video(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    out_video: *mut *mut PonkoVideo,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            if out_video.is_null() {
                return Err(invalid("output is null"));
            }
            made(Inner::load(c, path).map_err(video_error)?, out_video)
        })
    })
}

/// Makes a video from a file's `size` bytes at `data` (Rust
/// `Video::from_bytes`), paused on its first frame. Errors as
/// `ponko_ctx_load_video`.
///
/// Ownership: `data` is copied during the call. The caller owns the video
/// written to `out_video` and frees it with `ponko_ctx_free_video`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_from_bytes(std::ptr::null_mut(), std::ptr::null(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `data` readable for `size` bytes; `out_video`
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_from_bytes(
    ctx: *mut PonkoCtx,
    data: *const c_void,
    size: usize,
    out_video: *mut *mut PonkoVideo,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_video.is_null() || (data.is_null() && size > 0) {
                return Err(invalid("data or output is null"));
            }
            let bytes = if size == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(data.cast::<u8>(), size).to_vec()
            };
            made(Inner::from_bytes(c, bytes).map_err(video_error)?, out_video)
        })
    })
}

/// Stops a video's sound and frees it, its texture and its sound (Rust
/// `Video::free`). Null is ignored.
///
/// Ownership: takes `video` back; don't use it afterwards.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_free_video(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live (the one the video was made with); `video` null or
/// live, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_free_video(ctx: *mut PonkoCtx, video: *mut PonkoVideo) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if !video.is_null() {
                Box::from_raw(video).0.free(c);
            }
            Ok(())
        })
    })
}

/// The version of `PonkoVideoState` this header describes.
pub const PONKO_VIDEO_STATE_VERSION: u32 = 1;

/// Where a video is (`ponko_video_state`). Set `struct_size` and
/// `version`, then call.
///
/// Version 1: every field below.
/// ```
/// let s: ponko_ffi::PonkoVideoState = unsafe { std::mem::zeroed() };
/// assert!(!s.playing);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoVideoState {
    /// `sizeof(PonkoVideoState)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_VIDEO_STATE_VERSION` as the caller compiled it.
    pub version: u32,
    /// The texture the current frame is in: draw it, or put it on a
    /// material for a screen in the world.
    pub texture: PonkoTexture,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Length in seconds.
    pub seconds: f32,
    /// Seconds into the video.
    pub time: f32,
    /// The time of the frame shown.
    pub frame_time: f32,
    /// Its sound's volume (1 as recorded).
    pub volume: f32,
    /// Playing (not paused, not at its end).
    pub playing: bool,
    /// Played to its end (never while looping).
    pub finished: bool,
    /// Starts over at its end.
    pub looping: bool,
    /// Whether the file has sound; `sound` is it.
    pub has_sound: bool,
    /// The soundtrack, played by the video itself.
    pub sound: PonkoSound,
    /// The bus its sound plays on.
    pub bus: PonkoAudioBus,
}

impl Versioned for PonkoVideoState {
    const NAME: &'static str = "PonkoVideoState";
    const VERSION: u32 = PONKO_VIDEO_STATE_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, bus) + size_of::<PonkoAudioBus>()),
            _ => None,
        }
    }
}

const _: () = assert!(offset_of!(PonkoVideoState, struct_size) == 0 && offset_of!(PonkoVideoState, version) == 4);

/// Writes where `video` is into the fields of `out_state`'s version: its
/// texture, size, length, time, frame time, volume, whether it plays,
/// finished or loops, its sound and its bus.
///
/// Ownership: `out_state` is the caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_video_state(std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `video` null or live; `out_state` null or a writable descriptor with
/// its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_video_state(video: *const PonkoVideo, out_state: *mut PonkoVideoState) -> PonkoStatus {
    boundary(|| {
        // SAFETY: the caller's contract.
        let v = &unsafe { video.as_ref() }.ok_or_else(|| invalid("video is null"))?.0;
        let size = v.size();
        let state = PonkoVideoState {
            struct_size: size_of::<PonkoVideoState>() as u32,
            version: PONKO_VIDEO_STATE_VERSION,
            texture: texture_to_c(v.texture()),
            width: size.x,
            height: size.y,
            seconds: v.seconds(),
            time: v.time(),
            frame_time: v.frame_time(),
            volume: v.volume(),
            playing: v.playing(),
            finished: v.finished(),
            looping: v.looping(),
            has_sound: v.sound().is_some(),
            sound: v.sound().map_or(PonkoSound { id: 0, seconds: 0.0 }, sound_to_c),
            bus: bus_to_c(v.bus()),
        };
        // SAFETY: the caller's contract.
        unsafe { write_desc(out_state, &state) }
    })
}

/// The frame shown, as RGBA pixels (4 bytes a pixel, row by row, top row
/// first; Rust `Video::frame_rgba`): the texture's pixels, for tests and
/// tools.
///
/// Ownership: the pixels belong to the video and stay valid until the
/// next call that changes it (play, seek, update, advance, free).
/// ```
/// let (mut p, mut n) = (std::ptr::null(), 0usize);
/// assert_eq!(unsafe { ponko_ffi::ponko_video_frame_rgba(std::ptr::null(), &mut p, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `video` null or live; the outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_video_frame_rgba(
    video: *const PonkoVideo,
    out_pixels: *mut *const u8,
    out_size: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: the caller's contract.
        let v = &unsafe { video.as_ref() }.ok_or_else(|| invalid("video is null"))?.0;
        if out_pixels.is_null() || out_size.is_null() {
            return Err(invalid("output is null"));
        }
        let rgba = v.frame_rgba();
        // SAFETY: the caller's contract: both outputs are writable.
        unsafe {
            out(out_pixels, rgba.as_ptr())?;
            out(out_size, rgba.len())
        }
    })
}

/// Starts over at its end (for screens in the world), or stops there
/// (Rust `Video::set_looping`). Off by default.
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_video_set_looping(std::ptr::null_mut(), true) } as u32, 1);
/// ```
/// # Safety
/// `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_video_set_looping(video: *mut PonkoVideo, looping: bool) -> PonkoStatus {
    boundary(|| {
        video_mut(video)?.set_looping(looping);
        Ok(())
    })
}

/// Runs `f` on a video with the whole ctx.
unsafe fn with_video(
    ctx: *mut PonkoCtx,
    video: *mut PonkoVideo,
    f: impl FnOnce(&mut Inner, &mut ponko::prelude::Ctx) -> Result<(), Failure>,
) -> PonkoStatus {
    boundary(|| {
        let v = video_mut(video)?;
        // SAFETY: forwarded caller contract.
        unsafe { ctx_access(ctx, |c| f(v, c)) }
    })
}

/// Plays from where it is; from the start if it had finished (Rust
/// `Video::play`). Its sound starts on its bus.
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_play(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_play(ctx: *mut PonkoCtx, video: *mut PonkoVideo) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_video(ctx, video, |v, c| {
            v.play(c);
            Ok(())
        })
    }
}

/// Pauses: the frame stays and the sound stops (Rust `Video::pause`).
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_pause(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_pause(ctx: *mut PonkoCtx, video: *mut PonkoVideo) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_video(ctx, video, |v, c| {
            v.pause(c);
            Ok(())
        })
    }
}

/// Jumps to `seconds` (clamped to the video), playing or paused: the frame
/// at that time is decoded and shown, and the sound goes on from there
/// (Rust `Video::seek`). A frame that doesn't decode is
/// `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_seek(std::ptr::null_mut(), std::ptr::null_mut(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_seek(ctx: *mut PonkoCtx, video: *mut PonkoVideo, seconds: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_video(ctx, video, |v, c| v.seek(c, seconds).map_err(video_error)) }
}

/// Moves a playing video on by this frame's time and shows the frame for
/// the new time (Rust `Video::update`): call it once a frame, from a frame
/// system, then draw its texture. A frame that doesn't decode is
/// `PONKO_INVALID_ARGUMENT` (the last good frame stays).
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_update(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_update(ctx: *mut PonkoCtx, video: *mut PonkoVideo) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_video(ctx, video, |v, c| v.update(c).map_err(video_error)) }
}

/// `ponko_ctx_video_update` by `seconds` instead of the frame's time (Rust
/// `Video::advance`): another speed (the sound doesn't follow), or
/// stepping a video in a test.
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_advance(std::ptr::null_mut(), std::ptr::null_mut(), 0.1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_advance(
    ctx: *mut PonkoCtx,
    video: *mut PonkoVideo,
    seconds: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_video(ctx, video, |v, c| v.advance(c, seconds).map_err(video_error)) }
}

/// Its sound's volume, 0 silent to 1 as recorded, also while it plays
/// (Rust `Video::set_volume`; a value that isn't finite is 1).
///
/// Ownership: nothing is kept.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_set_volume(std::ptr::null_mut(), std::ptr::null_mut(), 0.5) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_set_volume(
    ctx: *mut PonkoCtx,
    video: *mut PonkoVideo,
    volume: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_video(ctx, video, |v, c| {
            v.set_volume(c, volume);
            Ok(())
        })
    }
}

/// Plays its sound on `bus` (from `ponko_ctx_audio_bus`) from now on; a
/// playing sound moves at once (Rust `Video::set_bus`).
///
/// Ownership: nothing is kept.
/// ```
/// let bus = ponko_ffi::PonkoAudioBus::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_video_set_bus(std::ptr::null_mut(), std::ptr::null_mut(), bus) } as u32, 1);
/// ```
/// # Safety
/// `ctx` and `video` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_video_set_bus(
    ctx: *mut PonkoCtx,
    video: *mut PonkoVideo,
    bus: PonkoAudioBus,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_video(ctx, video, |v, c| {
            v.set_bus(c, bus_from_c(bus));
            Ok(())
        })
    }
}
