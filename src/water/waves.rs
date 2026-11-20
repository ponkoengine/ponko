//! Gerstner waves: one table of waves that the water's vertex shader and
//! the CPU height query both evaluate, so floating bodies ride the waves
//! that are drawn.

use glam::{Vec2, Vec3};

/// Gravity the waves' speeds follow (deep-water dispersion), in m/s².
pub const WAVE_GRAVITY: f32 = 9.81;

/// The most waves one [`Waves`] set holds (the shader loops over them per
/// vertex and per pixel).
pub const MAX_WAVES: usize = 8;

/// One Gerstner wave: a train of crests rolling along `direction`.
///
/// Its speed follows from its length, as deep-water waves' does
/// (`ω = √(g·k)`), unless [`speed`](Self::speed) says otherwise.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::Wave;
/// let swell = Wave::new(vec2(1.0, 0.0), 20.0, 0.4);
/// assert!((swell.phase_speed() - 5.587).abs() < 0.01); // m/s, √(g·L / 2π)
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wave {
    /// Where the crests roll to, on the ground (x, z). Need not be unit length.
    pub direction: Vec2,
    /// From one crest to the next, in metres.
    pub wavelength: f32,
    /// Crest height above the rest level, in metres.
    pub amplitude: f32,
    /// How sharp the crests are, 0 (a sine) to 1 (pointed crests). A set
    /// whose crests would fold over is scaled down (see [`Waves::new`]).
    pub steepness: f32,
    /// Times the deep-water speed (1: the default).
    pub speed: f32,
    /// Where the wave starts, in radians, so equal waves don't line up.
    pub phase: f32,
}

impl Wave {
    /// A wave of `wavelength` and `amplitude` along `direction`, half steep,
    /// at deep-water speed.
    #[must_use]
    pub fn new(direction: Vec2, wavelength: f32, amplitude: f32) -> Self {
        Self {
            direction,
            wavelength,
            amplitude,
            steepness: 0.5,
            speed: 1.0,
            phase: 0.0,
        }
    }

    /// The same wave with crests `steepness` sharp (0 to 1).
    #[must_use]
    pub fn with_steepness(self, steepness: f32) -> Self {
        Self { steepness, ..self }
    }

    /// The same wave starting at `phase` radians.
    #[must_use]
    pub fn with_phase(self, phase: f32) -> Self {
        Self { phase, ..self }
    }

    /// The same wave at `speed` times the deep-water speed.
    #[must_use]
    pub fn with_speed(self, speed: f32) -> Self {
        Self { speed, ..self }
    }

    /// The wave number `k = 2π / wavelength`, in radians per metre.
    #[must_use]
    pub fn wave_number(&self) -> f32 {
        std::f32::consts::TAU / self.wavelength.max(1e-3)
    }

    /// How fast the phase turns, `ω`, in radians per second.
    #[must_use]
    pub fn angular_speed(&self) -> f32 {
        (WAVE_GRAVITY * self.wave_number()).sqrt() * self.speed
    }

    /// How fast a crest moves along the water, in m/s.
    #[must_use]
    pub fn phase_speed(&self) -> f32 {
        self.angular_speed() / self.wave_number()
    }
}

/// What the water is doing at one point: see [`Waves::sample`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveSample {
    /// The surface's height above the rest level, in metres.
    pub height: f32,
    /// The surface's unit normal.
    pub normal: Vec3,
    /// How fast the water at the surface moves (the waves' orbits), in m/s.
    pub velocity: Vec3,
    /// How much the surface is squeezed together: 1 on flat water, below
    /// 1 near sharp crests (where whitecaps form), 0 where it would fold.
    pub squeeze: f32,
}

/// One wave's numbers as both sides use them: direction (unit), `k`,
/// amplitude, horizontal amplitude, `ω`, phase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WaveTerm {
    pub dir: Vec2,
    pub k: f32,
    pub amplitude: f32,
    pub sway: f32,
    pub omega: f32,
    pub phase: f32,
}

/// A set of up to [`MAX_WAVES`] Gerstner waves on still water: what a sea
/// does, and what the water's shader draws. Built once; the CPU side
/// ([`height_at`](Self::height_at), [`sample`](Self::sample)) and the
/// shader evaluate the same table, so what floats rides the waves seen.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::Waves;
/// let sea = Waves::sea(vec2(1.0, 0.3), 1.2);
/// let h = sea.height_at(vec2(3.0, -2.0), 1.5);
/// assert!(h.abs() <= sea.max_height());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Waves {
    terms: Vec<WaveTerm>,
    waves: Vec<Wave>,
}

impl Default for Waves {
    fn default() -> Self {
        Self::none()
    }
}

impl Waves {
    /// Flat water.
    #[must_use]
    pub fn none() -> Self {
        Self {
            terms: Vec::new(),
            waves: Vec::new(),
        }
    }

    /// The waves given (the first [`MAX_WAVES`]). Waves of zero length or
    /// height are dropped. Where the crests together would fold over (the
    /// sum of each wave's `k · amplitude · steepness` past 0.9), every
    /// steepness is scaled down to keep the surface whole.
    #[must_use]
    pub fn new(waves: &[Wave]) -> Self {
        let waves: Vec<Wave> = waves
            .iter()
            .copied()
            .filter(|w| w.wavelength > 0.0 && w.amplitude > 0.0 && w.direction.length_squared() > 0.0)
            .take(MAX_WAVES)
            .collect();
        let fold: f32 = waves
            .iter()
            .map(|w| w.wave_number() * w.amplitude * w.steepness.clamp(0.0, 1.0))
            .sum();
        let scale = if fold > 0.9 { 0.9 / fold } else { 1.0 };
        let terms = waves
            .iter()
            .map(|w| WaveTerm {
                dir: w.direction.normalize(),
                k: w.wave_number(),
                amplitude: w.amplitude,
                sway: w.amplitude * w.steepness.clamp(0.0, 1.0) * scale,
                omega: w.angular_speed(),
                phase: w.phase,
            })
            .collect();
        Self { terms, waves }
    }

    /// Gentle ripples for a lake or pond: three short waves `height` metres
    /// from trough to crest, mostly along `wind`.
    #[must_use]
    pub fn lake(wind: Vec2, height: f32) -> Self {
        let wind = wind.try_normalize().unwrap_or(Vec2::X);
        let a = height.max(0.0) / 2.0 / 3.0;
        Self::new(&[
            Wave::new(wind, 3.1, a).with_steepness(0.3),
            Wave::new(rotate(wind, 0.6), 1.9, a).with_steepness(0.3).with_phase(1.3),
            Wave::new(rotate(wind, -0.5), 1.3, a)
                .with_steepness(0.3)
                .with_phase(2.6),
        ])
    }

    /// An open sea: six waves from a long swell down to short chop, spread
    /// around `wind` (x, z), their crests together reaching about `height`
    /// metres from trough to crest. The swell is about 14 times as long as
    /// it is high, as wind waves are.
    #[must_use]
    pub fn sea(wind: Vec2, height: f32) -> Self {
        let wind = wind.try_normalize().unwrap_or(Vec2::X);
        let height = height.max(0.01);
        let longest = 14.0 * height;
        // (length share, angle off the wind, phase)
        const SHAPE: [(f32, f32, f32); 6] = [
            (1.0, 0.0, 0.0),
            (0.62, 0.45, 1.7),
            (0.41, -0.35, 3.1),
            (0.27, 0.8, 4.4),
            (0.18, -0.7, 0.9),
            (0.12, 0.25, 2.2),
        ];
        let total: f32 = SHAPE.iter().map(|s| s.0).sum();
        let unit = height / 2.0 / total;
        let waves: Vec<Wave> = SHAPE
            .iter()
            .map(|&(share, angle, phase)| {
                Wave::new(rotate(wind, angle), longest * share, unit * share)
                    .with_steepness(0.7)
                    .with_phase(phase)
            })
            .collect();
        Self::new(&waves)
    }

    /// The waves as given (after dropping empty ones).
    #[must_use]
    pub fn waves(&self) -> &[Wave] {
        &self.waves
    }

    /// Whether there are no waves (flat water).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// The highest the surface can rise above (or sink below) its rest
    /// level: the sum of the amplitudes.
    #[must_use]
    pub fn max_height(&self) -> f32 {
        self.terms.iter().map(|t| t.amplitude).sum()
    }

    /// The farthest the surface moves sideways: the sum of the horizontal
    /// amplitudes.
    #[must_use]
    pub fn max_sway(&self) -> f32 {
        self.terms.iter().map(|t| t.sway).sum()
    }

    pub(crate) fn terms(&self) -> &[WaveTerm] {
        &self.terms
    }

    /// Where the water at rest point `p` (x, z) is at time `t`: moved
    /// sideways and up. This is what the vertex shader does to each vertex.
    #[must_use]
    pub fn displace(&self, p: Vec2, t: f32) -> Vec3 {
        let mut out = Vec3::new(p.x, 0.0, p.y);
        for w in &self.terms {
            let theta = w.k * w.dir.dot(p) - w.omega * t + w.phase;
            let (s, c) = theta.sin_cos();
            out.x += w.dir.x * w.sway * c;
            out.z += w.dir.y * w.sway * c;
            out.y += w.amplitude * s;
        }
        out
    }

    /// The rest point whose water is over `at` (x, z) at time `t`: undoes
    /// the sideways sway with Newton's method (a few steps; the sway is
    /// kept below folding, so it converges).
    #[must_use]
    pub fn rest_point(&self, at: Vec2, t: f32) -> Vec2 {
        if self.terms.is_empty() {
            return at;
        }
        let mut p = at;
        for _ in 0..8 {
            // f(p) = p + sway(p) - at, and its 2x2 Jacobian.
            let mut f = p - at;
            let (mut jxx, mut jxz, mut jzz) = (1.0f32, 0.0f32, 1.0f32);
            for w in &self.terms {
                let theta = w.k * w.dir.dot(p) - w.omega * t + w.phase;
                let (s, c) = theta.sin_cos();
                f += w.dir * (w.sway * c);
                let d = -w.sway * w.k * s;
                jxx += w.dir.x * w.dir.x * d;
                jxz += w.dir.x * w.dir.y * d;
                jzz += w.dir.y * w.dir.y * d;
            }
            if f.length_squared() < 1e-12 {
                break;
            }
            let det = jxx * jzz - jxz * jxz;
            if det.abs() < 1e-4 {
                p -= f; // fixed-point step where the crest is nearly folded
            } else {
                p -= Vec2::new(jzz * f.x - jxz * f.y, jxx * f.y - jxz * f.x) / det;
            }
        }
        p
    }

    /// The surface's height above the rest level at `at` (x, z) at time `t`,
    /// in metres: where the drawn water is, to within the mesh's
    /// tessellation.
    #[must_use]
    pub fn height_at(&self, at: Vec2, t: f32) -> f32 {
        if self.terms.is_empty() {
            return 0.0;
        }
        self.displace(self.rest_point(at, t), t).y
    }

    /// The surface at `at` (x, z) at time `t`: height, normal, the water's
    /// velocity there and how squeezed it is.
    #[must_use]
    pub fn sample(&self, at: Vec2, t: f32) -> WaveSample {
        let p = self.rest_point(at, t);
        self.sample_rest(p, t)
    }

    /// [`sample`](Self::sample) at rest point `p`.
    pub(crate) fn sample_rest(&self, p: Vec2, t: f32) -> WaveSample {
        let mut height = 0.0;
        let mut n = Vec3::Y;
        let mut v = Vec3::ZERO;
        let (mut jxx, mut jxz, mut jzz) = (1.0f32, 0.0f32, 1.0f32);
        for w in &self.terms {
            let theta = w.k * w.dir.dot(p) - w.omega * t + w.phase;
            let (s, c) = theta.sin_cos();
            height += w.amplitude * s;
            n.x -= w.dir.x * w.k * w.amplitude * c;
            n.z -= w.dir.y * w.k * w.amplitude * c;
            n.y -= w.k * w.sway * s;
            v.x += w.dir.x * w.sway * w.omega * s;
            v.z += w.dir.y * w.sway * w.omega * s;
            v.y -= w.amplitude * w.omega * c;
            let d = w.sway * w.k * s;
            jxx -= w.dir.x * w.dir.x * d;
            jxz -= w.dir.x * w.dir.y * d;
            jzz -= w.dir.y * w.dir.y * d;
        }
        WaveSample {
            height,
            normal: n.normalize_or(Vec3::Y),
            velocity: v,
            squeeze: (jxx * jzz - jxz * jxz).max(0.0),
        }
    }
}

/// `v` as a WGSL float literal that reads back as the same `f32`.
pub(crate) fn lit(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rest_point_moves_onto_the_asked_point() {
        let sea = Waves::sea(Vec2::new(1.0, 0.4), 2.0);
        for i in 0..200 {
            let at = Vec2::new((i as f32 * 0.37).sin() * 30.0, (i as f32 * 0.91).cos() * 30.0);
            let t = i as f32 * 0.13;
            let p = sea.rest_point(at, t);
            let moved = sea.displace(p, t);
            assert!(
                (Vec2::new(moved.x, moved.z) - at).length() < 1e-3,
                "{at} at {t}: rest point {p} lands at {moved}"
            );
        }
    }

    #[test]
    fn steep_sets_are_scaled_down_so_crests_never_fold() {
        let waves = Waves::new(&[
            Wave::new(Vec2::X, 2.0, 0.5).with_steepness(1.0),
            Wave::new(Vec2::Y, 2.0, 0.5).with_steepness(1.0),
        ]);
        let fold: f32 = waves.terms().iter().map(|w| w.k * w.sway).sum();
        assert!(fold <= 0.9 + 1e-5, "{fold}");
        for i in 0..400 {
            let s = waves.sample_rest(Vec2::new(i as f32 * 0.05, i as f32 * 0.031), 0.0);
            assert!(s.squeeze > 0.0, "the surface folds at sample {i}");
        }
    }

    #[test]
    fn the_wave_velocity_is_the_motion_of_the_surface() {
        let sea = Waves::sea(Vec2::new(0.3, 1.0), 1.5);
        let p = Vec2::new(4.0, -7.0);
        let dt = 1e-3;
        let a = sea.displace(p, 2.0);
        let b = sea.displace(p, 2.0 + dt);
        let v = sea.sample_rest(p, 2.0 + dt / 2.0).velocity;
        assert!(((b - a) / dt - v).length() < 0.01, "{:?} vs {v}", (b - a) / dt);
    }

    #[test]
    fn literals_read_back_as_the_same_float() {
        for v in [1.0f32, 0.1, -3.25e-7, 12345.678, 6.0e10] {
            let text = lit(v);
            assert_eq!(text.parse::<f32>().unwrap(), v, "{text}");
        }
    }
}
