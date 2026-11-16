//! Instance cells streamed around a moving focus: a `CellStreamer` says
//! which cells to load and drop, a `Job` per cell builds its instances on a
//! worker thread, and the upload step puts them on the GPU within the
//! per-frame budget; dropped cells free only their own instances. With a
//! mesh residency budget, the meshes live cells draw stay, and may go once
//! the cells do. Headless: byte counts are the engine's estimates.
#![cfg(feature = "3d")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use ponko::prelude::*;
use ponko::render::{InstanceBatch, InstanceCell, InstanceCellData, MeshData};
use ponko::streaming::{AssetState, CellStreamer, Job, ResidencyKind};

const CELL: f32 = 32.0;
const PER_CELL: usize = 64;

/// A cell's trees, scattered from its coordinates (what a game's job does).
fn trees((x, z): (i32, i32)) -> InstanceCellData {
    let mut seed = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32 | 1;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let transforms = (0..PER_CELL)
        .map(|_| {
            let at = vec3((x as f32 + next()) * CELL, 0.0, (z as f32 + next()) * CELL);
            Mat4::from_scale_rotation_translation(Vec3::splat(0.5 + next()), Quat::from_rotation_y(next() * 6.0), at)
        })
        .collect();
    InstanceCellData::new(transforms)
}

// ANCHOR: instance_cells
/// Trees for the cells around the camera: each cell's built on a worker
/// thread, uploaded within the frame's upload budget, and freed on its own
/// once it is far behind.
fn stream_trees(
    ctx: &mut Ctx,
    forest: InstanceBatch,
    streamer: &mut CellStreamer,
    cells: &mut HashMap<(i32, i32), InstanceCell>,
) {
    streamer.update(ctx.scene3d.camera.position, 4);
    for cell in streamer.to_drop() {
        if let Some(trees) = cells.remove(cell) {
            ctx.free(trees); // only this cell's instances go
        }
    }
    for &cell in streamer.to_load() {
        let job = Job::spawn(move || trees(cell));
        cells.insert(cell, ctx.gfx.stream_instance_cell(forest, job));
    }
}
// ANCHOR_END: instance_cells

#[test]
fn the_guides_trees_stream_in_and_out_as_the_camera_flies() {
    let mut streamer = CellStreamer::new(CELL, 100.0, 140.0);
    let mut cells = HashMap::new();
    let mut forest = None;
    let most = Rc::new(RefCell::new(0));
    let seen = most.clone();
    Game::new("guide", 1, 1)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let forest = *forest.get_or_insert_with(|| {
                let material = ctx.gfx.default_material();
                let tree = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
                ctx.gfx.create_instance_batch(tree, &[], Default::default())
            });
            ctx.scene3d.camera.position.x += 8.0;
            stream_trees(ctx, forest, &mut streamer, &mut cells);
            let live = ctx.live_counts().instance_cells;
            assert_eq!(live, cells.len());
            let mut most = seen.borrow_mut();
            *most = (*most).max(live);
        })
        .run_headless(120, |_| {});
    assert!(*most.borrow() > 20, "{} cells at most", most.borrow());
}

#[test]
fn instance_cells_stream_around_a_moving_focus_within_the_upload_budget() {
    // Two cells' instances a frame at most.
    let budget = 2 * PER_CELL as u64 * 96;
    let checked = Rc::new(RefCell::new(0u32));
    let frames = checked.clone();
    let mut streamer = CellStreamer::new(CELL, 100.0, 140.0);
    let mut cells: HashMap<(i32, i32), InstanceCell> = HashMap::new();
    let mut forest: Option<InstanceBatch> = None;
    let mut dropped: Vec<InstanceCell> = Vec::new();
    let mut most_pending = 0;
    Game::new("cells", 1, 1)
        .headless_frames(true)
        .system(move |ctx: &mut Ctx| {
            let batch = *forest.get_or_insert_with(|| {
                ctx.gfx.set_upload_budget(Some(budget));
                ctx.gfx.set_deterministic_uploads(true);
                let material = ctx.gfx.default_material();
                let tree = ctx.gfx.create_mesh(&MeshData::cube(1.0), material);
                ctx.gfx.create_instance_batch(tree, &[], Default::default())
            });
            // The last frame's upload step kept to the budget.
            let uploads = ctx.gfx.upload_stats();
            assert!(uploads.frame_bytes <= budget, "{uploads:?}");
            assert_eq!(uploads.oversized, 0);
            most_pending = most_pending.max(uploads.pending);
            // The focus flies on along X, and back a little along Z.
            let t = ctx.time.frame() as f32;
            let focus = vec3(t * 6.0, 0.0, -t * 1.5);
            streamer.update(focus, 4);
            for cell in streamer.to_drop() {
                let handle = cells.remove(cell).expect("a loaded cell");
                ctx.free(handle);
                dropped.push(handle);
            }
            for &cell in streamer.to_load() {
                let job = Job::spawn(move || trees(cell));
                cells.insert(cell, ctx.gfx.stream_instance_cell(batch, job));
            }
            // Every live cell is the streamer's; every dropped one is gone.
            assert_eq!(cells.len(), streamer.loaded_count());
            assert!(dropped.iter().all(|&c| !ctx.is_live(c)));
            assert_eq!(ctx.live_counts().instance_cells, cells.len());
            let resident: Vec<_> = cells
                .values()
                .filter(|&&c| ctx.gfx.instance_cell_state(c) == AssetState::Resident)
                .collect();
            let drawn = ctx.gfx.instance_batch_len(batch).unwrap();
            assert_eq!(drawn, resident.len() * PER_CELL, "only resident cells draw");
            *frames.borrow_mut() += 1;
            if ctx.time.frame() == 299 {
                assert!(dropped.len() > 30, "{} cells dropped", dropped.len());
                assert!(most_pending > 2, "uploads waited for the budget");
                ctx.finish_uploads();
                assert_eq!(ctx.gfx.instance_batch_len(batch), Some(cells.len() * PER_CELL));
                assert_eq!(ctx.gfx.upload_stats().pending, 0);
            }
        })
        .run_headless(300, |_| {});
    assert_eq!(*checked.borrow(), 300);
}

/// `n` copies of the test house, each its own file (and so its own mesh).
fn houses(test: &str, n: usize) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("ponko-cells-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let house = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/models/house.glb");
    (0..n)
        .map(|i| {
            let path = dir.join(format!("house{i}.glb"));
            std::fs::copy(&house, &path).unwrap();
            path.to_str().unwrap().to_string()
        })
        .collect()
}

type Step = Box<dyn FnMut(&mut Ctx)>;

/// Runs one step per simulation tick, in order.
fn run(steps: Vec<Step>) {
    let steps = Rc::new(RefCell::new(steps));
    let ticks = steps.borrow().len() as u64;
    let mut next = 0;
    Game::new("cells", 1, 1)
        .sim_system(move |ctx| {
            (steps.borrow_mut()[next])(ctx);
            next += 1;
        })
        .run_headless(ticks, |_| {});
}

#[test]
fn a_mesh_live_cells_draw_stays_under_a_residency_budget_and_goes_with_them() {
    let p = Rc::new(houses("budget", 2));
    let shared: Rc<RefCell<Option<(InstanceBatch, InstanceCell)>>> = Rc::default();
    let (p0, p1, p2, p3, p4, p5) = (p.clone(), p.clone(), p.clone(), p.clone(), p.clone(), p.clone());
    let (s0, s1, s2, s3) = (shared.clone(), shared.clone(), shared.clone(), shared.clone());
    let evictions = |ctx: &Ctx| ctx.gfx.residency(ResidencyKind::Mesh).evictions;
    run(vec![
        Box::new(move |ctx| {
            // Room for one house.
            let house = ctx.gfx.resident_mesh(&p0[0]);
            let bytes = ctx.gfx.residency(ResidencyKind::Mesh).resident_bytes;
            ctx.gfx.set_residency_budget(ResidencyKind::Mesh, Some(bytes));
            let batch = ctx.gfx.create_instance_batch(house, &[], Default::default());
            let cell = ctx.gfx.add_instance_cell(batch, &[Mat4::IDENTITY; 4], &[]);
            *s0.borrow_mut() = Some((batch, cell));
        }),
        Box::new(|_| {}),
        Box::new(move |ctx| {
            ctx.gfx.resident_mesh(&p1[1]); // over budget from the next frame
        }),
        Box::new(|_| {}),
        Box::new(move |ctx| {
            // The first house is the least recently asked for, but a live
            // cell draws it: the second goes instead.
            assert!(ctx.gfx.is_resident(ResidencyKind::Mesh, &p2[0]));
            assert!(!ctx.gfx.is_resident(ResidencyKind::Mesh, &p2[1]));
            assert_eq!(evictions(ctx), 1);
            let (_, cell) = s1.borrow().unwrap();
            ctx.free(cell); // the cell goes: so may its mesh
            ctx.gfx.resident_mesh(&p2[1]);
        }),
        Box::new(|_| {}),
        Box::new(move |ctx| {
            assert!(
                !ctx.gfx.is_resident(ResidencyKind::Mesh, &p3[0]),
                "evicted once no cell drew it"
            );
            assert!(ctx.gfx.is_resident(ResidencyKind::Mesh, &p3[1]));
            assert_eq!(evictions(ctx), 2);
            // A cell added meanwhile is built from the batch's bounds and
            // waits for the mesh.
            let (batch, _) = s2.borrow().unwrap();
            let cell = ctx.gfx.add_instance_cell(batch, &[Mat4::IDENTITY; 2], &[]);
            assert_eq!(ctx.gfx.instance_cell_len(cell), Some(2));
            *s2.borrow_mut() = Some((batch, cell));
        }),
        Box::new(move |ctx| {
            // Asked for again, it comes back under its id, and the cell with it.
            let (batch, cell) = s3.borrow().unwrap();
            let house = ctx.gfx.resident_mesh(&p4[0]);
            assert!(ctx.is_live(house) && ctx.is_live(cell));
            assert!(ctx.gfx.is_resident(ResidencyKind::Mesh, &p5[0]));
            assert_eq!(ctx.gfx.instance_batch_len(batch), Some(2));
        }),
    ]);
}
