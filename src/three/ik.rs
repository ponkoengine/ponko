//! Procedural pose corrections, applied after animation.
use super::{Skeleton, Transform3D};
use glam::{Mat4, Quat, Vec3};

fn weight(w: f32) -> f32 {
    if w.is_finite() { w.clamp(0.0, 1.0) } else { 0.0 }
}
fn inverse(m: Mat4) -> Option<Mat4> {
    if !m.is_finite() || m.determinant().abs() < 1e-8 {
        None
    } else {
        Some(m.inverse())
    }
}
fn position(s: &Skeleton, j: usize) -> Vec3 {
    s.joint_matrix(j).transform_point3(Vec3::ZERO)
}
fn rotate_toward(s: &mut Skeleton, j: usize, from: Vec3, to: Vec3, w: f32, max: f32) {
    let parent = s.joints()[j].parent.map_or(Mat4::IDENTITY, |p| s.joint_matrix(p));
    let Some(inv) = inverse(parent) else {
        return;
    };
    let a = inv.transform_vector3(from).normalize_or_zero();
    let b = inv.transform_vector3(to).normalize_or_zero();
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return;
    }
    let delta = Quat::from_rotation_arc(a, b);
    let angle = a.angle_between(b);
    let fraction = if angle > 1e-6 {
        (max.max(0.0) / angle).min(1.0)
    } else {
        1.0
    };
    let old = s.pose[j].rotation;
    s.pose[j].rotation = (Quat::IDENTITY.slerp(delta, fraction * w) * old).normalize();
}

/// Rotates a direct parent-child-child chain toward a world target without
/// changing translations or lengths. `pole` is a world-space bend direction.
/// Weight is clamped to 0..1. Invalid indices, incomplete poses, singular
/// transforms and zero-length bones are ignored. Collinear hints use a stable
/// perpendicular. Targets outside reach use the closest reachable distance.
///
/// Entity non-uniform scale is removed before solving. Exact reach requires
/// uniform positive joint scales; joint non-uniform scale/shear is approximate.
/// No allocations are made. Apply after animation, in a system of
/// [`Phase::Late`](crate::game::Phase::Late).
///
/// ```
/// use ponko::prelude::*;
/// let mut s = ponko::anim::Skeleton::new(vec![]);
/// ponko::anim::two_bone_ik(&mut s, &ponko::d3::Transform::default(), [0, 1, 2], Vec3::Y, Vec3::Z, 1.0);
/// ```
pub fn two_bone_ik(s: &mut Skeleton, model: &Transform3D, chain: [usize; 3], target: Vec3, pole: Vec3, w: f32) {
    let [upper, middle, end] = chain;
    let w = weight(w);
    if w == 0.0
        || chain.iter().any(|&j| j >= s.joints().len() || j >= s.pose.len())
        || s.joints()[middle].parent != Some(upper)
        || s.joints()[end].parent != Some(middle)
    {
        return;
    }
    let Some(inv) = inverse(model.matrix()) else {
        return;
    };
    let target = inv.transform_point3(target);
    let pole = inv.transform_vector3(pole);
    if !target.is_finite() || !pole.is_finite() {
        return;
    }
    let a = position(s, upper);
    let b = position(s, middle);
    let c = position(s, end);
    let l1 = a.distance(b);
    let l2 = b.distance(c);
    if l1 < 1e-6 || l2 < 1e-6 {
        return;
    }
    let direction = (target - a)
        .try_normalize()
        .unwrap_or((c - a).try_normalize().unwrap_or(Vec3::X));
    let bend = (pole - direction * pole.dot(direction))
        .try_normalize()
        .unwrap_or_else(|| direction.any_orthonormal_vector());
    let distance = a.distance(target).clamp((l1 - l2).abs().max(1e-6), l1 + l2);
    let along = (l1 * l1 + distance * distance - l2 * l2) / (2.0 * distance);
    let height = (l1 * l1 - along * along).max(0.0).sqrt();
    let elbow = a + direction * along + bend * height;
    let old_upper = s.pose[upper].rotation;
    let old_middle = s.pose[middle].rotation;
    rotate_toward(s, upper, b - a, elbow - a, 1.0, std::f32::consts::PI);
    let b = position(s, middle);
    let c = position(s, end);
    rotate_toward(
        s,
        middle,
        c - b,
        a + direction * distance - b,
        1.0,
        std::f32::consts::PI,
    );
    s.pose[upper].rotation = old_upper.slerp(s.pose[upper].rotation, w);
    s.pose[middle].rotation = old_middle.slerp(s.pose[middle].rotation, w);
}

/// Aims `axis` (joint-local) at a world point, limiting the correction angle
/// in radians in the joint parent's space, then applying weight (clamped 0..1).
/// Call in parent-to-child order with separate weights to spread over a chain.
/// Zero axes, coincident targets, invalid indices and singular transforms are
/// ignored. Entity scale is handled by inverse transformation; non-uniform
/// joint scales/shear make the angular limit approximate in world space.
///
/// ```
/// use ponko::prelude::*;
/// let mut s = ponko::anim::Skeleton::new(vec![ponko::anim::Joint::new("head", None)]);
/// ponko::anim::look_at_joint(&mut s, &ponko::d3::Transform::default(), 0, Vec3::Z, Vec3::X, 1.0, 0.5);
/// ```
#[allow(clippy::too_many_arguments)]
pub fn look_at_joint(
    s: &mut Skeleton,
    model: &Transform3D,
    joint: usize,
    axis: Vec3,
    target: Vec3,
    max_angle: f32,
    w: f32,
) {
    let w = weight(w);
    if w == 0.0
        || joint >= s.joints().len()
        || joint >= s.pose.len()
        || !axis.is_finite()
        || !target.is_finite()
        || !max_angle.is_finite()
    {
        return;
    }
    let Some(inv) = inverse(model.matrix()) else {
        return;
    };
    let matrix = s.joint_matrix(joint);
    rotate_toward(
        s,
        joint,
        matrix.transform_vector3(axis),
        inv.transform_point3(target) - matrix.transform_point3(Vec3::ZERO),
        w,
        max_angle,
    );
}

impl crate::ctx::Ctx {
    /// Draws parent-to-child bones as projected 2D lines over the 3D scene.
    /// Uses the existing line primitive: no depth testing; segments crossing
    /// the near/far plane are clipped. Missing components are ignored.
    ///
    /// ```no_run
    /// use ponko::prelude::*;
    /// fn debug(ctx: &mut Ctx, entity: Entity) { ctx.draw_skeleton(entity, Color::RED); }
    /// ```
    pub fn draw_skeleton(&mut self, entity: ponko_ecs::Entity, color: crate::color::Color) {
        let (Some(s), Some(t)) = (
            self.world.get::<Skeleton>(entity),
            self.world.get::<Transform3D>(entity),
        ) else {
            return;
        };
        let count = s.joints().len();
        let model = t.matrix();
        for j in 0..count {
            let s = self.world.get::<Skeleton>(entity).unwrap();
            let Some(p) = s.joints()[j].parent else {
                continue;
            };
            let m = self.scene3d.camera.view_proj() * model;
            let mut a = m * position(s, p).extend(1.0);
            let mut b = m * position(s, j).extend(1.0);
            let mut visible = true;
            for plane in [0, 1] {
                let distance = |v: glam::Vec4| if plane == 0 { v.z } else { v.w - v.z };
                let da = distance(a);
                let db = distance(b);
                if da < 0.0 && db < 0.0 {
                    visible = false;
                    break;
                }
                if da < 0.0 {
                    a = a.lerp(b, da / (da - db));
                } else if db < 0.0 {
                    b = b.lerp(a, db / (db - da));
                }
            }
            if !visible || a.w <= 0.0 || b.w <= 0.0 {
                continue;
            }
            let area = self.scene3d.camera.screen_rect();
            let screen = |v: glam::Vec4| {
                area.min + (glam::Vec2::new(v.x / v.w, -v.y / v.w) + glam::Vec2::ONE) * 0.5 * area.size()
            };
            let a = self.camera.screen_to_world(screen(a));
            let b = self.camera.screen_to_world(screen(b));
            self.gfx.draw_line(a, b, 1.0, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anim::Joint;
    fn arm() -> Skeleton {
        Skeleton::new(
            (0usize..3)
                .map(|i| {
                    Joint::new(i.to_string(), i.checked_sub(1)).with_rest(Transform3D::at(if i == 0 {
                        Vec3::ZERO
                    } else {
                        Vec3::X
                    }))
                })
                .collect(),
        )
    }
    fn end(s: &Skeleton) -> Vec3 {
        s.joint_matrix(2).transform_point3(Vec3::ZERO)
    }
    #[test]
    fn reaches_and_extends() {
        for target in [Vec3::new(1.0, 1.0, 0.0), Vec3::new(4.0, 0.0, 0.0)] {
            let mut s = arm();
            two_bone_ik(&mut s, &Transform3D::default(), [0, 1, 2], target, Vec3::Y, 1.0);
            assert!((end(&s) - target.normalize() * target.length().min(2.0)).length() < 1e-3);
        }
    }
    #[test]
    fn parent_rotations_and_model_scale_are_respected() {
        let mut s = arm();
        let mut joints = s.joints().to_vec();
        joints.insert(
            0,
            Joint::new("parent", None).with_rest(Transform3D {
                rotation: Quat::from_rotation_z(0.7),
                ..Transform3D::default()
            }),
        );
        for j in &mut joints[1..] {
            j.parent = Some(j.parent.map_or(0, |p| p + 1));
        }
        s = Skeleton::new(joints);
        let model = Transform3D {
            position: Vec3::new(3.0, 2.0, 1.0),
            rotation: Quat::from_rotation_y(0.6),
            scale: Vec3::new(2.0, 3.0, 0.5),
        };
        let target = model.matrix().transform_point3(Vec3::new(0.8, 0.9, 0.2));
        two_bone_ik(&mut s, &model, [1, 2, 3], target, Vec3::Z, 1.0);
        let tip = (model.matrix() * s.joint_matrix(3)).transform_point3(Vec3::ZERO);
        assert!(tip.distance(target) < 1e-3);
        look_at_joint(&mut s, &model, 3, Vec3::X, target + Vec3::Y, std::f32::consts::PI, 1.0);
        let aim = (model.matrix() * s.joint_matrix(3))
            .transform_vector3(Vec3::X)
            .normalize();
        assert!(aim.distance(Vec3::Y) < 1e-3);
    }
    #[test]
    fn degenerate_and_collinear_inputs_stay_finite() {
        for target in [Vec3::ZERO, Vec3::X, -Vec3::X, Vec3::X * 8.0] {
            let mut s = arm();
            two_bone_ik(&mut s, &Transform3D::default(), [0, 1, 2], target, Vec3::X, 1.0);
            assert!(s.pose.iter().all(|p| p.rotation.is_finite()));
            s.pose[1].position = Vec3::ZERO;
            two_bone_ik(&mut s, &Transform3D::default(), [0, 1, 2], target, Vec3::ZERO, 1.0);
            assert!(s.pose.iter().all(|p| p.rotation.is_finite()));
        }
    }
    #[test]
    fn zero_weight_preserves_pose() {
        let mut s = arm();
        let old = s.pose.clone();
        two_bone_ik(&mut s, &Transform3D::default(), [0, 1, 2], Vec3::Y, Vec3::Y, 0.0);
        look_at_joint(&mut s, &Transform3D::default(), 0, Vec3::X, Vec3::Y, 1.0, 0.0);
        assert_eq!(s.pose, old);
    }
    #[test]
    fn aim_hits_and_clamps() {
        for angle in [0.4, std::f32::consts::PI] {
            let mut s = arm();
            look_at_joint(&mut s, &Transform3D::default(), 0, Vec3::X, Vec3::Y, angle, 1.0);
            let aim = s.pose[0].rotation * Vec3::X;
            assert!((Vec3::X.angle_between(aim) - angle.min(std::f32::consts::FRAC_PI_2)).abs() < 1e-4);
        }
    }
}
