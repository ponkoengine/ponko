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
    /// The last two moments worked out (newest last), and the buffers the
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
                fields: Vec::with_capacity(2),
                ..Cache::default()
            }),
        }
    }

    /// The spectrum's start as the GPU's texture holds it (RGBA per texel:
    /// `h0(k)` and `conj(h0(-k))`).
    pub fn start_texels(&self) -> Vec<[f32; 4]> {
        self.h0.iter().map(|[a, b]| [a.re, a.im, b.re, b.im]).collect()
    }

    /// The field at time `t`, worked out on the CPU (with the sway's speed
    /// if `speed`); the last two moments are kept, so each is worked out
    /// once however many queries ask. When a moment worked out without the
    /// sway's speed is then asked with it (the WebGL2 upload, then buoyancy
    /// at the same time), the next moments are worked out with it from the
    /// start, until one such guess goes unused.
    pub fn field(&self, t: f32, speed: bool) -> Arc<Field> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Kept by the moment in the period, as `compute` names it.
        let key = wrap_time(t);
        if let Some(f) = cache
            .fields
            .iter()
            .find(|f| f.t == key && (!speed || !f.sway_speed.is_empty()))
            .cloned()
        {
            if speed && cache.guessed == Some(key) {
                cache.guessed = None;
            }
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
        // The oldest moment's buffers are reused (or the one replaced),
        // unless someone still holds it.
        let old = match again {
            Some(i) => Arc::try_unwrap(cache.fields.remove(i)).ok(),
            None if cache.fields.len() == 2 => Arc::try_unwrap(cache.fields.remove(0)).ok(),
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

/// `v` as a half float's bits (round to nearest even), for uploads.
pub(crate) fn f16_bits(v: f32) -> u16 {
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

/// `texels` as an RGBA half-float upload, into `out` (kept between frames).
pub(crate) fn write_half_texels(texels: &[[f32; 4]], out: &mut Vec<u8>) {
    out.clear();
    out.extend(texels.iter().flat_map(|t| t.map(f16_bits)).flat_map(u16::to_le_bytes));
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

    #[test]
    fn a_new_moment_reuses_the_oldest_ones_buffers() {
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let first = spectrum.field(1.0, false);
        let first_texels = first.texels.as_ptr();
        let kept = first.texels.clone();
        drop(first);
        spectrum.field(2.0, false);
        // The cache holds two moments: the third takes over the first's.
        let third = spectrum.field(3.0, false);
        assert_eq!(
            third.texels.as_ptr(),
            first_texels,
            "the oldest moment's texels are reused"
        );
        assert_ne!(third.texels, kept, "and hold the new moment");
        // A moment someone still holds is left alone.
        let held = spectrum.field(4.0, false);
        let held_texels = held.texels.clone();
        spectrum.field(5.0, false);
        spectrum.field(6.0, false);
        assert_eq!(held.texels, held_texels, "a held moment never changes");
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
