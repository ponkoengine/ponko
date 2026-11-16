//! The 2D camera, public as [`d2::Camera`](crate::d2::Camera).

#![deny(missing_docs)]

use glam::{Mat4, Vec2, vec4};

use crate::camera::{Rect, RenderLayers, Viewport};

/// Which part of the world the window shows: [`ctx.camera`](crate::ctx::Ctx::camera).
///
/// Sprites and [`Ctx::draw`](crate::ctx::Ctx::draw) are in world units; the camera
/// maps them to the window. The default camera shows world point `(0, 0)` at
/// the top-left at zoom 1, so world units are window pixels until you move
/// it. UI drawn with [`DrawParams::screen`](crate::texture::DrawParams::screen) or
/// [`Ctx::draw_text`](crate::ctx::Ctx::draw_text) ignores the camera.
///
/// ```
/// # use ponko::prelude::*;
/// # fn follow(ctx: &mut Ctx, player: Vec2) {
/// ctx.camera.look_at(player); // keep the player in the middle
/// let under_mouse = ctx.camera.screen_to_world(ctx.input.mouse_pos());
/// # let _ = under_mouse;
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera2D {
    /// The world point at the top-left corner of the part of the window
    /// it draws into ([`rect`](Self::rect)): the window's corner by default.
    pub position: Vec2,
    /// Window pixels per world unit: 2 shows everything twice as big.
    pub zoom: f32,
    /// The part of the window it draws into: the whole window by default.
    /// Split screen and minimaps give each camera its own (see
    /// [`d2::View`](crate::d2::View)); its conversions, [`viewport`](Self::viewport)
    /// and [`visible_rect`](Self::visible_rect) follow it at once.
    pub rect: Viewport,
    /// Which [`RenderLayers`] it draws: all by default. Sprites drawn on
    /// other layers (see [`Graphics::set_layers`](crate::gfx::Graphics::set_layers))
    /// don't show through it.
    pub layers: RenderLayers,
    /// The window's size in logical pixels, kept up to date by the engine;
    /// its part of it comes from `rect`.
    window: Vec2,
}

impl Camera2D {
    /// Smallest and largest zoom that [`zoom_at`](Self::zoom_at) allows.
    pub const ZOOM_LIMITS: (f32, f32) = (1.0 / 64.0, 64.0);

    pub(crate) fn new(viewport: Vec2) -> Self {
        Self {
            position: Vec2::ZERO,
            zoom: 1.0,
            rect: Viewport::FULL,
            layers: RenderLayers::ALL,
            window: viewport,
        }
    }

    /// Learns the window's size (its part of it comes from its
    /// [`rect`](Self::rect)).
    pub(crate) fn place(&mut self, window: Vec2) {
        self.window = window;
    }

    /// Its part of the window's top-left corner, in logical pixels.
    fn origin(&self) -> Vec2 {
        self.rect.pixels(self.window).min
    }

    /// The part of the window it draws into, in logical pixels (the whole
    /// window unless its [`rect`](Self::rect) says otherwise).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("Duel", 800, 600).run_headless(1, |ctx| {
    ///     assert_eq!(ctx.camera.screen_rect(), Rect::new(0.0, 0.0, 800.0, 600.0));
    /// });
    /// ```
    #[must_use]
    pub fn screen_rect(&self) -> Rect {
        self.rect.pixels(self.window)
    }

    /// The size in pixels of the part of the window it draws into: the
    /// window's size unless its [`rect`](Self::rect) says otherwise.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// let size = ctx.camera.viewport();
    /// # let _ = size;
    /// # }
    /// ```
    #[must_use]
    pub fn viewport(&self) -> Vec2 {
        self.rect.pixels(self.window).size()
    }

    /// The world point under window pixel `screen`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn place(ctx: &mut Ctx, flag: Texture) {
    /// if ctx.input.mouse_pressed(MouseButton::Left) {
    ///     let at = ctx.camera.screen_to_world(ctx.input.mouse_pos());
    ///     ctx.world.spawn((ponko::d2::Transform::at(at), ponko::d2::Sprite::new(flag)));
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        self.position + (screen - self.origin()) / self.zoom
    }

    /// The window pixel where world point `world` appears.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn label(ctx: &mut Ctx, unit_at: Vec2) {
    /// let above = ctx.camera.world_to_screen(unit_at) - vec2(0.0, 20.0);
    /// ctx.gfx.draw_text("Worker", above, 14.0, Color::WHITE);
    /// # }
    /// ```
    #[must_use]
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        self.origin() + (world - self.position) * self.zoom
    }

    /// The world point in the middle of the window.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx) {
    /// let middle = ctx.camera.center();
    /// # let _ = middle;
    /// # }
    /// ```
    #[must_use]
    pub fn center(&self) -> Vec2 {
        self.position + self.viewport() / (2.0 * self.zoom)
    }

    /// Moves the camera so `world` is in the middle of the window. Call it
    /// every frame to follow something.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn follow(ctx: &mut Ctx, hero: Vec2) {
    /// ctx.camera.look_at(hero);
    /// # }
    /// ```
    pub fn look_at(&mut self, world: Vec2) {
        self.position = world - self.viewport() / (2.0 * self.zoom);
    }

    /// Multiplies the zoom by `factor`, keeping the world point under window
    /// pixel `screen` where it is (zoom toward the mouse). The zoom stays
    /// within [`ZOOM_LIMITS`](Self::ZOOM_LIMITS).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn wheel_zoom(ctx: &mut Ctx) {
    /// let wheel = ctx.input.mouse_wheel().y;
    /// if wheel != 0.0 {
    ///     let mouse = ctx.input.mouse_pos();
    ///     ctx.camera.zoom_at(mouse, 1.1f32.powf(wheel));
    /// }
    /// # }
    /// ```
    pub fn zoom_at(&mut self, screen: Vec2, factor: f32) {
        let anchor = self.screen_to_world(screen);
        let (lo, hi) = Self::ZOOM_LIMITS;
        self.zoom = (self.zoom * factor).clamp(lo, hi);
        self.position = anchor - (screen - self.origin()) / self.zoom;
    }

    /// The part of the world the window shows.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &Ctx, p: Vec2) {
    /// let on_screen = ctx.camera.visible_rect().contains(p);
    /// # }
    /// ```
    #[must_use]
    pub fn visible_rect(&self) -> Rect {
        Rect {
            min: self.position,
            max: self.position + self.viewport() / self.zoom,
        }
    }

    /// World → clip space for the GPU.
    pub(crate) fn view_proj(&self) -> Mat4 {
        let size = self.viewport().max(Vec2::ONE);
        let (sx, sy) = (2.0 * self.zoom / size.x, -2.0 * self.zoom / size.y);
        Mat4::from_cols(
            vec4(sx, 0.0, 0.0, 0.0),
            vec4(0.0, sy, 0.0, 0.0),
            vec4(0.0, 0.0, 1.0, 0.0),
            vec4(-1.0 - self.position.x * sx, 1.0 - self.position.y * sy, 0.0, 1.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> Camera2D {
        Camera2D::new(Vec2::new(800.0, 600.0))
    }

    #[test]
    fn the_default_camera_maps_world_to_window_pixels() {
        let c = cam();
        let clip = |p: Vec2| c.view_proj().project_point3(p.extend(0.0)).truncate();
        assert!(clip(Vec2::ZERO).abs_diff_eq(Vec2::new(-1.0, 1.0), 1e-6));
        assert!(clip(Vec2::new(800.0, 600.0)).abs_diff_eq(Vec2::new(1.0, -1.0), 1e-6));
        assert_eq!(c.screen_to_world(Vec2::new(10.0, 20.0)), Vec2::new(10.0, 20.0));
    }

    #[test]
    fn a_camera_in_part_of_the_window_converts_from_that_part() {
        let mut c = cam();
        c.rect = Viewport::split(2, 1);
        c.place(Vec2::new(800.0, 600.0));
        assert_eq!(c.viewport(), Vec2::new(400.0, 600.0));
        assert_eq!(c.screen_rect(), Rect::new(400.0, 0.0, 400.0, 600.0));
        c.position = Vec2::new(10.0, 20.0);
        c.zoom = 2.0;
        // The right half's top-left corner shows the camera's position.
        assert_eq!(c.screen_to_world(Vec2::new(400.0, 0.0)), Vec2::new(10.0, 20.0));
        assert_eq!(c.world_to_screen(Vec2::new(10.0, 20.0)), Vec2::new(400.0, 0.0));
        let w = Vec2::new(-30.0, 77.0);
        assert!(c.screen_to_world(c.world_to_screen(w)).abs_diff_eq(w, 1e-4));
        let before = c.screen_to_world(Vec2::new(500.0, 100.0));
        c.zoom_at(Vec2::new(500.0, 100.0), 1.5);
        assert!(c.screen_to_world(Vec2::new(500.0, 100.0)).abs_diff_eq(before, 1e-3));
    }

    #[test]
    fn view_proj_agrees_with_world_to_screen() {
        let mut c = cam();
        c.position = Vec2::new(-300.0, 1234.5);
        c.zoom = 2.5;
        for w in [Vec2::new(0.0, 0.0), Vec2::new(-250.0, 1300.0), Vec2::new(17.0, -9.0)] {
            let clip = c.view_proj().project_point3(w.extend(0.0)).truncate();
            let screen = (clip * Vec2::new(0.5, -0.5) + 0.5) * c.viewport();
            assert!(screen.abs_diff_eq(c.world_to_screen(w), 1e-2), "{w}");
            assert!(c.screen_to_world(c.world_to_screen(w)).abs_diff_eq(w, 1e-3));
        }
    }

    #[test]
    fn zoom_at_keeps_the_point_under_the_cursor() {
        let mut c = cam();
        c.position = Vec2::new(100.0, 50.0);
        let mouse = Vec2::new(200.0, 150.0);
        let before = c.screen_to_world(mouse);
        c.zoom_at(mouse, 3.0);
        assert_eq!(c.zoom, 3.0);
        assert!(c.screen_to_world(mouse).abs_diff_eq(before, 1e-4));
        c.zoom_at(mouse, 1e9);
        assert_eq!(c.zoom, Camera2D::ZOOM_LIMITS.1);
    }

    #[test]
    fn look_at_centers_the_point() {
        let mut c = cam();
        c.zoom = 4.0;
        c.look_at(Vec2::new(5000.0, -20.0));
        assert!(c.center().abs_diff_eq(Vec2::new(5000.0, -20.0), 1e-3));
        let r = c.visible_rect();
        assert!(r.size().abs_diff_eq(Vec2::new(200.0, 150.0), 1e-3));
        assert!(r.contains(Vec2::new(5000.0, -20.0)));
    }
}
