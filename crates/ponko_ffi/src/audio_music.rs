//! Sound control, music, pause groups, voice limits and output devices
//! from C (Rust `Audio::set_volume`, `Audio::set_loop`, `Audio::play_music`,
//! `Audio::pause_bus`, `Audio::set_voice_limit`, `Audio::set_output_device`,
//! `Audio::open_offline`, `Game::pause_audio_in`).

use std::ffi::c_char;

use ponko::audio::{Quantize, Song};
use ponko::prelude::*;

use crate::audio3d::{PonkoAudioBus, bus_from_c, bus_to_c};
use crate::ctx::{ctx_access, out, text};
use crate::game::{build, ok};
use crate::media::{PonkoPlaying, PonkoSound, playing_from_c, playing_to_c, sound_from_c};
use crate::{PonkoCtx, PonkoGame, PonkoStatus, boundary, engine_error, invalid, write_text};

/// When a music change happens (Rust `Quantize`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoQuantize {
    /// At once.
    PONKO_QUANTIZE_NOW = 0,
    /// On the next beat.
    PONKO_QUANTIZE_BEAT = 1,
    /// On the next bar line.
    PONKO_QUANTIZE_BAR = 2,
}

fn quantize(q: PonkoQuantize) -> Quantize {
    match q {
        PonkoQuantize::PONKO_QUANTIZE_NOW => Quantize::Now,
        PonkoQuantize::PONKO_QUANTIZE_BEAT => Quantize::Beat,
        PonkoQuantize::PONKO_QUANTIZE_BAR => Quantize::Bar,
    }
}

/// Fades a playing sound to `volume` (1: as played) over `seconds` (Rust
/// `Audio::set_volume`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_sound_volume(std::ptr::null_mut(), Default::default(), 0.5, 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_sound_volume(
    ctx: *mut PonkoCtx,
    playing: PonkoPlaying,
    volume: f32,
    seconds: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_volume(playing_from_c(playing), volume, seconds);
            Ok(())
        })
    })
}

/// Changes a playing sound's speed and pitch (1: as played) over `seconds`
/// (Rust `Audio::set_speed`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_sound_speed(std::ptr::null_mut(), Default::default(), 2.0, 0.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_sound_speed(
    ctx: *mut PonkoCtx,
    playing: PonkoPlaying,
    speed: f32,
    seconds: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_speed(playing_from_c(playing), speed, seconds);
            Ok(())
        })
    })
}

/// Fades a playing sound out over `seconds`, then stops it (Rust `Audio::fade_out`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_fade_out_sound(std::ptr::null_mut(), Default::default(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_fade_out_sound(
    ctx: *mut PonkoCtx,
    playing: PonkoPlaying,
    seconds: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.fade_out(playing_from_c(playing), seconds);
            Ok(())
        })
    })
}

/// How important a playing sound is against the voice limits, 0–255
/// (Rust `Audio::set_priority`; 128 unless set).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_sound_priority(std::ptr::null_mut(), Default::default(), 200) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_sound_priority(
    ctx: *mut PonkoCtx,
    playing: PonkoPlaying,
    priority: u8,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_priority(playing_from_c(playing), priority);
            Ok(())
        })
    })
}

/// Whether a playing sound is virtual: followed but not heard (Rust
/// `Audio::is_virtual`).
/// ```
/// let mut v = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_sound_is_virtual(std::ptr::null_mut(), Default::default(), &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_virtual` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sound_is_virtual(
    ctx: *mut PonkoCtx,
    playing: PonkoPlaying,
    out_virtual: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_virtual, c.audio.is_virtual(playing_from_c(playing)))) })
}

/// How many sounds are heard and how many are virtual, music not counted
/// (Rust `Audio::voice_count`).
/// ```
/// let (mut r, mut v) = (0, 0);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_voice_count(std::ptr::null_mut(), &mut r, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_voice_count(
    ctx: *mut PonkoCtx,
    out_real: *mut u32,
    out_virtual: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let count = c.audio.voice_count();
            out(out_real, count.real)?;
            out(out_virtual, count.virtualized)
        })
    })
}

/// How many sounds are heard at once at most, 1–256 (Rust
/// `Audio::set_voice_limit`; 64 unless set).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_voice_limit(std::ptr::null_mut(), 32) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_voice_limit(ctx: *mut PonkoCtx, limit: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_voice_limit(limit);
            Ok(())
        })
    })
}

/// The voice limit (Rust `Audio::voice_limit`).
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_voice_limit(std::ptr::null_mut(), &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_limit` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_voice_limit(ctx: *mut PonkoCtx, out_limit: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_limit, c.audio.voice_limit())) })
}

/// How many sounds on `bus` itself are heard at once at most; 0 for no
/// limit of its own (Rust `Audio::set_bus_voice_limit`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_bus_voice_limit(std::ptr::null_mut(), Default::default(), 8) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_bus_voice_limit(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    limit: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio
                .set_bus_voice_limit(bus_from_c(bus), (limit > 0).then_some(limit));
            Ok(())
        })
    })
}

/// `bus`'s own voice limit, 0 for none (Rust `Audio::bus_voice_limit`).
/// ```
/// let mut n = 1;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_voice_limit(std::ptr::null_mut(), Default::default(), &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_limit` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_voice_limit(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_limit: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            out(out_limit, c.audio.bus_voice_limit(bus_from_c(bus)).unwrap_or(0))
        })
    })
}

/// Loop points for `sound`, in seconds: looping playbacks go back to
/// `start` at `end`; `end` below 0 is the end of the sound (Rust
/// `Audio::set_loop`). An intro plays once before `start`.
/// ```
/// let s = ponko_ffi::PonkoSound { id: 0, seconds: 0.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_sound_loop(std::ptr::null_mut(), s, 2.0, -1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_sound_loop(
    ctx: *mut PonkoCtx,
    sound: PonkoSound,
    start: f32,
    end: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio
                .set_loop(sound_from_c(sound), start, (end >= 0.0).then_some(end));
            Ok(())
        })
    })
}

/// `sound`'s loop points; `out_end` is -1 for the end of the sound (Rust
/// `Audio::sound_loop`).
/// ```
/// let s = ponko_ffi::PonkoSound { id: 0, seconds: 0.0 };
/// let (mut a, mut b) = (0.0, 0.0);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_sound_loop(std::ptr::null_mut(), s, &mut a, &mut b) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_sound_loop(
    ctx: *mut PonkoCtx,
    sound: PonkoSound,
    out_start: *mut f32,
    out_end: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (start, end) = c.audio.sound_loop(sound_from_c(sound));
            out(out_start, start)?;
            out(out_end, end.unwrap_or(-1.0))
        })
    })
}

/// The priority `entity`'s positional sound starts at (Rust
/// `AudioSource3D::with_priority`); set it right after
/// `ponko_ctx_set_audio_source3d`, before the sound starts.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_source3d_priority(std::ptr::null_mut(), 0, 200) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_source3d_priority(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    priority: u8,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = Entity::from_bits(entity_bits);
            let Some(source) = c.world.get_mut::<ponko::d3::AudioSource>(e) else {
                return Err(invalid("the entity has no positional sound"));
            };
            source.priority = priority;
            Ok(())
        })
    })
}

/// Pauses `bus` and the buses under it, or resumes it (Rust `Audio::pause_bus`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_pause_audio_bus(std::ptr::null_mut(), Default::default(), true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_pause_audio_bus(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    paused: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.pause_bus(bus_from_c(bus), paused);
            Ok(())
        })
    })
}

/// Whether `bus` is paused, by itself, a game state or a parent (Rust
/// `Audio::bus_paused`).
/// ```
/// let mut p = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_paused(std::ptr::null_mut(), Default::default(), &mut p) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_paused` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_paused(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_paused: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_paused, c.audio.bus_paused(bus_from_c(bus)))) })
}

/// Pauses the `count` buses named in `buses` while game state `state` is
/// on the stack (Rust `Game::pause_audio_in`). Needs `ponko_game_state`
/// first, or it is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_pause_audio_in_state(std::ptr::null_mut(), 1, std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// Live game; `buses` holds `count` NUL-terminated names (null when 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_pause_audio_in_state(
    game: *mut PonkoGame,
    state: u32,
    buses: *const *const c_char,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        let mut names = Vec::with_capacity(count);
        if count > 0 && buses.is_null() {
            return boundary(|| Err(invalid("bus names are null")));
        }
        for i in 0..count {
            match text(*buses.add(i)) {
                Ok(name) => names.push(name.to_owned()),
                Err(e) => return boundary(|| Err(e)),
            }
        }
        build(game, |g| {
            if !g.has_state::<u32>() {
                return (
                    g,
                    Err(invalid("the game has no state machine: call ponko_game_state first")),
                );
            }
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            ok(g.pause_audio_in(state, &refs))
        })
    }
}

/// Plays a song of `count` layers (`layers[i]` at `volumes[i]`; `volumes`
/// null: all at 1), crossfading from the one playing over `fade` seconds
/// from `at` (Rust `Audio::play_music`). `bpm` 0 means no beat grid
/// (changes happen at once).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_play_music(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), 0, 120.0, 4, 0.0, ponko_ffi::PonkoQuantize::PONKO_QUANTIZE_NOW) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `layers` (and `volumes` unless null) hold `count` items.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)] // a song's fields, flat for C
pub unsafe extern "C" fn ponko_ctx_play_music(
    ctx: *mut PonkoCtx,
    layers: *const PonkoSound,
    volumes: *const f32,
    count: usize,
    bpm: f32,
    beats_per_bar: u32,
    fade: f32,
    at: PonkoQuantize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if count == 0 || layers.is_null() {
                return Err(invalid("a song needs at least one layer"));
            }
            let layers: Vec<(Sound, f32)> = std::slice::from_raw_parts(layers, count)
                .iter()
                .enumerate()
                .map(|(i, &layer)| {
                    let volume = if volumes.is_null() { 1.0 } else { *volumes.add(i) };
                    (sound_from_c(layer), volume)
                })
                .collect();
            let Some(song) = Song::from_layers(&layers) else {
                return Err(invalid("a song needs at least one layer"));
            };
            c.audio.play_music(song.tempo(bpm, beats_per_bar), fade, quantize(at));
            Ok(())
        })
    })
}

/// Stops the music, fading out over `fade` seconds from `at` (Rust
/// `Audio::stop_music`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_stop_music(std::ptr::null_mut(), 1.0, ponko_ffi::PonkoQuantize::PONKO_QUANTIZE_BAR) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_stop_music(ctx: *mut PonkoCtx, fade: f32, at: PonkoQuantize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.stop_music(fade, quantize(at));
            Ok(())
        })
    })
}

/// Fades layer `layer` of the playing song to `volume` over `fade` seconds
/// from `at` (Rust `Audio::set_music_layer`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_music_layer(std::ptr::null_mut(), 1, 1.0, 0.5, ponko_ffi::PonkoQuantize::PONKO_QUANTIZE_BAR) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_music_layer(
    ctx: *mut PonkoCtx,
    layer: usize,
    volume: f32,
    fade: f32,
    at: PonkoQuantize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_music_layer(layer, volume, fade, quantize(at));
            Ok(())
        })
    })
}

/// Plays `sound` once on the music bus at `at` (Rust `Audio::play_stinger`);
/// writes the playback to `out_playing` unless that is null.
/// ```
/// let s = ponko_ffi::PonkoSound { id: 0, seconds: 0.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_play_stinger(std::ptr::null_mut(), s, ponko_ffi::PonkoQuantize::PONKO_QUANTIZE_BEAT, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_playing` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_play_stinger(
    ctx: *mut PonkoCtx,
    sound: PonkoSound,
    at: PonkoQuantize,
    out_playing: *mut PonkoPlaying,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let playing = c.audio.play_stinger(sound_from_c(sound), quantize(at));
            if !out_playing.is_null() {
                out_playing.write(playing_to_c(playing));
            }
            Ok(())
        })
    })
}

/// Changes the music's tempo and pitch over `fade` seconds (Rust
/// `Audio::set_music_speed`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_music_speed(std::ptr::null_mut(), 1.1, 2.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_music_speed(ctx: *mut PonkoCtx, speed: f32, fade: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_music_speed(speed, fade);
            Ok(())
        })
    })
}

/// Plays music on `bus` from now on (Rust `Audio::set_music_bus`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_music_bus(std::ptr::null_mut(), Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_music_bus(ctx: *mut PonkoCtx, bus: PonkoAudioBus) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.set_music_bus(bus_from_c(bus));
            Ok(())
        })
    })
}

/// The bus music plays on (`music` unless set; Rust `Audio::music_bus`).
/// ```
/// let mut b = Default::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_music_bus(std::ptr::null_mut(), &mut b) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_bus` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_music_bus(ctx: *mut PonkoCtx, out_bus: *mut PonkoAudioBus) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_bus, bus_to_c(c.audio.music_bus()))) })
}

/// Beats since the playing song started, and whether one plays (Rust
/// `Audio::music_beat`, `Audio::music`).
/// ```
/// let (mut b, mut p) = (0.0, false);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_music_beat(std::ptr::null_mut(), &mut b, &mut p) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_music_beat(
    ctx: *mut PonkoCtx,
    out_beat: *mut f64,
    out_playing: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            out(out_beat, c.audio.music_beat())?;
            out(out_playing, c.audio.music().is_some())
        })
    })
}

/// The volume layer `layer` of the playing song is set to (Rust
/// `Audio::music_layer_volume`).
/// ```
/// let mut v = 1.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_music_layer_volume(std::ptr::null_mut(), 0, &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_volume` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_music_layer_volume(
    ctx: *mut PonkoCtx,
    layer: usize,
    out_volume: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_volume, c.audio.music_layer_volume(layer))) })
}

/// The output devices as JSON, `[{"id": ..., "name": ..., "default": ...}]`
/// (Rust `Audio::output_devices`), into `buffer`: the length always, the
/// text when it fits (call with capacity 0 first to size it). Empty in
/// headless runs.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_output_devices(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_output_devices(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let json = serde_json::to_string(&c.audio.output_devices()).unwrap_or_else(|_| "[]".into());
            write_text(buffer, capacity, out_len, &json)
        })
    })
}

/// How the game's sound shares the device with other apps and a phone's
/// ring/silent switch (Rust `AudioSessionCategory`): iOS's audio session
/// categories; kept and without effect elsewhere.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAudioSessionCategory {
    /// The default: stops other apps' audio; the silent switch mutes the game.
    PONKO_AUDIO_SESSION_SOLO_AMBIENT = 0,
    /// Mixes with other apps' audio (the player's music plays on); the silent switch mutes the game.
    PONKO_AUDIO_SESSION_AMBIENT = 1,
    /// Heard with the silent switch on; stops other apps' audio.
    PONKO_AUDIO_SESSION_PLAYBACK = 2,
}

/// Sets the audio session's category from now on (Rust
/// `Audio::set_session_category`): a `PonkoAudioSessionCategory`. An
/// unknown value is `PONKO_INVALID_ARGUMENT`; a category the system refuses
/// is `PONKO_UNSUPPORTED`, and the old one stays.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_set_session_category(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_set_session_category(ctx: *mut PonkoCtx, category: u32) -> PonkoStatus {
    use ponko::audio::AudioSessionCategory as C;
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let category = match category {
            0 => C::SoloAmbient,
            1 => C::Ambient,
            2 => C::Playback,
            _ => {
                return Err(invalid(
                    "unknown audio session category: use a PonkoAudioSessionCategory",
                ));
            }
        };
        ctx_access(ctx, |c| c.audio.set_session_category(category).map_err(engine_error))
    })
}

/// Writes the audio session's category, a `PonkoAudioSessionCategory`
/// (Rust `Audio::session_category`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_session_category(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `category` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_session_category(ctx: *mut PonkoCtx, category: *mut u32) -> PonkoStatus {
    use ponko::audio::AudioSessionCategory as C;
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if category.is_null() {
            return Err(invalid("audio session category output is null"));
        }
        ctx_access(ctx, |c| {
            let value = match c.audio.session_category() {
                C::Ambient => PonkoAudioSessionCategory::PONKO_AUDIO_SESSION_AMBIENT,
                C::Playback => PonkoAudioSessionCategory::PONKO_AUDIO_SESSION_PLAYBACK,
                _ => PonkoAudioSessionCategory::PONKO_AUDIO_SESSION_SOLO_AMBIENT,
            };
            out(category, value as u32)
        })
    })
}

/// Plays on the device with this id or name from now on; null follows the
/// system's default (Rust `Audio::set_output_device`). An unknown device is
/// `PONKO_NOT_FOUND`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_output_device(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `device` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_output_device(ctx: *mut PonkoCtx, device: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let device = if device.is_null() { None } else { Some(text(device)?) };
            c.audio.set_output_device(device).map_err(engine_error)
        })
    })
}

/// The id of the device sound plays on now, "" for none (Rust
/// `Audio::output_device`), with the text-buffer convention of
/// `ponko_ctx_audio_output_devices`.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_output_device(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_output_device(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = c.audio.output_device().map_or("", |d| d.id.as_str()).to_owned();
            write_text(buffer, capacity, out_len, &id)
        })
    })
}

/// What happened to the output this frame as JSON, `[{"lost": name},
/// {"switched": name}]` (Rust `Audio::device_events`), with the text-buffer
/// convention of `ponko_ctx_audio_output_devices`.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_device_events(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes; `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_device_events(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let json = serde_json::to_string(c.audio.device_events()).unwrap_or_else(|_| "[]".into());
            write_text(buffer, capacity, out_len, &json)
        })
    })
}

/// Mixes into memory instead of a device from now on, at `sample_rate`
/// (Rust `Audio::open_offline`); works in headless runs.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_open_audio_offline(std::ptr::null_mut(), 48000) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_open_audio_offline(ctx: *mut PonkoCtx, sample_rate: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.open_offline(sample_rate);
            Ok(())
        })
    })
}

/// Renders the next `seconds` of the offline mix (Rust
/// `Audio::render_offline`) as interleaved left/right floats into `frames`,
/// up to `capacity` floats; `out_count` gets how many floats were rendered
/// (two per frame), all of which were written when they fit.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_render_audio_offline(std::ptr::null_mut(), 0.1, std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `frames` holds `capacity` writable floats (or is
/// null with capacity 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_render_audio_offline(
    ctx: *mut PonkoCtx,
    seconds: f32,
    frames: *mut f32,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_count.is_null() || (frames.is_null() && capacity != 0) {
                return Err(invalid("output is null"));
            }
            let mix = c.audio.render_offline(seconds);
            out_count.write(mix.len() * 2);
            for (i, v) in mix.iter().flatten().take(capacity).enumerate() {
                frames.add(i).write(*v);
            }
            Ok(())
        })
    })
}
