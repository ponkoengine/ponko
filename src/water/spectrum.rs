//! Ocean waves from a spectrum: a square of sea, [`WaveSpectrum::tile`]
//! metres across and repeating, whose heights and sideways sway at each
//! moment are the inverse FFT of a wind-driven wave spectrum (Phillips,
//! after Tessendorf's "Simulating Ocean Water").
//!
//! The spectrum's random start (`h0`, one complex amplitude per wave
//! vector) is made once on the CPU from the seed. Each frame it is turned
//! to the moment's waves and transformed back to the square, either by
//! compute passes on the GPU (`spectrum.wgsl`, `butterfly.wgsl`: one pass
//! for the spectrum, then one per FFT stage along x and along z) or, on a
//! GPU without compute shaders, by the same transform on the CPU
//! (rustfft's, with the CPU's SIMD), uploaded. The CPU's height query
//! works out the same field at the time asked, so what floats rides the
//! waves that are drawn. The tests keep the GPU's algorithm on the CPU
//! (`tests::reference`) and hold the fast field to it.
//!
//! The field packs two complex signals whose inverse transforms are real:
//! `A = h̃ + i·D̃x` and `B = D̃z + i·∂h̃/∂t`, so one RGBA texel holds the
//! height, the sway along x and z, and how fast the surface rises.

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use glam::{Vec2, Vec3};
use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

use super::waves::{WAVE_GRAVITY, WaveSample};

// rustfft's WASM SIMD code alone made a new moment three times slower in
// Chromium (its SIMD functions can't be inlined into callers built
// without `simd128`); with `simd128` everywhere it is 1.4 to 1.6 times
// faster (book/src/water.md, "Waves from a spectrum"). So the feature
// asks for both.
#[cfg(all(feature = "wasm-simd", target_arch = "wasm32", not(target_feature = "simd128")))]
compile_error!(
    "the `wasm-simd` feature needs the whole build compiled with SIMD: build with \
     `web/build.sh --simd`, which sets both, or set \
     CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS=\"-C target-feature=+simd128\" \
     (or add it to RUSTFLAGS), or leave the feature off"
);

/// The longest the waves take to repeat, in seconds: every wave's speed is
/// rounded to a whole number of turns in it, so the sea loops seamlessly
/// and the shader's phases stay small however long the game runs.
pub const SPECTRUM_PERIOD: f32 = 200.0;

/// Waves from a wind-driven spectrum ([`Waves::spectrum`](super::Waves::spectrum)):
/// thousands of waves of every length the square holds, each moving at
/// its own speed, instead of a handful of [`Wave`](super::Wave)s.
///
/// The surface repeats every [`tile`](Self::tile) metres; the smallest
/// waves are two cells of [`resolution`](Self::resolution) across, and the
/// water's fine ripples are drawn on top as with any waves.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::{WaveSpectrum, Waves};
/// let spectrum = WaveSpectrum::new(vec2(12.0, 4.0), 2.0); // wind in m/s, height in m
/// assert_eq!(spectrum.resolution, 128);
/// let sea = Waves::spectrum(spectrum);
/// assert!(sea.height_at(vec2(3.0, -2.0), 1.5).abs() < sea.max_height());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveSpectrum {
    /// The wind, along x and z, in m/s: the waves run with it, and its
    /// speed sets how long the biggest ones are (about `0.9 · speed²`
    /// metres: 22 m at 5 m/s, 90 m at 10 m/s). A square much smaller than
    /// that leaves the biggest waves out.
    pub wind: Vec2,
    /// The significant wave height, in metres: trough to crest of the
    /// highest third of the waves (four times the heights' spread).
    pub height: f32,
    /// The side of the square that repeats, in metres.
    pub tile: f32,
    /// Cells across the square: 64, 128 or 256 (rounded to the nearest of
    /// these). More cells draw shorter waves and cost more.
    pub resolution: u32,
    /// How sharp the crests are: 0 rounded, 1 (the default) pointed. The
    /// surface is pushed sideways towards the crests by this much.
    pub choppiness: f32,
    /// Picks the random sea: equal seeds draw equal seas.
    pub seed: u32,
}

impl WaveSpectrum {
    /// A sea under `wind` (m/s along x and z), `height` metres of
    /// significant wave height, on a 256 m square of 128 cells.
    #[must_use]
    pub fn new(wind: Vec2, height: f32) -> Self {
        Self {
            wind,
            height,
            tile: 256.0,
            resolution: 128,
            choppiness: 1.0,
            seed: 1,
        }
    }

    /// The same sea on a square of `tile` metres and `resolution` cells.
    #[must_use]
    pub fn with_tile(self, tile: f32, resolution: u32) -> Self {
        Self {
            tile,
            resolution,
            ..self
        }
    }

    /// The same sea with crests `choppiness` sharp (0 to about 1.5).
    #[must_use]
    pub fn with_choppiness(self, choppiness: f32) -> Self {
        Self { choppiness, ..self }
    }

    /// The same settings, another random sea.
    #[must_use]
    pub fn with_seed(self, seed: u32) -> Self {
        Self { seed, ..self }
    }

    /// The cells across the square the sea is made with: 64, 128 or 256.
    #[must_use]
    pub fn cells(&self) -> usize {
        match self.resolution {
            0..=90 => 64,
            91..=181 => 128,
            _ => 256,
        }
    }
}

/// A complex number (the spectrum's and the FFT's).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct C {
    pub re: f32,
    pub im: f32,
}

impl C {
    const ZERO: C = C { re: 0.0, im: 0.0 };

    fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    fn mul(self, o: C) -> C {
        C::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }

    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }

    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }

    fn scale(self, s: f32) -> C {
        C::new(self.re * s, self.im * s)
    }

    fn conj(self) -> C {
        C::new(self.re, -self.im)
    }

    fn cis(a: f32) -> C {
        let (s, c) = a.sin_cos();
        C::new(c, s)
    }

    fn norm2(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

/// One moment of the field, `n` x `n` texels row by row (z rows of x):
/// height, sway along x and z, and the surface's velocity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Field {
    pub t: f32,
    pub n: usize,
    /// Height, sway x, sway z, rising speed (the GPU texel's layout).
    pub texels: Vec<[f32; 4]>,
    /// The sway's speed along x and z (CPU queries only; empty when not made).
    pub sway_speed: Vec<[f32; 2]>,
}

/// The spectrum made from a [`WaveSpectrum`]: its random start and each
/// wave vector's direction and speed, and the last moments the CPU worked out.
#[derive(Debug)]
pub(crate) struct SpectrumField {
    pub settings: WaveSpectrum,
    pub n: usize,
    /// Per texel of the spectrum: `h0(k)` and `conj(h0(-k))`.
    pub h0: Vec<[C; 2]>,
    /// Per texel of the spectrum: its wave's direction and speed.
    waves: Vec<Wave>,
    /// The most turns any wave makes in a [`SPECTRUM_PERIOD`].
    most_turns: usize,
    /// The inverse FFT of `n` values.
    fft: Plan,
    /// The highest the surface practically rises and sways (ten times the
    /// heights' and sways' spread).
    pub max_height: f32,
    pub max_sway: f32,
    /// The last [`KEPT_MOMENTS`] moments asked for (the latest last), and the buffers the
    /// next one is worked out in.
    cache: Mutex<Cache>,
}

/// One wave vector `k` of the spectrum: its direction (`k / |k|`, zero for
/// the mean level) and its speed `ω`, a whole number of `turns` per
/// [`SPECTRUM_PERIOD`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Wave {
    ux: f32,
    uz: f32,
    omega: f32,
    turns: u32,
}

/// A planned inverse FFT (rustfft's, which picks the CPU's SIMD).
struct Plan(Arc<dyn Fft<f32>>);

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plan").field("len", &self.0.len()).finish()
    }
}

/// How many moments the CPU keeps: the two fixed steps a drawn frame sits
/// between (which floating bodies ask for too, see
/// [`SpectrumField::write_drawn`]), and one for a query at the frame's own
/// time.
const KEPT_MOMENTS: usize = 3;

/// The two fixed steps a drawn frame sits between: the next step's time
/// (as `float_bodies` asks it on that step), the one after, how far the
/// frame is from the first to the second (0 to 1), and the time the waves
/// are drawn at, that far between them (within the period: see
/// `water::drawn_steps`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawnSteps {
    pub t0: f32,
    pub t1: f32,
    pub between: f32,
    pub at: f32,
}

/// The CPU's moments and its work space: kept between frames, so a new
/// moment allocates nothing once the first few are made.
#[derive(Debug, Default)]
struct Cache {
    fields: Vec<Arc<Field>>,
    /// Each wave's turn at the moment: `cis(ω·t)` for each whole number of turns.
    spin: Vec<C>,
    /// The two packed signals and the sway's speed, spectrum texel by texel,
    /// transformed along x in place.
    a: Vec<Complex32>,
    b: Vec<Complex32>,
    v: Vec<Complex32>,
    /// The same turned on their side (columns as rows), then transformed
    /// along z: the field, column by column.
    a_t: Vec<Complex32>,
    b_t: Vec<Complex32>,
    v_t: Vec<Complex32>,
    /// The FFT's work space.
    scratch: Vec<Complex32>,
    /// Whether new moments are worked out with the sway's speed though
    /// not asked for it: on after a moment worked out without it was
    /// asked again with it (a WebGL2 upload, then buoyancy at the same
    /// time), off after such a guess goes unused.
    guess_speed: bool,
    /// The moment last worked out with the sway's speed on a guess, until
    /// a query with the sway's speed uses it.
    guessed: Option<f32>,
    /// Moments worked out (tests count them).
    #[cfg(test)]
    computed: usize,
    /// The next step's time (in the period) of the last frame drawn
    /// between two steps.
    drawn_t0: Option<f32>,
    /// Never draw from the steps' moments: the upload as it was before
    /// (tests time both).
    #[cfg(test)]
    never_share: bool,
    /// Never guess: the field as it was before guessing (tests time both).
    #[cfg(test)]
    never_guess: bool,
}

impl PartialEq for SpectrumField {
    fn eq(&self, other: &Self) -> bool {
        self.settings == other.settings
    }
}

/// The side of the blocks the grid is turned on its side in, to stay in
/// the cache.
const BLOCK: usize = 16;
/// The wave number of spectrum index `i` of `n`, in cycles across the
/// square: 0..n/2 up, then the negative ones (the FFT's order).
fn wave_index(i: usize, n: usize) -> f32 {
    if i < n / 2 { i as f32 } else { i as f32 - n as f32 }
}

/// A Gaussian random pair from the seed and the texel (Box–Muller over a
/// hash, so a seed always gives the same sea on every machine).
fn gaussian(seed: u32, i: usize, j: usize) -> (f32, f32) {
    let hash = |x: u32| {
        let mut h = x ^ seed.wrapping_mul(0x9E37_79B9);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7FEB_352D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x846C_A68B);
        h ^= h >> 16;
        h
    };
    let cell = (j as u32) << 16 | i as u32;
    let u1 = ((hash(cell.wrapping_mul(2)) >> 8) as f32 + 0.5) / (1u32 << 24) as f32;
    let u2 = ((hash(cell.wrapping_mul(2) + 1) >> 8) as f32 + 0.5) / (1u32 << 24) as f32;
    let r = (-2.0 * u1.ln()).sqrt();
    (r * (TAU * u2).cos(), r * (TAU * u2).sin())
}

/// The wave speed `ω` of wave number `k` (rad/m) in deep water, rounded
/// down to a whole number of turns per [`SPECTRUM_PERIOD`].
pub(crate) fn omega(k: f32) -> f32 {
    turns(k) as f32 * (TAU / SPECTRUM_PERIOD)
}

/// The whole turns per [`SPECTRUM_PERIOD`] of [`omega`]`(k)`.
fn turns(k: f32) -> u32 {
    ((WAVE_GRAVITY * k).sqrt() / (TAU / SPECTRUM_PERIOD)).floor() as u32
}
/// `t` folded into one period (the shader does the same).
pub(crate) fn wrap_time(t: f32) -> f32 {
    let t = f64::from(t);
    let p = f64::from(SPECTRUM_PERIOD);
    (t - (t / p).floor() * p) as f32
}

impl SpectrumField {
    pub fn new(settings: WaveSpectrum) -> Self {
        let n = settings.cells();
        let tile = settings.tile.max(1.0);
        let speed = settings.wind.length().max(0.5);
        let wind_dir = settings.wind.try_normalize().unwrap_or(Vec2::X);
        // The longest waves the wind raises, and the shortest kept.
        let big = speed * speed / WAVE_GRAVITY;
        let small = tile / n as f32 * 0.5;
        let phillips = |k: Vec2| -> f32 {
            let k2 = k.length_squared();
            if k2 < 1e-12 {
                return 0.0;
            }
            let along = k.dot(wind_dir) / k2.sqrt();
            // Waves against the wind are mostly gone.
            let facing = if along < 0.0 { 0.07 } else { 1.0 };
            (-1.0 / (k2 * big * big)).exp() / (k2 * k2) * along * along * facing * (-k2 * small * small).exp()
        };
        let k_of = |i: usize, j: usize| Vec2::new(wave_index(i, n), wave_index(j, n)) * (TAU / tile);
        let start = |i: usize, j: usize| {
            // The Nyquist row and column have no partner wave: left out.
            if i == n / 2 || j == n / 2 {
                return C::ZERO;
            }
            let (a, b) = gaussian(settings.seed, i, j);
            C::new(a, b).scale((phillips(k_of(i, j)) * 0.5).sqrt())
        };
        let mut h0 = vec![[C::ZERO; 2]; n * n];
        let mut waves = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let (mi, mj) = ((n - i) % n, (n - j) % n);
                h0[j * n + i] = [start(i, j), start(mi, mj).conj()];
                let k = k_of(i, j);
                let kl = k.length();
                let (ux, uz) = if kl > 0.0 { (k.x / kl, k.y / kl) } else { (0.0, 0.0) };
                waves.push(Wave {
                    ux,
                    uz,
                    omega: omega(kl),
                    turns: turns(kl),
                });
            }
        }
        // Scaled so four times the heights' spread is the asked height
        // (Parseval: the spread squared is the sum of the amplitudes').
        let variance: f32 = h0.iter().map(|[a, b]| a.norm2() + b.norm2()).sum();
        let scale = if variance > 0.0 {
            settings.height.max(0.0) / 4.0 / variance.sqrt()
        } else {
            0.0
        };
        for h in &mut h0 {
            *h = [h[0].scale(scale), h[1].scale(scale)];
        }
        // The sway's spread along each axis is at most the choppiness
        // times the heights'.
        let sigma = if variance > 0.0 {
            settings.height.max(0.0) / 4.0
        } else {
            0.0
        };
        let most_turns = waves.iter().map(|w| w.turns as usize).max().unwrap_or(0);
        Self {
            settings,
            n,
            h0,
            waves,
            most_turns,
            fft: Plan(FftPlanner::new().plan_fft_inverse(n)),
            max_height: 10.0 * sigma,
            max_sway: 10.0 * settings.choppiness.max(0.0) * sigma,
            cache: Mutex::new(Cache {
                fields: Vec::with_capacity(KEPT_MOMENTS),
                ..Cache::default()
            }),
        }
    }

    /// The field drawn between the two fixed steps `s` (see
    /// `water::drawn_steps`), as the half floats of the WebGL2 upload, into
    /// `out`: the blend of the two steps' moments, which floating bodies ask
    /// for on those steps (`floating`: they are worked out with the sway's
    /// speed for them), so a frame works out no moment of its own; within a
    /// few tenths of a millimetre of the moment at `s.at`
    /// (`the_drawn_blend_of_two_steps_is_the_moment_between`). Without
    /// floating bodies it blends when one of the two is kept already, or
    /// when the last frame was between the same two steps (frames come
    /// faster than steps: two moments once, then one a step instead of one
    /// a frame); a frame otherwise never works out more than one moment.
    /// Otherwise it is the moment at `s.at`. Returns whether it blended.
    pub(crate) fn write_drawn(&self, s: DrawnSteps, floating: bool, out: &mut Vec<u16>) -> bool {
        if self.shares(s, floating) {
            let (a, b) = (self.field(s.t0, floating), self.field(s.t1, floating));
            write_half_texels_between(&a.texels, &b.texels, s.between, out);
            return true;
        }
        write_half_texels(&self.field(s.at, false).texels, out);
        false
    }

    /// Whether drawing from the two steps' moments costs at most what the
    /// moment at the frame's time would.
    fn shares(&self, s: DrawnSteps, floating: bool) -> bool {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        #[cfg(test)]
        if cache.never_share {
            return false;
        }
        let kept = |t: f32| cache.fields.iter().any(|f| f.t == wrap_time(t));
        let again = cache.drawn_t0 == Some(wrap_time(s.t0));
        let shares = floating || again || kept(s.t0) || kept(s.t1);
        cache.drawn_t0 = Some(wrap_time(s.t0));
        shares
    }

    /// The surface's height at rest point `at` as drawn from `steps` (see
    /// [`write_drawn`](Self::write_drawn)): from the two kept moments, no
    /// new one.
    pub(crate) fn drawn_height(&self, at: Vec2, s: DrawnSteps) -> f32 {
        let tile = self.settings.tile.max(1.0);
        let (a, b) = (self.field(s.t0, false), self.field(s.t1, false));
        let height = |f: &Field| f.displace(f.rest_point(at, tile), tile).y;
        height(&a) + (height(&b) - height(&a)) * s.between
    }

    /// The spectrum's start as the GPU's texture holds it (RGBA per texel:
    /// `h0(k)` and `conj(h0(-k))`).
    pub fn start_texels(&self) -> Vec<[f32; 4]> {
        self.h0.iter().map(|[a, b]| [a.re, a.im, b.re, b.im]).collect()
    }

    /// The field at time `t`, worked out on the CPU (with the sway's speed
    /// if `speed`); the last three moments asked for are kept, so each is worked out
    /// once however many queries ask. When a moment worked out without the
    /// sway's speed is then asked with it (the WebGL2 upload, then buoyancy
    /// at the same time), the next moments are worked out with it from the
    /// start, until one such guess goes unused.
    pub fn field(&self, t: f32, speed: bool) -> Arc<Field> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Kept by the moment in the period, as `compute` names it.
        let key = wrap_time(t);
        if let Some(i) = cache
            .fields
            .iter()
            .position(|f| f.t == key && (!speed || !f.sway_speed.is_empty()))
        {
            if speed && cache.guessed == Some(key) {
                cache.guessed = None;
            }
            // Asked again: the last to go (the next step's moment outlives a
            // query at a frame's own time).
            let f = cache.fields.remove(i);
            cache.fields.push(f.clone());
            return f;
        }
        // The same moment without the sway's speed: replaced, and a sign
        // that the next moments will be asked with it too.
        let again = cache.fields.iter().position(|f| f.t == key);
        let with_speed = if speed {
            #[cfg(test)]
            let guess = !cache.never_guess;
            #[cfg(not(test))]
            let guess = true;
            if again.is_some() && guess {
                cache.guess_speed = true;
            }
            true
        } else if cache.guess_speed {
            if cache.guessed.is_some() {
                // The last guess went unused: stop guessing.
                cache.guess_speed = false;
                cache.guessed = None;
                false
            } else {
                cache.guessed = Some(key);
                true
            }
        } else {
            false
        };
        // The least recently asked moment's buffers are reused (or the one
        // replaced), unless someone still holds it.
        let old = match again {
            Some(i) => Arc::try_unwrap(cache.fields.remove(i)).ok(),
            None if cache.fields.len() == KEPT_MOMENTS => Arc::try_unwrap(cache.fields.remove(0)).ok(),
            None => None,
        };
        #[cfg(test)]
        {
            cache.computed += 1;
        }
        let field = Arc::new(self.compute(t, with_speed, &mut cache, old));
        cache.fields.push(field.clone());
        field
    }

    /// Works out the moment `t` (what `spectrum.wgsl` and the FFT passes
    /// compute): the spectrum turned to the moment, then the inverse FFT
    /// along x, and along z on the grid turned on its side.
    fn compute(&self, t: f32, speed: bool, work: &mut Cache, old: Option<Field>) -> Field {
        let n = self.n;
        let chop = self.settings.choppiness.max(0.0);
        let Cache {
            spin,
            a,
            b,
            v,
            a_t,
            b_t,
            v_t,
            scratch,
            ..
        } = work;
        // `cis(ω·t)` once per speed: every ω is a whole number of turns.
        let t = wrap_time(t);
        let turn = TAU / SPECTRUM_PERIOD;
        spin.clear();
        spin.extend((0..=self.most_turns).map(|m| C::cis(m as f32 * turn * t)));
        a.clear();
        b.clear();
        v.clear();
        for ([h0, h0m], w) in self.h0.iter().zip(&self.waves) {
            // The waves running with `k` (`up`) and against it (`down`).
            let e = spin[w.turns as usize];
            let (up, down) = (h0.mul(e), h0m.mul(e.conj()));
            let ht = up.add(down);
            // A = h̃ + i·D̃x with D̃x = -i·chop·k̂x·h̃: h̃·(1 + chop·k̂x).
            let pa = ht.scale(1.0 + chop * w.ux);
            // B = D̃z + i·∂h̃/∂t = -i·chop·k̂z·h̃ - ω·(up - down).
            let dz = ht.mul(C::new(0.0, -chop * w.uz));
            let pb = dz.sub(up.sub(down).scale(w.omega));
            a.push(Complex32::new(pa.re, pa.im));
            b.push(Complex32::new(pb.re, pb.im));
            if speed {
                // ∂D̃x/∂t + i·∂D̃z/∂t, with ∂h̃/∂t = i·ω·(up - down).
                let dht = up.sub(down).mul(C::new(0.0, w.omega));
                let dx = dht.mul(C::new(0.0, -chop * w.ux));
                let dz = dht.mul(C::new(0.0, -chop * w.uz));
                let s = dx.add(dz.mul(C::new(0.0, 1.0)));
                v.push(Complex32::new(s.re, s.im));
            }
        }
        let fft = &*self.fft.0;
        inverse_fft_2d(fft, a, a_t, scratch);
        inverse_fft_2d(fft, b, b_t, scratch);
        if speed {
            inverse_fft_2d(fft, v, v_t, scratch);
        }
        let mut field = old.unwrap_or(Field {
            t,
            n,
            texels: Vec::new(),
            sway_speed: Vec::new(),
        });
        field.t = t;
        field.n = n;
        // The transforms are column by column: turned back as they're packed.
        field.texels.clear();
        field.sway_speed.clear();
        for z in 0..n {
            field.texels.extend((0..n).map(|x| {
                let (a, b) = (a_t[x * n + z], b_t[x * n + z]);
                [a.re, a.im, b.re, b.im]
            }));
            if speed {
                field
                    .sway_speed
                    .extend((0..n).map(|x| [v_t[x * n + z].re, v_t[x * n + z].im]));
            }
        }
        field
    }
}

/// The 2D inverse FFT of `x` (`n` x `n`, rows of x), unnormalised: along x
/// in place, then along z into `x_t`, which ends up holding the result
/// turned on its side (`x_t[x * n + z]`).
fn inverse_fft_2d(fft: &dyn Fft<f32>, x: &mut [Complex32], x_t: &mut Vec<Complex32>, scratch: &mut Vec<Complex32>) {
    let n = fft.len();
    scratch.resize(fft.get_inplace_scratch_len(), Complex32::ZERO);
    fft.process_with_scratch(x, scratch);
    x_t.resize(n * n, Complex32::ZERO);
    // Turned on its side a block at a time, to stay in the cache.
    for i0 in (0..n).step_by(BLOCK) {
        for j0 in (0..n).step_by(BLOCK) {
            for i in i0..(i0 + BLOCK).min(n) {
                for j in j0..(j0 + BLOCK).min(n) {
                    x_t[i * n + j] = x[j * n + i];
                }
            }
        }
    }
    fft.process_with_scratch(x_t, scratch);
}

impl Field {
    /// The field at ground point `p` (x, z), filtered between texels and
    /// repeating every `tile` metres (as the shader reads it).
    pub fn at(&self, p: Vec2, tile: f32) -> [f32; 4] {
        let n = self.n;
        let g = p / tile * n as f32;
        let (x0, z0) = (g.x.floor(), g.y.floor());
        let (fx, fz) = (g.x - x0, g.y - z0);
        let wrap = |v: f32| (v as i64).rem_euclid(n as i64) as usize;
        let (x0, z0, x1, z1) = (wrap(x0), wrap(z0), wrap(x0 + 1.0), wrap(z0 + 1.0));
        let t = |x: usize, z: usize| self.texels[z * n + x];
        let lerp = |a: [f32; 4], b: [f32; 4], f: f32| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * f);
        lerp(lerp(t(x0, z0), t(x1, z0), fx), lerp(t(x0, z1), t(x1, z1), fx), fz)
    }

    /// The sway's speed at `p`, filtered as [`at`](Self::at).
    fn sway_speed_at(&self, p: Vec2, tile: f32) -> Vec2 {
        if self.sway_speed.is_empty() {
            return Vec2::ZERO;
        }
        let n = self.n;
        let g = p / tile * n as f32;
        let (x0, z0) = (g.x.floor(), g.y.floor());
        let (fx, fz) = (g.x - x0, g.y - z0);
        let wrap = |v: f32| (v as i64).rem_euclid(n as i64) as usize;
        let (x0, z0, x1, z1) = (wrap(x0), wrap(z0), wrap(x0 + 1.0), wrap(z0 + 1.0));
        let t = |x: usize, z: usize| Vec2::from(self.sway_speed[z * n + x]);
        t(x0, z0).lerp(t(x1, z0), fx).lerp(t(x0, z1).lerp(t(x1, z1), fx), fz)
    }

    /// Where the water at rest point `p` is (x, height, z).
    pub fn displace(&self, p: Vec2, tile: f32) -> Vec3 {
        let f = self.at(p, tile);
        Vec3::new(p.x + f[1], f[0], p.y + f[2])
    }

    /// The rest point whose water is over `at` (fixed-point steps, as the
    /// shader's `water_rest_point`, but more of them).
    pub fn rest_point(&self, at: Vec2, tile: f32) -> Vec2 {
        let mut p = at;
        for _ in 0..8 {
            let d = self.displace(p, tile);
            p -= Vec2::new(d.x, d.z) - at;
        }
        p
    }

    /// The surface over rest point `p`: its height, normal (from the
    /// neighbouring texels, as the shader works it out), velocity and squeeze.
    pub fn sample_rest(&self, p: Vec2, tile: f32) -> WaveSample {
        let e = tile / self.n as f32;
        let at = |dx: f32, dz: f32| self.displace(p + Vec2::new(dx, dz), tile);
        let (xp, xm, zp, zm) = (at(e, 0.0), at(-e, 0.0), at(0.0, e), at(0.0, -e));
        let (tx, tz) = (xp - xm, zp - zm);
        let normal = tz.cross(tx).normalize_or(Vec3::Y);
        let jxx = tx.x / (2.0 * e);
        let jzz = tz.z / (2.0 * e);
        let jxz = tz.x / (2.0 * e);
        let jzx = tx.z / (2.0 * e);
        let f = self.at(p, tile);
        let sway = self.sway_speed_at(p, tile);
        WaveSample {
            height: f[0],
            normal,
            velocity: Vec3::new(sway.x, f[3], sway.y),
            squeeze: (jxx * jzz - jxz * jzx).max(0.0),
        }
    }
}

/// `v` as a half float's bits, for uploads: rounded to the nearest (ties
/// to even), too large to infinity, and any NaN to the quiet NaN `0x7e00`
/// with `v`'s sign. Branch-free (a loop of it vectorises): the sum with
/// 0.5 rounds a subnormal half's bits into place, and a normal one's
/// exponent is rebiased in the same add that rounds its mantissa.
pub(crate) fn f16_bits(v: f32) -> u16 {
    let x = v.to_bits();
    let sign = (x >> 16) & 0x8000;
    let a = x & 0x7fff_ffff;
    // 65536 and up (rounding below it is the normal case's carry).
    let big = if a > 0x7f80_0000 { 0x7e00 } else { 0x7c00 };
    // Under 2^-14: the float add rounds to the half's subnormal steps.
    const HALF: u32 = 126 << 23;
    let small = (f32::from_bits(a) + f32::from_bits(HALF)).to_bits().wrapping_sub(HALF);
    // Exponent -127 + 15 (wrapping), half an ulp less one, and the odd bit
    // for ties to even.
    let normal = a.wrapping_add(0xc800_0fff).wrapping_add((a >> 13) & 1) >> 13;
    let bits = if a >= 143 << 23 {
        big
    } else if a < 113 << 23 {
        small
    } else {
        normal
    };
    (bits | sign) as u16
}

/// How many floats an upload converts at a time (a blended one on the
/// stack), so each chunk is still in the cache for its second pass.
const UPLOAD_CHUNK: usize = 1024;

/// [`f16_bits`] of every float of `src`, into `dst` (as long), as the GPU
/// reads them (little-endian). Where the CPU converts half floats itself
/// (F16C on x86, FP16 on ARM; the `half` crate checks at run time) it does,
/// about five times as fast as [`f16_bits`] in a loop; the result is the
/// same bits (`the_upload_converts_every_float_as_f16_bits`).
fn write_halves(src: &[f32], dst: &mut [u16]) {
    #[cfg(not(target_arch = "wasm32"))]
    for (dst, src) in dst.chunks_mut(UPLOAD_CHUNK).zip(src.chunks(UPLOAD_CHUNK)) {
        use half::slice::{HalfBitsSliceExt, HalfFloatSliceExt};
        dst.reinterpret_cast_mut::<half::f16>().convert_from_f32_slice(src);
        for h in dst.iter_mut() {
            // The CPU keeps some of a NaN's payload: the one quiet NaN.
            let nan = *h & 0x7fff > 0x7c00;
            *h = (if nan { (*h & 0x8000) | 0x7e00 } else { *h }).to_le();
        }
    }
    #[cfg(target_arch = "wasm32")]
    for (h, v) in dst.iter_mut().zip(src) {
        *h = f16_bits(*v).to_le();
    }
}

/// `texels` as an RGBA half-float upload, into `out` (kept between
/// frames; its bytes are what `write_render_target` takes).
pub(crate) fn write_half_texels(texels: &[[f32; 4]], out: &mut Vec<u16>) {
    let src = texels.as_flattened();
    out.resize(src.len(), 0);
    write_halves(src, out);
}

/// [`write_half_texels`] of the blend of two moments' texels, `f` (0 to 1)
/// of the way from `a` to `b`.
fn write_half_texels_between(a: &[[f32; 4]], b: &[[f32; 4]], f: f32, out: &mut Vec<u16>) {
    let (a, b) = (a.as_flattened(), b.as_flattened());
    out.resize(a.len(), 0);
    let mut blend = [0.0f32; UPLOAD_CHUNK];
    for ((out, a), b) in out
        .chunks_mut(UPLOAD_CHUNK)
        .zip(a.chunks(UPLOAD_CHUNK))
        .zip(b.chunks(UPLOAD_CHUNK))
    {
        let blend = &mut blend[..a.len()];
        for ((v, a), b) in blend.iter_mut().zip(a).zip(b) {
            *v = a + (b - a) * f;
        }
        write_halves(blend, out);
    }
}

#[cfg(test)]
mod tests {

    /// The CPU's field as #124 first worked it out: the spectrum turned to
    /// the moment texel by texel, then a radix-2 Stockham FFT, stage by
    /// stage and output by output, as the GPU's passes (`spectrum.wgsl`,
    /// `butterfly.wgsl`) do it. The reference the fast field is held to,
    /// and the "before" of `cpu_field_cost`.
    pub(super) mod reference {
        use super::super::*;

        /// The two packed signals at time `t`, spectrum texel by texel, and
        /// the sway's speed if `speed`.
        fn at_time(s: &SpectrumField, t: f32, ab: &mut Vec<[C; 2]>, v: &mut Vec<C>, speed: bool) {
            let n = s.n;
            let tile = s.settings.tile.max(1.0);
            let chop = s.settings.choppiness.max(0.0);
            let t = wrap_time(t);
            ab.clear();
            ab.resize(n * n, [C::ZERO; 2]);
            v.clear();
            v.resize(if speed { n * n } else { 0 }, C::ZERO);
            for j in 0..n {
                for i in 0..n {
                    let k = Vec2::new(wave_index(i, n), wave_index(j, n)) * (TAU / tile);
                    let kl = k.length();
                    let [h0, h0m] = s.h0[j * n + i];
                    let w = omega(kl);
                    let e = C::cis(w * t);
                    let (up, down) = (h0.mul(e), h0m.mul(e.conj()));
                    let ht = up.add(down);
                    let (ux, uz) = if kl > 0.0 { (k.x / kl, k.y / kl) } else { (0.0, 0.0) };
                    let a = ht.scale(1.0 + chop * ux);
                    let dz = ht.mul(C::new(0.0, -chop * uz));
                    let b = dz.sub(up.sub(down).scale(w));
                    ab[j * n + i] = [a, b];
                    if speed {
                        let dht = up.sub(down).mul(C::new(0.0, w));
                        let dx = dht.mul(C::new(0.0, -chop * ux));
                        let dz = dht.mul(C::new(0.0, -chop * uz));
                        v[j * n + i] = dx.add(dz.mul(C::new(0.0, 1.0)));
                    }
                }
            }
        }

        /// The field at `t`: texels, and the sway's speed if `speed`.
        pub fn field(s: &SpectrumField, t: f32, speed: bool) -> (Vec<[f32; 4]>, Vec<[f32; 2]>) {
            let n = s.n;
            let (mut ab, mut v) = (Vec::new(), Vec::new());
            at_time(s, t, &mut ab, &mut v, speed);
            let mut a: Vec<C> = ab.iter().map(|p| p[0]).collect();
            let mut b: Vec<C> = ab.iter().map(|p| p[1]).collect();
            let (mut scratch, mut column, mut turns) = (Vec::new(), Vec::new(), Vec::new());
            inverse_fft_2d(&mut a, n, &mut scratch, &mut column, &mut turns);
            inverse_fft_2d(&mut b, n, &mut scratch, &mut column, &mut turns);
            let texels = a.iter().zip(&b).map(|(a, b)| [a.re, a.im, b.re, b.im]).collect();
            if speed {
                inverse_fft_2d(&mut v, n, &mut scratch, &mut column, &mut turns);
            }
            (texels, v.iter().map(|c| [c.re, c.im]).collect())
        }

        /// The turns an FFT of length `n` multiplies by: `cis(τ·m/n)` for
        /// `m` in `0..n/2` (what the GPU's butterfly pass works out for each
        /// output).
        pub fn twiddles(n: usize, out: &mut Vec<C>) {
            if out.len() != n / 2 {
                out.clear();
                out.extend((0..n / 2).map(|m| C::cis(TAU * m as f32 / n as f32)));
            }
        }

        /// One stage of the Stockham FFT, as the GPU's butterfly pass does
        /// it for output `o` of `n`: stage size `ns` (1, 2, 4, ... n/2),
        /// reading `x`, with `turns` from [`twiddles`]. The inverse
        /// transform (positive exponent), unnormalised.
        fn butterfly(x: &[C], o: usize, n: usize, ns: usize, turns: &[C]) -> C {
            let r = (o % (2 * ns)) / ns;
            let j = (o / (2 * ns)) * ns + o % ns;
            // cis(τ·(j mod ns)/(2·ns)), from the table.
            let w = turns[(j % ns) * (n / (2 * ns))];
            let (a, b) = (x[j], x[j + n / 2].mul(w));
            if r == 0 { a.add(b) } else { a.sub(b) }
        }

        /// The inverse FFT of `x` (length `n`, a power of two) in place,
        /// with `turns` from [`twiddles`] for `n`.
        pub fn inverse_fft(x: &mut [C], scratch: &mut [C], turns: &[C]) {
            let n = x.len();
            let mut ns = 1;
            let mut in_x = true;
            while ns < n {
                if in_x {
                    for (o, out) in scratch[..n].iter_mut().enumerate() {
                        *out = butterfly(x, o, n, ns, turns);
                    }
                } else {
                    for (o, out) in x.iter_mut().enumerate() {
                        *out = butterfly(scratch, o, n, ns, turns);
                    }
                }
                in_x = !in_x;
                ns *= 2;
            }
            if !in_x {
                x.copy_from_slice(&scratch[..n]);
            }
        }

        /// The 2D inverse FFT of `x` (`n` x `n`, rows of x) in place: rows,
        /// then columns.
        fn inverse_fft_2d(x: &mut [C], n: usize, scratch: &mut Vec<C>, column: &mut Vec<C>, turns: &mut Vec<C>) {
            scratch.resize(n, C::ZERO);
            twiddles(n, turns);
            for row in x.chunks_exact_mut(n) {
                inverse_fft(row, scratch, turns);
            }
            column.resize(n, C::ZERO);
            for i in 0..n {
                for j in 0..n {
                    column[j] = x[j * n + i];
                }
                inverse_fft(column, scratch, turns);
                for j in 0..n {
                    x[j * n + i] = column[j];
                }
            }
        }
    }
    use super::*;

    /// The inverse DFT the slow way.
    fn slow(x: &[C]) -> Vec<C> {
        let n = x.len();
        (0..n)
            .map(|o| {
                (0..n).fold(C::ZERO, |acc, k| {
                    acc.add(x[k].mul(C::cis(TAU * (k * o % n) as f32 / n as f32)))
                })
            })
            .collect()
    }

    #[test]
    fn the_stockham_fft_is_the_inverse_dft() {
        for n in [2usize, 4, 8, 64] {
            let x: Vec<C> = (0..n)
                .map(|i| C::new((i as f32 * 0.7).sin(), (i as f32 * 1.3).cos() * 0.5))
                .collect();
            let want = slow(&x);
            let mut got = x.clone();
            let (mut scratch, mut turns) = (vec![C::ZERO; n], Vec::new());
            reference::twiddles(n, &mut turns);
            reference::inverse_fft(&mut got, &mut scratch, &turns);
            for (g, w) in got.iter().zip(&want) {
                assert!(g.sub(*w).norm2().sqrt() < 1e-3 * n as f32, "n {n}: {g:?} vs {w:?}");
            }
        }
    }

    /// The worst difference between the fast field and the reference's at
    /// `t`, against the sea's practical height.
    fn off_from_reference(spectrum: &SpectrumField, t: f32) -> (f32, f32) {
        let fast = spectrum.field(t, true);
        let (texels, sway_speed) = reference::field(spectrum, t, true);
        let texel_off = fast
            .texels
            .iter()
            .zip(&texels)
            .flat_map(|(f, r)| (0..4).map(move |c| (f[c] - r[c]).abs()))
            .fold(0.0f32, f32::max);
        let speed_off = fast
            .sway_speed
            .iter()
            .zip(&sway_speed)
            .flat_map(|(f, r)| (0..2).map(move |c| (f[c] - r[c]).abs()))
            .fold(0.0f32, f32::max);
        (texel_off, speed_off)
    }

    #[test]
    fn the_fast_cpu_field_is_the_reference_field_at_every_size() {
        for cells in [64, 128, 256] {
            let settings = WaveSpectrum::new(Vec2::new(9.0, -4.0), 2.0).with_tile(256.0, cells);
            let spectrum = SpectrumField::new(settings);
            for t in [0.0, 3.7, 151.25] {
                let (texel_off, speed_off) = off_from_reference(&spectrum, t);
                // Both are f32 FFTs summing the same waves in another
                // order: about a micrometre apart on a 2 m sea.
                assert!(
                    texel_off < 1e-5,
                    "{cells} cells, t {t}: the field is off by {texel_off} m"
                );
                assert!(
                    speed_off < 1e-5,
                    "{cells} cells, t {t}: the sway's speed is off by {speed_off} m/s"
                );
            }
        }
    }

    #[test]
    fn a_new_moment_allocates_nothing_once_warm() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 128));
        for i in 0..3 {
            drop(spectrum.field(i as f32, true));
        }
        let size = |c: &Cache| {
            [&c.a, &c.b, &c.v, &c.a_t, &c.b_t, &c.v_t, &c.scratch]
                .map(|v| (v.as_ptr(), v.capacity()))
                .to_vec()
        };
        let before = size(&spectrum.cache.lock().unwrap());
        for i in 3..6 {
            drop(spectrum.field(i as f32, true));
        }
        assert_eq!(size(&spectrum.cache.lock().unwrap()), before, "the work space is kept");
    }

    /// The CPU field's cost per new moment, the reference (#124's FFT) and
    /// the fast one, interleaved run after run. Run by hand in release:
    /// `cargo test -p ponko --lib --features 3d --release cpu_field_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cpu_field_cost() {
        use std::time::Instant;
        let runs: usize = std::env::var("PONKO_SPECTRUM_RUNS")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(5);
        let ms = |f: &mut dyn FnMut(f32)| {
            // Median of 15 moments, each a new time.
            let mut times: Vec<f32> = (0..15)
                .map(|i| {
                    let start = Instant::now();
                    f(i as f32 * 0.1 + 0.013);
                    start.elapsed().as_secs_f32() * 1000.0
                })
                .collect();
            times.sort_by(f32::total_cmp);
            times[times.len() / 2]
        };
        for run in 0..runs {
            for cells in [64, 128, 256] {
                let settings = WaveSpectrum::new(Vec2::new(8.0, -6.0), 1.5).with_tile(256.0, cells);
                let spectrum = SpectrumField::new(settings);
                let mut line = format!("run {run}, {cells} cells:");
                for speed in [false, true] {
                    let before = ms(&mut |t| {
                        std::hint::black_box(reference::field(&spectrum, t, speed));
                    });
                    let after = ms(&mut |t| {
                        std::hint::black_box(spectrum.field(t, speed));
                    });
                    line += &format!(
                        " {} before {before:.3} ms, after {after:.3} ms;",
                        if speed { "with velocities" } else { "field" }
                    );
                }
                eprintln!("{line}");
            }
        }
    }

    #[test]
    fn the_sea_is_real_and_as_high_as_asked() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(9.0, 3.0), 2.0).with_tile(200.0, 64));
        let field = spectrum.field(3.7, false);
        let n = field.n as f32;
        let mean: f32 = field.texels.iter().map(|t| t[0]).sum::<f32>() / (n * n);
        let spread = (field.texels.iter().map(|t| (t[0] - mean).powi(2)).sum::<f32>() / (n * n)).sqrt();
        // One random sea's spread is near the expected one, not exactly it.
        assert!((spread * 4.0 - 2.0).abs() < 0.4, "four spreads {}", spread * 4.0);
        assert!(mean.abs() < 0.05, "the sea's mean level stays: {mean}");
        assert!(field.texels.iter().all(|t| t[0].abs() < spectrum.max_height));
    }

    #[test]
    fn the_rising_speed_is_how_fast_the_height_changes() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(8.0, 0.0), 1.5).with_tile(128.0, 64));
        let (t, dt) = (5.0, 1e-3);
        let (a, b) = (spectrum.field(t, false), spectrum.field(t + dt, false));
        let mid = spectrum.field(t + dt / 2.0, true);
        for i in (0..a.texels.len()).step_by(97) {
            let rate = (b.texels[i][0] - a.texels[i][0]) / dt;
            let sway = (b.texels[i][1] - a.texels[i][1]) / dt;
            assert!(
                (rate - mid.texels[i][3]).abs() < 0.05,
                "texel {i}: {rate} vs {}",
                mid.texels[i][3]
            );
            assert!(
                (sway - mid.sway_speed[i][0]).abs() < 0.05,
                "texel {i}: {sway} vs {}",
                mid.sway_speed[i][0]
            );
        }
    }

    #[test]
    fn the_sea_loops_every_period() {
        let settings = WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64);
        // Two fields, so the second moment isn't the first one's cache.
        let (a, b) = (
            SpectrumField::new(settings).field(12.5, false),
            SpectrumField::new(settings).field(12.5 + SPECTRUM_PERIOD, false),
        );
        for (x, y) in a.texels.iter().zip(&b.texels) {
            assert!((x[0] - y[0]).abs() < 1e-3);
        }
    }

    fn computed(spectrum: &SpectrumField) -> usize {
        spectrum.cache.lock().unwrap().computed
    }

    /// A frame of a WebGL2 sea with something floating, as the game runs
    /// it on a fixed step: buoyancy at the step's time (with the sway's
    /// speed), then the texture upload at the frame's, which is the next
    /// step's time.
    fn upload_and_buoyancy_frame(spectrum: &SpectrumField, frame: u32) {
        let step = 1.0 / 60.0;
        spectrum.field(frame as f32 * step, true);
        spectrum.field((frame + 1) as f32 * step, false);
    }

    #[test]
    fn the_upload_and_buoyancy_work_out_one_moment_once() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        for frame in 0..3 {
            upload_and_buoyancy_frame(&spectrum, frame);
        }
        let before = computed(&spectrum);
        for frame in 3..13 {
            upload_and_buoyancy_frame(&spectrum, frame);
        }
        assert_eq!(computed(&spectrum) - before, 10, "one new moment a frame");
        // The other order (upload first, then buoyancy at the same time).
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        for frame in 0..10 {
            let t = frame as f32 * 0.1;
            spectrum.field(t, false);
            spectrum.field(t, true);
        }
        assert_eq!(computed(&spectrum), 11, "the first moment twice, then each once");
    }

    #[test]
    fn a_guessed_sway_speed_is_what_a_query_asking_for_it_gets() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(8.0, 0.0), 1.5).with_tile(128.0, 64));
        spectrum.field(1.0, false);
        spectrum.field(1.0, true);
        let guessed = spectrum.field(2.0, false);
        assert!(!guessed.sway_speed.is_empty(), "worked out with the sway's speed");
        let asked = SpectrumField::new(spectrum.settings).field(2.0, true);
        assert_eq!(*guessed, *asked, "the same field as when asked for it");
    }

    #[test]
    fn an_unused_guess_stops_the_guessing() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        spectrum.field(1.0, false);
        spectrum.field(1.0, true);
        assert!(!spectrum.field(2.0, false).sway_speed.is_empty(), "guessed");
        // Nothing asks 2.0 with the sway's speed: the next moment is plain.
        assert!(spectrum.field(3.0, false).sway_speed.is_empty(), "no longer guessed");
        assert!(spectrum.field(4.0, false).sway_speed.is_empty());
        // Without a moment asked twice, plain queries never pay for the speed.
        let plain = SpectrumField::new(spectrum.settings);
        for i in 0..5 {
            assert!(plain.field(i as f32, false).sway_speed.is_empty());
            plain.field(i as f32 + 0.5, true);
        }
    }

    #[test]
    fn a_moment_after_the_first_period_is_worked_out_once() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let t = SPECTRUM_PERIOD * 3.0 + 41.3;
        let a = spectrum.field(t, false);
        let b = spectrum.field(t, false);
        assert!(Arc::ptr_eq(&a, &b), "the second query reads the first one's moment");
        assert_eq!(computed(&spectrum), 1);
    }

    /// A frame's CPU field at 128 cells (and 64 and 256) for a WebGL2 sea
    /// with something floating (`upload_and_buoyancy_frame`), before (no
    /// guessing: the upload's moment worked out again for buoyancy) and
    /// after, run after run. Run by hand in release:
    /// `cargo test -p ponko --lib --features 3d --release upload_and_buoyancy_frame_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn upload_and_buoyancy_frame_cost() {
        use std::time::Instant;
        let runs: usize = std::env::var("PONKO_SPECTRUM_RUNS")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(5);
        for run in 0..runs {
            for cells in [64, 128, 256] {
                let mut line = format!("run {run}, {cells} cells:");
                for guess in [false, true] {
                    let settings = WaveSpectrum::new(Vec2::new(8.0, -6.0), 1.5).with_tile(256.0, cells);
                    let spectrum = SpectrumField::new(settings);
                    spectrum.cache.lock().unwrap().never_guess = !guess;
                    for frame in 0..4 {
                        upload_and_buoyancy_frame(&spectrum, frame);
                    }
                    let mut times: Vec<f32> = (4..19)
                        .map(|frame| {
                            let start = Instant::now();
                            upload_and_buoyancy_frame(&spectrum, frame);
                            start.elapsed().as_secs_f32() * 1000.0
                        })
                        .collect();
                    times.sort_by(f32::total_cmp);
                    line += &format!(
                        " {} {:.3} ms a frame;",
                        if guess { "after" } else { "before" },
                        times[times.len() / 2]
                    );
                }
                eprintln!("{line}");
            }
        }
    }

    /// A game's clocks driving a WebGL2 sea as the engine does: each frame
    /// the engine's clock at the game's speed (`ctx.time.pause`, `resume`,
    /// `set_speed` and `step` on `time`, as `game::frame_timing` feeds
    /// them), its fixed steps (50 a second; buoyancy asks each step's moment
    /// with the sway's speed if `floating`, as `float_bodies`), then the
    /// upload between the steps, unless it is the last frame's (as
    /// `Water::update`).
    struct Frames {
        clock: crate::time::Clock,
        time: crate::time::Time,
        upload: Vec<u16>,
        /// The steps last drawn between, and whether the upload blended.
        drawn: Option<(DrawnSteps, bool)>,
        /// Uploads made.
        uploads: usize,
        /// A frame system asks the height at the drawn time (what is
        /// drawn) between the steps and the upload.
        query: bool,
    }

    impl Frames {
        fn new() -> Self {
            Self {
                clock: crate::time::Clock::new(50),
                time: crate::time::Time::new(1.0 / 50.0),
                upload: Vec::new(),
                drawn: None,
                uploads: 0,
                query: false,
            }
        }

        /// One frame of `dt` seconds: its steps, then the upload. Whether
        /// the field drawn is the blend of two steps' moments.
        fn frame(&mut self, spectrum: &SpectrumField, dt: f64, floating: bool) -> bool {
            let mut timing = self.clock.advance(dt, self.time.effective_sim_speed());
            timing.fixed_steps += self.time.take_pending_steps();
            self.time.begin_frame(timing);
            for _ in 0..timing.fixed_steps {
                if floating {
                    spectrum.field(self.time.sim_time() as f32, true);
                }
                self.time.advance_tick();
            }
            let steps = crate::water::drawn_steps(&self.time);
            if self.query {
                drop(spectrum.field(steps.at, false));
            }
            match self.drawn {
                Some((last, blended)) if last == steps => blended,
                _ => {
                    let blended = spectrum.write_drawn(steps, floating, &mut self.upload);
                    self.drawn = Some((steps, blended));
                    self.uploads += 1;
                    blended
                }
            }
        }
    }

    /// Frame `i`'s length at `hz` frames a second, give or take a quarter
    /// (a variable frame rate).
    fn frame_dt(hz: f64, i: u32) -> f64 {
        (0.75 + 0.5 * f64::from((i * 7919) % 101) / 100.0) / hz
    }

    #[test]
    fn floating_bodies_and_the_upload_share_one_moment_per_step_at_any_frame_rate() {
        for hz in [144.0, 60.0, 50.0, 30.0] {
            let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
            let mut frames = Frames::new();
            for i in 0..3 {
                frames.frame(&spectrum, frame_dt(hz, i), true);
            }
            let (before, tick) = (computed(&spectrum), frames.time.sim_tick());
            for i in 3..90 {
                assert!(
                    frames.frame(&spectrum, frame_dt(hz, i), true),
                    "{hz} Hz: drawn from the steps"
                );
            }
            let steps = (frames.time.sim_tick() - tick) as usize;
            assert_eq!(
                computed(&spectrum) - before,
                steps,
                "{hz} Hz: one moment a step, none of the frames' own"
            );
        }
    }

    #[test]
    fn without_floating_bodies_the_upload_works_out_no_more_than_a_moment_a_frame() {
        for hz in [144.0, 60.0, 30.0, 20.0] {
            let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
            let mut frames = Frames::new();
            let (mut steps, start, mut twice) = (0, frames.time.sim_tick(), 0);
            for i in 0..90 {
                let before = computed(&spectrum);
                frames.frame(&spectrum, frame_dt(hz, i), false);
                let new = computed(&spectrum) - before;
                assert!(new <= 2, "{hz} Hz, frame {i}");
                twice += usize::from(new == 2);
                steps = frames.time.sim_tick() - start;
            }
            assert!(twice <= 1, "{hz} Hz: two moments in a frame only to start blending");
            assert!(computed(&spectrum) <= 90, "{hz} Hz: never more than a moment a frame");
            if hz > 50.0 {
                assert!(
                    computed(&spectrum) <= steps as usize + 3,
                    "{hz} Hz: one moment a step, not one a frame"
                );
            }
            // Not one with the sway's speed: nothing asks for it.
            assert!(
                spectrum
                    .cache
                    .lock()
                    .unwrap()
                    .fields
                    .iter()
                    .all(|f| f.sway_speed.is_empty())
            );
        }
    }

    /// Runs `count` frames at `hz` (give or take a quarter) from frame
    /// `first`, each drawn from the steps' moments; returns the moments
    /// worked out and the steps run.
    fn shared_frames(frames: &mut Frames, spectrum: &SpectrumField, hz: f64, first: u32, count: u32) -> (usize, usize) {
        let (before, tick) = (computed(spectrum), frames.time.sim_tick());
        for i in first..first + count {
            assert!(
                frames.frame(spectrum, frame_dt(hz, i), true),
                "frame {i}: drawn from the steps"
            );
        }
        (computed(spectrum) - before, (frames.time.sim_tick() - tick) as usize)
    }

    #[test]
    fn the_drawn_clock_is_the_frame_clock_until_a_pause_then_stops_with_the_simulation() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let mut frames = Frames::new();
        for i in 0..40 {
            frames.frame(&spectrum, frame_dt(60.0, i), true);
            let at = crate::water::drawn_steps(&frames.time).at;
            assert!(
                (f64::from(at) - frames.time.elapsed()).abs() < 1e-5,
                "frame {i}: on the frame's clock"
            );
        }
        frames.time.pause();
        frames.frame(&spectrum, 1.0 / 60.0, true);
        let paused = crate::water::drawn_steps(&frames.time);
        for _ in 0..30 {
            frames.frame(&spectrum, 1.0 / 60.0, true);
        }
        assert_eq!(crate::water::drawn_steps(&frames.time), paused, "the waves stand still");
        frames.time.resume();
        frames.frame(&spectrum, 1.0 / 60.0, true);
        let resumed = crate::water::drawn_steps(&frames.time).at;
        let behind = frames.time.elapsed() - f64::from(resumed);
        assert!(
            behind > 0.45,
            "half a second paused: the waves are {behind} s behind the frame's clock"
        );
        assert!(
            resumed > paused.at && resumed - paused.at < 0.02,
            "and go on where they stopped"
        );
    }

    #[test]
    fn pausing_resuming_and_changing_speed_keep_one_moment_a_step() {
        // The engine's real clock: pause, a single step while paused,
        // resume, double and half speed. Floating bodies and the upload
        // share one moment a step all along; a paused game works out
        // nothing and uploads nothing.
        for hz in [144.0, 60.0, 30.0] {
            let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
            let mut frames = Frames::new();
            for i in 0..3 {
                frames.frame(&spectrum, frame_dt(hz, i), true);
            }
            let (moments, steps) = shared_frames(&mut frames, &spectrum, hz, 3, 40);
            assert_eq!(moments, steps, "{hz} Hz before the pause");
            frames.time.pause();
            frames.frame(&spectrum, frame_dt(hz, 43), true);
            let (moments, uploads) = (computed(&spectrum), frames.uploads);
            let (_, steps) = shared_frames(&mut frames, &spectrum, hz, 44, 40);
            assert_eq!(steps, 0, "{hz} Hz: paused");
            assert_eq!(computed(&spectrum), moments, "{hz} Hz: paused, nothing worked out");
            assert_eq!(frames.uploads, uploads, "{hz} Hz: paused, the field drawn already");
            frames.time.step();
            let (moments, steps) = shared_frames(&mut frames, &spectrum, hz, 84, 5);
            assert_eq!((moments, steps), (1, 1), "{hz} Hz: one step while paused, its moment");
            frames.time.resume();
            let (moments, steps) = shared_frames(&mut frames, &spectrum, hz, 89, 40);
            assert!(
                moments <= steps + 1,
                "{hz} Hz after resuming: {moments} moments in {steps} steps"
            );
            for speed in [2.0, 0.5, 1.0] {
                frames.time.set_speed(speed);
                let (moments, steps) = shared_frames(&mut frames, &spectrum, hz, 129, 60);
                assert!(steps > 0, "{hz} Hz at {speed}x: the simulation runs");
                assert!(
                    moments <= steps + 1,
                    "{hz} Hz at {speed}x: {moments} moments in {steps} steps, at most one a step"
                );
            }
        }
    }

    #[test]
    fn the_blended_upload_reuses_the_kept_moments_buffers_with_a_query_at_the_frames_time() {
        // Floating bodies, the upload and a game asking the height at the
        // frame's time (what is drawn) every frame: three moments kept, no
        // new buffers once warm.
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let mut frames = Frames::new();
        frames.query = true;
        let buffers = |s: &SpectrumField| -> Vec<usize> {
            let mut b: Vec<usize> = s
                .cache
                .lock()
                .unwrap()
                .fields
                .iter()
                .map(|f| f.texels.as_ptr() as usize)
                .collect();
            b.sort_unstable();
            b
        };
        let (mut seen, mut start) = (Vec::new(), (0, 0));
        for i in 0..60 {
            assert!(frames.frame(&spectrum, frame_dt(60.0, i), true));
            if i == 10 {
                seen = buffers(&spectrum);
                start = (computed(&spectrum), frames.time.sim_tick());
            } else if i > 10 {
                assert!(
                    buffers(&spectrum).iter().all(|b| seen.contains(b)),
                    "frame {i}: a moment in new buffers"
                );
            }
        }
        let steps = (frames.time.sim_tick() - start.1) as usize;
        assert_eq!(
            computed(&spectrum) - start.0,
            steps + 49,
            "a moment a step, and the query's own"
        );
    }

    /// The largest difference between the drawn blend of two steps' moments
    /// and the moment the frame is at, in the height and the sway (metres).
    fn blend_error(settings: WaveSpectrum, t0: f32, step: f32, between: f32) -> f32 {
        let spectrum = SpectrumField::new(settings);
        let steps = DrawnSteps {
            t0,
            t1: t0 + step,
            between,
            at: t0 + step * between,
        };
        let mut blended = Vec::new();
        assert!(spectrum.write_drawn(steps, true, &mut blended));
        let exact = SpectrumField::new(settings).field(t0 + step * between, false);
        let mut exact_half = Vec::new();
        write_half_texels(&exact.texels, &mut exact_half);
        let half = |b: &[u16]| -> Vec<f32> {
            b.iter()
                .map(|&h| half::f16::from_bits(u16::from_le(h)).to_f32())
                .collect()
        };
        let (a, b) = (half(&blended), half(&exact_half));
        // Height and sway (x, z): the first three of each texel's four.
        a.as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_chunks::<4>().0)
            .flat_map(|(a, b)| (0..3).map(move |i| (a[i] - b[i]).abs()))
            .fold(0.0, f32::max)
    }

    #[test]
    fn the_drawn_blend_of_two_steps_is_the_moment_between() {
        // The sea of the GPU test (1.5 m, 128 cells over 256 m), a small
        // choppy tile whose shortest waves turn fastest, and a calm pond.
        let seas = [
            WaveSpectrum::new(Vec2::new(8.0, -6.0), 1.5).with_tile(256.0, 128),
            WaveSpectrum::new(Vec2::new(10.0, 0.0), 2.0).with_tile(64.0, 128),
            WaveSpectrum::new(Vec2::new(2.0, 1.0), 0.2).with_tile(32.0, 64),
        ];
        for (sea, most) in seas.into_iter().zip([0.002, 0.003, 0.001]) {
            let worst = [0.25, 0.5, 0.75]
                .into_iter()
                .map(|between| blend_error(sea, 41.3, 1.0 / 50.0, between))
                .fold(0.0, f32::max);
            eprintln!("{:?}: {:.2} mm", (sea.tile, sea.cells()), worst * 1000.0);
            assert!(worst < most, "{:?}: {worst} m off the moment between", sea.tile);
        }
    }

    /// A frame's CPU field on a WebGL2 sea at a variable frame rate (144,
    /// 60 and 30 frames a second, give or take a quarter; 50 steps a
    /// second), with and without floating bodies, at 64, 128 and 256
    /// cells: before (the upload works out the moment at the frame's time)
    /// and after (it blends the steps' moments), the moments worked out a
    /// frame and the thread's CPU time a frame (a busy machine stretches
    /// the wall clock), over 60 frames, run after run. Run by hand in
    /// release:
    /// `cargo test -p ponko --lib --features 3d --release variable_frame_rate_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn variable_frame_rate_cost() {
        /// This thread's CPU time so far, in milliseconds.
        fn cpu_ms() -> f64 {
            #[cfg(unix)]
            {
                let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                // SAFETY: `t` is a valid, writable timespec.
                let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0;
                assert!(ok, "the thread's CPU clock");
                t.tv_sec as f64 * 1000.0 + t.tv_nsec as f64 / 1e6
            }
            #[cfg(not(unix))]
            {
                use std::sync::OnceLock;
                static START: OnceLock<std::time::Instant> = OnceLock::new();
                START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
            }
        }
        let runs: usize = std::env::var("PONKO_SPECTRUM_RUNS")
            .ok()
            .and_then(|r| r.parse().ok())
            .unwrap_or(3);
        for run in 0..runs {
            for cells in [64, 128, 256] {
                for floating in [true, false] {
                    let mut line = format!("run {run}, {cells} cells, floating {floating}:");
                    for hz in [144.0, 60.0, 30.0] {
                        line += &format!(" {hz} Hz");
                        for share in [false, true] {
                            let settings = WaveSpectrum::new(Vec2::new(8.0, -6.0), 1.5).with_tile(256.0, cells);
                            let spectrum = SpectrumField::new(settings);
                            spectrum.cache.lock().unwrap().never_share = !share;
                            let mut frames = Frames::new();
                            for i in 0..10 {
                                frames.frame(&spectrum, frame_dt(hz, i), floating);
                            }
                            let (start, moments) = (cpu_ms(), computed(&spectrum));
                            for i in 10..70 {
                                frames.frame(&spectrum, frame_dt(hz, i), floating);
                            }
                            let ms = (cpu_ms() - start) / 60.0;
                            let per_frame = (computed(&spectrum) - moments) as f64 / 60.0;
                            line += &format!(
                                " {} {per_frame:.2} moments {ms:.3}",
                                if share { "after" } else { "before" }
                            );
                        }
                        line += " ms;";
                    }
                    eprintln!("{line}");
                }
            }
        }
    }

    #[test]
    fn a_new_moment_reuses_the_oldest_ones_buffers() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let first = spectrum.field(1.0, false);
        let first_texels = first.texels.as_ptr();
        let kept = first.texels.clone();
        drop(first);
        spectrum.field(2.0, false);
        spectrum.field(3.0, false);
        // The cache holds three moments: the fourth takes over the first's.
        let fourth = spectrum.field(4.0, false);
        assert_eq!(
            fourth.texels.as_ptr(),
            first_texels,
            "the oldest moment's texels are reused"
        );
        assert_ne!(fourth.texels, kept, "and hold the new moment");
        // A moment someone still holds is left alone.
        let held = spectrum.field(5.0, false);
        let held_texels = held.texels.clone();
        for t in 6..9 {
            spectrum.field(t as f32, false);
        }
        assert_eq!(held.texels, held_texels, "a held moment never changes");
    }

    /// This thread's CPU time so far, in milliseconds.
    fn thread_cpu_ms() -> f64 {
        #[cfg(unix)]
        {
            let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
            // SAFETY: `t` is a valid, writable timespec.
            let ok = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) } == 0;
            assert!(ok, "the thread's CPU clock");
            t.tv_sec as f64 * 1000.0 + t.tv_nsec as f64 / 1e6
        }
        #[cfg(not(unix))]
        {
            use std::sync::OnceLock;
            static START: OnceLock<std::time::Instant> = OnceLock::new();
            START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
        }
    }

    /// The conversion as it was before it was made branch-free: `v` as a
    /// half float's bits, by cases.
    fn reference_f16_bits(v: f32) -> u16 {
        let bits = v.to_bits();
        let sign = ((bits >> 16) & 0x8000) as u16;
        let exp = ((bits >> 23) & 0xff) as i32;
        let mant = bits & 0x007f_ffff;
        if exp == 0xff {
            // Infinity or NaN.
            return sign | 0x7c00 | if mant != 0 { 0x200 } else { 0 };
        }
        let e = exp - 127 + 15;
        if e >= 0x1f {
            return sign | 0x7c00;
        }
        if e <= 0 {
            // Subnormal, or zero.
            if e < -10 {
                return sign;
            }
            let m = mant | 0x0080_0000;
            let shift = (14 - e) as u32;
            let half = m >> shift;
            let rest = m & ((1 << shift) - 1);
            let mid = 1 << (shift - 1);
            let round = u32::from(rest > mid || (rest == mid && half & 1 == 1));
            return sign | (half + round) as u16;
        }
        let half = ((e as u32) << 10) | (mant >> 13);
        let rest = mant & 0x1fff;
        let round = u32::from(rest > 0x1000 || (rest == 0x1000 && half & 1 == 1));
        sign | (half + round) as u16
    }

    /// The upload as it was before: the reference conversion, a float at a
    /// time, into bytes.
    fn reference_upload(texels: &[[f32; 4]], out: &mut Vec<u8>) {
        out.clear();
        out.extend(
            texels
                .iter()
                .flat_map(|t| t.map(reference_f16_bits))
                .flat_map(u16::to_le_bytes),
        );
    }

    /// The blended upload as it was before.
    fn reference_upload_between(a: &[[f32; 4]], b: &[[f32; 4]], f: f32, out: &mut Vec<u8>) {
        out.clear();
        out.extend(
            a.iter()
                .zip(b)
                .flat_map(|(a, b)| std::array::from_fn::<_, 4, _>(|i| reference_f16_bits(a[i] + (b[i] - a[i]) * f)))
                .flat_map(u16::to_le_bytes),
        );
    }

    /// Floats where a conversion goes wrong if it does: both zeros, every
    /// exponent with mantissas on both sides of each bit the rounding may
    /// look at (ties included), infinities, NaNs with payloads, the edges
    /// of the half floats' range and a million more from a fixed sequence.
    fn interesting_floats() -> Vec<f32> {
        let mut bits = Vec::new();
        for sign in [0, 0x8000_0000u32] {
            for exp in 0..=0xffu32 {
                for k in 0..23 {
                    let unit = 1u32 << k;
                    for high in [0, 1, 0x155, 0x3ff] {
                        let top = (high << 13) & 0x007f_ffff;
                        for low in [0, unit - 1, unit, unit + 1, (unit << 1) - 1, unit | (unit >> 1)] {
                            bits.push(sign | (exp << 23) | ((top | low) & 0x007f_ffff));
                        }
                    }
                }
            }
        }
        let mut x = 0x9e37_79b9u32;
        for _ in 0..1_000_000 {
            // xorshift32
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            bits.push(x);
        }
        let mut floats: Vec<f32> = bits.into_iter().map(f32::from_bits).collect();
        floats.extend([
            0.0,
            -0.0,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            -f32::NAN,
            f32::from_bits(0x7f80_0001),
            f32::from_bits(0xffbf_ffff),
            f32::from_bits(0x7fff_ffff),
            f32::MIN_POSITIVE,
            f32::from_bits(1),
            f32::MAX,
            65504.0,
            65519.996,
            65520.0,
            65535.996,
            65536.0,
            6.103_515_6e-5, // 2^-14, the smallest normal half
            5.960_464_5e-8, // 2^-24, the smallest subnormal half
            2.980_232_2e-8, // 2^-25, a tie between 0 and the smallest
            8.940_697e-8,   // 1.5 × 2^-24, a tie up to 2 × 2^-24
            1.000_488_3,    // 1 + 2^-11, a tie down to 1
            1.001_464_8,    // 1 + 3 × 2^-11, a tie up
        ]);
        floats
    }

    #[test]
    fn f16_bits_rounds_every_interesting_float_as_the_reference() {
        for v in interesting_floats() {
            assert_eq!(f16_bits(v), reference_f16_bits(v), "{v:e} ({:#010x})", v.to_bits());
        }
    }

    #[test]
    fn the_upload_converts_every_float_as_f16_bits() {
        let floats = interesting_floats();
        // Texels of four, a whole number of them.
        let texels: Vec<[f32; 4]> = floats.as_chunks::<4>().0.to_vec();
        let mut upload = Vec::new();
        write_half_texels(&texels, &mut upload);
        let mut before = Vec::new();
        reference_upload(&texels, &mut before);
        assert_eq!(
            bytemuck::cast_slice::<u16, u8>(&upload),
            &before[..],
            "the exact upload"
        );
        // Blends, as the reference blends: with itself (each finite float
        // again, infinities to NaN) and with other values.
        let shifted: Vec<[f32; 4]> = texels.iter().map(|t| t.map(|v| v * 0.75 + 0.1)).collect();
        for (other, f) in [(&texels, 0.5), (&shifted, 0.3), (&shifted, 0.0), (&shifted, 1.0)] {
            write_half_texels_between(&texels, other, f, &mut upload);
            reference_upload_between(&texels, other, f, &mut before);
            assert_eq!(
                bytemuck::cast_slice::<u16, u8>(&upload),
                &before[..],
                "a blend {f} of the way"
            );
        }
    }

    /// Every one of the 2^32 floats converts as the reference does. Takes
    /// a few seconds in release (minutes in debug): run by hand,
    /// `cargo test -p ponko --lib --features 3d --release every_float_converts_as_the_reference -- --ignored`.
    #[test]
    #[ignore]
    fn every_float_converts_as_the_reference() {
        let mut texels = vec![[0.0f32; 4]; 1 << 16];
        let (mut upload, mut before) = (Vec::new(), Vec::new());
        for high in 0..1u32 << 14 {
            for (i, t) in texels.iter_mut().enumerate() {
                *t = std::array::from_fn(|j| f32::from_bits((high << 18) | ((i as u32) << 2) | j as u32));
            }
            write_half_texels(&texels, &mut upload);
            reference_upload(&texels, &mut before);
            assert_eq!(
                bytemuck::cast_slice::<u16, u8>(&upload),
                &before[..],
                "floats from {:#x}",
                high << 18
            );
            for t in &texels {
                for v in t {
                    assert_eq!(f16_bits(*v), reference_f16_bits(*v), "{:#x}", v.to_bits());
                }
            }
        }
    }

    /// The WebGL2 upload's f32 to f16 conversion alone, before (a float at
    /// a time, by cases) and after (the CPU's own conversion where it has
    /// one), against a moment of the CPU field: thread CPU time per
    /// upload, the medians of five runs of 100. Run by hand in release:
    /// `cargo test -p ponko --lib --features 3d --release half_upload_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn half_upload_cost() {
        let median = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            v[v.len() / 2]
        };
        for cells in [128, 256] {
            let settings = WaveSpectrum::new(Vec2::new(8.0, -6.0), 1.5).with_tile(256.0, cells);
            let spectrum = SpectrumField::new(settings);
            let (a, b) = (spectrum.field(10.0, false), spectrum.field(10.02, false));
            let (mut bytes, mut halves) = (Vec::new(), Vec::new());
            let (reps, mut moment) = (100, 0);
            let mut rows: Vec<(&str, Vec<f64>)> = Vec::new();
            for _ in 0..5 {
                let mut time = |name: &'static str, f: &mut dyn FnMut()| {
                    f();
                    let start = thread_cpu_ms();
                    for _ in 0..reps {
                        f();
                    }
                    let ms = (thread_cpu_ms() - start) / f64::from(reps);
                    match rows.iter_mut().find(|r| r.0 == name) {
                        Some(r) => r.1.push(ms),
                        None => rows.push((name, vec![ms])),
                    }
                };
                time("a moment of the field", &mut || {
                    moment += 1;
                    drop(spectrum.field(20.0 + moment as f32 * 0.02, false));
                });
                time("exact upload, before", &mut || reference_upload(&a.texels, &mut bytes));
                time("exact upload, after", &mut || write_half_texels(&a.texels, &mut halves));
                time("blended upload, before", &mut || {
                    reference_upload_between(&a.texels, &b.texels, 0.4, &mut bytes);
                });
                time("blended upload, after", &mut || {
                    write_half_texels_between(&a.texels, &b.texels, 0.4, &mut halves);
                });
            }
            for (name, v) in rows {
                let runs: Vec<String> = v.iter().map(|ms| format!("{ms:.4}")).collect();
                eprintln!("{cells} cells, {name}: {:.4} ms (runs {})", median(v), runs.join(" "));
            }
        }
    }

    #[test]
    fn half_floats_round_trip() {
        for v in [0.0f32, 1.0, -2.5, 0.333_333, 1e-5, 65504.0, -1e-7] {
            let h = f16_bits(v);
            let back = half_to_f32(h);
            assert!((back - v).abs() <= v.abs() * 1e-3 + 1e-7, "{v} -> {h:#x} -> {back}");
        }
    }

    fn half_to_f32(h: u16) -> f32 {
        let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
        let exponent = i32::from((h >> 10) & 0x1f);
        let mantissa = f32::from(h & 0x3ff);
        match exponent {
            0 => sign * mantissa * 2f32.powi(-24),
            31 => sign * f32::INFINITY,
            e => sign * (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
        }
    }
}
