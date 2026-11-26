//! Deprecated: the 3D physics moved to [`d3`](crate::d3), under the names
//! `d3` already gave it (`Collider3D` is `d3::Collider`, ...), and its
//! systems to [`systems`](crate::systems). These are 0.1's names, kept as
//! deprecated aliases until 0.3.0 removes the module: a game written for 0.1
//! builds, and the compiler names the replacement of each.
//!
//! An alias can't build a tuple struct or a unit struct (`Alias(..)`), so
//! such constructors need the new name at once; every other use (types,
//! associated functions, struct literals, enum variants) goes through.
//!
//! ```
//! #![allow(deprecated)]
//! use ponko::physics3d::{Collider3D, PHYSICS3D_HZ};
//! let ball = Collider3D::sphere(0.5);
//! assert_eq!(PHYSICS3D_HZ, ponko::d3::Physics::HZ);
//! ```
//!
//! The same code warns (here: fails, with the warning denied):
//!
//! ```compile_fail
//! #![deny(deprecated)]
//! use ponko::physics3d::{Collider3D, PHYSICS3D_HZ};
//! let ball = Collider3D::sphere(0.5);
//! assert_eq!(PHYSICS3D_HZ, ponko::d3::Physics::HZ);
//! ```

/// Moved: [`ponko::d3::Character`](crate::d3::Character).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Character`")]
pub type Character3D = crate::d3::Character;
/// Moved: [`ponko::d3::Collider`](crate::d3::Collider).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Collider`")]
pub type Collider3D = crate::d3::Collider;
/// Moved: [`ponko::d3::ColliderFocus`](crate::d3::ColliderFocus).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::ColliderFocus`")]
pub type ColliderFocus = crate::d3::ColliderFocus;
/// Moved: [`ponko::d3::CollisionGroups`](crate::d3::CollisionGroups).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::CollisionGroups`")]
pub type CollisionGroups = crate::d3::CollisionGroups;
/// Moved: [`ponko::d3::CollisionMesh`](crate::d3::CollisionMesh).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::CollisionMesh`")]
pub type CollisionMesh = crate::d3::CollisionMesh;
/// Moved: [`ponko::d3::CollisionMeshData`](crate::d3::CollisionMeshData).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::CollisionMeshData`")]
pub type CollisionMeshData = crate::d3::CollisionMeshData;
/// Moved: [`ponko::d3::ContactModifier`](crate::d3::ContactModifier).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::ContactModifier`")]
pub type ContactModifier3D = crate::d3::ContactModifier;
/// Moved: [`ponko::d3::ContactPair`](crate::d3::ContactPair).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::ContactPair`")]
pub type ContactPair3D<'a, 'b> = crate::d3::ContactPair<'a, 'b>;
/// Moved: [`ponko::d3::FrictionCurve`](crate::d3::FrictionCurve).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::FrictionCurve`")]
pub type FrictionCurve = crate::d3::FrictionCurve;
/// Moved: [`ponko::d3::Hit`](crate::d3::Hit).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Hit`")]
pub type Hit3D = crate::d3::Hit;
/// Moved: [`ponko::d3::Joint`](crate::d3::Joint).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Joint`")]
pub type Joint3D = crate::d3::Joint;
/// Moved: [`ponko::d3::JointAxis`](crate::d3::JointAxis).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::JointAxis`")]
pub type JointAxis3D = crate::d3::JointAxis;
/// Moved: [`ponko::d3::JointDrive`](crate::d3::JointDrive).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::JointDrive`")]
pub type JointDrive3D = crate::d3::JointDrive;
/// Moved: [`ponko::d3::JointMotion`](crate::d3::JointMotion).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::JointMotion`")]
pub type JointMotion3D = crate::d3::JointMotion;
/// Moved: [`ponko::d3::Physics`](crate::d3::Physics).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Physics`")]
pub type Physics3D = crate::d3::Physics;
/// Moved: [`ponko::d3::QueryFilter`](crate::d3::QueryFilter).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::QueryFilter`")]
pub type QueryFilter3D = crate::d3::QueryFilter;
/// Moved: [`ponko::d3::Ragdoll`](crate::d3::Ragdoll).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Ragdoll`")]
pub type Ragdoll3D = crate::d3::Ragdoll;
/// Moved: [`ponko::d3::RagdollBone`](crate::d3::RagdollBone).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::RagdollBone`")]
pub type RagdollBone = crate::d3::RagdollBone;
/// Moved: [`ponko::d3::RigidBody`](crate::d3::RigidBody).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::RigidBody`")]
pub type RigidBody3D = crate::d3::RigidBody;
/// Moved: [`ponko::d3::Shape`](crate::d3::Shape).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Shape`")]
pub type Shape3D = crate::d3::Shape;
/// Moved: [`ponko::d3::TerrainCollider`](crate::d3::TerrainCollider).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::TerrainCollider`")]
pub type TerrainCollider = crate::d3::TerrainCollider;
/// Moved: [`ponko::d3::Vehicle`](crate::d3::Vehicle).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Vehicle`")]
pub type Vehicle3D = crate::d3::Vehicle;
/// Moved: [`ponko::d3::Velocity`](crate::d3::Velocity).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Velocity`")]
pub type Velocity3D = crate::d3::Velocity;
/// Moved: [`ponko::d3::Wheel`](crate::d3::Wheel).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Wheel`")]
pub type Wheel3D = crate::d3::Wheel;

/// Moved: [`ponko::d3::Physics::HZ`](crate::d3::Physics::HZ).
#[deprecated(since = "0.2.0", note = "use `ponko::d3::Physics::HZ`")]
pub const PHYSICS3D_HZ: u32 = crate::d3::Physics::HZ;

/// Moved: [`ponko::systems::step_physics3d`](crate::systems::step_physics3d).
#[deprecated(since = "0.2.0", note = "use `ponko::systems::step_physics3d`")]
pub fn step_physics3d(ctx: &mut crate::ctx::Ctx) {
    crate::systems::step_physics3d(ctx)
}

/// Moved: [`ponko::systems::interpolate_physics3d`](crate::systems::interpolate_physics3d).
#[deprecated(since = "0.2.0", note = "use `ponko::systems::interpolate_physics3d`")]
pub fn interpolate_physics3d(ctx: &mut crate::ctx::Ctx) {
    crate::systems::interpolate_physics3d(ctx)
}

/// Moved: [`ponko::systems::step_physics3d_access`](crate::systems::step_physics3d_access).
#[deprecated(since = "0.2.0", note = "use `ponko::systems::step_physics3d_access`")]
#[must_use]
pub fn step_physics3d_access() -> crate::access::SystemAccess {
    crate::systems::step_physics3d_access()
}

/// Moved: [`ponko::systems::interpolate_physics3d_access`](crate::systems::interpolate_physics3d_access).
#[deprecated(since = "0.2.0", note = "use `ponko::systems::interpolate_physics3d_access`")]
#[must_use]
pub fn interpolate_physics3d_access() -> crate::access::SystemAccess {
    crate::systems::interpolate_physics3d_access()
}
