//! Heightfield terrain: 5 x 5 tiles of 256 m (129 x 129 heights, 2 m
//! apart), drawn coarser with distance, with grass, dirt and rock blended
//! by a weight map, and a lake from the water kit (`ponko::water`) with a
//! planar reflection, shore foam and its underwater view.
//!
//!   `cargo run --release --example terrain --features 3d`
//!
//! WASD moves, Q and E go down and up, the arrows turn, U switches the
//! underwater view. `SHOT=path.png` saves the third frame and quits;
//! `VIEW=high|low|under|cliff` picks where the camera starts, `HOUR=18.5` the
//! time of day. `BENCH=1` prints the frame's cost after 300 frames and quits.
//!
//! The scene and keys live in `scene.rs`, which the engine's offscreen GPU
//! tests also draw.

mod scene;

use ponko::prelude::*;
use ponko::water::WaterPlugin;

fn main() {
    let view = std::env::var("VIEW").unwrap_or_default();
    let hour = std::env::var("HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(16.0);
    let bench = std::env::var("BENCH").is_ok();
    Game::new("Ponko: terrain", 1280, 720)
        .vsync(!bench)
        .fullscreen(bench)
        .plugin(WaterPlugin)
        .setup(move |ctx| {
            let terrain = scene::setup(ctx, &view, hour);
            ctx.world.insert_resource(terrain);
        })
        .system(move |ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            let mut terrain = *ctx.world.resource::<scene::Terrain>();
            scene::update(ctx, &mut terrain);
            ctx.world.insert_resource(terrain);
            if ctx.time.frame() == 3
                && let Ok(path) = std::env::var("SHOT")
            {
                ctx.gfx.save_screenshot(path);
            }
            if ctx.time.frame() == 5 && std::env::var("SHOT").is_ok() {
                ctx.quit();
            }
            if ctx.time.frame() == 300 {
                let stats = ctx.perf.render_stats();
                println!("{stats:?} models drawn: {}", ctx.gfx.models_drawn());
                if bench {
                    ctx.quit();
                }
            }
        })
        .run();
}
