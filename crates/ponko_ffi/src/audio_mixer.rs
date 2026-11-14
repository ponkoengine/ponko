//! The mixer from C: the bus tree, bus effect chains, ducking, snapshots,
//! mixer files and the saved mixer state (Rust `Audio::set_bus_parent`,
//! `BusEffect`, `Ducking`, `MixerSnapshot`, `Audio::load_mixer`,
//! `MixerState`).

use std::ffi::c_char;

use ponko::audio::{
    BusEffect, BusSnapshot, CompressorEffect, DelayEffect, Ducking, EffectOverride, FilterEffect, FilterMode,
    MixerSnapshot, MixerState, ReverbEffect,
};

use crate::audio3d::{PonkoAudioBus, bus_from_c, bus_to_c};
use crate::ctx::{ctx_access, out, text};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, error, invalid, invalid_owned, write_text};

/// The kind of a [`PonkoBusEffect`] (Rust `BusEffect`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoBusEffectKind {
    /// A resonant filter (`FilterEffect`).
    PONKO_BUS_EFFECT_FILTER = 0,
    /// A room (`ReverbEffect`).
    PONKO_BUS_EFFECT_REVERB = 1,
    /// A compressor (`CompressorEffect`).
    PONKO_BUS_EFFECT_COMPRESSOR = 2,
    /// An echo (`DelayEffect`).
    PONKO_BUS_EFFECT_DELAY = 3,
}

/// Which frequencies a bus filter lets through (Rust `FilterMode`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoFilterMode {
    /// Below the cutoff: muffles.
    PONKO_FILTER_LOW_PASS = 0,
    /// Around the cutoff.
    PONKO_FILTER_BAND_PASS = 1,
    /// Above the cutoff: thins.
    PONKO_FILTER_HIGH_PASS = 2,
    /// All but the cutoff.
    PONKO_FILTER_NOTCH = 3,
}

/// One effect in a bus's chain (Rust `BusEffect`): `kind` says which
/// fields it reads; the others are ignored. Start from
/// `ponko_bus_effect_default(kind)`, which fills every field.
/// ```
/// use ponko_ffi::*;
/// let f = ponko_bus_effect_default(PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER);
/// assert_eq!((f.cutoff_hz, f.mix), (20000.0, 1.0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoBusEffect {
    /// Which effect.
    pub kind: PonkoBusEffectKind,
    /// Filter: which frequencies pass.
    pub mode: PonkoFilterMode,
    /// Filter: cutoff in Hz, 20–20,000.
    pub cutoff_hz: f32,
    /// Filter: emphasis at the cutoff, 0–1.
    pub resonance: f32,
    /// Reverb: how long it rings, 0–1.
    pub feedback: f32,
    /// Reverb: how fast the highs fade, 0–1.
    pub damping: f32,
    /// Reverb: 0 mono to 1 full stereo.
    pub stereo_width: f32,
    /// Compressor: level in dB above which it compresses.
    pub threshold_db: f32,
    /// Compressor: 4 turns 4 dB over the threshold into 1.
    pub ratio: f32,
    /// Compressor: seconds to clamp down.
    pub attack: f32,
    /// Compressor: seconds to let go.
    pub release: f32,
    /// Compressor: gain in dB after compressing.
    pub makeup_db: f32,
    /// Delay: seconds between echoes (fixed when the bus is built).
    pub delay: f32,
    /// Delay: each echo's level against the one before, in dB.
    pub feedback_db: f32,
    /// Every kind: the processed share of the output, 0–1.
    pub mix: f32,
}

/// An effect of `kind` with every field at its default (Rust
/// `FilterEffect::default()` and the others).
#[unsafe(no_mangle)]
pub extern "C" fn ponko_bus_effect_default(kind: PonkoBusEffectKind) -> PonkoBusEffect {
    effect_to_c(match kind {
        PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => BusEffect::Filter(FilterEffect::default()),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => BusEffect::Reverb(ReverbEffect::default()),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => BusEffect::Compressor(CompressorEffect::default()),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => BusEffect::Delay(DelayEffect::default()),
    })
}

fn mode_to_c(m: FilterMode) -> PonkoFilterMode {
    match m {
        FilterMode::BandPass => PonkoFilterMode::PONKO_FILTER_BAND_PASS,
        FilterMode::HighPass => PonkoFilterMode::PONKO_FILTER_HIGH_PASS,
        FilterMode::Notch => PonkoFilterMode::PONKO_FILTER_NOTCH,
        _ => PonkoFilterMode::PONKO_FILTER_LOW_PASS,
    }
}
fn mode_from_c(m: PonkoFilterMode) -> FilterMode {
    match m {
        PonkoFilterMode::PONKO_FILTER_LOW_PASS => FilterMode::LowPass,
        PonkoFilterMode::PONKO_FILTER_BAND_PASS => FilterMode::BandPass,
        PonkoFilterMode::PONKO_FILTER_HIGH_PASS => FilterMode::HighPass,
        PonkoFilterMode::PONKO_FILTER_NOTCH => FilterMode::Notch,
    }
}

fn effect_to_c(e: BusEffect) -> PonkoBusEffect {
    // Every field starts at its kind's default, so a C struct copied from
    // one kind to another still reads sensibly.
    let (f, r, c, d) = (
        FilterEffect::default(),
        ReverbEffect::default(),
        CompressorEffect::default(),
        DelayEffect::default(),
    );
    let mut out = PonkoBusEffect {
        kind: PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER,
        mode: mode_to_c(f.mode),
        cutoff_hz: f.cutoff_hz,
        resonance: f.resonance,
        feedback: r.feedback,
        damping: r.damping,
        stereo_width: r.stereo_width,
        threshold_db: c.threshold_db,
        ratio: c.ratio,
        attack: c.attack,
        release: c.release,
        makeup_db: c.makeup_db,
        delay: d.delay,
        feedback_db: d.feedback_db,
        mix: f.mix,
    };
    match e {
        BusEffect::Filter(f) => {
            out.mode = mode_to_c(f.mode);
            out.cutoff_hz = f.cutoff_hz;
            out.resonance = f.resonance;
            out.mix = f.mix;
        }
        BusEffect::Reverb(r) => {
            out.kind = PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB;
            out.feedback = r.feedback;
            out.damping = r.damping;
            out.stereo_width = r.stereo_width;
            out.mix = r.mix;
        }
        BusEffect::Compressor(c) => {
            out.kind = PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR;
            out.threshold_db = c.threshold_db;
            out.ratio = c.ratio;
            out.attack = c.attack;
            out.release = c.release;
            out.makeup_db = c.makeup_db;
            out.mix = c.mix;
        }
        BusEffect::Delay(d) => {
            out.kind = PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY;
            out.delay = d.delay;
            out.feedback_db = d.feedback_db;
            out.mix = d.mix;
        }
        _ => {}
    }
    out
}
fn effect_from_c(e: &PonkoBusEffect) -> BusEffect {
    match e.kind {
        PonkoBusEffectKind::PONKO_BUS_EFFECT_FILTER => BusEffect::Filter(FilterEffect {
            mode: mode_from_c(e.mode),
            cutoff_hz: e.cutoff_hz,
            resonance: e.resonance,
            mix: e.mix,
        }),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_REVERB => BusEffect::Reverb(ReverbEffect {
            feedback: e.feedback,
            damping: e.damping,
            stereo_width: e.stereo_width,
            mix: e.mix,
        }),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_COMPRESSOR => BusEffect::Compressor(CompressorEffect {
            threshold_db: e.threshold_db,
            ratio: e.ratio,
            attack: e.attack,
            release: e.release,
            makeup_db: e.makeup_db,
            mix: e.mix,
        }),
        PonkoBusEffectKind::PONKO_BUS_EFFECT_DELAY => BusEffect::Delay(DelayEffect {
            delay: e.delay,
            feedback_db: e.feedback_db,
            mix: e.mix,
        }),
    }
}

/// Sidechain ducking (Rust `Ducking`): while `key` is louder than
/// `threshold`, the bus it is set on drops to `gain`, over `attack`
/// seconds, and comes back over `release`. Start from
/// `ponko_ducking_default(key)`.
/// ```
/// let d = ponko_ffi::ponko_ducking_default(Default::default());
/// assert_eq!((d.gain, d.release), (0.3, 0.5));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoDucking {
    /// The bus whose sound ducks.
    pub key: PonkoAudioBus,
    /// Linear gain while ducked, 0–1.
    pub gain: f32,
    /// Peak level of `key` (linear) above which it ducks.
    pub threshold: f32,
    /// Seconds down to `gain`.
    pub attack: f32,
    /// Seconds back up.
    pub release: f32,
}

/// Ducking by `key` with the default amounts and times (Rust `Ducking::new`).
#[unsafe(no_mangle)]
pub extern "C" fn ponko_ducking_default(key: PonkoAudioBus) -> PonkoDucking {
    ducking_to_c(Ducking::new(bus_from_c(key)))
}
fn ducking_to_c(d: Ducking) -> PonkoDucking {
    PonkoDucking {
        key: bus_to_c(d.key),
        gain: d.gain,
        threshold: d.threshold,
        attack: d.attack,
        release: d.release,
    }
}
fn ducking_from_c(d: &PonkoDucking) -> Ducking {
    Ducking {
        key: bus_from_c(d.key),
        gain: d.gain,
        threshold: d.threshold,
        attack: d.attack,
        release: d.release,
    }
}

/// New settings for effect `index` of a bus's chain, in a snapshot (Rust
/// `EffectOverride`). It must be the same kind as the effect there.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoEffectOverride {
    /// Position in the chain, from 0.
    pub index: usize,
    /// The settings to blend to.
    pub effect: PonkoBusEffect,
}

/// What a snapshot changes on one bus (Rust `BusSnapshot`).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoBusSnapshot {
    /// The bus.
    pub bus: PonkoAudioBus,
    /// Whether `volume` applies; false keeps the bus's own.
    pub has_volume: bool,
    /// Its volume while the snapshot is on.
    pub volume: f32,
    /// `effect_count` effect settings, or null when the count is 0.
    pub effects: *const PonkoEffectOverride,
    /// How many `effects` there are.
    pub effect_count: usize,
}

/// `count` items at `items`, which may be null only when `count` is 0.
unsafe fn items<'a, T>(items: *const T, count: usize) -> Result<&'a [T], Failure> {
    if count == 0 {
        return Ok(&[]);
    }
    if items.is_null() {
        return Err(invalid("array is null"));
    }
    // SAFETY: the caller supplies `count` readable items.
    Ok(unsafe { std::slice::from_raw_parts(items, count) })
}

/// Copies `list` into `buffer` (up to `capacity`) and its length into
/// `out_count`, so a caller can ask for the count first with capacity 0.
unsafe fn write_list<T: Copy>(
    buffer: *mut T,
    capacity: usize,
    out_count: *mut usize,
    list: &[T],
) -> Result<(), Failure> {
    if out_count.is_null() || (buffer.is_null() && capacity != 0) {
        return Err(invalid("output is null"));
    }
    // SAFETY: checked pointers; the caller supplies `capacity` writable items.
    unsafe {
        out_count.write(list.len());
        for (i, item) in list.iter().take(capacity).enumerate() {
            buffer.add(i).write(*item);
        }
    }
    Ok(())
}

/// Puts `bus` under `parent` (null: straight to the output); see Rust
/// `Audio::set_bus_parent`. A loop is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_bus_parent(std::ptr::null_mut(), Default::default(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `parent` null or readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_bus_parent(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    parent: *const PonkoAudioBus,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let parent = parent.as_ref().map(|p| bus_from_c(*p));
            c.audio.set_bus_parent(bus_from_c(bus), parent).map_err(invalid_owned)
        })
    })
}

/// Writes the bus `bus` is under to `out_parent` and whether it has one to
/// `out_has_parent`.
/// ```
/// let (mut p, mut has) = (Default::default(), false);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_parent(std::ptr::null_mut(), Default::default(), &mut p, &mut has) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_parent(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_parent: *mut PonkoAudioBus,
    out_has_parent: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let parent = c.audio.bus_parent(bus_from_c(bus));
            out(out_parent, bus_to_c(parent.unwrap_or_default()))?;
            out(out_has_parent, parent.is_some())
        })
    })
}

/// The volume set on `bus`, before snapshots, parents and mutes (Rust
/// `Audio::bus_volume`).
/// ```
/// let mut v = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_volume(std::ptr::null_mut(), Default::default(), &mut v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_volume` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_volume(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_volume: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_volume, c.audio.bus_volume(bus_from_c(bus)))) })
}

/// The gain `bus` is heard at now: snapshots, parents and mutes included,
/// ducking not (Rust `Audio::bus_gain`).
/// ```
/// let mut g = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_gain(std::ptr::null_mut(), Default::default(), &mut g) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_gain` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_gain(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_gain: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_gain, c.audio.bus_gain(bus_from_c(bus)))) })
}

/// Whether `bus` is muted (Rust `Audio::bus_muted`).
/// ```
/// let mut m = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_muted(std::ptr::null_mut(), Default::default(), &mut m) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_muted` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_muted(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    out_muted: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_muted, c.audio.bus_muted(bus_from_c(bus)))) })
}

/// Replaces `bus`'s effect chain with `count` effects (Rust
/// `Audio::set_bus_effects`); 0 empties it.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_bus_effects(std::ptr::null_mut(), Default::default(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `effects` holds `count` readable items (null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_bus_effects(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    effects: *const PonkoBusEffect,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let effects: Vec<BusEffect> = items(effects, count)?.iter().map(effect_from_c).collect();
            c.audio.set_bus_effects(bus_from_c(bus), &effects);
            Ok(())
        })
    })
}

/// Writes `bus`'s chain as set (clamped) into `buffer`, at most `capacity`
/// effects, and how many it has to `out_count` (call with capacity 0 to
/// learn the count).
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_effects(std::ptr::null_mut(), Default::default(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` writable items (null if 0);
/// `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_effects(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    buffer: *mut PonkoBusEffect,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let list: Vec<PonkoBusEffect> = c
                .audio
                .bus_effects(bus_from_c(bus))
                .iter()
                .map(|e| effect_to_c(*e))
                .collect();
            write_list(buffer, capacity, out_count, &list)
        })
    })
}

/// Replaces the buses that duck `bus` (Rust `Audio::set_bus_ducking`); 0
/// turns ducking off. More than four is `PONKO_INVALID_ARGUMENT` (kind
/// limit), as is a bus ducking itself; nothing changes then.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_audio_bus_ducking(std::ptr::null_mut(), Default::default(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `ducking` holds `count` readable items (null if 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_audio_bus_ducking(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    ducking: *const PonkoDucking,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let ducking: Vec<Ducking> = items(ducking, count)?.iter().map(ducking_from_c).collect();
            c.audio
                .set_bus_ducking(bus_from_c(bus), &ducking)
                .map_err(invalid_owned)
        })
    })
}

/// Writes what ducks `bus` into `buffer`, as `ponko_ctx_audio_bus_effects` does.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_bus_ducking(std::ptr::null_mut(), Default::default(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// As `ponko_ctx_audio_bus_effects`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_bus_ducking(
    ctx: *mut PonkoCtx,
    bus: PonkoAudioBus,
    buffer: *mut PonkoDucking,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let list: Vec<PonkoDucking> = c
                .audio
                .bus_ducking(bus_from_c(bus))
                .iter()
                .map(|d| ducking_to_c(*d))
                .collect();
            write_list(buffer, capacity, out_count, &list)
        })
    })
}

/// Adds snapshot `name` (or replaces the one of that name) from `count`
/// bus settings (Rust `Audio::add_snapshot`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_add_audio_snapshot(std::ptr::null_mut(), c"x".as_ptr(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated; `buses` holds `count`
/// readable items (null if 0), each with `effect_count` readable effects.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_add_audio_snapshot(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    buses: *const PonkoBusSnapshot,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            let mut snapshot = MixerSnapshot::default();
            for b in items(buses, count)? {
                snapshot.buses.push(BusSnapshot {
                    bus: bus_from_c(b.bus),
                    volume: b.has_volume.then_some(b.volume),
                    effects: items(b.effects, b.effect_count)?
                        .iter()
                        .map(|o| EffectOverride {
                            index: o.index,
                            effect: effect_from_c(&o.effect),
                        })
                        .collect(),
                });
            }
            c.audio.add_snapshot(name, snapshot);
            Ok(())
        })
    })
}

/// Starts blending to snapshot `name` over `fade` seconds (Rust
/// `Audio::start_snapshot`). An unknown name is `PONKO_NOT_FOUND`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_start_audio_snapshot(std::ptr::null_mut(), c"x".as_ptr(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_start_audio_snapshot(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    fade: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if c.audio.start_snapshot(text(name)?, fade) {
                Ok(())
            } else {
                Err(error(PonkoStatus::PONKO_NOT_FOUND, "no snapshot of that name"))
            }
        })
    })
}

/// Blends snapshot `name` back out over `fade` seconds (Rust
/// `Audio::stop_snapshot`); nothing if it isn't on.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_stop_audio_snapshot(std::ptr::null_mut(), c"x".as_ptr(), 1.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_stop_audio_snapshot(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    fade: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.audio.stop_snapshot(text(name)?, fade);
            Ok(())
        })
    })
}

/// How much of snapshot `name` is heard, 0–1 (Rust `Audio::snapshot_weight`).
/// ```
/// let mut w = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_snapshot_weight(std::ptr::null_mut(), c"x".as_ptr(), &mut w) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated; `out_weight` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_snapshot_weight(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    out_weight: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_weight, c.audio.snapshot_weight(text(name)?))) })
}

/// Loads the mixer from a JSON file in the assets (Rust
/// `Audio::try_load_mixer`); it hot reloads. A missing file is
/// `PONKO_NOT_FOUND`, a wrong one `PONKO_INVALID_ARGUMENT`; see
/// `ponko_last_error_kind`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_audio_mixer(std::ptr::null_mut(), c"m.json".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_audio_mixer(ctx: *mut PonkoCtx, path: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.audio.try_load_mixer(text(path)?).map_err(engine_error)) })
}

/// Applies a mixer definition held in `json`, in the mixer file's format
/// (Rust `Audio::apply_mixer_json`). On an error nothing changes.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_apply_audio_mixer_json(std::ptr::null_mut(), c"{}".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `json` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_apply_audio_mixer_json(ctx: *mut PonkoCtx, json: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.audio.apply_mixer_json(text(json)?).map_err(invalid_owned)) })
}

/// Writes the mixer state to save with the settings (Rust
/// `Audio::mixer_state`, as JSON) into `buffer`: the length (without the
/// NUL) goes to `out_len`, the text only if it fits.
/// ```
/// let mut len = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_audio_mixer_state(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut len) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes (null if 0);
/// `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_audio_mixer_state(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let json = serde_json::to_string(&c.audio.mixer_state()).map_err(|e| invalid_owned(e.to_string()))?;
            write_text(buffer, capacity, out_len, &json)
        })
    })
}

/// Restores a mixer state saved from `ponko_ctx_audio_mixer_state` (Rust
/// `Audio::restore_mixer_state`). JSON that doesn't parse changes nothing.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_restore_audio_mixer_state(std::ptr::null_mut(), c"{}".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `json` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_restore_audio_mixer_state(ctx: *mut PonkoCtx, json: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let state: MixerState =
                serde_json::from_str(text(json)?).map_err(|e| invalid_owned(ponko::prelude::Error::from(e)))?;
            c.audio.restore_mixer_state(&state);
            Ok(())
        })
    })
}
