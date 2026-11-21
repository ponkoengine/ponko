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
//! GPU without compute shaders, by the same FFT on the CPU, uploaded. The
//! CPU's height query runs the same FFT at the time asked, so what floats
//! rides the waves that are drawn.
//!
//! The field packs two complex signals whose inverse transforms are real:
//! `A = h̃ + i·D̃x` and `B = D̃z + i·∂h̃/∂t`, so one RGBA texel holds the
//! height, the sway along x and z, and how fast the surface rises.

use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};

use glam::{Vec2, Vec3};

use super::waves::{WAVE_GRAVITY, WaveSample};

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
/// wave vector's speed, and the last moments the CPU worked out.
#[derive(Debug)]
pub(crate) struct SpectrumField {
    pub settings: WaveSpectrum,
    pub n: usize,
    /// Per texel of the spectrum: `h0(k)` and `conj(h0(-k))`.
    pub h0: Vec<[C; 2]>,
    /// The highest the surface practically rises and sways (ten times the
    /// heights' and sways' spread).
    pub max_height: f32,
    pub max_sway: f32,
    /// The last two moments worked out (newest last).
    cache: Mutex<Vec<Arc<Field>>>,
}

impl PartialEq for SpectrumField {
    fn eq(&self, other: &Self) -> bool {
        self.settings == other.settings
    }
}

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
    let base = TAU / SPECTRUM_PERIOD;
    ((WAVE_GRAVITY * k).sqrt() / base).floor() * base
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
        for j in 0..n {
            for i in 0..n {
                let (mi, mj) = ((n - i) % n, (n - j) % n);
                h0[j * n + i] = [start(i, j), start(mi, mj).conj()];
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
        Self {
            settings,
            n,
            h0,
            max_height: 10.0 * sigma,
            max_sway: 10.0 * settings.choppiness.max(0.0) * sigma,
            cache: Mutex::new(Vec::with_capacity(2)),
        }
    }

    /// The spectrum's start as the GPU's texture holds it (RGBA per texel:
    /// `h0(k)` and `conj(h0(-k))`).
    pub fn start_texels(&self) -> Vec<[f32; 4]> {
        self.h0.iter().map(|[a, b]| [a.re, a.im, b.re, b.im]).collect()
    }

    /// The two packed signals at time `t`, spectrum texel by texel (what
    /// `spectrum.wgsl` computes), and the sway's speed if `speed`.
    fn at_time(&self, t: f32, speed: bool) -> (Vec<[C; 2]>, Vec<C>) {
        let n = self.n;
        let tile = self.settings.tile.max(1.0);
        let chop = self.settings.choppiness.max(0.0);
        let t = wrap_time(t);
        let mut ab = vec![[C::ZERO; 2]; n * n];
        let mut v = if speed { vec![C::ZERO; n * n] } else { Vec::new() };
        for j in 0..n {
            for i in 0..n {
                let k = Vec2::new(wave_index(i, n), wave_index(j, n)) * (TAU / tile);
                let kl = k.length();
                let [h0, h0m] = self.h0[j * n + i];
                let w = omega(kl);
                let e = C::cis(w * t);
                let (up, down) = (h0.mul(e), h0m.mul(e.conj()));
                let ht = up.add(down);
                let (ux, uz) = if kl > 0.0 { (k.x / kl, k.y / kl) } else { (0.0, 0.0) };
                // A = h̃ + i·D̃x with D̃x = -i·chop·k̂x·h̃: h̃·(1 + chop·k̂x).
                let a = ht.scale(1.0 + chop * ux);
                // B = D̃z + i·∂h̃/∂t = -i·chop·k̂z·h̃ - ω·(up - down).
                let dz = ht.mul(C::new(0.0, -chop * uz));
                let b = dz.sub(up.sub(down).scale(w));
                ab[j * n + i] = [a, b];
                if speed {
                    // ∂D̃x/∂t + i·∂D̃z/∂t, with ∂h̃/∂t = i·ω·(up - down).
                    let dht = up.sub(down).mul(C::new(0.0, w));
                    let dx = dht.mul(C::new(0.0, -chop * ux));
                    let dz = dht.mul(C::new(0.0, -chop * uz));
                    v[j * n + i] = dx.add(dz.mul(C::new(0.0, 1.0)));
                }
            }
        }
        (ab, v)
    }

    /// The field at time `t`, worked out on the CPU (with the sway's speed
    /// if `speed`); the last two moments are kept.
    pub fn field(&self, t: f32, speed: bool) -> Arc<Field> {
        let mut cache = self.cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = cache.iter().find(|f| f.t == t && (!speed || !f.sway_speed.is_empty())) {
            return f.clone();
        }
        let field = Arc::new(self.compute(t, speed));
        if cache.len() == 2 {
            cache.remove(0);
        }
        cache.push(field.clone());
        field
    }

    fn compute(&self, t: f32, speed: bool) -> Field {
        let n = self.n;
        let (ab, v) = self.at_time(t, speed);
        let mut a: Vec<C> = ab.iter().map(|p| p[0]).collect();
        let mut b: Vec<C> = ab.iter().map(|p| p[1]).collect();
        let mut scratch = vec![C::ZERO; n];
        inverse_fft_2d(&mut a, n, &mut scratch);
        inverse_fft_2d(&mut b, n, &mut scratch);
        let texels = a.iter().zip(&b).map(|(a, b)| [a.re, a.im, b.re, b.im]).collect();
        let sway_speed = if speed {
            let mut v = v;
            inverse_fft_2d(&mut v, n, &mut scratch);
            v.iter().map(|c| [c.re, c.im]).collect()
        } else {
            Vec::new()
        };
        Field {
            t,
            n,
            texels,
            sway_speed,
        }
    }
}

/// One stage of the Stockham FFT, as the GPU's butterfly pass does it for
/// output `o` of `n`: stage size `ns` (1, 2, 4, ... n/2), reading `x`.
/// The inverse transform (positive exponent), unnormalised.
pub(crate) fn butterfly(x: &[C], o: usize, n: usize, ns: usize) -> C {
    let r = (o % (2 * ns)) / ns;
    let j = (o / (2 * ns)) * ns + o % ns;
    let w = C::cis(TAU * (j % ns) as f32 / (2 * ns) as f32);
    let (a, b) = (x[j], x[j + n / 2].mul(w));
    if r == 0 { a.add(b) } else { a.sub(b) }
}

/// The inverse FFT of `x` (length `n`, a power of two) in place.
pub(crate) fn inverse_fft(x: &mut [C], scratch: &mut [C]) {
    let n = x.len();
    let mut ns = 1;
    let mut in_x = true;
    while ns < n {
        if in_x {
            for o in 0..n {
                scratch[o] = butterfly(x, o, n, ns);
            }
        } else {
            for o in 0..n {
                x[o] = butterfly(scratch, o, n, ns);
            }
        }
        in_x = !in_x;
        ns *= 2;
    }
    if !in_x {
        x.copy_from_slice(&scratch[..n]);
    }
}

/// The 2D inverse FFT of `x` (`n` x `n`, rows of x) in place: rows, then columns.
fn inverse_fft_2d(x: &mut [C], n: usize, scratch: &mut Vec<C>) {
    scratch.resize(n, C::ZERO);
    for row in x.chunks_exact_mut(n) {
        inverse_fft(row, scratch);
    }
    let mut column = vec![C::ZERO; n];
    for i in 0..n {
        for j in 0..n {
            column[j] = x[j * n + i];
        }
        inverse_fft(&mut column, scratch);
        for j in 0..n {
            x[j * n + i] = column[j];
        }
    }
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
            let mut scratch = vec![C::ZERO; n];
            inverse_fft(&mut got, &mut scratch);
            for (g, w) in got.iter().zip(&want) {
                assert!(g.sub(*w).norm2().sqrt() < 1e-3 * n as f32, "n {n}: {g:?} vs {w:?}");
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
        let spectrum = SpectrumField::new(WaveSpectrum::new(Vec2::new(6.0, 6.0), 1.0).with_tile(100.0, 64));
        let (a, b) = (
            spectrum.field(12.5, false),
            spectrum.field(12.5 + SPECTRUM_PERIOD, false),
        );
        for (x, y) in a.texels.iter().zip(&b.texels) {
            assert!((x[0] - y[0]).abs() < 1e-3);
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
