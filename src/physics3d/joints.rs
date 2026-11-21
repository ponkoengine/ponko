//! Entity-owned impulse joints. Local X is the hinge, slider and twist axis.
use super::*;
use ponko_ecs::Component;

/// The six axes in the joint's local frames, translations followed by rotations.
/// ```
/// use ponko::prelude::*;
/// assert_eq!(ponko::d3::JointAxis::AngularX as usize, 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JointAxis3D {
    /// Translation along X.
    LinearX,
    /// Translation along Y.
    LinearY,
    /// Translation along Z.
    LinearZ,
    /// Twist around X.
    AngularX,
    /// Rotation around Y.
    AngularY,
    /// Rotation around Z.
    AngularZ,
}

/// Freedom on one joint axis; distances are meters, angles radians.
/// ```
/// use ponko::prelude::*;
/// let motion = ponko::d3::JointMotion::Limited(-0.5, 0.5);
/// assert_ne!(motion, ponko::d3::JointMotion::Locked);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum JointMotion3D {
    /// Unconstrained.
    Free,
    /// No relative movement.
    Locked,
    /// Inclusive lower and upper limits.
    Limited(f32, f32),
}

/// A spring/damper drive. Zero stiffness gives a velocity motor.
/// Rapier's default acceleration motor model is used; targets use meters or radians.
/// ```
/// use ponko::prelude::*;
/// let drive = ponko::d3::JointDrive::velocity(2.0, 10.0);
/// assert_eq!(drive.target_velocity, 2.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JointDrive3D {
    /// Target displacement or angle.
    pub target_position: f32,
    /// Target linear or angular speed.
    pub target_velocity: f32,
    /// Spring coefficient (zero for velocity only).
    pub stiffness: f32,
    /// Damping coefficient.
    pub damping: f32,
    /// Maximum force or torque; infinity means unlimited (`null` in JSON).
    #[serde(with = "unlimited")]
    pub max_force: f32,
}

/// Infinity as JSON `null`, which is what JSON writes for it anyway, so
/// that it reads back.
mod unlimited {
    pub fn serialize<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        serde::Serialize::serialize(&v.is_finite().then_some(*v), s)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        Ok(<Option<f32> as serde::Deserialize>::deserialize(d)?.unwrap_or(f32::INFINITY))
    }
}

impl JointDrive3D {
    /// A velocity motor with damping controlling convergence.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::d3::JointDrive::velocity(3.0, 5.0).stiffness, 0.0);
    /// ```
    pub const fn velocity(target_velocity: f32, damping: f32) -> Self {
        Self {
            target_position: 0.0,
            target_velocity,
            stiffness: 0.0,
            damping,
            max_force: f32::INFINITY,
        }
    }
    /// A spring drive toward a position or angle with zero target speed.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::d3::JointDrive::spring(0.5, 100.0, 20.0).target_position, 0.5);
    /// ```
    pub const fn spring(target_position: f32, stiffness: f32, damping: f32) -> Self {
        Self {
            target_position,
            target_velocity: 0.0,
            stiffness,
            damping,
            max_force: f32::INFINITY,
        }
    }
}

/// Put this component on a separate entity to connect two bodies. Removing that
/// entity/component or either body's physics components removes the joint next step.
/// Missing endpoints are retried on later steps. Multiple joint entities may share bodies.
/// Changing this component rebuilds its joint and wakes both endpoints.
/// ```
/// use ponko::prelude::*;
/// let mut world = World::new();
/// let a = world.spawn((ponko::d3::RigidBody::fixed(),));
/// let b = world.spawn((ponko::d3::RigidBody::dynamic(),));
/// let joint = ponko::d3::Joint::spherical(a, b).motion(ponko::d3::JointAxis::AngularX, ponko::d3::JointMotion::Limited(-0.785, 0.785)).swing_cone(0.524);
/// world.spawn((joint,));
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Component, serde::Serialize, serde::Deserialize)]
pub struct Joint3D {
    /// First body entity.
    pub body_a: Entity,
    /// Second body entity (must differ from the first).
    pub body_b: Entity,
    /// Joint frame in the first body's local space; scale ignored.
    pub frame_a: Transform3D,
    /// Joint frame in the second body's local space; scale ignored.
    pub frame_b: Transform3D,
    /// X/Y/Z translations, then X/Y/Z rotations.
    pub axes: [JointMotion3D; 6],
    /// Drives in the same order as `axes`; locked axes ignore drives.
    pub drives: [Option<JointDrive3D>; 6],
    /// Whether the joined bodies collide (default false).
    pub contacts_enabled: bool,
    /// Optional circular swing limit around local X, in radians. Couples Y/Z
    /// rotation; their individual limits/drives are replaced by the cone.
    pub swing_cone: Option<f32>,
}
impl Joint3D {
    /// Six free axes, identity local frames, collisions disabled.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::generic(a,b).axes, [ponko::d3::JointMotion::Free; 6]);
    /// ```
    pub fn generic(body_a: Entity, body_b: Entity) -> Self {
        Self {
            body_a,
            body_b,
            frame_a: Transform3D::default(),
            frame_b: Transform3D::default(),
            axes: [JointMotion3D::Free; 6],
            drives: [None; 6],
            contacts_enabled: false,
            swing_cone: None,
        }
    }
    /// Locks all six axes.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::fixed(a,b).axes, [ponko::d3::JointMotion::Locked; 6]);
    /// ```
    pub fn fixed(a: Entity, b: Entity) -> Self {
        Self {
            axes: [JointMotion3D::Locked; 6],
            ..Self::generic(a, b)
        }
    }
    /// Ball joint: locks translations, leaves rotations free.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::spherical(a,b).axes[3], ponko::d3::JointMotion::Free);
    /// ```
    pub fn spherical(a: Entity, b: Entity) -> Self {
        let mut j = Self::generic(a, b);
        j.axes[..3].fill(JointMotion3D::Locked);
        j
    }
    /// Hinge around local X. Rotate the anchor frames for another axis.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::revolute(a,b).axes[3], ponko::d3::JointMotion::Free);
    /// ```
    pub fn revolute(a: Entity, b: Entity) -> Self {
        Self::fixed(a, b).motion(JointAxis3D::AngularX, JointMotion3D::Free)
    }
    /// Slider along local X.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::prismatic(a,b).axes[0], ponko::d3::JointMotion::Free);
    /// ```
    pub fn prismatic(a: Entity, b: Entity) -> Self {
        Self::fixed(a, b).motion(JointAxis3D::LinearX, JointMotion3D::Free)
    }
    /// Sets freedom or limits on one axis.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// let j = ponko::d3::Joint::revolute(a,b).motion(ponko::d3::JointAxis::AngularX, ponko::d3::JointMotion::Limited(-1.0,1.0));
    /// assert_eq!(j.axes[3], ponko::d3::JointMotion::Limited(-1.0,1.0));
    /// ```
    #[must_use]
    pub fn motion(mut self, axis: JointAxis3D, motion: JointMotion3D) -> Self {
        self.axes[axis as usize] = motion;
        self
    }
    /// Sets a drive on an unlocked axis.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// let j = ponko::d3::Joint::revolute(a,b).drive(ponko::d3::JointAxis::AngularX, ponko::d3::JointDrive::spring(0.5,100.0,20.0));
    /// assert!(j.drives[3].is_some());
    /// ```
    #[must_use]
    pub fn drive(mut self, axis: JointAxis3D, drive: JointDrive3D) -> Self {
        self.drives[axis as usize] = Some(drive);
        self
    }
    /// Sets a circular swing cone; twist remains independently configurable.
    /// ```
    /// use ponko::prelude::*;
    /// let mut w = World::new(); let a = w.spawn((ponko::d3::RigidBody::dynamic(),)); let b = w.spawn((ponko::d3::RigidBody::dynamic(),));
    /// assert_eq!(ponko::d3::Joint::spherical(a,b).swing_cone(0.5).swing_cone, Some(0.5));
    /// ```
    #[must_use]
    pub fn swing_cone(mut self, radians: f32) -> Self {
        self.swing_cone = Some(radians);
        self
    }

    fn rapier(self) -> rp::GenericJoint {
        let mut j = rp::GenericJoint::new(rp::JointAxesMask::empty());
        j.set_local_frame1(pose_to_rp(self.frame_a.position, self.frame_a.rotation));
        j.set_local_frame2(pose_to_rp(self.frame_b.position, self.frame_b.rotation));
        j.set_contacts_enabled(self.contacts_enabled);
        let axes = [
            rp::JointAxis::LinX,
            rp::JointAxis::LinY,
            rp::JointAxis::LinZ,
            rp::JointAxis::AngX,
            rp::JointAxis::AngY,
            rp::JointAxis::AngZ,
        ];
        for (i, axis) in axes.into_iter().enumerate() {
            if i >= 4 && self.swing_cone.is_some() {
                continue;
            }
            match self.axes[i] {
                JointMotion3D::Locked => {
                    j.locked_axes |= axis.into();
                    continue;
                }
                JointMotion3D::Limited(min, max) => {
                    j.set_limits(axis, [min, max]);
                }
                JointMotion3D::Free => {}
            }
            if let Some(d) = self.drives[i] {
                j.set_motor(axis, d.target_position, d.target_velocity, d.stiffness, d.damping);
                j.set_motor_max_force(axis, d.max_force);
            }
        }
        if let Some(angle) = self.swing_cone {
            j.coupled_axes = rp::JointAxesMask::ANG_Y | rp::JointAxesMask::ANG_Z;
            j.set_limits(rp::JointAxis::AngY, [0.0, angle]);
        }
        j
    }
}

#[derive(Clone)]
pub(super) struct Link {
    entity: Entity,
    config: Joint3D,
    a: rp::RigidBodyHandle,
    b: rp::RigidBodyHandle,
    handle: rp::ImpulseJointHandle,
    seen: u64,
}

/// Removes every joint (their bodies are going too); the next sync makes
/// them again from the components.
pub(super) fn forget(physics: &mut Physics3D) {
    for joint in physics.joints.drain(..) {
        physics.world.remove_impulse_joint(joint.handle);
    }
}

pub(super) fn sync(physics: &mut Physics3D, world: &mut World) {
    world.query::<(Entity, &Joint3D)>().for_each(|(entity, config)| {
        let (Some(a), Some(b)) = (physics.link(config.body_a), physics.link(config.body_b)) else {
            return;
        };
        let (a, b) = (a.body, b.body);
        if a == b {
            return;
        }
        if let Some(old) = physics.joints.iter_mut().find(|j| j.entity == entity) {
            if old.config == *config
                && old.a == a
                && old.b == b
                && physics.world.impulse_joints.get(old.handle).is_some()
            {
                old.seen = physics.step;
                return;
            }
            physics.world.remove_impulse_joint(old.handle);
            old.seen = 0;
        }
        let handle = physics.world.insert_impulse_joint(a, b, config.rapier());
        physics.joints.push(Link {
            entity,
            config: *config,
            a,
            b,
            handle,
            seen: physics.step,
        });
    });
    physics.joints.retain(|j| {
        if j.seen == physics.step {
            true
        } else {
            physics.world.remove_impulse_joint(j.handle);
            false
        }
    });
}

impl Physics3D {
    /// Number of live impulse joints after the most recent physics step.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::d3::Physics::default().joint_count(), 0);
    /// ```
    pub fn joint_count(&self) -> usize {
        self.world.impulse_joints.len()
    }
}
