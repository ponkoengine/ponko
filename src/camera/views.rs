//! Several cameras in one window: [`Viewport`] (the part of the window a
//! camera draws into), [`RenderLayers`] (what it draws) and the 2D view
//! list's entry, [`d2::View`](crate::d2::View). The 3D one is
//! `d3::View` (feature `3d`).

use glam::Vec2;
use ponko_ecs::Component;

use super::Rect;
use crate::color::Color;

/// The part of the window a camera draws into, as fractions of the
/// window's size: `x` and `y` from its top-left corner, then `width` and
/// `height`. [`Viewport::FULL`] (the default) is the whole window.
///
/// Fractions keep a layout when the window is resized: a left half stays a
/// left half. Each camera has one ([`d2::Camera::rect`](crate::d2::Camera::rect),
/// `d3::Camera::rect`), and its screen and world conversions, picking and
/// aspect ratio follow it. Holding one costs nothing.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::camera::Viewport;
/// // Two players side by side, and a minimap in the top-right corner.
/// let left = Viewport::split(2, 0);
/// let right = Viewport::split(2, 1);
/// let minimap = Viewport::new(0.75, 0.0, 0.25, 0.25);
/// assert_eq!(left.pixels(vec2(1280.0, 720.0)), Rect::new(0.0, 0.0, 640.0, 720.0));
/// assert!(right.contains(vec2(1280.0, 720.0), vec2(1000.0, 10.0)));
/// # let _ = minimap;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[repr(C)] // shared with C/C++ (`PonkoViewport`)
pub struct Viewport {
    /// The left edge, from 0 (the window's left) to 1 (its right).
    pub x: f32,
    /// The top edge, from 0 (the window's top) to 1 (its bottom).
    pub y: f32,
    /// The width, as a fraction of the window's.
    pub width: f32,
    /// The height, as a fraction of the window's.
    pub height: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::FULL
    }
}

impl Viewport {
    /// The whole window.
    pub const FULL: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    };

    /// The rectangle at `x`, `y` (fractions of the window from its
    /// top-left corner) of `width` x `height` (fractions of its size).
    ///
    /// ```
    /// # use ponko::camera::Viewport;
    /// let bottom_half = Viewport::new(0.0, 0.5, 1.0, 0.5);
    /// # let _ = bottom_half;
    /// ```
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Player `index`'s part of a split screen for `players` players: the
    /// whole window for one, the left and right halves for two, the
    /// window's quarters for three and four (left to right, top to bottom;
    /// with three the bottom-right one stays free for a map or a score),
    /// and equal columns for more. `index` past the last player gives the
    /// last one's part.
    ///
    /// ```
    /// # use ponko::camera::Viewport;
    /// assert_eq!(Viewport::split(2, 1), Viewport::new(0.5, 0.0, 0.5, 1.0));
    /// assert_eq!(Viewport::split(4, 2), Viewport::new(0.0, 0.5, 0.5, 0.5));
    /// assert_eq!(Viewport::split(1, 0), Viewport::FULL);
    /// ```
    #[must_use]
    pub fn split(players: u32, index: u32) -> Self {
        let players = players.max(1);
        let index = index.min(players - 1);
        match players {
            1 => Self::FULL,
            2 => Self::new(index as f32 * 0.5, 0.0, 0.5, 1.0),
            3 | 4 => Self::new((index % 2) as f32 * 0.5, (index / 2) as f32 * 0.5, 0.5, 0.5),
            n => {
                let w = 1.0 / n as f32;
                Self::new(index as f32 * w, 0.0, w, 1.0)
            }
        }
    }

    /// The rectangle in a window of `window` logical pixels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::camera::Viewport;
    /// let r = Viewport::new(0.5, 0.0, 0.5, 0.5).pixels(vec2(800.0, 600.0));
    /// assert_eq!(r, Rect::new(400.0, 0.0, 400.0, 300.0));
    /// ```
    #[must_use]
    pub fn pixels(&self, window: Vec2) -> Rect {
        let min = Vec2::new(self.x, self.y) * window;
        Rect {
            min,
            max: min + Vec2::new(self.width, self.height).max(Vec2::ZERO) * window,
        }
    }

    /// Whether window point `point` (logical pixels, as
    /// [`Input::mouse_pos`](crate::input::Input::mouse_pos)) is inside, in a
    /// window of `window` logical pixels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::camera::Viewport;
    /// assert!(!Viewport::split(2, 0).contains(vec2(800.0, 600.0), vec2(500.0, 10.0)));
    /// ```
    #[must_use]
    pub fn contains(&self, window: Vec2, point: Vec2) -> bool {
        let r = self.pixels(window);
        point.cmpge(r.min).all() && point.cmplt(r.max).all()
    }

    /// Whether it covers the whole window (to the nearest pixel of a
    /// 16k-wide one).
    pub(crate) fn is_full(&self) -> bool {
        let close = |a: f32, b: f32| (a - b).abs() < 1.0 / 16384.0;
        close(self.x, 0.0) && close(self.y, 0.0) && close(self.width, 1.0) && close(self.height, 1.0)
    }

    /// The rectangle in physical pixels of a `size` window, snapped to whole
    /// pixels and kept inside it: `[x, y, width, height]`, at least 1 x 1
    /// (a viewport that covers less than a pixel still draws one).
    pub(crate) fn physical(&self, size: [u32; 2]) -> [u32; 4] {
        let [w, h] = [size[0].max(1) as f32, size[1].max(1) as f32];
        // A broken (NaN or infinite) number: the window's edge.
        let or = |v: f32, default: f32| if v.is_finite() { v } else { default };
        let (x, y) = (or(self.x, 0.0), or(self.y, 0.0));
        let (right, bottom) = (or(x + self.width, 1.0), or(y + self.height, 1.0));
        let x0 = (x * w).round().clamp(0.0, w - 1.0);
        let y0 = (y * h).round().clamp(0.0, h - 1.0);
        let x1 = (right * w).round().clamp(0.0, w).max(x0 + 1.0);
        let y1 = (bottom * h).round().clamp(0.0, h).max(y0 + 1.0);
        [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32]
    }
}

/// Which views draw something: one bit per layer, 32 layers. As a
/// component on an entity it says which layers the entity is on; as a
/// camera's [`layers`](crate::d2::Camera::layers) it says which layers that
/// camera draws. A camera draws what shares at least one layer with it.
///
/// An entity without it is on [`RenderLayers::DEFAULT`] (layer 0), and a
/// camera draws [`RenderLayers::ALL`] unless told otherwise, so nothing
/// changes until a game uses layers. Typical uses: map icons only a
/// minimap camera draws (put them on layer 1 and leave layer 1 out of the
/// players' cameras), or a first-person body only the other players see.
///
/// In 3D it applies to `Model`s (plain, skinned
/// and in a `LodGroup`): a model a camera doesn't
/// draw still casts its shadow there. 2D [`Sprite`](crate::d2::Sprite)
/// entities take it too, and sprites drawn with calls take theirs from
/// [`Graphics::set_layers`](crate::gfx::Graphics::set_layers). Checking it
/// costs one AND per model or sprite batch and camera.
///
/// In scene files it's `"RenderLayers": 2` (the bits).
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::camera::RenderLayers;
/// let icons = RenderLayers::layer(1);
/// let players = RenderLayers::ALL.without(1);
/// assert!(!players.intersects(icons));
/// assert!(players.intersects(RenderLayers::DEFAULT));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Component)]
#[repr(transparent)]
pub struct RenderLayers(pub u32);

impl Default for RenderLayers {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl RenderLayers {
    /// Layer 0 only: what an entity without layers is on.
    pub const DEFAULT: Self = Self(1);
    /// Every layer: what a camera draws by default.
    pub const ALL: Self = Self(u32::MAX);
    /// No layer: a camera that draws nothing of the world (its UI still shows).
    pub const NONE: Self = Self(0);

    /// Layer `n` alone (0 to 31; larger numbers wrap).
    ///
    /// ```
    /// # use ponko::camera::RenderLayers;
    /// assert_eq!(RenderLayers::layer(3).0, 0b1000);
    /// ```
    #[must_use]
    pub const fn layer(n: u32) -> Self {
        Self(1 << (n % 32))
    }

    /// These layers and layer `n`.
    ///
    /// ```
    /// # use ponko::camera::RenderLayers;
    /// let both = RenderLayers::DEFAULT.with(1);
    /// assert_eq!(both.0, 0b11);
    /// ```
    #[must_use]
    pub const fn with(self, n: u32) -> Self {
        Self(self.0 | Self::layer(n).0)
    }

    /// These layers without layer `n`.
    ///
    /// ```
    /// # use ponko::camera::RenderLayers;
    /// assert_eq!(RenderLayers::ALL.without(0).0, u32::MAX - 1);
    /// ```
    #[must_use]
    pub const fn without(self, n: u32) -> Self {
        Self(self.0 & !Self::layer(n).0)
    }

    /// Whether the two share a layer: a camera with `self` draws an entity
    /// on `other`.
    ///
    /// ```
    /// # use ponko::camera::RenderLayers;
    /// assert!(RenderLayers::ALL.intersects(RenderLayers::layer(7)));
    /// assert!(!RenderLayers::NONE.intersects(RenderLayers::ALL));
    /// ```
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// What the renderer draws one 2D camera with: its matrix, its part of the
/// window, its layers and its clear color.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(feature = "window"), allow(dead_code))]
pub(crate) struct Camera2DFrame {
    pub view_proj: glam::Mat4,
    pub rect: Viewport,
    pub layers: u32,
    pub clear: Option<Color>,
}

impl Camera2DFrame {
    pub(crate) fn of(camera: &crate::camera2d::Camera2D, clear: Option<Color>) -> Self {
        Self {
            view_proj: camera.view_proj(),
            rect: camera.rect,
            layers: camera.layers.0,
            clear,
        }
    }
}

/// One more 2D camera, drawing the world into its part of the window: an
/// entry of [`Ctx::views2d`](crate::ctx::Ctx::views2d). Split screen, a
/// minimap, a picture in picture.
///
/// The main camera ([`Ctx::camera`](crate::ctx::Ctx::camera)) draws first,
/// into its own [`rect`](crate::d2::Camera::rect); then each view, in the
/// list's order, on top. Each draws the world's sprites (this frame's draw
/// calls and [`Sprite`](crate::d2::Sprite)s) through its camera, clipped to
/// its rectangle, and only those on its camera's
/// [`layers`](crate::d2::Camera::layers). Screen-space drawing (text, UI)
/// is drawn once, over the whole window, after every view: a HUD per view
/// goes where its camera's
/// [`screen_rect`](crate::d2::Camera::screen_rect) is.
///
/// Cost: one more pass over the frame's world sprite batches per view (the
/// sprites are uploaded once), and a rectangle fill when it clears. 2D
/// lights light the world only while one camera draws it all (the main
/// camera fills the window on every layer and there are no views); with
/// views, sprites draw unlit.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::camera::Viewport;
/// Game::new("Duel", 800, 400).run_headless(1, |ctx| {
///     ctx.camera.rect = Viewport::split(2, 0);
///     let mut second = ctx.camera;
///     second.rect = Viewport::split(2, 1);
///     ctx.views2d.push(d2::View::new(second));
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View2D {
    /// What it shows, where in the window ([`rect`](crate::d2::Camera::rect))
    /// and which [`layers`](crate::d2::Camera::layers). The engine keeps its
    /// size in pixels up to date.
    pub camera: crate::camera2d::Camera2D,
    /// Fills its rectangle with this color before drawing. `None` (the
    /// default) draws over whatever is there: the 3D frame, or the views
    /// before it.
    pub clear: Option<Color>,
}

impl View2D {
    /// A view through `camera`, into the camera's
    /// [`rect`](crate::d2::Camera::rect), not cleared.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let mut map = ctx.camera;
    /// map.rect = ponko::camera::Viewport::new(0.75, 0.0, 0.25, 0.25);
    /// map.zoom = 0.1;
    /// ctx.views2d.push(d2::View::new(map).with_clear(Color::BLACK));
    /// # }
    /// ```
    #[must_use]
    pub fn new(camera: crate::camera2d::Camera2D) -> Self {
        Self { camera, clear: None }
    }

    /// The same view, filling its rectangle with `color` first.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let view = d2::View::new(ctx.camera).with_clear(Color::hex(0x102030));
    /// assert_eq!(view.clear, Some(Color::hex(0x102030)));
    /// # }
    /// ```
    #[must_use]
    pub fn with_clear(mut self, color: Color) -> Self {
        self.clear = Some(color);
        self
    }

    /// The 2D camera that draws the window at `screen` (logical pixels, as
    /// [`mouse_pos`](crate::input::Input::mouse_pos)), for picking: the
    /// last of `views` whose rectangle holds it (they draw on top), else
    /// `main`, also outside every rectangle. Its
    /// [`screen_to_world`](crate::d2::Camera::screen_to_world) takes window
    /// coordinates. [`Ctx::mouse_world`](crate::ctx::Ctx::mouse_world) uses
    /// it for the mouse; the 3D one is `ctx.scene3d.camera_at`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn update(ctx: &mut Ctx) {
    /// if ctx.input.mouse_pressed(MouseButton::Left) {
    ///     let click = ctx.input.mouse_pos();
    ///     let camera = d2::View::camera_at(&ctx.camera, &ctx.views2d, click);
    ///     println!("clicked the world at {}", camera.screen_to_world(click));
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn camera_at<'a>(
        main: &'a crate::camera2d::Camera2D,
        views: &'a [View2D],
        screen: Vec2,
    ) -> &'a crate::camera2d::Camera2D {
        views
            .iter()
            .rev()
            .map(|v| &v.camera)
            .find(|c| c.screen_rect().contains(screen))
            .unwrap_or(main)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_screens_tile_the_window_without_gaps() {
        for players in 1..=4 {
            let area: f32 = (0..players)
                .map(|i| {
                    let v = Viewport::split(players, i);
                    v.width * v.height
                })
                .sum();
            let free = if players == 3 { 0.25 } else { 0.0 };
            assert!((area + free - 1.0).abs() < 1e-6, "{players} players cover {area}");
        }
        assert_eq!(Viewport::split(3, 9), Viewport::split(3, 2), "past the last player");
        assert_eq!(Viewport::split(0, 0), Viewport::FULL);
    }

    #[test]
    fn physical_rectangles_stay_inside_the_window_and_are_never_empty() {
        assert_eq!(Viewport::FULL.physical([640, 480]), [0, 0, 640, 480]);
        assert_eq!(Viewport::split(2, 1).physical([641, 480]), [321, 0, 320, 480]);
        assert_eq!(Viewport::new(0.5, 0.5, 2.0, 2.0).physical([100, 100]), [50, 50, 50, 50]);
        assert_eq!(Viewport::new(1.0, 1.0, 0.0, 0.0).physical([100, 100]), [99, 99, 1, 1]);
        assert_eq!(
            Viewport::new(f32::NAN, 0.0, 1.0, 1.0).physical([10, 10]),
            [0, 0, 10, 10]
        );
        assert_eq!(
            Viewport::new(0.5, 0.0, f32::INFINITY, 1.0).physical([10, 10]),
            [5, 0, 5, 10]
        );
        assert!(Viewport::FULL.is_full());
        assert!(!Viewport::split(2, 0).is_full());
    }

    #[test]
    fn neighbouring_viewports_share_no_point() {
        let window = Vec2::new(800.0, 600.0);
        let edge = Vec2::new(400.0, 300.0);
        assert!(!Viewport::split(2, 0).contains(window, edge));
        assert!(Viewport::split(2, 1).contains(window, edge));
    }
}
