//! The 3D view list's entry, [`d3::View`](crate::d3::View).

use crate::color::Color;
use crate::three::{Camera3D, Post};

/// One more 3D camera, drawing the world into its part of the window: an
/// entry of [`SceneSettings::views`](crate::render::SceneSettings::views).
/// Split screen, a minimap, a picture in picture, a rear-view mirror.
///
/// The main camera (`ctx.scene3d.camera`) draws first, into its own
/// [`rect`](crate::d3::Camera::rect); then each view, in the list's order,
/// on top. Each view is a whole frame of its own camera: its own culling
/// (only what it sees, on its camera's
/// [`layers`](crate::d3::Camera::layers)), its own temporal history
/// (temporal antialiasing, temporal ambient occlusion and screen-space
/// reflections never mix two cameras' pictures) and its own [`Post`]
/// settings, at its rectangle's size. What doesn't depend on the camera
/// is done once per frame for all of them: skinning, the game's compute
/// passes, target cameras, global illumination and the shadow maps. The
/// sun's cascades are fitted around every view (the farther apart the
/// cameras, the coarser the shadows; see
/// [`Lighting::shadow_distance`](crate::render::Lighting::shadow_distance)),
/// and point and spot light shadows go to the shadowed lights nearest any
/// camera, in one atlas drawn once per frame, each light's tiles as sharp
/// as the camera nearest it needs. `ctx.debug`'s 3D lines and labels show
/// in every view whose camera shows layer 0.
///
/// # Cost and limits
///
/// Each view costs about what the main view costs at its size: its
/// collection on the CPU and its depth, opaque, transparent and post passes
/// on the GPU, plus a copy of its picture into the window. A view's
/// scene-sized targets and buffers are made when it first draws, kept
/// between frames and freed when it leaves the list. Views draw without
/// occlusion culling (only the main camera has the depth it is read from;
/// a view draws everything in its frustum, never too little). The game's
/// scene compute passes and target effects run once, for the main view.
/// [`Post::msaa`] is the main camera's for every view: every 3D pipeline is
/// built for the renderer's one sample count, and a count per view would
/// build and keep a second set of them. 2D sprites and UI draw on top of
/// every 3D view.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::camera::Viewport;
/// Game::new("Rally", 800, 400).run_headless(1, |ctx| {
///     ctx.scene3d.camera.rect = Viewport::split(2, 0);
///     let mut p2 = ctx.scene3d.camera;
///     p2.rect = Viewport::split(2, 1);
///     ctx.scene3d.views.push(d3::View::new(p2));
///     // Each frame, move both cameras:
///     ctx.scene3d.camera.position = vec3(0.0, 2.0, 6.0);
///     ctx.scene3d.views[0].camera.position = vec3(30.0, 2.0, 6.0);
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View3D {
    /// What it shows, where in the window ([`rect`](crate::d3::Camera::rect))
    /// and which [`layers`](crate::d3::Camera::layers). The engine keeps
    /// its size in pixels up to date.
    pub camera: Camera3D,
    /// The color behind a flat sky (as [`Graphics::clear`](crate::gfx::Graphics::clear)
    /// is for the main camera); `None` (the default) uses the game's.
    pub clear: Option<Color>,
    /// Its own post settings (resolution, antialiasing, ambient occlusion,
    /// bloom, tone mapping, ...); `None` (the default) uses the main
    /// camera's `ctx.scene3d.post`. A minimap can turn the costly ones
    /// off. [`Post::msaa`] is ignored: the main camera's applies.
    pub post: Option<Post>,
}

impl View3D {
    /// A view through `camera`, into the camera's
    /// [`rect`](crate::d3::Camera::rect), with the game's clear color and
    /// post settings.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let mut map = ctx.scene3d.camera;
    /// map.rect = ponko::camera::Viewport::new(0.75, 0.0, 0.25, 0.25);
    /// map.position = vec3(0.0, 80.0, 0.0);
    /// // Straight down, with north (-z) at the top.
    /// map.rotation = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    /// ctx.scene3d.views.push(d3::View::new(map));
    /// # }
    /// ```
    #[must_use]
    pub fn new(camera: Camera3D) -> Self {
        Self {
            camera,
            clear: None,
            post: None,
        }
    }

    /// The same view with its own post settings.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let mut cheap = ctx.scene3d.post;
    /// cheap.bloom = 0.0;
    /// cheap.anti_aliasing = ponko::render::AntiAliasing::Fxaa;
    /// let view = d3::View::new(ctx.scene3d.camera).with_post(cheap);
    /// assert!(view.post.is_some());
    /// # }
    /// ```
    #[must_use]
    pub fn with_post(mut self, post: Post) -> Self {
        self.post = Some(post);
        self
    }

    /// The same view with `color` behind a flat sky.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let view = d3::View::new(ctx.scene3d.camera).with_clear(Color::BLACK);
    /// assert_eq!(view.clear, Some(Color::BLACK));
    /// # }
    /// ```
    #[must_use]
    pub fn with_clear(mut self, color: Color) -> Self {
        self.clear = Some(color);
        self
    }
}
