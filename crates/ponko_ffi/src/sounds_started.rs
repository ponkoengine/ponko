//! What the game played, for tests and tools (Rust `Audio::started`): the
//! sounds started this tick, headless runs included, which play nothing.

use std::mem::{offset_of, size_of};

use ponko::audio::SoundStarted;

use crate::audio3d::{PonkoAudioBus, bus_to_c};
use crate::ctx::{ctx_access, out};
use crate::descriptors::{Versioned, write_desc};
use crate::media::{PonkoPlaying, PonkoSound, playing_to_c, sound_to_c};
use crate::{PonkoCtx, PonkoStatus, boundary, error};

/// The version of `PonkoSoundStarted` this header describes.
pub const PONKO_SOUND_STARTED_VERSION: u32 = 1;

/// A sound that was started (Rust `SoundStarted`), read with
/// `ponko_ctx_sound_started`. A versioned descriptor: set `struct_size` and
/// `version` before the call, which fills the fields of that version.
///
/// Version 1: `playing`, `sound`, `bus`, `volume`, `speed`, `looping`.
/// ```
/// use ponko_ffi::*;
/// let mut s: PonkoSoundStarted = unsafe { std::mem::zeroed() };
/// (s.struct_size, s.version) = (size_of::<PonkoSoundStarted>() as u32, PONKO_SOUND_STARTED_VERSION);
/// assert_eq!(unsafe { ponko_ctx_sound_started(std::ptr::null_mut(), 0, &mut s) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoSoundStarted {
    /// `sizeof(PonkoSoundStarted)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_SOUND_STARTED_VERSION` as the caller compiled it.
    pub version: u32,
    /// Its handle, as the play call returned it (headless runs hand out
    /// handles too, a different one for each sound).
    pub playing: PonkoPlaying,
    /// The sound.
    pub sound: PonkoSound,
    /// The bus it plays on: `sfx` for `ponko_ctx_play_sound`.
    pub bus: PonkoAudioBus,
    /// Its volume, 0 to 1 as asked for; for a sound played at a 3D point,
    /// with the distance's fall-off applied at the start.
    pub volume: f32,
    /// Its speed (and pitch), 1 as recorded.
    pub speed: f32,
    /// Whether it starts over at its end.
    pub looping: bool,
}

const _: () = assert!(offset_of!(PonkoSoundStarted, struct_size) == 0 && offset_of!(PonkoSoundStarted, version) == 4);

impl Versioned for PonkoSoundStarted {
    const NAME: &'static str = "PonkoSoundStarted";
    const VERSION: u32 = PONKO_SOUND_STARTED_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, looping) + size_of::<bool>()),
            _ => None,
        }
    }
}

fn started_to_c(s: &SoundStarted) -> PonkoSoundStarted {
    PonkoSoundStarted {
        struct_size: size_of::<PonkoSoundStarted>() as u32,
        version: PONKO_SOUND_STARTED_VERSION,
        playing: playing_to_c(s.playing),
        sound: sound_to_c(s.sound),
        bus: bus_to_c(s.bus),
        volume: s.volume,
        speed: s.speed,
        looping: s.looping,
    }
}

/// How many sounds were started this tick (Rust `ctx.audio.started().len()`):
/// by the systems of the tick that is running, or, in a frame system, by
/// the frame's systems so far. The list is emptied when a tick ends and
/// when a frame begins. Read each with `ponko_ctx_sound_started`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_sounds_started(std::ptr::null_mut(), &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sounds_started(ctx: *mut PonkoCtx, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_count, c.audio.started().len())) })
}

/// The `index`th sound started this tick (from 0, oldest first), written
/// into the fields of `out_started`'s version (set its `struct_size` and
/// `version` first). `PONKO_NOT_FOUND` past the last one; a header that
/// isn't set, or null: `PONKO_INVALID_ARGUMENT`.
/// # Safety
/// `ctx` null or live; `out_started` null or a writable descriptor with its
/// header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sound_started(
    ctx: *mut PonkoCtx,
    index: usize,
    out_started: *mut PonkoSoundStarted,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_started.is_null() {
            return Err(crate::invalid("PonkoSoundStarted output is null"));
        }
        let started = ctx_access(ctx, |c| Ok(c.audio.started().get(index).map(started_to_c)))?;
        let Some(started) = started else {
            return Err(error(
                PonkoStatus::PONKO_NOT_FOUND,
                "no sound was started at that index",
            ));
        };
        write_desc(out_started, &started)
    })
}
