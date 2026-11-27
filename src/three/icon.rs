//! Item icons: a model drawn once into a render target, for a UI image
//! (`Graphics::render_icon`).
//!
//! The model is drawn by the renderer the game draws with, as a target
//! camera of one frame, but from a world of its own holding only it: the
//! game's world isn't touched (nothing spawned, nothing that casts or
//! receives a shadow there), and nothing of the game's world shows in the
//! icon. The icon is lit by the scene's sun and sky light and tone mapped
//! on the way into its target, so it holds what a UI shows: colours from 0
//! to 1 over the icon's background.

use glam::{Vec3, Vec4};

use super::{MaterialDesc, Mesh, Model, ModelMaterials, RenderTarget, ScreenEffect, TargetCamera, Transform3D};
use crate::color::Color;
use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;

/// How [`Graphics::render_icon`] draws a model: the icon's size, where it is
/// seen from, how bright, on what background.
///
/// ```
/// # use ponko::prelude::*;
/// let icon = ponko::render::Icon::new(96)
///     .with_from(vec3(0.0, 0.3, 1.0))
///     .with_background(Color::rgb(0.1, 0.1, 0.15));
/// assert_eq!(icon.size, 96);
/// assert_eq!(icon.background, Color::rgb(0.1, 0.1, 0.15));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Icon {
    /// Width and height in pixels. Default 128.
    pub size: u32,
    /// Where the camera is, seen from the model's center: only the
    /// direction counts. Default from the front-right and above, as the
    /// editor's thumbnails.
    pub from: Vec3,
    /// The camera's vertical field of view, in radians. Default 0.5 (about
    /// 29°): little perspective, as icons are usually drawn.
    pub fov_y: f32,
    /// Multiplies the light before tone mapping. Default 1.
    pub exposure: f32,
    /// Behind the model. Default [`Color::TRANSPARENT`]: alpha 0 where the
    /// model isn't, so a UI shows what is behind the icon there.
    pub background: Color,
    /// Multiplied with the materials' colours, as [`Model::color`]. Default
    /// white.
    pub color: Color,
    /// Other materials for chosen parts of the mesh, as on a model.
    /// Default none.
    pub materials: Option<ModelMaterials>,
}

impl Default for Icon {
    fn default() -> Self {
        Self::new(128)
    }
}

impl Icon {
    /// An icon `size` pixels wide and high, with the defaults for the rest.
    ///
    /// ```
    /// assert_eq!(ponko::render::Icon::new(64).size, 64);
    /// ```
    #[must_use]
    pub fn new(size: u32) -> Self {
        Self {
            size,
            from: Vec3::new(0.55, 0.45, 0.7),
            fov_y: 0.5,
            exposure: 1.0,
            background: Color::TRANSPARENT,
            color: Color::WHITE,
            materials: None,
        }
    }

    /// The same icon seen from `from` (see [`from`](Self::from)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// // Straight from the front.
    /// let icon = ponko::render::Icon::new(64).with_from(Vec3::Z);
    /// assert_eq!(icon.from, Vec3::Z);
    /// ```
    #[must_use]
    pub fn with_from(mut self, from: Vec3) -> Self {
        self.from = from;
        self
    }

    /// The same icon on `background` (see [`background`](Self::background)).
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let icon = ponko::render::Icon::new(64).with_background(Color::BLACK);
    /// assert_eq!(icon.background, Color::BLACK);
    /// ```
    #[must_use]
    pub fn with_background(mut self, background: Color) -> Self {
        self.background = background;
        self
    }

    /// The same icon, `exposure` times as bright before tone mapping.
    ///
    /// ```
    /// let icon = ponko::render::Icon::new(64).with_exposure(2.0);
    /// assert_eq!(icon.exposure, 2.0);
    /// ```
    #[must_use]
    pub fn with_exposure(mut self, exposure: f32) -> Self {
        self.exposure = exposure;
        self
    }

    /// The same icon with the model tinted `color`, as [`Model::color`].
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// let icon = ponko::render::Icon::new(64).with_color(Color::rgb(1.0, 0.8, 0.2));
    /// assert_eq!(icon.color, Color::rgb(1.0, 0.8, 0.2));
    /// ```
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// The same icon with other materials for chosen parts of the mesh.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, sword: ponko::render::Mesh) {
    /// let gold = ctx.gfx.create_material(ponko::render::MaterialDesc::color(Color::rgb(1.0, 0.8, 0.3)));
    /// // The blade (part 1) in gold.
    /// let icon = ponko::render::Icon::new(64).with_materials(ponko::render::ModelMaterials::new(&[(1, gold)]));
    /// let image = ctx.gfx.render_icon(sword, &icon);
    /// # }
    /// ```
    #[must_use]
    pub fn with_materials(mut self, materials: ModelMaterials) -> Self {
        self.materials = Some(materials);
        self
    }
}

/// An icon waiting for its frame: the world holding its model, the camera
/// drawing it and the material of its tone mapping.
pub(crate) struct PendingIcon {
    pub(crate) world: ponko_ecs::World,
    pub(crate) camera: TargetCamera,
    pub(crate) material: super::Material,
    /// [`Icon::exposure`]: the material gets it with the main view's tone
    /// mapping curve each frame the icon is drawn.
    pub(crate) exposure: f32,
    /// Handed to the renderer this frame.
    pub(crate) sent: bool,
    /// A frame drew it: its target holds the picture.
    pub(crate) drawn: bool,
}

/// The tone mapping curves of the main view's post pass, renamed so no
/// effect prelude name clashes with them.
fn curves() -> String {
    let post = include_str!("../render/post.wgsl");
    let start = post.find("// Narkowicz's fit").expect("post.wgsl has its ACES curve");
    let end = post.find("fn tonemap(").expect("post.wgsl has its tonemap function");
    post[start..end]
        .replace("fn aces(", "fn icon_aces(")
        .replace("fn agx(", "fn icon_agx(")
        .replace("fn neutral(", "fn icon_neutral(")
}

/// The icon's effect: the picture times `custom[0].x` (exposure), tone
/// mapped with curve `custom[0].y` (a [`Tonemap`](super::Tonemap)).
fn tone_mapping_source() -> String {
    format!(
        "{}
fn effect(p: Pixel) -> vec3<f32> {{
    let c = p.color * material.custom[0].x;
    switch u32(material.custom[0].y) {{
        case 0u: {{ return icon_aces(c); }}
        case 1u: {{ return icon_agx(c); }}
        case 2u: {{ return clamp(icon_neutral(c), vec3<f32>(0.0), vec3<f32>(1.0)); }}
        default: {{ return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)); }}
    }}
}}",
        curves()
    )
}

/// The material of an icon's tone mapping: `custom[0]` is the exposure and
/// the curve.
pub(crate) fn icon_material(exposure: f32, tonemap: super::Tonemap) -> MaterialDesc {
    MaterialDesc::default().with_custom([
        Vec4::new(exposure, tonemap as u32 as f32, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::ZERO,
        Vec4::ZERO,
    ])
}

impl Graphics {
    /// After a frame: icons handed to the renderer are done if it was drawn
    /// (one that wasn't hands them to the next frame).
    pub(crate) fn settle_icons(&mut self, drawn: bool) {
        for icon in &mut self.icons {
            icon.drawn |= std::mem::take(&mut icon.sent) && drawn;
        }
    }
}

impl Graphics {
    /// Draws `mesh` into a new square render target that keeps the picture,
    /// for a UI image: an item icon (feature `3d`). Show it with `target.texture()`, as
    /// [`Ui::image`](crate::ui::Ui::image) or any texture.
    ///
    /// The mesh is framed to fill the icon, seen from [`Icon::from`]
    /// through a camera of [`Icon::fov_y`], lit by the scene's sun and sky
    /// light (no shadows, no other lights) and tone mapped as the main
    /// view is ([`Post::tonemap`](crate::render::Post::tonemap)), on
    /// [`Icon::background`]. Nothing of the game's world is in it, and
    /// nothing is added to the world: the model is drawn from a world of
    /// its own.
    ///
    /// It is drawn with the next frame, before the UI, so the UI can show
    /// it that frame (also the game's first frame, for an icon asked for in
    /// setup); then the target keeps the picture until it is freed
    /// ([`Graphics::free`]).
    /// It costs one small target camera's view in that frame and nothing
    /// after. Without a GPU
    /// (headless) the target is made and stays empty.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx, sword: ponko::render::Mesh) {
    /// let icon = ctx.gfx.render_icon(sword, &ponko::render::Icon::new(64));
    /// let root = ctx.ui.root().id();
    /// ctx.ui.image(root, icon.texture());
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If the mesh isn't live or the size is zero or larger than the GPU
    /// takes ([`try_render_icon`](Self::try_render_icon) returns those).
    pub fn render_icon(&mut self, mesh: Mesh, icon: &Icon) -> RenderTarget {
        self.try_render_icon(mesh, icon).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`render_icon`](Self::render_icon) with errors as values.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] for a mesh that isn't live, a size of zero, a
    /// `from` of zero length or not finite, or a field of view outside 0
    /// to π; as [`Graphics::try_create_render_target`](crate::gfx::Graphics::try_create_render_target) otherwise.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let white = ctx.gfx.default_material();
    ///     let cube = ctx.gfx.create_mesh(&ponko::render::MeshData::cube(1.0), white);
    ///     let icon = ctx.gfx.try_render_icon(cube, &ponko::render::Icon::new(32)).unwrap();
    ///     assert_eq!((icon.width(), icon.height()), (32, 32));
    ///     let err = ctx.gfx.try_render_icon(cube, &ponko::render::Icon::new(0)).unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::Invalid);
    /// });
    /// ```
    pub fn try_render_icon(&mut self, mesh: Mesh, icon: &Icon) -> Result<RenderTarget, Error> {
        let invalid = |what: String| Error::new(ErrorKind::Invalid, what);
        let Some((center, radius)) = self.assets3d.live_bounds(mesh.id) else {
            return Err(invalid("render_icon: the mesh isn't live".to_owned()));
        };
        if icon.size == 0 {
            return Err(invalid("render_icon: the icon's size must not be zero".to_owned()));
        }
        let from = icon.from.normalize_or_zero();
        if from == Vec3::ZERO || !from.is_finite() {
            return Err(invalid(format!(
                "render_icon: `from` must be a direction (got {})",
                icon.from
            )));
        }
        if !(icon.fov_y > 0.0 && icon.fov_y < std::f32::consts::PI) {
            return Err(invalid(format!(
                "render_icon: the field of view must be between 0 and π radians (got {})",
                icon.fov_y
            )));
        }
        let target = self.try_create_render_target(icon.size, icon.size)?;
        if self.renderer.is_none() || self.headless {
            return Ok(target);
        }
        let radius = radius.max(1e-3);
        let mut camera = super::Camera3D::new(glam::Vec2::splat(icon.size as f32));
        camera.fov_y = icon.fov_y;
        let distance = radius / (icon.fov_y * 0.5).sin() * 1.05;
        camera.position = center + from * distance;
        camera.look_at(center);
        camera.near = (distance - radius * 1.1).max(distance * 1e-3);
        camera.far = distance + radius * 1.1;
        let effect = match self.assets3d.icon_effect {
            Some(e) => e,
            None => {
                let e = self.try_create_effect("icon tone mapping", &tone_mapping_source())?;
                self.assets3d.icon_effect = Some(e);
                e
            }
        };
        let material = self.create_material(icon_material(icon.exposure, super::Tonemap::default()));
        let mut world = ponko_ecs::World::new();
        let model = world.spawn((
            Transform3D::default(),
            Model {
                mesh,
                color: icon.color,
            },
        ));
        if let Some(materials) = &icon.materials {
            world.insert(model, materials.clone());
        }
        self.icons.push(PendingIcon {
            world,
            camera: TargetCamera::new(camera, target)
                .with_particles(false)
                .with_debug_draw(false)
                .with_clear(icon.background)
                .with_effect(ScreenEffect::new(effect).with_material(material)),
            material,
            exposure: icon.exposure,
            sent: false,
            drawn: false,
        });
        Ok(target)
    }
}
