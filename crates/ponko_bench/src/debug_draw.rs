//! Debug drawing without a window (`debug/...`): 100,000 lines drawn with
//! `ctx.debug` every frame, then handed over as a drawn frame does. The
//! time is the CPU side of one frame: the drawing calls, and turning them
//! into the GPU's line list (3D) or ordered, batched sprites (2D). The GPU
//! side is measured by the offscreen test
//! `debug_lines_render_test::hundred_thousand_lines_gpu_time` (ignored by
//! default).
//!
//! - `debug/100k-lines-3d`: 100,000 3D lines, half of them on top.
//! - `debug/100k-lines-2d`: 100,000 2D lines in the world (sprites).
//! - `debug/100k-lines-off`: the same 100,000 calls with debug drawing off.
use std::hint::black_box;
use std::time::{Duration, Instant};

use ponko::debug::DebugDraw;
use ponko::prelude::*;

use crate::{Bench, alloc, report};

const LINES: usize = 100_000;
const WARMUP: usize = 10;
const FRAMES: usize = 100;

/// Every timed frame, the allocations meanwhile, and what the last one handed over.
struct Times(Vec<Duration>, alloc::Counts, (usize, usize, usize));

fn lines_3d(d: &mut DebugDraw) {
    for i in 0..LINES {
        let x = (i % 1000) as f32 * 0.01;
        let z = (i / 1000) as f32 * 0.01;
        let shape = d.line(vec3(x, 0.0, z), vec3(x, 1.0, z), Color::GREEN);
        if i % 2 == 0 {
            shape.on_top();
        }
    }
}

fn lines_2d(d: &mut DebugDraw) {
    for i in 0..LINES {
        let x = (i % 1000) as f32;
        let y = (i / 1000) as f32;
        d.line(vec2(x, y), vec2(x + 10.0, y + 5.0), Color::GREEN);
    }
}

/// A case: its name, whether drawing is on, and what a frame draws.
type Case = (&'static str, bool, fn(&mut DebugDraw));

pub fn run(b: &mut Bench) {
    let cases: [Case; 3] = [
        ("debug/100k-lines-3d", true, lines_3d),
        ("debug/100k-lines-2d", true, lines_2d),
        ("debug/100k-lines-off", false, lines_3d),
    ];
    for (name, on, draw) in cases {
        if b.skips(name) {
            continue;
        }
        let mut world = Game::new("debug bench", 1280, 720)
            .debug_draw(on)
            .run_headless(0, move |ctx| {
                let mut times = Vec::with_capacity(FRAMES);
                let mut last = (0, 0, 0);
                let mut allocs = alloc::Counts::default();
                for frame in 0..WARMUP + FRAMES {
                    let before = alloc::Counts::now();
                    let start = Instant::now();
                    let counts = ctx.bench_debug_frame(draw);
                    let took = start.elapsed();
                    let counted = alloc::Counts::now().since(before);
                    black_box(counts);
                    if frame >= WARMUP {
                        times.push(took);
                        allocs.count += counted.count;
                        allocs.bytes += counted.bytes;
                        last = counts;
                    }
                }
                ctx.world.insert_resource(Times(times, allocs, last));
            });
        let Times(mut times, allocs, (lines, sprites, batches)) = world.remove_resource::<Times>().expect("the times");
        if on {
            assert_eq!(lines + sprites, LINES, "{name}: every line handed over");
        } else {
            assert_eq!(lines + sprites, 0, "{name}: nothing while off");
        }
        eprintln!("{name}: {lines} 3D lines, {sprites} sprites in {batches} batches");
        let mut record = report::Record::new(name, report::Kind::Steady, WARMUP, &mut times);
        record.set_allocs(allocs, FRAMES);
        b.report(record);
    }
}
