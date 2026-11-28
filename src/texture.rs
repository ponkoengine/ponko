//! [`Texture`] handles and [`DrawParams`].
//!
//! **Maturity: proven**, from the guide chapter `drawing.md`. Game: Coin Dash and Ember Hop,
//! packaged and played by the list of checks (recorded on macOS). Tests: the 2D render tests
//! (`draw_order_render_test.rs`, `views2d_render_test.rs`, `atlas_render_test.rs`, ...). Platforms:
//! Linux, macOS and Windows, headless (the workspace tests in the list of checks). Graphics: the
//! packaged games run in a window on Linux (software Vulkan under Xvfb), macOS and Windows (DirectX
//! 12, WARP when there is no GPU); Coin Dash also runs in headless Chromium and WebKit on WebGL2;
//! offscreen render tests, required on Windows (DirectX 12 on the WARP software adapter) and run on
//! Linux and macOS only where a GPU is found.

#![deny(missing_docs)]

use glam::Vec2;

use crate::color::Color;

/// A loaded image on the GPU. Cheap to copy: it's just an id plus its size.
///
/// Get one from [`Ctx::load_texture`](crate::ctx::Ctx::load_texture) or
/// [`Graphics::create_texture`](crate::gfx::Graphics::create_texture).
///
/// ```
/// # use ponko::prelude::*;
/// # fn setup(ctx: &mut Ctx) {
/// let crab = ctx.gfx.load_texture("crab.png");
/// println!("{} x {}", crab.width(), crab.height());
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(C)] // shared with C/C++ (`PonkoTexture`)
pub struct Texture {
    pub(crate) id: u32,
    width: u32,
    height: u32,
}

impl Texture {
    pub(crate) const fn new(id: u32, width: u32, height: u32) -> Self {
        Self { id, width, height }
    }

    /// Width in pixels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(tex: Texture) {
    /// let w = tex.width();
    /// # }
    /// ```
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(tex: Texture) {
    /// let h = tex.height();
    /// # }
    /// ```
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Size in pixels as a vector.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(tex: Texture) {
    /// let half = tex.size() / 2.0;
    /// # }
    /// ```
    #[must_use]
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.width as f32, self.height as f32)
    }
}

/// How a texture looks when drawn bigger or smaller than its pixels:
/// [`Graphics::set_texture_filter`](crate::gfx::Graphics::set_texture_filter).
///
/// ```
/// # use ponko::prelude::*;
/// assert_eq!(TextureFilter::default(), TextureFilter::Linear);
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let tiles = ctx.gfx.create_texture(1, 1, &[255; 4]);
///     ctx.gfx.set_texture_filter(tiles, TextureFilter::Nearest); // crisp pixel art
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureFilter {
    /// Smooth blending between pixels (default): photos, painted art.
    #[default]
    Linear,
    /// Hard pixel edges: pixel art and tiles (no seams between tiles).
    Nearest,
}

use crate::draw_space::DrawSpace;

/// Optional settings for [`Graphics::draw_ex`](crate::gfx::Graphics::draw_ex).
/// Start from `DrawParams::default()` and change what you need.
///
/// ```
/// # use ponko::prelude::*;
/// # fn update(ctx: &mut Ctx, tex: Texture) {
/// ctx.gfx.draw_ex(
///     tex,
///     vec2(100.0, 100.0),
///     DrawParams::default().with_rotation(0.5).with_scale(vec2(2.0, 2.0)).with_color(Color::RED),
/// );
/// # }
/// ```
///
/// It is `#[non_exhaustive]`, so later versions can add settings: start
/// from `DrawParams::default()` and chain the `with_*` methods (or set the
/// public fields on a mutable value).
///
/// # Defaults and cost
///
/// [`Default`] draws the whole texture at its own size, unrotated, untinted,
/// at `z` 0, pivoted on its center, in world units through the camera, with
/// alpha blending and the built-in look. Whatever the settings, a sprite
/// uploads 36 bytes per frame. Consecutive sprites (after sorting by `z`)
/// with the same texture, clip, blend mode and material are one instanced
/// draw call, so alternating blend modes or materials between sprites ends a
/// batch and costs another draw call; rotation, scale, tint and pivot are free.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DrawParams {
    /// Size multiplier. Negative values flip the sprite. Default `(1, 1)`.
    pub scale: Vec2,
    /// Rotation in radians, clockwise on screen. Default `0`.
    pub rotation: f32,
    /// Tint multiplied with the texture. Default white (no tint).
    pub color: Color,
    /// Draw order: higher `z` is drawn on top. Sprites with equal `z` are
    /// drawn in the order you call `draw`. Default `0`.
    pub z: f32,
    /// Which point of the sprite sits at the draw position and is rotated
    /// around: `(0, 0)` top-left, `(0.5, 0.5)` center (default), `(1, 1)` bottom-right.
    pub pivot: Vec2,
    /// Draw in window pixels, ignoring the camera, on top of the world:
    /// for UI and HUDs. Default `false` (world units, through the camera).
    pub screen: bool,
    /// The part of the texture to draw, in texture pixels (a frame of a
    /// [`SpriteSheet`](crate::sheet::SpriteSheet)). Default `None`: all of it.
    pub region: Option<crate::camera::Rect>,
    /// How it combines with what's behind it. Default [`BlendMode::Alpha`](crate::render::BlendMode::Alpha).
    pub blend: crate::render::BlendMode,
    /// A custom look ([`SpriteMaterial`](crate::render::SpriteMaterial)).
    /// Default `None`: the built-in look, texture × tint.
    pub material: Option<crate::render::SpriteMaterial>,
}

impl Default for DrawParams {
    fn default() -> Self {
        Self {
            scale: Vec2::ONE,
            rotation: 0.0,
            color: Color::WHITE,
            z: 0.0,
            pivot: Vec2::splat(0.5),
            screen: false,
            region: None,
            blend: crate::render::BlendMode::Alpha,
            material: None,
        }
    }
}

impl DrawParams {
    /// Same settings with size multiplier `scale` (negative flips).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let mirrored = DrawParams::default().with_scale(vec2(-1.0, 1.0));
    /// assert_eq!(mirrored.scale, vec2(-1.0, 1.0));
    /// ```
    #[must_use]
    pub fn with_scale(self, scale: Vec2) -> Self {
        Self { scale, ..self }
    }
    /// Same settings rotated `rotation` radians, clockwise on screen.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let quarter_turn = DrawParams::default().with_rotation(std::f32::consts::FRAC_PI_2);
    /// assert_eq!(quarter_turn.rotation, std::f32::consts::FRAC_PI_2);
    /// ```
    #[must_use]
    pub fn with_rotation(self, rotation: f32) -> Self {
        Self { rotation, ..self }
    }
    /// Same settings with tint `color`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let hurt = DrawParams::default().with_color(Color::RED);
    /// assert_eq!(hurt.color, Color::RED);
    /// ```
    #[must_use]
    pub fn with_color(self, color: Color) -> Self {
        Self { color, ..self }
    }
    /// Same settings with draw order `z` (higher is on top).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(DrawParams::default().with_z(5.0).z > DrawParams::default().z);
    /// ```
    #[must_use]
    pub fn with_z(self, z: f32) -> Self {
        Self { z, ..self }
    }
    /// Same settings with a different pivot (`(0, 0)` = top-left corner).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// // Rotate a door around its hinge instead of its middle.
    /// let door = DrawParams::default().with_pivot(vec2(0.0, 0.5)).with_rotation(0.3);
    /// assert_eq!(door.pivot, vec2(0.0, 0.5));
    /// ```
    #[must_use]
    pub fn with_pivot(self, pivot: Vec2) -> Self {
        Self { pivot, ..self }
    }
    /// Same settings drawn in window pixels (`true`, for UI and HUDs) or in
    /// world units through the camera (`false`).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let hud = DrawParams::default().with_screen(true);
    /// assert!(hud.screen && !DrawParams::default().screen);
    /// ```
    #[must_use]
    #[deprecated(note = "use with_space(DrawSpace::World) or with_space(DrawSpace::Screen)")]
    pub fn with_screen(self, screen: bool) -> Self {
        Self { screen, ..self }
    }
    /// Selects world units through the camera or window pixels for a HUD.
    #[must_use]
    pub fn with_space(self, space: DrawSpace) -> Self {
        Self {
            screen: matches!(space, DrawSpace::Screen),
            ..self
        }
    }
    /// Same settings drawing only `region` of the texture (texture pixels).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let p = DrawParams::default().with_region(Rect::new(16.0, 0.0, 16.0, 16.0)).with_z(2.0);
    /// assert_eq!(p.region, Some(Rect::new(16.0, 0.0, 16.0, 16.0)));
    /// assert_eq!((p.z, p.scale), (2.0, Vec2::ONE));
    /// ```
    #[must_use]
    pub fn with_region(self, region: crate::camera::Rect) -> Self {
        Self {
            region: Some(region),
            ..self
        }
    }
    /// Same settings with blend mode `blend`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # use ponko::render::BlendMode;
    /// # fn update(ctx: &mut Ctx, spark: Texture) {
    /// ctx.gfx.draw_ex(spark, vec2(50.0, 50.0), DrawParams::default().with_blend(BlendMode::Additive));
    /// # }
    /// ```
    #[must_use]
    pub fn with_blend(self, blend: crate::render::BlendMode) -> Self {
        Self { blend, ..self }
    }
    /// Same settings drawn with sprite material `material`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, glow: ponko::render::SpriteMaterial) {
    /// let params = DrawParams::default().with_material(glow);
    /// assert_eq!(params.material, Some(glow));
    /// # }
    /// ```
    #[must_use]
    pub fn with_material(self, material: crate::render::SpriteMaterial) -> Self {
        Self {
            material: Some(material),
            ..self
        }
    }
}
