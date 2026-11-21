//! The browser smoke-test fixture for saving in a web build with the
//! `editor` feature (`web/build.sh --features editor web_editor_save`,
//! driven by `web/smoke/`; the guide's "Saving in a browser").
//!
//! A browser has no `assets/` folder: with the editor in the build, saved
//! files go to the page's storage, are read from there at once, and come
//! back over the fetched files when the page is loaded again. On its first
//! load the fixture finds no `smoke/editor-kept.json` (the page never
//! fetched one), saves a scene there and reads it back; after a reload the
//! scene is found and loaded.
//!
//! It also saves over a file the build ships, `levels/beach.json` (two
//! palms), checks the save is read instead, lists what the browser keeps,
//! and reverts the file to the build's: it is read again at once, and
//! after the reload too (the guide's "Saved in this browser"). It
//! publishes (see `common/web_smoke.rs`):
//!
//! `{"fixture":"web_editor_save","ticks":120,"frames":118,"clicks":0,"size":[800,600],"asset":[32,32],"kept":"Saved","read_back":1,"listed":2,"reverted":1,"shipped":"Back"}`
//!
//! `kept` is `Saved` on the first load, `Loaded` after a reload (or the
//! error); `read_back` is 1 when the saved scene loaded again at once.
//! On the first load `listed` counts the files kept in the browser after
//! both saves and `reverted` is 1 when the revert brought the build's
//! file back at once; `shipped` is `Back` when the build's file is read
//! and the browser keeps no copy of it (else what is wrong). On the
//! desktop it writes `assets/smoke/editor-kept.json`, which a second run
//! finds; there the browser steps report nothing kept.

use ponko::hierarchy::EntityName;
use ponko::prelude::*;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

#[path = "common/web_smoke.rs"]
mod smoke;

/// The scene the fixture saves and finds again.
const KEPT: &str = "smoke/editor-kept.json";
/// The name of the entity the saved scene holds.
const MARK: &str = "kept by the browser";
/// A scene the build ships: two palms.
const SHIPPED: &str = "levels/beach.json";
/// The name of the entity of the scene saved over it, then reverted.
const OVER: &str = "saved over the build's file";

/// The component of the shipped scene's entities.
#[derive(Serialize, Deserialize, Clone, Copy, Component)]
struct Palm {
    height: f32,
}

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
            ctx.world.reflect::<Palm>();
            let (kept, read_back) = keep(ctx);
            let (listed, reverted) = if kept == "Saved" { revert(ctx) } else { (0, 0) };
            let shipped = shipped(ctx);
            ctx.world.insert_resource(Fixture {
                smoke,
                crab,
                kept,
                read_back,
                listed,
                reverted,
                shipped,
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
    named(ctx, MARK)
}

/// Whether the world has an entity named `name`.
fn named(ctx: &mut Ctx, name: &str) -> bool {
    ctx.world.query::<&EntityName>().iter().any(|n| n.0 == name)
}

/// The palms in the world.
fn palms(ctx: &mut Ctx) -> usize {
    ctx.world.query::<&Palm>().iter().count()
}

/// Saves over the shipped scene, then reverts it: what to publish as
/// `listed` (files kept in the browser after both saves) and `reverted`
/// (1 when the build's file came back at once).
fn revert(ctx: &mut Ctx) -> (u32, u32) {
    ctx.world.clear();
    ctx.world
        .spawn((ponko::d2::Transform::at(vec2(100.0, 100.0)), EntityName(OVER.into())));
    if ctx.assets.save_scene(&ctx.world, SHIPPED).is_err() {
        return (0, 0);
    }
    let replaced = ctx.load_scene(SHIPPED).is_ok() && named(ctx, OVER) && palms(ctx) == 0;
    let listed = ctx.assets.browser_saves().len() as u32;
    let dropped = ctx.assets.revert_browser_save(SHIPPED) == Ok(true);
    let back = ctx.load_scene(SHIPPED).is_ok() && palms(ctx) == 2 && !named(ctx, OVER);
    (listed, u32::from(replaced && dropped && back))
}

/// Whether the shipped scene is the build's (`Back`), else what is wrong.
fn shipped(ctx: &mut Ctx) -> String {
    if ctx.assets.browser_saves().iter().any(|n| n == SHIPPED) {
        return "Still kept in the browser".into();
    }
    match ctx.load_scene(SHIPPED) {
        Ok(()) if palms(ctx) == 2 => "Back".into(),
        Ok(()) => format!("{} palms", palms(ctx)),
        Err(e) => format!("Error: {e}"),
    }
}

/// What setup made, for the frame system.
struct Fixture {
    smoke: smoke::Smoke,
    crab: Texture,
    kept: String,
    read_back: u32,
    listed: u32,
    reverted: u32,
    shipped: String,
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
            "listed": fixture.listed,
            "reverted": fixture.reverted,
            "shipped": fixture.shipped,
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
