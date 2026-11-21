//! The water kit: lakes, rivers and seas, ready to use (feature `3d`).
//!
//! A [`Water`] is one body of water: a mesh with a ready-made material on
//! the engine's custom shader hooks, and the passes it asks for. Make one
//! with [`Water::spawn`] from a [`WaterDesc`] ([`WaterDesc::lake`],
//! [`WaterDesc::river`], [`WaterDesc::sea`]) and add [`WaterPlugin`], which
//! keeps it up to date each frame (and, with `physics3d`, floats
//! `Buoyancy` bodies on it).
//!
//! What the material does, all in its own shader:
//! - **Waves**: up to [`MAX_WAVES`] Gerstner waves ([`Waves`]) move the
//!   vertices. The CPU evaluates the same table ([`Water::height_at`],
//!   [`Water::sample`]), so what floats rides the waves that are drawn.
//!   Or waves from a spectrum ([`Waves::spectrum`], [`WaveSpectrum`]): a
//!   field worked out each frame by an FFT, in compute passes (or on the
//!   CPU where the GPU has no compute shaders), the height query running
//!   the same FFT.
//! - **Refraction with depth absorption**: what lies under the surface is
//!   bent by its normal and dimmed per colour channel by the length of
//!   water the light crosses ([`WaterDesc::absorption`]), towards the
//!   water's own colour ([`WaterDesc::scatter`]).
//! - **Reflection**: the sky, a march over the depth buffer
//!   ([`WaterReflection::ScreenSpace`]) or a mirrored camera
//!   ([`WaterReflection::Planar`]), mixed in by Fresnel, plus the sun's
//!   glint.
//! - **Foam** where the water is shallow (the shoreline, from the scene's
//!   depth) and on squeezed crests.
//! - **Flow**: a current ([`WaterDesc::flow`]) or a [`FlowMap`] moves the
//!   small ripples and the foam downstream.
//! - **Contact ripples**: [`Water::touch`] (and bodies crossing the
//!   surface) start rings in a ripple simulation run by a compute pass,
//!   or on the CPU (and uploaded) on a GPU without compute shaders.
//! - **Underwater**: when the camera dips below the surface, a screen
//!   effect tints and dims what it sees by the water between.
//!
//! Nothing here costs anything until a water body is spawned: no pass,
//! texture, shader or uniform is added before that.

mod buoyancy;
mod ripples;
pub(crate) mod shaders;
pub(crate) mod spectrum;
mod waves;

#[cfg(feature = "physics3d")]
pub use buoyancy::{Buoyancy, float_bodies};
pub use spectrum::{SPECTRUM_PERIOD, WaveSpectrum};
pub use waves::{MAX_WAVES, WAVE_GRAVITY, Wave, WaveSample, Waves};

use glam::{Vec2, Vec3, Vec4};
use ponko_ecs::{Component, Entity};

use crate::color::Color;
use crate::ctx::Ctx;
use crate::render::{
    Blend, Compute, ComputePass, Effect, Material, MaterialDesc, Mesh, MeshData, Model, RenderTarget, ScreenEffect,
    Shader, TargetCamera,
};
use crate::three::{TextureArray, TextureKind, Transform3D};

/// How much a water body's shader and passes do: the quality presets.
/// Measured GPU times are in the guide ("Water").
///
/// | | `Low` | `Medium` | `High` |
/// |---|---|---|---|
/// | mesh cells per side | 48 | 96 | 160 |
/// | fine ripple octaves | 1 | 2 | 3 |
/// | refraction (bending) | no | yes | yes |
/// | screen-space reflection steps | 0 (sky only) | 12 | 24 |
/// | planar reflection size | quarter of the window | half | half |
/// | contact ripple field | 64² | 128² | 256² |
///
/// Every preset evaluates all waves, so the CPU height query matches the
/// drawn surface at each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WaterQuality {
    /// For weak GPUs and the web: sky reflections, no bending, few vertices.
    Low,
    /// The default.
    #[default]
    Medium,
    /// More vertices, ripple detail and reflection steps.
    High,
}

/// The numbers behind a [`WaterQuality`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct QualitySpec {
    pub cells: u32,
    pub octaves: u32,
    pub refraction: bool,
    pub ssr_steps: u32,
    pub planar_scale: f32,
    pub ripple_texels: u32,
}

impl WaterQuality {
    pub(crate) fn spec(self) -> QualitySpec {
        match self {
            WaterQuality::Low => QualitySpec {
                cells: 48,
                octaves: 1,
                refraction: false,
                ssr_steps: 0,
                planar_scale: 0.25,
                ripple_texels: 64,
            },
            WaterQuality::Medium => QualitySpec {
                cells: 96,
                octaves: 2,
                refraction: true,
                ssr_steps: 12,
                planar_scale: 0.5,
                ripple_texels: 128,
            },
            WaterQuality::High => QualitySpec {
                cells: 160,
                octaves: 3,
                refraction: true,
                ssr_steps: 24,
                planar_scale: 0.5,
                ripple_texels: 256,
            },
        }
    }
}

/// What the water reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WaterReflection {
    /// Only the sky (whatever draws it: the atmosphere, a sky shader or
    /// the flat sky colour) and the sun's glint. Free.
    Sky,
    /// What is on screen, found by marching the reflected ray over the
    /// depth buffer in the water's own shader, falling back to the sky
    /// where the ray leaves the screen. No extra pass. `Low` quality
    /// reflects the sky only.
    #[default]
    ScreenSpace,
    /// A second camera mirrored under the water level draws the world
    /// into a render target each frame (a [`TargetCamera::reflection`]):
    /// exact for still water, also off screen, but it draws the scene
    /// again. Waves are approximated by bending the mirror image.
    Planar,
}

/// A current that varies over a river: one velocity per texel, laid over
/// the water's whole area (texel (0, 0) at its −x, −z corner). The shader
/// reads it as a data texture; the CPU keeps a copy, so floating bodies
/// drift with it.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::FlowMap;
/// // Fast in the middle of the channel, still at its banks.
/// let flow = FlowMap::from_fn(16, 16, 2.0, |uv| vec2(0.0, 2.0 * (1.0 - (uv.x * 2.0 - 1.0).powi(2))));
/// assert!(flow.at(vec2(0.5, 0.5)).y > 1.9);
/// assert!(flow.at(vec2(0.0, 0.5)).y < 0.3);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FlowMap {
    pub(crate) width: u32,
    pub(crate) height: u32,
    max_speed: f32,
    texels: Vec<Vec2>,
}

impl FlowMap {
    /// A `width` x `height` map whose texel at `uv` (0 to 1 over the area,
    /// texel centres) holds `f(uv)`, in m/s along x and z. Speeds above
    /// `max_speed` are clamped to it (the texture stores velocities in
    /// steps of `max_speed / 127`).
    ///
    /// # Panics
    ///
    /// If either size is zero or `max_speed` isn't positive.
    #[must_use]
    pub fn from_fn(width: u32, height: u32, max_speed: f32, f: impl Fn(Vec2) -> Vec2) -> Self {
        assert!(
            width > 0 && height > 0 && max_speed > 0.0,
            "ponko: FlowMap needs a size above zero and a positive max_speed"
        );
        let texels = (0..width * height)
            .map(|i| {
                let uv = Vec2::new(
                    ((i % width) as f32 + 0.5) / width as f32,
                    ((i / width) as f32 + 0.5) / height as f32,
                );
                f(uv).clamp_length_max(max_speed)
            })
            .map(|v| quantize(v, max_speed))
            .collect();
        Self {
            width,
            height,
            max_speed,
            texels,
        }
    }

    /// A map read from RGBA8 texels (as an image file holds them): red is
    /// the speed along x and green along z, 128 meaning still and 0 / 255
    /// `-max_speed` / `+max_speed`.
    ///
    /// # Panics
    ///
    /// If `rgba` isn't `width * height * 4` bytes, or `max_speed` isn't positive.
    #[must_use]
    pub fn from_rgba(width: u32, height: u32, max_speed: f32, rgba: &[u8]) -> Self {
        assert!(
            rgba.len() == (width * height * 4) as usize && max_speed > 0.0 && width > 0 && height > 0,
            "ponko: FlowMap::from_rgba needs width * height * 4 bytes and a positive max_speed"
        );
        let texels = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Vec2::new(decode(p[0]), decode(p[1])) * max_speed)
            .collect();
        Self {
            width,
            height,
            max_speed,
            texels,
        }
    }

    /// The fastest the map can say, in m/s.
    #[must_use]
    pub fn max_speed(&self) -> f32 {
        self.max_speed
    }

    /// The velocity at `uv` (0 to 1 over the area), filtered between
    /// texels as the GPU filters it.
    #[must_use]
    pub fn at(&self, uv: Vec2) -> Vec2 {
        let (w, h) = (self.width as i32, self.height as i32);
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let texel = |tx: i32, ty: i32| self.texels[(ty.clamp(0, h - 1) * w + tx.clamp(0, w - 1)) as usize];
        let (x0, y0) = (x0 as i32, y0 as i32);
        let top = texel(x0, y0).lerp(texel(x0 + 1, y0), fx);
        let bottom = texel(x0, y0 + 1).lerp(texel(x0 + 1, y0 + 1), fx);
        top.lerp(bottom, fy)
    }

    /// The texels as RGBA8, as the texture stores them.
    pub(crate) fn rgba(&self) -> Vec<u8> {
        self.texels
            .iter()
            .flat_map(|v| [encode(v.x / self.max_speed), encode(v.y / self.max_speed), 0, 255])
            .collect()
    }
}

fn encode(v: f32) -> u8 {
    (v.clamp(-1.0, 1.0) * 127.0 + 128.0).round() as u8
}

fn decode(b: u8) -> f32 {
    (f32::from(b) - 128.0) / 127.0
}

/// `v` as the texture will hold it.
fn quantize(v: Vec2, max: f32) -> Vec2 {
    Vec2::new(decode(encode(v.x / max)), decode(encode(v.y / max))) * max
}

/// Everything about one body of water. Start from [`WaterDesc::lake`],
/// [`WaterDesc::river`] or [`WaterDesc::sea`] and change what you like.
///
/// ```
/// use ponko::prelude::*;
/// use ponko::water::{WaterDesc, WaterQuality, WaterReflection};
/// let pond = WaterDesc::lake(vec2(0.0, 0.0), vec2(40.0, 30.0), 1.5)
///     .with_quality(WaterQuality::High)
///     .with_reflection(WaterReflection::Planar);
/// assert_eq!(pond.level, 1.5);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WaterDesc {
    /// The rest height of the surface, in metres.
    pub level: f32,
    /// The middle of the water's area on the ground (x, z).
    pub center: Vec2,
    /// How far the area reaches along x and z, in metres. The mesh covers
    /// it; shores hide its edges.
    pub size: Vec2,
    /// The mesh moves with the camera and covers everything (a sea to the
    /// horizon): `center` and the area are then ignored, and `size` is how
    /// far the mesh reaches. Its vertices are dense near the camera.
    pub follow_camera: bool,
    /// The waves.
    pub waves: Waves,
    /// A current, in m/s along x and z, where there is no flow map.
    pub flow: Vec2,
    /// A current that varies over the area (rivers). Wins over `flow`.
    pub flow_map: Option<FlowMap>,
    /// How much of each colour channel the water swallows per metre of
    /// light path (red goes first in real water). Higher is murkier.
    pub absorption: Vec3,
    /// The water's own colour: what light scattered inside it looks like,
    /// where the bottom is out of sight.
    pub scatter: Color,
    /// How much foam: 0 none, 1 the default.
    pub foam: f32,
    /// How far from the shore foam reaches, as depth in metres.
    pub shore_foam: f32,
    /// How strongly the surface bends what is under it (0: not at all).
    pub refraction: f32,
    /// What the surface reflects.
    pub reflection: WaterReflection,
    /// How much work the shader and passes do.
    pub quality: WaterQuality,
    /// Contact ripples ([`Water::touch`]): a ripple simulation over a
    /// square of `ripple_area` metres around `center`, in a compute pass,
    /// or on the CPU on a GPU without compute shaders (WebGL2).
    pub ripples: bool,
    /// The side of the ripple square, in metres.
    pub ripple_area: f32,
    /// The underwater screen effect, when the camera dips below.
    pub underwater: bool,
}

impl WaterDesc {
    /// A still lake: light ripples, clear greenish water, screen-space
    /// reflections, shoreline foam and contact ripples.
    #[must_use]
    pub fn lake(center: Vec2, size: Vec2, level: f32) -> Self {
        Self {
            level,
            center,
            size,
            follow_camera: false,
            waves: Waves::lake(Vec2::new(1.0, 0.3), 0.06),
            flow: Vec2::ZERO,
            flow_map: None,
            absorption: Vec3::new(0.45, 0.12, 0.09),
            scatter: Color::rgb(0.02, 0.16, 0.14),
            foam: 1.0,
            shore_foam: 0.25,
            refraction: 1.0,
            reflection: WaterReflection::ScreenSpace,
            quality: WaterQuality::Medium,
            ripples: true,
            ripple_area: size.x.max(size.y).min(64.0),
            underwater: true,
        }
    }

    /// A river: a lake whose water runs at `flow` (m/s along x and z),
    /// carrying the ripples and foam downstream, murkier, without waves.
    #[must_use]
    pub fn river(center: Vec2, size: Vec2, level: f32, flow: Vec2) -> Self {
        Self {
            waves: Waves::none(),
            flow,
            absorption: Vec3::new(0.6, 0.25, 0.2),
            scatter: Color::rgb(0.05, 0.14, 0.1),
            shore_foam: 0.35,
            ..Self::lake(center, size, level)
        }
    }

    /// An open sea to the horizon at `level`: [`Waves::sea`] from `wind`,
    /// `height` metres from trough to crest, deep blue water, crest foam,
    /// a mesh that follows the camera.
    #[must_use]
    pub fn sea(level: f32, wind: Vec2, height: f32) -> Self {
        Self {
            level,
            center: Vec2::ZERO,
            size: Vec2::splat(1500.0),
            follow_camera: true,
            waves: Waves::sea(wind, height),
            flow: Vec2::ZERO,
            flow_map: None,
            absorption: Vec3::new(0.35, 0.07, 0.04),
            scatter: Color::rgb(0.0, 0.06, 0.12),
            foam: 1.0,
            shore_foam: 0.5,
            refraction: 1.0,
            reflection: WaterReflection::ScreenSpace,
            quality: WaterQuality::Medium,
            ripples: true,
            ripple_area: 48.0,
            underwater: true,
        }
    }

    /// The same water at another quality.
    #[must_use]
    pub fn with_quality(self, quality: WaterQuality) -> Self {
        Self { quality, ..self }
    }

    /// The same water reflecting `reflection`.
    #[must_use]
    pub fn with_reflection(self, reflection: WaterReflection) -> Self {
        Self { reflection, ..self }
    }

    /// The same water with these waves.
    #[must_use]
    pub fn with_waves(self, waves: Waves) -> Self {
        Self { waves, ..self }
    }

    /// The same water with a flow map over its area.
    #[must_use]
    pub fn with_flow_map(self, flow_map: FlowMap) -> Self {
        Self {
            flow_map: Some(flow_map),
            ..self
        }
    }

    /// The same water, coloured: `scatter` its own colour, `absorption`
    /// how fast it swallows each channel per metre.
    #[must_use]
    pub fn with_color(self, scatter: Color, absorption: Vec3) -> Self {
        Self {
            scatter,
            absorption,
            ..self
        }
    }

    /// Whether ground point `at` (x, z) is in the water's area.
    #[must_use]
    pub fn contains(&self, at: Vec2) -> bool {
        self.follow_camera || (at - self.center).abs().cmple(self.size * 0.5).all()
    }

    /// The current at ground point `at`, in m/s along x and z.
    #[must_use]
    pub fn flow_at(&self, at: Vec2) -> Vec2 {
        match &self.flow_map {
            Some(map) => map.at((at - self.center) / self.size.max(Vec2::splat(1e-3)) + 0.5),
            None => self.flow,
        }
    }

    /// Where the planar reflection, the ripple field and the mesh are, in
    /// numbers the shaders take.
    fn ripple_region(&self) -> Vec4 {
        let side = self.ripple_area.max(1.0);
        Vec4::new(self.center.x - side / 2.0, self.center.y - side / 2.0, side, 1.0)
    }
}

/// The handles one water body owns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WaterParts {
    pub shader: Shader,
    pub material: Material,
    pub mesh: Mesh,
    pub flow: Option<TextureArray>,
    pub reflection: Option<RenderTarget>,
    pub ripples: Option<RippleParts>,
    pub underwater: Option<(Effect, Material)>,
    pub spectrum: Option<SpectrumParts>,
}

/// The contact ripple simulation's handles. `compute` is `None` where
/// the CPU runs the simulation (no compute shaders) and uploads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RippleParts {
    pub compute: Option<Compute>,
    pub target: RenderTarget,
    pub material: Material,
}

/// The most FFT passes a spectrum takes: the spectrum, then eight stages
/// along x and eight along z (256 texels across).
pub(crate) const MAX_SPECTRUM_PASSES: usize = 17;

/// The handles of waves from a spectrum: the field the surface reads, and
/// on the GPU path its start and its passes (`gpu` is `None` where the CPU
/// works the field out and uploads it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpectrumParts {
    pub field: RenderTarget,
    pub gpu: Option<SpectrumPasses>,
}

/// The spectrum's GPU passes: the start texture, the two shaders and a
/// material per pass (`count` of them, the spectrum's first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpectrumPasses {
    pub start: RenderTarget,
    pub spectrum: Compute,
    pub butterfly: Compute,
    pub materials: [Material; MAX_SPECTRUM_PASSES],
    pub count: usize,
}

/// How many ripple contacts one frame's simulation step takes; more wait
/// for the next frames.
const CONTACTS_PER_FRAME: usize = 3;
/// How many contacts may wait; older ones are dropped past it.
const MAX_WAITING_CONTACTS: usize = 64;

/// One body of water (a component, on the entity [`Water::spawn`] makes,
/// next to its [`Transform`](crate::d3::Transform) and
/// [`Model`]).
///
/// The surface's height at a point and time is
/// [`level`](WaterDesc::level) plus the waves', on the CPU as on the GPU,
/// at the time [`drawn_time`](Water::drawn_time) gives. `float_bodies`
/// runs on the simulation's clock (`ctx.time.sim_time()`), which waves
/// from a spectrum are drawn on too (a pause freezes them); other waves are
/// drawn on the frame's clock (`ctx.time.elapsed()`).
#[derive(Debug, Component)]
pub struct Water {
    desc: WaterDesc,
    pub(crate) parts: WaterParts,
    /// The surface material as last given to the GPU.
    surface: MaterialDesc,
    /// Ripple contacts waiting for the simulation: x, z, radius, strength.
    contacts: Vec<Vec4>,
    /// How many contacts [`Water::touch`] accepted since the water was made.
    contacts_started: u64,
    /// The ripple simulation, where the CPU runs it (no compute shaders).
    cpu_ripples: Option<ripples::CpuRipples>,
    /// The spectrum's field as uploaded, where the CPU works it out.
    field_upload: Vec<u8>,
    /// Whether the ripple material carries contacts now.
    contacts_sent: bool,
    /// Whether the underwater effect is in the frame's passes now.
    under: bool,
    /// The ripple material as last updated (the GPU tests hand it to
    /// their renderer).
    pub(crate) ripple_sent: Option<MaterialDesc>,
    /// The steps the spectrum's field was last drawn between, and whether
    /// the CPU's upload blended their moments: a paused game draws the
    /// same field again, so nothing is worked out or sent.
    field_drawn: Option<(spectrum::DrawnSteps, bool)>,
}

impl Water {
    /// Makes a body of water and spawns it: its mesh, material, shader and
    /// the passes it needs. Add [`WaterPlugin`] to keep it up to date.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::water::{Water, WaterDesc, WaterPlugin};
    /// Game::new("pond", 320, 180).plugin(WaterPlugin).run_headless(2, |ctx| {
    ///     let pond = Water::spawn(ctx, WaterDesc::lake(Vec2::ZERO, vec2(20.0, 20.0), 0.0));
    ///     let water = ctx.world.get::<Water>(pond).unwrap();
    ///     assert_eq!(water.height_at(vec2(1.0, 2.0), 0.0).round(), 0.0);
    /// });
    /// ```
    ///
    /// # Panics
    ///
    /// If its shaders don't compile (a bug: they are tested on every
    /// backend), or the GPU can't make its render targets.
    pub fn spawn(ctx: &mut Ctx, desc: WaterDesc) -> Entity {
        Self::try_spawn(ctx, desc).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`spawn`](Self::spawn) with errors as values.
    ///
    /// # Errors
    ///
    /// What making its shaders, textures or render targets returned. On a
    /// GPU without compute shaders (WebGL2) the ripples are left out
    /// instead of failing.
    pub fn try_spawn(ctx: &mut Ctx, desc: WaterDesc) -> Result<Entity, crate::error::Error> {
        let spec = desc.quality.spec();
        let flow = match &desc.flow_map {
            Some(map) => {
                Some(
                    ctx.gfx
                        .try_create_texture_array(map.width, map.height, &[&map.rgba()], TextureKind::Data)?,
                )
            }
            None => None,
        };
        let reflection = if desc.reflection == WaterReflection::Planar {
            let (w, h) = planar_size(ctx.window_size(), spec.planar_scale);
            Some(ctx.gfx.try_create_render_target(w, h)?)
        } else {
            None
        };
        // Without compute shaders (WebGL2) making the pass fails: the CPU
        // runs the ripples instead.
        let ripples = if desc.ripples {
            let compute = ctx.gfx.try_create_compute("water ripples", shaders::RIPPLES).ok();
            let target = ctx
                .gfx
                .try_create_render_target(spec.ripple_texels, spec.ripple_texels)?;
            let material = ctx.gfx.try_create_material(ripple_material(&desc, &[]))?;
            Some(RippleParts {
                compute,
                target,
                material,
            })
        } else {
            None
        };
        let spectrum = match desc.waves.spectrum_field() {
            Some(field) => Some(spawn_spectrum(ctx, field)?),
            None => None,
        };
        let has_ripples = ripples.is_some();
        let shader = ctx.gfx.try_create_shader(
            "water surface",
            &shaders::surface(&desc, reflection.is_some(), has_ripples),
        )?;
        let mut surface = surface_material(&desc, shader, reflection, ripples.map(|r| r.target), flow);
        surface.texture = spectrum.map(|s| s.field.texture());
        let material = ctx.gfx.try_create_material(surface)?;
        let data = water_mesh(&desc);
        let mesh = ctx.gfx.try_create_mesh(&data, material)?;
        let (center, radius) = mesh_bounds(&desc);
        ctx.gfx.set_mesh_gpu_bounds(mesh, center, radius);
        let underwater = if desc.underwater {
            let effect = ctx
                .gfx
                .try_create_effect("water underwater", &shaders::underwater(&desc))?;
            let mut under = underwater_material(&desc);
            under.texture = spectrum.map(|s| s.field.texture());
            let material = ctx.gfx.try_create_material(under)?;
            Some((effect, material))
        } else {
            None
        };
        if let Some(RippleParts {
            compute: Some(compute),
            target,
            material,
        }) = ripples
        {
            ctx.scene3d
                .passes
                .compute
                .push(ComputePass::new(compute, target).with_material(material));
        }
        if let Some(SpectrumParts { field, gpu: Some(gpu) }) = spectrum {
            ctx.scene3d
                .passes
                .compute
                .extend(gpu.materials[..gpu.count].iter().enumerate().map(|(i, &m)| {
                    ComputePass::new(if i == 0 { gpu.spectrum } else { gpu.butterfly }, field).with_material(m)
                }));
        }
        let parts = WaterParts {
            shader,
            material,
            mesh,
            flow,
            reflection,
            ripples,
            underwater,
            spectrum,
        };
        let at = mesh_position(&desc, ctx.scene3d.camera.position);
        let water = Water::from_parts(desc, parts, surface);
        Ok(ctx.world.spawn((Transform3D::at(at), Model::new(mesh), water)))
    }

    /// A water body over handles already made (the GPU tests make them on
    /// both sides themselves).
    pub(crate) fn from_parts(desc: WaterDesc, parts: WaterParts, surface: MaterialDesc) -> Self {
        let cpu_ripples = parts
            .ripples
            .filter(|r| r.compute.is_none())
            .map(|r| ripples::CpuRipples::new(r.target.width()));
        Self {
            desc,
            parts,
            surface,
            contacts: Vec::with_capacity(MAX_WAITING_CONTACTS),
            contacts_started: 0,
            cpu_ripples,
            field_upload: Vec::new(),
            contacts_sent: false,
            under: false,
            ripple_sent: None,
            field_drawn: None,
        }
    }

    /// What it was made from.
    #[must_use]
    pub fn desc(&self) -> &WaterDesc {
        &self.desc
    }

    /// Its surface material (a custom shader on the engine's hooks):
    /// change its colours with
    /// [`update_material`](crate::gfx::Graphics::update_material).
    #[must_use]
    pub fn material(&self) -> Material {
        self.parts.material
    }

    /// Its mesh.
    #[must_use]
    pub fn mesh(&self) -> Mesh {
        self.parts.mesh
    }

    /// The render target its planar reflection is drawn into, if it has one.
    #[must_use]
    pub fn reflection_target(&self) -> Option<RenderTarget> {
        self.parts.reflection
    }

    /// The render target its contact ripples live in (red: height), if it has them.
    #[must_use]
    pub fn ripple_target(&self) -> Option<RenderTarget> {
        self.parts.ripples.map(|r| r.target)
    }

    /// Whether ground point `at` (x, z) is in its area.
    #[must_use]
    pub fn contains(&self, at: Vec2) -> bool {
        self.desc.contains(at)
    }

    /// The time the surface is drawn at this frame (seconds), to pass to
    /// [`height_at`](Self::height_at), [`sample`](Self::sample) and
    /// [`is_under`](Self::is_under) for what the player sees.
    ///
    /// Waves from a [spectrum](Waves::spectrum) run on the simulation's
    /// clock, as floating bodies do: [`sim_time`](crate::time::Time::sim_time)
    /// plus [`fixed_alpha`](crate::time::Time::fixed_alpha) of a fixed step
    /// (taken within [`SPECTRUM_PERIOD`], after which the sea repeats). They
    /// stop while the game is paused and follow
    /// [`set_speed`](crate::time::Time::set_speed). Until the game first
    /// pauses or changes speed this is the frame's clock to a microsecond.
    /// Other waves run on the frame's clock,
    /// [`elapsed`](crate::time::Time::elapsed) (the shaders' `frame.time.x`).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::water::{Water, WaterDesc, WaveSpectrum, Waves};
    /// Game::new("sea", 320, 180).run_headless(0, |ctx| {
    ///     let desc = WaterDesc::sea(0.0, vec2(8.0, 0.0), 1.0)
    ///         .with_waves(Waves::spectrum(WaveSpectrum::new(vec2(8.0, 0.0), 1.0).with_tile(64.0, 64)));
    ///     let sea = Water::spawn(ctx, desc);
    ///     let water = ctx.world.get::<Water>(sea).unwrap();
    ///     let t = water.drawn_time(&ctx.time);
    ///     assert_eq!(t, ctx.time.sim_time() as f32);
    ///     let seen = water.height_at(vec2(3.0, -2.0), t);
    ///     assert!(seen.abs() < water.desc().waves.max_height());
    /// });
    /// ```
    #[must_use]
    pub fn drawn_time(&self, time: &crate::time::Time) -> f32 {
        if self.desc.waves.spectrum_field().is_some() {
            drawn_steps(time).at
        } else {
            time.elapsed() as f32
        }
    }

    /// The surface's height at ground point `at` (x, z) at time `t`
    /// (seconds; [`drawn_time`](Self::drawn_time) is what the shader
    /// shows), in metres: the level plus the waves. Contact ripples are
    /// drawn only.
    #[must_use]
    pub fn height_at(&self, at: Vec2, t: f32) -> f32 {
        self.desc.level + self.desc.waves.height_at(at, t)
    }

    /// The surface at `at` (x, z) at time `t`: its height (level
    /// included), normal, the water's velocity (waves and current) and how
    /// squeezed it is.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::water::{Water, WaterDesc};
    /// Game::new("river", 320, 180).run_headless(0, |ctx| {
    ///     let river = Water::spawn(ctx, WaterDesc::river(Vec2::ZERO, vec2(8.0, 60.0), 0.0, vec2(0.0, 1.5)));
    ///     let water = ctx.world.get::<Water>(river).unwrap();
    ///     assert_eq!(water.sample(Vec2::ZERO, 0.0).velocity, vec3(0.0, 0.0, 1.5));
    /// });
    /// ```
    #[must_use]
    pub fn sample(&self, at: Vec2, t: f32) -> WaveSample {
        let mut s = self.desc.waves.sample(at, t);
        s.height += self.desc.level;
        let flow = self.desc.flow_at(at);
        s.velocity += Vec3::new(flow.x, 0.0, flow.y);
        s
    }

    /// Whether `point` is under the surface at time `t`.
    #[must_use]
    pub fn is_under(&self, point: Vec3, t: f32) -> bool {
        self.contains(Vec2::new(point.x, point.z)) && point.y < self.height_at(Vec2::new(point.x, point.z), t)
    }

    /// Starts a ring of ripples at `at` (x, z): `radius` metres across at
    /// first, `strength` how high (about metres; negative pulls the water
    /// down). Three contacts start per frame; more wait their turn. Does
    /// nothing outside the ripple area or without ripples.
    pub fn touch(&mut self, at: Vec2, radius: f32, strength: f32) {
        if self.parts.ripples.is_none() || !radius.is_finite() || !strength.is_finite() {
            return;
        }
        let region = self.desc.ripple_region();
        let local = at - Vec2::new(region.x, region.y);
        if local.cmplt(Vec2::ZERO).any() || local.cmpgt(Vec2::splat(region.z)).any() {
            return;
        }
        if self.contacts.len() == MAX_WAITING_CONTACTS {
            self.contacts.remove(0);
        }
        self.contacts.push(Vec4::new(at.x, at.y, radius.max(0.05), strength));
        self.contacts_started += 1;
    }

    /// How many ripple contacts wait for a frame. The water's update in
    /// [`Phase::Late`](crate::game::Phase::Late) takes them, so a system after
    /// physics sees the ones the step started; for a test that only needs to
    /// know whether any started, see [`contacts_started`](Self::contacts_started).
    #[must_use]
    pub fn waiting_contacts(&self) -> usize {
        self.contacts.len()
    }

    /// How many ripple contacts have started since the water was made: the
    /// rings [`touch`](Self::touch) accepted, and floating bodies' crossings of
    /// the surface. Unlike [`waiting_contacts`](Self::waiting_contacts) the
    /// update doesn't drain it, so a test reads it once after the physics step
    /// instead of keeping the most of a count over every tick.
    #[must_use]
    pub fn contacts_started(&self) -> u64 {
        self.contacts_started
    }

    /// Whether the underwater effect is in the frame's passes now.
    #[must_use]
    pub fn shows_underwater(&self) -> bool {
        self.under
    }

    /// One frame's upkeep: the mirror camera follows the camera, waiting
    /// contacts go to the ripple simulation, and the underwater effect is
    /// on exactly while the camera may be under. `t` is the frame's clock;
    /// waves from a spectrum are drawn between the fixed `steps` instead
    /// (see [`drawn_steps`]), from their moments if the CPU uploads the
    /// field (`floating`: buoyancy asks for them too). Returns where the
    /// mesh goes now.
    pub(crate) fn update(
        &mut self,
        gfx: &mut crate::gfx::Graphics,
        scene: &mut crate::render::SceneSettings,
        window: Vec2,
        t: f32,
        steps: spectrum::DrawnSteps,
        floating: bool,
    ) -> Vec3 {
        let camera = scene.camera;
        let mut drawn = None;
        if let Some(target) = self.parts.reflection {
            let wanted = planar_size(window, self.desc.quality.spec().planar_scale);
            let target = if (target.width(), target.height()) == wanted {
                target
            } else {
                self.resize_reflection(gfx, scene, target, wanted)
            };
            let mirror = TargetCamera::reflection(camera, target, self.desc.level);
            let passes = &mut scene.passes.target_cameras;
            match passes.iter_mut().find(|c| c.target == target) {
                Some(c) => *c = mirror,
                None => passes.push(mirror),
            }
        }
        if let (Some(r), Some(sim)) = (self.parts.ripples, self.cpu_ripples.as_mut()) {
            // The CPU's simulation: a step a frame, as the compute pass.
            let take = self.contacts.len().min(CONTACTS_PER_FRAME);
            let mut now = [Vec4::ZERO; CONTACTS_PER_FRAME];
            for (slot, c) in now.iter_mut().zip(self.contacts.drain(..take)) {
                *slot = c;
            }
            let region = ripple_material(&self.desc, &[]).custom[0];
            if sim.step(region, &now[..take]) {
                gfx.write_render_target(r.target, &sim.upload);
            }
        } else if let Some(r) = self.parts.ripples {
            let take = self.contacts.len().min(CONTACTS_PER_FRAME);
            // A frame without contacts after one with them clears them;
            // otherwise the material stays as it is.
            if take > 0 || self.contacts_sent {
                let mut now = [Vec4::ZERO; CONTACTS_PER_FRAME];
                for (slot, c) in now.iter_mut().zip(self.contacts.drain(..take)) {
                    *slot = c;
                }
                let desc = ripple_material(&self.desc, &now[..take]);
                gfx.update_material(r.material, desc);
                self.ripple_sent = Some(desc);
                self.contacts_sent = take > 0;
            }
        }
        if let (Some(SpectrumParts { field, gpu }), Some(spectrum)) =
            (self.parts.spectrum, self.desc.waves.spectrum_field())
        {
            // The same steps (a paused game): the field drawn already.
            let blended = match self.field_drawn {
                Some((last, blended)) if last == steps => blended,
                _ => {
                    let blended = match gpu {
                        // The GPU's passes turn the spectrum to the drawn time.
                        Some(gpu) => {
                            gfx.update_material(gpu.materials[0], spectrum_material(spectrum, gpu.start, steps.at));
                            false
                        }
                        // No compute shaders: the CPU works the field out and uploads it.
                        None => {
                            let blended = spectrum.write_drawn(steps, floating, &mut self.field_upload);
                            gfx.write_render_target(field, &self.field_upload);
                            blended
                        }
                    };
                    self.field_drawn = Some((steps, blended));
                    blended
                }
            };
            drawn = Some((steps, blended));
        }
        if let Some((effect, material)) = self.parts.underwater {
            let under = self.camera_may_be_under(&camera, t, drawn);
            let entry = ScreenEffect::new(effect).with_material(material);
            let effects = &mut scene.passes.effects;
            let at = effects.iter().position(|e| *e == entry);
            match (under, at) {
                (true, None) => effects.push(entry),
                (false, Some(i)) => {
                    effects.remove(i);
                }
                _ => {}
            }
            self.under = under;
        }
        mesh_position(&self.desc, camera.position)
    }

    /// Whether any of the camera's near plane may be below the surface:
    /// its centre is within reach of it (and over the area). `t` is the
    /// frame's clock; `drawn`, for waves from a spectrum, the steps the
    /// field is drawn between and whether the upload blended their moments.
    fn camera_may_be_under(
        &self,
        camera: &crate::three::Camera3D,
        t: f32,
        drawn: Option<(spectrum::DrawnSteps, bool)>,
    ) -> bool {
        let p = camera.position;
        let reach = camera.near * 2.0 + 0.05;
        if !self.desc.follow_camera {
            let margin = Vec2::splat(reach + self.desc.waves.max_sway());
            if ((Vec2::new(p.x, p.z) - self.desc.center).abs() - self.desc.size * 0.5)
                .cmpgt(margin)
                .any()
            {
                return false;
            }
        }
        // Clearly over or under every wave: no need to ask the waves
        // (which, from a spectrum, is a CPU FFT at a new moment).
        let reach_of_waves = self.desc.waves.max_height() + 0.05;
        if p.y - reach > self.desc.level + reach_of_waves {
            return false;
        }
        if p.y - reach < self.desc.level - reach_of_waves {
            return true;
        }
        let at = Vec2::new(p.x, p.z);
        let height = match (drawn, self.desc.waves.spectrum_field()) {
            (Some((steps, true)), Some(spectrum)) => self.desc.level + spectrum.drawn_height(at, steps),
            (Some((steps, _)), _) => self.height_at(at, steps.at),
            (None, _) => self.height_at(at, t),
        };
        p.y - reach < height + 0.05
    }

    /// Replaces the planar reflection's target with one of `size`.
    fn resize_reflection(
        &mut self,
        gfx: &mut crate::gfx::Graphics,
        scene: &mut crate::render::SceneSettings,
        old: RenderTarget,
        size: (u32, u32),
    ) -> RenderTarget {
        let Ok(target) = gfx.try_create_render_target(size.0, size.1) else {
            return old;
        };
        scene.passes.target_cameras.retain(|c| c.target != old);
        gfx.free(old);
        self.parts.reflection = Some(target);
        self.surface.custom_textures[0] = Some(target.texture());
        gfx.update_material(self.parts.material, self.surface);
        target
    }

    /// Takes the water out of the frame's passes and frees everything it
    /// made. The entity is left (despawn it, or spawn water anew).
    pub fn free(&mut self, ctx: &mut Ctx) {
        let parts = self.parts;
        let passes = &mut ctx.scene3d.passes;
        if let Some(target) = parts.reflection {
            passes.target_cameras.retain(|c| c.target != target);
            ctx.gfx.free(target);
        }
        if let Some(r) = parts.ripples {
            passes.compute.retain(|c| c.target != r.target);
            if let Some(compute) = r.compute {
                ctx.gfx.free(compute);
            }
            ctx.gfx.free(r.target);
            ctx.gfx.free(r.material);
        }
        if let Some(s) = parts.spectrum {
            passes.compute.retain(|c| c.target != s.field);
            if let Some(gpu) = s.gpu {
                ctx.gfx.free(gpu.spectrum);
                ctx.gfx.free(gpu.butterfly);
                ctx.gfx.free(gpu.start);
                for &m in &gpu.materials[..gpu.count] {
                    ctx.gfx.free(m);
                }
            }
            ctx.gfx.free(s.field);
        }
        if let Some((effect, material)) = parts.underwater {
            passes.effects.retain(|e| e.effect != effect);
            ctx.gfx.free(effect);
            ctx.gfx.free(material);
        }
        if let Some(flow) = parts.flow {
            ctx.gfx.free(flow);
        }
        ctx.gfx.free(parts.mesh);
        ctx.gfx.free(parts.material);
        ctx.gfx.free(parts.shader);
        self.contacts.clear();
        self.contacts_sent = false;
        self.under = false;
        self.cpu_ripples = None;
    }
}

/// Makes the field of waves from a spectrum: on the GPU its start and its
/// FFT passes (added to the frame by the caller), or, without compute
/// shaders, just the field the CPU uploads to.
fn spawn_spectrum(ctx: &mut Ctx, field: &spectrum::SpectrumField) -> Result<SpectrumParts, crate::error::Error> {
    let n = field.n as u32;
    let target = ctx.gfx.try_create_render_target(n, n)?;
    let Ok(first) = ctx.gfx.try_create_compute("water spectrum", shaders::SPECTRUM) else {
        return Ok(SpectrumParts {
            field: target,
            gpu: None,
        });
    };
    let butterfly = ctx.gfx.try_create_compute("water spectrum fft", shaders::BUTTERFLY)?;
    let start = ctx.gfx.try_create_render_target(n, n)?;
    let mut upload = Vec::new();
    spectrum::write_half_texels(&field.start_texels(), &mut upload);
    ctx.gfx.write_render_target(start, &upload);
    let mut materials = [<Material as crate::handle::Handle>::NONE; MAX_SPECTRUM_PASSES];
    let descs = spectrum_materials(field, start);
    for (slot, desc) in materials.iter_mut().zip(&descs) {
        *slot = ctx.gfx.try_create_material(*desc)?;
    }
    Ok(SpectrumParts {
        field: target,
        gpu: Some(SpectrumPasses {
            start,
            spectrum: first,
            butterfly,
            materials,
            count: descs.len(),
        }),
    })
}

/// The values of the spectrum's passes, in order: the spectrum (reading
/// the start), then each FFT stage along x and along z.
pub(crate) fn spectrum_materials(field: &spectrum::SpectrumField, start: RenderTarget) -> Vec<MaterialDesc> {
    let n = field.n as f32;
    let mut out = vec![spectrum_material(field, start, 0.0)];
    for along in [0.0, 1.0] {
        let mut ns = 1;
        while ns < field.n {
            out.push(MaterialDesc {
                custom: [Vec4::new(n, ns as f32, along, 0.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
                ..Default::default()
            });
            ns *= 2;
        }
    }
    out
}

/// The spectrum pass's values with the waves at time `t` (seconds within
/// the period, see [`drawn_steps`]).
pub(crate) fn spectrum_material(field: &spectrum::SpectrumField, start: RenderTarget, t: f32) -> MaterialDesc {
    let s = field.settings;
    MaterialDesc {
        custom: [
            Vec4::new(field.n as f32, s.tile.max(1.0), s.choppiness.max(0.0), SPECTRUM_PERIOD),
            Vec4::new(t, 0.0, 0.0, 0.0),
            Vec4::ZERO,
            Vec4::ZERO,
        ],
        custom_textures: [Some(start.texture()), None],
        ..Default::default()
    }
}

/// Despawns water body `entity` and frees what it made (see [`Water::free`]).
/// Returns false if `entity` has no [`Water`].
pub fn despawn_water(ctx: &mut Ctx, entity: Entity) -> bool {
    let Some(mut water) = ctx.world.remove::<Water>(entity) else {
        return false;
    };
    water.free(ctx);
    ctx.world.despawn(entity);
    true
}

/// Keeps every [`Water`] up to date for the frame: the sea's mesh follows
/// the camera, planar reflections follow it mirrored, contact ripples go to
/// the simulation and the underwater effect turns on and off. Added by
/// [`WaterPlugin`] in [`Phase::Late`](crate::game::Phase::Late).
pub fn update_water(ctx: &mut Ctx) {
    let mut bodies = ctx
        .world
        .get_resource_mut::<WaterScratch>()
        .map(|s| std::mem::take(&mut s.0))
        .unwrap_or_default();
    bodies.clear();
    ctx.world.query::<(Entity, &Water)>().for_each(|(e, _)| bodies.push(e));
    let (window, t) = (ctx.window_size(), ctx.time.elapsed() as f32);
    let steps = drawn_steps(&ctx.time);
    #[cfg(feature = "physics3d")]
    let floating = ctx.world.query::<&Buoyancy>().iter().next().is_some();
    #[cfg(not(feature = "physics3d"))]
    let floating = false;
    for &e in &bodies {
        let Some(water) = ctx.world.get_mut::<Water>(e) else {
            continue;
        };
        let at = water.update(&mut ctx.gfx, &mut ctx.scene3d, window, t, steps, floating);
        if let Some(transform) = ctx.world.get_mut::<Transform3D>(e)
            && transform.position != at
        {
            transform.position = at;
        }
    }
    match ctx.world.get_resource_mut::<WaterScratch>() {
        Some(scratch) => scratch.0 = bodies,
        None => ctx.world.insert_resource(WaterScratch(bodies)),
    }
}

/// The two fixed steps the drawn frame sits between on the simulation's
/// clock, which waves from a spectrum are drawn on: the next step's time as
/// `float_bodies` asks it then, the one after, how far the frame is between
/// them ([`Time::fixed_alpha`](crate::time::Time::fixed_alpha)), and the
/// time that far between them, taken within [`SPECTRUM_PERIOD`] in full
/// precision. It stands still while the game is paused and runs at the
/// simulation's speed; until the first pause or speed change it is the
/// frame's clock ([`Time::elapsed`](crate::time::Time::elapsed)).
pub(crate) fn drawn_steps(time: &crate::time::Time) -> spectrum::DrawnSteps {
    let (step, tick) = (time.sim_step(), time.sim_tick());
    let between = time.fixed_alpha().clamp(0.0, 1.0);
    let t0 = tick as f64 * step;
    let at = t0 + f64::from(between) * step;
    let period = f64::from(SPECTRUM_PERIOD);
    spectrum::DrawnSteps {
        t0: t0 as f32,
        t1: ((tick + 1) as f64 * step) as f32,
        between,
        at: (at - (at / period).floor() * period) as f32,
    }
}

/// The water entities of this frame (kept, so the update doesn't allocate).
#[derive(Default)]
struct WaterScratch(Vec<Entity>);

/// The water kit's systems: [`update_water`] each frame and, with
/// `physics3d`, `float_bodies` before each
/// physics step. A game without water bodies pays nothing for it.
///
/// ```
/// use ponko::prelude::*;
/// Game::new("sea", 320, 180).plugin(ponko::water::WaterPlugin).run_headless(1, |ctx| {
///     assert!(ctx.scene3d.passes.effects.is_empty());
/// });
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct WaterPlugin;

impl crate::game::Plugin for WaterPlugin {
    fn build(&self, game: crate::game::Game) -> crate::game::Game {
        let game = game.add(crate::game::Phase::Late, update_water);
        #[cfg(feature = "physics3d")]
        let game = game.add(crate::game::Phase::BeforePhysics, float_bodies);
        game
    }
}

/// The planar reflection's size for a window of `window` pixels.
fn planar_size(window: Vec2, scale: f32) -> (u32, u32) {
    (
        ((window.x * scale) as u32).clamp(16, 2048),
        ((window.y * scale) as u32).clamp(16, 2048),
    )
}

/// Where the water's mesh goes: on its area, or under the camera, moved
/// in steps of its finest cells so the vertices don't swim.
fn mesh_position(desc: &WaterDesc, camera: Vec3) -> Vec3 {
    if desc.follow_camera {
        let step = (desc.size.x.max(desc.size.y) / desc.quality.spec().cells as f32 * 0.25).max(0.01);
        Vec3::new(
            (camera.x / step).round() * step,
            desc.level,
            (camera.z / step).round() * step,
        )
    } else {
        Vec3::new(desc.center.x, desc.level, desc.center.y)
    }
}

/// The sphere (in the mesh's space) the waves keep the surface in.
fn mesh_bounds(desc: &WaterDesc) -> (Vec3, f32) {
    let half = desc.size * 0.5;
    (
        Vec3::ZERO,
        half.length() + desc.waves.max_sway() + desc.waves.max_height(),
    )
}

/// The water's mesh: a grid of flat cells around its origin, uniform over
/// its area, or (following the camera) dense near the middle and coarse
/// towards the horizon.
pub(crate) fn water_mesh(desc: &WaterDesc) -> MeshData {
    let cells = desc.quality.spec().cells;
    let half = desc.size * 0.5;
    let n = cells + 1;
    // Where line i of n is, from -1 to 1.
    let line = |i: u32| {
        let u = i as f32 / cells as f32 * 2.0 - 1.0;
        if desc.follow_camera {
            // Fine at the centre: spacing grows with the square of the distance.
            u * (0.08 + 0.92 * u * u)
        } else {
            u
        }
    };
    let mut positions = Vec::with_capacity((n * n) as usize);
    let mut uvs = Vec::with_capacity((n * n) as usize);
    for z in 0..n {
        for x in 0..n {
            let (u, v) = (line(x), line(z));
            positions.push(Vec3::new(u * half.x, 0.0, v * half.y));
            uvs.push(Vec2::new(u * 0.5 + 0.5, v * 0.5 + 0.5));
        }
    }
    let mut indices = Vec::with_capacity((cells * cells * 6) as usize);
    for z in 0..cells {
        for x in 0..cells {
            let i = z * n + x;
            // Counter-clockwise seen from above (+y), as MeshData::plane.
            indices.extend_from_slice(&[i + n, i + n + 1, i + 1, i + n, i + 1, i]);
        }
    }
    MeshData {
        normals: vec![Vec3::Y; positions.len()],
        positions,
        uvs,
        indices,
        ..Default::default()
    }
}

/// The surface material: the kit's shader, its values and textures.
pub(crate) fn surface_material(
    desc: &WaterDesc,
    shader: Shader,
    reflection: Option<RenderTarget>,
    ripples: Option<RenderTarget>,
    flow: Option<TextureArray>,
) -> MaterialDesc {
    let [r, g, b, _] = crate::three::linear(desc.scatter);
    let region = desc.ripple_region();
    let max_flow = desc.flow_map.as_ref().map_or(0.0, FlowMap::max_speed);
    MaterialDesc {
        color: Color::WHITE,
        roughness: 0.03,
        shader: Some(shader),
        blend: Blend::Transparent,
        double_sided: true,
        custom: [
            desc.absorption.max(Vec3::ZERO).extend(desc.refraction.max(0.0)),
            Vec4::new(r, g, b, desc.foam.max(0.0)),
            Vec4::new(desc.flow.x, desc.flow.y, max_flow, desc.shore_foam.max(1e-3)),
            region,
        ],
        custom_textures: [reflection.map(|t| t.texture()), ripples.map(|t| t.texture())],
        custom_arrays: [flow, None, None],
        ..Default::default()
    }
}

/// The underwater effect's values: absorption and level, colour.
pub(crate) fn underwater_material(desc: &WaterDesc) -> MaterialDesc {
    let [r, g, b, _] = crate::three::linear(desc.scatter);
    MaterialDesc {
        custom: [
            desc.absorption.max(Vec3::ZERO).extend(desc.level),
            Vec4::new(r, g, b, 1.0),
            Vec4::ZERO,
            Vec4::ZERO,
        ],
        ..Default::default()
    }
}

/// The ripple simulation's values: its region and damping, and up to
/// three contacts (x, z, radius, strength).
pub(crate) fn ripple_material(desc: &WaterDesc, contacts: &[Vec4]) -> MaterialDesc {
    let region = desc.ripple_region();
    let mut custom = [
        Vec4::new(region.x, region.y, region.z, 0.985),
        Vec4::ZERO,
        Vec4::ZERO,
        Vec4::ZERO,
    ];
    for (slot, c) in custom[1..].iter_mut().zip(contacts) {
        *slot = *c;
    }
    MaterialDesc {
        custom,
        ..Default::default()
    }
}
