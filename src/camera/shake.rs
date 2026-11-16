//! Trauma-based camera shake: [`CameraShake`].

use glam::{Vec2, Vec3};

/// Camera shake that grows with "trauma": hits add trauma, it wears off
/// over time, and the camera shakes with trauma squared, so small hits
/// barely nudge it and big ones shake it hard. The shake follows smooth
/// noise, not random jumps, and only depends on the time passed, so it
/// looks the same at every frame rate.
///
/// Keep one (as a resource, say), call [`add_trauma`](Self::add_trauma)
/// when something happens, [`update`](Self::update) once a frame, then
/// [`apply_2d`](Self::apply_2d) or `apply_3d` (with `3d`) **after**
/// the camera has been placed for the frame (by a rig or by your code).
/// Applying moves the camera away from where it was put, so a camera that
/// nobody places again next frame would drift: shake one that is placed
/// every frame.
///
/// 2D shake moves the camera up to [`max_offset`](Self::max_offset) window
/// pixels. 3D shake turns it up to [`max_angles`](Self::max_angles): a
/// turning camera never shakes into a wall.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::camera::CameraShake;
///
/// Game::new("Boom", 320, 180).run_headless(30, |ctx| {
///     let mut shake = CameraShake::default();
///     shake.add_trauma(0.6); // an explosion
///     shake.update(ctx.time.frame_dt());
///     ctx.camera.look_at(vec2(160.0, 90.0)); // where the camera wants to be
///     shake.apply_2d(&mut ctx.camera);
///     assert!(ctx.camera.center().distance(vec2(160.0, 90.0)) <= 10.0 * 2f32.sqrt());
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CameraShake {
    /// How shaken the camera is, 0 to 1. [`add_trauma`](Self::add_trauma)
    /// raises it, [`update`](Self::update) lowers it.
    pub trauma: f32,
    /// How much trauma wears off per second. Default 1: a full shake lasts
    /// a second.
    pub decay: f32,
    /// The shake is `trauma.powf(exponent)`. Default 2: small trauma
    /// shakes little.
    pub exponent: f32,
    /// How fast the shake wiggles, in wiggles per second. Default 15.
    pub frequency: f32,
    /// The farthest a full 2D shake moves the camera, in window pixels
    /// (so the same at every zoom). Default 10 x 10.
    pub max_offset: Vec2,
    /// The most a full 3D shake turns the camera, in radians: about its
    /// up axis (yaw), its right axis (pitch) and its view axis (roll).
    /// Default 3°, 3° and 5°.
    pub max_angles: Vec3,
    /// Picks the noise: two shakes with different seeds move differently.
    pub seed: u32,
    /// Seconds of shake played so far: [`update`](Self::update) advances
    /// it. The shake is a function of it.
    pub time: f32,
}

impl Default for CameraShake {
    fn default() -> Self {
        Self {
            trauma: 0.0,
            decay: 1.0,
            exponent: 2.0,
            frequency: 15.0,
            max_offset: Vec2::splat(10.0),
            max_angles: Vec3::new(3f32.to_radians(), 3f32.to_radians(), 5f32.to_radians()),
            seed: 0,
            time: 0.0,
        }
    }
}

impl CameraShake {
    /// Adds `amount` of trauma (kept within 0 to 1): 0.2 for a bump, 0.5
    /// for a hit, 1 for an explosion next to the camera.
    ///
    /// ```
    /// let mut shake = ponko::camera::CameraShake::default();
    /// shake.add_trauma(0.7);
    /// shake.add_trauma(0.7);
    /// assert_eq!(shake.trauma, 1.0);
    /// ```
    pub fn add_trauma(&mut self, amount: f32) {
        if amount.is_finite() {
            self.trauma = (self.trauma + amount).clamp(0.0, 1.0);
        }
    }

    /// Advances the shake by `dt` seconds and wears trauma off. Call it
    /// once a frame with [`Time::frame_dt`](crate::time::Time::frame_dt).
    ///
    /// ```
    /// let mut shake = ponko::camera::CameraShake::default();
    /// shake.add_trauma(1.0);
    /// shake.update(0.25);
    /// assert_eq!(shake.trauma, 0.75);
    /// ```
    pub fn update(&mut self, dt: f32) {
        if !super::damp::positive(dt) {
            return;
        }
        self.trauma = (self.trauma - self.decay.max(0.0) * dt).clamp(0.0, 1.0);
        // Wrap so the noise keeps its precision in long games.
        self.time = (self.time + dt) % 4096.0;
    }

    /// How strongly the camera shakes now, 0 to 1: trauma to the
    /// [`exponent`](Self::exponent).
    ///
    /// ```
    /// let mut shake = ponko::camera::CameraShake::default();
    /// shake.add_trauma(0.5);
    /// assert_eq!(shake.strength(), 0.25);
    /// ```
    #[must_use]
    pub fn strength(&self) -> f32 {
        if self.trauma <= 0.0 {
            0.0
        } else {
            self.trauma.powf(self.exponent.max(0.0)).clamp(0.0, 1.0)
        }
    }

    /// Noise channel `channel` now, -1 to 1.
    fn channel(&self, channel: u32) -> f32 {
        noise(
            self.time * self.frequency.max(0.0),
            self.seed.wrapping_mul(31).wrapping_add(channel),
        )
    }

    /// How far the 2D shake moves the camera now, in window pixels.
    ///
    /// ```
    /// let shake = ponko::camera::CameraShake::default();
    /// assert_eq!(shake.offset_2d(), ponko::prelude::Vec2::ZERO); // no trauma
    /// ```
    #[must_use]
    pub fn offset_2d(&self) -> Vec2 {
        let s = self.strength();
        if s == 0.0 {
            return Vec2::ZERO;
        }
        Vec2::new(self.channel(0), self.channel(1)) * self.max_offset * s
    }

    /// How far the 3D shake turns the camera now, in radians: yaw, pitch
    /// and roll.
    ///
    /// ```
    /// let mut shake = ponko::camera::CameraShake::default();
    /// shake.add_trauma(1.0);
    /// shake.update(0.1);
    /// assert!(shake.angles().abs().cmple(shake.max_angles).all());
    /// ```
    #[must_use]
    pub fn angles(&self) -> Vec3 {
        let s = self.strength();
        if s == 0.0 {
            return Vec3::ZERO;
        }
        Vec3::new(self.channel(2), self.channel(3), self.channel(4)) * self.max_angles * s
    }

    /// Moves the 2D camera by [`offset_2d`](Self::offset_2d). Call it after
    /// the camera is placed for this frame.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, shake: &ponko::camera::CameraShake) {
    /// shake.apply_2d(&mut ctx.camera);
    /// # }
    /// ```
    pub fn apply_2d(&self, camera: &mut crate::d2::Camera) {
        let offset = self.offset_2d();
        if offset != Vec2::ZERO && camera.zoom > 0.0 {
            camera.position += offset / camera.zoom;
        }
    }

    /// Turns the 3D camera by [`angles`](Self::angles), about its own axes.
    /// Call it after the camera is placed for this frame.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, shake: &ponko::camera::CameraShake) {
    /// shake.apply_3d(&mut ctx.scene3d.camera);
    /// # }
    /// ```
    #[cfg(feature = "3d")]
    pub fn apply_3d(&self, camera: &mut crate::d3::Camera) {
        let a = self.angles();
        if a != Vec3::ZERO {
            camera.rotation =
                (camera.rotation * glam::Quat::from_euler(glam::EulerRot::YXZ, a.x, a.y, a.z)).normalize();
        }
    }
}

/// Smooth 1D gradient noise in -1..1 along `x`, one curve per `seed`.
fn noise(x: f32, seed: u32) -> f32 {
    let cell = x.floor();
    let f = x - cell;
    let i = cell as i32;
    let g0 = gradient(i, seed);
    let g1 = gradient(i.wrapping_add(1), seed);
    let t = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    // Gradient noise peaks at half its slope: scale to about -1..1.
    ((g0 * f) * (1.0 - t) + (g1 * (f - 1.0)) * t) * 2.0
}

/// A gradient in -1..1 for lattice point `i`.
fn gradient(i: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / ((1u32 << 24) - 1) as f32 * 2.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trauma_wears_off_the_same_at_30_60_and_144_hz() {
        let mut ends = Vec::new();
        for hz in [30.0f32, 60.0, 144.0] {
            let mut s = CameraShake::default();
            s.add_trauma(1.0);
            for _ in 0..(0.5 * hz).round() as usize {
                s.update(1.0 / hz);
            }
            ends.push((s.trauma, s.offset_2d(), s.angles()));
        }
        for e in &ends[1..] {
            assert!((e.0 - ends[0].0).abs() < 1e-4);
            assert!(e.1.abs_diff_eq(ends[0].1, 1e-2), "{} vs {}", e.1, ends[0].1);
            assert!(e.2.abs_diff_eq(ends[0].2, 1e-4));
        }
        assert!((ends[0].0 - 0.5).abs() < 1e-4);
    }

    #[test]
    fn the_shake_stays_within_its_limits_and_stops_without_trauma() {
        let mut s = CameraShake::default();
        s.add_trauma(1.0);
        s.decay = 0.0;
        let mut biggest = Vec2::ZERO;
        for _ in 0..2000 {
            s.update(1.0 / 144.0);
            let o = s.offset_2d();
            assert!(o.abs().cmple(s.max_offset + 1e-3).all(), "{o}");
            assert!(s.angles().abs().cmple(s.max_angles + 1e-5).all());
            biggest = biggest.max(o.abs());
        }
        assert!(biggest.min_element() > 3.0, "the noise reaches out: {biggest}");
        s.trauma = 0.0;
        assert_eq!((s.offset_2d(), s.angles()), (Vec2::ZERO, Vec3::ZERO));
    }

    #[test]
    fn the_shake_moves_smoothly() {
        let mut s = CameraShake::default();
        s.add_trauma(1.0);
        s.decay = 0.0;
        let mut last = s.offset_2d();
        for _ in 0..1000 {
            s.update(1.0 / 1000.0);
            let o = s.offset_2d();
            assert!(o.distance(last) < 2.0, "a jump from {last} to {o}");
            last = o;
        }
    }

    #[test]
    fn different_seeds_shake_differently() {
        let mut a = CameraShake::default();
        let mut b = CameraShake { seed: 7, ..a };
        for s in [&mut a, &mut b] {
            s.add_trauma(1.0);
            s.update(0.37);
        }
        assert_ne!(a.offset_2d(), b.offset_2d());
    }
}
