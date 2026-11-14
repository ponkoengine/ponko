//! Effects on a whole bus ([`BusEffect`]) and sidechain ducking
//! ([`Ducking`]): what [`Audio::set_bus_effects`](super::Audio::set_bus_effects)
//! and [`Audio::set_bus_ducking`](super::Audio::set_bus_ducking) take, and
//! what a mixer file's `effects` and `ducking` lists hold.

use super::AudioBus;
use super::mixer::finite;
use serde::{Deserialize, Serialize};

/// One effect in a bus's chain. A bus runs its chain in order on the mix of
/// everything playing on it (its sounds and its child buses), before its
/// volume. In a mixer file each is an object named after its kind, holding
/// the fields to change: `{"filter": {"cutoff_hz": 800}}`.
///
/// ```
/// use ponko::audio::{BusEffect, FilterEffect, FilterMode};
/// let muffled = BusEffect::Filter(FilterEffect { mode: FilterMode::LowPass, cutoff_hz: 800.0, ..Default::default() });
/// assert_eq!(muffled, serde_json::from_str(r#"{"filter": {"cutoff_hz": 800}}"#).unwrap());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BusEffect {
    /// A resonant filter: low-pass (muffled, underwater), high-pass (thin,
    /// a radio), band-pass or notch.
    Filter(FilterEffect),
    /// A room: the bus's own reverb, mixed with the dry sound.
    Reverb(ReverbEffect),
    /// Evens out loud and quiet parts.
    Compressor(CompressorEffect),
    /// Echoes. Its delay time is fixed when the bus is built.
    Delay(DelayEffect),
}

/// Which frequencies a [`FilterEffect`] lets through.
///
/// ```
/// use ponko::audio::FilterMode;
/// assert_eq!(FilterMode::default(), FilterMode::LowPass);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FilterMode {
    /// Below the cutoff (the default): muffles.
    #[default]
    LowPass,
    /// Around the cutoff.
    BandPass,
    /// Above the cutoff: thins.
    HighPass,
    /// All but the cutoff.
    Notch,
}

/// A bus filter. Out-of-range values are clamped when applied: cutoff to
/// 20–20,000 Hz, resonance to 0–1, mix to 0–1.
///
/// ```
/// use ponko::audio::FilterEffect;
/// assert_eq!(FilterEffect::default().cutoff_hz, 20_000.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterEffect {
    /// Which frequencies pass. Default low-pass.
    pub mode: FilterMode,
    /// Cutoff in Hz. Default 20,000 (a low-pass that changes nothing, for a
    /// snapshot to lower).
    pub cutoff_hz: f32,
    /// Emphasis at the cutoff, 0–1. Default 0.
    pub resonance: f32,
    /// How much of the filtered sound replaces the dry one, 0–1. Default 1.
    pub mix: f32,
}
impl Default for FilterEffect {
    fn default() -> Self {
        Self {
            mode: FilterMode::LowPass,
            cutoff_hz: 20_000.0,
            resonance: 0.0,
            mix: 1.0,
        }
    }
}

/// A bus reverb. Values are clamped to 0–1 when applied.
///
/// ```
/// use ponko::audio::ReverbEffect;
/// assert_eq!(ReverbEffect::default().mix, 0.5);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReverbEffect {
    /// Room size: how long it rings, 0–1. Default 0.9.
    pub feedback: f32,
    /// How fast the highs fade from the tail, 0–1. Default 0.1.
    pub damping: f32,
    /// 0 mono to 1 full stereo. Default 1.
    pub stereo_width: f32,
    /// Wet share of the output, 0–1. Default 0.5.
    pub mix: f32,
}
impl Default for ReverbEffect {
    fn default() -> Self {
        Self {
            feedback: 0.9,
            damping: 0.1,
            stereo_width: 1.0,
            mix: 0.5,
        }
    }
}

/// A bus compressor. Ratio is at least 1, times at least 1 ms, mix 0–1.
///
/// ```
/// use ponko::audio::CompressorEffect;
/// assert_eq!(CompressorEffect::default().ratio, 4.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompressorEffect {
    /// Level in dB above which it compresses. Default -18.
    pub threshold_db: f32,
    /// 4 turns 4 dB above the threshold into 1 dB. Default 4.
    pub ratio: f32,
    /// Seconds to clamp down once over the threshold. Default 0.01.
    pub attack: f32,
    /// Seconds to let go below it. Default 0.1.
    pub release: f32,
    /// Gain in dB after compressing. Default 0.
    pub makeup_db: f32,
    /// Compressed share of the output, 0–1. Default 1.
    pub mix: f32,
}
impl Default for CompressorEffect {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack: 0.01,
            release: 0.1,
            makeup_db: 0.0,
            mix: 1.0,
        }
    }
}

/// A bus echo. Delay 1 ms–5 s (fixed when the bus is built: changing it
/// rebuilds the bus), feedback at most 0 dB, mix 0–1.
///
/// ```
/// use ponko::audio::DelayEffect;
/// assert_eq!(DelayEffect::default().delay, 0.25);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayEffect {
    /// Seconds between echoes. Default 0.25.
    pub delay: f32,
    /// Each echo's level against the one before, in dB (the first echo's
    /// against the dry sound). Default -6.
    pub feedback_db: f32,
    /// Echo share of the output, 0–1. Default 0.3.
    pub mix: f32,
}
impl Default for DelayEffect {
    fn default() -> Self {
        Self {
            delay: 0.25,
            feedback_db: -6.0,
            mix: 0.3,
        }
    }
}

/// Sidechain ducking: while `key` (dialogue, say) is louder than
/// `threshold`, the bus it is set on (music) drops to `gain`, ramping down
/// over `attack` seconds and back up over `release` seconds. It follows the
/// level `key` is really heard at (after its effects, and its own and its
/// parents' volumes and mutes), on the audio thread, so pauses between
/// lines let the music back up. A bus takes up to [`Ducking::MAX_KEYS`] keys; while several are
/// loud the lowest gain wins.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::audio::Ducking;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let (music, dialogue) = (ctx.audio.bus("music"), ctx.audio.bus("dialogue"));
///     ctx.audio.set_bus_ducking(music, &[Ducking { gain: 0.3, ..Ducking::new(dialogue) }]).unwrap();
///     assert_eq!(ctx.audio.bus_ducking(music)[0].key, dialogue);
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ducking {
    /// The bus whose sound ducks this one.
    pub key: AudioBus,
    /// Linear gain while ducked, 0–1. Default 0.3 (about -10 dB).
    pub gain: f32,
    /// Peak level of `key` (linear, 0–1) above which it ducks. Default
    /// 0.01 (-40 dB): anything audible.
    pub threshold: f32,
    /// Seconds to go from full volume down to `gain`. Default 0.05.
    pub attack: f32,
    /// Seconds to come back up once `key` is quiet. Default 0.5.
    pub release: f32,
}
impl Ducking {
    /// How many keys one bus can be ducked by.
    pub const MAX_KEYS: usize = 4;

    /// Ducking by `key` with the default amounts and times.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let duck = ponko::audio::Ducking::new(AudioBus::default());
    /// assert_eq!(duck.gain, 0.3);
    /// ```
    #[must_use]
    pub fn new(key: AudioBus) -> Self {
        Self {
            key,
            gain: 0.3,
            threshold: 0.01,
            attack: 0.05,
            release: 0.5,
        }
    }

    pub(super) fn validated(self) -> Self {
        Self {
            key: self.key,
            gain: finite(self.gain, 0.3).clamp(0.0, 1.0),
            threshold: finite(self.threshold, 0.01).max(0.0),
            attack: finite(self.attack, 0.05).max(0.0),
            release: finite(self.release, 0.5).max(0.0),
        }
    }
}

impl BusEffect {
    /// The same effect with every parameter in range (nonfinite values
    /// become the defaults).
    pub(super) fn validated(self) -> Self {
        match self {
            Self::Filter(f) => {
                let d = FilterEffect::default();
                Self::Filter(FilterEffect {
                    mode: f.mode,
                    cutoff_hz: finite(f.cutoff_hz, d.cutoff_hz).clamp(20.0, 20_000.0),
                    resonance: finite(f.resonance, d.resonance).clamp(0.0, 1.0),
                    mix: finite(f.mix, d.mix).clamp(0.0, 1.0),
                })
            }
            Self::Reverb(r) => {
                let d = ReverbEffect::default();
                Self::Reverb(ReverbEffect {
                    feedback: finite(r.feedback, d.feedback).clamp(0.0, 1.0),
                    damping: finite(r.damping, d.damping).clamp(0.0, 1.0),
                    stereo_width: finite(r.stereo_width, d.stereo_width).clamp(0.0, 1.0),
                    mix: finite(r.mix, d.mix).clamp(0.0, 1.0),
                })
            }
            Self::Compressor(c) => {
                let d = CompressorEffect::default();
                Self::Compressor(CompressorEffect {
                    threshold_db: finite(c.threshold_db, d.threshold_db).clamp(-100.0, 0.0),
                    ratio: finite(c.ratio, d.ratio).max(1.0),
                    attack: finite(c.attack, d.attack).max(0.001),
                    release: finite(c.release, d.release).max(0.001),
                    makeup_db: finite(c.makeup_db, d.makeup_db).clamp(-60.0, 60.0),
                    mix: finite(c.mix, d.mix).clamp(0.0, 1.0),
                })
            }
            Self::Delay(e) => {
                let d = DelayEffect::default();
                Self::Delay(DelayEffect {
                    delay: finite(e.delay, d.delay).clamp(0.001, 5.0),
                    feedback_db: finite(e.feedback_db, d.feedback_db).clamp(-60.0, 0.0),
                    mix: finite(e.mix, d.mix).clamp(0.0, 1.0),
                })
            }
        }
    }

    /// What a bus has to be rebuilt for when it changes: the kind, and a
    /// delay's time (Kira fixes it at build). Everything else is set live.
    pub(super) fn shape(&self) -> (u8, u32) {
        match self {
            Self::Filter(_) => (0, 0),
            Self::Reverb(_) => (1, 0),
            Self::Compressor(_) => (2, 0),
            Self::Delay(d) => (3, d.delay.to_bits()),
        }
    }

    /// `self` moved `t` (0–1) of the way to `to`. Frequencies move evenly in
    /// pitch, everything else linearly; the filter mode and the delay time
    /// switch at once. Different kinds don't blend: `self` is kept.
    pub(super) fn lerp(&self, to: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| lerp(a, b, t);
        let pitch = |a: f32, b: f32| match t {
            0.0 => a,
            1.0 => b,
            _ => (a.max(1.0).ln() + (b.max(1.0).ln() - a.max(1.0).ln()) * t).exp(),
        };
        match (self, to) {
            (Self::Filter(a), Self::Filter(b)) => Self::Filter(FilterEffect {
                mode: if t > 0.0 { b.mode } else { a.mode },
                cutoff_hz: pitch(a.cutoff_hz, b.cutoff_hz),
                resonance: mix(a.resonance, b.resonance),
                mix: mix(a.mix, b.mix),
            }),
            (Self::Reverb(a), Self::Reverb(b)) => Self::Reverb(ReverbEffect {
                feedback: mix(a.feedback, b.feedback),
                damping: mix(a.damping, b.damping),
                stereo_width: mix(a.stereo_width, b.stereo_width),
                mix: mix(a.mix, b.mix),
            }),
            (Self::Compressor(a), Self::Compressor(b)) => Self::Compressor(CompressorEffect {
                threshold_db: mix(a.threshold_db, b.threshold_db),
                ratio: mix(a.ratio, b.ratio),
                attack: mix(a.attack, b.attack),
                release: mix(a.release, b.release),
                makeup_db: mix(a.makeup_db, b.makeup_db),
                mix: mix(a.mix, b.mix),
            }),
            (Self::Delay(a), Self::Delay(b)) => Self::Delay(DelayEffect {
                delay: a.delay,
                feedback_db: mix(a.feedback_db, b.feedback_db),
                mix: mix(a.mix, b.mix),
            }),
            _ => *self,
        }
    }

    /// Whether `other` is the same kind of effect.
    pub(super) fn same_kind(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

/// `a` moved `t` of the way to `b`, exactly `a` at 0 and `b` at 1.
pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_effects_clamp_out_of_range_and_nonfinite_parameters() {
        let wild = BusEffect::Filter(FilterEffect {
            mode: FilterMode::HighPass,
            cutoff_hz: f32::NAN,
            resonance: 7.0,
            mix: -1.0,
        });
        assert_eq!(
            wild.validated(),
            BusEffect::Filter(FilterEffect {
                mode: FilterMode::HighPass,
                cutoff_hz: 20_000.0,
                resonance: 1.0,
                mix: 0.0
            })
        );
        let BusEffect::Delay(delay) = BusEffect::Delay(DelayEffect {
            delay: 60.0,
            feedback_db: 6.0,
            mix: 0.5,
        })
        .validated() else {
            unreachable!()
        };
        assert_eq!((delay.delay, delay.feedback_db), (5.0, 0.0));
        let BusEffect::Compressor(c) = BusEffect::Compressor(CompressorEffect {
            ratio: 0.5,
            ..Default::default()
        })
        .validated() else {
            unreachable!()
        };
        assert_eq!(c.ratio, 1.0);
    }

    #[test]
    fn filter_cutoff_blends_evenly_in_pitch() {
        let open = BusEffect::Filter(FilterEffect::default());
        let shut = BusEffect::Filter(FilterEffect {
            cutoff_hz: 200.0,
            ..Default::default()
        });
        let BusEffect::Filter(half) = open.lerp(&shut, 0.5) else {
            unreachable!()
        };
        // Halfway in pitch between 20 kHz and 200 Hz is 2 kHz.
        assert!((half.cutoff_hz - 2000.0).abs() < 1.0, "{}", half.cutoff_hz);
        assert_eq!(open.lerp(&shut, 0.0), open);
        assert_eq!(open.lerp(&shut, 1.0), shut);
    }

    #[test]
    fn different_effect_kinds_never_blend() {
        let filter = BusEffect::Filter(FilterEffect::default());
        let reverb = BusEffect::Reverb(ReverbEffect::default());
        assert_eq!(filter.lerp(&reverb, 0.7), filter);
        assert!(!filter.same_kind(&reverb));
    }

    #[test]
    fn only_kind_and_delay_time_change_a_bus_shape() {
        let a = BusEffect::Delay(DelayEffect::default());
        let b = BusEffect::Delay(DelayEffect {
            mix: 0.9,
            ..Default::default()
        });
        let c = BusEffect::Delay(DelayEffect {
            delay: 0.5,
            ..Default::default()
        });
        assert_eq!(a.shape(), b.shape());
        assert_ne!(a.shape(), c.shape());
    }

    #[test]
    fn ducking_validation_keeps_gain_in_range() {
        let d = Ducking {
            gain: 3.0,
            attack: -1.0,
            release: f32::NAN,
            ..Ducking::new(AudioBus::default())
        }
        .validated();
        assert_eq!((d.gain, d.attack, d.release), (1.0, 0.0, 0.5));
    }
}
