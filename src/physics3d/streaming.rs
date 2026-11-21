//! Collision meshes built on any thread, colliders for terrain tiles, and
//! keeping static colliders only near the places that need them, for big
//! streamed worlds.

use rapier3d::parry::shape::{HeightField, HeightFieldCellStatus};
use rapier3d::parry::utils::Array2;
use rapier3d::prelude as rp;
use serde::{Deserialize, Serialize};

use super::{CollisionGroups, CollisionMesh, Physics3D, pose_to_rp, to_rp};
use crate::d3::Transform as Transform3D;
use glam::{Vec2, Vec3};
use ponko_ecs::{Component, Entity, World};

/// A convex hull, triangle mesh or heightfield built on the CPU, on any
/// thread: the slow half of [`Physics3D::heightfield`] and friends. Build
/// it in a [`Job`](crate::streaming::Job) with the rest of a streamed cell, then hand
/// it to [`Physics3D::add_mesh`] on the game's thread, which is quick.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).physics3d(Vec3::ZERO).run_headless(0, |ctx| {
///     let job = ponko::streaming::Job::spawn(|| ponko::d3::CollisionMeshData::heightfield(&[0.0; 33 * 33], 64.0));
///     let ground = ctx.physics3d.add_mesh(&job.wait());
///     assert!(ctx.physics3d.is_live(ground));
/// });
/// ```
#[derive(Clone)]
pub struct CollisionMeshData {
    pub(super) shape: rp::SharedShape,
    /// From the entity's position to the shape's center.
    pub(super) offset: Vec3,
}

impl std::fmt::Debug for CollisionMeshData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollisionMeshData")
            .field("shape", &self.shape.shape_type())
            .field("offset", &self.offset)
            .finish()
    }
}

impl CollisionMeshData {
    /// Ground for a terrain tile, from the same n × n heights and size as
    /// [`TerrainTileData::new`](crate::terrain::TerrainTileData::new) (row after row
    /// along +Z, NaN for a hole), as [`Physics3D::heightfield`] makes it.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let ground = ponko::d3::CollisionMeshData::heightfield(&[0.0, 1.0, 0.0, 1.0], 4.0);
    /// # let _ = ground;
    /// ```
    ///
    /// # Panics
    ///
    /// If `heights` isn't a square grid of at least 2 × 2 or `size` isn't above 0.
    #[track_caller]
    #[must_use]
    pub fn heightfield(heights: &[f32], size: f32) -> Self {
        Self::heightfield_named(heights, size, "CollisionMeshData::heightfield")
    }

    #[track_caller]
    pub(super) fn heightfield_named(heights: &[f32], size: f32, name: &str) -> Self {
        let n = (heights.len() as f64).sqrt() as usize;
        assert!(
            n >= 2 && n * n == heights.len(),
            "{name}: {} heights isn't a square grid of at least 2 x 2",
            heights.len()
        );
        assert!(
            size > 0.0 && size.is_finite(),
            "{name}: size must be above 0 (got {size})"
        );
        // rapier's rows run along Z and its storage is by column: element
        // (z, x) sits at z + x * n, where ours is at z * n + x.
        let mut data = vec![0.0; n * n];
        for z in 0..n {
            for x in 0..n {
                let h = heights[z * n + x];
                data[z + x * n] = if h.is_finite() { h } else { 0.0 };
            }
        }
        let grid = rapier3d::parry::utils::Array2::new(n, n, data);
        let mut field = rapier3d::parry::shape::HeightField::with_flags(
            grid,
            rp::Vector::new(size, 1.0, size),
            rapier3d::parry::shape::HeightFieldFlags::FIX_INTERNAL_EDGES,
        );
        // Cells with a hole in any corner aren't drawn, so they aren't solid.
        for z in 0..n - 1 {
            for x in 0..n - 1 {
                let corners = [(x, z), (x + 1, z), (x, z + 1), (x + 1, z + 1)];
                if corners.iter().any(|&(x, z)| !heights[z * n + x].is_finite()) {
                    field.set_cell_status(z, x, rapier3d::parry::shape::HeightFieldCellStatus::CELL_REMOVED);
                }
            }
        }
        let half = size * 0.5;
        Self {
            shape: rp::SharedShape::new(field),
            offset: Vec3::new(half, 0.0, half),
        }
    }

    /// A convex hull around `points`, as [`Physics3D::convex_hull`] makes
    /// it. `None` if the points are all on one plane or not finite.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let corners = [vec3(0.0, 0.0, 0.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0)];
    /// assert!(ponko::d3::CollisionMeshData::convex_hull(&corners).is_some());
    /// ```
    #[must_use]
    pub fn convex_hull(points: &[Vec3]) -> Option<Self> {
        if points.len() < 4 || !points.iter().all(|p| p.is_finite()) {
            return None;
        }
        let points: Vec<rp::Vector> = points.iter().map(|&p| to_rp(p)).collect();
        let shape = rp::SharedShape::convex_hull(&points)?;
        // A hull of points on one plane has no volume.
        let mass = shape.mass_properties(1.0).mass();
        if !(mass > 0.0 && mass.is_finite()) {
            return None;
        }
        Some(Self {
            shape,
            offset: Vec3::ZERO,
        })
    }

    /// A triangle mesh, as [`Physics3D::trimesh`] makes it: `indices` are
    /// three per triangle into `vertices`. `None` if there are no
    /// triangles, an index is out of range or a vertex isn't finite.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let floor = ponko::d3::CollisionMeshData::trimesh(&[Vec3::ZERO, Vec3::X, Vec3::Z], &[[0, 2, 1]]);
    /// assert!(floor.is_some());
    /// ```
    #[must_use]
    pub fn trimesh(vertices: &[Vec3], indices: &[[u32; 3]]) -> Option<Self> {
        if indices.is_empty()
            || !vertices.iter().all(|p| p.is_finite())
            || indices.iter().flatten().any(|&i| i as usize >= vertices.len())
        {
            return None;
        }
        let vertices: Vec<rp::Vector> = vertices.iter().map(|&p| to_rp(p)).collect();
        let shape =
            rp::SharedShape::trimesh_with_flags(vertices, indices.to_vec(), rp::TriMeshFlags::FIX_INTERNAL_EDGES)
                .ok()?;
        Some(Self {
            shape,
            offset: Vec3::ZERO,
        })
    }
}

/// Makes an entity's terrain tile solid: a fixed collider of a
/// [heightfield](Physics3D::heightfield) made from the tile's heights, at
/// the entity's [`d3::Transform`](crate::d3::Transform) (the tile's corner), turned and scaled with
/// it, so it matches the drawn ground triangle for triangle. Holes are open.
/// Put it on the entity with the [`TerrainTile`](crate::terrain::TerrainTile) (not
/// with a [`RigidBody3D`](super::RigidBody3D)): despawning the tile removes
/// its collider, and giving it a new mesh after an edit swaps the collider
/// at the next physics step.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).physics3d(vec3(0.0, -9.81, 0.0)).run_headless(1, |ctx| {
///     let heights = [0.0; 33 * 33];
///     let ground = ctx.gfx.default_material();
///     let tile = ctx.gfx.create_terrain_tile(&heights, 64.0, ground);
///     let solid = ponko::d3::TerrainCollider::new(ctx.physics3d.heightfield(&heights, 64.0));
///     ctx.world.spawn((ponko::d3::Transform::at(vec3(-32.0, 0.0, -32.0)), tile, solid));
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Component)]
pub struct TerrainCollider {
    /// The heightfield, from [`Physics3D::heightfield`] or
    /// [`Physics3D::add_mesh`] (any collision mesh works). Serde writes it
    /// as `null` and reads it back as [`CollisionMesh::NONE`](crate::handle::Handle::NONE).
    #[serde(with = "super::unsaved_mesh")]
    pub mesh: CollisionMesh,
    /// 0 ice, 1 rubber. Default 0.5.
    pub friction: f32,
    /// Bounciness: 0 none (the default), 1 keeps all its speed.
    pub restitution: f32,
    /// Which colliders it touches. Default all.
    pub groups: CollisionGroups,
}

impl TerrainCollider {
    /// The ground of `mesh`, with the default surface.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(mesh: ponko::d3::CollisionMesh) {
    /// let ice = ponko::d3::TerrainCollider::new(mesh).friction(0.05);
    /// # }
    /// ```
    #[must_use]
    pub fn new(mesh: CollisionMesh) -> Self {
        Self {
            mesh,
            friction: 0.5,
            restitution: 0.0,
            groups: CollisionGroups::ALL,
        }
    }
    /// With this friction.
    #[must_use]
    pub fn friction(self, friction: f32) -> Self {
        Self { friction, ..self }
    }
    /// With this bounciness.
    #[must_use]
    pub fn restitution(self, restitution: f32) -> Self {
        Self { restitution, ..self }
    }
    /// In these collision groups.
    #[must_use]
    pub fn groups(self, groups: CollisionGroups) -> Self {
        Self { groups, ..self }
    }
}

/// What [`Physics3D::set_terrain_rebuild`] keeps.
pub(super) type TerrainRebuild = Box<dyn Fn(&World, Entity) -> Option<CollisionMeshData> + Send + Sync>;

impl Physics3D {
    /// Makes terrain tiles solid again after a world load. A saved world
    /// keeps each [`TerrainCollider`] (its friction, restitution and
    /// groups) but not its heightfield, which was made in code: after
    /// [`Ctx::load_world`](crate::ctx::Ctx::load_world) or
    /// [`load_scene`](crate::ctx::Ctx::load_scene), `rebuild` is called with
    /// the loaded world and each tile whose collider has no mesh, and the
    /// mesh it returns (the tile's heightfield, from the same heights as
    /// before: [`CollisionMeshData::heightfield`]) goes in the tile's
    /// collider, ready for the next physics step. The tile's saved
    /// components say which heights it had: its
    /// [`d3::Transform`](crate::d3::Transform), or a component of your own
    /// (a tile coordinate, a seed). `None` leaves that tile without ground.
    /// Set it once, in setup; it stays through loads.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # let dir = std::env::temp_dir().join(format!("ponko-doc-terrain-rebuild-{}", std::process::id()));
    /// use ponko::d3::{CollisionMeshData, TerrainCollider, Transform};
    /// fn heights() -> Vec<f32> {
    ///     vec![0.0; 33 * 33]
    /// }
    /// Game::new("t", 1, 1).data_dir(&dir).physics3d(vec3(0.0, -9.81, 0.0)).run_headless(1, |ctx| {
    ///     ctx.physics3d.set_terrain_rebuild(|_world, _tile| Some(CollisionMeshData::heightfield(&heights(), 64.0)));
    ///     let ground = ctx.physics3d.heightfield(&heights(), 64.0);
    ///     ctx.world.spawn((Transform::default(), TerrainCollider::new(ground)));
    ///     ctx.storage.save_world(&ctx.world, "slot").unwrap();
    ///     ctx.load_world("slot").unwrap();
    ///     let (_, loaded) = ctx.world.query::<(Entity, &TerrainCollider)>().iter().next().unwrap();
    ///     assert!(ctx.physics3d.is_live(loaded.mesh), "solid again");
    /// });
    /// # let _ = std::fs::remove_dir_all(dir);
    /// ```
    pub fn set_terrain_rebuild(
        &mut self,
        rebuild: impl Fn(&World, Entity) -> Option<CollisionMeshData> + Send + Sync + 'static,
    ) {
        self.terrain_rebuild = Some(Box::new(rebuild));
    }

    /// Stops making terrain tiles solid again after a load.
    pub fn clear_terrain_rebuild(&mut self) {
        self.terrain_rebuild = None;
    }

    /// After a load: a mesh from the rebuild for every terrain collider
    /// without one.
    pub(crate) fn rebuild_terrain(&mut self, world: &mut World) {
        let Some(rebuild) = self.terrain_rebuild.take() else {
            return;
        };
        let mut lost = Vec::new();
        world.query::<(Entity, &TerrainCollider)>().for_each(|(e, t)| {
            if !self.is_live(t.mesh) {
                lost.push(e);
            }
        });
        for e in lost {
            if let Some(data) = rebuild(world, e) {
                let mesh = self.add_mesh(&data);
                if let Some(t) = world.get_mut::<TerrainCollider>(e) {
                    t.mesh = mesh;
                }
            }
        }
        self.terrain_rebuild = Some(rebuild);
    }
}

/// Keeps static colliders around an entity (the player, a vehicle) once
/// [`Physics3D::stream_colliders`] is on: the colliders of fixed bodies and
/// [`TerrainCollider`]s within `radius` of it (on the ground, X and Z) are
/// in the physics world, nearest first, and stay until they're beyond
/// `keep_radius` of every focus. The gap between the two stops a collider
/// at the edge from coming and going as the focus moves back and forth.
///
/// ```
/// # use ponko::prelude::*;
/// let player = ponko::d3::ColliderFocus::new(150.0, 200.0);
/// assert_eq!(player.keep_radius, 200.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Component)]
#[repr(C)] // shared with C/C++ (`PonkoColliderFocus`)
pub struct ColliderFocus {
    /// Colliders nearer than this come in.
    pub radius: f32,
    /// Colliders beyond this from every focus go (raised to `radius` when less).
    pub keep_radius: f32,
}

impl Default for ColliderFocus {
    fn default() -> Self {
        Self::new(100.0, 150.0)
    }
}

impl ColliderFocus {
    /// Colliders within `radius` come in, and go beyond `keep_radius`.
    #[must_use]
    pub fn new(radius: f32, keep_radius: f32) -> Self {
        Self { radius, keep_radius }
    }
}

/// A rectangle on the ground: min X, min Z, max X, max Z.
pub(super) type Rect = [f32; 4];

/// How far `p` is from the nearest point of `r`, on the ground.
fn away(r: Rect, p: Vec2) -> f32 {
    let dx = (r[0] - p.x).max(p.x - r[2]).max(0.0);
    let dz = (r[1] - p.y).max(p.y - r[3]).max(0.0);
    (dx * dx + dz * dz).sqrt()
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Moving bodies keep colliders within this of their bounds (in X and Z):
/// wheels and feet reach a little beyond a body's own shape.
const HOLD_MARGIN: f32 = 1.0;

/// Reused memory for [`stream`].
#[derive(Default)]
pub(super) struct Scratch {
    /// Each focus: where it is, its radius and its keep radius.
    focus: Vec<(Vec2, f32, f32)>,
    /// The ground under each moving body, widened by [`HOLD_MARGIN`].
    holders: Vec<Rect>,
    /// Static colliders in reach and not in: distance, link index.
    wanted: Vec<(f32, u32)>,
}

impl Physics3D {
    /// Turns collider streaming on: from the next step, the colliders of
    /// fixed bodies and [`TerrainCollider`]s are only in the physics world
    /// near entities with a [`ColliderFocus`] (and a [`d3::Transform`](crate::d3::Transform)), at
    /// most `max_new` more each step, nearest first, so a big world's
    /// colliders come in over a few steps. `None` turns it off: every
    /// collider comes back at the next step. Drawing isn't affected, so
    /// the world can be drawn much further out than it's solid.
    ///
    /// A collider never goes while a moving body (dynamic, kinematic, a
    /// character) is over it, so a crate left behind doesn't fall through
    /// the ground. Bodies far from every focus find no new ground, though:
    /// give anything that travels far on its own (vehicles) a focus too.
    /// Ray casts and other queries only see colliders that are in.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).physics3d(vec3(0.0, -9.81, 0.0)).run_headless(1, |ctx| {
    ///     ctx.physics3d.stream_colliders(Some(16));
    ///     ctx.world.spawn((ponko::d3::Transform::default(), ponko::d3::Character::new(1.0, 0.3), ponko::d3::ColliderFocus::new(150.0, 200.0)));
    /// });
    /// ```
    pub fn stream_colliders(&mut self, max_new: Option<usize>) {
        self.streaming = max_new;
    }

    /// Whether collider streaming is on, and how many colliders may come in per step.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut physics = ponko::d3::Physics::default();
    /// physics.stream_colliders(Some(8));
    /// assert_eq!(physics.collider_streaming(), Some(8));
    /// ```
    #[must_use]
    pub fn collider_streaming(&self) -> Option<usize> {
        self.streaming
    }

    /// Whether `e`'s collider is in the physics world now: false for an
    /// entity that isn't a body (yet: bodies join at the next physics
    /// step), and for a static collider that [collider
    /// streaming](Physics3D::stream_colliders) left out.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let physics = ponko::d3::Physics::default();
    /// assert!(!physics.is_collider_active(Entity::from_bits(0)));
    /// ```
    #[must_use]
    pub fn is_collider_active(&self, e: Entity) -> bool {
        self.link(e)
            .and_then(|l| self.world.colliders.get(l.collider))
            .is_some_and(rp::Collider::is_enabled)
    }

    /// A collision mesh built by [`CollisionMeshData`] (on any thread),
    /// for [`Collider3D::mesh`](super::Collider3D::mesh) and
    /// [`TerrainCollider`]. Quick: the data's shape is shared, not copied.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mut physics = ponko::d3::Physics::default();
    /// let rock = ponko::d3::CollisionMeshData::convex_hull(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z]).unwrap();
    /// let mesh = physics.add_mesh(&rock);
    /// assert!(physics.is_live(mesh));
    /// ```
    pub fn add_mesh(&mut self, data: &CollisionMeshData) -> CollisionMesh {
        self.add_shape(data.shape.clone(), data.offset)
    }

    /// Where a static link's collider lies on the ground, with its body at `t`.
    pub(super) fn rect_of(&self, collider: rp::ColliderHandle, t: &Transform3D) -> Rect {
        let Some(c) = self.world.colliders.get(collider) else {
            return [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
        };
        let local = c.position_wrt_parent().copied().unwrap_or_default();
        let aabb = c.shape().compute_aabb(&(pose_to_rp(t.position, t.rotation) * local));
        [aabb.mins.x, aabb.mins.z, aabb.maxs.x, aabb.maxs.z]
    }

    pub(super) fn set_active(&mut self, i: usize, active: bool) {
        let link = self.links.at_mut(i);
        link.active = active;
        if let Some(c) = self.world.colliders.get_mut(link.collider) {
            c.set_enabled(active);
        }
    }
}

/// A copy of `shape` scaled by `scale` for a
/// [`Shape3D::ScaledMesh`](super::Shape3D::ScaledMesh): a hull or triangle
/// mesh is built again from its scaled points, so a negative component
/// mirrors it and a stretch keeps its normals right (a triangle mesh
/// mirrored along one or three axes turns its triangles around, so they
/// still face out); a heightfield takes the scale's size and is mirrored
/// by a negative X or Z as [`scaled_field`] builds it (a negative Y is
/// left out). Each component is at least 1e-6 from zero. `None` if
/// the scaled hull has no volume or the triangle mesh can't be built.
pub(super) fn scaled_copy(shape: &rp::SharedShape, scale: Vec3) -> Option<rp::SharedShape> {
    let tiny = 1e-6;
    let scale = Vec3::select(
        scale.abs().cmplt(Vec3::splat(tiny)),
        Vec3::splat(tiny).copysign(scale),
        scale,
    );
    let s = to_rp(scale);
    if let Some(mesh) = shape.as_trimesh() {
        let vertices: Vec<rp::Vector> = mesh.vertices().iter().map(|&v| v * s).collect();
        let mirrored = scale.x * scale.y * scale.z < 0.0;
        let indices: Vec<[u32; 3]> = mesh
            .indices()
            .iter()
            .map(|&[a, b, c]| if mirrored { [a, c, b] } else { [a, b, c] })
            .collect();
        rp::SharedShape::trimesh_with_flags(vertices, indices, mesh.flags()).ok()
    } else if let Some(hull) = shape.as_convex_polyhedron() {
        let points: Vec<rp::Vector> = hull.points().iter().map(|&p| p * s).collect();
        rp::SharedShape::convex_hull(&points)
    } else if let Some(field) = shape.as_heightfield() {
        Some(rp::SharedShape::new(scaled_field(field, scale)))
    } else {
        Some(shape.clone())
    }
}

/// `field` stretched by the size of `scale` and mirrored by a negative X
/// or Z, so it is solid where a tile of that scale is drawn. A heightfield
/// can't take a negative scale, so a mirrored one is built again with its
/// heights, cells (holes) and cell diagonals flipped (about 0.1 ms for
/// 257 × 257 heights, where an unmirrored copy is a plain clone). Its
/// center then sits at the unmirrored offset times the signed scale. The
/// ground always faces up: a negative Y takes its size only.
fn scaled_field(field: &HeightField, scale: Vec3) -> HeightField {
    let size = to_rp(scale.abs());
    let (flip_x, flip_z) = (scale.x < 0.0, scale.z < 0.0);
    if !flip_x && !flip_z {
        return field.clone().scaled(size);
    }
    let heights = Array2::new(
        field.heights().nrows(),
        field.heights().ncols(),
        mirrored(field.heights(), flip_x, flip_z),
    );
    let mut out = HeightField::with_flags(heights, field.scale() * size, field.flags());
    // One mirror turns each cell's diagonal the other way; two (a half
    // turn) keep it. A mirror along X also swaps a cell's two triangles.
    let turn = if flip_x == flip_z {
        HeightFieldCellStatus::empty()
    } else {
        HeightFieldCellStatus::ZIGZAG_SUBDIVISION
    };
    let cells = field.cells_statuses();
    let mut statuses = mirrored(cells, flip_x, flip_z);
    let (turn, left, right) = (
        turn.bits(),
        HeightFieldCellStatus::LEFT_TRIANGLE_REMOVED.bits(),
        HeightFieldCellStatus::RIGHT_TRIANGLE_REMOVED.bits(),
    );
    let both = left | right;
    for status in &mut statuses {
        let mut bits = status.bits() ^ turn;
        // Each removed triangle to the other side, if only one is.
        if flip_x && bits & both != both && bits & both != 0 {
            bits ^= both;
        }
        *status = HeightFieldCellStatus::from_bits_retain(bits);
    }
    *out.cells_statuses_mut() = Array2::new(cells.nrows(), cells.ncols(), statuses);
    out
}

/// The elements of a grid of rows along Z and columns along X (stored a
/// column after another), with the columns in reverse order for `flip_x`
/// and each column backwards for `flip_z`.
fn mirrored<T: Copy>(grid: &Array2<T>, flip_x: bool, flip_z: bool) -> Vec<T> {
    let mut out = Vec::with_capacity(grid.data().len());
    let mut push = |column: &[T]| {
        let start = out.len();
        out.extend_from_slice(column);
        if flip_z {
            out[start..].reverse();
        }
    };
    let columns = grid.data().chunks_exact(grid.nrows());
    if flip_x {
        columns.rev().for_each(&mut push);
    } else {
        columns.for_each(&mut push);
    }
    out
}

/// Brings static colliders near a focus in (nearest first, at most the
/// budget) and takes those far from every focus out, unless a moving body
/// is over them. With streaming off, brings back any that are out.
pub(super) fn stream(physics: &mut Physics3D, world: &mut World) {
    physics.moving();
    let Some(max_new) = physics.streaming else {
        if physics.streamed_out {
            physics.streamed_out = false;
            for k in 0..physics.statics.len() {
                let i = physics.statics[k] as usize;
                if !physics.links.at(i).active {
                    physics.set_active(i, true);
                }
            }
        }
        return;
    };
    physics.streamed_out = true;
    let mut scratch = std::mem::take(&mut physics.stream_scratch);
    scratch.focus.clear();
    world.query::<(&Transform3D, &ColliderFocus)>().for_each(|(t, f)| {
        let at = Vec2::new(t.position.x, t.position.z);
        scratch.focus.push((at, f.radius, f.keep_radius.max(f.radius)));
    });
    scratch.wanted.clear();
    let mut holders_found = false;
    for k in 0..physics.statics.len() {
        let i = physics.statics[k];
        let link = *physics.links.at(i as usize);
        if link.active {
            if scratch.focus.iter().any(|&(at, _, keep)| away(link.rect, at) <= keep) {
                continue;
            }
            if !holders_found {
                holders_found = true;
                find_holders(physics, &mut scratch.holders);
            }
            if scratch.holders.iter().any(|&h| overlaps(h, link.rect)) {
                continue;
            }
            physics.set_active(i as usize, false);
        } else {
            let nearest = scratch
                .focus
                .iter()
                .map(|&(at, radius, _)| (away(link.rect, at), radius))
                .filter(|&(d, radius)| d <= radius)
                .map(|(d, _)| d)
                .reduce(f32::min);
            if let Some(d) = nearest {
                // By position in `statics`, which is entity-index order.
                scratch.wanted.push((d, k as u32));
            }
        }
    }
    if scratch.wanted.len() > max_new {
        // Nearest first; equal distances in a fixed order.
        scratch
            .wanted
            .sort_unstable_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    }
    for &(_, k) in scratch.wanted.iter().take(max_new) {
        physics.set_active(physics.statics[k as usize] as usize, true);
    }
    physics.stream_scratch = scratch;
}

/// The ground under every moving body, widened a little.
fn find_holders(physics: &mut Physics3D, holders: &mut Vec<Rect>) {
    holders.clear();
    let n = physics.moving().len();
    for k in 0..n {
        let i = physics.moving[k] as usize;
        let link = *physics.links.at(i);
        let Some(c) = physics.world.colliders.get(link.collider) else {
            continue;
        };
        let aabb = c.compute_aabb();
        holders.push([
            aabb.mins.x - HOLD_MARGIN,
            aabb.mins.z - HOLD_MARGIN,
            aabb.maxs.x + HOLD_MARGIN,
            aabb.maxs.z + HOLD_MARGIN,
        ]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics3d::from_rp;

    /// Every triangle of `field`, its corners rounded to a millimeter and
    /// sorted, so two fields can be compared as sets of triangles.
    fn triangles(field: &HeightField, scale: Vec3) -> Vec<[[i64; 3]; 3]> {
        let mut out = Vec::new();
        for i in 0..field.nrows() {
            for j in 0..field.ncols() {
                let (a, b) = field.triangles_at(i, j);
                for t in [a, b].into_iter().flatten() {
                    let mut corners =
                        [t.a, t.b, t.c].map(|p| (from_rp(p) * scale * 1000.0).round().as_i64vec3().to_array());
                    corners.sort_unstable();
                    out.push(corners);
                }
            }
        }
        out.sort_unstable();
        out
    }

    #[test]
    fn a_mirrored_heightfield_is_the_original_with_every_triangle_mirrored() {
        // Bumpy heights (no cell is flat, so a wrong diagonal shows), a
        // hole, and a cell with only its left triangle.
        let n = 6;
        let heights: Vec<f32> = (0..n * n)
            .map(|i| {
                if i == 2 * n + 3 {
                    f32::NAN
                } else {
                    (i * 37 % 11) as f32 * 0.4
                }
            })
            .collect();
        let data = CollisionMeshData::heightfield(&heights, 10.0);
        let mut field = data.shape.as_heightfield().unwrap().clone();
        field.set_cell_status(4, 1, HeightFieldCellStatus::LEFT_TRIANGLE_REMOVED);
        let original = field.clone();
        for scale in [
            Vec3::new(-1.0, 1.0, 1.0),
            Vec3::new(1.0, 1.0, -1.0),
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(-2.0, 0.5, 3.0),
            Vec3::new(1.5, 2.0, -0.5),
            Vec3::new(-0.75, 1.0, -1.25),
        ] {
            // Both fields in their own frames, centered on the tile: the
            // mirrored tile's center is the original's times the scale.
            let mirrored = scaled_field(&field, scale);
            let want = triangles(&original, scale);
            assert_eq!(triangles(&mirrored, Vec3::ONE), want, "at scale {scale}");
            assert_eq!(want.len(), 2 * 25 - 2 * 4 - 1, "the hole and the half cell stay open");
        }
        // Negative control: mirroring the heights alone, without turning
        // the diagonals, is not the mirrored ground.
        let flip = Vec3::new(-1.0, 1.0, 1.0);
        let mut naive = scaled_field(&field, flip);
        for i in 0..n - 1 {
            for j in 0..n - 1 {
                let status = naive.cell_status(i, j) ^ HeightFieldCellStatus::ZIGZAG_SUBDIVISION;
                naive.set_cell_status(i, j, status);
            }
        }
        assert_ne!(triangles(&naive, Vec3::ONE), triangles(&original, flip));
        // Unmirrored, it is only stretched.
        let stretched = scaled_field(&field, Vec3::new(2.0, 3.0, 0.5));
        assert_eq!(stretched.heights().data(), field.heights().data());
        assert_eq!(stretched.cells_statuses().data(), field.cells_statuses().data());
    }
}
