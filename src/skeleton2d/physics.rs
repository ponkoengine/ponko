//! Physics constraints, as Spine's: a bone that lags, sways and springs
//! back as it moves, stepped at a fixed rate.

use std::sync::Arc;

use glam::{Affine2, Mat2, Vec2};

/// A physics constraint (Spine's): bone `bone` lags behind where the pose
/// puts it and springs back, so hair, tails and cloth sway as the skeleton
/// moves. Its position (`x`, `y`), turn (`rotate`, `shear_x`) and stretch
/// (`scale_x`) each follow a damped spring, stepped `1 / step` times a
/// second whatever the frame rate. Applied after animation, in its turn
/// among the other constraints; each [`Skeleton`](super::Skeleton) keeps
/// its own springs.
///
/// Units are Spine's: `strength` is the spring's stiffness, `damping` the
/// share of speed kept per sixtieth of a second, `mass` the weight that
/// `strength`, `wind` and `gravity` push, `inertia` how much of the bone's
/// own movement it resists (0 none, 1 all), and `wind` (+x) and `gravity`
/// (down, +y) are scaled by `reference_scale`.
///
/// ```
/// use ponko::skeleton2d::PhysicsConstraint;
/// let mut tail = PhysicsConstraint::new("tail", 3);
/// tail.rotate = 1.0; // it swings
/// tail.gravity = 50.0;
/// assert_eq!(tail.strength, 100.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PhysicsConstraint {
    /// Its name, unique among the physics constraints.
    pub name: Arc<str>,
    /// The bone it moves.
    pub bone: usize,
    /// How much its x position lags, 0 to 1.
    pub x: f32,
    /// How much its y position lags, 0 to 1.
    pub y: f32,
    /// How much it turns, 0 to 1.
    pub rotate: f32,
    /// How much it stretches along its length, 0 to 1.
    pub scale_x: f32,
    /// How much its x axis shears, 0 to 1.
    pub shear_x: f32,
    /// The farthest it lags per second of movement, in skeleton units.
    pub limit: f32,
    /// Seconds per simulation step (Spine's `1 / fps`).
    pub step: f32,
    /// How much of the bone's movement it resists, 0 to 1.
    pub inertia: f32,
    /// The spring's stiffness.
    pub strength: f32,
    /// The share of speed kept per sixtieth of a second (1: no damping).
    pub damping: f32,
    /// The weight the forces push (above 0).
    pub mass: f32,
    /// Force along +x.
    pub wind: f32,
    /// Force down (+y).
    pub gravity: f32,
    /// How much it applies, 0 (none) to 1.
    pub mix: f32,
    /// The skeleton's size that `wind` and `gravity` are measured against
    /// (Spine's `referenceScale`, default 100).
    pub reference_scale: f32,
}

impl PhysicsConstraint {
    /// A constraint named `name` on bone `bone` with Spine's defaults: no
    /// axis lags until set, 60 steps a second, inertia 1, strength 100,
    /// damping 1, mass 1, no wind or gravity, fully applied.
    #[must_use]
    pub fn new(name: &str, bone: usize) -> Self {
        Self {
            name: name.into(),
            bone,
            x: 0.0,
            y: 0.0,
            rotate: 0.0,
            scale_x: 0.0,
            shear_x: 0.0,
            limit: 5000.0,
            step: 1.0 / 60.0,
            inertia: 1.0,
            strength: 100.0,
            damping: 1.0,
            mass: 1.0,
            wind: 0.0,
            gravity: 0.0,
            mix: 1.0,
            reference_scale: 100.0,
        }
    }
}

/// A physics constraint's values that clips key: inertia, strength,
/// damping, mass, wind, gravity, mix.
pub(crate) const PHYSICS_VALUES: usize = 7;

impl PhysicsConstraint {
    pub(crate) fn values(&self) -> [f32; PHYSICS_VALUES] {
        [
            self.inertia,
            self.strength,
            self.damping,
            self.mass,
            self.wind,
            self.gravity,
            self.mix,
        ]
    }
}

/// One skeleton's state of one physics constraint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spring {
    reset: bool,
    /// Where the bone was, for inertia.
    u: Vec2,
    /// Where it was after the constraint.
    c: Vec2,
    /// Its tip as posed (its x axis times its length).
    t: Vec2,
    offset: Vec2,
    velocity: Vec2,
    rotate_offset: f32,
    rotate_velocity: f32,
    scale_offset: f32,
    scale_velocity: f32,
    /// Time not yet stepped.
    remaining: f32,
}

impl Default for Spring {
    fn default() -> Self {
        Self {
            reset: true,
            u: Vec2::ZERO,
            c: Vec2::ZERO,
            t: Vec2::ZERO,
            offset: Vec2::ZERO,
            velocity: Vec2::ZERO,
            rotate_offset: 0.0,
            rotate_velocity: 0.0,
            scale_offset: 0.0,
            scale_velocity: 0.0,
            remaining: 0.0,
        }
    }
}

impl Spring {
    /// At rest again: the next update starts it where the bone is.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Advances `spring` by `delta` seconds and moves the bone whose
/// skeleton-space matrix is `m` (rest length `length`), as constraint `c`
/// with keyed values `v` says (Spine's physics, mirrored to y-down: gravity
/// pulls +y and angles turn clockwise).
#[allow(clippy::many_single_char_names)]
pub(crate) fn apply(
    c: &PhysicsConstraint,
    v: [f32; PHYSICS_VALUES],
    length: f32,
    delta: f32,
    spring: &mut Spring,
    m: &mut Affine2,
) {
    let [inertia, strength, damping, mass, wind, gravity, mix] = v;
    if mix == 0.0 {
        return;
    }
    let mass_inverse = if mass > 0.0 { 1.0 / mass } else { 0.0 };
    let (x, y) = (c.x > 0.0, c.y > 0.0);
    let rotate_or_shear = c.rotate > 0.0 || c.shear_x > 0.0;
    let scale_x = c.scale_x > 0.0;
    let l = length;
    spring.remaining += delta.max(0.0);
    let b = m.translation;
    if spring.reset {
        spring.reset = false;
        spring.u = b;
    } else {
        let mut a = spring.remaining;
        let i = inertia;
        let t = c.step;
        let f = c.reference_scale;
        let mut d = -1.0f32;
        let q = c.limit * delta.max(0.0);
        let clamp = |u: f32| u.clamp(-q, q);
        if x || y {
            if x {
                spring.offset.x += clamp((spring.u.x - b.x) * i);
                spring.u.x = b.x;
            }
            if y {
                spring.offset.y += clamp((spring.u.y - b.y) * i);
                spring.u.y = b.y;
            }
            if a >= t && t > 0.0 {
                d = damping.powf(60.0 * t);
                let m = mass_inverse * t;
                let e = strength;
                let w = wind * f;
                let g = gravity * f;
                while a >= t {
                    if x {
                        spring.velocity.x += (w - spring.offset.x * e) * m;
                        spring.offset.x += spring.velocity.x * t;
                        spring.velocity.x *= d;
                    }
                    if y {
                        spring.velocity.y += (g - spring.offset.y * e) * m;
                        spring.offset.y += spring.velocity.y * t;
                        spring.velocity.y *= d;
                    }
                    a -= t;
                }
            }
            if x {
                m.translation.x += spring.offset.x * mix * c.x;
            }
            if y {
                m.translation.y += spring.offset.y * mix * c.y;
            }
        }
        if rotate_or_shear || scale_x {
            let axis = m.matrix2.x_axis;
            let ca = axis.y.atan2(axis.x);
            let mut mr = 0.0;
            let dx = clamp(spring.c.x - m.translation.x);
            let dy = clamp(spring.c.y - m.translation.y);
            let (mut cos, mut sin);
            if rotate_or_shear {
                mr = (c.rotate + c.shear_x) * mix;
                let r = (dy + spring.t.y).atan2(dx + spring.t.x) - ca - spring.rotate_offset * mr;
                spring.rotate_offset += (r - (r / std::f32::consts::TAU - 0.5).ceil() * std::f32::consts::TAU) * i;
                let r = spring.rotate_offset * mr + ca;
                (sin, cos) = r.sin_cos();
                if scale_x {
                    let r = l * axis.length();
                    if r > 0.0 {
                        spring.scale_offset += (dx * cos + dy * sin) * i / r;
                    }
                }
            } else {
                (sin, cos) = ca.sin_cos();
                let r = l * axis.length();
                if r > 0.0 {
                    spring.scale_offset += (dx * cos + dy * sin) * i / r;
                }
            }
            a = spring.remaining;
            if a >= t && t > 0.0 {
                if d == -1.0 {
                    d = damping.powf(60.0 * t);
                }
                let m = mass_inverse * t;
                let e = strength;
                let (w, g) = (wind, gravity);
                let h = l / f;
                loop {
                    a -= t;
                    if scale_x {
                        spring.scale_velocity += (w * cos + g * sin - spring.scale_offset * e) * m;
                        spring.scale_offset += spring.scale_velocity * t;
                        spring.scale_velocity *= d;
                    }
                    if rotate_or_shear {
                        spring.rotate_velocity -= ((w * sin - g * cos) * h + spring.rotate_offset * e) * m;
                        spring.rotate_offset += spring.rotate_velocity * t;
                        spring.rotate_velocity *= d;
                        if a < t {
                            break;
                        }
                        let r = spring.rotate_offset * mr + ca;
                        (sin, cos) = r.sin_cos();
                    } else if a < t {
                        break;
                    }
                }
            }
        }
        spring.remaining = a;
    }
    spring.c = m.translation;
    if rotate_or_shear {
        let o = spring.rotate_offset * mix;
        if c.shear_x > 0.0 {
            let mut r = 0.0;
            if c.rotate > 0.0 {
                r = o * c.rotate;
                m.matrix2.y_axis = Mat2::from_angle(r) * m.matrix2.y_axis;
            }
            r += o * c.shear_x;
            m.matrix2.x_axis = Mat2::from_angle(r) * m.matrix2.x_axis;
        } else {
            m.matrix2 = Mat2::from_angle(o * c.rotate) * m.matrix2;
        }
    }
    if scale_x {
        m.matrix2.x_axis *= 1.0 + spring.scale_offset * mix * c.scale_x;
    }
    spring.t = m.matrix2.x_axis * l;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One step by hand: a bone that jumps 10 units right in one 60th of a
    /// second lags behind and starts springing after it.
    #[test]
    fn a_bone_that_jumps_lags_by_one_spring_step() {
        let mut c = PhysicsConstraint::new("lag", 0);
        c.x = 1.0;
        let mut spring = Spring::default();
        let mut m = Affine2::IDENTITY;
        apply(&c, c.values(), 0.0, 0.0, &mut spring, &mut m);
        assert_eq!(m.translation, Vec2::ZERO);
        let mut m = Affine2::from_translation(Vec2::new(10.0, 0.0));
        apply(&c, c.values(), 0.0, 1.0 / 60.0, &mut spring, &mut m);
        // Inertia 1: offset -10. One step: velocity += (0 - (-10) * 100) *
        // (1 / 60) = 16.667; offset += 16.667 / 60 = -9.7222; the bone
        // shows at 10 - 9.7222.
        assert!(
            (m.translation.x - (10.0 - 10.0 + 1000.0 / 60.0 / 60.0)).abs() < 1e-4,
            "{m}"
        );
        assert_eq!(m.translation.y, 0.0);
    }
}
