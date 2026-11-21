//! Offscreen instance cells: a world streamed around a moving camera one
//! cell at a time draws, on both batch paths and in every kind of view
//! (the camera, sun cascades, a target camera, point and spot light
//! shadows), exactly what one batch of the same instances made from scratch
//! draws; freed cells draw nowhere, and the pool's memory is reused. The
//! helpers are shared with `instance_cells_bench`.
use super::gpu3d::{BatchPath, Route};
use super::render_test_support as support;
use super::static_batch_render_test::{
    BatchRig, RigSetup, SAME_DEVICE, assert_same, glow, ground, shadowed_lamp, target_image,
};
use crate::d3::{SpotLight as SpotLight3D, Transform as Transform3D};
use crate::prelude::*;
use crate::render::{GpuFeature, InstanceBatch, InstanceCell, Mesh, MeshData};
use crate::streaming::CellStreamer;
use std::collections::BTreeMap;

/// No difference allowed: cells draw the very pixels one batch draws.
const EXACT: support::Tolerance = support::Tolerance {
    channel: 0,
    outliers: 0.0,
    edge: 0,
};

/// Width of a streamed cell, in metres.
pub(super) const CELL: f32 = 8.0;

/// Hands `row` of the batch table to the rig's renderer, as `Graphics`
/// does with its own.
fn sync(rig: &mut BatchRig, row: u32) {
    let data = rig.ctx.gfx.assets3d.instance_batches[row as usize].as_ref();
    rig.renderer
        .set_instance_batch(&rig.gpu.device, &rig.gpu.queue, row, data);
}

/// Adds a cell of `transforms` to `batch` on both sides.
pub(super) fn add_cell(
    rig: &mut BatchRig,
    batch: InstanceBatch,
    transforms: &[Mat4],
    colors: &[Color],
) -> InstanceCell {
    let cell = rig.ctx.gfx.add_instance_cell(batch, transforms, colors);
    let row = rig.ctx.gfx.instance_cell_row(cell).expect("uploaded");
    sync(rig, row);
    cell
}

/// Frees `cell` on both sides.
pub(super) fn free_cell(rig: &mut BatchRig, cell: InstanceCell) {
    let row = rig.ctx.gfx.instance_cell_row(cell).expect("live");
    rig.ctx.free(cell);
    sync(rig, row);
}

/// A batch of `mesh` made from scratch on both sides.
pub(super) fn whole_batch(rig: &mut BatchRig, mesh: Mesh, transforms: &[Mat4], colors: &[Color]) -> InstanceBatch {
    // Made as the cells' batch is, with their colours.
    let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    assert!(rig.ctx.gfx.update_instance_batch_colors(batch, transforms, colors));
    sync(rig, batch.id);
    batch
}

/// Frees `batch` on both sides.
pub(super) fn free_batch(rig: &mut BatchRig, batch: InstanceBatch) {
    rig.free(batch);
}

/// The instances of cell `(x, z)`: `side` × `side` pillars of three
/// heights, none touching another, and their colours.
pub(super) fn cell_instances((x, z): (i32, i32), side: usize) -> (Vec<Mat4>, Vec<Color>) {
    let step = CELL / side as f32;
    let mut transforms = Vec::new();
    let mut colors = Vec::new();
    for j in 0..side {
        for i in 0..side {
            let hash = (x * 73 + z * 19 + (i * 7 + j * 3) as i32).rem_euclid(3) as f32;
            let height = 0.6 + hash * 0.5;
            let at = vec3(
                x as f32 * CELL + (i as f32 + 0.5) * step,
                height * 0.5,
                z as f32 * CELL + (j as f32 + 0.5) * step,
            );
            transforms.push(Mat4::from_translation(at) * Mat4::from_scale(vec3(0.5, height, 0.5)));
            colors.push(Color::rgb(0.3 + 0.3 * hash, 0.9 - 0.3 * hash, 0.5));
        }
    }
    (transforms, colors)
}

/// One side of the comparison: the same streamed world kept as cells, or
/// made again as one batch whenever a cell comes or goes.
struct World {
    rig: BatchRig,
    mesh: Mesh,
    /// The empty batch the cells belong to.
    batch: InstanceBatch,
    cells: BTreeMap<(i32, i32), InstanceCell>,
    /// The batch made from scratch, on the reference side.
    whole: Option<InstanceBatch>,
    target: wgpu::Texture,
}

impl World {
    /// The scene, on a rig of `path`: a lit ground, sun shadows, a
    /// shadowed point light and spot light, and a target camera looking
    /// over the field from the side.
    fn new(path: BatchPath) -> Option<Self> {
        let mut rig = BatchRig::new(RigSetup::default())?;
        if path == BatchPath::Gpu && !support::needs(&rig.gpu.device, GpuFeature::ComputeCulling) {
            return None;
        }
        rig.renderer.batching.path = path;
        ground(&mut rig);
        let mesh = rig.mesh(&MeshData::cube(1.0), glow(Color::rgb(0.2, 0.9, 0.4)));
        let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
        let lighting = &mut rig.ctx.scene3d.lighting;
        lighting.shadows = true;
        lighting.shadow_distance = 60.0;
        lighting.light_shadow_views = 12;
        shadowed_lamp(&mut rig, vec3(4.0, 5.0, -10.0), 24.0);
        rig.ctx.world.spawn((
            Transform3D::at(vec3(30.0, 9.0, -14.0)).looking_at(vec3(28.0, 0.0, -16.0)),
            SpotLight3D {
                intensity: 500.0,
                range: 30.0,
                inner_angle: 0.6,
                outer_angle: 0.9,
                shadows: true,
                ..Default::default()
            },
        ));
        rig.ctx.scene3d.camera.far = 200.0;
        let mut camera = rig.ctx.scene3d.camera;
        camera.position = vec3(10.0, 22.0, 20.0);
        camera.look_at(vec3(14.0, 0.0, -14.0));
        let target = rig.target_camera(camera);
        Some(Self {
            rig,
            mesh,
            batch,
            cells: BTreeMap::new(),
            whole: None,
            target,
        })
    }

    /// Takes the streamer's latest loads and drops: as cells, or (`whole`)
    /// by making the batch again.
    fn stream(&mut self, streamer: &CellStreamer, whole: bool) {
        for &cell in streamer.to_drop() {
            if let Some(handle) = self.cells.remove(&cell)
                && !whole
            {
                free_cell(&mut self.rig, handle);
            }
        }
        for &cell in streamer.to_load() {
            let handle = if whole {
                InstanceCell::NONE
            } else {
                let (transforms, colors) = cell_instances(cell, 3);
                add_cell(&mut self.rig, self.batch, &transforms, &colors)
            };
            self.cells.insert(cell, handle);
        }
        let changed = !streamer.to_load().is_empty() || !streamer.to_drop().is_empty();
        if whole && changed {
            if let Some(old) = self.whole.take() {
                free_batch(&mut self.rig, old);
            }
            let (mut transforms, mut colors) = (Vec::new(), Vec::new());
            for &cell in self.cells.keys() {
                let (t, c) = cell_instances(cell, 3);
                transforms.extend(t);
                colors.extend(c);
            }
            self.whole = Some(whole_batch(&mut self.rig, self.mesh, &transforms, &colors));
        }
    }

    fn look(&mut self, k: usize) {
        let camera = &mut self.rig.ctx.scene3d.camera;
        camera.position = vec3(-14.0 + k as f32 * 2.5, 9.0, 10.0);
        camera.look_at(camera.position + vec3(0.6, -0.55, -1.0));
    }
}

#[test]
fn streamed_cells_draw_what_one_batch_draws_in_every_view_on_both_paths() {
    for path in [BatchPath::Gpu, BatchPath::Cpu] {
        let (Some(mut cells), Some(mut whole)) = (World::new(path), World::new(path)) else {
            continue;
        };
        let mut streamer = CellStreamer::new(CELL, 20.0, 28.0);
        let (mut added, mut dropped) = (0, 0);
        let mut pool_bytes = Vec::new();
        for k in 0..24 {
            cells.look(k);
            whole.look(k);
            let camera = cells.rig.ctx.scene3d.camera.position;
            // The focus is ahead of the camera, on the ground.
            streamer.update(camera + vec3(6.0, 0.0, -14.0), 4);
            added += streamer.to_load().len();
            dropped += streamer.to_drop().len();
            cells.stream(&streamer, false);
            whole.stream(&streamer, true);
            let (a, b) = (cells.rig.image(), whole.rig.image());
            assert_same(&format!("{path:?} frame {k}"), &a, &b, EXACT, 0.2);
            if let Ok(dir) = std::env::var("PONKO_BATCH_IMAGES") {
                std::fs::create_dir_all(&dir).unwrap();
                let file = std::path::Path::new(&dir).join(format!("cells-{path:?}-{k}.png"));
                image::save_buffer(file, &a.rgba, a.width, a.height, image::ColorType::Rgba8).unwrap();
            }
            let (a, b) = (
                target_image(&cells.rig, &cells.target),
                target_image(&whole.rig, &whole.target),
            );
            assert_same(&format!("{path:?} target camera, frame {k}"), &a, &b, EXACT, 0.1);
            if path == BatchPath::Gpu {
                cells
                    .rig
                    .renderer
                    .check_mirrors(&cells.rig.gpu.device, &cells.rig.gpu.queue);
                pool_bytes.push((cells.cells.len(), cells.rig.renderer.instance_batch_stats().pool_bytes));
            }
        }
        assert!(
            added > 20 && dropped > 8,
            "{path:?}: {added} cells came, {dropped} went"
        );
        let rows: Vec<u32> = cells
            .cells
            .values()
            .map(|&c| cells.rig.ctx.gfx.instance_cell_row(c).unwrap())
            .collect();
        let expected = match path {
            BatchPath::Gpu => Route::Gpu,
            _ => Route::CpuSmall,
        };
        assert!(
            rows.iter()
                .all(|&row| cells.rig.renderer.batch_route(row) == Some(expected)),
            "{path:?}"
        );
        let stats = cells.rig.renderer.instance_batch_stats();
        assert_eq!(
            stats.batches as usize,
            cells.cells.len(),
            "only the live cells are drawn"
        );
        if path == BatchPath::Gpu {
            // Cells keep coming and going but the pool stops growing: freed
            // ranges are handed to the cells after them.
            assert!(
                stats.cull_jobs as usize > cells.cells.len(),
                "shadow views culled cells too"
            );
            // From the frame with the most live cells on (where cells drawn
            // together pad each cell's block to a record boundary, it is
            // reached later than without).
            let peak = (0..pool_bytes.len()).max_by_key(|&k| (pool_bytes[k].0, k)).unwrap();
            let (early, late) = (pool_bytes[peak].1, pool_bytes.last().unwrap().1);
            assert!(late <= early, "the pool grew from {early} to {late} bytes");
        }
        // Negative control: the cell under the focus, gone on one side
        // only, shows.
        let focus = cells.rig.ctx.scene3d.camera.position + vec3(6.0, 0.0, -14.0);
        let under = ((focus.x / CELL).floor() as i32, (focus.z / CELL).floor() as i32);
        let handle = cells.cells.remove(&under).expect("the focus's cell is loaded");
        free_cell(&mut cells.rig, handle);
        let (a, b) = (cells.rig.image(), whole.rig.image());
        let d = support::diff(&a, &b, SAME_DEVICE.channel);
        assert!(!d.within(SAME_DEVICE), "{path:?}: a missing cell must show: {d}");
    }
}

#[test]
fn a_freed_cell_draws_in_no_view_from_the_next_frame_on() {
    let Some(mut world) = World::new(BatchPath::Gpu) else {
        return;
    };
    world.look(6);
    // Only the cells around the camera: the empty frame has no green.
    let green = |image: &support::Image| image.fraction(0..image.width, 0..image.height, |p| p[1] > 120 && p[0] < 110);
    let empty = world.rig.image();
    assert!(green(&empty) < 0.001, "no cells yet");
    let mut handles = Vec::new();
    for z in -3..1 {
        for x in -1..3 {
            let (transforms, colors) = cell_instances((x, z), 3);
            handles.push(add_cell(&mut world.rig, world.batch, &transforms, &colors));
        }
    }
    let full = world.rig.image();
    assert!(green(&full) > 0.02, "the cells show: {}", green(&full));
    for handle in handles {
        free_cell(&mut world.rig, handle);
    }
    // The very next frame, in every view: the picture is the empty one,
    // shadows included, and nothing is culled for the freed cells.
    let gone = world.rig.image();
    assert_same("all cells freed", &gone, &empty, SAME_DEVICE, 0.2);
    assert_eq!(world.rig.renderer.instance_batch_stats().cull_jobs, 0);
}

#[test]
fn cells_of_a_heavy_batch_take_the_gpu_driven_path_together() {
    let Some(mut rig) = BatchRig::new(RigSetup::default()) else {
        return;
    };
    if !support::needs(&rig.gpu.device, GpuFeature::ComputeCulling) {
        return;
    }
    // The renderer's own choice: heavy enough together, not alone.
    let sphere = MeshData::sphere(0.4, 48);
    let triangles = sphere.indices.len() as u64 / 3;
    let mesh = rig.mesh(&sphere, glow(Color::WHITE));
    let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    let per_cell = 256usize;
    let needed = (crate::render::capabilities::GPU_BATCH_TRIANGLES / triangles) as usize;
    assert!(per_cell < needed, "one cell alone stays under the crossover");
    let mut routes = Vec::new();
    for i in 0..needed.div_ceil(per_cell) {
        let transforms: Vec<Mat4> = (0..per_cell)
            .map(|j| Mat4::from_translation(vec3(j as f32, 0.0, -(i as f32))))
            .collect();
        let cell = add_cell(&mut rig, batch, &transforms, &[]);
        routes.push(rig.renderer.batch_route(rig.ctx.gfx.instance_cell_row(cell).unwrap()));
    }
    assert_eq!(routes[0], Some(Route::CpuSmall));
    assert_eq!(routes.last().copied().flatten(), Some(Route::Gpu), "{routes:?}");
}

/// Cells streamed around a moving camera, under the sun's cascades (far
/// ones held between frames, `Lighting::shadow_refresh`) and the lights'
/// shadow views: the renderer keeping unchanged views draws, frame by
/// frame, exactly what drawing every view again draws, on both batch
/// paths. The reference collects the same frames, so it holds the same
/// cascade boxes, and keeps no view. A cell coming or going draws again
/// only the views it reaches. (A reference whose far cascades are fitted
/// anew every frame differs from a held box by a texel at shadow edges:
/// another box, not a stale map. That is what an earlier version of
/// `streamed_cells_draw_what_one_batch_draws_in_every_view_on_both_paths`,
/// which made its whole batch again every frame, saw.)
#[test]
fn streamed_cells_redraw_only_the_shadow_views_they_reach() {
    for path in [BatchPath::Gpu, BatchPath::Cpu] {
        let (Some(mut cached), Some(mut reference)) = (World::new(path), World::new(path)) else {
            continue;
        };
        for w in [&mut cached, &mut reference] {
            w.rig.ctx.scene3d.lighting.shadow_refresh = 4;
        }
        let mut streamer = CellStreamer::new(CELL, 20.0, 28.0);
        let (mut streamed, mut kept_while_streaming, mut held) = (0, 0, 0);
        for k in 0..24 {
            cached.look(k);
            reference.look(k);
            let camera = cached.rig.ctx.scene3d.camera.position;
            streamer.update(camera + vec3(6.0, 0.0, -14.0), 4);
            let changed = !streamer.to_load().is_empty() || !streamer.to_drop().is_empty();
            cached.stream(&streamer, false);
            reference.stream(&streamer, false);
            let a = cached.rig.image();
            reference.rig.renderer.forget_shadows();
            let b = reference.rig.image();
            assert_eq!(
                cached.rig.frame3d().sun_view_proj,
                reference.rig.frame3d().sun_view_proj,
                "{path:?} frame {k}: the same boxes"
            );
            let d = support::diff(&a, &b, 0);
            assert!(
                a.rgba == b.rgba,
                "{path:?} frame {k}: kept shadow views differ from drawing every view: {d}"
            );
            let kept = cached.rig.renderer.shadow_views_kept();
            held += usize::from(cached.rig.frame3d().redrawn & 0b1100 != 0b1100);
            if changed {
                streamed += 1;
                kept_while_streaming += kept.count_ones() as usize;
            }
            eprintln!("{path:?} frame {k}: cells changed {changed}, views kept {kept:b}");
        }
        assert!(streamed > 10, "{path:?}: cells came and went on {streamed} frames");
        assert!(held > 10, "{path:?}: far boxes were held on {held} frames");
        // Every view used to be drawn again whenever a cell came or went.
        assert!(
            kept_while_streaming >= streamed,
            "{path:?}: views a streamed cell doesn't reach are kept: {kept_while_streaming} on {streamed} frames"
        );
    }
}

/// A rig of `path` with `cells` cells of 3 x 3 pillars, `columns` wide, all
/// in view from above under a lit sky with sun shadows; each cell on its
/// own (`separate`, as before cells were drawn together) or not.
fn field(path: BatchPath, separate: bool, cells: usize, columns: i32) -> Option<(BatchRig, Vec<InstanceCell>)> {
    field_of(path, (separate, false), cells, columns)
}

/// [`field`], with the GPU-driven path's cells in their own pools but each
/// drawn on its own (`ungrouped`), as on a device without
/// `Features::INDIRECT_FIRST_INSTANCE`.
fn field_of(
    path: BatchPath,
    (separate, ungrouped): (bool, bool),
    cells: usize,
    columns: i32,
) -> Option<(BatchRig, Vec<InstanceCell>)> {
    let mut rig = BatchRig::new(RigSetup::default())?;
    if path == BatchPath::Gpu && !support::needs(&rig.gpu.device, GpuFeature::ComputeCulling) {
        return None;
    }
    rig.renderer.batching.path = path;
    rig.renderer.batching.separate_cells = separate;
    rig.renderer.batching.ungrouped_cells = ungrouped;
    ground(&mut rig);
    let mesh = rig.mesh(&MeshData::cube(1.0), glow(Color::rgb(0.2, 0.9, 0.4)));
    let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.shadow_distance = 80.0;
    let handles = (0..cells as i32)
        .map(|c| {
            let (transforms, colors) = cell_instances((c % columns, -(c / columns)), 3);
            add_cell(&mut rig, batch, &transforms, &colors)
        })
        .collect();
    let camera = &mut rig.ctx.scene3d.camera;
    camera.far = 200.0;
    camera.position = vec3(20.0, 44.0, 8.0);
    camera.look_at(vec3(20.0, 0.0, -12.0));
    Some((rig, handles))
}

/// What the batch passes of the frame last drawn set and drew.
fn drawn(rig: &mut BatchRig) -> super::gpu3d::StateChanges {
    rig.frame().changes
}

#[test]
fn cells_of_a_batch_share_a_bind_group_and_draw_once_per_material_and_view() {
    let (Some((mut apart, _)), Some((mut together, _))) =
        (field(BatchPath::Gpu, true, 24, 6), field(BatchPath::Gpu, false, 24, 6))
    else {
        return;
    };
    let Some((mut few, _)) = field(BatchPath::Gpu, false, 6, 6) else {
        return;
    };
    // Drawn together or each on its own, the picture is the same, with no
    // difference allowed.
    let (a, b) = (apart.image(), together.image());
    let d = support::diff(&a, &b, 0);
    assert!(
        a.rgba == b.rgba,
        "cells drawn together differ from each on its own: {d}"
    );
    assert!(
        a.fraction(0..a.width, 0..a.height, |p| p[1] > 120 && p[0] < 110) > 0.02,
        "the cells show"
    );
    few.image();
    let (apart_n, together_n, few_n) = (drawn(&mut apart), drawn(&mut together), drawn(&mut few));
    eprintln!(
        "draws: apart {} together {} (6 cells: {}); bind groups: apart {} together {}",
        apart_n.draws, together_n.draws, few_n.draws, apart_n.bind_groups, together_n.bind_groups
    );
    if !together
        .gpu
        .device
        .features()
        .contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
    {
        // The fallback: each cell on its own, as before.
        assert_eq!(together.renderer.cell_groups(), (0, 0));
        assert_eq!(together_n.draws, apart_n.draws);
        return;
    }
    let (groups, members) = together.renderer.cell_groups();
    assert!(groups >= 2, "the camera's and the cascades' groups: {groups}");
    assert!(members >= 24, "every cell's camera view is in a group: {members}");
    assert_eq!(
        apart.renderer.cell_groups(),
        (0, 0),
        "the control draws each cell on its own"
    );
    // One draw per material and view, however many cells.
    assert_eq!(together_n.draws, few_n.draws, "24 cells draw as often as 6");
    assert_eq!(together_n.bind_groups, few_n.bind_groups);
    assert!(
        apart_n.draws >= together_n.draws + 23,
        "each on its own: a draw per cell ({} against {})",
        apart_n.draws,
        together_n.draws
    );
    // A freed cell leaves its group drawing nothing for it, and the
    // group's last cell takes the group with it.
    let (Some((mut cells, handles)), Some((mut reference, _))) =
        (field(BatchPath::Gpu, false, 12, 6), field(BatchPath::Gpu, false, 6, 6))
    else {
        return;
    };
    cells.image();
    for &handle in &handles[6..] {
        free_cell(&mut cells, handle);
    }
    let (a, b) = (cells.image(), reference.image());
    assert!(
        a.rgba == b.rgba,
        "freed cells draw nothing: {}",
        support::diff(&a, &b, 0)
    );
    for &handle in &handles[..6] {
        free_cell(&mut cells, handle);
    }
    assert_eq!(cells.renderer.cell_groups(), (0, 0), "emptied groups are gone");
}

#[test]
fn cpu_cells_take_ranges_of_one_vertex_buffer_and_reuse_freed_ones() {
    let (Some((mut apart, _)), Some((mut together, handles))) =
        (field(BatchPath::Cpu, true, 24, 6), field(BatchPath::Cpu, false, 24, 6))
    else {
        return;
    };
    let (a, b) = (apart.image(), together.image());
    assert!(
        a.rgba == b.rgba,
        "a shared buffer draws the same: {}",
        support::diff(&a, &b, 0)
    );
    let rows: Vec<u32> = handles
        .iter()
        .map(|&c| together.ctx.gfx.instance_cell_row(c).unwrap())
        .collect();
    assert!(rows.iter().all(|&row| together.renderer.batch_shares_vertices(row)));
    assert!(
        apart.renderer.cell_vertices_use().is_none(),
        "the control's cells own buffers"
    );
    let (apart_n, together_n) = (drawn(&mut apart), drawn(&mut together));
    eprintln!(
        "vertex buffers set: apart {} together {}; draws apart {} together {}",
        apart_n.vertex_buffers, together_n.vertex_buffers, apart_n.draws, together_n.draws
    );
    assert!(
        together_n.vertex_buffers + 23 <= apart_n.vertex_buffers,
        "consecutive cells bind the shared buffer once"
    );
    // A long walk: cells come and go, the buffer stops growing.
    let batch = InstanceBatch {
        id: together.ctx.gfx.assets3d.instance_batches[rows[0] as usize]
            .as_ref()
            .unwrap()
            .parent
            .unwrap(),
    };
    let mut live: std::collections::VecDeque<InstanceCell> = handles.into();
    let mut capacity = Vec::new();
    for step in 0..200 {
        let (transforms, colors) = cell_instances((step % 9, step % 5), 3);
        live.push_back(add_cell(&mut together, batch, &transforms, &colors));
        free_cell(&mut together, live.pop_front().unwrap());
        let (bytes, used, cells) = together.renderer.cell_vertices_use().unwrap();
        assert_eq!(cells, 24);
        assert_eq!(used, 24 * 9 * 80, "only the live cells' ranges are handed out");
        capacity.push(bytes);
    }
    assert!(
        capacity.iter().all(|&c| c == capacity[0]),
        "freed ranges are reused: {capacity:?}"
    );
    for handle in live {
        free_cell(&mut together, handle);
    }
    assert_eq!(together.renderer.cell_vertices_use().map(|u| u.1), Some(0));
}

#[test]
fn cpu_cells_next_to_each_other_in_the_shared_buffer_draw_as_one() {
    let (Some((mut apart, apart_handles)), Some((mut together, handles))) =
        (field(BatchPath::Cpu, true, 24, 6), field(BatchPath::Cpu, false, 24, 6))
    else {
        return;
    };
    let (a, b) = (apart.image(), together.image());
    assert!(
        a.rgba == b.rgba,
        "joined draws draw the same: {}",
        support::diff(&a, &b, 0)
    );
    // Every cell is in the camera's view and their ranges follow one
    // another: one draw for all 24.
    let in_view = together
        .renderer
        .static_batches
        .iter()
        .flatten()
        .filter(|b| b.visible.first().is_some_and(|l| !l.is_empty()))
        .count();
    assert_eq!(in_view, 24, "every cell is culled into the camera's view");
    assert_eq!(together.renderer.joined_cell_draws(0), 1);
    let (apart_n, together_n) = (drawn(&mut apart), drawn(&mut together));
    eprintln!("draws: apart {} together {}", apart_n.draws, together_n.draws);
    // Joining keeps its lists between frames: a steady frame allocates
    // nothing preparing or recording the cells' draws.
    assert_eq!(together.batch_preparation_allocations(), 0);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .use_current_thread()
        .build()
        .unwrap();
    let [_, ran, ours] = together.recording_allocations(&pool);
    assert!(ran > 0, "the batch drawing code ran");
    assert_eq!(ours, 0, "the batch drawing code allocated recording the joined draws");
    assert!(
        together_n.draws + 2 * 23 <= apart_n.draws,
        "the camera's two passes draw the 24 cells once each ({} against {})",
        together_n.draws,
        apart_n.draws
    );
    // A freed cell leaves a gap in the buffer: the run splits around it,
    // and a cell taking its range joins it again.
    free_cell(&mut apart, apart_handles[9]);
    free_cell(&mut together, handles[9]);
    let (a, b) = (apart.image(), together.image());
    assert!(a.rgba == b.rgba, "a gap draws the same: {}", support::diff(&a, &b, 0));
    assert_eq!(together.renderer.joined_cell_draws(0), 2, "split around the gap");
    let batch = InstanceBatch {
        id: together.ctx.gfx.assets3d.instance_batches
            [together.ctx.gfx.instance_cell_row(handles[0]).unwrap() as usize]
            .as_ref()
            .unwrap()
            .parent
            .unwrap(),
    };
    let apart_batch = InstanceBatch {
        id: apart.ctx.gfx.assets3d.instance_batches
            [apart.ctx.gfx.instance_cell_row(apart_handles[0]).unwrap() as usize]
            .as_ref()
            .unwrap()
            .parent
            .unwrap(),
    };
    let (transforms, colors) = cell_instances((3, -1), 3);
    add_cell(&mut together, batch, &transforms, &colors);
    add_cell(&mut apart, apart_batch, &transforms, &colors);
    let (a, b) = (apart.image(), together.image());
    assert!(
        a.rgba == b.rgba,
        "the refilled gap draws the same: {}",
        support::diff(&a, &b, 0)
    );
    assert_eq!(together.renderer.joined_cell_draws(0), 1, "the new cell took the gap");
    // Negative control: a cell missing on one side only shows.
    free_cell(&mut together, handles[0]);
    let (a, b) = (apart.image(), together.image());
    assert!(a.rgba != b.rgba, "a missing cell must show");
}

#[test]
fn gpu_cells_take_pools_of_their_own_whose_blocks_start_on_a_record() {
    let (Some((mut apart, apart_handles)), Some((mut together, handles)), Some((mut alone, alone_handles))) = (
        field(BatchPath::Gpu, true, 24, 6),
        field(BatchPath::Gpu, false, 24, 6),
        field_of(BatchPath::Gpu, (false, true), 24, 6),
    ) else {
        return;
    };
    let align = together.renderer.storage_alignment().expect("GPU-driven");
    let rows = |rig: &BatchRig, handles: &[InstanceCell]| -> Vec<u32> {
        handles
            .iter()
            .map(|&c| rig.ctx.gfx.instance_cell_row(c).unwrap())
            .collect()
    };
    // A cell of 9 instances: a header, 9 records and its one cell box.
    let bytes = 16 + 9 * 96 + 32;
    for (rig, handles, own) in [
        (&together, &handles, true),
        (&alone, &alone_handles, true),
        (&apart, &apart_handles, false),
    ] {
        for row in rows(rig, handles) {
            let (start, cells) = rig.renderer.batch_record_start(row).expect("GPU-driven");
            assert_eq!(cells, own, "cells in pools of their own: {own}");
            let step = if own { 96 } else { align };
            assert_eq!(start % step, 0, "a block starts on a multiple of {step}");
        }
        let padded = if own {
            u64::div_ceil(bytes, 96) * 96
        } else {
            u64::div_ceil(bytes, align) * align
        };
        let (used, _) = rig.renderer.pool_records();
        assert_eq!(used, 24 * padded, "each block is padded to its pool's alignment only");
    }
    eprintln!("storage binding alignment {align}");
    // Drawn together, each on its own from a cell pool, or as before: the
    // same pixels.
    let (a, b, c) = (apart.image(), together.image(), alone.image());
    assert!(a.rgba == b.rgba, "together: {}", support::diff(&a, &b, 0));
    assert!(
        a.rgba == c.rgba,
        "on their own from a cell pool: {}",
        support::diff(&a, &c, 0)
    );
    for rig in [&together, &alone] {
        rig.renderer.check_batch_order(&rig.gpu.device, &rig.gpu.queue);
    }
    // Freed cells' ranges go to the next ones; still the same pixels.
    for (rig, handles) in [
        (&mut apart, &apart_handles),
        (&mut together, &handles),
        (&mut alone, &alone_handles),
    ] {
        for &h in &handles[4..10] {
            free_cell(rig, h);
        }
    }
    let (a, b, c) = (apart.image(), together.image(), alone.image());
    assert!(a.rgba == b.rgba, "together, after frees: {}", support::diff(&a, &b, 0));
    assert!(a.rgba == c.rgba, "alone, after frees: {}", support::diff(&a, &c, 0));
    // Negative control: a cell missing on one side only shows.
    free_cell(&mut together, handles[0]);
    let (a, b) = (apart.image(), together.image());
    assert!(a.rgba != b.rgba, "a missing cell must show");
}
