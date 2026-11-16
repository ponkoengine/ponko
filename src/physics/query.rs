//! Ray casts, shape casts, overlap tests, closest points and penetration
//! depths against the 2D physics world.
//!
//! The rules are in [`Physics`]'s docs ("Query rules"), the same as 3D's.

use rapier2d::parry::query::ShapeCastOptions;
use rapier2d::prelude as rp;

use super::{CollisionGroups, MAX_POLYGON_POINTS, Physics, Shape, from_rp, to_rp};
use glam::Vec2;
use ponko_ecs::Entity;

/// Which colliders a 2D query sees. The default sees every solid collider
/// and no sensors.
///
/// ```
/// use ponko::prelude::*;
/// # let player = Entity::from_bits(1);
/// const TERRAIN: u32 = 1 << 2;
/// let ground_only = ponko::d2::QueryFilter::default().groups(ponko::d2::CollisionGroups::new(u32::MAX, TERRAIN));
/// let not_me = ponko::d2::QueryFilter::default().exclude(player);
/// let triggers_too = ponko::d2::QueryFilter::default().with_sensors();
/// assert!(triggers_too.sensors && !not_me.sensors);
/// # let _ = ground_only;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryFilter {
    /// An entity to look through (the one asking, usually).
    pub exclude: Option<Entity>,
    /// The query touches colliders as a collider with these groups would
    /// ([`CollisionGroups::touches`]). Every group by default.
    pub groups: CollisionGroups,
    /// Sees sensors as well.
    pub sensors: bool,
}

impl QueryFilter {
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

/// What a ray or a shape cast hit (`d2::Hit`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    /// The entity whose collider was hit.
    pub entity: Entity,
    /// Where it was hit, in world pixels. For a shape cast, the first point
    /// the shape touched.
    pub point: Vec2,
    /// The surface's outward direction at `point` (zero for a ray that
    /// started inside the collider).
    pub normal: Vec2,
    /// How far the ray or shape went before the hit (0 if it started out
    /// inside or touching it).
    pub distance: f32,
}

/// The nearest point of a collider to a point
/// ([`Physics::closest_point`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointHit {
    /// The entity whose collider is nearest.
    pub entity: Entity,
    /// The nearest point of that collider, in world pixels: the query point
    /// itself when it is inside.
    pub point: Vec2,
    /// From the query point to `point` (0 inside).
    pub distance: f32,
    /// Whether the query point is inside the collider.
    pub inside: bool,
}

/// How deep a shape sinks into one collider, and the way out
/// ([`Physics::penetrations`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Penetration {
    /// The entity whose collider the shape overlaps.
    pub entity: Entity,
    /// The deepest point of the collider inside the shape, in world pixels.
    pub point: Vec2,
    /// The direction that moves the shape out of the collider (the
    /// collider's outward surface direction).
    pub normal: Vec2,
    /// How far to move the shape along `normal` to just touch: 0 when they
    /// only touch.
    pub depth: f32,
}

/// Distances are measured from non-negative, non-NaN maximums only.
fn in_range(max_distance: f32) -> bool {
    max_distance >= 0.0
}

/// Calls `f` with the rapier form of `shape`, built on the stack (a polygon
/// builds its convex hull).
fn with_shape<R>(shape: &Shape, f: impl FnOnce(&dyn rp::Shape) -> R) -> Option<R> {
    match *shape {
        Shape::Circle { radius } => Some(f(&rp::Ball::new(radius.max(f32::EPSILON)))),
        Shape::Rect { size } => {
            let h = (size * 0.5).max(Vec2::splat(f32::EPSILON));
            Some(f(&rp::Cuboid::new(to_rp(h))))
        }
        Shape::Capsule { height, radius } => Some(f(&rp::Capsule::new_y(
            (height * 0.5).max(0.0),
            radius.max(f32::EPSILON),
        ))),
        Shape::Polygon { .. } => {
            let mut corners = [rp::Vector::ZERO; MAX_POLYGON_POINTS];
            let points = shape.points();
            for (c, p) in corners.iter_mut().zip(points) {
                *c = to_rp(*p);
            }
            let hull = rp::ConvexPolygon::from_convex_hull(&corners[..points.len()])?;
            Some(f(&hull))
        }
    }
}

impl Physics {
    fn query_filter(&self, filter: QueryFilter) -> rp::QueryFilter<'static> {
        let mut result = rp::QueryFilter::default().groups(filter.groups.to_rp());
        if !filter.sensors {
            result = result.exclude_sensors();
        }
        if let Some(link) = filter.exclude.and_then(|entity| self.link(entity)) {
            result = result.exclude_rigid_body(link.body);
        }
        result
    }

    /// The closest collider a ray from `origin` towards `direction` hits
    /// within `max_distance`, among those `filter` lets through (solid
    /// colliders by default; `QueryFilter::default().with_sensors()` for
    /// sensors too). Up to date as of the last physics step. A zero
    /// direction or a non-finite origin hits nothing, as in 3D; see the
    /// [query rules](Physics#query-rules).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// fn look_right(ctx: &mut Ctx) {
    ///     let filter = ponko::d2::QueryFilter::default();
    ///     if let Some(hit) = ctx.physics.cast_ray(Vec2::ZERO, Vec2::X, 500.0, filter) {
    ///         ctx.world.insert_resource(hit);
    ///     }
    /// }
    /// let world = Game::new("t", 1, 1).physics(Vec2::ZERO).sim_system(look_right).run_headless(2, |ctx| {
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(100.0, 0.0)), ponko::d2::RigidBody::fixed(), ponko::d2::Collider::circle(10.0)));
    /// });
    /// // Queries see bodies once they have been stepped: the second tick hits.
    /// assert_eq!(world.resource::<ponko::d2::Hit>().distance, 90.0);
    /// ```
    pub fn cast_ray(&self, origin: Vec2, direction: Vec2, max_distance: f32, filter: QueryFilter) -> Option<RayHit> {
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO || !origin.is_finite() || !in_range(max_distance) {
            return None;
        }
        let ray = rp::Ray::new(to_rp(origin), to_rp(dir));
        let (handle, hit) = self
            .world
            .cast_ray_and_get_normal(&ray, max_distance, true, self.query_filter(filter))?;
        Some(RayHit {
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
    /// let physics = ponko::d2::Physics::default();
    /// let mut hits = Vec::new();
    /// physics.cast_ray_all(Vec2::ZERO, Vec2::X, 500.0, ponko::d2::QueryFilter::default(), &mut hits);
    /// assert!(hits.is_empty());
    /// ```
    pub fn cast_ray_all(
        &self,
        origin: Vec2,
        direction: Vec2,
        max_distance: f32,
        filter: QueryFilter,
        out: &mut Vec<RayHit>,
    ) {
        out.clear();
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO || !origin.is_finite() || !in_range(max_distance) {
            return;
        }
        let ray = rp::Ray::new(to_rp(origin), to_rp(dir));
        for (handle, _, hit) in self
            .world
            .intersect_ray(ray, max_distance, true, self.query_filter(filter))
        {
            if let Some(entity) = self.entity_of(handle) {
                out.push(RayHit {
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

    #[allow(clippy::too_many_arguments)]
    fn cast(
        &self,
        shape: &Shape,
        origin: Vec2,
        rotation: f32,
        direction: Vec2,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO || !origin.is_finite() || !rotation.is_finite() || !in_range(max_distance) {
            return None;
        }
        let options = ShapeCastOptions {
            max_time_of_impact: max_distance,
            target_distance: 0.0,
            stop_at_penetration: true,
            compute_impact_geometry_on_penetration: true,
        };
        let pose = rp::Pose::new(to_rp(origin), rotation);
        with_shape(shape, |s| {
            // A sweep may miss a shape moving deeper into a collider, or
            // one with no distance left to travel. Check the initial pose.
            let mut deepest = None::<Penetration>;
            self.each_penetration(s, &pose, filter, |p| {
                if deepest.is_none_or(|b| {
                    p.depth
                        .total_cmp(&b.depth)
                        .then(b.entity.to_bits().cmp(&p.entity.to_bits()))
                        .is_gt()
                }) {
                    deepest = Some(p);
                }
            });
            if let Some(p) = deepest {
                return Some(RayHit {
                    entity: p.entity,
                    point: p.point,
                    normal: p.normal,
                    distance: 0.0,
                });
            }
            let (handle, hit) = self
                .world
                .cast_shape(&pose, to_rp(dir), s, options, self.query_filter(filter))?;
            Some(RayHit {
                entity: self.entity_of(handle)?,
                point: from_rp(hit.witness1),
                normal: from_rp(hit.normal1),
                distance: hit.time_of_impact,
            })
        })?
    }

    /// Sweeps a circle of `radius` from `origin` towards `direction` and
    /// returns the first collider it touches within `max_distance` (thick
    /// rays: bullets with size, "is there room to move there"). The 2D form
    /// of `d3::Physics::cast_sphere`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// assert!(physics.cast_circle(Vec2::ZERO, 8.0, Vec2::X, 100.0, ponko::d2::QueryFilter::default()).is_none());
    /// ```
    pub fn cast_circle(
        &self,
        origin: Vec2,
        radius: f32,
        direction: Vec2,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        self.cast(&Shape::Circle { radius }, origin, 0.0, direction, max_distance, filter)
    }

    /// Sweeps a rectangle of `size` centered on `origin` and turned by
    /// `rotation` (radians, as [`d2::Transform::rotation`](crate::d2::Transform::rotation))
    /// and returns the first collider it touches within `max_distance`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let crate_size = vec2(32.0, 32.0);
    /// assert!(physics.cast_box(Vec2::ZERO, crate_size, 0.0, Vec2::Y, 100.0, ponko::d2::QueryFilter::default()).is_none());
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub fn cast_box(
        &self,
        origin: Vec2,
        size: Vec2,
        rotation: f32,
        direction: Vec2,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        self.cast(&Shape::Rect { size }, origin, rotation, direction, max_distance, filter)
    }

    /// Sweeps an upright capsule centered on `origin` (`height` between the
    /// centers of its round ends, as [`Collider::capsule`](super::Collider::capsule))
    /// and returns the first collider it touches within `max_distance`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// assert!(physics.cast_capsule(Vec2::ZERO, 24.0, 8.0, Vec2::X, 100.0, ponko::d2::QueryFilter::default()).is_none());
    /// ```
    pub fn cast_capsule(
        &self,
        origin: Vec2,
        height: f32,
        radius: f32,
        direction: Vec2,
        max_distance: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        self.cast(
            &Shape::Capsule { height, radius },
            origin,
            0.0,
            direction,
            max_distance,
            filter,
        )
    }

    fn overlap(&self, shape: &Shape, center: Vec2, rotation: f32, filter: QueryFilter, out: &mut Vec<Entity>) {
        out.clear();
        if !center.is_finite() || !rotation.is_finite() {
            return;
        }
        let pose = rp::Pose::new(to_rp(center), rotation);
        with_shape(shape, |s| {
            for (handle, _) in self.world.intersect_shape(pose, s, self.query_filter(filter)) {
                out.extend(self.entity_of(handle));
            }
        });
    }

    /// Every entity whose collider contains `point`, into `out` (cleared
    /// first).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let mut here = Vec::new();
    /// physics.overlap_point(Vec2::ZERO, ponko::d2::QueryFilter::default(), &mut here);
    /// assert!(here.is_empty());
    /// ```
    pub fn overlap_point(&self, point: Vec2, filter: QueryFilter, out: &mut Vec<Entity>) {
        out.clear();
        if !point.is_finite() {
            return;
        }
        for (handle, _) in self.world.intersect_point(to_rp(point), self.query_filter(filter)) {
            out.extend(self.entity_of(handle));
        }
    }

    /// Every entity whose collider overlaps the circle, into `out` (cleared
    /// first). The 2D form of `d3::Physics::overlap_sphere`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let mut near = Vec::new();
    /// physics.overlap_circle(Vec2::ZERO, 50.0, ponko::d2::QueryFilter::default(), &mut near);
    /// assert!(near.is_empty());
    /// ```
    pub fn overlap_circle(&self, center: Vec2, radius: f32, filter: QueryFilter, out: &mut Vec<Entity>) {
        self.overlap(&Shape::Circle { radius }, center, 0.0, filter, out);
    }

    /// Every entity whose collider overlaps the rectangle of `size` centered
    /// on `center` and turned by `rotation` (radians), into `out` (cleared
    /// first).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let mut inside = Vec::new();
    /// physics.overlap_box(Vec2::ZERO, vec2(64.0, 32.0), 0.0, ponko::d2::QueryFilter::default(), &mut inside);
    /// assert!(inside.is_empty());
    /// ```
    pub fn overlap_box(&self, center: Vec2, size: Vec2, rotation: f32, filter: QueryFilter, out: &mut Vec<Entity>) {
        self.overlap(&Shape::Rect { size }, center, rotation, filter, out);
    }

    /// Every entity whose collider overlaps the upright capsule centered on
    /// `center`, into `out` (cleared first). Can a character stand up here?
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let mut blocking = Vec::new();
    /// physics.overlap_capsule(Vec2::ZERO, 24.0, 8.0, ponko::d2::QueryFilter::default(), &mut blocking);
    /// assert!(blocking.is_empty());
    /// ```
    pub fn overlap_capsule(&self, center: Vec2, height: f32, radius: f32, filter: QueryFilter, out: &mut Vec<Entity>) {
        self.overlap(&Shape::Capsule { height, radius }, center, 0.0, filter, out);
    }

    /// The nearest collider to `point` within `max_distance`, and its
    /// nearest point. A point inside a collider is its own nearest point
    /// (distance 0, `inside` set). Snapping to a surface, "how far is the
    /// nearest wall".
    ///
    /// ```
    /// use ponko::prelude::*;
    /// fn nearest(ctx: &mut Ctx) {
    ///     let filter = ponko::d2::QueryFilter::default();
    ///     if let Some(near) = ctx.physics.closest_point(Vec2::ZERO, 500.0, filter) {
    ///         ctx.world.insert_resource(near);
    ///     }
    /// }
    /// let world = Game::new("t", 1, 1).physics(Vec2::ZERO).sim_system(nearest).run_headless(2, |ctx| {
    ///     ctx.world.spawn((ponko::d2::Transform::at(vec2(100.0, 0.0)), ponko::d2::RigidBody::fixed(), ponko::d2::Collider::circle(10.0)));
    /// });
    /// let near = world.resource::<ponko::d2::PointHit>();
    /// assert_eq!((near.point, near.distance, near.inside), (vec2(90.0, 0.0), 90.0, false));
    /// ```
    pub fn closest_point(&self, point: Vec2, max_distance: f32, filter: QueryFilter) -> Option<PointHit> {
        if !point.is_finite() || !in_range(max_distance) {
            return None;
        }
        let (handle, projection) =
            self.world
                .project_point(to_rp(point), max_distance, true, self.query_filter(filter))?;
        let nearest = from_rp(projection.point);
        Some(PointHit {
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
    /// `rotation` (radians), overlaps: how deep, and which way pushes the
    /// shape out, deepest first (equal depths by entity id), into `out`
    /// (cleared first). Moving the shape by `normal * depth` of the first
    /// entry frees it from that collider: the step a character controller
    /// takes after it was pushed into a wall.
    ///
    /// The shape's own offset doesn't apply (place it with `position`). A
    /// polygon query shape builds its convex hull on each call. Circle,
    /// rectangle and capsule queries reuse per-thread solver scratch memory
    /// after warm-up and do not allocate, including against polygon colliders,
    /// when `out` has enough capacity.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let physics = ponko::d2::Physics::default();
    /// let mut pushes = Vec::new();
    /// let player = ponko::d2::Collider::capsule(24.0, 8.0).shape;
    /// physics.penetrations(player, Vec2::ZERO, 0.0, ponko::d2::QueryFilter::default(), &mut pushes);
    /// assert!(pushes.is_empty());
    /// ```
    pub fn penetrations(
        &self,
        shape: Shape,
        position: Vec2,
        rotation: f32,
        filter: QueryFilter,
        out: &mut Vec<Penetration>,
    ) {
        out.clear();
        if !position.is_finite() || !rotation.is_finite() {
            return;
        }
        let pose = rp::Pose::new(to_rp(position), rotation);
        with_shape(&shape, |s| {
            self.each_penetration(s, &pose, filter, |p| out.push(p));
        });
        out.sort_unstable_by(|a, b| {
            b.depth
                .total_cmp(&a.depth)
                .then(a.entity.to_bits().cmp(&b.entity.to_bits()))
        });
    }

    fn each_penetration(
        &self,
        shape: &dyn rp::Shape,
        pose: &rp::Pose,
        filter: QueryFilter,
        mut f: impl FnMut(Penetration),
    ) {
        for (handle, collider) in self.world.intersect_shape(*pose, shape, self.query_filter(filter)) {
            let Some(entity) = self.entity_of(handle) else { continue };
            if let Some(c) = scratch::contact(pose, shape, collider.position(), collider.shape()) {
                f(Penetration {
                    entity,
                    point: from_rp(c.point2),
                    normal: from_rp(c.normal2),
                    depth: (-c.dist).max(0.0),
                });
            }
        }
    }
}

/// Parry's convex contacts with one penetration-depth solver per thread.
/// GJK and solid overlap tests use stack memory, but EPA needs reusable
/// heap-backed scratch storage once the shapes penetrate.
mod scratch {
    use rapier2d::parry::math::{Pose, Real, Vector};
    use rapier2d::parry::query::epa::EPA;
    use rapier2d::parry::query::gjk::{self, ConstantOrigin, CsoPoint, GJKResult, VoronoiSimplex};
    use rapier2d::parry::query::{Contact, DefaultQueryDispatcher, QueryDispatcher};
    use rapier2d::parry::shape::{Ball, ConvexPolygon, Shape};
    use std::cell::RefCell;

    thread_local! {
        static EPA_SCRATCH: RefCell<EPA> = RefCell::new(EPA::new());
    }

    pub(super) fn contact(pose1: &Pose, shape1: &dyn Shape, pose2: &Pose, shape2: &dyn Shape) -> Option<Contact> {
        let pos12 = pose1.inv_mul(pose2);
        let mut c = if let (Some(poly), Some(ball)) = (shape1.as_convex_polygon(), shape2.as_ball()) {
            polygon_ball(&pos12, poly, ball, 0.0)
        } else if let (Some(ball), Some(poly)) = (shape1.as_ball(), shape2.as_convex_polygon()) {
            polygon_ball(&pos12.inverse(), poly, ball, 0.0).map(Contact::flipped)
        } else if shape1.as_ball().is_some() || shape2.as_ball().is_some() {
            DefaultQueryDispatcher.contact(&pos12, shape1, shape2, 0.0).ok()?
        } else {
            support_maps(&pos12, shape1.as_support_map()?, shape2.as_support_map()?, 0.0)
        }?;
        c.transform_by_mut(pose1, pose2);
        Some(c)
    }

    /// GJK, then (when the shapes overlap) the reused EPA, as parry's
    /// `contact_support_map_support_map`.
    fn support_maps(
        pos12: &Pose,
        g1: &dyn rapier2d::parry::shape::SupportMap,
        g2: &dyn rapier2d::parry::shape::SupportMap,
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
    fn project(poly: &ConvexPolygon, point: Vector) -> (Vector, bool) {
        let m = Pose::new(-point, 0.0);
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

    /// parry's `contact_convex_polygon_ball`, projecting with [`project`].
    fn polygon_ball(pos12: &Pose, poly: &ConvexPolygon, ball: &Ball, prediction: Real) -> Option<Contact> {
        let center = pos12.translation;
        let (nearest, inside) = project(poly, center);
        let (dir, len) = (nearest - center).normalize_and_length();
        if len < rapier2d::parry::math::DEFAULT_EPSILON {
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
}
