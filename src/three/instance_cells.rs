//! Instance cells: the parts of an [`InstanceBatch`] that an open world adds
//! and drops one spatial cell at a time.
//!
//! Each cell is a row of its own in the batch table (`Assets3D::instance_batches`),
//! built with its batch's levels, options and mesh bounds and marked with
//! the batch as its parent. The renderer culls and draws a row like any
//! batch, so every view (the camera, sun cascades, target cameras, point
//! and spot light shadows) and both culling paths take cells as they come
//! and go. Adding, replacing or removing a cell uploads or frees that cell
//! alone: on the GPU-driven path its records take a range of a shared pool
//! that the next cell reuses once it is freed.
//!
//! Rows that freed cells leave are given only to later cells, never to a
//! batch: an [`InstanceBatch`] handle is never reused, so a freed one can't
//! name a cell. Cell handles are slot ids (`SlotIds`): a freed cell's slot
//! goes to the next cell under a new handle.
use super::instances::{Batch, InstanceBatch};
use crate::color::Color;
use crate::gfx::Graphics;
use crate::handle::{Handle, SlotIds};
use crate::streaming::{AssetState, Job, Step};
use glam::Mat4;

/// One cell of an [`InstanceBatch`]: instances an open world adds, replaces
/// and removes on their own, without uploading the batch's other instances
/// again. Add one with [`Graphics::add_instance_cell`] (instances at hand)
/// or [`Graphics::stream_instance_cell`] (instances built by a
/// [`Job`](crate::streaming::Job), uploaded within the per-frame upload
/// budget); remove it with [`Ctx::free`](crate::ctx::Ctx::free).
///
/// A cell draws with its batch's levels, options and tint, in every view
/// the batch draws in, and is culled on its own (a cell out of a view costs
/// that view nothing). Freeing the batch frees its cells. A freed cell's
/// handle stays dead; its slot and its GPU memory go to the next cells.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::render::{InstanceBatchOptions, MaterialDesc, MeshData};
/// Game::new("cells", 1, 1).run_headless(0, |ctx| {
///     let material = ctx.gfx.create_material(MaterialDesc::default());
///     let tree = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
///     // An empty batch: the world's cells bring its instances.
///     let forest = ctx.gfx.create_instance_batch(tree, &[], InstanceBatchOptions::default());
///     let near = ctx.gfx.add_instance_cell(forest, &[Mat4::from_translation(vec3(4.0, 0.0, 0.0))], &[]);
///     let far = ctx.gfx.add_instance_cell(forest, &[Mat4::IDENTITY; 3], &[]);
///     assert_eq!(ctx.gfx.instance_batch_len(forest), Some(4));
///     ctx.free(far); // only this cell's instances go
///     assert_eq!(ctx.gfx.instance_batch_len(forest), Some(1));
///     assert!(ctx.is_live(near) && !ctx.is_live(far));
/// });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct InstanceCell {
    pub(crate) id: u32,
}

/// A cell's instances, built anywhere (a [`Job`] on a worker thread) and
/// handed to [`Graphics::stream_instance_cell`]: column-major affine
/// transforms, and optionally one sRGB colour each.
///
/// ```
/// use ponko::prelude::*;
/// let data = ponko::render::InstanceCellData::new(vec![Mat4::IDENTITY; 2]).with_colors(vec![Color::RED; 2]);
/// assert_eq!(data.len(), 2);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstanceCellData {
    transforms: Vec<Mat4>,
    colors: Vec<Color>,
}

impl InstanceCellData {
    /// Instances at `transforms`, in the batch's tint.
    #[must_use]
    pub fn new(transforms: Vec<Mat4>) -> Self {
        Self {
            transforms,
            colors: Vec::new(),
        }
    }

    /// One sRGB colour per instance (empty: the batch's tint). The counts
    /// are checked when the cell is uploaded, which panics if they differ.
    #[must_use]
    pub fn with_colors(mut self, colors: Vec<Color>) -> Self {
        self.colors = colors;
        self
    }

    /// How many instances.
    #[must_use]
    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    /// Whether it has no instances.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
}

/// One live cell.
pub(crate) struct Entry {
    /// Its batch's id.
    parent: u32,
    /// Its row in `instance_batches`, once its instances are uploaded.
    row: Option<u32>,
    /// A streamed cell's instances being built (in a mutex only so the
    /// asset tables stay `Sync`; the game thread alone takes it), then
    /// waiting for the budget.
    job: std::sync::Mutex<Option<Job<InstanceCellData>>>,
    ready: Option<InstanceCellData>,
    state: AssetState,
}

/// Every live cell, by slot (an `Assets3D` field).
#[derive(Default)]
pub(crate) struct InstanceCells {
    ids: SlotIds,
    entries: Vec<Option<Entry>>,
    /// Rows of `instance_batches` that freed cells left, for later cells.
    free_rows: Vec<u32>,
}

impl InstanceCells {
    fn slot(&self, cell: InstanceCell) -> Option<usize> {
        let slot = self.ids.slot(cell.id)?;
        self.entries.get(slot)?.as_ref().map(|_| slot)
    }

    fn entry(&self, cell: InstanceCell) -> Option<&Entry> {
        self.entries[self.slot(cell)?].as_ref()
    }

    /// How many cells are live.
    pub(crate) fn live(&self) -> usize {
        self.entries.iter().flatten().count()
    }
}

/// Bytes a cell of `instances` instances uploads: its GPU-driven records
/// (six vec4s each), the larger of the two paths' layouts.
pub(crate) fn upload_bytes(instances: usize) -> u64 {
    instances as u64 * 96
}

impl Graphics {
    /// Adds a cell of `transforms` (column-major affine) to `batch`, and
    /// uploads it now: nothing else of the batch is uploaded again. Empty
    /// `colors` use the batch's tint; otherwise one sRGB colour per
    /// transform. Returns [`Handle::NONE`] for a freed batch.
    /// See [`InstanceCell`].
    ///
    /// Cells of a batch whose culling is [`BatchCulling::Auto`](crate::render::BatchCulling::Auto)
    /// take the path the whole batch would, counting every instance it
    /// has when the cell is added.
    ///
    /// # Panics
    ///
    /// If the colours are neither empty nor one per transform, or a
    /// transform isn't finite and affine.
    pub fn add_instance_cell(&mut self, batch: InstanceBatch, transforms: &[Mat4], colors: &[Color]) -> InstanceCell {
        let Some(cell) = self.new_instance_cell(batch) else {
            return InstanceCell::NONE;
        };
        self.upload_instance_cell(cell, transforms, colors);
        cell
    }

    /// Adds a cell to `batch` whose instances `job` builds on a worker
    /// thread. The cell is uploaded once the job is done, in the frame's
    /// upload step, within the [upload budget](Self::set_upload_budget) and
    /// in the order streamed assets were asked for; until then it draws
    /// nothing and [`instance_cell_state`](Self::instance_cell_state) says
    /// where it is. With [deterministic uploads](Self::set_deterministic_uploads)
    /// the step waits for the job. Freeing the cell before it is uploaded
    /// drops the job's result. Returns [`Handle::NONE`] for a freed batch.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::render::{InstanceCellData, MaterialDesc, MeshData};
    /// use ponko::streaming::{AssetState, Job};
    /// Game::new("cells", 1, 1).run_headless(0, |ctx| {
    ///     let material = ctx.gfx.create_material(MaterialDesc::default());
    ///     let rock = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
    ///     let rocks = ctx.gfx.create_instance_batch(rock, &[], Default::default());
    ///     // A worker scatters the cell's rocks; the game keeps running.
    ///     let job = Job::spawn(|| {
    ///         InstanceCellData::new((0..100).map(|i| Mat4::from_translation(vec3(i as f32, 0.0, 0.0))).collect())
    ///     });
    ///     let cell = ctx.gfx.stream_instance_cell(rocks, job);
    ///     ctx.finish_uploads(); // a game just carries on; this waits
    ///     assert_eq!(ctx.gfx.instance_cell_state(cell), AssetState::Resident);
    ///     assert_eq!(ctx.gfx.instance_cell_len(cell), Some(100));
    /// });
    /// ```
    pub fn stream_instance_cell(&mut self, batch: InstanceBatch, job: Job<InstanceCellData>) -> InstanceCell {
        let Some(cell) = self.new_instance_cell(batch) else {
            return InstanceCell::NONE;
        };
        let cells = &mut self.assets3d.instance_cells;
        let slot = cells.slot(cell).expect("just made");
        let entry = cells.entries[slot].as_mut().expect("just made");
        *entry.job.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(job);
        entry.state = AssetState::Loading;
        self.streaming.queue_instance_cell(cell.id);
        cell
    }

    /// Replaces a cell's instances with `transforms` and `colors` (as
    /// [`add_instance_cell`](Self::add_instance_cell) takes them) and
    /// uploads them now; a streamed cell still being built drops its job.
    /// The other cells stay as they are. Returns false for a freed cell.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("cells", 1, 1).run_headless(0, |ctx| {
    ///     let material = ctx.gfx.default_material();
    ///     let mesh = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), material);
    ///     let batch = ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    ///     let cell = ctx.gfx.add_instance_cell(batch, &[Mat4::IDENTITY], &[]);
    ///     assert!(ctx.gfx.replace_instance_cell(cell, &[Mat4::IDENTITY; 5], &[]));
    ///     assert_eq!(ctx.gfx.instance_cell_len(cell), Some(5));
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// As [`add_instance_cell`](Self::add_instance_cell) does.
    pub fn replace_instance_cell(&mut self, cell: InstanceCell, transforms: &[Mat4], colors: &[Color]) -> bool {
        if self.assets3d.instance_cells.slot(cell).is_none() {
            return false;
        }
        self.upload_instance_cell(cell, transforms, colors);
        true
    }

    /// How many instances `cell` draws (0 while a streamed one is being
    /// built), or `None` for a freed cell.
    #[must_use]
    pub fn instance_cell_len(&self, cell: InstanceCell) -> Option<usize> {
        let entry = self.assets3d.instance_cells.entry(cell)?;
        Some(entry.row.map_or(0, |row| self.cell_row(row).instances.len()))
    }

    /// Where `cell` is: [`Loading`](AssetState::Loading) while its job
    /// builds it, [`Uploading`](AssetState::Uploading) while it waits for
    /// the upload budget, [`Resident`](AssetState::Resident) once it is on
    /// the GPU (a cell added with its instances is at once), and
    /// [`Unknown`](AssetState::Unknown) once freed.
    #[must_use]
    pub fn instance_cell_state(&self, cell: InstanceCell) -> AssetState {
        self.assets3d
            .instance_cells
            .entry(cell)
            .map_or(AssetState::Unknown, |e| e.state)
    }

    /// `cell`'s row in the batch table, once uploaded (GPU tests hand rows
    /// to their own renderer).
    #[cfg(test)]
    pub(crate) fn instance_cell_row(&self, cell: InstanceCell) -> Option<u32> {
        self.assets3d.instance_cells.entry(cell)?.row
    }

    /// A new, empty cell of `batch`; `None` for a freed batch.
    fn new_instance_cell(&mut self, batch: InstanceBatch) -> Option<InstanceCell> {
        let parent = self
            .assets3d
            .instance_batches
            .get_mut(batch.id as usize)?
            .as_mut()
            .filter(|b| b.parent.is_none())?;
        let cells = &mut self.assets3d.instance_cells;
        let cell = InstanceCell {
            id: cells.ids.make(cells.entries.len(), "instance cell"),
        };
        let slot = (cell.id & crate::handle::INDEX_MASK) as usize;
        if slot == cells.entries.len() {
            cells.entries.push(None);
        }
        cells.entries[slot] = Some(Entry {
            parent: batch.id,
            row: None,
            job: std::sync::Mutex::new(None),
            ready: None,
            state: AssetState::Uploading,
        });
        parent.members.push(cell);
        Some(cell)
    }

    fn cell_row(&self, row: u32) -> &Batch {
        self.assets3d.instance_batches[row as usize]
            .as_ref()
            .expect("a live cell's row")
    }

    /// Builds live `cell`'s row from its batch and hands it to the renderer.
    fn upload_instance_cell(&mut self, cell: InstanceCell, transforms: &[Mat4], colors: &[Color]) {
        let a = &mut self.assets3d;
        let slot = a.instance_cells.slot(cell).expect("a live cell");
        let entry = a.instance_cells.entries[slot].as_ref().expect("a live cell");
        let (parent_id, row) = (entry.parent, entry.row);
        let parent = a.instance_batches[parent_id as usize]
            .as_ref()
            .expect("a live cell's batch");
        // The whole batch's instances, this cell's new ones in place of its old.
        let others: usize = parent
            .members
            .iter()
            .filter(|&&c| c != cell)
            .filter_map(|&c| a.instance_cells.entry(c)?.row)
            .map(|r| a.instance_batches[r as usize].as_ref().map_or(0, |b| b.instances.len()))
            .sum();
        let mut data = Batch::levels(
            &parent.levels,
            transforms,
            colors,
            parent.options,
            parent.bounds,
            parent.legacy_distance,
        );
        data.parent = Some(parent_id);
        data.route_count = u32::try_from(parent.instances.len() + others + transforms.len()).unwrap_or(u32::MAX);
        // A mesh freed or evicted meanwhile: the renderer may have another
        // mesh in its slot, so the row waits until it is live again
        // (`validate_instance_batches` hands it back).
        data.suspended = !data.levels.iter().all(|l| a.live_bounds(l.mesh.id).is_some());
        let row = row.unwrap_or_else(|| match a.instance_cells.free_rows.pop() {
            Some(row) => row,
            None => {
                a.instance_batches.push(None);
                (a.instance_batches.len() - 1) as u32
            }
        });
        if let Some(r) = self.renderer.as_mut() {
            r.set_instance_batch(row, (!data.suspended).then_some(&data));
        }
        let a = &mut self.assets3d;
        a.instance_batches[row as usize] = Some(data);
        a.instance_batch_revision = a.instance_batch_revision.wrapping_add(1);
        let entry = a.instance_cells.entries[slot].as_mut().expect("a live cell");
        entry.row = Some(row);
        *entry.job.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        entry.ready = None;
        entry.state = AssetState::Resident;
    }

    /// Frees `cell`: the engine side of [`Graphics::free`] with a cell.
    pub(crate) fn free_instance_cell(&mut self, cell: InstanceCell) {
        let cells = &mut self.assets3d.instance_cells;
        let Some(slot) = cells.slot(cell) else { return };
        let entry = cells.entries[slot].take().expect("live");
        cells.ids.release(cell.id);
        if let Some(Some(parent)) = self.assets3d.instance_batches.get_mut(entry.parent as usize) {
            parent.members.retain(|&c| c != cell);
        }
        if let Some(row) = entry.row {
            self.free_instance_cell_row(row);
        }
    }

    fn free_instance_cell_row(&mut self, row: u32) {
        let a = &mut self.assets3d;
        a.instance_batches[row as usize] = None;
        a.instance_cells.free_rows.push(row);
        a.instance_batch_revision = a.instance_batch_revision.wrapping_add(1);
        if let Some(r) = self.renderer.as_mut() {
            r.set_instance_batch(row, None);
        }
    }

    /// Frees every cell of batch `id` (the batch is being freed).
    pub(crate) fn free_instance_cells_of(&mut self, id: u32) {
        let Some(Some(batch)) = self.assets3d.instance_batches.get_mut(id as usize) else {
            return;
        };
        if batch.parent.is_some() {
            return;
        }
        for cell in std::mem::take(&mut batch.members) {
            self.free_instance_cell(cell);
        }
    }

    /// The instances of `batch`'s cells.
    pub(crate) fn instance_cells_len(&self, batch: &Batch) -> usize {
        batch.members.iter().filter_map(|&c| self.instance_cell_len(c)).sum()
    }

    /// Gives batch `id`'s new culling preference to its cells' rows.
    pub(crate) fn set_instance_cells_culling(&mut self, id: u32, culling: crate::render::BatchCulling) {
        let a = &mut self.assets3d;
        let Some(Some(batch)) = a.instance_batches.get(id as usize) else {
            return;
        };
        for cell in batch.members.clone() {
            let Some(row) = a.instance_cells.entry(cell).and_then(|e| e.row) else {
                continue;
            };
            let Some(data) = a.instance_batches[row as usize].as_mut() else {
                continue;
            };
            data.options.culling = culling;
            if !data.suspended
                && let Some(r) = self.renderer.as_mut()
            {
                r.set_instance_batch(row, Some(data));
            }
        }
    }

    /// One upload of streamed cell `id` (the upload queue's step): wait
    /// for its job, then for room in the budget (`room` bytes, or any as
    /// the frame's `first` upload).
    pub(crate) fn instance_cell_step(&mut self, id: u32, room: u64, first: bool) -> Step {
        let cell = InstanceCell { id };
        let cells = &mut self.assets3d.instance_cells;
        let Some(slot) = cells.slot(cell) else {
            return Step::Dropped;
        };
        let entry = cells.entries[slot].as_mut().expect("live");
        if entry.ready.is_none() {
            // Replaced meanwhile: nothing left to upload.
            let jobs = entry.job.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(job) = jobs.take() else {
                return Step::Dropped;
            };
            let mut job = Some(job);
            let done = self.take_decoded(&mut job);
            let entry = self.assets3d.instance_cells.entries[slot].as_mut().expect("live");
            *entry.job.get_mut().unwrap_or_else(std::sync::PoisonError::into_inner) = job;
            match done {
                None => return Step::NotReady,
                Some(data) => {
                    entry.ready = Some(data);
                    entry.state = AssetState::Uploading;
                }
            }
        }
        let entry = self.assets3d.instance_cells.entries[slot].as_mut().expect("live");
        let bytes = upload_bytes(entry.ready.as_ref().map_or(0, InstanceCellData::len));
        if !first && bytes > room {
            return Step::NotReady;
        }
        let data = entry.ready.take().expect("built");
        self.upload_instance_cell(cell, &data.transforms, &data.colors);
        Step::Uploaded { bytes, again: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;
    use crate::render::MeshData;

    fn at(x: f32) -> Mat4 {
        Mat4::from_translation(vec3(x, 0.0, 0.0))
    }

    #[test]
    fn a_cell_changes_only_its_own_row() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[at(0.0)], Default::default());
            let a = ctx.gfx.add_instance_cell(batch, &[at(10.0), at(11.0)], &[]);
            let b = ctx.gfx.add_instance_cell(batch, &[at(50.0)], &[]);
            let rows = &ctx.gfx.assets3d.instance_batches;
            assert_eq!(rows.len(), 3, "the batch and a row per cell");
            let row_b = rows[2].as_ref().unwrap();
            let before = (row_b.instances.len(), row_b.spheres.clone());
            assert!(ctx.gfx.replace_instance_cell(a, &[at(12.0)], &[]));
            let row_b = ctx.gfx.assets3d.instance_batches[2].as_ref().unwrap();
            assert_eq!((row_b.instances.len(), row_b.spheres.clone()), before, "b is untouched");
            assert_eq!(ctx.gfx.instance_batch_len(batch), Some(3));
            assert_eq!(ctx.gfx.instance_cell_len(a), Some(1));
            assert_eq!(ctx.gfx.instance_cell_len(b), Some(1));
            assert_eq!(ctx.live_counts().instance_batches, 1, "cells aren't batches");
            assert_eq!(ctx.live_counts().instance_cells, 2);
        });
    }

    #[test]
    fn freed_cells_rows_and_slots_go_to_new_cells_under_new_handles() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[], Default::default());
            let mut live = std::collections::VecDeque::new();
            // A long walk: cells come and go, the tables stay small.
            for step in 0..1000 {
                live.push_back(ctx.gfx.add_instance_cell(batch, &[at(step as f32)], &[]));
                if live.len() > 8 {
                    let gone = live.pop_front().unwrap();
                    ctx.free(gone);
                    assert!(!ctx.is_live(gone));
                    assert!(
                        !ctx.gfx.replace_instance_cell(gone, &[at(0.0)], &[]),
                        "a freed handle stays dead"
                    );
                }
            }
            assert!(ctx.gfx.assets3d.instance_batches.len() <= 10, "rows are reused");
            assert!(ctx.gfx.assets3d.instance_cells.entries.len() <= 10, "slots are reused");
            assert_eq!(ctx.gfx.instance_batch_len(batch), Some(8));
            // A freed batch's id is never a cell's row, nor the other way round.
            let other = ctx.gfx.create_instance_batch(mesh, &[at(0.0)], Default::default());
            assert_eq!(
                other.id as usize,
                ctx.gfx.assets3d.instance_batches.len() - 1,
                "a batch always takes a new row"
            );
            ctx.free(batch);
            assert_eq!(ctx.live_counts().instance_cells, 0, "a batch's cells go with it");
            assert!(live.iter().all(|&c| !ctx.is_live(c)));
            assert_eq!(ctx.gfx.add_instance_cell(batch, &[at(0.0)], &[]), InstanceCell::NONE);
            assert_eq!(ctx.gfx.instance_batch_len(other), Some(1));
        });
    }

    #[test]
    fn cells_take_the_path_of_their_whole_batch() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[at(0.0); 3], Default::default());
            ctx.gfx.add_instance_cell(batch, &[at(1.0); 10], &[]);
            let c = ctx.gfx.add_instance_cell(batch, &[at(2.0); 5], &[]);
            let row = ctx.gfx.assets3d.instance_cells.entry(c).unwrap().row.unwrap();
            assert_eq!(ctx.gfx.cell_row(row).route_count, 18);
            assert!(
                ctx.gfx
                    .set_instance_batch_culling(batch, crate::render::BatchCulling::PreferCpu)
            );
            assert_eq!(
                ctx.gfx.cell_row(row).options.culling,
                crate::render::BatchCulling::PreferCpu,
                "a batch's culling preference is its cells'"
            );
            // An update of the batch's own instances keeps its cells.
            assert!(ctx.gfx.update_instance_batch(batch, &[at(0.0)]));
            assert_eq!(ctx.gfx.instance_batch_len(batch), Some(16));
        });
    }

    #[test]
    fn streamed_cells_upload_within_the_budget_in_order() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[], Default::default());
            ctx.gfx.set_deterministic_uploads(true);
            // Room for two cells of 100 instances a frame.
            ctx.gfx.set_upload_budget(Some(2 * upload_bytes(100)));
            let cells: Vec<InstanceCell> = (0..5)
                .map(|i| {
                    let job = Job::spawn(move || InstanceCellData::new(vec![at(i as f32); 100]));
                    ctx.gfx.stream_instance_cell(batch, job)
                })
                .collect();
            assert!(cells.iter().all(|&c| ctx.gfx.instance_cell_len(c) == Some(0)));
            assert_eq!(ctx.gfx.upload_stats().pending, 5);
            // The one freed before its upload is dropped from the queue.
            ctx.free(cells[1]);
            let mut resident = Vec::new();
            for _ in 0..3 {
                ctx.gfx.upload_step(&mut ctx.world);
                assert!(ctx.gfx.upload_stats().frame_bytes <= 2 * upload_bytes(100));
                resident.push(
                    cells
                        .iter()
                        .filter(|&&c| ctx.gfx.instance_cell_state(c) == AssetState::Resident)
                        .count(),
                );
            }
            assert_eq!(resident, [2, 4, 4], "two a frame, nearest-asked first");
            assert_eq!(ctx.gfx.instance_cell_state(cells[1]), AssetState::Unknown);
            assert_eq!(ctx.gfx.instance_batch_len(batch), Some(400));
            assert_eq!(ctx.gfx.upload_stats().pending, 0);
        });
    }

    #[test]
    fn a_replaced_streamed_cell_drops_its_job() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[], Default::default());
            let job = Job::spawn(|| InstanceCellData::new(vec![Mat4::IDENTITY; 50]));
            let cell = ctx.gfx.stream_instance_cell(batch, job);
            assert!(ctx.gfx.replace_instance_cell(cell, &[Mat4::IDENTITY; 2], &[]));
            ctx.finish_uploads();
            assert_eq!(ctx.gfx.instance_cell_len(cell), Some(2), "the job's 50 never arrive");
        });
    }

    #[test]
    fn a_cell_whose_mesh_is_gone_waits_and_comes_back_with_it() {
        Game::new("t", 1, 1).run_headless(0, |ctx| {
            let material = ctx.gfx.default_material();
            let mesh = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
            let batch = ctx.gfx.create_instance_batch(mesh, &[], Default::default());
            // An evicted resident mesh: freed, its id kept for its reload.
            ctx.gfx.free(mesh);
            ctx.gfx.assets3d.mesh_ids.keep(mesh.id);
            let cell = ctx.gfx.add_instance_cell(batch, &[Mat4::IDENTITY], &[]);
            let row = ctx.gfx.assets3d.instance_cells.entry(cell).unwrap().row.unwrap();
            assert!(
                ctx.gfx.cell_row(row).suspended,
                "built from the batch's bounds, not drawn"
            );
            ctx.gfx.assets3d.reuse_mesh_id = Some(mesh.id);
            assert_eq!(ctx.gfx.create_mesh(&MeshData::cube(1.0), material), mesh);
            let mut sent = Vec::new();
            ctx.gfx
                .assets3d
                .validate_instance_batches(|id, b| sent.push((id, b.is_some())));
            assert_eq!(sent, [(row, true)], "handed to the renderer with its mesh");
        });
    }
}
