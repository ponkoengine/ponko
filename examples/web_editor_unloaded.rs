//! The browser smoke-test fixture for the editor's panel of glTF scenes that
//! didn't load (`web/build.sh --features editor,3d web_editor_unloaded`,
//! driven by `web/smoke/`; the guide's "Scenes that didn't load").
//!
//! The page opens in edit mode on `smoke/unloaded/scene.json`
//! (`?editor=smoke/unloaded/scene.json`): two glTF scenes, a yard and a
//! post, with the yard's wall hung off the post. Setup saves something
//! that isn't a glTF file over the post's file in the browser's storage, so
//! when the editor opens the scene the post doesn't load: the editor lists
//! it in its panel with why and a **Retry**, and the wall stays on its own
//! scene. The fixture then reverts that save (the build's file is read again
//! at once), and the runner clicks **Retry**: the post spawns where it was
//! saved and the wall hangs off it again.
//!
//! It publishes (see `common/web_smoke.rs`):
//!
//! `{"fixture":"web_editor_unloaded","ticks":240,"frames":240,"clicks":1,"size":[800,600],"roots":2,"failed":1,"linked":1}`
//!
//! `ticks` counts frames (edit mode pauses the simulation). `roots` is how
//! many glTF scenes are spawned, `failed` is 1 once the post was seen left
//! out (and its broken copy reverted), and `linked` is 1 when the wall hangs
//! off the post. Web builds only: on the desktop it would write over the
//! shipped file, so there it only reports.

use ponko::d3::GltfRoot;
use ponko::prelude::*;

use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

#[path = "common/web_smoke.rs"]
mod smoke;

/// The glTF scene that fails to load at first.
const POSTS: &str = "smoke/unloaded/posts.gltf";
/// The scene the page opens in edit mode.
#[allow(dead_code)] // named in the page's address, and the docs
const SCENE: &str = "smoke/unloaded/scene.json";

/// What the frame system keeps between frames.
struct Fixture {
    smoke: smoke::Smoke,
    failed: bool,
}

fn main() {
    let frames = Rc::new(Cell::new(0u64));
    Game::new("Ponko: web editor, scenes that didn't load", 800, 600)
        .system(frame(frames))
        .try_setup(|ctx| {
            let smoke = smoke::Smoke::new("web_editor_unloaded");
            // A broken copy of the post's file, kept in the browser: the
            // editor reads it instead of the build's when it opens the scene.
            if cfg!(target_arch = "wasm32") {
                ctx.assets.save_scene(&ctx.world, POSTS)?;
            }
            ctx.world.insert_resource(Fixture { smoke, failed: false });
            Ok(())
        })
        .run();
}

/// The spawned glTF scenes: each one's root and file.
fn roots(ctx: &mut Ctx) -> Vec<(Entity, String)> {
    ctx.world
        .query::<(Entity, &GltfRoot)>()
        .iter()
        .map(|(e, r)| (e, r.path.clone()))
        .collect()
}

/// Whether the yard's wall hangs off the post.
fn linked(ctx: &mut Ctx, roots: &[(Entity, String)]) -> bool {
    let node = |ctx: &mut Ctx, file: &str, path: &str| {
        let (root, _) = roots.iter().find(|(_, f)| f == file)?;
        ctx.world.get::<GltfRoot>(*root)?.entity(path)
    };
    let (Some(wall), Some(post)) = (
        node(ctx, "smoke/unloaded/yard.gltf", "Wall"),
        node(ctx, POSTS, "Post"),
    ) else {
        return false;
    };
    ctx.world
        .get::<ponko::attach::Attached3D>(wall)
        .is_some_and(|l| l.to == post)
}

/// The frame system: once the editor opened the scene without the post,
/// reverts the broken copy (so Retry finds the build's file), and
/// publishes the state.
fn frame(frames: Rc<Cell<u64>>) -> impl FnMut(&mut Ctx) {
    move |ctx: &mut Ctx| {
        frames.set(frames.get() + 1);
        let mut fixture = ctx.world.remove_resource::<Fixture>().expect("made in setup");
        let now = roots(ctx);
        let yard_only = now.len() == 1 && now[0].1 != POSTS;
        if !fixture.failed && yard_only && ctx.assets.revert_browser_save(POSTS) == Ok(true) {
            fixture.failed = true;
        }
        let state = json!({
            "roots": now.len(),
            "failed": u32::from(fixture.failed),
            "linked": u32::from(linked(ctx, &now)),
        });
        fixture.smoke.frame(ctx, frames.get(), state);
        ctx.world.insert_resource(fixture);
    }
}
