//! The Kira side of the mixer: one track per bus, nested as the buses are,
//! each running the bus's effect chain, then the ducker and the meter.

use super::dsp::{BusLevel, DuckShared, DuckSlot, DuckerBuilder, MeterBuilder, PitchShared, PitchShiftBuilder};
use super::mixer::{BusRegistry, volume_db};
use super::{AudioBus, BusEffect, Ducking, FilterMode};
use kira::clock::{ClockHandle, ClockSpeed};
use kira::effect::{
    compressor::{CompressorBuilder, CompressorHandle},
    delay::{DelayBuilder, DelayHandle},
    filter::{self, FilterBuilder, FilterHandle},
    reverb::{ReverbBuilder, ReverbHandle},
};
use kira::track::{SendTrackBuilder, SendTrackHandle, TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Mix, ResourceLimitReached, Tween};
use std::sync::Arc;
use std::time::Duration;

/// The output: a device, or a buffer that offline mixing and tests read.
pub(super) enum Manager {
    Device(AudioManager<DefaultBackend>),
    Buffer(AudioManager<super::dsp::buffer::BufferBackend>),
}
impl Manager {
    /// A manager mixing into memory; `None` if Kira can't make one.
    pub(super) fn buffer(sample_rate: u32) -> Option<(Self, super::dsp::buffer::Output)> {
        use super::dsp::buffer::{BufferSettings, Output};
        let renderer = super::dsp::buffer::Shared::default();
        let settings = AudioManagerSettings {
            backend_settings: BufferSettings {
                sample_rate,
                renderer: renderer.clone(),
            },
            capacities: Default::default(),
            main_track_builder: Default::default(),
            internal_buffer_size: 128,
        };
        let manager = AudioManager::new(settings).ok()?;
        Some((Self::Buffer(manager), Output { renderer, sample_rate }))
    }
    pub(super) fn add_sub_track(&mut self, builder: TrackBuilder) -> Result<TrackHandle, ResourceLimitReached> {
        match self {
            Self::Device(m) => m.add_sub_track(builder),
            Self::Buffer(m) => m.add_sub_track(builder),
        }
    }
    pub(super) fn add_clock(&mut self, speed: ClockSpeed) -> Result<ClockHandle, ResourceLimitReached> {
        match self {
            Self::Device(m) => m.add_clock(speed),
            Self::Buffer(m) => m.add_clock(speed),
        }
    }
    pub(super) fn add_send_track(
        &mut self,
        builder: SendTrackBuilder,
    ) -> Result<SendTrackHandle, ResourceLimitReached> {
        match self {
            Self::Device(m) => m.add_send_track(builder),
            Self::Buffer(m) => m.add_send_track(builder),
        }
    }
}

enum EffectHandle {
    Filter(FilterHandle),
    Reverb(ReverbHandle),
    Compressor(CompressorHandle),
    Delay(DelayHandle),
    PitchShift(Arc<PitchShared>),
}

fn kira_mode(mode: FilterMode) -> filter::FilterMode {
    match mode {
        FilterMode::LowPass => filter::FilterMode::LowPass,
        FilterMode::BandPass => filter::FilterMode::BandPass,
        FilterMode::HighPass => filter::FilterMode::HighPass,
        FilterMode::Notch => filter::FilterMode::Notch,
    }
}
fn secs(s: f32) -> Duration {
    Duration::from_secs_f32(s.max(0.0))
}

fn add_effect(builder: &mut TrackBuilder, effect: &BusEffect) -> EffectHandle {
    match *effect {
        BusEffect::Filter(f) => EffectHandle::Filter(
            builder.add_effect(
                FilterBuilder::new()
                    .mode(kira_mode(f.mode))
                    .cutoff(f64::from(f.cutoff_hz))
                    .resonance(f64::from(f.resonance))
                    .mix(Mix(f.mix)),
            ),
        ),
        BusEffect::Reverb(r) => EffectHandle::Reverb(
            builder.add_effect(
                ReverbBuilder::new()
                    .feedback(f64::from(r.feedback))
                    .damping(f64::from(r.damping))
                    .stereo_width(f64::from(r.stereo_width))
                    .mix(Mix(r.mix)),
            ),
        ),
        BusEffect::Compressor(c) => EffectHandle::Compressor(
            builder.add_effect(
                CompressorBuilder::new()
                    .threshold(f64::from(c.threshold_db))
                    .ratio(f64::from(c.ratio))
                    .attack_duration(secs(c.attack))
                    .release_duration(secs(c.release))
                    .makeup_gain(Decibels(c.makeup_db))
                    .mix(Mix(c.mix)),
            ),
        ),
        BusEffect::Delay(d) => EffectHandle::Delay(
            builder.add_effect(
                DelayBuilder::new()
                    .delay_time(secs(d.delay))
                    .feedback(Decibels(d.feedback_db))
                    .mix(Mix(d.mix)),
            ),
        ),
        BusEffect::PitchShift(p) => {
            let shared = PitchShared::new(p.ratio, p.mix);
            builder.add_effect(PitchShiftBuilder(shared.clone()));
            EffectHandle::PitchShift(shared)
        }
    }
}

/// Sends an effect's new settings (same kind and shape as when built).
fn set_effect(handle: &mut EffectHandle, effect: &BusEffect) {
    let t = Tween::default();
    match (handle, *effect) {
        (EffectHandle::Filter(h), BusEffect::Filter(f)) => {
            h.set_mode(kira_mode(f.mode));
            h.set_cutoff(f64::from(f.cutoff_hz), t);
            h.set_resonance(f64::from(f.resonance), t);
            h.set_mix(Mix(f.mix), t);
        }
        (EffectHandle::Reverb(h), BusEffect::Reverb(r)) => {
            h.set_feedback(f64::from(r.feedback), t);
            h.set_damping(f64::from(r.damping), t);
            h.set_stereo_width(f64::from(r.stereo_width), t);
            h.set_mix(Mix(r.mix), t);
        }
        (EffectHandle::Compressor(h), BusEffect::Compressor(c)) => {
            h.set_threshold(f64::from(c.threshold_db), t);
            h.set_ratio(f64::from(c.ratio), t);
            h.set_attack_duration(secs(c.attack), t);
            h.set_release_duration(secs(c.release), t);
            h.set_makeup_gain(Decibels(c.makeup_db), t);
            h.set_mix(Mix(c.mix), t);
        }
        (EffectHandle::Delay(h), BusEffect::Delay(d)) => {
            h.set_feedback(Decibels(d.feedback_db), t);
            h.set_mix(Mix(d.mix), t);
        }
        (EffectHandle::PitchShift(shared), BusEffect::PitchShift(p)) => shared.set(p.ratio, p.mix),
        _ => {}
    }
}

struct BusTrack {
    track: TrackHandle,
    effects: Vec<EffectHandle>,
    /// The bus's `shape` it was built for.
    shape: u32,
    /// What was last sent: the own gain and the effects' settings.
    gain: f32,
    sent: Vec<BusEffect>,
    /// Whether it was last paused.
    paused: bool,
}

/// Per bus: its track once something played on it, and what its meter and
/// ducker share with the audio thread (kept across rebuilds).
#[derive(Default)]
struct Slot {
    track: Option<BusTrack>,
    level: Arc<BusLevel>,
    duck: Arc<DuckShared>,
    /// What the ducker was last given.
    ducking: Vec<Ducking>,
}

#[derive(Default)]
pub(super) struct BusTracks {
    slots: Vec<Slot>,
}
impl BusTracks {
    fn grow(&mut self, buses: &BusRegistry) {
        while self.slots.len() < buses.entries.len() {
            self.slots.push(Slot::default());
        }
    }

    pub(super) fn has_track(&self, bus: AudioBus) -> bool {
        self.slots.get(bus.0).is_some_and(|s| s.track.is_some())
    }

    pub(super) fn track(&mut self, bus: AudioBus) -> Option<&mut TrackHandle> {
        self.slots.get_mut(bus.0)?.track.as_mut().map(|t| &mut t.track)
    }

    /// Builds `bus`'s track, and its parents' first, if they aren't yet.
    pub(super) fn ensure(&mut self, manager: &mut Manager, buses: &BusRegistry, bus: AudioBus) -> Option<()> {
        self.grow(buses);
        self.ensure_depth(manager, buses, bus, buses.entries.len())
    }

    fn ensure_depth(&mut self, manager: &mut Manager, buses: &BusRegistry, bus: AudioBus, depth: usize) -> Option<()> {
        let state = buses.get(bus)?;
        if self.slots[bus.0].track.is_some() {
            return Some(());
        }
        // Room for every voice the limits allow, and the child buses.
        let mut builder = TrackBuilder::new()
            .volume(volume_db(state.gain()))
            .sub_track_capacity(super::MAX_VOICE_LIMIT as usize + 32);
        let effects = state
            .heard_effects
            .iter()
            .map(|e| add_effect(&mut builder, e))
            .collect();
        builder.add_effect(DuckerBuilder(self.slots[bus.0].duck.clone()));
        builder.add_effect(MeterBuilder(self.slots[bus.0].level.clone()));
        let track = match state.parent {
            Some(parent) if depth > 0 => {
                self.ensure_depth(manager, buses, parent, depth - 1)?;
                self.slots[parent.0].track.as_mut()?.track.add_sub_track(builder).ok()?
            }
            _ => manager.add_sub_track(builder).ok()?,
        };
        let mut track = track;
        if state.own_pause() {
            track.pause(Tween::default());
        }
        self.slots[bus.0].track = Some(BusTrack {
            track,
            effects,
            shape: state.shape,
            gain: state.gain(),
            sent: state.heard_effects.clone(),
            paused: state.own_pause(),
        });
        Some(())
    }

    /// Drops the tracks of buses whose parent or chain changed since they
    /// were built, and of every bus under one. Whether any went: their
    /// voices have to start again.
    pub(super) fn drop_stale(&mut self, buses: &BusRegistry) -> bool {
        self.grow(buses);
        let is_stale = |(slot, state): (&Slot, &super::mixer::BusState)| {
            slot.track.as_ref().is_some_and(|t| t.shape != state.shape)
        };
        // Checked first without allocating: this runs on every mixer change.
        if !self.slots.iter().zip(&buses.entries).any(is_stale) {
            return false;
        }
        let stale: Vec<bool> = self.slots.iter().zip(&buses.entries).map(is_stale).collect();
        for (b, slot) in self.slots.iter_mut().enumerate().take(stale.len()) {
            let mut at = Some(AudioBus(b));
            for _ in 0..=stale.len() {
                let Some(bus) = at else {
                    break;
                };
                if stale[bus.0] {
                    slot.track = None;
                    break;
                }
                at = buses.get(bus).and_then(|s| s.parent);
            }
        }
        true
    }

    /// Sends what changed: each bus's gain and effect settings, what its
    /// meter scales by, and who ducks it.
    pub(super) fn push(&mut self, buses: &BusRegistry) {
        self.grow(buses);
        for (b, state) in buses.entries.iter().enumerate() {
            let slot = &mut self.slots[b];
            slot.level.set_fader(buses.chain_gain(AudioBus(b)));
            if let Some(t) = &mut slot.track {
                let gain = state.gain();
                if gain != t.gain {
                    t.track.set_volume(volume_db(gain), Tween::default());
                    t.gain = gain;
                }
                if state.own_pause() != t.paused {
                    t.paused = state.own_pause();
                    if t.paused {
                        t.track.pause(Tween::default());
                    } else {
                        t.track.resume(Tween::default());
                    }
                }
                for ((handle, sent), now) in t.effects.iter_mut().zip(&mut t.sent).zip(&state.heard_effects) {
                    if sent != now {
                        set_effect(handle, now);
                        *sent = *now;
                    }
                }
            }
            if slot.ducking != state.ducking {
                slot.ducking.clone_from(&state.ducking);
                let mut keys: [Option<DuckSlot>; Ducking::MAX_KEYS] = Default::default();
                for (k, d) in state.ducking.iter().enumerate().take(Ducking::MAX_KEYS) {
                    keys[k] = Some(DuckSlot {
                        level: self.level(d.key),
                        gain: d.gain,
                        threshold: d.threshold,
                        attack: d.attack,
                        release: d.release,
                    });
                }
                let slot = &mut self.slots[b];
                *slot.duck.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = keys;
            }
        }
    }

    fn level(&self, bus: AudioBus) -> Arc<BusLevel> {
        self.slots[bus.0].level.clone()
    }
}
