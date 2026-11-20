//! The two effects the engine adds to every bus track, on the audio
//! thread: a level meter (what a bus's sound ducks others by) and a ducker
//! (which follows the meters of the buses it is ducked by).

use kira::Frame;
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// A bus's level as its meter last measured it, and the gain the bus is
/// heard at (its volume and mutes, set by the game side), shared with the
/// audio thread.
pub(super) struct BusLevel {
    /// Peak of the last block, times `fader`.
    peak: AtomicU32,
    fader: AtomicU32,
}
impl Default for BusLevel {
    fn default() -> Self {
        Self {
            peak: AtomicU32::new(0),
            fader: AtomicU32::new(1.0f32.to_bits()),
        }
    }
}
impl BusLevel {
    pub(super) fn set_fader(&self, gain: f32) {
        self.fader.store(gain.to_bits(), Ordering::Relaxed);
    }
    pub(super) fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

/// Measures the peak of each block into a [`BusLevel`].
pub(super) struct MeterBuilder(pub(super) Arc<BusLevel>);
impl EffectBuilder for MeterBuilder {
    type Handle = ();
    fn build(self) -> (Box<dyn Effect>, ()) {
        (Box::new(Meter(self.0)), ())
    }
}
struct Meter(Arc<BusLevel>);
impl Effect for Meter {
    fn process(&mut self, input: &mut [Frame], _dt: f64, _info: &Info) {
        let peak = input.iter().fold(0.0f32, |m, f| m.max(f.left.abs()).max(f.right.abs()));
        let fader = f32::from_bits(self.0.fader.load(Ordering::Relaxed));
        self.0.peak.store((peak * fader).to_bits(), Ordering::Relaxed);
    }
}

/// One bus that ducks another: its meter and the amounts.
#[derive(Clone)]
pub(super) struct DuckSlot {
    pub(super) level: Arc<BusLevel>,
    pub(super) gain: f32,
    pub(super) threshold: f32,
    pub(super) attack: f32,
    pub(super) release: f32,
}

/// What ducks a bus, written by the game side under the lock and read by
/// the audio thread only with `try_lock`, so the audio thread never waits.
#[derive(Default)]
pub(super) struct DuckShared(pub(super) Mutex<[Option<DuckSlot>; super::Ducking::MAX_KEYS]>);

pub(super) struct DuckerBuilder(pub(super) Arc<DuckShared>);
impl EffectBuilder for DuckerBuilder {
    type Handle = ();
    fn build(self) -> (Box<dyn Effect>, ()) {
        (
            Box::new(Ducker {
                shared: self.0,
                envelope: [1.0; super::Ducking::MAX_KEYS],
            }),
            (),
        )
    }
}
struct Ducker {
    shared: Arc<DuckShared>,
    /// The gain each key holds the bus at now, 1 when it doesn't.
    envelope: [f32; super::Ducking::MAX_KEYS],
}
impl Effect for Ducker {
    fn process(&mut self, input: &mut [Frame], dt: f64, _info: &Info) {
        let dt = dt as f32;
        // Per key: where its envelope goes this block, and how fast.
        let mut target = [1.0f32; super::Ducking::MAX_KEYS];
        let mut down = [f32::INFINITY; super::Ducking::MAX_KEYS];
        let mut up = [f32::INFINITY; super::Ducking::MAX_KEYS];
        if let Ok(slots) = self.shared.0.try_lock() {
            for (k, slot) in slots.iter().enumerate() {
                let Some(slot) = slot else {
                    continue;
                };
                let depth = 1.0 - slot.gain;
                if slot.level.peak() > slot.threshold {
                    target[k] = slot.gain;
                }
                if slot.attack > 0.0 {
                    down[k] = depth * dt / slot.attack;
                }
                if slot.release > 0.0 {
                    up[k] = depth * dt / slot.release;
                }
            }
        } else {
            // The game is changing the keys: hold where they are.
            target = self.envelope;
        }
        if target.iter().chain(&self.envelope).all(|&g| g == 1.0) {
            return;
        }
        for frame in input {
            let mut gain = 1.0f32;
            for k in 0..super::Ducking::MAX_KEYS {
                let e = &mut self.envelope[k];
                if *e > target[k] {
                    *e = (*e - down[k]).max(target[k]);
                } else if *e < target[k] {
                    *e = (*e + up[k]).min(target[k]);
                }
                gain = gain.min(*e);
            }
            *frame *= gain;
        }
    }
}

/// A pitch shift's settings, written by the game side and read by the
/// audio thread without locking.
pub(super) struct PitchShared {
    ratio: AtomicU32,
    mix: AtomicU32,
}
impl PitchShared {
    pub(super) fn new(ratio: f32, mix: f32) -> Arc<Self> {
        Arc::new(Self {
            ratio: AtomicU32::new(ratio.to_bits()),
            mix: AtomicU32::new(mix.to_bits()),
        })
    }
    pub(super) fn set(&self, ratio: f32, mix: f32) {
        self.ratio.store(ratio.to_bits(), Ordering::Relaxed);
        self.mix.store(mix.to_bits(), Ordering::Relaxed);
    }
}

/// The window each read head crosses, in seconds.
const PITCH_WINDOW: f64 = 0.04;
/// The delay line holds this much sound, at the highest sample rate taken.
const PITCH_LINE: usize = 2 * 192_000 * 40 / 1000 + 4;

/// Shifts the pitch with a delay line read by two heads half a window
/// apart, each moving at `ratio` against the writing and faded in and out
/// (sin²) as it wraps, so their sum stays at full level.
pub(super) struct PitchShiftBuilder(pub(super) Arc<PitchShared>);
impl EffectBuilder for PitchShiftBuilder {
    type Handle = ();
    fn build(self) -> (Box<dyn Effect>, ()) {
        (
            Box::new(PitchShift {
                shared: self.0,
                line: vec![Frame::ZERO; PITCH_LINE].into_boxed_slice(),
                write: 0,
                phase: 0.0,
            }),
            (),
        )
    }
}
struct PitchShift {
    shared: Arc<PitchShared>,
    line: Box<[Frame]>,
    write: usize,
    /// Where the first head is in its window, 0–1.
    phase: f64,
}
impl PitchShift {
    /// The sound `delay` frames (fractional) behind the writing.
    fn read(&self, delay: f64) -> Frame {
        let n = self.line.len();
        let at = self.write as f64 - delay;
        let at = at.rem_euclid(n as f64);
        let i = at.floor() as usize % n;
        let j = (i + 1) % n;
        let t = (at - at.floor()) as f32;
        self.line[i] * (1.0 - t) + self.line[j] * t
    }
}
impl Effect for PitchShift {
    fn process(&mut self, input: &mut [Frame], dt: f64, _info: &Info) {
        let ratio = f64::from(f32::from_bits(self.shared.ratio.load(Ordering::Relaxed)));
        let mix = f32::from_bits(self.shared.mix.load(Ordering::Relaxed));
        // A rate the line can't hold a window of is left as it is.
        let window = (PITCH_WINDOW / dt.max(1e-9)).min((self.line.len() / 2) as f64);
        let step = (1.0 - ratio) / window.max(1.0);
        let n = self.line.len();
        for frame in input {
            self.line[self.write] = *frame;
            if mix > 0.0 && ratio != 1.0 {
                let other = (self.phase + 0.5).fract();
                let fade = |p: f64| (std::f64::consts::PI * p).sin().powi(2) as f32;
                let wet = self.read(self.phase * window) * fade(self.phase) + self.read(other * window) * fade(other);
                *frame = *frame * (1.0 - mix) + wet * mix;
            }
            self.phase = (self.phase + step).rem_euclid(1.0);
            self.write = (self.write + 1) % n;
        }
    }
}

/// A backend that plays to nothing and lets the game (offline mixing) or a
/// test pull the mix out a block at a time, the way an output device would.
pub(crate) mod buffer {
    use kira::backend::{Backend, Renderer};
    use std::sync::{Arc, Mutex};

    pub(crate) type Shared = Arc<Mutex<Option<Renderer>>>;

    pub(crate) struct BufferBackend {
        renderer: Shared,
    }
    pub(crate) struct BufferSettings {
        pub sample_rate: u32,
        pub renderer: Shared,
    }
    impl Backend for BufferBackend {
        type Settings = BufferSettings;
        type Error = ();
        fn setup(settings: BufferSettings, _internal_buffer_size: usize) -> Result<(Self, u32), ()> {
            Ok((
                Self {
                    renderer: settings.renderer,
                },
                settings.sample_rate,
            ))
        }
        fn start(&mut self, renderer: Renderer) -> Result<(), ()> {
            *self.renderer.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(renderer);
            Ok(())
        }
    }

    /// The output side: renders stereo frames as an audio callback would.
    pub(crate) struct Output {
        pub(crate) renderer: Shared,
        pub(crate) sample_rate: u32,
    }
    impl Output {
        /// The next `seconds` of the mix, as (left, right) pairs.
        pub(crate) fn render(&self, seconds: f32) -> Vec<[f32; 2]> {
            let frames = (seconds * self.sample_rate as f32).round() as usize;
            let mut guard = self.renderer.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(renderer) = guard.as_mut() else {
                return vec![[0.0; 2]; frames];
            };
            let mut out = Vec::with_capacity(frames);
            let mut block = [0.0f32; 2 * 128];
            let mut left = frames;
            while left > 0 {
                let n = left.min(128);
                renderer.on_start_processing();
                renderer.process(&mut block[..2 * n], 2);
                out.extend(block[..2 * n].chunks(2).map(|c| [c[0], c[1]]));
                left -= n;
            }
            out
        }
    }

    /// Root-mean-square of the left channel.
    #[cfg(test)]
    pub(crate) fn rms(frames: &[[f32; 2]]) -> f32 {
        (frames.iter().map(|f| f[0] * f[0]).sum::<f32>() / frames.len().max(1) as f32).sqrt()
    }
}
