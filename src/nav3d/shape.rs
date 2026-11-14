//! Obstacle footprints: the shapes a [`NavObstacle3D`] can take and the 2D
//! geometry that blocking and carving test them with.

use super::NavObstacle3D;
use glam::{Vec2, Vec3};

/// The footprint of a [`NavObstacle`](crate::d3::NavObstacle), an upright
/// prism turned by its `yaw` around world Y. Its height is always
/// `center.y ± half_extents.y`.
/// ```
/// use ponko::prelude::*;
/// use ponko::d3::{NavObstacle, NavObstacleShape};
/// assert_eq!(NavObstacle::new(Vec3::ZERO, Vec3::ONE).shape, NavObstacleShape::Box);
/// let pillar = NavObstacle::cylinder(Vec3::ZERO, 0.5, 1.0);
/// assert_eq!(pillar.shape, NavObstacleShape::Cylinder);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[non_exhaustive]
pub enum NavObstacleShape {
    /// A box with half sizes `half_extents`.
    #[default]
    Box,
    /// The upright cylinder inside the box: radius `half_extents.x` along
    /// the obstacle's X and `half_extents.z` along its Z (equal for a circle).
    Cylinder,
    /// A convex polygon of points around `center` in the obstacle's own XZ
    /// plane; only `half_extents.y` (the half height) is used.
    Convex(NavConvexPolygon),
}

/// A convex polygon of 3 to [`MAX_POINTS`](Self::MAX_POINTS) points for
/// [`NavObstacleShape::Convex`]. Each point is an offset `(x, z)` from the
/// obstacle's center, before its `yaw` turns it.
/// ```
/// use ponko::prelude::*;
/// let wedge = ponko::d3::NavConvexPolygon::new(&[vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 2.0)]).unwrap();
/// assert_eq!(wedge.points().len(), 3);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavConvexPolygon {
    points: [Vec2; NavConvexPolygon::MAX_POINTS],
    len: u8,
}
impl NavConvexPolygon {
    /// Most points a polygon holds.
    /// ```
    /// assert_eq!(ponko::d3::NavConvexPolygon::MAX_POINTS, 8);
    /// ```
    pub const MAX_POINTS: usize = 8;
    /// A polygon through `points` in order, either winding. `None` unless
    /// there are 3 to 8 finite points forming a strictly convex polygon
    /// (no repeated points, no three in a line, no reflex corner).
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::d3::NavConvexPolygon;
    /// assert!(NavConvexPolygon::new(&[Vec2::ZERO, Vec2::X, Vec2::Y]).is_some());
    /// // An arrow head is not convex.
    /// let arrow = [vec2(0.0, 0.0), vec2(2.0, 1.0), vec2(0.0, 2.0), vec2(0.5, 1.0)];
    /// assert!(NavConvexPolygon::new(&arrow).is_none());
    /// ```
    #[must_use]
    pub fn new(points: &[Vec2]) -> Option<Self> {
        let n = points.len();
        if !(3..=Self::MAX_POINTS).contains(&n) || points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let turn = |i: usize| {
            let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
            (b - a).perp_dot(c - b)
        };
        let sign = turn(0).signum();
        if turn(0) == 0.0 || (0..n).any(|i| turn(i) * sign <= 0.0) {
            return None;
        }
        // Equal turn signs still allow a star that winds twice; a convex
        // polygon turns through exactly one full circle.
        let mut angle = 0.0f64;
        for i in 0..n {
            let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
            let (u, v) = ((b - a).as_dvec2(), (c - b).as_dvec2());
            angle += u.perp_dot(v).atan2(u.dot(v)).abs();
        }
        if angle > std::f64::consts::TAU + 1e-3 {
            return None;
        }
        let mut stored = [Vec2::ZERO; Self::MAX_POINTS];
        stored[..n].copy_from_slice(points);
        if sign < 0.0 {
            stored[..n].reverse();
        }
        Some(Self {
            points: stored,
            len: n as u8,
        })
    }
    /// The points, counter-clockwise in the (x, z) plane (each corner turns
    /// left with x to the right and z up), whatever winding [`new`](Self::new)
    /// was given.
    /// ```
    /// use ponko::prelude::*;
    /// let p = ponko::d3::NavConvexPolygon::new(&[Vec2::ZERO, Vec2::Y, Vec2::X]).unwrap();
    /// assert_eq!(p.points()[0], Vec2::X);
    /// ```
    #[must_use]
    pub fn points(&self) -> &[Vec2] {
        &self.points[..usize::from(self.len)]
    }
}

/// Points of the polygon that stands in for a cylinder where polygons are
/// needed (bounds and the polygon overlap test). It is circumscribed, so it
/// never blocks less than the cylinder.
const CIRCLE_POINTS: usize = 16;

/// A validated obstacle in world space: its XZ footprint and height range.
#[derive(Clone, Copy, Debug)]
pub(super) struct Footprint {
    /// Footprint center (world X, Z).
    center: Vec2,
    /// `cos(yaw)`, `sin(yaw)`.
    rotation: Vec2,
    /// Cylinder radii; zero for polygons.
    radii: Vec2,
    cylinder: bool,
    /// World-space polygon, counter-clockwise in the (x, z) plane. For a
    /// cylinder, a circumscribed polygon.
    points: [Vec2; CIRCLE_POINTS],
    len: usize,
    /// World Y range.
    pub(super) bottom: f32,
    pub(super) top: f32,
}

impl Footprint {
    /// `None` for an obstacle with nonfinite values or negative sizes.
    pub(super) fn of(o: &NavObstacle3D) -> Option<Self> {
        if !o.center.is_finite()
            || !o.half_extents.is_finite()
            || o.half_extents.min_element() < 0.0
            || !o.yaw.is_finite()
        {
            return None;
        }
        let rotation = Vec2::new(libm::cosf(o.yaw), libm::sinf(o.yaw));
        let center = Vec2::new(o.center.x, o.center.z);
        // glam's `Quat::from_rotation_y(yaw)` convention: +X turns toward -Z.
        let to_world =
            |p: Vec2| center + Vec2::new(p.x * rotation.x + p.y * rotation.y, p.y * rotation.x - p.x * rotation.y);
        let he = Vec2::new(o.half_extents.x, o.half_extents.z);
        let mut points = [Vec2::ZERO; CIRCLE_POINTS];
        let mut radii = Vec2::ZERO;
        let len = match o.shape {
            NavObstacleShape::Box => {
                for (i, s) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .into_iter()
                    .enumerate()
                {
                    points[i] = to_world(he * Vec2::new(s.0, s.1));
                }
                4
            }
            NavObstacleShape::Cylinder => {
                radii = he;
                // Vertices at the circumscribed radius r / cos(pi / n).
                let scale = 1.0 / libm::cosf(std::f32::consts::PI / CIRCLE_POINTS as f32);
                for (i, p) in points.iter_mut().enumerate() {
                    let a = i as f32 * std::f32::consts::TAU / CIRCLE_POINTS as f32;
                    *p = to_world(he * scale * Vec2::new(libm::cosf(a), libm::sinf(a)));
                }
                CIRCLE_POINTS
            }
            NavObstacleShape::Convex(polygon) => {
                // A rotation keeps the counter-clockwise order.
                let n = polygon.points().len();
                for (i, &p) in polygon.points().iter().enumerate() {
                    points[i] = to_world(p);
                }
                n
            }
        };
        Some(Self {
            center,
            rotation,
            radii,
            cylinder: o.shape == NavObstacleShape::Cylinder,
            points,
            len,
            bottom: o.center.y - o.half_extents.y,
            top: o.center.y + o.half_extents.y,
        })
    }

    fn points(&self) -> &[Vec2] {
        &self.points[..self.len]
    }

    /// World XZ bounds of the footprint.
    pub(super) fn bounds(&self) -> (Vec2, Vec2) {
        self.points().iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    /// Whether the world XZ point `p` lies within `margin` of the footprint.
    /// Exact for boxes, polygons and circles; an elliptic cylinder grows each
    /// radius by `margin` instead.
    pub(super) fn reaches(&self, p: Vec2, margin: f32) -> bool {
        if self.cylinder {
            let d = p - self.center;
            let local = Vec2::new(
                d.x * self.rotation.x - d.y * self.rotation.y,
                d.x * self.rotation.y + d.y * self.rotation.x,
            );
            let r = self.radii + margin;
            if r.min_element() <= 0.0 {
                return false;
            }
            let q = local / r;
            return q.length_squared() <= 1.0;
        }
        let pts = self.points();
        let mut inside = true;
        let mut nearest = f32::INFINITY;
        for (i, &a) in pts.iter().enumerate() {
            let b = pts[(i + 1) % pts.len()];
            let edge = b - a;
            // Counter-clockwise: the interior is on the left of every edge.
            if edge.perp_dot(p - a) < 0.0 {
                inside = false;
            }
            let t = if edge.length_squared() > 0.0 {
                ((p - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            nearest = nearest.min(p.distance_squared(a + edge * t));
        }
        inside || nearest <= margin * margin
    }

    /// Whether the footprint overlaps the XZ rectangle `[min, max]`
    /// (separating axes; a cylinder uses its circumscribed polygon).
    pub(super) fn overlaps_rect(&self, min: Vec2, max: Vec2) -> bool {
        let (lo, hi) = self.bounds();
        if hi.x < min.x || lo.x > max.x || hi.y < min.y || lo.y > max.y {
            return false;
        }
        let corners = [min, Vec2::new(min.x, max.y), max, Vec2::new(max.x, min.y)];
        let pts = self.points();
        for (i, &a) in pts.iter().enumerate() {
            let axis = (pts[(i + 1) % pts.len()] - a).perp();
            let project = |q: &Vec2| axis.dot(*q);
            let (pa, pb) = pts
                .iter()
                .map(project)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| (l.min(v), h.max(v)));
            let (ra, rb) = corners
                .iter()
                .map(project)
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| (l.min(v), h.max(v)));
            if pb < ra || rb < pa {
                return false;
            }
        }
        true
    }
}

/// Whether two obstacles differ enough to carve again: a different shape or
/// size, or any point of the footprint moving at least `threshold`.
pub(super) fn carve_moved(a: &NavObstacle3D, b: &NavObstacle3D, threshold: f32) -> bool {
    if a.half_extents != b.half_extents || a.shape != b.shape || !a.center.is_finite() || !a.yaw.is_finite() {
        return true;
    }
    let reach = match a.shape {
        NavObstacleShape::Convex(p) => p.points().iter().fold(0.0f32, |m, q| m.max(q.length())),
        _ => Vec2::new(a.half_extents.x, a.half_extents.z).length(),
    };
    // A turn by d radians moves a point at distance r by at most r * d.
    a.center.distance(b.center) + reach * (a.yaw - b.yaw).abs() >= threshold
}

/// The world bounds a footprint can touch once grown by `margin` sideways and
/// `below` downwards (the agent's clearance).
pub(super) fn reach_bounds(f: &Footprint, margin: f32, below: f32) -> (Vec3, Vec3) {
    let (lo, hi) = f.bounds();
    (
        Vec3::new(lo.x - margin, f.bottom - below, lo.y - margin),
        Vec3::new(hi.x + margin, f.top, hi.y + margin),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obstacle(shape: NavObstacleShape, half: Vec3, yaw: f32) -> NavObstacle3D {
        let mut o = NavObstacle3D::new(Vec3::ZERO, half);
        o.shape = shape;
        o.yaw = yaw;
        o
    }

    #[test]
    fn convex_polygons_reject_reflex_repeated_and_star_shapes() {
        assert!(NavConvexPolygon::new(&[Vec2::ZERO, Vec2::X]).is_none());
        assert!(NavConvexPolygon::new(&[Vec2::ZERO, Vec2::X, Vec2::X * 2.0]).is_none());
        assert!(NavConvexPolygon::new(&[Vec2::ZERO, Vec2::X, Vec2::X, Vec2::Y]).is_none());
        assert!(NavConvexPolygon::new(&[Vec2::ZERO, Vec2::X, Vec2::new(f32::NAN, 1.0)]).is_none());
        let star: Vec<Vec2> = (0..5)
            .map(|i| {
                let a = i as f32 * 2.0 * std::f32::consts::TAU / 5.0;
                Vec2::new(a.cos(), a.sin())
            })
            .collect();
        assert!(NavConvexPolygon::new(&star).is_none());
        assert!(NavConvexPolygon::new(&[Vec2::ZERO; 9]).is_none());
        let square = [Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y];
        let mut reversed = square;
        reversed.reverse();
        assert_eq!(NavConvexPolygon::new(&square), NavConvexPolygon::new(&reversed));
    }

    #[test]
    fn a_turned_box_reaches_its_own_corners_but_not_its_bounds_corners() {
        let f = Footprint::of(&obstacle(
            NavObstacleShape::Box,
            Vec3::new(2.0, 1.0, 0.25),
            std::f32::consts::FRAC_PI_4,
        ))
        .unwrap();
        // +X turns toward -Z: the long axis runs from (+x, -z) to (-x, +z).
        let along = Vec2::new(1.0, -1.0).normalize();
        assert!(f.reaches(along * 1.9, 0.0));
        assert!(!f.reaches(Vec2::new(1.0, 1.0), 0.0));
        assert!(f.reaches(Vec2::new(0.3, 0.3), 0.2));
        let (lo, hi) = f.bounds();
        assert!(hi.x > 1.4 && lo.y < -1.4);
        assert!(!f.overlaps_rect(Vec2::new(0.6, 0.6), Vec2::new(1.5, 1.5)));
        assert!(f.overlaps_rect(Vec2::new(-0.1, -0.1), Vec2::new(0.1, 0.1)));
    }

    #[test]
    fn a_cylinder_reaches_its_radius_but_not_its_bounds_corners() {
        let f = Footprint::of(&obstacle(NavObstacleShape::Cylinder, Vec3::ONE, 0.0)).unwrap();
        assert!(f.reaches(Vec2::new(0.99, 0.0), 0.0));
        assert!(!f.reaches(Vec2::new(0.8, 0.8), 0.0));
        assert!(f.reaches(Vec2::new(0.8, 0.8), 0.15));
        assert!(!f.overlaps_rect(Vec2::new(0.8, 0.8), Vec2::new(2.0, 2.0)));
        // The circumscribed polygon never blocks less than the cylinder.
        for i in 0..64 {
            let a = i as f32 * std::f32::consts::TAU / 64.0;
            let p = Vec2::new(a.cos(), a.sin()) * 0.999;
            assert!(f.overlaps_rect(p, p), "{p}");
        }
    }

    #[test]
    fn convex_footprints_follow_their_points() {
        let wedge = NavConvexPolygon::new(&[Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 2.0)]).unwrap();
        let f = Footprint::of(&obstacle(NavObstacleShape::Convex(wedge), Vec3::ONE, 0.0)).unwrap();
        assert!(f.reaches(Vec2::new(0.0, 1.0), 0.0));
        assert!(!f.reaches(Vec2::new(0.9, 1.5), 0.0));
        assert!(!f.reaches(Vec2::new(0.0, -0.5), 0.2));
        assert!(f.reaches(Vec2::new(0.0, -0.5), 0.6));
        // Half a turn mirrors it through the center.
        let turned = Footprint::of(&obstacle(
            NavObstacleShape::Convex(wedge),
            Vec3::ONE,
            std::f32::consts::PI,
        ))
        .unwrap();
        assert!(turned.reaches(Vec2::new(0.0, -1.0), 0.0));
        assert!(!turned.reaches(Vec2::new(0.0, 1.0), 0.0));
    }

    #[test]
    fn invalid_obstacles_have_no_footprint() {
        assert!(Footprint::of(&obstacle(NavObstacleShape::Box, Vec3::ONE, f32::NAN)).is_none());
        assert!(Footprint::of(&obstacle(NavObstacleShape::Box, -Vec3::ONE, 0.0)).is_none());
    }
}
