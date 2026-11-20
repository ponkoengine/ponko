//! The sky kit: the engine's ready-made sky with drifting clouds, their
//! shadows on the ground, a moon with phases and turning stars, through a
//! day on fast forward over a field of blocks.
//!
//! Space holds the clock; C steps the cloud cover (clear to overcast); Q
//! steps the cloud quality (`flat`, `low`, `medium`, `high`); Escape quits.
//!
//!   `cargo run --release --example sky_kit --features 3d`
//!
//! `HOUR=21` starts at nine in the evening.

mod scene;

use ponko::prelude::*;

fn main() {
    let hour = std::env::var("HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(6.0);
    let demo = std::rc::Rc::new(std::cell::RefCell::new(None::<scene::SkyDemo>));
    let state = demo.clone();
    Game::new("Ponko: sky kit", 1280, 720)
        .setup(move |ctx| {
            *state.borrow_mut() = Some(scene::setup(ctx, hour));
        })
        .system(move |ctx: &mut Ctx| {
            if ctx.input.key_pressed(Key::Escape) {
                ctx.quit();
            }
            if let Some(demo) = demo.borrow_mut().as_mut() {
                let before = ctx.scene3d.sky.kit.map(|k| (k.clouds.coverage, k.quality));
                scene::update(ctx, demo);
                let now = ctx.scene3d.sky.kit.map(|k| (k.clouds.coverage, k.quality));
                if let Some((cover, quality)) = now.filter(|_| now != before) {
                    println!("clouds: cover {cover}, quality {}", quality.name());
                }
            }
        })
        .run();
}
