//! The views debug drawing can show by itself: colliders, the navigation
//! mesh, bounds, light volumes and the hierarchy's axes. Each is drawn
//! from the world as it is when the frame is drawn, through the same
//! calls a game makes, so it costs nothing while it is off.

use glam::{Quat, Vec2, Vec3};

use super::DebugDraw;
use crate::color::Color;
use crate::ctx::Ctx;
use ponko_ecs::{Entity, World};

/// A view the engine draws by itself while it is on
/// ([`DebugDraw::show`]) and debug drawing is enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DebugView {
    /// Physics colliders, 2D (feature `physics`) and 3D (feature
    /// `physics3d`): green, sensors yellow. 3D collision meshes and terrain
    /// (`TerrainCollider`, feature `physics3d`) show their
    /// triangles within [`collider_distance`](DebugDraw::collider_distance)
    /// of the 3D camera; convex hulls show all their edges.
    Colliders,
    /// The 3D navigation mesh's polygon edges (feature `navmesh`), blue.
    NavMesh,
    /// Bounds: each 3D model's bounding sphere (what culling tests), and
    /// each sprite's rectangle, cyan.
    Bounds,
    /// Light volumes in the light's color: 3D point lights' range spheres
    /// and spot lights' cones, 2D point lights' circles and spot lights'
    /// wedges.
    Lights,
    /// Every transform's axes (x red, y green, z blue in 3D; x red, y
    /// green in 2D) and a white line from each attached entity to the one
    /// it follows.
    Hierarchy,
}

impl DebugView {
    /// Every view, in the order the control endpoint lists them.
    pub const ALL: [DebugView; 5] = [
        DebugView::Colliders,
        DebugView::NavMesh,
        DebugView::Bounds,
        DebugView::Lights,
        DebugView::Hierarchy,
    ];

    /// Its name, as the control endpoint takes it: `colliders`, `navmesh`,
    /// `bounds`, `lights`, `hierarchy`.
    ///
    /// ```
    /// use ponko::debug::DebugView;
    /// assert_eq!(DebugView::NavMesh.name(), "navmesh");
    /// assert_eq!(DebugView::from_name("navmesh"), Some(DebugView::NavMesh));
    /// ```
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            DebugView::Colliders => "colliders",
            DebugView::NavMesh => "navmesh",
            DebugView::Bounds => "bounds",
            DebugView::Lights => "lights",
            DebugView::Hierarchy => "hierarchy",
        }
    }

    /// The view called `name` (see [`name`](Self::name)).
    #[must_use]
    pub fn from_name(name: &str) -> Option<DebugView> {
        Self::ALL.into_iter().find(|v| v.name() == name)
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Which views are on, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Views(u8);

impl Views {
    pub(crate) fn set(&mut self, view: DebugView, on: bool) {
        if on {
            self.0 |= view.bit();
        } else {
            self.0 &= !view.bit();
        }
    }

    pub(crate) fn get(self, view: DebugView) -> bool {
        self.0 & view.bit() != 0
    }
}

#[cfg_attr(not(any(feature = "physics", feature = "physics3d")), allow(dead_code))]
const COLLIDER: Color = Color::rgb(0.2, 1.0, 0.3);
#[cfg_attr(not(any(feature = "physics", feature = "physics3d")), allow(dead_code))]
const SENSOR: Color = Color::rgb(1.0, 0.9, 0.2);
const BOUNDS: Color = Color::rgb(0.2, 0.9, 1.0);
#[cfg_attr(not(feature = "navmesh"), allow(dead_code))]
const NAVMESH: Color = Color::rgb(0.3, 0.5, 1.0);
const LINK: Color = Color::WHITE;
/// Length of the 2D hierarchy axes, in pixels.
const AXES_2D: f32 = 24.0;
/// Length of the 3D hierarchy axes, in world units.
#[cfg_attr(not(feature = "3d"), allow(dead_code))]
const AXES_3D: f32 = 0.5;

/// Draws the views that are on into `ctx.debug`, for this frame.
pub(crate) fn draw(ctx: &mut Ctx) {
    let views = ctx.debug.views;
    if views == Views::default() {
        return;
    }
    if views.get(DebugView::Colliders) {
        colliders(ctx);
    }
    if views.get(DebugView::NavMesh) {
        navmesh(ctx);
    }
    if views.get(DebugView::Bounds) {
        bounds(ctx);
    }
    if views.get(DebugView::Lights) {
        lights(&mut ctx.world, &mut ctx.debug);
    }
    if views.get(DebugView::Hierarchy) {
        let zoom = ctx.camera.zoom.max(f32::MIN_POSITIVE);
        hierarchy(&mut ctx.world, &mut ctx.debug, AXES_2D / zoom);
    }
}

fn turn_2d(rotation: f32, v: Vec2) -> Vec2 {
    Vec2::from_angle(rotation).rotate(v)
}

fn colliders(ctx: &mut Ctx) {
    #[cfg(feature = "physics")]
    {
        use crate::physics::Shape;
        let debug = &mut ctx.debug;
        ctx.world
            .query::<(&crate::d2::Transform, &crate::d2::Collider)>()
            .for_each(|(t, c)| {
                let color = if c.sensor { SENSOR } else { COLLIDER };
                let center = t.position + turn_2d(t.rotation, c.offset);
                match c.shape {
                    Shape::Circle { radius } => {
                        debug.circle(center, radius, color);
                    }
                    Shape::Rect { size } => {
                        debug.obb(center, size, Quat::from_rotation_z(t.rotation), color);
                    }
                    Shape::Capsule { height, radius } => {
                        let up = turn_2d(t.rotation, Vec2::new(0.0, height * 0.5));
                        debug.capsule(center - up, center + up, radius, color);
                    }
                    Shape::Polygon { .. } => {
                        let points = c.shape.points();
                        let mut corners = [Vec2::ZERO; Shape::MAX_POLYGON_POINTS];
                        for (to, p) in corners.iter_mut().zip(points) {
                            *to = center + turn_2d(t.rotation, *p);
                        }
                        debug.polyline(&corners[..points.len()], true, color);
                    }
                }
            });
    }
    #[cfg(feature = "physics3d")]
    {
        use crate::physics3d_impl::Shape3D;
        let (debug, physics) = (&mut ctx.debug, &ctx.physics3d);
        let eye = (ctx.scene3d.camera.position, debug.collider_distance());
        ctx.world
            .query::<(&crate::d3::Transform, &crate::d3::Collider)>()
            .for_each(|(t, c)| {
                let color = if c.sensor { SENSOR } else { COLLIDER };
                let center = t.position + t.rotation * c.offset;
                match c.shape {
                    Shape3D::Sphere { radius } => {
                        debug.sphere(center, radius, color);
                    }
                    Shape3D::Cuboid { size } => {
                        debug.obb(center, size, t.rotation, color);
                    }
                    Shape3D::Capsule { height, radius } => {
                        let up = t.rotation * Vec3::new(0.0, height * 0.5, 0.0);
                        debug.capsule(center - up, center + up, radius, color);
                    }
                    Shape3D::Mesh(mesh) => {
                        mesh_edges(debug, physics, (center, t.rotation, Vec3::ONE), mesh, eye, color);
                    }
                    Shape3D::ScaledMesh { mesh, scale } => {
                        mesh_edges(debug, physics, (center, t.rotation, scale), mesh, eye, color);
                    }
                }
            });
        // Terrain: scaled with its tile, placed by its corner.
        ctx.world
            .query::<(&crate::d3::Transform, &crate::d3::TerrainCollider)>()
            .for_each(|(t, c)| {
                mesh_edges(debug, physics, (t.position, t.rotation, t.scale), c.mesh, eye, COLLIDER);
            });
    }
    let _ = ctx;
}

/// The triangles of a collision mesh placed at `at` (position, rotation,
/// scale) within `reach` of `eye`, as lines.
#[cfg(feature = "physics3d")]
fn mesh_edges(
    debug: &mut DebugDraw,
    physics: &crate::physics3d_impl::Physics3D,
    (at, rotation, scale): (Vec3, Quat, Vec3),
    mesh: crate::physics3d_impl::CollisionMesh,
    (eye, reach): (Vec3, f32),
    color: Color,
) {
    let near = rotation.inverse() * (eye - at);
    physics.mesh_edges_near(mesh, scale, (near, reach), |a, b| {
        debug.line(at + rotation * a, at + rotation * b, color);
    });
}

fn navmesh(ctx: &mut Ctx) {
    #[cfg(feature = "navmesh")]
    {
        let debug = &mut ctx.debug;
        if let Some(nav) = ctx.world.get_resource::<crate::d3::NavMesh>() {
            nav.for_each_edge(|a, b| {
                debug.line(a, b, NAVMESH);
            });
        }
    }
    let _ = ctx;
}

fn bounds(ctx: &mut Ctx) {
    #[cfg(feature = "3d")]
    {
        let (debug, assets) = (&mut ctx.debug, &ctx.gfx.assets3d);
        ctx.world
            .query::<(&crate::d3::Transform, &crate::render::Model)>()
            .for_each(|(t, m)| {
                let Some(&(center, radius)) = assets.mesh_slot(m.mesh.id).and_then(|s| assets.bounds.get(s)) else {
                    return;
                };
                if radius < 0.0 {
                    return; // freed
                }
                let scale = t.scale.abs().max_element();
                debug.sphere(t.matrix().transform_point3(center), radius * scale, BOUNDS);
            });
    }
    let debug = &mut ctx.debug;
    ctx.world
        .query::<(&crate::d2::Transform, &crate::d2::Sprite)>()
        .for_each(|(t, s)| {
            let size = s.region.map_or_else(|| s.texture.size(), |r| r.size()) * t.scale.abs();
            let center = t.position + turn_2d(t.rotation, (Vec2::splat(0.5) - s.pivot) * size);
            debug.obb(center, size, Quat::from_rotation_z(t.rotation), BOUNDS);
        });
}

fn lights(world: &mut World, debug: &mut DebugDraw) {
    #[cfg(feature = "3d")]
    {
        world
            .query::<(&crate::d3::Transform, &crate::d3::PointLight)>()
            .for_each(|(t, l)| {
                debug.sphere(t.position, l.range, l.color.with_alpha(1.0));
            });
        world
            .query::<(&crate::d3::Transform, &crate::d3::SpotLight)>()
            .for_each(|(t, l)| {
                let color = l.color.with_alpha(1.0);
                let forward = t.forward().try_normalize().unwrap_or(Vec3::NEG_Z);
                let end = t.position + forward * l.range;
                let radius = l.range * l.outer_angle.clamp(0.0, 1.5).tan();
                let p = forward.any_orthonormal_vector();
                let q = forward.cross(p);
                for side in [p, q, -p, -q] {
                    debug.line(t.position, end + side * radius, color);
                }
                debug.capsule_ring(end, (p, q), radius, color);
            });
    }
    world
        .query::<(&crate::d2::Transform, &crate::d2::PointLight)>()
        .for_each(|(t, l)| {
            debug.circle(t.position, l.radius, l.color.with_alpha(1.0));
        });
    world
        .query::<(&crate::d2::Transform, &crate::d2::SpotLight)>()
        .for_each(|(t, l)| {
            let color = l.color.with_alpha(1.0);
            let left = t.position + Vec2::from_angle(t.rotation - l.angle) * l.radius;
            let right = t.position + Vec2::from_angle(t.rotation + l.angle) * l.radius;
            debug.line(t.position, left, color);
            debug.line(t.position, right, color);
            debug.wedge_arc(
                t.position,
                l.radius,
                (t.rotation - l.angle, t.rotation + l.angle),
                color,
            );
        });
}

fn hierarchy(world: &mut World, debug: &mut DebugDraw, axes_2d: f32) {
    // Entities and where they are first, then who they hang off: a query
    // holds the world, and the parent lookups read it.
    let mut placed = std::mem::take(&mut debug.buf.placed);
    #[cfg(feature = "3d")]
    {
        placed.clear();
        world.query::<(Entity, &crate::d3::Transform)>().for_each(|(e, t)| {
            debug.axes(t.position, t.rotation, AXES_3D);
            placed.push((e, t.position));
        });
        for &(e, at) in &placed {
            if let Some(parent) = parent_3d(world, e).and_then(|p| world.get::<crate::d3::Transform>(p)) {
                debug.line(parent.position, at, LINK);
            }
        }
    }
    placed.clear();
    world.query::<(Entity, &crate::d2::Transform)>().for_each(|(e, t)| {
        debug.line(
            t.position,
            t.position + turn_2d(t.rotation, Vec2::X * axes_2d),
            Color::RED,
        );
        debug.line(
            t.position,
            t.position + turn_2d(t.rotation, Vec2::Y * axes_2d),
            Color::GREEN,
        );
        placed.push((e, t.position.extend(0.0)));
    });
    for &(e, at) in &placed {
        if let Some(parent) = parent_2d(world, e).and_then(|p| world.get::<crate::d2::Transform>(p)) {
            debug.line(parent.position, at.truncate(), LINK);
        }
    }
    placed.clear();
    debug.buf.placed = placed;
}

/// The entity `e` hangs off in 2D, for the hierarchy view: the one its
/// [`Attached`](crate::attach::Attached) follows.
fn parent_2d(world: &World, e: Entity) -> Option<Entity> {
    world.get::<crate::attach::Attached>(e).map(|a| a.to)
}

/// The entity `e` hangs off in 3D, for the hierarchy view: the one its
/// [`Attached3D`](crate::attach::Attached3D) follows.
#[cfg(feature = "3d")]
fn parent_3d(world: &World, e: Entity) -> Option<Entity> {
    world.get::<crate::attach::Attached3D>(e).map(|a| a.to)
}

impl DebugDraw {
    /// A ring around `center` in the plane of `u` and `v` (built-in views).
    #[cfg(feature = "3d")]
    fn capsule_ring(&mut self, center: Vec3, plane: (Vec3, Vec3), radius: f32, color: Color) {
        if self.begin().is_some() {
            let full = (0.0, std::f32::consts::TAU);
            self.push_arc(
                center,
                plane,
                radius,
                full,
                super::CIRCLE_SEGMENTS,
                color.to_rgba8(),
                true,
            );
        }
    }

    /// A 2D arc from angle `from` to `to` (built-in views).
    fn wedge_arc(&mut self, center: Vec2, radius: f32, (from, to): (f32, f32), color: Color) {
        if self.begin().is_some() {
            let plane = (Vec3::X, Vec3::Y);
            let segments = super::CIRCLE_SEGMENTS / 2;
            self.push_arc(
                center.extend(0.0),
                plane,
                radius,
                (from, to),
                segments,
                color.to_rgba8(),
                false,
            );
        }
    }
}
