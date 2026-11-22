//! The browser smoke-test fixture for saving in a web build with the
//! `editor` feature (`web/build.sh --features editor web_editor_save`,
//! driven by `web/smoke/`; the guide's "Saving in a browser").
//!
//! A browser has no `assets/` folder: with the editor in the build, saved
//! files go to the page's storage, are read from there at once, and come
//! back over the fetched files when the page is loaded again. The fixture
//! runs over three page loads, each deciding what to do from what the
//! browser kept:
//!
//! 1. Nothing kept: it saves a scene to `smoke/editor-kept.json` (the page
//!    never fetched one) and over `levels/beach.json` (a file the build
//!    ships, two palms), and reads both back at once.
//! 2. After a reload: both saves are found (the saved-over scene instead
//!    of the build's palms), and it reverts `levels/beach.json` to the
//!    build's file, which is read again at once.
//! 3. After another reload: the revert held (the build's palms, no copy
//!    kept in the browser), and the other save is still there.
//!
//! It publishes (see `common/web_smoke.rs`), on the first load:
//!
//! `{"fixture":"web_editor_save","ticks":120,"frames":118,"clicks":0,"size":[800,600],"asset":[32,32],"found":"Build","kept":"Saved","read_back":1,"listed":2,"reverted":0,"shipped":"Saved","persistent":"Refused"}`
//!
//! `found` is what `levels/beach.json` was when the page started: `Build`
//! (the build's file) or `Saved` (the save over it). `kept` is `Saved` on
//! the first load, `Loaded` once the scene saved there is found (or the
//! error); `read_back` is 1 when both saves were read back at once, and
//! `listed` counts the files the browser keeps then. `reverted` is 1 on
//! the load that reverted the saved-over scene and read the build's file
//! again at once. `shipped` is what `levels/beach.json` is at the end of
//! setup, in the same words as `found`, else what is wrong. `persistent`
//! is whether the browser keeps the page's storage: `Granted`, `Refused`
//! (headless Chromium refuses; also before the page asked, which only a
//! save does, so on the later loads unless an earlier one was granted)
//! or `Asking` (no answer yet). The runner's `--persist granted|refused`
//! answers for the browser and counts the asks (`persist_asks`): the
//! first load's saves ask once, the later loads save nothing and never
//! ask. On the desktop
//! it writes `assets/smoke/editor-kept.json`, which a second run finds;
//! the shipped file is left alone there.

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
            let found = shipped(ctx);
            let mut fixture = Fixture {
                smoke,
                crab,
                found: found.clone(),
                kept: String::new(),
                read_back: 0,
                listed: 0,
                reverted: 0,
                shipped: String::new(),
            };
            // A browser runs all three steps; the desktop only the first
            // part, as it has no browser copies to revert.
            let browser = cfg!(target_arch = "wasm32");
            if found == "Saved" {
                // Second load: both saves came back; revert the one.
                fixture.kept = find_kept(ctx);
                fixture.reverted = revert(ctx);
            } else if ctx.load_scene(KEPT).is_ok() {
                // Third load (or a later desktop run).
                fixture.kept = find_kept(ctx);
            } else {
                // First load: save both and read them back.
                let (kept, kept_back) = keep(ctx);
                let over_back = !browser || save_over(ctx);
                fixture.kept = kept;
                fixture.read_back = u32::from(kept_back && over_back);
                fixture.listed = ctx.assets.browser_saves().len() as u32;
            }
            fixture.shipped = shipped(ctx);
            ctx.world.insert_resource(fixture);
            Ok(())
        })
        .run();
}

/// Loads the kept scene: `Loaded` when it has the mark, else what is wrong.
fn find_kept(ctx: &mut Ctx) -> String {
    match ctx.load_scene(KEPT) {
        Ok(()) if marked(ctx) => "Loaded".into(),
        Ok(()) => "Loaded without the mark".into(),
        Err(e) => format!("Error: {e}"),
    }
}

/// Saves the kept scene and reads it back: what to publish as `kept`, and
/// whether it was read back at once.
fn keep(ctx: &mut Ctx) -> (String, bool) {
    ctx.world.clear();
    ctx.world
        .spawn((ponko::d2::Transform::at(vec2(400.0, 300.0)), EntityName(MARK.into())));
    if let Err(e) = ctx.assets.save_scene(&ctx.world, KEPT) {
        return (format!("Error: {e}"), false);
    }
    let read_back = ctx.load_scene(KEPT).is_ok() && marked(ctx);
    ("Saved".into(), read_back)
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

/// Saves over the shipped scene: whether the save is read instead of the
/// build's file at once.
fn save_over(ctx: &mut Ctx) -> bool {
    ctx.world.clear();
    ctx.world
        .spawn((ponko::d2::Transform::at(vec2(100.0, 100.0)), EntityName(OVER.into())));
    if ctx.assets.save_scene(&ctx.world, SHIPPED).is_err() {
        return false;
    }
    ctx.load_scene(SHIPPED).is_ok() && named(ctx, OVER) && palms(ctx) == 0
}

/// Reverts the shipped scene to the build's file: 1 when the browser had
/// a copy and the build's file is read again at once.
fn revert(ctx: &mut Ctx) -> u32 {
    let dropped = ctx.assets.revert_browser_save(SHIPPED) == Ok(true);
    let back = ctx.load_scene(SHIPPED).is_ok() && palms(ctx) == 2 && !named(ctx, OVER);
    u32::from(dropped && back)
}

/// What the shipped scene is now: `Build` (the build's file, no copy
/// kept), `Saved` (the save over it, kept in the browser), else what is
/// wrong.
fn shipped(ctx: &mut Ctx) -> String {
    let kept = ctx.assets.browser_saves().iter().any(|n| n == SHIPPED);
    match ctx.load_scene(SHIPPED) {
        Ok(()) if kept && named(ctx, OVER) && palms(ctx) == 0 => "Saved".into(),
        Ok(()) if !kept && palms(ctx) == 2 => "Build".into(),
        Ok(()) => format!("{} palms, kept in the browser: {kept}", palms(ctx)),
        Err(e) => format!("Error: {e}"),
    }
}

/// What setup made, for the frame system.
struct Fixture {
    smoke: smoke::Smoke,
    crab: Texture,
    found: String,
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
            "found": fixture.found,
            "kept": fixture.kept,
            "read_back": fixture.read_back,
            "listed": fixture.listed,
            "reverted": fixture.reverted,
            "shipped": fixture.shipped,
            "persistent": match ctx.assets.browser_saves_persistent() {
                Some(true) => "Granted",
                Some(false) => "Refused",
                None => "Asking",
            },
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
