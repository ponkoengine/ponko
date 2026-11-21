//! Persistent static geometry, partitioned into bounded spatial cells.
use super::{Mesh, collect::Instance3D};
use crate::color::Color;
use crate::gfx::Graphics;
use glam::{Mat4, Vec3};

/// A persistent batch. Handles are never recycled; freeing twice is harmless.
/// A streamed world adds and drops its instances one cell at a time
/// ([`InstanceCell`](crate::render::InstanceCell)), or makes one batch per
/// streamed region and frees it when that region unloads.
///
/// Free a batch before the meshes it draws. A batch whose mesh (or one of
/// whose level meshes) was freed draws nothing from the next frame on, and
/// never the mesh made later in the freed one's place; it draws again if
/// that mesh comes back under its id (an evicted resident mesh reloaded).
/// ```
/// use ponko::prelude::*;
/// Game::new("instances", 1, 1).run_headless(0, |ctx| {
///     let mat = ctx.gfx.create_material(ponko::render::MaterialDesc::default());
///     let mesh = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), mat);
///     let batch = ctx.gfx.create_instance_batch(mesh, &[Mat4::IDENTITY], ponko::render::InstanceBatchOptions::default());
///     ctx.gfx.free(batch);
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct InstanceBatch {
    pub(crate) id: u32,
}
/// Settings for a batch; materials come from the mesh's parts.
/// ```
/// use ponko::prelude::*;
/// let settings = ponko::render::InstanceBatchOptions { max_distance: 500.0, ..Default::default() };
/// assert!(settings.cast_shadows);
/// ```
///
/// # Defaults and cost
///
/// [`Default`] is 32-unit cells, no distance limit, shadows cast, a white
/// tint and [`BatchCulling::Auto`]. Instances are culled cell by cell, so a
/// smaller `cell_size` culls more precisely and a larger one tests fewer
/// cells; an infinite `max_distance` draws everything the camera sees, while
/// a finite one drops far instances before they cost a draw. `cast_shadows`
/// adds the batch to every shadow view that reaches it (the sun's cascades
/// and each shadow-casting light), so turning it off for small props saves
/// shadow draws.
#[derive(Clone, Copy, Debug)]
pub struct InstanceBatchOptions {
    /// Width of each spatial cell in world units. Must be finite and positive.
    pub cell_size: f32,
    /// Maximum distance from this view's camera or shadow-casting light to
    /// an instance's bounding sphere. Sun cascades use the main camera.
    /// Positive infinity disables distance culling.
    pub max_distance: f32,
    /// Whether the batch casts sun, point-light and spot-light shadows.
    pub cast_shadows: bool,
    /// Uniform tint, in sRGB like `Model::with_color`.
    pub tint: Color,
    /// Which culling path the batch takes: the renderer's choice, or yours.
    pub culling: BatchCulling,
}
/// How a batch is culled and drawn: [`InstanceBatchOptions::culling`].
/// Both paths draw the same picture; they cost differently. The renderer's
/// own choice (`Auto`) comes from a benchmark on one machine, so a game
/// that measured its own scene can overrule it per batch.
///
/// The path is chosen when the batch is made, updated, or its preference
/// changes. An automatic batch is also reselected immediately when one of
/// its materials changes between opaque/cutout and transparent blending.
/// ```
/// use ponko::render::{BatchCulling, InstanceBatchOptions};
/// // A few thousand heavy meshes: cull them one by one on the GPU.
/// let options = InstanceBatchOptions { culling: BatchCulling::PreferGpu, ..Default::default() };
/// assert_eq!(InstanceBatchOptions::default().culling, BatchCulling::Auto);
/// # let _ = options;
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BatchCulling {
    /// The renderer picks by the batch's size, and keeps batches of several
    /// distance levels and see-through batches on the CPU path (see the
    /// guide's "Which path a batch takes"). WebGL2 always takes the CPU path.
    #[default]
    Auto,
    /// Cull every instance in the compute pass and draw with indirect
    /// draws, where the device has compute culling (elsewhere, and for a
    /// batch past a device limit, the CPU path is used all the same).
    ///
    /// Visible instances keep the batch's stable cell order, including depth
    /// ties. Transparent instances are not sorted back to front; `Auto`
    /// conservatively keeps blended batches on the CPU path.
    PreferGpu,
    /// Cull cell by cell on the CPU and draw directly, also where the
    /// device has compute culling.
    PreferCpu,
}
impl Default for InstanceBatchOptions {
    fn default() -> Self {
        Self {
            cell_size: 32.0,
            max_distance: f32::INFINITY,
            cast_shadows: true,
            tint: Color::WHITE,
            culling: BatchCulling::Auto,
        }
    }
}
/// A mesh drawn in a half-open distance band, measured to the instance origin.
/// Overlapping bands draw both levels. Sun views use the main camera; local
/// shadows use the light position. Mesh shadow settings still apply.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::render::{InstanceBatchOptions, InstanceLevel, MaterialDesc, MeshData};
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let mat = ctx.gfx.create_material(MaterialDesc::default());
///     let near = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
///     let far = ctx.gfx.create_mesh(&MeshData::cube(0.5), mat);
///     // The detailed mesh up to 50 units, a cheaper one beyond.
///     let levels = [
///         InstanceLevel { mesh: near, min_distance: 0.0, max_distance: 50.0, camera: true, shadows: true },
///         InstanceLevel { mesh: far, min_distance: 50.0, max_distance: f32::INFINITY, camera: true, shadows: false },
///     ];
///     let batch = ctx.gfx.create_instance_batch_levels(&levels, &[Mat4::IDENTITY], &[], InstanceBatchOptions::default());
///     assert_eq!(ctx.gfx.instance_batch_len(batch), Some(1));
/// });
/// ```
#[derive(Clone, Copy, Debug)]
pub struct InstanceLevel {
    /// Mesh and materials for this level. Must outlive the batch: while it
    /// is freed, the whole batch draws nothing.
    pub mesh: Mesh,
    /// Inclusive, finite, nonnegative lower distance.
    pub min_distance: f32,
    /// Exclusive upper distance; positive infinity is allowed.
    pub max_distance: f32,
    /// Draw in the main camera and target cameras.
    pub camera: bool,
    /// Cast sun and local-light shadows (also requires `cast_shadows`).
    pub shadows: bool,
}
#[cfg(test)]
impl InstanceLevel {
    pub(crate) fn contains(&self, distance: f32) -> bool {
        distance >= self.min_distance && distance < self.max_distance
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Cell {
    pub min: Vec3,
    pub max: Vec3,
    pub start: u32,
    pub count: u32,
    /// The largest radius global illumination's probes test an instance of
    /// the cell with for their size cut: its bounding sphere's, or for
    /// distance levels a sphere around its origin that holds it. Its
    /// centre is in the cell's box, so a probe farther from the box than
    /// `reach / GI_SMALLEST` sees none of them a pixel wide.
    pub reach: f32,
}
pub(crate) struct Batch {
    pub levels: Vec<InstanceLevel>,
    pub legacy_distance: bool,
    pub min: Vec3,
    pub max: Vec3,
    pub options: InstanceBatchOptions,
    pub instances: Vec<Instance3D>,
    /// Each instance's bounding sphere in the world. Shared with the
    /// renderer's copy of a batch of distance levels on the CPU path, which
    /// reads it only while global illumination is on (nothing is copied).
    pub spheres: std::sync::Arc<Vec<[f32; 4]>>,
    pub cells: Vec<Cell>,
    /// Withdrawn from the renderer because a level's mesh was freed (see
    /// `Assets3D::validate_instance_batches`).
    pub suspended: bool,
    /// The level meshes' bounds this batch was built with: its cells are
    /// built with them too, also while a mesh is evicted.
    pub bounds: (Vec3, f32),
    /// Instances counted for the automatic path choice: its own, or for an
    /// instance cell, its whole batch's when it was uploaded.
    pub route_count: u32,
    /// For an instance cell's row: the batch it belongs to.
    pub parent: Option<u32>,
    /// For a batch: its live instance cells (see `instance_cells.rs`).
    pub members: Vec<super::InstanceCell>,
}
impl Batch {
    fn new(mesh: Mesh, transforms: &[Mat4], options: InstanceBatchOptions, bounds: (Vec3, f32)) -> Self {
        Self::levels(
            &[InstanceLevel {
                mesh,
                min_distance: 0.0,
                max_distance: options.max_distance,
                camera: true,
                shadows: true,
            }],
            transforms,
            &[],
            options,
            bounds,
            true,
        )
    }
    pub(crate) fn levels(
        levels: &[InstanceLevel],
        transforms: &[Mat4],
        colors: &[Color],
        options: InstanceBatchOptions,
        bounds: (Vec3, f32),
        legacy_distance: bool,
    ) -> Self {
        assert!(
            colors.is_empty() || colors.len() == transforms.len(),
            "instance colors must be empty or match transform count"
        );
        assert!(
            options.cell_size.is_finite() && options.cell_size > 0.0,
            "instance batch: cell_size must be finite and > 0; choose a positive cell size"
        );
        assert!(
            options.max_distance >= 0.0,
            "instance batch: max_distance must be >= 0; use 0 for unlimited distance"
        );
        assert!(
            transforms.len() <= 16_777_216,
            "instance batch: too many instances; split into batches of at most 16777216"
        );
        let mut sorted: Vec<_> = transforms
            .iter()
            .enumerate()
            .map(|(index, &m)| {
                assert!(
                    m.is_finite() && m.row(3) == glam::Vec4::W,
                    "instances require finite affine transforms"
                );
                let center = m.transform_point3(bounds.0);
                let radius = bounds.1 * Vec3::new(m.x_axis.length(), m.y_axis.length(), m.z_axis.length()).length();
                let key = (center / options.cell_size).floor().as_ivec3().to_array();
                (
                    key,
                    m,
                    center,
                    radius,
                    colors.get(index).copied().unwrap_or(options.tint),
                )
            })
            .collect();
        sorted.sort_by_key(|e| e.0);
        let mut batch = Self {
            levels: levels.to_vec(),
            legacy_distance,
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
            options,
            instances: Vec::with_capacity(sorted.len()),
            spheres: Default::default(),
            cells: Vec::new(),
            suspended: false,
            bounds,
            route_count: transforms.len() as u32,
            parent: None,
            members: Vec::new(),
        };
        let mut spheres = Vec::with_capacity(sorted.len());
        let mut previous = None;
        for (key, matrix, center, radius, color) in sorted {
            if previous != Some(key) || batch.cells.last().is_some_and(|c| c.count == 256) {
                batch.cells.push(Cell {
                    min: center - radius,
                    max: center + radius,
                    start: batch.instances.len() as u32,
                    count: 0,
                    reach: 0.0,
                });
                previous = Some(key);
            }
            let cell = batch.cells.last_mut().unwrap();
            cell.min = cell.min.min(center - radius);
            cell.max = cell.max.max(center + radius);
            if !legacy_distance {
                // Origin bounds keep broad distance rejection conservative even
                // for off-centre meshes and shadow proxies.
                cell.min = cell.min.min(matrix.w_axis.truncate());
                cell.max = cell.max.max(matrix.w_axis.truncate());
            }
            batch.min = batch.min.min(cell.min);
            batch.max = batch.max.max(cell.max);
            cell.count += 1;
            let reach = if legacy_distance {
                radius
            } else {
                center.distance(matrix.w_axis.truncate()) + radius
            };
            cell.reach = cell.reach.max(reach);
            let mut model = matrix.to_cols_array_2d();
            model[0][3] = super::projected::receiver_mask(None);
            batch.instances.push(Instance3D {
                model,
                color: super::linear(color),
            });
            spheres.push(center.extend(radius).to_array());
        }
        batch.spheres = std::sync::Arc::new(spheres);
        batch
    }
}
impl Graphics {
    /// Uploads transforms once; the mesh retains its normal materials.
    /// See [`InstanceBatch`] for an example. Empty batches are valid. For
    /// skinned meshes use model entities with `Skeleton`/`SkinOf` instead.
    ///
    /// # Panics
    ///
    /// For a freed mesh or a skinned one, an `options.cell_size` that is not
    /// finite and positive, a negative `options.max_distance`, more than
    /// 16 777 216 transforms, or a transform that is not finite and affine.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mat = ctx.gfx.create_material(MaterialDesc::default());
    ///     let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
    ///     // A grove: three trees, culled cell by cell.
    ///     let trees: Vec<Mat4> = (0..3).map(|i| Mat4::from_translation(vec3(i as f32 * 10.0, 0.0, 0.0))).collect();
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &trees, InstanceBatchOptions::default());
    ///     assert_eq!(ctx.gfx.instance_batch_len(batch), Some(3));
    /// });
    /// ```
    pub fn create_instance_batch(
        &mut self,
        mesh: Mesh,
        transforms: &[Mat4],
        options: InstanceBatchOptions,
    ) -> InstanceBatch {
        self.check_static_instance_mesh(mesh);
        let bounds = self
            .assets3d
            .live_bounds(mesh.id)
            .expect("instance batch: mesh is freed or foreign; create a live mesh in this game");
        let batch = Batch::new(mesh, transforms, options, bounds);
        let id = self.assets3d.instance_batches.len() as u32;
        if let Some(r) = self.renderer.as_mut() {
            r.set_instance_batch(id, Some(&batch));
        }
        self.assets3d.instance_batches.push(Some(batch));
        self.assets3d.instance_batch_revision = self.assets3d.instance_batch_revision.wrapping_add(1);
        InstanceBatch { id }
    }
    /// Creates levels sharing one transform/colour population. Bands use origin
    /// distance, independently of `options.max_distance` (legacy API only).
    /// Empty colours use `options.tint`; otherwise one sRGB colour is required
    /// per transform. Cell width and `cast_shadows` apply to every level.
    ///
    /// # Panics
    ///
    /// For no levels, a level whose mesh is freed or skinned, or a distance
    /// band that is not finite, nonnegative and ordered; for colours that are
    /// neither empty nor one per transform; and for the invalid options and
    /// transforms of [`create_instance_batch`](Self::create_instance_batch).
    ///
    /// See [`InstanceLevel`] for an example.
    pub fn create_instance_batch_levels(
        &mut self,
        levels: &[InstanceLevel],
        transforms: &[Mat4],
        colors: &[Color],
        options: InstanceBatchOptions,
    ) -> InstanceBatch {
        let bounds = self
            .instance_level_bounds(levels)
            .expect("instance level: mesh is freed or foreign; create a live mesh in this game");
        let batch = Batch::levels(levels, transforms, colors, options, bounds, false);
        let id = self.assets3d.instance_batches.len() as u32;
        if let Some(r) = self.renderer.as_mut() {
            r.set_instance_batch(id, Some(&batch));
        }
        self.assets3d.instance_batches.push(Some(batch));
        self.assets3d.instance_batch_revision = self.assets3d.instance_batch_revision.wrapping_add(1);
        InstanceBatch { id }
    }
    fn check_static_instance_mesh(&self, mesh: Mesh) {
        assert!(
            self.assets3d
                .skins
                .get(mesh.slot() as usize)
                .is_none_or(Option::is_none),
            "skinned meshes cannot use persistent instance batches: spawn Model entities with Skeleton or SkinOf so each pose is evaluated"
        );
    }
    fn instance_level_bounds(&self, levels: &[InstanceLevel]) -> Option<(Vec3, f32)> {
        assert!(!levels.is_empty(), "instance batch requires at least one level");
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for level in levels {
            assert!(
                level.min_distance.is_finite() && level.min_distance >= 0.0 && level.max_distance > level.min_distance,
                "instance level: use finite min_distance >= 0 and max_distance > min_distance"
            );
            self.check_static_instance_mesh(level.mesh);
            let (center, radius) = self.assets3d.live_bounds(level.mesh.id)?;
            lo = lo.min(center - radius);
            hi = hi.max(center + radius);
        }
        // A sphere around the boxes' centre holding every level's sphere:
        // a level's own when they are alike (the box's corners would be
        // farther by up to the square root of 3).
        let center = (lo + hi) * 0.5;
        let radius = levels
            .iter()
            .filter_map(|l| self.assets3d.live_bounds(l.mesh.id))
            .map(|(c, r)| c.distance(center) + r)
            .fold(0.0, f32::max);
        Some((center, radius))
    }
    /// Changes a live batch's culling preference and immediately reselects its
    /// path using the current materials. Returns false for a freed batch.
    /// Device capability and size limits still apply to `PreferGpu`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mat = ctx.gfx.create_material(MaterialDesc::default());
    ///     let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &[Mat4::IDENTITY], InstanceBatchOptions::default());
    ///     assert!(ctx.gfx.set_instance_batch_culling(batch, ponko::render::BatchCulling::PreferCpu));
    ///     ctx.gfx.free(batch);
    ///     assert!(!ctx.gfx.set_instance_batch_culling(batch, ponko::render::BatchCulling::PreferGpu), "freed");
    /// });
    /// ```
    pub fn set_instance_batch_culling(&mut self, batch: InstanceBatch, culling: BatchCulling) -> bool {
        let Some(Some(data)) = self.assets3d.instance_batches.get_mut(batch.id as usize) else {
            return false;
        };
        data.options.culling = culling;
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.set_instance_batch(batch.id, Some(data));
        }
        self.assets3d.instance_batch_revision = self.assets3d.instance_batch_revision.wrapping_add(1);
        self.set_instance_cells_culling(batch.id, culling);
        true
    }
    /// Replaces transforms and rebuilds cells. Returns false for a freed handle.
    /// See [`InstanceBatch`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mat = ctx.gfx.create_material(MaterialDesc::default());
    ///     let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &[Mat4::IDENTITY], InstanceBatchOptions::default());
    ///     assert!(ctx.gfx.update_instance_batch(batch, &[Mat4::IDENTITY, Mat4::from_translation(Vec3::X)]));
    ///     assert_eq!(ctx.gfx.instance_batch_len(batch), Some(2));
    /// });
    /// ```
    pub fn update_instance_batch(&mut self, batch: InstanceBatch, transforms: &[Mat4]) -> bool {
        self.update_instance_batch_colors(batch, transforms, &[])
    }
    /// Replaces transforms and sRGB colours, rebuilding cells. Empty colours
    /// restore the uniform tint, as does `update_instance_batch`.
    /// Returns false for a freed batch or mesh.
    ///
    /// # Panics
    ///
    /// For colours that are neither empty nor one per transform, or a
    /// transform that is not finite and affine.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mat = ctx.gfx.create_material(MaterialDesc::default());
    ///     let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &[Mat4::IDENTITY; 2], InstanceBatchOptions::default());
    ///     // One sRGB colour per transform.
    ///     assert!(ctx.gfx.update_instance_batch_colors(batch, &[Mat4::IDENTITY; 2], &[Color::RED, Color::BLUE]));
    ///     // Empty colours restore the batch's tint.
    ///     assert!(ctx.gfx.update_instance_batch_colors(batch, &[Mat4::IDENTITY; 2], &[]));
    /// });
    /// ```
    pub fn update_instance_batch_colors(
        &mut self,
        batch: InstanceBatch,
        transforms: &[Mat4],
        colors: &[Color],
    ) -> bool {
        let Some(Some(old)) = self.assets3d.instance_batches.get(batch.id as usize) else {
            return false;
        };
        let bounds = if old.legacy_distance {
            self.assets3d.live_bounds(old.levels[0].mesh.id)
        } else {
            self.instance_level_bounds(&old.levels)
        };
        let Some(bounds) = bounds else {
            return false;
        };
        let mut new = Batch::levels(
            &old.levels,
            transforms,
            colors,
            old.options,
            bounds,
            old.legacy_distance,
        );
        // Its instance cells stay as they are.
        new.members = old.members.clone();
        if let Some(r) = self.renderer.as_mut() {
            r.set_instance_batch(batch.id, Some(&new));
        }
        self.assets3d.instance_batches[batch.id as usize] = Some(new);
        self.assets3d.instance_batch_revision = self.assets3d.instance_batch_revision.wrapping_add(1);
        true
    }
    /// Frees instance batch `id`: the engine side of [`Graphics::free`](crate::gfx::Graphics::free).
    pub(crate) fn free_instance_batch_id(&mut self, id: u32) {
        let batch = InstanceBatch { id };
        self.free_instance_cells_of(id);
        if let Some(slot) = self.assets3d.instance_batches.get_mut(batch.id as usize) {
            *slot = None;
            self.assets3d.instance_batch_revision = self.assets3d.instance_batch_revision.wrapping_add(1);
            if let Some(r) = self.renderer.as_mut() {
                r.set_instance_batch(batch.id, None);
            }
        }
    }
    /// Withdraws batches whose level meshes were freed from the renderer,
    /// and hands back those whose meshes are live again (see
    /// [`InstanceBatch`]). Runs before each frame's 3D scene is collected.
    pub(crate) fn validate_instance_batches(&mut self) {
        let renderer = &mut self.renderer;
        self.assets3d.validate_instance_batches(|id, batch| {
            if let Some(r) = renderer.as_mut() {
                r.set_instance_batch(id, batch);
            }
        });
    }
    /// Returns the instance count, its [instance cells](crate::render::InstanceCell)'
    /// included, or None for a freed handle. See [`InstanceBatch`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let mat = ctx.gfx.create_material(MaterialDesc::default());
    ///     let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), mat);
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &[], InstanceBatchOptions::default());
    ///     assert_eq!(ctx.gfx.instance_batch_len(batch), Some(0), "empty batches are valid");
    ///     ctx.gfx.free(batch);
    ///     assert_eq!(ctx.gfx.instance_batch_len(batch), None);
    /// });
    /// ```
    pub fn instance_batch_len(&self, batch: InstanceBatch) -> Option<usize> {
        let data = self.assets3d.instance_batches.get(batch.id as usize)?.as_ref()?;
        Some(data.instances.len() + self.instance_cells_len(data))
    }
}
impl super::Assets3D {
    /// Checks every batch's level meshes against the mesh table: a batch
    /// with a freed one is withdrawn (`send(id, None)`), because the
    /// renderer finds meshes by slot and the slot may hold a newer mesh; a
    /// withdrawn batch whose meshes are all live again is handed back
    /// (`send(id, Some(batch))`). A few lookups per batch, no allocation.
    pub(crate) fn validate_instance_batches(&mut self, mut send: impl FnMut(u32, Option<&Batch>)) {
        let mut changed = false;
        for id in 0..self.instance_batches.len() {
            let Some(batch) = &self.instance_batches[id] else {
                continue;
            };
            let live = batch.levels.iter().all(|l| self.live_bounds(l.mesh.id).is_some());
            if live != batch.suspended {
                continue;
            }
            let Some(batch) = self.instance_batches[id].as_mut() else {
                continue;
            };
            batch.suspended = !live;
            send(id as u32, live.then_some(&*batch));
            changed = true;
        }
        if changed {
            self.instance_batch_revision = self.instance_batch_revision.wrapping_add(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dense_cells_split_into_bounded_clusters() {
        let b = Batch::new(
            Mesh { id: 0 },
            &vec![Mat4::IDENTITY; 1000],
            Default::default(),
            (Vec3::ZERO, 1.0),
        );
        assert_eq!(b.cells.len(), 4);
        assert!(b.cells.iter().all(|c| c.count <= 256));
        assert_eq!(b.cells.iter().map(|c| c.count).sum::<u32>(), 1000);
    }
    #[test]
    fn cells_contain_transformed_bounds_and_partition_instances() {
        let matrices = [-33.0, -1.0, 0.0, 2.0, 40.0].map(|x| Mat4::from_translation(Vec3::new(x, 0.0, 0.0)));
        let b = Batch::new(Mesh { id: 0 }, &matrices, Default::default(), (Vec3::ZERO, 1.0));
        assert_eq!(b.cells.len(), 4);
        assert_eq!(b.cells.iter().map(|c| c.count).sum::<u32>(), 5);
        for c in &b.cells {
            for s in &b.spheres[c.start as usize..(c.start + c.count) as usize] {
                let p = Vec3::from_slice(s);
                assert!((p - s[3]).cmpge(c.min).all() && (p + s[3]).cmple(c.max).all());
            }
        }
    }
}

#[cfg(test)]
mod lifetime_tests {
    use super::*;
    use crate::render::MeshData;

    /// What `validate_instance_batches` sent: (batch id, handed back).
    fn validate(ctx: &mut crate::ctx::Ctx) -> Vec<(u32, bool)> {
        let mut sent = Vec::new();
        ctx.gfx
            .assets3d
            .validate_instance_batches(|id, b| sent.push((id, b.is_some())));
        sent
    }

    #[test]
    fn a_batch_whose_mesh_was_freed_is_withdrawn_and_never_shows_the_next_mesh() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let kept = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let old = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let other = ctx
                .gfx
                .create_instance_batch(kept, &[Mat4::IDENTITY], Default::default());
            let batch = ctx
                .gfx
                .create_instance_batch(old, &[Mat4::IDENTITY], Default::default());
            assert!(validate(ctx).is_empty(), "live meshes: nothing to do");
            let revision = ctx.gfx.assets3d.instance_batch_revision;
            ctx.free(old);
            let new = ctx.gfx.create_mesh(&MeshData::sphere(1.0, 8), material);
            assert_eq!(new.slot(), old.slot(), "the freed slot holds the new mesh");
            assert_eq!(validate(ctx), [(batch.id, false)], "withdrawn, the other batch kept");
            assert_ne!(ctx.gfx.assets3d.instance_batch_revision, revision, "frames rebuild");
            assert!(validate(ctx).is_empty(), "withdrawn once");
            assert!(ctx.is_live(batch) && ctx.is_live(other), "the handles stay live");
            assert!(
                !ctx.gfx.update_instance_batch(batch, &[Mat4::IDENTITY]),
                "it can't be updated onto the new mesh"
            );
            ctx.free(batch);
            assert!(validate(ctx).is_empty(), "a freed batch is left alone");
        });
    }

    #[test]
    fn a_withdrawn_batch_comes_back_with_its_mesh_id() {
        crate::game::Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let near = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let far = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let level = |mesh, min_distance, max_distance| InstanceLevel {
                mesh,
                min_distance,
                max_distance,
                camera: true,
                shadows: true,
            };
            let batch = ctx.gfx.create_instance_batch_levels(
                &[level(near, 0.0, 10.0), level(far, 10.0, f32::INFINITY)],
                &[Mat4::IDENTITY],
                &[],
                Default::default(),
            );
            // An evicted resident mesh: freed, its id kept for its reload.
            ctx.gfx.free(far);
            assert_eq!(validate(ctx), [(batch.id, false)], "one dead level withdraws it");
            ctx.gfx.assets3d.mesh_ids.keep(far.id);
            ctx.gfx.assets3d.reuse_mesh_id = Some(far.id);
            let back = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            assert_eq!(back, far);
            assert_eq!(validate(ctx), [(batch.id, true)], "handed back");
            assert!(validate(ctx).is_empty());
        });
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;
    #[test]
    fn half_open_bands_partition_overlap_and_leave_gaps() {
        let level = |min_distance, max_distance| InstanceLevel {
            mesh: Mesh { id: 0 },
            min_distance,
            max_distance,
            camera: true,
            shadows: true,
        };
        let levels = [level(0.0, 10.0), level(10.0, 20.0), level(15.0, 30.0)];
        for (distance, expected) in [
            (0.0, vec![0]),
            (9.999, vec![0]),
            (10.0, vec![1]),
            (15.0, vec![1, 2]),
            (20.0, vec![2]),
            (30.0, vec![]),
        ] {
            assert_eq!(
                levels
                    .iter()
                    .enumerate()
                    .filter_map(|(i, l)| l.contains(distance).then_some(i))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    #[test]
    fn batch_bounds_include_origins_and_every_transformed_proxy() {
        let levels = [InstanceLevel {
            mesh: Mesh { id: 0 },
            min_distance: 0.0,
            max_distance: 100.0,
            camera: true,
            shadows: true,
        }];
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 3.0, 1.0),
            glam::Quat::IDENTITY,
            Vec3::new(-20.0, 3.0, 8.0),
        );
        let b = Batch::levels(
            &levels,
            &[transform],
            &[Color::RED],
            Default::default(),
            (Vec3::new(10.0, 0.0, 0.0), 2.0),
            false,
        );
        let origin = transform.w_axis.truncate();
        assert!(origin.cmpge(b.min).all() && origin.cmple(b.max).all());
        let sphere = b.spheres[0];
        let center = Vec3::from_slice(&sphere);
        assert!((center - sphere[3]).cmpge(b.min).all() && (center + sphere[3]).cmple(b.max).all());
        assert_eq!(b.instances[0].color, super::super::linear(Color::RED));
    }
}
