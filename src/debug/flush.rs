//! Turning the debug buffer into this frame's drawing, once per drawn
//! frame: 3D lines into one GPU list (depth-tested, then on top), 2D lines
//! into sprites (one batch: same texture, same draw order), labels into
//! screen text.

use glam::Vec2;

use super::{Batch, DebugLine, ON_TOP, SCREEN, THREE_D};
use crate::camera::{Rect, RenderLayers};
use crate::ctx::Ctx;
use crate::render::batch::SpriteInstance;
use crate::text::TextParams;

/// Draw order of 2D debug lines among sprites (in the world) or the UI
/// (on top): above anything a game normally draws.
const ORDER: f32 = 1.0e6;

/// One 3D line as the GPU reads it (32 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct LineInstance {
    pub a: [f32; 3],
    /// In pixels.
    pub width: f32,
    pub b: [f32; 3],
    /// sRGB, 8 bits a channel.
    pub color: [u8; 4],
}

/// This frame's 3D lines, for the renderer: the depth-tested ones, then
/// those on top. Cleared each frame, capacity kept.
#[derive(Default, Debug)]
pub(crate) struct GpuLines {
    pub tested: Vec<LineInstance>,
    pub top: Vec<LineInstance>,
}

impl GpuLines {
    /// No line this frame.
    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.tested.is_empty() && self.top.is_empty()
    }

    #[cfg_attr(not(feature = "3d"), allow(dead_code))]
    pub(crate) fn clear(&mut self) {
        self.tested.clear();
        self.top.clear();
    }

    fn push(&mut self, l: &DebugLine) {
        let line = LineInstance {
            a: l.a.to_array(),
            width: l.width,
            b: l.b.to_array(),
            color: l.color,
        };
        if l.flags & ON_TOP != 0 {
            self.top.push(line);
        } else {
            self.tested.push(line);
        }
    }
}

impl Ctx {
    /// Engine side, while drawing a frame (before the 3D frame is
    /// prepared): adds the built-in views, then hands everything drawn to
    /// the sprite lists, the text and the 3D frame.
    pub(crate) fn flush_debug_draw(&mut self) {
        #[cfg(feature = "3d")]
        self.gfx.frame3d.debug_lines.clear();
        if !self.debug.enabled() || self.gfx.renderer.is_none() {
            return;
        }
        self.hand_over_debug_draw();
    }

    /// [`flush_debug_draw`](Self::flush_debug_draw) after its checks (GPU
    /// tests call it without a window's renderer).
    pub(crate) fn hand_over_debug_draw(&mut self) {
        super::builtin::draw(self);
        #[cfg(feature = "3d")]
        let mut lines = std::mem::take(&mut self.gfx.frame3d.debug_lines);
        // Without the `3d` feature 3D lines are dropped: this never grows.
        #[cfg(not(feature = "3d"))]
        let mut lines = GpuLines::default();
        // Debug drawing is on the default layer, outside any clip the game
        // left set; both are put back afterwards.
        let (layers, clip) = (self.gfx.draw_list.layers(), self.gfx.ui_list.clip());
        self.gfx.draw_list.set_layers(RenderLayers::DEFAULT.0);
        self.gfx.ui_list.set_clip(None);
        let mut out = Out {
            gfx: &mut self.gfx,
            camera: &self.camera,
            views2d: &self.views2d,
            #[cfg(feature = "3d")]
            scene3d: &self.scene3d,
        };
        for batch in self.debug.visible() {
            out.batch(batch, &mut lines);
        }
        self.gfx.draw_list.set_layers(layers);
        self.gfx.ui_list.set_clip(clip);
        #[cfg(feature = "3d")]
        {
            self.gfx.frame3d.debug_lines = lines;
        }
    }
}

/// Where the debug buffer goes: borrowed fields of `Ctx`, beside the
/// borrowed buffer itself.
struct Out<'a> {
    gfx: &'a mut crate::gfx::Graphics,
    camera: &'a crate::d2::Camera,
    views2d: &'a [crate::d2::View],
    #[cfg(feature = "3d")]
    scene3d: &'a crate::render::SceneSettings,
}

/// The 2D cameras that show debug drawing (those on the default layer):
/// the main one, then each view's, with the window rectangle to clip to
/// (none when the main camera alone fills the window).
fn cameras2d<'a>(
    main: &'a crate::d2::Camera,
    views: &'a [crate::d2::View],
) -> impl Iterator<Item = (&'a crate::d2::Camera, Option<Rect>)> + 'a {
    let alone = views.is_empty() && main.rect.is_full();
    std::iter::once(main)
        .chain(views.iter().map(|v| &v.camera))
        .filter(|c| c.layers.intersects(RenderLayers::DEFAULT))
        .map(move |c| (c, (!alone).then(|| c.screen_rect())))
}

/// [`cameras2d`] for the 3D cameras.
#[cfg(feature = "3d")]
fn cameras3d(scene: &crate::render::SceneSettings) -> impl Iterator<Item = (&crate::d3::Camera, Option<Rect>)> + '_ {
    let alone = scene.views.is_empty() && scene.camera.rect.is_full();
    std::iter::once(&scene.camera)
        .chain(scene.views.iter().map(|v| &v.camera))
        .filter(|c| c.layers.intersects(RenderLayers::DEFAULT))
        .map(move |c| (c, (!alone).then(|| c.screen_rect())))
}

/// Where `at` is in `camera`'s picture, in window pixels; `None` when it
/// is behind the camera or nearer than its near plane.
#[cfg(feature = "3d")]
fn on_screen3d(camera: &crate::d3::Camera, at: glam::Vec3) -> Option<Vec2> {
    let clip = camera.view_proj() * at.extend(1.0);
    // Reverse-Z: in front of the near plane is z <= w.
    if clip.w <= 0.0 || clip.z > clip.w {
        return None;
    }
    camera.world_to_screen(at)
}

impl Out<'_> {
    fn batch(&mut self, batch: &Batch, lines: &mut GpuLines) {
        // Only 2D lines need the white texture (3D-only drawing makes none).
        let mut white = None;
        let zoom = self.camera.zoom.max(f32::MIN_POSITIVE);
        let (main, views) = (self.camera, self.views2d);
        for l in &batch.lines {
            if l.flags & THREE_D != 0 {
                if cfg!(feature = "3d") {
                    lines.push(l);
                }
                continue;
            }
            let white = *white.get_or_insert_with(|| self.gfx.white_texture().id);
            let (a, b) = (l.a.truncate(), l.b.truncate());
            let width = l.width.max(1.0);
            if l.flags & SCREEN != 0 {
                // Window pixels: once, over every view.
                let order = if l.flags & ON_TOP != 0 { ORDER } else { 0.0 };
                self.gfx.ui_list.push(white, order, quad(a, b, width, l.color));
            } else if l.flags & ON_TOP != 0 {
                // Over the UI, in each camera that shows it, inside its rectangle.
                for (camera, clip) in cameras2d(main, views) {
                    self.gfx.ui_list.set_clip(clip);
                    let (a, b) = (camera.world_to_screen(a), camera.world_to_screen(b));
                    self.gfx.ui_list.push(white, ORDER, quad(a, b, width, l.color));
                }
                self.gfx.ui_list.set_clip(None);
            } else {
                // World units, on the default layer: every view showing it
                // draws it. As wide as `width` pixels at the main camera's zoom.
                self.gfx.draw_list.push(white, ORDER, quad(a, b, width / zoom, l.color));
            }
        }
        for label in &batch.labels {
            let text = &batch.text[label.text.clone()];
            let params = TextParams {
                size: label.size,
                color: label.color,
                z: ORDER + 1.0,
                ..Default::default()
            };
            if label.flags & THREE_D != 0 {
                #[cfg(feature = "3d")]
                for (camera, clip) in cameras3d(self.scene3d) {
                    if let Some(at) = on_screen3d(camera, label.at) {
                        self.gfx.ui_list.set_clip(clip);
                        self.gfx.draw_text_ex(text, at, params);
                    }
                }
            } else if label.flags & SCREEN != 0 {
                self.gfx.draw_text_ex(text, label.at.truncate(), params);
            } else {
                for (camera, clip) in cameras2d(main, views) {
                    self.gfx.ui_list.set_clip(clip);
                    self.gfx
                        .draw_text_ex(text, camera.world_to_screen(label.at.truncate()), params);
                }
            }
            self.gfx.ui_list.set_clip(None);
        }
    }
}

/// A sprite stretched from `a` to `b`, `width` thick.
fn quad(a: Vec2, b: Vec2, width: f32, color: [u8; 4]) -> SpriteInstance {
    let d = b - a;
    SpriteInstance::new(
        a.into(),
        [d.length(), width],
        [0.0, 0.5],
        if d == Vec2::ZERO { 0.0 } else { d.to_angle() },
        color,
    )
}

impl Ctx {
    /// One frame of debug drawing without a GPU, for `ponko_bench`: starts
    /// a frame, lets `draw` draw, hands everything over as a drawn frame
    /// does (3D lines to the GPU list, 2D lines to sprites, which are then
    /// ordered and batched as the renderer does), and forgets the sprites.
    /// Returns the 3D lines handed over, the sprites and their batches.
    /// Unstable (`bench-internals`).
    #[doc(hidden)]
    #[cfg(feature = "bench-internals")]
    pub fn bench_debug_frame(&mut self, draw: impl FnOnce(&mut super::DebugDraw)) -> (usize, usize, usize) {
        self.debug.begin_frame(self.time.elapsed());
        draw(&mut self.debug);
        #[cfg(feature = "3d")]
        self.gfx.frame3d.debug_lines.clear();
        if self.debug.enabled() {
            self.hand_over_debug_draw();
        }
        #[cfg(feature = "3d")]
        let lines = self.gfx.frame3d.debug_lines.tested.len() + self.gfx.frame3d.debug_lines.top.len();
        #[cfg(not(feature = "3d"))]
        let lines = 0;
        let (sprites, batches) = self.gfx.draw_list.finish();
        let counts = (lines, sprites.len(), batches.len());
        self.gfx.draw_list.clear();
        self.gfx.ui_list.clear();
        counts
    }
}
