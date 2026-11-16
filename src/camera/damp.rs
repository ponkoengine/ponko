//! Frame-rate independent smoothing: [`smooth_damp`], [`damp`] and
//! [`damp_angle`].

use std::f32::consts::{PI, TAU};
use std::ops::{Add, Mul, Sub};

/// Moves `current` toward `target` like a critically damped spring: it
/// speeds up, then settles on the target without overshooting it. The
/// usual way to make a camera (or anything else) follow smoothly.
///
/// `velocity` is the spring's speed: keep it between calls (start it at
/// zero) and the motion carries on smoothly when the target moves.
/// `smooth_time` is roughly how long it takes to get most of the way there
/// (after one `smooth_time` from rest, 40% of the way is left; after two,
/// 9%); 0 or less jumps to the target. Works with `f32`, `Vec2`, `Vec3`
/// and anything else that adds, subtracts and scales by an `f32`.
///
/// The step is the spring's exact solution, not an approximation: for a
/// target that holds still, ten steps of 1/60 s land exactly where one of
/// 1/6 s does, so it behaves the same at every frame rate.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::camera::smooth_damp;
///
/// let (mut at, mut speed) = (Vec2::ZERO, Vec2::ZERO);
/// for _ in 0..120 {
///     at = smooth_damp(at, vec2(100.0, 0.0), &mut speed, 0.2, 1.0 / 60.0);
/// }
/// assert!(at.distance(vec2(100.0, 0.0)) < 0.01); // settled, no overshoot
/// ```
#[must_use]
pub fn smooth_damp<T>(current: T, target: T, velocity: &mut T, smooth_time: f32, dt: f32) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    if !positive(smooth_time) {
        *velocity = *velocity * 0.0;
        return target;
    }
    if dt.is_nan() || dt <= 0.0 {
        return current;
    }
    // y(t) = (y0 + c t) e^(-w t), with y the offset from the target and
    // c = v0 + w y0: the critically damped spring, solved exactly.
    let omega = 2.0 / smooth_time;
    let offset = current - target;
    let c = *velocity + offset * omega;
    let decay = (-omega * dt).exp();
    *velocity = (*velocity - c * (omega * dt)) * decay;
    target + (offset + c * dt) * decay
}

/// [`smooth_damp`] toward a target that moved in a straight line from
/// `from` to `to` during the step, solved exactly. A rig that follows a
/// moving thing uses it, so its lag is the same at every frame rate: a
/// target held still for each step would lead the spring by up to a frame.
pub(crate) fn smooth_damp_moving<T>(current: T, from: T, to: T, velocity: &mut T, smooth_time: f32, dt: f32) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    if !positive(smooth_time) || dt.is_nan() || dt <= 0.0 {
        return smooth_damp(current, to, velocity, smooth_time, dt);
    }
    // In the frame of the moving target the spring trails it by a steady
    // 2u/w (u its speed) and is otherwise free: step that exactly.
    let omega = 2.0 / smooth_time;
    let speed = (to - from) * (1.0 / dt);
    let lag = speed * (2.0 / omega);
    let mut relative = *velocity - speed;
    let at = smooth_damp(current + lag, from, &mut relative, smooth_time, dt);
    *velocity = relative + speed;
    // `at` is relative to the start's target; the target moved on by `to - from`.
    at + (to - from) - lag
}

/// [`damp`] toward a target that moved in a straight line from `from` to
/// `to` during the step, solved exactly (see [`smooth_damp_moving`]).
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
pub(crate) fn damp_moving<T>(current: T, from: T, to: T, half_life: f32, dt: f32) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    if !positive(half_life) || dt.is_nan() || dt <= 0.0 {
        return damp(current, to, half_life, dt);
    }
    // Exponential smoothing trails a steadily moving target by u/k.
    let k = std::f32::consts::LN_2 / half_life;
    let lag = (to - from) * (1.0 / (k * dt));
    let decay = (-dt / half_life).exp2();
    to - lag + (current - from + lag) * decay
}

/// Moves `current` toward `target`, covering half of the remaining way
/// every `half_life` seconds (exponential smoothing, the frame-rate
/// independent form of `lerp(current, target, k)` every frame). 0 or less
/// jumps to the target. Keeps no velocity, so it starts moving at once:
/// [`smooth_damp`] eases in too.
///
/// ```
/// use ponko::camera::damp;
///
/// let half = damp(0.0, 10.0, 0.5, 0.5);
/// assert!((half - 5.0).abs() < 1e-5);
/// ```
#[must_use]
pub fn damp<T>(current: T, target: T, half_life: f32, dt: f32) -> T
where
    T: Copy + Add<Output = T> + Sub<Output = T> + Mul<f32, Output = T>,
{
    if !positive(half_life) {
        return target;
    }
    if dt.is_nan() || dt <= 0.0 {
        return current;
    }
    target + (current - target) * (-dt / half_life).exp2()
}

/// [`damp`] for angles in radians: turns the short way round, so going
/// from 350° to 10° passes 0°, not 180°. The result isn't wrapped: it
/// stays near `current`.
///
/// ```
/// use ponko::camera::damp_angle;
///
/// let a = damp_angle(350f32.to_radians(), 10f32.to_radians(), 0.1, 10.0);
/// assert!((a.to_degrees() - 370.0).abs() < 1e-3); // 10°, reached going up
/// ```
#[must_use]
pub fn damp_angle(current: f32, target: f32, half_life: f32, dt: f32) -> f32 {
    let delta = wrap_angle(target - current);
    damp(current, current + delta, half_life, dt)
}

/// `x` is a finite number above 0 (not NaN).
pub(crate) fn positive(x: f32) -> bool {
    x > 0.0 && x.is_finite()
}

/// `angle` in `-π..=π`.
pub(crate) fn wrap_angle(angle: f32) -> f32 {
    let a = (angle + PI).rem_euclid(TAU) - PI;
    if a == -PI { PI } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Vec2, Vec3};

    /// Steps `f` for `seconds` at `hz` and returns where it ends.
    fn run(hz: f32, seconds: f32, mut f: impl FnMut(f32)) {
        let steps = (seconds * hz).round() as usize;
        for _ in 0..steps {
            f(1.0 / hz);
        }
    }

    #[test]
    fn smooth_damp_lands_in_the_same_place_at_30_60_and_144_hz() {
        let mut ends = Vec::new();
        for hz in [30.0, 60.0, 144.0] {
            let (mut at, mut v) = (Vec3::ZERO, Vec3::ZERO);
            run(hz, 0.5, |dt| {
                at = smooth_damp(at, Vec3::new(10.0, -4.0, 2.0), &mut v, 0.3, dt)
            });
            ends.push((at, v));
        }
        for (at, v) in &ends[1..] {
            assert!(at.abs_diff_eq(ends[0].0, 1e-4), "{at} vs {}", ends[0].0);
            assert!(v.abs_diff_eq(ends[0].1, 1e-3), "{v} vs {}", ends[0].1);
        }
    }

    #[test]
    fn smooth_damp_never_overshoots_from_rest() {
        let (mut at, mut v) = (0.0f32, 0.0f32);
        for _ in 0..600 {
            at = smooth_damp(at, 1.0, &mut v, 0.25, 1.0 / 60.0);
            assert!(at <= 1.0 + 1e-6, "{at}");
        }
        assert!((at - 1.0).abs() < 1e-4);
    }

    #[test]
    fn smooth_damp_with_no_time_jumps_and_stops() {
        let mut v = Vec2::new(5.0, 5.0);
        let at = smooth_damp(Vec2::ZERO, Vec2::ONE, &mut v, 0.0, 0.016);
        assert_eq!((at, v), (Vec2::ONE, Vec2::ZERO));
        let mut v = 1.0;
        assert_eq!(smooth_damp(3.0, 9.0, &mut v, 0.2, 0.0), 3.0, "dt 0 holds still");
    }

    #[test]
    fn damp_is_frame_rate_independent() {
        let mut ends = Vec::new();
        for hz in [30.0, 60.0, 144.0] {
            let mut at = 0.0f32;
            run(hz, 1.0, |dt| at = damp(at, 8.0, 0.2, dt));
            ends.push(at);
        }
        assert!(
            (ends[0] - ends[1]).abs() < 1e-4 && (ends[1] - ends[2]).abs() < 1e-4,
            "{ends:?}"
        );
        // After five half-lives, 1/32 of the way is left.
        assert!((ends[0] - (8.0 - 8.0 / 32.0)).abs() < 1e-3);
    }

    #[test]
    fn following_a_moving_target_is_frame_rate_independent() {
        let mut ends = Vec::new();
        for hz in [30.0f32, 60.0, 144.0] {
            let (mut a, mut v, mut b) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
            let path = |t: f32| Vec2::new(50.0 * t, (3.0 * t).sin() * 20.0);
            let n = (hz * 1.5).round() as usize;
            for i in 0..n {
                let (t0, t1) = (i as f32 / hz, (i + 1) as f32 / hz);
                a = smooth_damp_moving(a, path(t0), path(t1), &mut v, 0.2, 1.0 / hz);
                b = damp_moving(b, path(t0), path(t1), 0.1, 1.0 / hz);
            }
            ends.push((a, b));
        }
        for e in &ends[1..] {
            // A curved path is only straight within a step: close, not exact.
            assert!(e.0.distance(ends[0].0) < 0.05, "{ends:?}");
            assert!(e.1.distance(ends[0].1) < 0.05, "{ends:?}");
        }
        // A straight one is exact: a target at 100 units/s is trailed by
        // its steady lag (2u/w = u * smooth_time, and u/k) at every rate.
        for hz in [30.0f32, 144.0] {
            let (mut a, mut v, mut b) = (0.0f32, 0.0f32, 0.0f32);
            for i in 0..(hz * 3.0) as usize {
                let (t0, t1) = (i as f32 / hz, (i + 1) as f32 / hz);
                a = smooth_damp_moving(a, 100.0 * t0, 100.0 * t1, &mut v, 0.2, 1.0 / hz);
                b = damp_moving(b, 100.0 * t0, 100.0 * t1, 0.1, 1.0 / hz);
            }
            assert!((a - (300.0 - 20.0)).abs() < 0.05 && (v - 100.0).abs() < 0.05, "{a} {v}");
            assert!((b - (300.0 - 10.0 / std::f32::consts::LN_2)).abs() < 0.05, "{b}");
        }
    }

    #[test]
    fn damp_angle_turns_the_short_way() {
        let a = damp_angle(170f32.to_radians(), -170f32.to_radians(), 1.0, 1.0);
        assert!((a.to_degrees() - 180.0).abs() < 1e-3, "{}", a.to_degrees());
        assert!((wrap_angle(3.0 * PI) - PI).abs() < 1e-6);
        assert!((wrap_angle(-0.5) + 0.5).abs() < 1e-6);
    }
}
