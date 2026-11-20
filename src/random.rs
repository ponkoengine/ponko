//! Random numbers that keep simulations reproducible, even on all cores.

use glam::Vec2;
use ponko_ecs::Entity;

/// The seed games use unless [`Game::seed`](crate::game::Game::seed) sets one, so
/// runs are reproducible by default.
pub const DEFAULT_SEED: u64 = 0x00C0_FFEE_C4AB_B1E5;

/// One splitmix64 step: a well-mixed 64-bit value from any 64-bit input.
#[inline(always)]
fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A small, fast random number generator (xoshiro256++). Same seed, same
/// numbers, on every machine.
///
/// In systems, get one from [`Ctx::random`](crate::ctx::Ctx::random), which
/// keeps runs reproducible even inside `par_for_each`. `Rng::new` is for
/// setup code and tests.
///
/// ```
/// use ponko::prelude::*;
///
/// let mut rng = Rng::new(42);
/// let x = rng.range(-1.0, 1.0);
/// assert!((-1.0..1.0).contains(&x));
/// assert_eq!(Rng::new(42).u64(), Rng::new(42).u64());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    /// A generator seeded with `seed`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut a = Rng::new(1);
    /// let mut b = Rng::new(2);
    /// assert_ne!(a.u64(), b.u64());
    /// ```
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let a = mix(seed);
        let b = mix(a);
        let c = mix(b);
        let d = mix(c);
        Self { s: [a, b, c, d] }
    }

    /// 64 random bits.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let bits = Rng::new(7).u64();
    /// # let _ = bits;
    /// ```
    #[inline]
    pub fn u64(&mut self) -> u64 {
        let s = &mut self.s;
        let out = s[0].wrapping_add(s[3]).rotate_left(23).wrapping_add(s[0]);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        out
    }

    /// A number in `[0, 1)`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let x = Rng::new(7).f32();
    /// assert!((0.0..1.0).contains(&x));
    /// ```
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.u64() >> 40) as f32 * (1.0 / (1u64 << 24) as f32)
    }

    /// A number in `[low, high)`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let speed = Rng::new(7).range(20.0, 40.0);
    /// assert!((20.0..40.0).contains(&speed));
    /// ```
    #[inline]
    pub fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.f32()
    }

    /// A whole number in `0..n` (`0` when `n` is 0).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let pick = Rng::new(7).below(6); // a die roll, 0..=5
    /// assert!(pick < 6);
    /// ```
    #[inline]
    pub fn below(&mut self, n: u32) -> u32 {
        // Lemire's multiply-shift: unbiased enough for games, no division.
        (((self.u64() >> 32) * u64::from(n)) >> 32) as u32
    }

    /// `true` with probability `p` (0 never, 1 always).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut rng = Rng::new(7);
    /// assert!(!rng.chance(0.0));
    /// assert!(rng.chance(1.0));
    /// ```
    #[inline]
    pub fn chance(&mut self, p: f32) -> bool {
        self.f32() < p
    }

    /// An angle in radians, `[0, TAU)`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let heading = Rng::new(7).angle();
    /// assert!((0.0..std::f32::consts::TAU).contains(&heading));
    /// ```
    #[inline]
    pub fn angle(&mut self) -> f32 {
        self.f32() * std::f32::consts::TAU
    }

    /// A direction: a vector of length 1 pointing anywhere, every direction
    /// equally likely.
    ///
    /// Bit-identical on every platform (and in the C++ header's `rng`): it
    /// picks a point in a disk with integers and divides it by its length,
    /// with no `sin` or `cos`, whose results differ between platforms'
    /// math libraries. It takes one or more numbers from the generator
    /// (1.27 on average).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let dir = Rng::new(7).unit_vec2();
    /// assert!((dir.length() - 1.0).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn unit_vec2(&mut self) -> Vec2 {
        // Two 24-bit integer coordinates in [-2^23, 2^23), kept when the
        // point lies in the disk of radius 2^23 (78.5 % of draws). The
        // square length is exact as an integer and as an `f64` (< 2^53),
        // and `sqrt` and `/` round the same everywhere.
        const HALF: i64 = 1 << 23;
        loop {
            let bits = self.u64();
            let x = (bits >> 40) as i64 - HALF;
            let y = ((bits >> 16) & 0xFF_FFFF) as i64 - HALF;
            let d2 = x * x + y * y;
            if d2 != 0 && d2 <= HALF * HALF {
                let d = (d2 as f64).sqrt();
                return Vec2::new((x as f64 / d) as f32, (y as f64 / d) as f32);
            }
        }
    }
}

/// Where a system's random numbers come from this tick: from
/// [`Ctx::random`](crate::ctx::Ctx::random). `Copy`, so it can be used inside
/// `par_for_each`.
///
/// Every (seed, tick, system, entity) gets its own stream, so the numbers
/// don't depend on thread scheduling or on how many entities other systems
/// touched: a simulation run twice with the same seed comes out the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Random {
    base: u64,
}

impl Random {
    pub(crate) fn new(seed: u64, tick: u64, system: usize) -> Self {
        Self {
            base: mix(mix(seed ^ mix(tick)) ^ system as u64),
        }
    }

    /// The random stream of `entity` for this tick and system.
    ///
    /// ```
    /// use ponko::prelude::*;
    ///
    /// #[derive(Component)]
    /// struct Wander(f32);
    ///
    /// fn wander(ctx: &mut Ctx) {
    ///     let random = ctx.random();
    ///     ctx.world.query::<(Entity, &mut Wander)>().par_for_each(|(e, w)| {
    ///         w.0 += random.of(e).range(-0.1, 0.1);
    ///     });
    /// }
    /// # Game::new("t", 1, 1).sim_system(wander).run_headless(1, |_| {});
    /// ```
    #[inline]
    #[must_use]
    pub fn of(self, entity: Entity) -> Rng {
        self.stream(entity.to_bits())
    }

    /// The raw stream base, so other languages (the C++ header) can derive
    /// the same streams as [`of`](Self::of) and [`stream`](Self::stream).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let bits: u64 = ctx.random().to_bits();
    /// # let _ = bits;
    /// # }
    /// ```
    #[must_use]
    pub fn to_bits(self) -> u64 {
        self.base
    }

    /// Stream number `n` for this tick and system, for random numbers that
    /// don't belong to one entity (where to spawn, which event fires).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let mut rng = ctx.random().stream(0);
    /// let spawn_at = vec2(rng.range(0.0, 800.0), rng.range(0.0, 600.0));
    /// # let _ = spawn_at;
    /// # }
    /// ```
    #[inline]
    #[must_use]
    pub fn stream(self, n: u64) -> Rng {
        Rng::new(self.base ^ mix(n))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins the generator: saved runs and the C++ header (`ponko::rng`,
    /// checked against the same numbers in examples/cpp/tests.cpp) rely on it.
    #[test]
    fn the_generator_never_changes() {
        let mut rng = Rng::new(42);
        assert_eq!(
            [rng.u64(), rng.u64(), rng.u64()],
            [0xc757960b442b0ac3, 0x4bb22a7f77ff8c6c, 0x04950439d3c5eafe]
        );
        assert_eq!(Random { base: 7 }.stream(3).u64(), 0x6045c0e00af2e148);
    }

    #[test]
    fn values_stay_in_range_and_look_uniform() {
        let mut rng = Rng::new(3);
        let mut buckets = [0u32; 10];
        for _ in 0..100_000 {
            let x = rng.f32();
            assert!((0.0..1.0).contains(&x));
            buckets[(x * 10.0) as usize] += 1;
        }
        assert!(buckets.iter().all(|&b| (9_000..11_000).contains(&b)), "{buckets:?}");
        let mut seen = [false; 6];
        for _ in 0..1000 {
            seen[rng.below(6) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn unit_vectors_have_length_one_and_point_every_way_alike() {
        let mut rng = Rng::new(11);
        let mut sectors = [0u32; 8];
        for _ in 0..80_000 {
            let v = rng.unit_vec2();
            assert!((v.length() - 1.0).abs() < 1e-6, "{v}");
            let angle = v.y.atan2(v.x) + std::f32::consts::PI;
            sectors[((angle / std::f32::consts::TAU * 8.0) as usize).min(7)] += 1;
        }
        assert!(sectors.iter().all(|&n| (9_500..10_500).contains(&n)), "{sectors:?}");
    }

    /// Pins `unit_vec2` bit for bit: no platform math library is involved,
    /// and the C++ header's `rng::unit_vec2` (examples/cpp/tests.cpp)
    /// checks the same bits.
    #[test]
    fn unit_vectors_are_the_same_bits_everywhere() {
        let mut rng = Rng::new(42);
        let bits: Vec<[u32; 2]> = (0..4).map(|_| rng.unit_vec2().to_array().map(f32::to_bits)).collect();
        assert_eq!(
            bits,
            [
                [0xbf7ffc9f, 0xbc266865],
                [0x3f2e9265, 0xbf3b3eb5],
                [0xbf70cf38, 0x3eadbea9],
                [0xbd82aa6b, 0xbf7f7a7a]
            ]
        );
    }

    #[test]
    fn streams_differ_by_tick_system_and_key() {
        let base = Random::new(1, 10, 0).stream(5).u64();
        assert_ne!(base, Random::new(1, 11, 0).stream(5).u64());
        assert_ne!(base, Random::new(1, 10, 1).stream(5).u64());
        assert_ne!(base, Random::new(1, 10, 0).stream(6).u64());
        assert_ne!(base, Random::new(2, 10, 0).stream(5).u64());
        assert_eq!(base, Random::new(1, 10, 0).stream(5).u64());
    }
}
