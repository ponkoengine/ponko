//! The browser smoke-test fixture for saving in a web build with the
//! `editor` feature (`web/build.sh --features editor web_editor_save`,
//! driven by `web/smoke/`; the guide's "Saving in a browser").
//!
//! A browser has no `assets/` folder: with the editor in the build, saved
//! files go to the page's storage, are read from there at once, and come
//! back over the fetched files when the page is loaded again. On its first
//! load the fixture finds no `smoke/editor-kept.json` (the page never
//! fetched one), saves a scene there and reads it back; after a reload the
//! scene is found and loaded. It publishes (see `common/web_smoke.rs`):
//!
//! `{"fixture":"web_editor_save","ticks":120,"frames":118,"clicks":0,"size":[800,600],"asset":[32,32],"kept":"Saved","read_back":1}`
//!
//! `kept` is `Saved` on the first load, `Loaded` after a reload (or the
//! error); `read_back` is 1 when the saved scene loaded again at once. On
//! the desktop it writes `assets/smoke/editor-kept.json`, which a second
//! run finds.

use ponko::hierarchy::EntityName;
use ponko::prelude::*;

use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

#[path = "common/web_smoke.rs"]
mod smoke;

/// The scene the fixture saves and finds again.
const KEPT: &str = "smoke/editor-kept.json";
/// The name of the entity the saved scene holds.
const MARK: &str = "kept by the browser";

fn main() {
    let ticks = Rc::new(Cell::new(0u64));
    let sim_ticks = ticks.clone();
    Game::new("Ponko: web editor save", 800, 600)
        .fixed_hz(60)
        .sim_system(move |_ctx| sim_ticks.set(sim_ticks.get() + 1))
        .system(frame(ticks))
        .try_setup(|ctx| {
            let smoke = smoke::Smoke::new("web_editor_save");
            let crab = ctx.gfx.try_load_texture("crab.png")?;
            let (kept, read_back) = keep(ctx);
            ctx.world.insert_resource(Fixture {
                smoke,
                crab,
                kept,
                read_back,
            });
            Ok(())
        })
        .run();
}

/// Loads the kept scene, or saves it and reads it back: what to publish
/// as `kept` and `read_back`.
fn keep(ctx: &mut Ctx) -> (String, u32) {
    if ctx.load_scene(KEPT).is_ok() {
        let kept = if marked(ctx) {
            "Loaded"
        } else {
            "Loaded without the mark"
        };
        return (kept.into(), 0);
    }
    ctx.world
        .spawn((ponko::d2::Transform::at(vec2(400.0, 300.0)), EntityName(MARK.into())));
    if let Err(e) = ctx.assets.save_scene(&ctx.world, KEPT) {
        return (format!("Error: {e}"), 0);
    }
    let read_back = ctx.load_scene(KEPT).is_ok() && marked(ctx);
    ("Saved".into(), u32::from(read_back))
}

/// Whether the world has the saved scene's entity.
fn marked(ctx: &mut Ctx) -> bool {
    ctx.world.query::<&EntityName>().iter().any(|n| n.0 == MARK)
}

/// What setup made, for the frame system.
struct Fixture {
    smoke: smoke::Smoke,
    crab: Texture,
    kept: String,
    read_back: u32,
}

/// The frame system: publishes the state and draws.
fn frame(ticks: Rc<Cell<u64>>) -> impl FnMut(&mut Ctx) {
    move |ctx: &mut Ctx| {
        // Taken out while it runs, as `Smoke::frame` needs all of `ctx`.
        let mut fixture = ctx.world.remove_resource::<Fixture>().expect("made in setup");
        let crab = fixture.crab;
        let state = json!({
            "asset": [crab.width(), crab.height()],
            "kept": fixture.kept,
            "read_back": fixture.read_back,
        });
        fixture.smoke.frame(ctx, ticks.get(), state);
        ctx.gfx.clear(Color::hex(0x1E2A38));
        let size = ctx.window_size();
        let t = ticks.get() as f32 / 60.0;
        for i in 0..6 {
            let angle = t + i as f32 * std::f32::consts::TAU / 6.0;
            let at = size / 2.0 + Vec2::from_angle(angle) * size.min_element() * 0.3;
            ctx.gfx
                .draw_ex(crab, at, DrawParams::default().with_scale(Vec2::splat(3.0)));
        }
        ctx.world.insert_resource(fixture);
    }
}
