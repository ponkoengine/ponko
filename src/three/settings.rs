//! [`SceneSettings`]: everything the renderer reads when it draws the 3D
//! frame, in one place (`ctx.scene3d`).

use glam::Vec2;

use super::autotune::AutotuneSettings;
use super::{Camera3D, ComputePass, Lighting, LodSettings, Post, ScreenEffect, Sky, TargetCamera, TargetEffect};

/// How the 3D world is drawn (feature `3d`): the camera, the light, the sky
/// and fog, what happens to the frame on its way to the screen, levels of
/// detail and the game's own GPU passes. It is [`Ctx::scene3d`](crate::ctx::Ctx::scene3d); the
/// renderer reads it when the frame is drawn.
///
/// Each 3D setting has one home here, and the fields are public, so a system
/// can change several at once (`let s = &mut ctx.scene3d;`). The one setting
/// shared with 2D stays outside: the background color,
/// [`Graphics::clear`](crate::gfx::Graphics::clear), which shows behind the
/// 3D world only with a flat sky ([`Sky::atmosphere`] off). The graphics
/// quality presets ([`set_quality`](Self::set_quality)) and automatic quality
/// ([`set_quality_autotune`](Self::set_quality_autotune)) write into
/// [`lighting`](Self::lighting) and [`post`](Self::post).
///
/// It holds plain values, so it can be copied and replaced: `ctx.scene3d =
/// SceneSettings::default()` starts over with the engine's defaults (and
/// automatic quality off), and restoring a clone restores those settings. The
/// engine keeps the camera's view at the window's size either way, from the
/// next frame on.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let s = &mut ctx.scene3d;
/// s.camera.position = vec3(0.0, 4.0, 10.0);
/// s.camera.look_at(Vec3::ZERO);
/// s.lighting.sun_direction = ponko::render::Sky::sun_direction_at(17.0);
/// s.sky.fog_density = 0.01;
/// s.post.bloom = 0.2;
/// s.set_quality(ponko::render::GraphicsQuality::High);
/// # }
/// ```
///
/// # Defaults and cost
///
/// [`Default`] is the default of each part: [`Camera3D`], [`Lighting`] (with
/// its shadows on), [`Sky`] (the atmosphere on, no fog), [`Post`] (4x
/// multisampling, tone mapping and a little bloom), [`LodSettings`], no
/// [`RenderPasses`] and automatic quality off. Holding the settings costs
/// nothing; the renderer pays for what each part turns on, as the docs of
/// the part say. They are plain values (the pass lists are `Vec`s), so
/// copying them allocates only for those.
#[derive(Debug, Clone)]
pub struct SceneSettings {
    /// The 3D camera: position, rotation, field of view.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.camera.position = vec3(0.0, 5.0, 10.0);
    /// ctx.scene3d.camera.look_at(Vec3::ZERO);
    /// # }
    /// ```
    pub camera: Camera3D,
    /// The sun, the ambient light's strength, shadows and bounced light.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.lighting.sun_intensity = 1.0; // overcast
    /// # }
    /// ```
    pub lighting: Lighting,
    /// The sky behind the world and the fog in front of it: a physical sky
    /// lit by the sun by default, which also lights the scene. Without it,
    /// the ambient light comes from the sky's flat colors.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.lighting.sun_direction = ponko::render::Sky::sun_direction_at(7.0); // morning
    /// ctx.scene3d.sky.fog_density = 0.01;
    /// # }
    /// ```
    pub sky: Sky,
    /// The scene-wide diffuse and specular environment. `None` (the
    /// default) uses the sky. A map supplies the light wherever local
    /// reflection probes don't cover the scene, leaving the visible sky
    /// alone. [`Lighting::ambient_intensity`] scales the map's radiance.
    pub environment: Option<super::EnvironmentMap>,
    /// What happens to the lit frame on its way to the screen: resolution,
    /// antialiasing, ambient occlusion, reflections, exposure, bloom, color
    /// grading, tone mapping.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.post.bloom = 0.2;
    /// ctx.scene3d.post.vignette = 0.3;
    /// # }
    /// ```
    pub post: Post,
    /// Global settings for [`LodGroup`](crate::render::LodGroup)s: the
    /// distance bias and crossfades.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.lod.bias = 1.5; // keep detail up to 50% farther
    /// # }
    /// ```
    pub lod: LodSettings,
    /// The game's own GPU passes, run each frame: see [`RenderPasses`].
    pub passes: RenderPasses,
    /// More 3D cameras, each drawing the world into its part of the window
    /// after the main [`camera`](Self::camera), in order: split screen, a
    /// minimap, a picture in picture. Empty by default, which costs
    /// nothing; each view costs about a frame of its size (see
    /// [`d3::View`](crate::d3::View)).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// use ponko::camera::Viewport;
    /// ctx.scene3d.camera.rect = Viewport::split(2, 0);
    /// let mut p2 = ctx.scene3d.camera;
    /// p2.rect = Viewport::split(2, 1);
    /// ctx.scene3d.views.push(d3::View::new(p2));
    /// # }
    /// ```
    pub views: Vec<crate::d3::View>,
    /// First-person held items drawn with their own camera over the world,
    /// so they never clip into walls: see [`ViewModel`](crate::d3::ViewModel)
    /// and [`HeldItem`](crate::d3::HeldItem). `None` by default, which
    /// costs nothing.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.view_model = Some(d3::ViewModel::default());
    /// # }
    /// ```
    pub view_model: Option<crate::d3::ViewModel>,
    /// Automatic quality's policy, freeze and pins (the running controller
    /// lives on the context).
    pub(crate) autotune: AutotuneSettings,
}

impl Default for SceneSettings {
    /// The engine's defaults. The camera's view is 1×1 until the engine
    /// gives it the window's size at the start of the next frame.
    fn default() -> Self {
        Self::new(Vec2::ONE)
    }
}

impl SceneSettings {
    pub(crate) fn new(viewport: Vec2) -> Self {
        Self {
            camera: Camera3D::new(viewport),
            lighting: Lighting::default(),
            sky: Sky::default(),
            environment: None,
            post: Post::default(),
            lod: LodSettings::default(),
            passes: RenderPasses::default(),
            views: Vec::new(),
            view_model: None,
            autotune: AutotuneSettings::default(),
        }
    }

    /// The camera that draws the window at `screen` (logical pixels, as
    /// [`mouse_pos`](crate::input::Input::mouse_pos)): the last of the
    /// [`views`](Self::views) whose rectangle holds it (they draw on top),
    /// else the main [`camera`](Self::camera), also outside every
    /// rectangle. For picking with several cameras: aim through it with
    /// its [`screen_ray`](Camera3D::screen_ray) or
    /// [`screen_to_ground`](Camera3D::screen_to_ground), which work in the
    /// window's coordinates.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// # fn update(ctx: &mut Ctx) {
    /// if ctx.input.mouse_pressed(MouseButton::Left) {
    ///     let click = ctx.input.mouse_pos();
    ///     let camera = ctx.scene3d.camera_at(click);
    ///     if let Some(ground) = camera.screen_to_ground(click, 0.0) {
    ///         println!("clicked the ground at {ground}");
    ///     }
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn camera_at(&self, screen: Vec2) -> &Camera3D {
        self.views
            .iter()
            .rev()
            .map(|v| &v.camera)
            .find(|c| c.screen_rect().contains(screen))
            .unwrap_or(&self.camera)
    }
}

/// The game's own GPU passes, run every frame in this order:
/// [`compute`](Self::compute) before anything is drawn, then
/// [`target_cameras`](Self::target_cameras), the main view,
/// [`scene_compute`](Self::scene_compute),
/// [`target_effects`](Self::target_effects) and last
/// [`effects`](Self::effects), before bloom and tone mapping. Each list runs
/// in its own order; add, remove or reorder entries freely. At
/// `ctx.scene3d.passes`.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let haze = ctx.gfx.load_effect("effects/haze.wgsl");
/// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(haze));
/// ctx.scene3d.passes.effects.clear(); // and gone again
/// # }
/// ```
///
/// # Defaults and cost
///
/// [`Default`] has every list empty, which costs nothing. Each entry costs
/// its own pass every frame: a compute pass runs its shader once per texel
/// of its target or element of its buffer, a screen or target effect is a
/// full-screen pass, and a target camera draws the 3D world again into its
/// target. Compute passes need compute shaders and buffer passes need
/// storage buffers: on WebGL2 texture passes are rejected when made and
/// buffer passes do nothing.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct RenderPasses {
    /// Compute passes run before anything is drawn: each updates a render
    /// target texel by texel, or a [`GpuBuffer`](crate::render::GpuBuffer).
    /// See [`ComputePass`] for the ordering and resource rules.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let ripples = ctx.gfx.load_compute("compute/ripples.wgsl");
    /// let water = ctx.gfx.create_render_target(256, 256);
    /// ctx.scene3d.passes.compute.push(ponko::render::ComputePass::new(ripples, water));
    /// # }
    /// ```
    pub compute: Vec<ComputePass>,
    /// Cameras drawing the 3D world into render targets, before the main
    /// view: mirrors, reflections, security cameras. See [`TargetCamera`].
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let lake = ctx.gfx.create_render_target(960, 540);
    /// let camera = ctx.scene3d.camera;
    /// ctx.scene3d.passes.target_cameras.push(ponko::render::TargetCamera::reflection(camera, lake, 0.0));
    /// # }
    /// ```
    pub target_cameras: Vec<TargetCamera>,
    /// Compute passes run at the scene stage: after the opaque and
    /// transparent surfaces and temporal antialiasing, before
    /// [`target_effects`](Self::target_effects), [`effects`](Self::effects),
    /// bloom and tone mapping. Their shaders read this frame's lit image and
    /// depth (`scene_color`, `scene_position`, `scene_distance`). Same
    /// entries as [`compute`](Self::compute).
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// // Each texel keeps a running average of how bright the frame is there.
    /// let meter = ctx.gfx.create_compute("meter", "
    ///     fn compute(c: Cell) -> vec4<f32> {
    ///         let seen = scene_color(c.uv * frame.screen.xy);
    ///         return mix(c.value, vec4<f32>(seen, 1.0), 0.05);
    ///     }");
    /// let light = ctx.gfx.create_render_target(16, 9);
    /// ctx.scene3d.passes.scene_compute.push(ponko::render::ComputePass::new(meter, light));
    /// # }
    /// ```
    pub scene_compute: Vec<ComputePass>,
    /// Effects over the lit frame that write into render targets instead of
    /// the frame: after the transparent surfaces, before
    /// [`effects`](Self::effects). See [`TargetEffect`] for the ordering
    /// and resource rules.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, glow: ponko::render::Effect) {
    /// let mask = ctx.gfx.create_render_target(256, 256);
    /// ctx.scene3d.passes.target_effects.push(ponko::render::TargetEffect::new(glow, mask));
    /// # }
    /// ```
    pub target_effects: Vec<TargetEffect>,
    /// Screen effects over the lit frame, before bloom and tone mapping.
    /// Each reads what the one before it left.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let haze = ctx.gfx.load_effect("effects/haze.wgsl");
    /// ctx.scene3d.passes.effects.push(ponko::render::ScreenEffect::new(haze));
    /// # }
    /// ```
    pub effects: Vec<ScreenEffect>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_settings_get_the_windows_view_at_the_next_frame() {
        let mut ctx = crate::ctx::Ctx::new(1.0 / 60.0, Vec2::new(800.0, 600.0));
        ctx.scene3d = SceneSettings::default();
        let center = Vec2::new(400.0, 300.0);
        let off = ctx.scene3d.camera.screen_ray(center).1;
        assert!(
            off.dot(ctx.scene3d.camera.forward()) < 0.99,
            "a 1x1 view: the center is far off"
        );
        ctx.frame_prepare_services();
        let (_, ray) = ctx.scene3d.camera.screen_ray(center);
        assert!(ray.dot(ctx.scene3d.camera.forward()) > 0.9999, "{ray}");
    }
}
