//! The character controller: a capsule that walks, climbs slopes and
//! stairs, sticks to the ground going downhill and pushes things.

use rapier3d::control::{CharacterAutostep, CharacterCollision, CharacterLength, KinematicCharacterController};
use rapier3d::prelude as rp;
use serde::{Deserialize, Serialize};

use super::{CollisionGroups, LinkKind, Physics3D, from_rp, to_rp};
use glam::Vec3;
use ponko_ecs::{Component, Entity, World};

/// A walking character (a player, a person): an upright capsule moved by
/// its [`velocity`](Self::velocity) that slides along walls, climbs slopes
/// up to [`max_slope`](Self::max_slope) and steps up to
/// [`step_height`](Self::step_height), stays on the ground going downhill,
/// falls by gravity and pushes dynamic bodies. Needs a
/// [`d3::Transform`](crate::d3::Transform), whose position is at its feet.
///
/// Set `velocity`'s X and Z each tick to walk; Y keeps falling speed (set
/// it to jump). After the step, `grounded` says whether it stands on
/// something. Only the position is the physics': turn the transform freely
/// (to face where the player looks).
///
/// ```
/// use ponko::prelude::*;
/// let mut player = ponko::d3::Character::new(1.8, 0.3);
/// player.velocity.x = 4.0; // walk right at 4 m/s
/// if player.grounded {
///     player.velocity.y = 5.0; // jump
/// }
/// assert_eq!(player.max_slope, 45f32.to_radians());
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Component)]
#[serde(default)]
pub struct Character3D {
    /// From feet to head, in meters (at least twice the radius).
    pub height: f32,
    /// The capsule's radius.
    pub radius: f32,
    /// The steepest ground it walks up, in radians (default 45°). It slides
    /// down anything steeper.
    pub max_slope: f32,
    /// The highest step it climbs without jumping (default 0.35 m); 0 for none.
    pub step_height: f32,
    /// How far down it follows the ground (down slopes and stairs) instead
    /// of flying off it (default 0.3 m); 0 for never.
    pub snap_distance: f32,
    /// How hard it pushes dynamic bodies it walks into, as a mass in kg
    /// (default 80); 0 for not at all.
    pub push_mass: f32,
    /// 1 falls by the world's gravity, 0 floats.
    pub gravity_scale: f32,
    /// Which colliders it touches.
    pub groups: CollisionGroups,
    /// Meters per second. Yours to set; the step adds gravity and clears
    /// the fall on landing and on hitting a ceiling.
    pub velocity: Vec3,
    /// Stands on something, as of the last step.
    pub grounded: bool,
}

impl Default for Character3D {
    fn default() -> Self {
        Self::new(1.8, 0.3)
    }
}

impl Character3D {
    /// A character `height` tall (feet to head) and `radius` wide, with the
    /// default limits.
    #[must_use]
    pub fn new(height: f32, radius: f32) -> Self {
        Self {
            height,
            radius,
            max_slope: 45f32.to_radians(),
            step_height: 0.35,
            snap_distance: 0.3,
            push_mass: 80.0,
            gravity_scale: 1.0,
            groups: CollisionGroups::ALL,
            velocity: Vec3::ZERO,
            grounded: false,
        }
    }
}

/// What a character's body is made of: change these and it's rebuilt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Capsule {
    radius: f32,
    half_height: f32,
    groups: CollisionGroups,
}

/// The gap kept between a character and the ground or walls.
const SKIN: f32 = 0.01;

impl Capsule {
    pub(super) fn of(c: &Character3D) -> Self {
        let radius = c.radius.max(0.01);
        Self {
            radius,
            half_height: (c.height * 0.5 - radius).max(0.0),
            groups: c.groups,
        }
    }
    fn center(self) -> f32 {
        self.half_height + self.radius
    }
    fn shape(self) -> rp::Capsule {
        rp::Capsule::new_y(self.half_height, self.radius)
    }
    pub(super) fn collider(self, e: Entity) -> rp::Collider {
        rp::ColliderBuilder::new(rp::SharedShape::new(self.shape()))
            .translation(rp::Vector::new(0.0, self.center(), 0.0))
            .collision_groups(self.groups.to_rp())
            .active_events(rp::ActiveEvents::COLLISION_EVENTS)
            .user_data(u128::from(e.to_bits()))
            .build()
    }
}

/// Memory the controller reuses every step: the engine's share of a step
/// allocates nothing once these have grown. What is left per character per
/// tick (about 8 small allocations) is inside rapier's
/// `KinematicCharacterController::move_shape`, which builds fresh contact
/// lists on every call; it takes no caller-provided buffers, so only an
/// upstream change can remove it.
#[derive(Default)]
pub(super) struct Scratch {
    plans: Vec<Option<Plan>>,
    characters: Vec<(Entity, Character3D)>,
    /// One buffer per character, parallel to `characters`: the dynamic
    /// bodies each one walked into this step. Emptied and refilled every
    /// step, so they keep their capacity instead of allocating again.
    hits: Vec<Vec<CharacterCollision>>,
}

/// One character's move, worked out against the world as it stood at the
/// start of the step.
pub(super) struct Plan {
    body: Option<rp::RigidBodyHandle>,
    controller: KinematicCharacterController,
    shape: rp::Capsule,
    filter_groups: rp::InteractionGroups,
    feet: Vec3,
    velocity: Vec3,
    desired: Vec3,
    moved: rapier3d::control::EffectiveCharacterMovement,
}

/// Fewer characters than this move on one thread: splitting the work
/// costs more than it saves. Measured with `ponko_bench --bin characters`
/// (walking on a flat floor, Apple M4 Pro, 12 cores): one thread is faster
/// at 8 (0.036 vs 0.044 ms a tick), the two tie at 12, and the pool wins
/// from 16 (0.053 vs 0.044 ms; 0.107 vs 0.059 ms at 32).
const PARALLEL_FROM: usize = 16;

/// Works out `c`'s move. Fills `hits` (emptied first) with the dynamic
/// bodies it walked into, if it pushes.
fn plan(physics: &Physics3D, e: Entity, c: &Character3D, dt: f32, hits: &mut Vec<CharacterCollision>) -> Option<Plan> {
    let link = physics.link(e)?;
    let LinkKind::Character(capsule) = link.kind else {
        return None;
    };
    let body = link.body;
    let feet = physics.world.bodies.get(body).map(|b| from_rp(b.translation()))?;
    hits.clear();
    let plan = plan_from(physics, Some(body), capsule, c, feet, dt, |hit| hits.push(hit));
    if c.push_mass > 0.0 {
        let bodies = &physics.world.bodies;
        let colliders = &physics.world.colliders;
        hits.retain(|h| {
            colliders
                .get(h.handle)
                .and_then(|co| co.parent())
                .and_then(|b| bodies.get(b))
                .is_some_and(|b| b.is_dynamic())
        });
    } else {
        hits.clear();
    }
    Some(plan)
}

/// `c`'s move from `feet`, against the world as it stands, looking through
/// `body` (its own) if it has one. `hit` gets every collider it walked into.
fn plan_from(
    physics: &Physics3D,
    body: Option<rp::RigidBodyHandle>,
    capsule: Capsule,
    c: &Character3D,
    feet: Vec3,
    dt: f32,
    hit: impl FnMut(CharacterCollision),
) -> Plan {
    let gravity = physics.gravity;
    let up = Vec3::Y;
    let controller = KinematicCharacterController {
        up: to_rp(up),
        offset: CharacterLength::Absolute(SKIN),
        slide: true,
        autostep: (c.step_height > 0.0).then_some(CharacterAutostep {
            max_height: CharacterLength::Absolute(c.step_height),
            min_width: CharacterLength::Absolute(capsule.radius),
            include_dynamic_bodies: false,
        }),
        max_slope_climb_angle: c.max_slope,
        min_slope_slide_angle: c.max_slope,
        snap_to_ground: (c.snap_distance > 0.0).then_some(CharacterLength::Absolute(c.snap_distance)),
        ..KinematicCharacterController::default()
    };

    let mut velocity = c.velocity;
    if !velocity.is_finite() {
        velocity = Vec3::ZERO;
    }
    if c.grounded && velocity.dot(up) <= 0.0 {
        // Standing: no fall to push into the ground (snapping keeps it
        // on the ground going downhill); walking off an edge drops it.
        velocity -= up * velocity.dot(up);
    } else {
        velocity += gravity * c.gravity_scale * dt;
    }
    let desired = velocity * dt;
    let shape = capsule.shape();
    let center = rp::Pose::from_translation(to_rp(feet + up * capsule.center()));
    let filter_groups = capsule.groups.to_rp();
    let mut filter = rp::QueryFilter::default().exclude_sensors().groups(filter_groups);
    if let Some(body) = body {
        filter = filter.exclude_rigid_body(body);
    }
    let moved = {
        let queries = physics.world.query_pipeline_with_filter(filter);
        controller.move_shape(dt, &queries, &shape, &center, to_rp(desired), hit)
    };
    Plan {
        body,
        controller,
        shape,
        filter_groups,
        feet,
        velocity,
        desired,
        moved,
    }
}

impl Plan {
    /// The velocity after the move: the fall stops on landing and the rise
    /// on hitting a ceiling.
    fn velocity_after(&self) -> Vec3 {
        let up = Vec3::Y;
        let moved_by = from_rp(self.moved.translation);
        let mut velocity = self.velocity;
        let vertical = velocity.dot(up);
        if self.moved.grounded && vertical < 0.0 {
            // Landed: stop falling.
            velocity -= up * vertical;
        } else if vertical > 0.0 && moved_by.dot(up) < self.desired.dot(up) * 0.5 {
            // Hit a ceiling: stop rising.
            velocity -= up * vertical;
        }
        velocity
    }
}

impl Physics3D {
    /// One tick of a walking character, moved from `feet` the way the
    /// physics step moves a [`Character3D`], and nothing else: for client
    /// prediction, which runs the same step for its own character ahead of
    /// the server, and replays it for the inputs the server hasn't confirmed
    /// when a correction arrives (see [`net::Prediction`](crate::net::Prediction)).
    ///
    /// From the same feet, character and world, the result is the one the
    /// step would give, bit for bit: it updates `character.velocity` and
    /// `grounded` the same way and returns the new feet position. It reads
    /// the world as of the last physics step (other bodies are where they
    /// are now, not where they were at the replayed tick), pushes nothing
    /// and moves no body; write the result to the entity yourself.
    /// `entity` is the character's own, whose body (if it has one) it looks
    /// through; its capsule comes from `character`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let world = Game::new("t", 1, 1)
    ///     .physics3d(vec3(0.0, -9.81, 0.0))
    ///     .sim_system(|ctx: &mut Ctx| {
    ///         if ctx.time.sim_tick() != 2 {
    ///             return; // the ground joins at the first step
    ///         }
    ///         let me = ctx.world.spawn((ponko::d3::Transform::at(Vec3::ZERO),)); // no body yet
    ///         let mut walker = ponko::d3::Character::new(1.8, 0.3);
    ///         walker.velocity.x = 5.0;
    ///         let mut feet = vec3(0.0, 0.0, 0.0);
    ///         for _ in 0..10 {
    ///             feet = ctx.physics3d.move_character(me, &mut walker, feet, 0.02);
    ///         }
    ///         ctx.world.insert_resource((feet, walker.grounded));
    ///     })
    ///     .run_headless(3, |ctx| {
    ///         let ground = ponko::d3::Collider::cuboid(vec3(50.0, 1.0, 50.0));
    ///         ctx.world.spawn((ponko::d3::Transform::at(vec3(0.0, -0.5, 0.0)), ponko::d3::RigidBody::fixed(), ground));
    ///     });
    /// let (feet, grounded) = *world.resource::<(Vec3, bool)>();
    /// assert!((feet.x - 1.0).abs() < 1e-3 && feet.y.abs() < 0.05 && grounded, "{feet}");
    /// ```
    pub fn move_character(&self, entity: Entity, character: &mut Character3D, feet: Vec3, dt: f32) -> Vec3 {
        if dt <= 0.0 || !feet.is_finite() {
            return feet;
        }
        let body = self.link(entity).map(|l| l.body);
        let plan = plan_from(self, body, Capsule::of(character), character, feet, dt, |_| {});
        character.velocity = plan.velocity_after();
        character.grounded = plan.moved.grounded;
        feet + from_rp(plan.moved.translation)
    }
}

/// Moves every character. The shape casts only read the world (a move
/// takes effect at the next physics step, and pushes change speeds, not
/// positions), so they run in parallel; the pushes and moves are then
/// applied one by one in query order, which keeps runs deterministic.
pub(super) fn move_characters(physics: &mut Physics3D, world: &mut World, dt: f32) {
    if dt <= 0.0 {
        return;
    }
    let mut scratch = std::mem::take(&mut physics.character);
    scratch.characters.clear();
    world
        .query::<(Entity, &Character3D)>()
        .for_each(|(e, c)| scratch.characters.push((e, *c)));
    scratch.plans.clear();
    if scratch.hits.len() < scratch.characters.len() {
        scratch.hits.resize_with(scratch.characters.len(), Vec::new);
    }
    {
        let physics = &*physics;
        let Scratch {
            plans,
            characters,
            hits,
        } = &mut scratch;
        let hits = &mut hits[..characters.len()];
        if characters.len() >= PARALLEL_FROM {
            use rayon::prelude::*;
            characters
                .par_iter()
                .zip(hits.par_iter_mut())
                .map(|((e, c), hits)| plan(physics, *e, c, dt, hits))
                .collect_into_vec(plans);
        } else {
            plans.extend(
                characters
                    .iter()
                    .zip(hits.iter_mut())
                    .map(|((e, c), hits)| plan(physics, *e, c, dt, hits)),
            );
        }
    }

    for ((&(e, c), plan), hits) in scratch
        .characters
        .iter()
        .zip(scratch.plans.drain(..))
        .zip(&scratch.hits)
    {
        // A plan of the step always has the character's body.
        let Some((plan, body)) = plan.and_then(|p| p.body.map(|body| (p, body))) else {
            continue;
        };
        // Push the dynamic bodies it walked into (rapier's impulses only
        // allocate when there are some).
        if !hits.is_empty() {
            let filter = rp::QueryFilter::default()
                .exclude_rigid_body(body)
                .exclude_sensors()
                .groups(plan.filter_groups);
            let w = &mut physics.world;
            let mut queries = w.broad_phase.as_query_pipeline_mut(
                w.narrow_phase.query_dispatcher(),
                &mut w.bodies,
                &mut w.colliders,
                filter,
            );
            plan.controller
                .solve_character_collision_impulses(dt, &mut queries, &plan.shape, c.push_mass, hits.iter());
        }

        let moved_by = from_rp(plan.moved.translation);
        let velocity = plan.velocity_after();
        if let Some(character) = world.get_mut::<Character3D>(e) {
            character.velocity = velocity;
            character.grounded = plan.moved.grounded;
        }
        if let Some(b) = physics.world.bodies.get_mut(body) {
            b.set_next_kinematic_translation(to_rp(plan.feet + moved_by));
        }
    }
    physics.character = scratch;
}
