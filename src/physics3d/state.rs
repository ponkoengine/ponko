//! Saving and loading the 3D physics world: [`Physics3D::save_state`] and
//! [`Physics3D::load_state`], for rollback and replays.

use rapier3d::prelude as rp;

use super::{ContactModifier3D, Link, Physics3D, joints};
use crate::physics_links::LinkTable;
use glam::Vec3;
use ponko_ecs::Entity;

/// A copy of the 3D physics world ([`Physics3D::save_state`]): every body,
/// collider and joint, the contacts, islands and broad phase, what the
/// engine knows of each entity's body, joint and contact rule, which
/// streamed colliders are in, gravity and the step count. Loading it
/// ([`Physics3D::load_state`]) puts the world back exactly: stepped from
/// there, it moves bit for bit as it did after the save.
///
/// Registered collision meshes, collider streaming's settings and the
/// contact hook are settings, not state: they stay as they are.
///
/// Keep one and save into it again: it reuses its memory where it can.
/// A state never saved into is an empty world without gravity.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::{Physics, PhysicsState};
///
/// let physics = Physics::new(vec3(0.0, -9.81, 0.0));
/// let mut state = PhysicsState::default();
/// physics.save_state(&mut state);
/// let mut other = Physics::default();
/// other.load_state(&state);
/// assert_eq!(other.gravity(), vec3(0.0, -9.81, 0.0));
/// ```
#[derive(Default)]
pub struct Physics3DState {
    params: rp::IntegrationParameters,
    islands: rp::IslandManager,
    broad_phase: rp::BroadPhaseBvh,
    narrow_phase: rp::NarrowPhase,
    bodies: rp::RigidBodySet,
    colliders: rp::ColliderSet,
    impulse_joints: rp::ImpulseJointSet,
    multibody_joints: rp::MultibodyJointSet,
    soft_bodies: rp::SoftBodySet,
    ccd: rp::CCDSolver,
    joints: Vec<joints::Link>,
    links: LinkTable<Link>,
    moving: Vec<u32>,
    moving_dirty: bool,
    statics: Vec<u32>,
    streamed_out: bool,
    dominance: bool,
    gravity: Vec3,
    step: u64,
    paused: bool,
    world_loads: u64,
    contact_table: Vec<Option<(Entity, ContactModifier3D)>>,
    contact_hooked: Vec<(Entity, ContactModifier3D)>,
}

impl std::fmt::Debug for Physics3DState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsState")
            .field("bodies", &self.bodies.len())
            .field("step", &self.step)
            .finish_non_exhaustive()
    }
}

impl Physics3D {
    /// Copies the physics world into `into` (see
    /// [`PhysicsState`](crate::d3::PhysicsState)). Save it with the ECS
    /// world ([`World::save_snapshot`](ponko_ecs::World::save_snapshot))
    /// between steps: bodies follow the components, and the two are
    /// restored together.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::d3::{Physics, PhysicsState};
    ///
    /// let mut physics = Physics::new(Vec3::ZERO);
    /// let mut state = PhysicsState::default();
    /// physics.save_state(&mut state);
    /// physics.set_gravity(vec3(0.0, -1.0, 0.0));
    /// physics.load_state(&state);
    /// assert_eq!(physics.gravity(), Vec3::ZERO);
    /// ```
    pub fn save_state(&self, into: &mut Physics3DState) {
        let w = &self.world;
        into.params = w.integration_parameters;
        into.islands.clone_from(&w.islands);
        into.broad_phase.clone_from(&w.broad_phase);
        into.narrow_phase.clone_from(&w.narrow_phase);
        into.bodies.clone_from(&w.bodies);
        into.colliders.clone_from(&w.colliders);
        into.impulse_joints.clone_from(&w.impulse_joints);
        into.multibody_joints.clone_from(&w.multibody_joints);
        into.soft_bodies.clone_from(&w.soft_bodies);
        into.ccd.clone_from(&w.ccd_solver);
        into.joints.clone_from(&self.joints);
        into.links.clone_from(&self.links);
        into.moving.clone_from(&self.moving);
        into.moving_dirty = self.moving_dirty;
        into.statics.clone_from(&self.statics);
        into.streamed_out = self.streamed_out;
        into.dominance = self.dominance;
        into.gravity = self.gravity;
        into.step = self.step;
        into.paused = self.paused;
        into.world_loads = self.world_loads;
        into.contact_table.clone_from(&self.contact_table);
        into.contact_hooked.clone_from(&self.contact_hooked);
    }

    /// Makes the physics world what it was when `from` was saved. Queries
    /// answer as they did then; the next step moves bodies exactly as the
    /// step after the save did, given the same components.
    ///
    /// Collision events of steps after the save that weren't sent yet are
    /// dropped.
    pub fn load_state(&mut self, from: &Physics3DState) {
        let w = &mut self.world;
        w.integration_parameters = from.params;
        w.islands.clone_from(&from.islands);
        w.broad_phase.clone_from(&from.broad_phase);
        w.narrow_phase.clone_from(&from.narrow_phase);
        w.bodies.clone_from(&from.bodies);
        w.colliders.clone_from(&from.colliders);
        w.impulse_joints.clone_from(&from.impulse_joints);
        w.multibody_joints.clone_from(&from.multibody_joints);
        w.soft_bodies.clone_from(&from.soft_bodies);
        w.ccd_solver.clone_from(&from.ccd);
        // The pipeline keeps only scratch buffers, but the joint set
        // remembers which of its selections the pipeline's buffer holds: a
        // new pipeline makes the next step select again.
        w.physics_pipeline = rp::PhysicsPipeline::new();
        self.joints.clone_from(&from.joints);
        self.links.clone_from(&from.links);
        self.moving.clone_from(&from.moving);
        self.moving_dirty = from.moving_dirty;
        self.statics.clone_from(&from.statics);
        self.streamed_out = from.streamed_out;
        self.dominance = from.dominance;
        self.gravity = from.gravity;
        self.step = from.step;
        self.paused = from.paused;
        self.world_loads = from.world_loads;
        self.contact_table.clone_from(&from.contact_table);
        self.contact_hooked.clone_from(&from.contact_hooked);
        self.events.clear();
        self.collector
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Collider3D, RigidBody3D, Velocity3D, step};
    use super::*;
    use crate::collision::{CollisionStarted, CollisionStopped};
    use crate::d3::Transform as Transform3D;
    use crate::events::Events;
    use crate::physics3d::Joint3D;
    use glam::vec3;
    use ponko_ecs::{World, WorldSnapshot};

    const DT: f32 = 1.0 / 60.0;

    /// A floor with a conveyor strip, a stack of boxes and balls, and a
    /// chain of three links hanging from a fixed post by joints.
    fn scene(world: &mut World) -> Vec<Entity> {
        let mut entities = vec![
            world.spawn((
                Transform3D::at(vec3(0.0, -0.5, 0.0)),
                RigidBody3D::fixed(),
                Collider3D::cuboid(vec3(20.0, 1.0, 20.0)),
            )),
            world.spawn((
                Transform3D::at(vec3(4.0, 0.05, 0.0)),
                RigidBody3D::fixed(),
                Collider3D::cuboid(vec3(3.0, 0.1, 1.0)),
                ContactModifier3D::conveyor(vec3(1.5, 0.0, 0.0)),
            )),
        ];
        for i in 0..24 {
            let at = vec3(
                (i % 4) as f32 * 0.6 - 1.0,
                0.6 + (i / 4) as f32 * 0.7,
                (i % 3) as f32 * 0.3,
            );
            let shape = if i % 2 == 0 {
                Collider3D::cuboid(vec3(0.5, 0.5, 0.5))
            } else {
                Collider3D::sphere(0.3)
            };
            entities.push(world.spawn((
                Transform3D::at(at),
                RigidBody3D::dynamic(),
                shape,
                Velocity3D::default(),
            )));
        }
        let post = world.spawn((
            Transform3D::at(vec3(-4.0, 5.0, 0.0)),
            RigidBody3D::fixed(),
            Collider3D::sphere(0.1),
        ));
        let mut last = post;
        for i in 0..3 {
            let link = world.spawn((
                Transform3D::at(vec3(-4.0 + (i + 1) as f32, 5.0, 0.0)),
                RigidBody3D::dynamic(),
                Collider3D::capsule(0.6, 0.15),
            ));
            world.spawn((Joint3D::spherical(last, link),));
            entities.push(link);
            last = link;
        }
        entities.push(world.spawn((
            Transform3D::at(vec3(4.0, 0.6, 0.0)),
            RigidBody3D::dynamic(),
            Collider3D::cuboid(vec3(0.4, 0.4, 0.4)),
        )));
        entities
    }

    type Print = (Vec<[u32; 13]>, Vec<(Entity, Entity, bool)>);

    fn run(physics: &mut Physics3D, world: &mut World, ticks: usize) -> Vec<Print> {
        (0..ticks)
            .map(|_| {
                let mut events = Events::default();
                step(physics, world, &mut events, DT);
                let mut bodies: Vec<(Entity, [u32; 13])> = world
                    .query::<(Entity, &Transform3D, Option<&Velocity3D>)>()
                    .iter()
                    .map(|(e, t, v)| {
                        let v = v.copied().unwrap_or_default();
                        let (p, r) = (t.position, t.rotation);
                        let bits = [
                            p.x,
                            p.y,
                            p.z,
                            r.x,
                            r.y,
                            r.z,
                            r.w,
                            v.linear.x,
                            v.linear.y,
                            v.linear.z,
                            v.angular.x,
                            v.angular.y,
                            v.angular.z,
                        ];
                        (e, bits.map(f32::to_bits))
                    })
                    .collect();
                bodies.sort_by_key(|b| b.0.to_bits());
                let mut contacts: Vec<_> = events
                    .peek::<CollisionStarted>()
                    .iter()
                    .map(|c| (c.a, c.b, true))
                    .collect();
                contacts.extend(events.peek::<CollisionStopped>().iter().map(|c| (c.a, c.b, false)));
                (bodies.into_iter().map(|b| b.1).collect(), contacts)
            })
            .collect()
    }

    #[test]
    fn a_restored_3d_world_with_joints_steps_bit_for_bit_like_the_original() {
        let mut physics = Physics3D::new(vec3(0.0, -9.81, 0.0));
        let mut world = World::new();
        let bodies = scene(&mut world);
        run(&mut physics, &mut world, 45);
        let (mut snapshot, mut state) = (WorldSnapshot::new(), Physics3DState::default());
        world.save_snapshot(&mut snapshot).unwrap();
        physics.save_state(&mut state);
        let straight = run(&mut physics, &mut world, 60);
        assert!(straight.iter().any(|p| !p.1.is_empty()), "bodies touch after the save");

        // Another history: pushes, a link gone (its joint breaks), a new body.
        for &e in &bodies[2..12] {
            physics.apply_impulse(e, vec3(2.0, 3.0, -1.0));
        }
        let n = bodies.len();
        world.despawn(bodies[n - 2]);
        world.spawn((
            Transform3D::at(vec3(0.0, 4.0, 0.0)),
            RigidBody3D::dynamic(),
            Collider3D::sphere(0.5),
        ));
        run(&mut physics, &mut world, 30);

        world.restore_snapshot(&snapshot).unwrap();
        physics.load_state(&state);
        let again = run(&mut physics, &mut world, 60);
        assert!(
            again == straight,
            "the restored world moves exactly as it did after the save"
        );
        let mut hits = Vec::new();
        physics.overlap_point(vec3(0.0, -0.5, 0.0), Default::default(), &mut hits);
        assert_eq!(hits, vec![bodies[0]]);
        assert!(format!("{state:?}").contains("PhysicsState"));
    }
}
