//! Render targets: textures a [`TargetCamera`] draws the 3D world into,
//! read by materials (mirrors, water reflections, security cameras,
//! portals). A [`RenderTarget`] is its own handle type, so a plain texture
//! can't be handed to what writes one.

use glam::{Mat4, Vec3, Vec4};

use super::Camera3D;
use crate::gfx::Graphics;
use crate::texture::Texture;

/// A texture the GPU draws into (feature `3d`): made by
/// [`Graphics::create_render_target`](crate::gfx::Graphics::create_render_target),
/// written by [`TargetCamera`]s, [`ComputePass`](crate::render::ComputePass)es,
/// [`TargetEffect`](crate::render::TargetEffect)s and 3D UI panels, and read
/// like any texture through [`texture`](Self::texture).
///
/// It is its own handle type: what writes a render target takes a
/// `RenderTarget`, so handing it a texture loaded from a file is a compile
/// error, not a pass skipped at run time. It is the [`Texture`] it wraps
/// and nothing more: the same size and layout, and
/// [`texture`](Self::texture) costs nothing. Check it with
/// [`Ctx::is_live`](crate::ctx::Ctx::is_live) and release it with
/// [`Ctx::free`](crate::ctx::Ctx::free), like every handle.
///
/// ```
/// # use ponko::prelude::*;
/// Game::new("t", 1, 1).run_headless(0, |ctx| {
///     let mirror = ctx.gfx.create_render_target(256, 128);
///     ctx.scene3d.passes.target_cameras.push(ponko::render::TargetCamera::new(ctx.scene3d.camera, mirror));
///     // A material shows it as a texture.
///     let glass = ctx.gfx.create_material(ponko::render::MaterialDesc::default().with_texture(mirror.texture()));
///     assert_eq!(mirror.texture().size(), mirror.size());
///     assert!(ctx.is_live(mirror) && ctx.is_live(glass));
///     ctx.free(mirror);
///     assert!(!ctx.is_live(mirror));
/// });
/// ```
///
/// A texture is not a render target:
///
/// ```compile_fail,E0308
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let crab = ctx.gfx.load_texture("crab.png");
/// let camera = ponko::render::TargetCamera::new(ctx.scene3d.camera, crab);
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct RenderTarget {
    pub(crate) texture: Texture,
}

impl RenderTarget {
    /// The engine's own targets and the C API's: `texture` must be a render
    /// target (or the engine's placeholder for a buffer pass).
    pub(crate) const fn from_texture(texture: Texture) -> Self {
        Self { texture }
    }

    /// The target as a texture, for whatever reads it: a material, a
    /// sprite, `ctx.gfx.draw`. Free: it is the same handle.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let screen = ctx.gfx.create_render_target(64, 32);
    ///     let monitor = ponko::render::MaterialDesc::color(Color::BLACK).with_emissive_map(screen.texture());
    ///     assert_eq!(monitor.emissive_map, Some(screen.texture()));
    /// });
    /// ```
    #[must_use]
    #[inline]
    pub const fn texture(self) -> Texture {
        self.texture
    }

    /// Width in pixels.
    #[must_use]
    #[inline]
    pub fn width(&self) -> u32 {
        self.texture.width()
    }

    /// Height in pixels.
    #[must_use]
    #[inline]
    pub fn height(&self) -> u32 {
        self.texture.height()
    }

    /// Size in pixels as a vector.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let target = ctx.gfx.create_render_target(64, 32);
    ///     assert_eq!(target.size(), vec2(64.0, 32.0));
    /// });
    /// ```
    #[must_use]
    #[inline]
    pub fn size(&self) -> glam::Vec2 {
        self.texture.size()
    }
}

/// A camera drawing the 3D world into a render target each frame, before
/// the main view: one entry of [`RenderPasses::target_cameras`](crate::render::RenderPasses::target_cameras).
///
/// The target holds the lit scene with its sky and fog, as linear HDR
/// light, before screen effects, bloom and tone mapping. It uses the main
/// view's shadows. Transparent models aren't drawn into it; particles,
/// trails and lines are (sorted and facing this camera), unless its
/// camera's [`particles`](crate::d3::Camera::particles) is off, but
/// without [`softness`](crate::d3::ParticleSettings::softness). So is
/// `ctx.debug`'s 3D drawing, while it draws particles or its camera's
/// [`debug_draw`](crate::d3::Camera::debug_draw) says so
/// ([`with_debug_draw`](Self::with_debug_draw)).
///
/// Target cameras have no temporal history: no jitter, TAA, temporal AO or
/// SSR is applied. Each draw clears and replaces the target's color/depth.
/// Changing MSAA recreates their attachments before the next draw.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let screen = ctx.gfx.create_render_target(512, 512);
/// let mut camera = ctx.scene3d.camera;
/// camera.position = vec3(0.0, 10.0, 0.0);
/// camera.look_at(Vec3::ZERO);
/// ctx.scene3d.passes.target_cameras.push(ponko::render::TargetCamera::new(camera, screen));
/// // A monitor showing it: a material with the target as its texture.
/// let monitor = ctx.gfx.create_material(
///     ponko::render::MaterialDesc::color(Color::BLACK).with_emissive_map(screen.texture()).with_emissive(Color::WHITE),
/// );
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TargetCamera {
    /// Where it looks from. Its aspect ratio follows the target's size.
    pub camera: Camera3D,
    /// Made by [`Graphics::create_render_target`](crate::gfx::Graphics::create_render_target).
    pub target: RenderTarget,
    /// Only what lies on the front of this plane is drawn: `(normal, d)`,
    /// keeping points where `normal.dot(p) + d >= 0`. For reflections,
    /// which mustn't show what's below the mirror. It costs nothing: the
    /// camera's near plane is tilted onto it.
    pub clip_plane: Option<Vec4>,
}

impl TargetCamera {
    /// `camera` drawing into `target`, with no clip plane.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let target = ctx.gfx.create_render_target(256, 256);
    /// ctx.scene3d.passes.target_cameras.push(ponko::render::TargetCamera::new(ctx.scene3d.camera, target));
    /// # }
    /// ```
    #[must_use]
    pub fn new(camera: Camera3D, target: RenderTarget) -> Self {
        Self {
            camera,
            target,
            clip_plane: None,
        }
    }

    /// The same camera, drawing particles, trails and lines or not (its
    /// camera's [`particles`](crate::d3::Camera::particles)). A camera
    /// copied from the main one draws them as the main one does.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let target = ctx.gfx.create_render_target(256, 256);
    /// // A security camera that shows the room but not the smoke.
    /// let camera = ponko::render::TargetCamera::new(ctx.scene3d.camera, target).with_particles(false);
    /// assert!(!camera.camera.particles);
    /// # }
    /// ```
    #[must_use]
    pub fn with_particles(mut self, particles: bool) -> Self {
        self.camera.particles = particles;
        self
    }

    /// The same camera, drawing `ctx.debug`'s 3D lines and labels or not,
    /// whether it draws particles or not (its camera's
    /// [`debug_draw`](crate::d3::Camera::debug_draw)). Without it, a target
    /// camera draws them while it draws particles.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("Monitor", 640, 360).run_headless(0, |ctx| {
    ///     let target = ctx.gfx.create_render_target(256, 256);
    ///     // A security camera that shows the room and the debug drawing, not the smoke.
    ///     let camera = ponko::render::TargetCamera::new(ctx.scene3d.camera, target)
    ///         .with_particles(false)
    ///         .with_debug_draw(true);
    ///     assert_eq!(camera.camera.debug_draw, Some(true));
    ///     ctx.scene3d.passes.target_cameras.push(camera);
    /// });
    /// ```
    #[must_use]
    pub fn with_debug_draw(mut self, debug_draw: bool) -> Self {
        self.camera.debug_draw = Some(debug_draw);
        self
    }

    /// The camera seeing `camera`'s view mirrored in the level plane at
    /// height `level` (a lake, a polished floor), clipped to what's above
    /// it. Read its target at the screen position flipped upside down:
    /// `vec2(uv.x, 1.0 - uv.y)`.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("lake", 640, 360).run_headless(0, |ctx| {
    ///     let reflection = ctx.gfx.create_render_target(640, 360);
    ///     ctx.scene3d.camera.position = vec3(0.0, 5.0, 0.0);
    ///     let mirrored = ponko::render::TargetCamera::reflection(ctx.scene3d.camera, reflection, 1.0);
    ///     assert_eq!(mirrored.camera.position, vec3(0.0, -3.0, 0.0));
    ///     ctx.scene3d.passes.target_cameras.push(mirrored);
    /// });
    /// ```
    #[must_use]
    pub fn reflection(camera: Camera3D, target: RenderTarget, level: f32) -> Self {
        let mut mirrored = camera;
        mirrored.position.y = 2.0 * level - camera.position.y;
        // Forward and up mirror in y; the image comes out upside down,
        // which the flipped lookup undoes.
        let forward = camera.forward() * Vec3::new(1.0, -1.0, 1.0);
        mirrored.rotation = super::look_rotation(forward);
        Self {
            camera: mirrored,
            target,
            clip_plane: Some(Vec4::new(0.0, 1.0, 0.0, -level)),
        }
    }
}

/// `view_proj` with its near plane tilted onto world plane `plane` (Eric
/// Lengyel's oblique frustum), for a camera at `camera`. Unchanged when
/// the camera is on the plane's kept side (the plane is then in front).
pub(crate) fn oblique(view_proj: Mat4, view: Mat4, plane: Vec4) -> Mat4 {
    // The plane in view space: transforming planes takes the inverse
    // transpose, and `view.inverse()` is the camera's own matrix.
    let c = view.inverse().transpose() * plane;
    let c = c.normalize_or_zero();
    if c.w >= 0.0 || c == Vec4::ZERO {
        return view_proj;
    }
    let projection = view_proj * view.inverse();
    // The far corner of the frustum on the plane's side, in view space
    // (reverse-Z: the far plane is at depth 0).
    let clip_plane = projection.inverse().transpose() * c;
    let q = projection.inverse() * Vec4::new(clip_plane.x.signum(), clip_plane.y.signum(), 0.0, 1.0);
    // Reverse-Z depth 1..0: the near plane is `w - z >= 0` and the far
    // plane `z >= 0`. The near plane becomes the clip plane (w - z = c,
    // scaled), and the far plane still passes through that corner (where
    // w is 1).
    let mut rows = projection.transpose().to_cols_array_2d();
    let z = Vec4::from_array(rows[3]) - c / c.dot(q);
    rows[2] = z.to_array();
    Mat4::from_cols_array_2d(&rows).transpose() * view
}

/// Whether `p` is on the kept side of `plane`.
#[cfg(test)]
fn kept(plane: Vec4, p: Vec3) -> bool {
    use glam::Vec4Swizzles;
    plane.xyz().dot(p) + plane.w >= 0.0
}

impl Graphics {
    /// A [`RenderTarget`] of `width` x `height` pixels that a [`TargetCamera`] in
    /// [`RenderPasses::target_cameras`](crate::render::RenderPasses::target_cameras) draws the 3D world into (feature `3d`).
    /// Use it in a material like any texture ([`RenderTarget::texture`]); it holds linear HDR light.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let mirror = ctx.gfx.create_render_target(512, 512);
    /// assert_eq!(mirror.width(), 512);
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the size is zero or larger than the GPU takes.
    pub fn create_render_target(&mut self, width: u32, height: u32) -> RenderTarget {
        self.try_create_render_target(width, height)
            .unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`create_render_target`](Self::create_render_target) with errors as
    /// values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`](crate::error::ErrorKind::Invalid) for a zero
    /// size, [`Limit`](crate::error::ErrorKind::Limit) for one larger than the
    /// GPU takes.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_create_render_target(0, 64).unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Invalid);
    /// });
    /// ```
    pub fn try_create_render_target(&mut self, width: u32, height: u32) -> Result<RenderTarget, crate::error::Error> {
        let target = self.make_render_target(width, height)?;
        self.textures_live.insert(target.texture.id);
        self.asset_ledger
            .made(crate::inventory::AssetClass::RenderTarget, target.texture.id);
        Ok(target)
    }

    /// [`Graphics::try_create_render_target`](crate::gfx::Graphics::try_create_render_target) for the engine's own targets
    /// (reflection probe faces): not the game's to free or count.
    pub(crate) fn make_render_target(&mut self, width: u32, height: u32) -> Result<RenderTarget, crate::error::Error> {
        if width == 0 || height == 0 {
            return Err(crate::error::Error::new(
                crate::error::ErrorKind::Invalid,
                format!("render target size must not be zero (got {width} x {height})"),
            ));
        }
        let texture = if self.headless {
            Texture::new(self.headless_textures.make("texture"), width, height)
        } else {
            let renderer = self.renderer();
            let max = renderer.max_texture_size();
            if width > max || height > max {
                return Err(crate::error::Error::new(
                    crate::error::ErrorKind::Limit,
                    format!("{width} x {height} is larger than this GPU's maximum texture size ({max})"),
                ));
            }
            Texture::new(renderer.create_render_target(width, height), width, height)
        };
        self.assets3d.render_targets.push(texture);
        Ok(RenderTarget::from_texture(texture))
    }

    /// Whether `texture` is a render target made by this game and not freed
    /// (a [`RenderTarget`]'s [`texture`](RenderTarget::texture)). Only its id is compared.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let target = ctx.gfx.create_render_target(64, 64);
    ///     assert!(ctx.gfx.is_render_target(target.texture()));
    ///     let image = ctx.gfx.create_texture(1, 1, &[255; 4]);
    ///     assert!(!ctx.gfx.is_render_target(image));
    /// });
    /// ```
    #[must_use]
    pub fn is_render_target(&self, texture: Texture) -> bool {
        self.assets3d.render_targets.iter().any(|t| t.id == texture.id)
    }

    /// Engine side: replaces every texel of `target` with `texels`, RGBA
    /// half floats (8 bytes a texel), for fields the CPU works out where
    /// the GPU can't (the water kit's ripples and spectrum on WebGL2).
    /// Does nothing headless, for a freed target or a wrong length.
    pub(crate) fn write_render_target(&mut self, target: RenderTarget, texels: &[u8]) {
        let (w, h) = (target.width(), target.height());
        if self.headless
            || texels.len() != w as usize * h as usize * 8
            || !self.textures_live.contains(target.texture.id)
        {
            return;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.write_render_target(target.texture.id, w, h, texels);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::{Quat, Vec2};

    fn camera() -> Camera3D {
        let mut c = Camera3D::new(Vec2::new(800.0, 600.0));
        c.position = Vec3::new(0.0, -4.0, 10.0);
        c.rotation = super::super::look_rotation(Vec3::new(0.0, 0.5, -1.0));
        c
    }

    fn view(c: &Camera3D) -> Mat4 {
        Mat4::from_rotation_translation(c.rotation, c.position).inverse()
    }

    #[test]
    fn the_oblique_near_plane_is_the_clip_plane() {
        let c = camera();
        let plane = Vec4::new(0.0, 1.0, 0.0, -1.0); // keep y >= 1
        let m = oblique(c.view_proj(), view(&c), plane);
        let depth = |p: Vec3| {
            let clip = m * p.extend(1.0);
            clip.z / clip.w
        };
        // In front of the camera: above the plane drawn, below cut.
        for p in [
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(3.0, 4.0, -20.0),
            Vec3::new(-2.0, 2.0, 5.0),
        ] {
            assert!(kept(plane, p));
            let d = depth(p);
            assert!((0.0..=1.0).contains(&d), "{p} kept, depth {d}");
        }
        for p in [Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.0, -1.0, -10.0)] {
            assert!(depth(p) > 1.0, "{p} is below the plane");
        }
        // On the plane: depth 1, the (reverse-Z) near plane.
        assert!((depth(Vec3::new(0.0, 1.0, -3.0)) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_plane_behind_the_camera_changes_nothing() {
        let c = camera();
        let plane = Vec4::new(0.0, 1.0, 0.0, 10.0); // the camera is on the kept side
        assert_eq!(oblique(c.view_proj(), view(&c), plane), c.view_proj());
    }

    #[test]
    fn reflections_mirror_the_camera_in_the_level() {
        let mut c = Camera3D::new(Vec2::new(800.0, 600.0));
        c.position = Vec3::new(1.0, 6.0, 2.0);
        c.rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.4);
        let target = RenderTarget::from_texture(Texture::new(1, 8, 8));
        let r = TargetCamera::reflection(c, target, 2.0);
        assert_eq!(r.camera.position, Vec3::new(1.0, -2.0, 2.0));
        let (f, g) = (c.forward(), r.camera.forward());
        assert!((g - Vec3::new(f.x, -f.y, f.z)).length() < 1e-5);
        assert_eq!(r.clip_plane, Some(Vec4::new(0.0, 1.0, 0.0, -2.0)));
    }
}
