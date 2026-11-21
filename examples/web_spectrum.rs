//! The sea's CPU wave spectrum in the browser: the benchmark the runner
//! `web/smoke/spectrum.mjs` reads (the water guide's "Waves from a
//! spectrum"). Build it with `web/build.sh --features 3d web_spectrum`;
//! the runner also builds it with `-C target-feature=+simd128` alone and
//! with `web/build.sh --simd` (that flag and the `wasm-simd` feature), and
//! compares the three.
//!
//! What it times is what a WebGL2 sea pays each frame: one new moment of
//! the spectrum's field on the CPU (`Waves::height_at` at a time not asked
//! before, which a WebGL2 upload, a query or the underwater check costs),
//! and one with the sway's speed (`Waves::sample`, what buoyancy asks), at
//! 64, 128 and 256 cells. The browser's clock is coarse (0.1 ms on a page
//! that isn't cross-origin isolated), so it times [`BATCH`] new moments at
//! a time and divides. One size and kind per frame, so the page keeps
//! answering; then it publishes, with the usual smoke state:
//!
//! `{"fixture":"web_spectrum","simd":true,"rustfft_simd":true,"done":true,
//! "bench":[{"cells":64,"kind":"height","moment_ms":[0.21,0.20,...]},...]}`
//!
//! `simd` says whether the build had `simd128` everywhere, `rustfft_simd`
//! whether rustfft's own WASM SIMD code was built (the `wasm-simd`
//! feature). On the desktop it opens a window like any other example and
//! prints the same state.

use ponko::prelude::*;
use ponko::water::{WaveSpectrum, Waves};
use serde_json::json;
use web_time::Instant;

#[path = "common/web_smoke.rs"]
mod smoke;

/// The grid sizes timed.
const CELLS: [u32; 3] = [64, 128, 256];
/// New moments timed together, so the browser's coarse clock still resolves one.
const BATCH: usize = 8;
/// Batches timed per size and kind (each gives one per-moment time).
const SAMPLES: usize = 15;
/// New moments worked out before the timing starts.
const WARMUP: usize = 4;

/// One size and kind, timed: milliseconds per new moment, one per batch.
struct Timed {
    cells: u32,
    kind: &'static str,
    moment_ms: Vec<f64>,
}

struct Bench {
    smoke: smoke::Smoke,
    /// The jobs left: (cells, with the sway's speed).
    todo: Vec<(u32, bool)>,
    done: Vec<Timed>,
    /// Every moment asked so far: each one is new, so none comes from the cache.
    moments: u32,
}

impl Bench {
    /// The next time no one has asked for.
    fn next_t(&mut self) -> f32 {
        self.moments += 1;
        self.moments as f32 * 0.0173
    }

    fn run(&mut self, cells: u32, speed: bool) -> Timed {
        let waves = Waves::spectrum(WaveSpectrum::new(vec2(12.0, 4.0), 2.5).with_tile(256.0, cells));
        let point = vec2(30.0, -8.0);
        let mut sink = 0.0f32;
        let mut ask = |bench: &mut Self| {
            let t = bench.next_t();
            sink += if speed {
                waves.sample(point, t).velocity.x
            } else {
                waves.height_at(point, t)
            };
        };
        for _ in 0..WARMUP {
            ask(self);
        }
        let mut moment_ms = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let start = Instant::now();
            for _ in 0..BATCH {
                ask(self);
            }
            moment_ms.push(start.elapsed().as_secs_f64() * 1000.0 / BATCH as f64);
        }
        std::hint::black_box(sink);
        Timed {
            cells,
            kind: if speed { "sway" } else { "height" },
            moment_ms,
        }
    }
}

fn main() {
    Game::new("Ponko: the sea's spectrum on the web", 320, 180)
        .setup(|ctx| {
            let todo = CELLS.iter().flat_map(|&c| [(c, false), (c, true)]).rev().collect();
            ctx.world.insert_resource(Bench {
                smoke: smoke::Smoke::new("web_spectrum"),
                todo,
                done: Vec::new(),
                moments: 0,
            });
        })
        .system(|ctx: &mut Ctx| {
            let mut bench = ctx.world.remove_resource::<Bench>().expect("made in setup");
            // The first frames draw before anything is timed.
            if ctx.time.frame() > 10
                && let Some((cells, speed)) = bench.todo.pop()
            {
                let timed = bench.run(cells, speed);
                bench.done.push(timed);
            }
            let rows: Vec<_> = bench
                .done
                .iter()
                .map(|t| json!({ "cells": t.cells, "kind": t.kind, "moment_ms": t.moment_ms }))
                .collect();
            let state = json!({
                "simd": cfg!(target_feature = "simd128"),
                "rustfft_simd": cfg!(all(target_arch = "wasm32", feature = "wasm-simd")),
                "batch": BATCH,
                "bench": rows,
                "done": bench.todo.is_empty(),
            });
            let ticks = ctx.time.sim_tick();
            bench.smoke.frame(ctx, ticks, state);
            ctx.world.insert_resource(bench);
        })
        .run();
}
