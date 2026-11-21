//! Ray casts, shape casts, overlap tests, closest points and penetration
//! depths against the 3D physics world.
//!
//! The rules are in [`Physics3D`]'s docs ("Query rules"), the same as 2D's.

use rapier3d::prelude as rp;

use super::{CollisionGroups, Physics3D, Shape3D, from_rp, pose_to_rp, to_rp};
use glam::{Quat, Vec3};
use ponko_ecs::Entity;

/// What a ray or a shape cast hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit3D {
    /// The entity whose collider was hit.
    pub entity: Entity,
    /// Where it was hit, in the world. For a shape cast, the first point
    /// the shape touched.
    pub point: Vec3,
    /// The surface's outward direction at `point` (zero for a ray that
    /// started inside the collider).
    pub normal: Vec3,
    /// How far the ray or shape went before the hit (0 if it started out
    /// inside or touching it).
    pub distance: f32,
}

/// The nearest point of a collider to a point
/// ([`Physics3D::closest_point`]; `d3::PointHit`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointHit3D {
    /// The entity whose collider is nearest.
    pub entity: Entity,
    /// The nearest point of that collider, in the world: the query point
    /// itself when it is inside.
    pub point: Vec3,
    /// From the query point to `point` (0 inside).
    pub distance: f32,
    /// Whether the query point is inside the collider.
    pub inside: bool,
}

/// How deep a shape sinks into one collider, and the way out
/// ([`Physics3D::penetrations`]; `d3::Penetration`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Penetration3D {
    /// The entity whose collider the shape overlaps.
    pub entity: Entity,
    /// The deepest point of the collider inside the shape, in the world.
    pub point: Vec3,
    /// The direction that moves the shape out of the collider (the
    /// collider's outward surface direction).
    pub normal: Vec3,
    /// How far to move the shape along `normal` to just touch: 0 when they
    /// only touch.
    pub depth: f32,
}

/// Which colliders a query sees. The default sees every solid collider
/// and no sensors.
///
/// ```
/// use ponko::prelude::*;
/// # let player = Entity::from_bits(1);
/// const TERRAIN: u32 = 1 << 2;
/// let ground_only = ponko::d3::QueryFilter::default().groups(ponko::d3::CollisionGroups::new(u32::MAX, TERRAIN));
/// let not_me = ponko::d3::QueryFilter::default().exclude(player);
/// let triggers_too = ponko::d3::QueryFilter::default().with_sensors();
/// assert!(triggers_too.sensors && !not_me.sensors);
/// # let _ = ground_only;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryFilter3D {
    /// An entity to look through (the one asking, usually).
    pub exclude: Option<Entity>,
    /// The query touches colliders as a collider with these groups would.
    pub groups: CollisionGroups,
    /// Sees sensors as well.
    pub sensors: bool,
}

impl QueryFilter3D {
    /// Looking through `entity`.
    #[must_use]
    pub fn exclude(self, entity: Entity) -> Self {
        Self {
            exclude: Some(entity),
            ..self
        }
    }
    /// Seeing only what these groups touch.
    #[must_use]
    pub fn groups(self, groups: CollisionGroups) -> Self {
        Self { groups, ..self }
    }
    /// Seeing sensors too.
    #[must_use]
    pub fn with_sensors(self) -> Self {
        Self { sensors: true, ..self }
    }
}

/// A usable `max_distance`: not negative, not NaN.
fn in_range(max_distance: f32) -> bool {
    max_distance >= 0.0
}

/// Where a query shape sits, or `None` when its place or turn isn't a
/// usable number (the turn is checked before it is normalized, which would
/// make any non-finite one the identity).
fn query_pose(position: Vec3, rotation: Quat) -> Option<rp::Pose> {
    (position.is_finite() && rotation.is_finite()).then(|| pose_to_rp(position, rotation))
}

fn ball(radius: f32) -> rp::Ball {
    rp::Ball::new(radius.max(f32::EPSILON))
}
fn cuboid(size: Vec3) -> rp::Cuboid {
    let h = (size * 0.5).max(Vec3::splat(f32::EPSILON));
    rp::Cuboid::new(to_rp(h))
}
fn capsule(height: f32, radius: f32) -> rp::Capsule {
    rp::Capsule::new_y((height * 0.5).max(0.0), radius.max(f32::EPSILON))
}

impl Physics3D {
    fn rp_filter(&self, f: QueryFilter3D) -> rp::QueryFilter<'static> {
        let mut filter = rp::QueryFilter::default().groups(f.groups.to_rp());
        if !f.sensors {
            filter = filter.exclude_sensors();
        }
        if let Some(link) = f.exclude.and_then(|e| self.link(e)) {
            filter = filter.exclude_rigid_body(link.body);
        }
        filter
    }

    /// rapier's query pipeline with [`scratch`]'s dispatcher, for the
    /// overlap and penetration tests (it also tests shapes against
    /// heightfields, which parry's alone can't).
    fn shape_queries(&self, filter: QueryFilter3D) -> rp::QueryPipeline<'_> {
        self.world.broad_phase.as_query_pipeline(
            &scratch::Scratch,
            &self.world.bodies,
            &self.world.colliders,
            self.rp_filter(filter),
        )
    }

    /// The closest collider a ray from `origin` towards `direction` hits
    /// within `max_distance`. Up to date as of the last physics step. A
    /// zero direction or a non-finite origin hits nothing, as in 2D; see the
    /// [query rules](Physics3D#query-rules).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// fn look_down(ctx: &mut Ctx) {
    ///     let filter = ponko::d3::QueryFilter::default();
    ///     if let Some(hit) = ctx.physics3d.cast_ray(vec3(0.0, 5.0, 0.0), Vec3::NEG_Y, 100.0, filter) {
    ///         assert!((hit.distance - 4.5).abs() < 1e-4, "the ground's top is at 0.5");
    ///     }
    /// }
    /// Game::new("t", 1, 1).physics3d(Vec3::ZERO).sim_system(look_down).run_headless(2, |ctx| {
    ///     ctx.world.spawn((ponko::d3::Transform::default(), ponko::d3::RigidBody::fixed(), ponko::d3::Collider::cuboid(vec3(10.0, 1.0, 10.0))));
    /// });
    /// ```
    pub fn cast_ray(&self, origin: Vec3, direction: Vec3, max_distance: f32, filter: QueryFilter3D) -> Option<Hit3D> {
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO || !origin.is_finite() || !in_range(max_distance) {
            return None;
        }
        let ray = rp::Ray::new(to_rp(origin), to_rp(dir));
        let (handle, hit) = self
            .world
            .cast_ray_and_get_normal(&ray, max_distance, true, self.rp_filter(filter))?;
        Some(Hit3D {
            entity: self.entity_of(handle)?,
            point: origin + dir * hit.time_of_impact,
            normal: from_rp(hit.normal),
            distance: hit.time_of_impact,
        })
    }

    /// Every collider a ray hits within `max_distance`, nearest first (equal
    /// distances by entity id), into `out` (cleared first; reuse it so casts
    /// don't allocate). A collider the ray starts inside comes first, at
    /// distance 0.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut hits = Vec::new();
    /// physics.cast_ray_all(Vec3::ZERO, Vec3::X, 50.0, ponko::d3::QueryFilter::default(), &mut hits);
    /// assert!(hits.is_empty());
    /// ```
    pub fn cast_ray_all(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter3D,
        out: &mut Vec<Hit3D>,
    ) {
        out.clear();
        let dir = direction.normalize_or_zero();
        if dir == Vec3::ZERO || !origin.is_finite() || !in_range(max_distance) {
            return;
        }
        let ray = rp::Ray::new(to_rp(origin), to_rp(dir));
        for (handle, _, hit) in self
            .world
            .intersect_ray(ray, max_distance, true, self.rp_filter(filter))
        {
            if let Some(entity) = self.entity_of(handle) {
                out.push(Hit3D {
                    entity,
                    point: origin + dir * hit.time_of_impact,
                    normal: from_rp(hit.normal),
                    distance: hit.time_of_impact,
                });
            }
        }
        out.sort_unstable_by(|a, b| {
            a.distance
                .total_cmp(&b.distance)
                .then(a.entity.to_bits().cmp(&b.entity.to_bits()))
        });
    }

    fn cast(
        &self,
        shape: &dyn rp::Shape,
        pose: Option<rp::Pose>,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter3D,
    ) -> Option<Hit3D> {
        let dir = direction.normalize_or_zero();
        let pose = pose?;
        if dir == Vec3::ZERO || !in_range(max_distance) {
            return None;
        }
        // A shape that starts out overlapping something hits it at distance
        // 0, whichever way it moves (rapier's sweep can miss a shape moving
        // deeper in).
        if let Some(start) = self.deepest(shape, &pose, filter) {
            return Some(Hit3D {
                entity: start.entity,
                point: start.point,
                normal: start.normal,
                distance: 0.0,
            });
        }
        let options = rapier3d::parry::query::ShapeCastOptions {
            max_time_of_impact: max_distance,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: true,
        };
        let (handle, hit) = self
            .world
            .cast_shape(&pose, to_rp(dir), shape, options, self.rp_filter(filter))?;
        Some(Hit3D {
            entity: self.entity_of(handle)?,
            point: from_rp(hit.witness1),
            normal: from_rp(hit.normal1),
            distance: hit.time_of_impact,
        })
    }

    /// Sweeps a ball of `radius` from `origin` towards `direction` and
    /// returns the first collider it touches within `max_distance` (thick
    /// rays: bullets with size, "is there room to walk there").
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// assert!(physics.cast_sphere(Vec3::ZERO, 0.5, Vec3::Z, 10.0, ponko::d3::QueryFilter::default()).is_none());
    /// ```
    pub fn cast_sphere(
        &self,
        origin: Vec3,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter3D,
    ) -> Option<Hit3D> {
        self.cast(
            &ball(radius),
            query_pose(origin, Quat::IDENTITY),
            direction,
            max_distance,
            filter,
        )
    }

    /// Sweeps a box of `size` centered on `origin` and turned by `rotation`
    /// and returns the first collider it touches within `max_distance` (a
    /// crate sliding, a door swinging shut, "does this fit there").
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let crate_size = vec3(1.0, 1.0, 1.0);
    /// assert!(physics.cast_box(Vec3::ZERO, crate_size, Quat::IDENTITY, Vec3::NEG_Y, 10.0, ponko::d3::QueryFilter::default()).is_none());
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn cast_box(
        &self,
        origin: Vec3,
        size: Vec3,
        rotation: Quat,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter3D,
    ) -> Option<Hit3D> {
        self.cast(
            &cuboid(size),
            query_pose(origin, rotation),
            direction,
            max_distance,
            filter,
        )
    }

    /// Sweeps an upright capsule centered on `origin` (`height` between the
    /// centers of its round ends, as [`Collider3D::capsule`](super::Collider3D::capsule))
    /// and returns the first collider it touches within `max_distance`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// assert!(physics.cast_capsule(Vec3::ZERO, 1.2, 0.3, Vec3::X, 10.0, ponko::d3::QueryFilter::default()).is_none());
    /// ```
    pub fn cast_capsule(
        &self,
        origin: Vec3,
        height: f32,
        radius: f32,
        direction: Vec3,
        max_distance: f32,
        filter: QueryFilter3D,
    ) -> Option<Hit3D> {
        self.cast(
            &capsule(height, radius),
            query_pose(origin, Quat::IDENTITY),
            direction,
            max_distance,
            filter,
        )
    }

    fn overlap(&self, shape: &dyn rp::Shape, pose: Option<rp::Pose>, filter: QueryFilter3D, out: &mut Vec<Entity>) {
        out.clear();
        let Some(pose) = pose else {
            return;
        };
        let pipeline = self.shape_queries(filter);
        for (handle, _) in pipeline.intersect_shape(pose, shape) {
            out.extend(self.entity_of(handle));
        }
    }

    /// Every entity whose collider overlaps the ball, into `out` (cleared first).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut near = Vec::new();
    /// physics.overlap_sphere(Vec3::ZERO, 5.0, ponko::d3::QueryFilter::default(), &mut near);
    /// assert!(near.is_empty());
    /// ```
    pub fn overlap_sphere(&self, center: Vec3, radius: f32, filter: QueryFilter3D, out: &mut Vec<Entity>) {
        self.overlap(&ball(radius), query_pose(center, Quat::IDENTITY), filter, out);
    }

    /// Every entity whose collider overlaps the box of `size` centered on
    /// `center` and turned by `rotation`, into `out` (cleared first).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut inside = Vec::new();
    /// physics.overlap_box(Vec3::ZERO, Vec3::ONE, Quat::IDENTITY, ponko::d3::QueryFilter::default(), &mut inside);
    /// assert!(inside.is_empty());
    /// ```
    pub fn overlap_box(&self, center: Vec3, size: Vec3, rotation: Quat, filter: QueryFilter3D, out: &mut Vec<Entity>) {
        self.overlap(&cuboid(size), query_pose(center, rotation), filter, out);
    }

    /// Every entity whose collider overlaps the upright capsule centered on
    /// `center`, into `out` (cleared first). Can a character stand up here?
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut blocking = Vec::new();
    /// physics.overlap_capsule(Vec3::ZERO, 1.2, 0.3, ponko::d3::QueryFilter::default(), &mut blocking);
    /// assert!(blocking.is_empty());
    /// ```
    pub fn overlap_capsule(
        &self,
        center: Vec3,
        height: f32,
        radius: f32,
        filter: QueryFilter3D,
        out: &mut Vec<Entity>,
    ) {
        self.overlap(
            &capsule(height, radius),
            query_pose(center, Quat::IDENTITY),
            filter,
            out,
        );
    }

    /// Every entity whose collider contains `point`, into `out` (cleared
    /// first).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut here = Vec::new();
    /// physics.overlap_point(Vec3::ZERO, ponko::d3::QueryFilter::default(), &mut here);
    /// assert!(here.is_empty());
    /// ```
    pub fn overlap_point(&self, point: Vec3, filter: QueryFilter3D, out: &mut Vec<Entity>) {
        out.clear();
        if !point.is_finite() {
            return;
        }
        for (handle, _) in self.world.intersect_point(to_rp(point), self.rp_filter(filter)) {
            out.extend(self.entity_of(handle));
        }
    }

    /// The nearest collider to `point` within `max_distance`, and its
    /// nearest point. A point inside a collider is its own nearest point
    /// (distance 0, `inside` set). Snapping to a surface, "how far is the
    /// nearest wall". A triangle mesh or heightfield has no inside: a point
    /// below one gets its nearest surface point.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// fn nearest(ctx: &mut Ctx) {
    ///     let filter = ponko::d3::QueryFilter::default();
    ///     if let Some(near) = ctx.physics3d.closest_point(Vec3::ZERO, 50.0, filter) {
    ///         ctx.world.insert_resource(near);
    ///     }
    /// }
    /// let world = Game::new("t", 1, 1).physics3d(Vec3::ZERO).sim_system(nearest).run_headless(2, |ctx| {
    ///     ctx.world.spawn((ponko::d3::Transform::at(vec3(10.0, 0.0, 0.0)), ponko::d3::RigidBody::fixed(), ponko::d3::Collider::sphere(1.0)));
    /// });
    /// let near = world.resource::<ponko::d3::PointHit>();
    /// assert_eq!((near.point, near.distance, near.inside), (vec3(9.0, 0.0, 0.0), 9.0, false));
    /// ```
    pub fn closest_point(&self, point: Vec3, max_distance: f32, filter: QueryFilter3D) -> Option<PointHit3D> {
        if !point.is_finite() || !in_range(max_distance) {
            return None;
        }
        let (handle, projection) =
            self.world
                .project_point(to_rp(point), max_distance, true, self.rp_filter(filter))?;
        let nearest = from_rp(projection.point);
        Some(PointHit3D {
            entity: self.entity_of(handle)?,
            point: nearest,
            distance: if projection.is_inside {
                0.0
            } else {
                point.distance(nearest)
            },
            inside: projection.is_inside,
        })
    }

    /// Every collider that `shape`, placed at `position` and turned by
    /// `rotation`, overlaps: how deep, and which way pushes the shape out,
    /// deepest first (equal depths by entity id), into `out` (cleared
    /// first). Moving the shape by `normal * depth` of the first entry
    /// frees it from that collider: the step a character controller takes
    /// after it was pushed into a wall.
    ///
    /// The shape's own offset doesn't apply (place it with `position`); a
    /// [`Shape3D::Mesh`] keeps the offset its mesh was made with, and a freed
    /// one finds nothing. A [`Shape3D::ScaledMesh`] uses the copy a collider
    /// at the same scale already has, else makes one for the query (which
    /// allocates). A pair that has no penetration depth (a triangle
    /// mesh or heightfield against another) is left out. Doesn't allocate,
    /// but for that scaled copy.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// let mut pushes = Vec::new();
    /// let player = ponko::d3::Collider::capsule(1.2, 0.3).shape;
    /// physics.penetrations(player, Vec3::ZERO, Quat::IDENTITY, ponko::d3::QueryFilter::default(), &mut pushes);
    /// assert!(pushes.is_empty());
    /// ```
    pub fn penetrations(
        &self,
        shape: Shape3D,
        position: Vec3,
        rotation: Quat,
        filter: QueryFilter3D,
        out: &mut Vec<Penetration3D>,
    ) {
        out.clear();
        if !position.is_finite() || !rotation.is_finite() {
            return;
        }
        match shape {
            Shape3D::Sphere { radius } => self.push_out(&ball(radius), position, rotation, filter, out),
            Shape3D::Cuboid { size } => self.push_out(&cuboid(size), position, rotation, filter, out),
            Shape3D::Capsule { height, radius } => {
                self.push_out(&capsule(height, radius), position, rotation, filter, out)
            }
            Shape3D::Mesh(mesh) => {
                if let Some(slot) = self.mesh(mesh) {
                    let shape = slot.shape.as_ref().expect("live slot");
                    let at = position + rotation.normalize() * slot.offset;
                    self.push_out(&**shape, at, rotation, filter, out);
                }
            }
            Shape3D::ScaledMesh { mesh, scale } => {
                if let Some((shape, offset, _)) = self.scaled_mesh(mesh, scale) {
                    let at = position + rotation.normalize() * offset;
                    self.push_out(&*shape, at, rotation, filter, out);
                }
            }
        }
        out.sort_unstable_by(|a, b| {
            b.depth
                .total_cmp(&a.depth)
                .then(a.entity.to_bits().cmp(&b.entity.to_bits()))
        });
    }

    fn push_out(
        &self,
        shape: &dyn rp::Shape,
        position: Vec3,
        rotation: Quat,
        filter: QueryFilter3D,
        out: &mut Vec<Penetration3D>,
    ) {
        self.each_penetration(shape, &pose_to_rp(position, rotation), filter, |p| out.push(p));
    }

    /// Calls `f` with how deep `shape` at `pose` sinks into each collider it
    /// overlaps.
    fn each_penetration(
        &self,
        shape: &dyn rp::Shape,
        pose: &rp::Pose,
        filter: QueryFilter3D,
        mut f: impl FnMut(Penetration3D),
    ) {
        let pipeline = self.shape_queries(filter);
        for (handle, collider) in pipeline.intersect_shape(*pose, shape) {
            let Some(entity) = self.entity_of(handle) else {
                continue;
            };
            if let Some(c) = scratch::contact(pose, shape, collider.position(), collider.shape()) {
                f(Penetration3D {
                    entity,
                    point: from_rp(c.point2),
                    normal: from_rp(c.normal2),
                    depth: (-c.dist).max(0.0),
                });
            }
        }
    }

    /// The deepest collider `shape` at `pose` overlaps (equal depths: the
    /// lowest entity id), if any.
    fn deepest(&self, shape: &dyn rp::Shape, pose: &rp::Pose, filter: QueryFilter3D) -> Option<Penetration3D> {
        let mut best = None::<Penetration3D>;
        self.each_penetration(shape, pose, filter, |p| {
            let deeper = best.is_none_or(|b| {
                p.depth
                    .total_cmp(&b.depth)
                    .then(b.entity.to_bits().cmp(&p.entity.to_bits()))
                    .is_gt()
            });
            if deeper {
                best = Some(p);
            }
        });
        best
    }
}

/// Contacts that keep their scratch memory between calls, and overlap tests
/// against heightfields.
///
/// parry's own [`contact`](rapier3d::parry::query::contact) builds a fresh
/// EPA (the penetration-depth solver) on the heap whenever two convex
/// shapes overlap, including each triangle of a mesh. This dispatcher does
/// the same work with one EPA per thread, so penetration queries don't
/// allocate once it has grown. parry also has no overlap test between a
/// heightfield and anything but a ball; this one tests the heightfield's
/// triangles under the shape. Everything else goes to parry's default.
mod scratch {
    use std::cell::RefCell;

    use rapier3d::parry::math::{Pose, Real, Vector};
    use rapier3d::parry::query::details::{
        contact_composite_shape_shape, contact_heightfield_shape, contact_shape_composite_shape,
        contact_shape_heightfield,
    };
    use rapier3d::parry::query::epa::EPA;
    use rapier3d::parry::query::gjk::{self, ConstantOrigin, CsoPoint, GJKResult, VoronoiSimplex};
    use rapier3d::parry::query::{
        ClosestPoints, Contact, DefaultQueryDispatcher, NonlinearRigidMotion, QueryDispatcher, ShapeCastHit,
        ShapeCastOptions, ShapeDistance, ShapeIntersection, Unsupported,
    };
    use rapier3d::parry::shape::{Ball, ConvexPolyhedron, HalfSpace, HeightField, Shape};

    thread_local! {
        static EPA_SCRATCH: RefCell<EPA> = RefCell::new(EPA::new());
    }

    /// The contact between `shape1` at `pose1` and `shape2` at `pose2`, in
    /// world space (as parry's `contact` with no prediction), or `None` when
    /// they don't touch or parry has no contact for the pair.
    pub(super) fn contact(pose1: &Pose, shape1: &dyn Shape, pose2: &Pose, shape2: &dyn Shape) -> Option<Contact> {
        let mut c = Scratch.contact(&pose1.inv_mul(pose2), shape1, shape2, 0.0).ok()??;
        c.transform_by_mut(pose1, pose2);
        Some(c)
    }

    /// GJK, then (when the shapes overlap) the reused EPA, as parry's
    /// `contact_support_map_support_map`.
    fn support_maps(
        pos12: &Pose,
        g1: &dyn rapier3d::parry::shape::SupportMap,
        g2: &dyn rapier3d::parry::shape::SupportMap,
        prediction: Real,
    ) -> Option<Contact> {
        let dir = pos12.translation.try_normalize().unwrap_or(Vector::X);
        let mut simplex = VoronoiSimplex::new();
        simplex.reset(CsoPoint::from_shapes(pos12, g1, g2, dir));
        let found = match gjk::closest_points(pos12, g1, g2, prediction, true, &mut simplex) {
            GJKResult::Intersection => EPA_SCRATCH
                .with_borrow_mut(|epa| epa.closest_points(pos12, g1, g2, &simplex))
                .map(|(p1, p2, n)| GJKResult::ClosestPoints(p1, p2, n)),
            other => Some(other),
        };
        match found? {
            GJKResult::ClosestPoints(point1, point2_1, normal1) => {
                let dist = (point2_1 - point1).dot(normal1);
                let point2 = pos12.inverse_transform_point(point2_1);
                let normal2 = pos12.rotation.inverse() * -normal1;
                Some(Contact::new(point1, point2, normal1, normal2, dist))
            }
            _ => None,
        }
    }

    /// The point of `poly` nearest `point` (both in its frame), and whether
    /// `point` is inside: parry's support-map point projection, with the
    /// reused EPA for points inside.
    fn project(poly: &ConvexPolyhedron, point: Vector) -> (Vector, bool) {
        let m = Pose::new(-point, Vector::ZERO);
        let dir = point.try_normalize().unwrap_or(Vector::X);
        let mut simplex = VoronoiSimplex::new();
        simplex.reset(CsoPoint::from_shapes(&m.inverse(), poly, &ConstantOrigin, dir));
        match gjk::project_origin(&m, poly, &mut simplex) {
            Some(nearest) => (nearest, false),
            None => {
                let nearest = EPA_SCRATCH.with_borrow_mut(|epa| epa.project_origin(&m, poly, &simplex));
                (nearest.unwrap_or(point), true)
            }
        }
    }

    /// parry's `contact_convex_polyhedron_ball`, projecting with [`project`].
    fn polyhedron_ball(pos12: &Pose, poly: &ConvexPolyhedron, ball: &Ball, prediction: Real) -> Option<Contact> {
        let center = pos12.translation;
        let (nearest, inside) = project(poly, center);
        let (dir, len) = (nearest - center).normalize_and_length();
        if len < rapier3d::parry::math::DEFAULT_EPSILON {
            // The center is on the surface: parry finds the face's normal
            // (no EPA: the point isn't inside).
            return DefaultQueryDispatcher.contact(pos12, poly, ball, prediction).ok()?;
        }
        let (dist, normal1) = if inside {
            (-len - ball.radius, dir)
        } else {
            (len - ball.radius, -dir)
        };
        (dist <= prediction).then(|| {
            let normal2 = pos12.rotation.inverse() * -normal1;
            Contact::new(nearest, normal2 * ball.radius, normal1, normal2, dist)
        })
    }

    /// Whether any triangle of `hf` under `shape` (at `pos12` in the
    /// heightfield's frame) overlaps it.
    fn heightfield_overlaps(pos12: &Pose, hf: &HeightField, shape: &dyn Shape) -> bool {
        let mut hit = false;
        hf.map_elements_in_local_aabb(&shape.compute_aabb(pos12), &mut |_, triangle| {
            hit = hit
                || DefaultQueryDispatcher
                    .intersection_test(pos12, triangle, shape)
                    .is_ok_and(|i| i.intersecting);
        });
        hit
    }

    pub(super) struct Scratch;

    impl QueryDispatcher for Scratch {
        fn intersection_test(
            &self,
            pos12: &Pose,
            g1: &dyn Shape,
            g2: &dyn Shape,
        ) -> Result<ShapeIntersection, Unsupported> {
            // A ball against a heightfield is parry's (a point query).
            if g1.as_ball().is_none() && g2.as_ball().is_none() {
                if let Some(hf) = g1.as_heightfield() {
                    return Ok(heightfield_overlaps(pos12, hf, g2).into());
                }
                if let Some(hf) = g2.as_heightfield() {
                    return Ok(heightfield_overlaps(&pos12.inverse(), hf, g1).into());
                }
            }
            DefaultQueryDispatcher.intersection_test(pos12, g1, g2)
        }

        fn distance(&self, pos12: &Pose, g1: &dyn Shape, g2: &dyn Shape) -> Result<ShapeDistance, Unsupported> {
            DefaultQueryDispatcher.distance(pos12, g1, g2)
        }

        fn contact(
            &self,
            pos12: &Pose,
            g1: &dyn Shape,
            g2: &dyn Shape,
            prediction: Real,
        ) -> Result<Option<Contact>, Unsupported> {
            match (
                g1.as_convex_polyhedron(),
                g2.as_ball(),
                g1.as_ball(),
                g2.as_convex_polyhedron(),
            ) {
                (Some(poly), Some(ball), ..) => return Ok(polyhedron_ball(pos12, poly, ball, prediction)),
                (_, _, Some(ball), Some(poly)) => {
                    return Ok(polyhedron_ball(&pos12.inverse(), poly, ball, prediction).map(Contact::flipped));
                }
                _ => {}
            }
            let half_space = g1.as_shape::<HalfSpace>().is_some() || g2.as_shape::<HalfSpace>().is_some();
            let ball = g1.as_ball().is_some() || g2.as_ball().is_some();
            if half_space || (ball && g1.is_convex() && g2.is_convex()) {
                // parry's exact answers for these allocate nothing.
                return DefaultQueryDispatcher.contact(pos12, g1, g2, prediction);
            }
            if let (Some(s1), Some(s2)) = (g1.as_support_map(), g2.as_support_map()) {
                return Ok(support_maps(pos12, s1, s2, prediction));
            }
            // The composite cases, with this dispatcher for their parts.
            if let Some(hf) = g1.as_heightfield() {
                Ok(contact_heightfield_shape(self, pos12, hf, g2, prediction))
            } else if let Some(hf) = g2.as_heightfield() {
                Ok(contact_shape_heightfield(self, pos12, g1, hf, prediction))
            } else if let Some(c) = g1.as_composite_shape() {
                Ok(contact_composite_shape_shape(self, pos12, c, g2, prediction))
            } else if let Some(c) = g2.as_composite_shape() {
                Ok(contact_shape_composite_shape(self, pos12, g1, c, prediction))
            } else {
                DefaultQueryDispatcher.contact(pos12, g1, g2, prediction)
            }
        }

        fn closest_points(
            &self,
            pos12: &Pose,
            g1: &dyn Shape,
            g2: &dyn Shape,
            max_dist: Real,
        ) -> Result<ClosestPoints, Unsupported> {
            DefaultQueryDispatcher.closest_points(pos12, g1, g2, max_dist)
        }

        fn cast_shapes(
            &self,
            pos12: &Pose,
            local_vel12: Vector,
            g1: &dyn Shape,
            g2: &dyn Shape,
            options: ShapeCastOptions,
        ) -> Result<Option<ShapeCastHit>, Unsupported> {
            DefaultQueryDispatcher.cast_shapes(pos12, local_vel12, g1, g2, options)
        }

        fn cast_shapes_nonlinear(
            &self,
            motion1: &NonlinearRigidMotion,
            g1: &dyn Shape,
            motion2: &NonlinearRigidMotion,
            g2: &dyn Shape,
            start_time: Real,
            end_time: Real,
            stop_at_penetration: bool,
        ) -> Result<Option<ShapeCastHit>, Unsupported> {
            DefaultQueryDispatcher.cast_shapes_nonlinear(
                motion1,
                g1,
                motion2,
                g2,
                start_time,
                end_time,
                stop_at_penetration,
            )
        }
    }
}
