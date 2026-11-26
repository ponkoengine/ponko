//! The editor's model thumbnails, drawn by the engine's own renderer: a
//! small offscreen world on the game's GPU (the same device and queue, its
//! own renderer), where a model is loaded, framed from the front-right and
//! above, drawn once and captured, then freed again. It runs on the game
//! thread, one model per editor frame; `editor_thumbs.rs` keeps the
//! results on disk by the model's content.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use glam::{Vec2, Vec3};

use super::thumbs::{SIZE, Thumb};
use crate::ctx::Ctx;
use crate::render::Renderer;

/// Pixels the studio draws on each side, before the picture is scaled
/// down to [`SIZE`] (which smooths its edges).
const DRAWN: u32 = 2 * SIZE as u32;

/// The offscreen world thumbnails are drawn in.
pub(crate) struct Studio {
    ctx: Ctx,
    /// The files of the model being drawn, by asset name.
    files: Rc<RefCell<HashMap<String, Vec<u8>>>>,
    /// Frames drawn so far, for the test that a thumbnail takes one.
    #[cfg(test)]
    frames: u32,
}

/// The studio's only asset source: the files of the model being drawn.
struct Files(Rc<RefCell<HashMap<String, Vec<u8>>>>);

impl crate::assets::AssetSource for Files {
    fn id(&self) -> &str {
        "editor-thumbnails"
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        self.0
            .borrow()
            .get(name)
            .cloned()
            .ok_or_else(|| std::io::ErrorKind::NotFound.into())
    }
}

impl Studio {
    /// A studio drawing on `gpu`'s device.
    pub(crate) fn new(gpu: &Renderer) -> Self {
        let mut ctx = Ctx::new(1.0 / 60.0, Vec2::splat(DRAWN as f32));
        ctx.audio.headless = true;
        ctx.gfx.renderer = Some(gpu.offscreen_twin((DRAWN, DRAWN)));
        ctx.scene3d.sky.atmosphere = false;
        let files = Rc::default();
        ctx.assets.add_source(Files(Rc::clone(&files)));
        Self {
            ctx,
            files,
            #[cfg(test)]
            frames: 0,
        }
    }

    /// The frames drawn since the studio was made.
    #[cfg(test)]
    pub(crate) fn frames_drawn(&self) -> u32 {
        self.frames
    }

    /// Draws model `rel` (in assets folder `root`) and saves its thumbnail
    /// to `out` as a PNG.
    pub(crate) fn render(&mut self, root: &Path, rel: &str, out: &Path) -> Result<Thumb, String> {
        let file = root.join(rel);
        let bytes = std::fs::read(&file).map_err(|e| format!("{rel}: {e}"))?;
        {
            // The file and those it names, under the names it uses.
            let mut files = self.files.borrow_mut();
            files.clear();
            let folder = file.parent().unwrap_or(root);
            let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
            for uri in super::thumbs::named_files(&bytes) {
                if let Ok(b) = std::fs::read(folder.join(&uri)) {
                    let name = if dir.is_empty() { uri } else { format!("{dir}/{uri}") };
                    files.insert(name, b);
                }
            }
            files.insert(rel.to_string(), bytes);
        }
        let drawn = self.draw(rel, out);
        self.files.borrow_mut().clear();
        drawn?;
        let image = image::open(out)
            .map_err(|e| format!("{}: {e}", out.display()))?
            .to_rgba8();
        Ok(Thumb {
            width: image.width() as usize,
            height: image.height() as usize,
            rgba: image.into_raw(),
            note: None,
        })
    }

    /// Loads `rel`, frames it, draws it into `out` and frees it again.
    fn draw(&mut self, rel: &str, out: &Path) -> Result<(), String> {
        let ctx = &mut self.ctx;
        let mesh = ctx.gfx.try_load_mesh(rel).map_err(|e| e.to_string())?;
        let (center, radius) = ctx.gfx.assets3d.live_bounds(mesh.id).unwrap_or((Vec3::ZERO, 1.0));
        let radius = radius.max(1e-3);
        let camera = &mut ctx.scene3d.camera;
        // As the software thumbnails look: from the front-right, above.
        let distance = radius / (camera.fov_y * 0.5).sin() * 1.05;
        camera.position = center + Vec3::new(0.55, 0.45, 0.7).normalize() * distance;
        camera.look_at(center);
        camera.near = (distance - radius) * 0.5;
        camera.far = distance + radius * 2.0;
        let e = ctx
            .world
            .spawn((crate::d3::Transform::default(), crate::render::Model::new(mesh)));
        // One frame is enough: the first frame of a view draws its 3D
        // picture (#632), and the model's mesh and materials are made in it.
        let (reply, answer) = std::sync::mpsc::channel();
        ctx.gfx.queue_screenshot(out.to_path_buf(), SIZE as u32, Some(reply));
        ctx.render_frame();
        #[cfg(test)]
        {
            self.frames += 1;
        }
        ctx.world.despawn(e);
        ctx.gfx.free(mesh);
        ctx.gfx.release_file_materials(rel);
        match answer.try_recv() {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(format!("{rel}: the GPU drew no frame to capture")),
        }
    }
}
