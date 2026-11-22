//! Time to first frame, Ponko's side of the scoreboard (`bench/compare`):
//! a game with one sprite whose first frame is drawn offscreen with the
//! window's renderer and finished on the GPU, then the process exits. The
//! scoreboard times the whole process from outside; this prints the part
//! from `main` on. Run with
//! `cargo run --release -p ponko_bench --features render --bin first_frame`.

use std::time::Instant;

use ponko::d2::{Sprite, Transform};
use ponko::prelude::*;

fn main() {
    let started = Instant::now();
    Game::new("first frame", 1280, 720)
        .bench_offscreen()
        .run_headless(1, |ctx| {
            let texture = ctx.gfx.create_texture(32, 32, &[255; 32 * 32 * 4]);
            ctx.world
                .spawn((Transform::at(vec2(640.0, 360.0)), Sprite::new(texture)));
        });
    println!(
        "first frame {:.1} ms after main started",
        started.elapsed().as_secs_f64() * 1e3
    );
}
