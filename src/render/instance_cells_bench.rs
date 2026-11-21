//! Offscreen benchmark of instance cells: a world of instances streamed
//! around a flying camera, kept as cells (each added and freed on its own)
//! against one batch made again whenever a cell comes or goes, on the
//! GPU-driven and the CPU path. No window is opened. Run explicitly, in
//! release:
//! `cargo test --release --lib --features 3d instance_cells_benchmark -- --ignored --nocapture`.
//! The results are in `crates/ponko_bench/INSTANCE_CELLS.md`.
use super::gpu3d::BatchPath;
use super::instance_cells_render_test::{add_cell, free_batch, free_cell, whole_batch};
use super::static_batch_render_test::{BatchRig, RigSetup};
use crate::prelude::*;
use crate::render::{InstanceBatch, InstanceCell, MaterialDesc, MeshData};
use crate::streaming::CellStreamer;
use std::collections::BTreeMap;
use std::time::Instant;

const SIZE: (u32, u32) = (1280, 720);
/// Width of a streamed cell, in metres, and its instances.
const CELL: f32 = 32.0;
const PER_CELL: usize = 1024;
const LOAD: f32 = 160.0;
const KEEP: f32 = 200.0;
/// New cells a frame at most (the game's upload budget).
const PER_FRAME: usize = 2;
const WARM_UP: usize = 60;
const FRAMES: usize = 600;
/// Metres the camera flies a frame: a new row of cells every 16 frames.
const SPEED: f32 = 2.0;

/// A cell's instances: a jittered grid of cubes 1 m apart.
fn instances((x, z): (i32, i32)) -> Vec<Mat4> {
    let side = (PER_CELL as f32).sqrt() as usize;
    let step = CELL / side as f32;
    (0..PER_CELL)
        .map(|i| {
            let (u, v) = ((i % side) as f32, (i / side) as f32);
            let jitter = ((i as i32 * 31 + x * 7 + z * 13).rem_euclid(9) as f32 - 4.0) * 0.05;
            Mat4::from_translation(vec3(
                x as f32 * CELL + (u + 0.5) * step + jitter,
                0.2,
                z as f32 * CELL + (v + 0.5) * step - jitter,
            ))
        })
        .collect()
}

/// What one run measured.
struct Measured {
    /// Milliseconds the game thread spent adding and freeing (or making the
    /// batch again), per cell that came or went.
    per_cell: Vec<f64>,
    /// Milliseconds of each frame that changed cells: the streaming work
    /// plus drawing, until the GPU finished.
    changed: Vec<f64>,
    /// Milliseconds of every frame, the same.
    frames: Vec<f64>,
    cells: usize,
}

fn run(path: BatchPath, as_cells: bool) -> Option<Measured> {
    let mut rig = BatchRig::new(RigSetup {
        size: SIZE,
        ..Default::default()
    })?;
    rig.renderer.batching.path = path;
    let mesh = rig.mesh(
        &MeshData::cube(0.3),
        MaterialDesc {
            color: Color::WHITE,
            ..Default::default()
        },
    );
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.shadow_distance = 80.0;
    rig.ctx.scene3d.camera.far = LOAD + CELL;
    let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    let mut streamer = CellStreamer::new(CELL, LOAD, KEEP);
    let mut cells: BTreeMap<(i32, i32), InstanceCell> = BTreeMap::new();
    let mut whole: Option<InstanceBatch> = None;
    let mut out = Measured {
        per_cell: Vec::new(),
        changed: Vec::new(),
        frames: Vec::new(),
        cells: 0,
    };
    for k in 0..WARM_UP + FRAMES {
        let at = vec3(k as f32 * SPEED, 6.0, 0.0);
        rig.ctx.scene3d.camera.position = at;
        rig.ctx.scene3d.camera.look_at(at + vec3(1.0, -0.25, -0.4));
        let start = Instant::now();
        streamer.update(at, PER_FRAME);
        let moved = streamer.to_load().len() + streamer.to_drop().len();
        for cell in streamer.to_drop() {
            let handle = cells.remove(cell).expect("loaded");
            if as_cells {
                free_cell(&mut rig, handle);
            }
        }
        for &cell in streamer.to_load() {
            let handle = if as_cells {
                add_cell(&mut rig, batch, &instances(cell), &[])
            } else {
                InstanceCell::NONE
            };
            cells.insert(cell, handle);
        }
        if !as_cells && moved > 0 {
            if let Some(old) = whole.take() {
                free_batch(&mut rig, old);
            }
            let all: Vec<Mat4> = cells.keys().flat_map(|&c| instances(c)).collect();
            whole = Some(whole_batch(&mut rig, mesh, &all, &[]));
        }
        let streaming = start.elapsed().as_secs_f64() * 1e3;
        let cost = rig.frame();
        if k < WARM_UP {
            continue;
        }
        let frame = streaming + cost.frame_ms;
        out.frames.push(frame);
        if moved > 0 {
            out.changed.push(frame);
            out.per_cell.push(streaming / moved as f64);
        }
        out.cells = out.cells.max(cells.len());
    }
    Some(out)
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// Width of a cell of the draw benchmark, in metres, and its instances
/// (a 4 x 4 grid).
const DRAW_CELL: f32 = 4.0;
const DRAW_PER_CELL: usize = 16;
const DRAW_WARM_UP: usize = 20;
const DRAW_FRAMES: usize = 120;

/// What one run of the draw benchmark measured.
struct Drawn {
    /// Cells with instances in the camera's view (counted on the CPU path).
    visible: Option<usize>,
    /// Per frame, over every pass and view: draw calls, bind groups set,
    /// vertex buffers set (the same every frame: the scene stands still).
    draws: u32,
    bind_groups: u32,
    vertex_buffers: u32,
    frames: Vec<f64>,
}

/// `cells` cells laid out `columns` wide, all in the camera's view (from
/// above), with sun shadows: each cell on its own (`separate`, as before
/// cells were drawn together) or drawn together.
fn draw_run(path: BatchPath, cells: usize, columns: usize, separate: bool) -> Option<Drawn> {
    let mut rig = BatchRig::new(RigSetup {
        size: SIZE,
        ..Default::default()
    })?;
    rig.renderer.batching.path = path;
    rig.renderer.batching.separate_cells = separate;
    let mesh = rig.mesh(
        &MeshData::cube(0.3),
        MaterialDesc {
            color: Color::WHITE,
            ..Default::default()
        },
    );
    rig.ctx.scene3d.lighting.shadows = true;
    rig.ctx.scene3d.lighting.shadow_distance = 120.0;
    let batch = rig.ctx.gfx.create_instance_batch(mesh, &[], Default::default());
    let rows = cells.div_ceil(columns);
    let side = (DRAW_PER_CELL as f32).sqrt() as usize;
    for c in 0..cells {
        let (x, z) = ((c % columns) as f32, (c / columns) as f32);
        let transforms: Vec<Mat4> = (0..DRAW_PER_CELL)
            .map(|i| {
                let (u, v) = ((i % side) as f32 + 0.5, (i / side) as f32 + 0.5);
                Mat4::from_translation(vec3(
                    (x + u / side as f32) * DRAW_CELL,
                    0.2,
                    (z + v / side as f32) * DRAW_CELL,
                ))
            })
            .collect();
        add_cell(&mut rig, batch, &transforms, &[]);
    }
    // From above, the whole field in the 16:9 view (60 degrees tall).
    let (width, depth) = (columns as f32 * DRAW_CELL, rows as f32 * DRAW_CELL);
    let centre = vec3(width * 0.5, 0.0, depth * 0.5);
    let height = (width * 0.5 / 1.0).max(depth * 0.5 / 0.577) * 1.15;
    let camera = &mut rig.ctx.scene3d.camera;
    camera.far = height * 3.0;
    camera.position = centre + vec3(0.0, height, 1.0);
    camera.look_at(centre);
    let mut out = Drawn {
        visible: None,
        draws: 0,
        bind_groups: 0,
        vertex_buffers: 0,
        frames: Vec::new(),
    };
    for k in 0..DRAW_WARM_UP + DRAW_FRAMES {
        let cost = rig.frame();
        if k < DRAW_WARM_UP {
            continue;
        }
        out.frames.push(cost.frame_ms);
        (out.draws, out.bind_groups, out.vertex_buffers) = (
            cost.changes.draws,
            cost.changes.bind_groups,
            cost.changes.vertex_buffers,
        );
    }
    if path == BatchPath::Cpu {
        let in_view = rig
            .renderer
            .static_batches
            .iter()
            .flatten()
            .filter(|b| b.visible.first().is_some_and(|l| !l.is_empty()));
        out.visible = Some(in_view.count());
    }
    Some(out)
}

/// Draw calls and frame times of 100 and 1,000 visible cells, each cell on
/// its own and drawn together, on both paths.
fn draw_table() {
    println!(
        "| path | visible cells | cells drawn | draws per frame | bind groups set | vertex buffers set | frame ms (p50 / p95 / p99 / max) |"
    );
    println!("|---|---|---|---|---|---|---|");
    for path in [BatchPath::Gpu, BatchPath::Cpu] {
        for (cells, columns) in [(100, 10), (1000, 40)] {
            for separate in [true, false] {
                let Some(m) = draw_run(path, cells, columns, separate) else {
                    return;
                };
                println!(
                    "| {path:?} | {} | {} | {} | {} | {} | {:.2} / {:.2} / {:.2} / {:.2} |",
                    m.visible.map_or_else(|| cells.to_string(), |v| v.to_string()),
                    if separate {
                        "each on its own (before)"
                    } else {
                        "together (after)"
                    },
                    m.draws,
                    m.bind_groups,
                    m.vertex_buffers,
                    percentile(&m.frames, 0.5),
                    percentile(&m.frames, 0.95),
                    percentile(&m.frames, 0.99),
                    percentile(&m.frames, 1.0),
                );
            }
        }
    }
}

/// `PONKO_CELLS_BENCH=draws` runs only the draw table, `stream` only the
/// streaming one; unset, both.
#[test]
#[ignore = "benchmark: run explicitly in release"]
fn instance_cells_benchmark() {
    let only = std::env::var("PONKO_CELLS_BENCH").unwrap_or_default();
    if only != "stream" {
        draw_table();
    }
    if only == "draws" {
        return;
    }
    println!(
        "| path | streamed as | live cells | upload ms per cell (p50 / p95 / max) | frame ms (p50 / p95 / p99 / max) | frames that changed cells, ms (p50 / p99) |"
    );
    println!("|---|---|---|---|---|---|");
    for path in [BatchPath::Gpu, BatchPath::Cpu] {
        for as_cells in [true, false] {
            // `None` only without a GPU, which the rig reports.
            let Some(m) = run(path, as_cells) else {
                return;
            };
            println!(
                "| {path:?} | {} | {} | {:.3} / {:.3} / {:.3} | {:.2} / {:.2} / {:.2} / {:.2} | {:.2} / {:.2} |",
                if as_cells { "cells" } else { "one batch, made again" },
                m.cells,
                percentile(&m.per_cell, 0.5),
                percentile(&m.per_cell, 0.95),
                percentile(&m.per_cell, 1.0),
                percentile(&m.frames, 0.5),
                percentile(&m.frames, 0.95),
                percentile(&m.frames, 0.99),
                percentile(&m.frames, 1.0),
                percentile(&m.changed, 0.5),
                percentile(&m.changed, 0.99),
            );
        }
    }
}
