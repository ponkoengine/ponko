//! Saving and loading the 2D physics world: [`Physics::save_state`] and
//! [`Physics::restore_state`], for rollback and replays.

use rapier2d::prelude as rp;

use super::{Link, Physics};
use crate::physics_links::LinkTable;
use glam::Vec2;

/// A copy of the 2D physics world ([`Physics::save_state`]): every body,
/// collider and joint, the contacts, islands and broad phase, what the
/// engine knows of each entity's body, gravity and the step count. Loading
/// it ([`Physics::restore_state`]) puts the world back exactly: stepped from
/// there, it moves bit for bit as it did after the save.
///
/// Keep one and save into it again: it reuses its memory where it can.
/// A state never saved into is an empty world without gravity.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::d2::{Physics, PhysicsState};
///
/// let physics = Physics::new(vec2(0.0, 980.0));
/// let mut state = PhysicsState::default();
/// physics.save_state(&mut state);
/// let mut other = Physics::default();
/// other.restore_state(&state);
/// assert_eq!(other.gravity(), vec2(0.0, 980.0));
/// ```
#[derive(Default)]
pub struct PhysicsState {
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
    links: LinkTable<Link>,
    gravity: Vec2,
    step: u64,
    paused: bool,
    world_loads: u64,
}

impl std::fmt::Debug for PhysicsState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PhysicsState")
            .field("bodies", &self.bodies.len())
            .field("step", &self.step)
            .finish_non_exhaustive()
    }
}

impl Physics {
    /// Copies the physics world into `into` (see [`PhysicsState`]). Save
    /// it with the ECS world ([`World::save_snapshot`](ponko_ecs::World::save_snapshot))
    /// between steps: the bodies follow the components, and the two are
    /// restored together.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::d2::{Physics, PhysicsState};
    ///
    /// let mut physics = Physics::new(Vec2::ZERO);
    /// let mut state = PhysicsState::default();
    /// physics.save_state(&mut state);
    /// physics.set_gravity(vec2(0.0, 10.0));
    /// physics.restore_state(&state);
    /// assert_eq!(physics.gravity(), Vec2::ZERO);
    /// ```
    pub fn save_state(&self, into: &mut PhysicsState) {
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
        into.links.clone_from(&self.links);
        into.gravity = self.gravity;
        into.step = self.step;
        into.paused = self.paused;
        into.world_loads = self.world_loads;
    }

    /// Makes the physics world what it was when `from` was saved. Queries
    /// answer as they did then; the next step moves bodies exactly as the
    /// step after the save did, given the same components.
    ///
    /// Collision events of steps after the save that weren't sent yet are
    /// dropped. Settings that aren't state stay: whether
    /// [`set_paused`](Self::set_paused) holds the step is part of the state.
    pub fn restore_state(&mut self, from: &PhysicsState) {
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
        self.links.clone_from(&from.links);
        self.gravity = from.gravity;
        self.step = from.step;
        self.paused = from.paused;
        self.world_loads = from.world_loads;
        while self.collisions.try_recv().is_ok() {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Collider, RigidBody, Velocity, step};
    use super::*;
    use crate::collision::{CollisionStarted, CollisionStopped};
    use crate::d2::Transform as Transform2D;
    use crate::events::Events;
    use glam::vec2;
    use ponko_ecs::{Entity, World, WorldSnapshot};

    const DT: f32 = 1.0 / 60.0;

    /// A floor and a heap of balls and boxes that fall, roll, touch and sleep.
    fn pile(world: &mut World) -> Vec<Entity> {
        let mut entities = vec![world.spawn((
            Transform2D::at(vec2(0.0, 200.0)),
            RigidBody::fixed(),
            Collider::rect(vec2(400.0, 20.0)),
        ))];
        for i in 0..40 {
            let at = vec2((i % 7) as f32 * 9.0 - 30.0, -(i as f32) * 11.0);
            let shape = if i % 3 == 0 {
                Collider::rect(vec2(8.0, 8.0))
            } else {
                Collider::circle(5.0)
            };
            entities.push(world.spawn((Transform2D::at(at), RigidBody::dynamic(), shape, Velocity::default())));
        }
        entities
    }

    /// Every body's pose and velocity, bit for bit, and the step's events.
    type Print = (Vec<[u32; 6]>, Vec<(Entity, Entity, bool)>);

    fn run(physics: &mut Physics, world: &mut World, ticks: usize) -> Vec<Print> {
        (0..ticks)
            .map(|_| {
                let mut events = Events::default();
                step(physics, world, &mut events, DT);
                let mut bodies: Vec<(Entity, [u32; 6])> = world
                    .query::<(Entity, &Transform2D, Option<&Velocity>)>()
                    .iter()
                    .map(|(e, t, v)| {
                        let v = v.copied().unwrap_or_default();
                        let bits = [
                            t.position.x,
                            t.position.y,
                            t.rotation,
                            v.linear.x,
                            v.linear.y,
                            v.angular,
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
    fn a_restored_world_steps_bit_for_bit_like_the_original() {
        let mut physics = Physics::new(vec2(0.0, 980.0));
        let mut world = World::new();
        let bodies = pile(&mut world);
        run(&mut physics, &mut world, 90); // some fall asleep, some still roll
        let (mut snapshot, mut state) = (WorldSnapshot::new(), PhysicsState::default());
        world.save_snapshot(&mut snapshot).unwrap();
        physics.save_state(&mut state);
        let straight = run(&mut physics, &mut world, 60);
        assert!(straight.iter().any(|p| !p.1.is_empty()), "bodies touch after the save");

        // Another history: pushes, a teleport, a body gone and one new.
        for &e in &bodies[1..10] {
            physics.apply_impulse(e, vec2(300.0, -500.0));
        }
        world.get_mut::<Transform2D>(bodies[12]).unwrap().position = vec2(0.0, -300.0);
        world.despawn(bodies[20]);
        world.spawn((
            Transform2D::at(vec2(5.0, -100.0)),
            RigidBody::dynamic(),
            Collider::circle(9.0),
        ));
        physics.set_gravity(vec2(0.0, -50.0));
        run(&mut physics, &mut world, 25);

        world.restore_snapshot(&snapshot).unwrap();
        physics.restore_state(&state);
        assert_eq!(physics.gravity(), vec2(0.0, 980.0));
        let again = run(&mut physics, &mut world, 60);
        assert!(
            again == straight,
            "the restored world moves exactly as it did after the save"
        );

        // And once more, from a state saved into again (memory reused).
        world.restore_snapshot(&snapshot).unwrap();
        physics.restore_state(&state);
        physics.save_state(&mut state);
        assert!(run(&mut physics, &mut world, 60) == straight);
    }

    #[test]
    fn queries_after_a_load_see_the_saved_world() {
        let mut physics = Physics::new(Vec2::ZERO);
        let mut world = World::new();
        let ball = world.spawn((
            Transform2D::at(vec2(0.0, 0.0)),
            RigidBody::fixed(),
            Collider::circle(5.0),
        ));
        run(&mut physics, &mut world, 1);
        let mut state = PhysicsState::default();
        physics.save_state(&mut state);
        world.despawn(ball);
        run(&mut physics, &mut world, 1);
        let mut hits = Vec::new();
        physics.overlap_point(Vec2::ZERO, Default::default(), &mut hits);
        assert!(hits.is_empty());
        physics.restore_state(&state);
        assert_eq!(physics.body_count(), 1);
        physics.overlap_point(Vec2::ZERO, Default::default(), &mut hits);
        assert_eq!(hits, vec![ball]);
        assert!(format!("{state:?}").contains("PhysicsState"));
    }
}
