//! Post-processing of the 3D frame: exposure, bloom, color grading, tone
//! mapping and vignette, depth of field, and volumes that blend settings by
//! where the camera is.

use crate::color::Color;
use crate::texture::Texture;
use glam::Vec3;

/// How HDR light (which can be far brighter than white) is mapped to the
/// screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
#[repr(u32)]
pub enum Tonemap {
    /// Film-like: rich contrast, bright colors drift towards white.
    #[default]
    Aces = 0,
    /// Like a camera: keeps the hue of bright colors, softer contrast.
    AgX = 1,
    /// Khronos PBR Neutral: colors stay close to the material's own, only
    /// highlights roll off.
    Neutral = 2,
    /// No curve: everything above white clips.
    None = 3,
}

/// Post antialiasing of the 3D frame ([`Post::anti_aliasing`]). FXAA and
/// SMAA work on the tone mapped image before sprites/UI; [`Taa`](Self::Taa)
/// works on the lit HDR frame. Multisampling ([`Post::msaa`]) smooths the
/// edges of what is drawn as well, under any of these.
/// ```
/// use ponko::render::AntiAliasing;
/// assert_eq!(ponko::render::AntiAliasing::default(), ponko::render::AntiAliasing::None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
#[repr(u32)]
pub enum AntiAliasing {
    /// Preserve the image (multisampling still smooths geometry edges).
    #[default]
    None = 0,
    /// Fast approximate antialiasing, one full-screen pass.
    Fxaa = 1,
    /// SMAA 1x, three full-screen passes.
    Smaa = 2,
    /// Temporal antialiasing: the camera jitters by a sub-pixel amount each
    /// frame, and the lit HDR frame (before bloom and tone mapping) is
    /// blended with last frame's, reprojected along per-pixel motion and
    /// clamped to the colors nearby, so edges and shading converge on
    /// smooth ones while still. Costs a motion pass, a resolve pass and
    /// three full-size HDR images.
    ///
    /// Moving models are tracked per entity; skinned models move with their
    /// model matrix only (limbs bending between frames can ghost slightly),
    /// and particles, transparent surfaces and [`LodGroup`](crate::render::LodGroup)s
    /// have no motion of their own (they reproject with the camera and what's
    /// behind them, and may smear a little when they move).
    /// Needs compute-capable devices: WebGL2 draws with FXAA instead. See
    /// [`Post::taa_sharpness`] and `Graphics::reset_temporal_history`.
    Taa = 3,
}

/// How a 3D frame drawn below the window's size ([`Post::render_scale`]
/// under 1) is brought up to it ([`Post::upscaler`]).
/// ```
/// use ponko::render::{AntiAliasing, Post, Upscaler};
/// assert_eq!(Upscaler::default(), Upscaler::Bilinear);
/// let mut post = Post::default();
/// // Draw two thirds of the pixels across and rebuild the rest over time.
/// (post.render_scale, post.anti_aliasing, post.upscaler) = (0.67, AntiAliasing::Taa, Upscaler::Temporal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
#[repr(u32)]
pub enum Upscaler {
    /// One pass stretches the finished frame to the window, sharpened by
    /// [`Post::sharpen`]: cheap, and as soft as the lower resolution is.
    /// Detail smaller than a drawn pixel is lost.
    #[default]
    Bilinear = 0,
    /// Temporal upscaling, built on [`AntiAliasing::Taa`]: the frame is
    /// drawn at the render size with a sub-pixel jitter that differs every
    /// frame, and each window pixel collects the samples that land near it
    /// over several frames, reprojected along the motion vectors and
    /// clipped to the colors nearby as TAA's are. A still or slowly moving
    /// picture converges on detail close to the window's resolution; what
    /// was just uncovered or moves fast starts from a smooth stretch of the
    /// drawn frame. It replaces TAA's own resolve, so it costs about what
    /// TAA does at the render size, plus two history images of the
    /// window's size. [`Post::taa_sharpness`] sharpens it.
    ///
    /// Only used with [`AntiAliasing::Taa`] and a
    /// [`render_scale`](Post::render_scale) below 1; otherwise the frame is
    /// stretched as with [`Bilinear`](Self::Bilinear) (WebGL2, which has
    /// no TAA, included). It runs after tone mapping, so bloom, depth of
    /// field and grading work at the render size, and the lens effects
    /// ([`Post::lens_distortion`], [`Post::chromatic_aberration`]) blur a
    /// little more near the edges of the picture.
    Temporal = 1,
}

/// Smallest [`Post::render_scale`]: [`Post::MIN_RENDER_SCALE`].
pub(crate) const MIN_RENDER_SCALE: f32 = 0.25;
/// Largest [`Post::render_scale`]: [`Post::MAX_RENDER_SCALE`].
pub(crate) const MAX_RENDER_SCALE: f32 = 2.0;

/// Ground-truth ambient occlusion. Radius is in world units; quality controls
/// angular slices and horizon steps. Disabled by default to preserve scenes.
/// ```
/// use ponko::render::AmbientOcclusion;
/// let ao = ponko::render::AmbientOcclusion { enabled: true, radius: 1.0, ..Default::default() };
/// assert_eq!(ao.slices, 3);
/// ```
///
/// # Defaults and cost
///
/// [`Default`] is off, with a radius of 1, an intensity of 1, 3 slices and 6
/// steps. While off it costs nothing. On, it adds a full-screen pass before
/// the opaque lighting (shared with contact shadows, [`Post::contact_shadows`]),
/// in which each pixel takes about `slices × steps × 2` depth samples (36 at
/// the default); [`Post::ao_half_resolution`] traces a quarter of the pixels
/// and [`Post::ao_temporal`] adds a history image. It needs compute shaders:
/// WebGL2 draws without it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientOcclusion {
    /// Enable screen-space occlusion of ambient light only. Ignored on
    /// WebGL2/downlevel devices without compute capabilities.
    pub enabled: bool,
    /// Maximum occluder distance in world units, default 1.
    pub radius: f32,
    /// Occlusion strength, default 1. Zero preserves ambient light.
    pub intensity: f32,
    /// Angular slices, clamped to 1..=8; default 3.
    pub slices: u32,
    /// Horizon samples in each direction, clamped to 1..=16; default 6.
    pub steps: u32,
}
impl Default for AmbientOcclusion {
    fn default() -> Self {
        Self {
            enabled: false,
            radius: 1.0,
            intensity: 1.0,
            slices: 3,
            steps: 6,
        }
    }
}

/// Split toning ([`Post::split_toning`]): the dark parts of the picture
/// take one tint and the bright parts another, as in film grading (teal
/// shadows and orange highlights, cold shadows under a warm sunset).
///
/// **Cost:** no pass of its own: about twenty instructions per pixel of the
/// tone mapping pass, and none while both tints are [`Color::GRAY`] (the
/// default, which changes nothing).
/// ```
/// use ponko::prelude::*;
/// let mut post = ponko::render::Post::default();
/// assert!(post.split_toning.is_neutral());
/// post.split_toning.shadows = Color::rgb(0.35, 0.5, 0.6); // teal
/// post.split_toning.highlights = Color::rgb(0.65, 0.5, 0.35); // orange
/// assert!(!post.split_toning.is_neutral());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitToning {
    /// The tint of the dark parts, blended in with soft light: grey (0.5)
    /// changes nothing, a brighter or more colorful one tints more. Alpha
    /// is ignored. Default [`Color::GRAY`].
    pub shadows: Color,
    /// The tint of the bright parts, the same way. Default [`Color::GRAY`].
    pub highlights: Color,
    /// Where the shadows end and the highlights begin, -1 to 1: above 0
    /// more of the picture counts as highlights, below 0 as shadows.
    /// Default 0.
    pub balance: f32,
}

impl Default for SplitToning {
    fn default() -> Self {
        Self {
            shadows: Color::GRAY,
            highlights: Color::GRAY,
            balance: 0.0,
        }
    }
}

impl SplitToning {
    /// Whether this changes nothing: both tints grey (the pass then skips
    /// it).
    #[must_use]
    pub fn is_neutral(&self) -> bool {
        let grey = |c: Color| c.r == 0.5 && c.g == 0.5 && c.b == 0.5;
        grey(self.shadows) && grey(self.highlights)
    }
}

/// Depth of field ([`Post::depth_of_field`]): what is far from the focus
/// distance blurs, as through a camera lens, with bright out-of-focus
/// spots spread into round discs. Off by default.
///
/// **Cost:** one full-screen pass at the scene's resolution while it is
/// on, before bloom: 32 color and 32 depth reads per pixel, over a disc as
/// wide as the largest blur. About 0.8 ms at 1920x1080 on an Apple M4 Max,
/// whatever the blur. It needs no image of its own: it swaps between the
/// two the screen effects swap between. Off, it costs nothing. Things in front of the focus blur over what is behind
/// them only as far as their own blur reaches, and transparent surfaces
/// blur with what is behind them (they write no depth). The depth of a
/// multisampled edge pixel is one of its samples', so the antialiased
/// edge of something sharp in front of a blurred background can leave a
/// faint fringe in it.
/// ```
/// use ponko::prelude::*;
/// let mut post = ponko::render::Post::default();
/// assert!(!post.depth_of_field.enabled);
/// post.depth_of_field.enabled = true;
/// post.depth_of_field.focus_distance = 4.0; // the person talking
/// assert_eq!(post.depth_of_field.blur, 0.01);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthOfField {
    /// Blur what is out of focus. Default false.
    pub enabled: bool,
    /// How far from the camera, in world units along its view direction,
    /// things are sharp. Default 10.
    pub focus_distance: f32,
    /// How far in front of and behind the focus distance things stay
    /// fully sharp, in world units. Default 0.
    pub focus_range: f32,
    /// The largest blur: the radius of a blurred spot, as a fraction of
    /// the scene's height (0.01 is about 11 pixels at 1080p), 0 to 0.05.
    /// Past the sharp band, blur grows as through a lens: it reaches this
    /// at twice the focus distance and at two thirds of it. Default 0.01.
    pub blur: f32,
}

impl Default for DepthOfField {
    fn default() -> Self {
        Self {
            enabled: false,
            focus_distance: 10.0,
            focus_range: 0.0,
            blur: 0.01,
        }
    }
}

impl DepthOfField {
    /// Largest [`blur`](Self::blur).
    pub const MAX_BLUR: f32 = 0.05;

    /// Whether the pass runs: enabled, with a blur above zero and finite
    /// settings.
    pub(crate) fn is_on(&self) -> bool {
        self.enabled
            && self.blur.is_finite()
            && self.blur > 0.0
            && self.focus_distance.is_finite()
            && self.focus_range.is_finite()
    }

    /// The blur, as a fraction of the scene's height, of something `depth`
    /// world units in front of the camera (what the pass computes).
    ///
    /// ```
    /// use ponko::render::DepthOfField;
    /// let dof = DepthOfField { enabled: true, focus_distance: 10.0, focus_range: 2.0, blur: 0.01 };
    /// assert_eq!(dof.blur_at(10.0), 0.0, "in focus");
    /// assert_eq!(dof.blur_at(11.0), 0.0, "inside the sharp band");
    /// assert!(dof.blur_at(30.0) > dof.blur_at(15.0), "blurrier the further out of focus");
    /// assert!(dof.blur_at(1000.0) <= dof.blur);
    /// ```
    #[must_use]
    pub fn blur_at(&self, depth: f32) -> f32 {
        if !self.is_on() {
            return 0.0;
        }
        let off = ((depth - self.focus_distance).abs() - self.focus_range.max(0.0)).max(0.0);
        let blur = self.blur.clamp(0.0, Self::MAX_BLUR);
        blur * (2.0 * off / depth.max(1e-3)).min(1.0)
    }
}

/// Auto exposure ([`Post::auto_exposure`]): the picture's exposure follows
/// its brightness, as an eye or a camera adapts. Walk from daylight into a
/// dark interior and the picture brightens over a few seconds; walk back out
/// and it darkens again. Off by default.
///
/// Each frame the lit HDR frame is metered: the average of its brightness
/// (the mean of the log luminance, so one bright lamp doesn't decide it)
/// over what the 3D frame drew (the background color the game clears to
/// doesn't count), weighted toward the middle by
/// [`center_weight`](Self::center_weight) and by the [`mask`](Self::mask).
/// The exposure that brings that average to middle grey (0.18), moved by
/// [`compensation`](Self::compensation) and held between [`min`](Self::min)
/// and [`max`](Self::max), is the target, and the exposure moves toward it
/// at most [`brighten_speed`](Self::brighten_speed) or
/// [`darken_speed`](Self::darken_speed) stops a second. The result adds to
/// [`Post::exposure`], which still applies on top (so fades and
/// [`PostVolume`]s that set it keep working), before bloom and tone mapping.
///
/// The first frame drawn with it on starts at the target, as does the
/// first after `Graphics::reset_temporal_history` (a teleport or a new
/// scene). Each camera of a split screen ([`d3::View`](crate::d3::View))
/// adapts on its own. The adapted exposure stays on the GPU: nothing is read
/// back, so the game can't read it.
///
/// **Cost:** off, nothing: no pass, no image, and one skipped branch in the
/// tone mapping pass. On, a metering pass over an eighth of the frame's
/// width and height, a few tiny passes that reduce it to one value and one
/// that adapts it (about 0.03 ms at 1920x1080 and 2560x1440 on an Apple M4
/// Max; the guide has the numbers). It works on every GPU tier, WebGL2
/// included: it uses render passes only.
/// ```
/// use ponko::render::{AutoExposure, Post};
/// let mut post = Post::default();
/// assert!(!post.auto_exposure.enabled);
/// post.auto_exposure = AutoExposure { enabled: true, max: 6.0, ..Default::default() };
/// assert_eq!((post.auto_exposure.min, post.auto_exposure.brighten_speed), (-4.0, 1.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoExposure {
    /// Follow the picture's brightness. Default false.
    pub enabled: bool,
    /// The most it darkens the picture, in stops (0 or below; -4 is a
    /// sixteenth of the light). Default -4.
    pub min: f32,
    /// The most it brightens the picture, in stops (0 or above). Default 4.
    /// A max below `min` counts as `min`.
    pub max: f32,
    /// Where it settles, in stops from middle grey: +1 keeps the picture a
    /// stop brighter than the meter says, -1 a stop darker. It is part of
    /// the target, so `min` and `max` hold it too. Default 0.
    pub compensation: f32,
    /// How fast the picture brightens (after the camera walks into the
    /// dark), in stops a second. Default 1. 0 holds the exposure where it
    /// is; `f32::INFINITY` jumps to the target at once.
    pub brighten_speed: f32,
    /// How fast the picture darkens (after the camera walks into bright
    /// light), in stops a second. Default 3, as eyes adapt to light faster
    /// than to the dark. 0 holds; `f32::INFINITY` jumps.
    pub darken_speed: f32,
    /// How much more the middle of the picture counts than its edges, 0 to
    /// 1: 0 meters the whole picture evenly, 1 leaves the corners out.
    /// Default 0.5.
    pub center_weight: f32,
    /// A metering mask: a texture stretched over the picture whose red
    /// channel says how much each part counts (white fully, black not at
    /// all), on top of `center_weight`. For example a white disc in the
    /// middle for spot metering, or black under the HUD's panel. Default
    /// none.
    pub mask: Option<Texture>,
}

impl Default for AutoExposure {
    fn default() -> Self {
        Self {
            enabled: false,
            min: -4.0,
            max: 4.0,
            compensation: 0.0,
            brighten_speed: 1.0,
            darken_speed: 3.0,
            center_weight: 0.5,
            mask: None,
        }
    }
}

impl AutoExposure {
    /// The exposure, in stops added to [`Post::exposure`], that auto
    /// exposure settles at for a picture whose metered average luminance
    /// (the weighted mean of its log luminance, as a luminance) is
    /// `average`: what brings it to middle grey, moved by the compensation
    /// and held between `min` and `max`. What the GPU computes, for
    /// working out settings.
    /// ```
    /// use ponko::render::AutoExposure;
    /// let auto = AutoExposure { enabled: true, ..Default::default() };
    /// assert_eq!(auto.target(0.18), 0.0, "middle grey already");
    /// assert!((auto.target(0.045) - 2.0).abs() < 1e-5, "two stops too dark: brightened by two");
    /// assert_eq!(auto.target(0.001), 4.0, "held at max");
    /// let brighter = AutoExposure { compensation: 1.0, ..auto };
    /// assert_eq!(brighter.target(0.18), 1.0);
    /// ```
    #[must_use]
    pub fn target(&self, average: f32) -> f32 {
        let [min, max, compensation, _] = self.meter();
        let metered = average.max(f32::MIN_POSITIVE).log2().clamp(-METER_STOPS, METER_STOPS);
        (MIDDLE_GREY.log2() - metered + compensation).clamp(min, max)
    }

    /// `min`, `max`, `compensation` and `center_weight` as the GPU reads
    /// them: a non-finite value is the default, the weight clamped to 0..1
    /// and `max` at least `min`.
    fn meter(&self) -> [f32; 4] {
        let or = |v: f32, d: f32| if v.is_finite() { v } else { d };
        let d = AutoExposure::default();
        let min = or(self.min, d.min);
        let max = or(self.max, d.max).max(min);
        [
            min,
            max,
            or(self.compensation, 0.0),
            or(self.center_weight, d.center_weight).clamp(0.0, 1.0),
        ]
    }

    /// How far the exposure may move in a frame `dt` seconds long, in
    /// stops: brightening and darkening (a negative or NaN speed holds,
    /// an infinite one jumps).
    fn steps(&self, dt: f32) -> [f32; 2] {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        let step = |speed: f32| {
            if speed == f32::INFINITY {
                f32::MAX
            } else if speed.is_nan() || speed <= 0.0 {
                0.0
            } else {
                (speed * dt).min(f32::MAX)
            }
        };
        [step(self.brighten_speed), step(self.darken_speed)]
    }
}

/// Middle grey: what auto exposure brings the metered average to.
pub(crate) const MIDDLE_GREY: f32 = 0.18;
/// The metered log luminance is held within this many stops of 1 (in the
/// shader too), so a black or blinding frame still meters finitely.
pub(crate) const METER_STOPS: f32 = 16.0;

/// What happens to the lit 3D frame before it reaches the screen: the
/// resolution it is drawn at, antialiasing, ambient occlusion, reflections,
/// depth of field, exposure, bloom, color grading and tone mapping. At
/// [`SceneSettings::post`](crate::render::SceneSettings::post)
/// (`ctx.scene3d.post`). Sprites and UI drawn on top are not changed.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// ctx.scene3d.post.exposure = 0.5; // half a stop brighter
/// ctx.scene3d.post.bloom = 0.15;
/// ctx.scene3d.post.tonemap = ponko::render::Tonemap::Neutral;
/// ctx.scene3d.post.temperature = 0.2; // warmer
/// # }
/// ```
///
/// Two looks blend with [`Post::lerp`], and [`PostVolume`]s blend them by
/// where the camera is.
///
/// # Defaults and cost
///
/// [`Default`] draws at the window's resolution with 4x multisampling and
/// no post antialiasing, ACES tone mapping, a bloom of 0.05, and ambient
/// occlusion, contact shadows, reflections, depth of field, auto exposure,
/// vignette and the lens effects off; exposure, contrast, saturation, temperature and tint are
/// neutral. What is off costs nothing. What is on adds work, as each field
/// says: a pass for a `render_scale` other than 1, one full-screen pass for
/// FXAA and three for SMAA, a motion pass, a resolve pass and three
/// full-size HDR images for TAA, a full-screen pass for ambient occlusion
/// and contact shadows together, a half-resolution pass for reflections,
/// and multisampling at 2 instead of 4 saves about a third of the GPU time on
/// a large screen. On WebGL2 the compute-based ones (TAA, ambient occlusion,
/// contact shadows, reflections) are off.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Post {
    /// The 3D scene is drawn at this fraction of the window's size in each
    /// direction, then scaled to the window before sprites and UI, which stay
    /// at the window's resolution: below 1 is cheaper and softer, above 1
    /// (supersampling) sharper and costlier. From [`Post::MIN_RENDER_SCALE`]
    /// to [`Post::MAX_RENDER_SCALE`]; anything else is clamped. Default 1
    /// (no extra pass).
    pub render_scale: f32,
    /// How much the scaled image is sharpened, 0 to 1 (an adaptive
    /// contrast filter that only touches soft edges). Only used with a
    /// `render_scale` other than 1. Default 0.
    pub sharpen: f32,
    /// How a `render_scale` below 1 is brought up to the window's size:
    /// stretched ([`Upscaler::Bilinear`], the default) or rebuilt over
    /// frames ([`Upscaler::Temporal`], with [`AntiAliasing::Taa`]).
    pub upscaler: Upscaler,
    /// Ambient-only GTAO, before opaque lighting.
    pub ambient_occlusion: AmbientOcclusion,
    /// Trace GTAO at half width and height, then bilaterally upsample.
    /// Contact shadows remain full resolution. Default false. Only matters
    /// where ambient occlusion runs (not on WebGL2).
    pub ao_half_resolution: bool,
    /// Accumulate GTAO over frames with rotating slices, following the
    /// camera and moving models (the same motion vectors as TAA) and
    /// rejecting history whose depth disagrees. Independent of TAA. Default
    /// false; enable to trade history cost for fewer samples. Only matters
    /// where ambient occlusion runs (not on WebGL2).
    pub ao_temporal: bool,
    /// Contact shadows: how far, in world units, each pixel looks toward
    /// the sun across the depth buffer for something close that blocks
    /// it. They catch the small shadows the shadow map is too coarse for:
    /// under feet, pebbles and cups, in creases. 0 (the default) is off;
    /// 0.3 suits people and props. Only things on screen cast them. Shares
    /// a full-screen pass with [`AmbientOcclusion`] (one pass for both).
    /// **WebGL2:** off whatever the value, as ambient occlusion is: it needs
    /// compute shaders.
    pub contact_shadows: f32,
    /// Screen-space reflections: how strongly glossy surfaces (roughness
    /// below about 0.7, fully below 0.4) reflect what's on screen, 0 to 1,
    /// on top of the sky and reflection probes they reflect anyway. Where
    /// the reflected ray finds nothing on screen (it leaves the screen, goes
    /// behind something, or runs past
    /// [`ssr_max_distance`](Self::ssr_max_distance)), the environment's
    /// reflection stays. Wet streets, polished floors, still water. Things
    /// off screen or hidden aren't reflected, and normal maps don't bend the
    /// reflection. The colors come from the last frame's image, so the
    /// first frame after a camera cut reflects only the environment. A
    /// half-resolution pass after the depth prepass; [`AntiAliasing::Taa`]
    /// smooths its grain. The blur follows the cone a rough surface
    /// reflects: it grows with roughness and with how far the reflected
    /// ray went, so a contact reflection stays sharp on a rough floor
    /// while a distant one blurs (depth-aware filtered mips, up to about 32
    /// pixels). 0 (the default) is off. **WebGL2:** off whatever the value:
    /// it needs compute shaders.
    /// ```
    /// use ponko::prelude::*;
    /// assert_eq!(ponko::render::Post::default().ssr, 0.0);
    /// let mut wet_street = ponko::render::Post::default();
    /// wet_street.ssr = 1.0;
    /// assert_eq!(wet_street.ssr_max_distance, 30.0);
    /// ```
    pub ssr: f32,
    /// How far, in world units, a screen-space reflection looks along the
    /// reflected ray; it fades out toward the end. Default 30.
    pub ssr_max_distance: f32,
    /// Screen-space reflections keep thin things whole without
    /// [`AntiAliasing::Taa`]: rails, cables, poles and window frames near
    /// the camera. Far along a reflected ray the trace takes steps tens of
    /// pixels long and jumps over such things now and then, so without TAA
    /// to fill the gaps their reflections come out dotted and dimmer. With
    /// this on, the trace walks a coarse copy of the depth instead (the
    /// nearest depth of 8 to 64 pixel blocks): it skips blocks the ray stays
    /// in front of and reads the depth closely where something could be in
    /// front of it. It costs about 0.2 ms more at 2560x1440 on a fast GPU
    /// (the guide's reflections section has the numbers), and is off by
    /// default and on in [`GraphicsQuality::High`](crate::render::GraphicsQuality::High)
    /// and [`GraphicsQuality::Ultra`](crate::render::GraphicsQuality::Ultra).
    /// With TAA the gaps are filled anyway. Off, the reflections are
    /// exactly as without the setting. Only matters where [`ssr`](Self::ssr)
    /// is on.
    /// ```
    /// let mut post = ponko::render::Post::default();
    /// assert!(!post.ssr_thin_objects);
    /// (post.ssr, post.ssr_thin_objects) = (1.0, true); // a railing over a polished floor
    /// ```
    pub ssr_thin_objects: bool,
    /// Post antialiasing. Default off to preserve existing scenes.
    pub anti_aliasing: AntiAliasing,
    /// Samples per pixel of the 3D frame (multisampling), smoothing the
    /// edges of what is drawn: 4 (the default) or 2, which costs about a
    /// third less GPU time on a large screen, with a little more
    /// stair-stepping on thin edges. Not part of the quality presets.
    ///
    /// Set in a setup callback, it applies right after that callback, before
    /// the first frame. Set before any 3D asset is loaded (in its own, earlier
    /// callback or a plugin), the 3D pipelines are built only once, with it;
    /// otherwise, and for a change while the game runs, the 3D targets and
    /// pipelines are made again (meshes, materials and shaders stay) and
    /// temporal history restarts.
    ///
    /// Only 2 and 4 are counts: any other value keeps the current one. GPUs
    /// that can't take 2 keep 4, and a browser's WebGL2 has no multisampling
    /// at all, so turn on FXAA or SMAA there. A count that isn't used is
    /// said once on stderr and is the Multisampling fallback in
    /// [`GpuCapabilities`](crate::render::GpuCapabilities).
    ///
    /// With several cameras ([`d3::View`](crate::d3::View)) the main
    /// camera's count applies to every view, and a view's own `msaa` is
    /// ignored: every 3D pipeline is built for the renderer's one sample
    /// count, and a count per view would build and keep a second set of them.
    /// ```no_run
    /// # use ponko::prelude::*;
    /// Game::new("game", 1280, 720)
    ///     .setup(|ctx| ctx.scene3d.post.msaa = 2) // before the rest of the setup
    ///     .setup(|ctx| {
    ///         // load models ...
    ///     })
    ///     .run();
    /// ```
    pub msaa: u32,
    /// How much [`AntiAliasing::Taa`] sharpens the result (it softens the
    /// image a little), 0 to 1. Default 0.25.
    pub taa_sharpness: f32,
    /// Brightness in stops: +1 doubles the light, -1 halves it. Default 0.
    /// With [`auto_exposure`](Self::auto_exposure) on, it still applies, on
    /// top of the adapted exposure.
    pub exposure: f32,
    /// Exposure that follows the picture's brightness, as eyes adapt.
    /// Default off.
    pub auto_exposure: AutoExposure,
    /// How light maps to the screen. Default [`Tonemap::Aces`].
    pub tonemap: Tonemap,
    /// How strongly bright light glows into its surroundings; 0 turns bloom
    /// off. Default 0.05.
    pub bloom: f32,
    /// How bright light must be to glow (1 is white). Default 1.
    pub bloom_threshold: f32,
    /// 1 keeps the contrast, higher is punchier, lower flatter. Default 1.
    pub contrast: f32,
    /// 1 keeps the colors, 0 is grey, higher is more vivid. Default 1.
    pub saturation: f32,
    /// White balance, -1 (cool, blue) to 1 (warm, orange). Default 0.
    pub temperature: f32,
    /// White balance, -1 (green) to 1 (magenta). Default 0.
    pub tint: f32,
    /// Tints the shadows and the highlights apart. Default neutral (off).
    pub split_toning: SplitToning,
    /// Blurs what is out of focus. Default off.
    pub depth_of_field: DepthOfField,
    /// How much the corners darken, 0 to 1. Default 0.
    pub vignette: f32,
    /// Color fringes toward the edges, as a cheap lens splits red, green
    /// and blue apart, 0 to 1. Default 0.
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// assert_eq!(ponko::render::Post::default().chromatic_aberration, 0.0);
    /// let speed = &mut ctx.scene3d.post;
    /// (speed.chromatic_aberration, speed.lens_distortion, speed.lens_scale) = (0.8, -0.26, 1.04);
    /// # }
    /// ```
    pub chromatic_aberration: f32,
    /// Bends the picture as a lens would, -1 to 1: above 0 bulges the
    /// middle out (barrel), below 0 pinches it in (pincushion). Default 0.
    pub lens_distortion: f32,
    /// Zooms the picture in (above 1) or out after the distortion, to hide
    /// the edges it pulls in. Above zero; default 1.
    pub lens_scale: f32,
    /// A color lookup table for grading, applied after tone mapping: a
    /// strip of `n` squares of `n` x `n` pixels (for example 1024 x 32),
    /// red across each square, green down it, blue from square to square.
    /// A neutral one changes nothing; paint over it in an image editor to
    /// grade. Default none.
    pub lut: Option<Texture>,
}

impl Default for Post {
    fn default() -> Self {
        Self {
            render_scale: 1.0,
            sharpen: 0.0,
            upscaler: Upscaler::Bilinear,
            ambient_occlusion: AmbientOcclusion::default(),
            ao_half_resolution: false,
            ao_temporal: false,
            contact_shadows: 0.0,
            ssr: 0.0,
            ssr_max_distance: 30.0,
            ssr_thin_objects: false,
            anti_aliasing: AntiAliasing::None,
            msaa: 4,
            taa_sharpness: 0.25,
            exposure: 0.0,
            auto_exposure: AutoExposure::default(),
            tonemap: Tonemap::Aces,
            bloom: 0.05,
            bloom_threshold: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            temperature: 0.0,
            tint: 0.0,
            split_toning: SplitToning::default(),
            depth_of_field: DepthOfField::default(),
            vignette: 0.0,
            chromatic_aberration: 0.0,
            lens_distortion: 0.0,
            lens_scale: 1.0,
            lut: None,
        }
    }
}

impl Post {
    /// Smallest [`render_scale`](Self::render_scale).
    /// ```
    /// assert_eq!(ponko::render::Post::MIN_RENDER_SCALE, 0.25);
    /// ```
    pub const MIN_RENDER_SCALE: f32 = MIN_RENDER_SCALE;
    /// Largest [`render_scale`](Self::render_scale).
    /// ```
    /// assert_eq!(ponko::render::Post::MAX_RENDER_SCALE, 2.0);
    /// ```
    pub const MAX_RENDER_SCALE: f32 = MAX_RENDER_SCALE;

    /// The size the 3D scene is drawn at in a window of `window` pixels:
    /// scaled by [`render_scale`](Self::render_scale) (clamped; a NaN counts
    /// as 1), at least 1 pixel and at most `limit` wide (0: no limit).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// let mut post = ponko::render::Post::default();
    /// post.render_scale = 0.5;
    /// assert_eq!(post.render_size((1920, 1080)), (960, 540));
    /// ```
    #[must_use]
    pub fn render_size(&self, window: (u32, u32)) -> (u32, u32) {
        scaled_size(window, self.render_scale, 0)
    }

    /// A neutral lookup table of `n` x `n` x `n` colors as RGBA pixels,
    /// `n * n` wide and `n` high: the starting point for a [`Post::lut`].
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// let (w, h, pixels) = ponko::render::Post::neutral_lut(32);
    /// ctx.scene3d.post.lut = Some(ctx.gfx.create_texture(w, h, &pixels));
    /// # }
    /// ```
    ///
    /// # Panics
    ///
    /// If `n` is not between 2 and 64.
    pub fn neutral_lut(n: u32) -> (u32, u32, Vec<u8>) {
        assert!(
            (2..=64).contains(&n),
            "a lookup table has 2 to 64 steps per color, not {n}"
        );
        let step = |i: u32| (i * 255 + (n - 1) / 2) / (n - 1);
        let mut pixels = Vec::with_capacity((n * n * n * 4) as usize);
        for g in 0..n {
            for b in 0..n {
                for r in 0..n {
                    pixels.extend_from_slice(&[step(r) as u8, step(g) as u8, step(b) as u8, 255]);
                }
            }
        }
        (n * n, n, pixels)
    }

    /// The settings `t` of the way from `self` to `other` (clamped to 0 to
    /// 1; a NaN counts as 0): `self` at 0, `other` at 1. For a transition
    /// between two looks (indoors to outdoors, day to night, under water),
    /// or through [`PostVolume`]s.
    ///
    /// Amounts and colors blend: exposure, bloom, grading, white balance,
    /// split toning, vignette, lens effects, sharpening, contact shadows
    /// and reflections. An effect on at one end and off at the other
    /// (ambient occlusion, depth of field, auto exposure) runs through the
    /// blend with its strength fading to nothing at the end where it is off
    /// (for auto exposure: its `min` and `max` closing in on 0 stops), and
    /// its other settings from the end where it is on. Choices that can't be mixed
    /// switch halfway: the tone map, the lookup table, antialiasing,
    /// multisampling, the render scale (blending it would make the scene's
    /// images again every frame), the ambient occlusion's quality and
    /// whether reflections trace thin objects.
    /// ```
    /// use ponko::render::Post;
    /// let (mut day, mut night) = (Post::default(), Post::default());
    /// night.exposure = -1.0;
    /// night.saturation = 0.6;
    /// day.bloom = 0.0;
    /// let dusk = day.lerp(&night, 0.25);
    /// assert_eq!(dusk.exposure, -0.25);
    /// assert!((dusk.saturation - 0.9).abs() < 1e-6);
    /// assert_eq!(day.lerp(&night, 0.0), day);
    /// assert_eq!(day.lerp(&night, 1.0), night);
    /// ```
    #[must_use]
    pub fn lerp(&self, other: &Post, t: f32) -> Post {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        if t == 0.0 {
            return *self;
        }
        if t == 1.0 {
            return *other;
        }
        let f = |a: f32, b: f32| a + (b - a) * t;
        // Choices that can't be mixed: `self`'s until halfway, then `other`'s.
        fn pick<T>(t: f32, a: T, b: T) -> T {
            if t < 0.5 { a } else { b }
        }
        let color = |a: Color, b: Color| Color {
            r: f(a.r, b.r),
            g: f(a.g, b.g),
            b: f(a.b, b.b),
            a: f(a.a, b.a),
        };
        let (a, b) = (self, other);
        let ao = {
            let (x, y) = (&a.ambient_occlusion, &b.ambient_occlusion);
            // An end that is off counts as no occlusion, with the other's settings.
            let (from, to) = match (x.enabled, y.enabled) {
                (true, false) => (x, x),
                (false, true) => (y, y),
                _ => (x, y),
            };
            let strength = |o: &AmbientOcclusion| if o.enabled { o.intensity } else { 0.0 };
            AmbientOcclusion {
                enabled: x.enabled || y.enabled,
                radius: f(from.radius, to.radius),
                intensity: f(strength(x), strength(y)),
                slices: pick(t, from.slices, to.slices),
                steps: pick(t, from.steps, to.steps),
            }
        };
        let dof = {
            let (x, y) = (&a.depth_of_field, &b.depth_of_field);
            let (from, to) = match (x.enabled, y.enabled) {
                (true, false) => (x, x),
                (false, true) => (y, y),
                _ => (x, y),
            };
            let blur = |d: &DepthOfField| if d.enabled { d.blur } else { 0.0 };
            DepthOfField {
                enabled: x.enabled || y.enabled,
                focus_distance: f(from.focus_distance, to.focus_distance),
                focus_range: f(from.focus_range, to.focus_range),
                blur: f(blur(x), blur(y)),
            }
        };
        let auto_exposure = {
            let (x, y) = (&a.auto_exposure, &b.auto_exposure);
            // An end that is off adds no exposure: its range is 0 stops.
            let off = |o: &AutoExposure| AutoExposure {
                min: 0.0,
                max: 0.0,
                ..*o
            };
            // An infinite speed (a jump) doesn't blend: it switches halfway.
            let speed = |a: f32, b: f32| {
                if a.is_finite() && b.is_finite() {
                    f(a, b)
                } else {
                    pick(t, a, b)
                }
            };
            let (from, to) = match (x.enabled, y.enabled) {
                (true, false) => (*x, off(x)),
                (false, true) => (off(y), *y),
                _ => (*x, *y),
            };
            AutoExposure {
                enabled: x.enabled || y.enabled,
                min: f(from.min, to.min),
                max: f(from.max, to.max),
                compensation: f(from.compensation, to.compensation),
                brighten_speed: speed(from.brighten_speed, to.brighten_speed),
                darken_speed: speed(from.darken_speed, to.darken_speed),
                center_weight: f(from.center_weight, to.center_weight),
                mask: pick(t, from.mask, to.mask),
            }
        };
        Post {
            render_scale: pick(t, a.render_scale, b.render_scale),
            sharpen: f(a.sharpen, b.sharpen),
            upscaler: pick(t, a.upscaler, b.upscaler),
            ambient_occlusion: ao,
            ao_half_resolution: pick(t, a.ao_half_resolution, b.ao_half_resolution),
            ao_temporal: pick(t, a.ao_temporal, b.ao_temporal),
            contact_shadows: f(a.contact_shadows, b.contact_shadows),
            ssr: f(a.ssr, b.ssr),
            ssr_max_distance: f(a.ssr_max_distance, b.ssr_max_distance),
            ssr_thin_objects: pick(t, a.ssr_thin_objects, b.ssr_thin_objects),
            anti_aliasing: pick(t, a.anti_aliasing, b.anti_aliasing),
            msaa: pick(t, a.msaa, b.msaa),
            taa_sharpness: f(a.taa_sharpness, b.taa_sharpness),
            exposure: f(a.exposure, b.exposure),
            auto_exposure,
            tonemap: pick(t, a.tonemap, b.tonemap),
            bloom: f(a.bloom, b.bloom),
            bloom_threshold: f(a.bloom_threshold, b.bloom_threshold),
            contrast: f(a.contrast, b.contrast),
            saturation: f(a.saturation, b.saturation),
            temperature: f(a.temperature, b.temperature),
            tint: f(a.tint, b.tint),
            split_toning: SplitToning {
                shadows: color(a.split_toning.shadows, b.split_toning.shadows),
                highlights: color(a.split_toning.highlights, b.split_toning.highlights),
                balance: f(a.split_toning.balance, b.split_toning.balance),
            },
            depth_of_field: dof,
            vignette: f(a.vignette, b.vignette),
            chromatic_aberration: f(a.chromatic_aberration, b.chromatic_aberration),
            lens_distortion: f(a.lens_distortion, b.lens_distortion),
            lens_scale: f(a.lens_scale, b.lens_scale),
            lut: pick(t, a.lut, b.lut),
        }
    }
}

/// A box in the world with post settings of its own: the camera inside it
/// sees them, and [`blend_distance`](Self::blend_distance) outside it they
/// fade back into what is around it. For places that look different (a
/// cave, a building, under water, a fog bank) without blending settings
/// field by field in game code.
///
/// Volumes are plain values: keep them where suits the game (a resource,
/// a level's data) and blend them into `ctx.scene3d.post` each frame with
/// [`PostVolume::blend`]. A volume that doesn't touch the camera costs a
/// distance check.
/// ```
/// use ponko::prelude::*;
/// use ponko::render::{Post, PostVolume};
/// let outdoors = Post::default();
/// let mut cave = Post::default();
/// cave.exposure = 1.0;
/// cave.saturation = 0.5;
/// let volumes = [PostVolume::new(vec3(0.0, 2.0, -20.0), vec3(10.0, 4.0, 10.0), 4.0, cave)];
/// // Deep inside: the cave's look. Far away: the outdoors'. Halfway
/// // through the blend distance: halfway between.
/// assert_eq!(PostVolume::blend(&outdoors, &volumes, vec3(0.0, 2.0, -20.0)).exposure, 1.0);
/// assert_eq!(PostVolume::blend(&outdoors, &volumes, vec3(0.0, 2.0, 20.0)).exposure, 0.0);
/// let edge = PostVolume::blend(&outdoors, &volumes, vec3(0.0, 2.0, -13.0));
/// assert!((edge.exposure - 0.5).abs() < 1e-6);
/// ```
///
/// In a game, every frame (after the camera moved):
/// ```no_run
/// # use ponko::prelude::*;
/// # use ponko::render::{Post, PostVolume};
/// # struct Looks { outdoors: Post, volumes: Vec<PostVolume> }
/// # fn f(ctx: &mut Ctx, looks: &Looks) {
/// let camera = ctx.scene3d.camera.position;
/// ctx.scene3d.post = PostVolume::blend(&looks.outdoors, &looks.volumes, camera);
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostVolume {
    /// The box's corner with the smallest coordinates.
    pub min: Vec3,
    /// The box's corner with the largest coordinates.
    pub max: Vec3,
    /// How far outside the box, in world units, its settings fade out,
    /// smoothly. 0 switches at the box's faces.
    pub blend_distance: f32,
    /// The settings inside the box.
    pub post: Post,
}

impl PostVolume {
    /// A box of `size` around `center`, fading out over `blend_distance`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::render::{Post, PostVolume};
    /// let mut dim = Post::default();
    /// dim.exposure = 1.0;
    /// let cave = PostVolume::new(vec3(0.0, 2.0, -30.0), vec3(20.0, 6.0, 20.0), 5.0, dim);
    /// assert_eq!(cave.min, vec3(-10.0, -1.0, -40.0));
    /// assert_eq!(cave.max, vec3(10.0, 5.0, -20.0));
    /// ```
    #[must_use]
    pub fn new(center: Vec3, size: Vec3, blend_distance: f32, post: Post) -> Self {
        let half = size.abs() * 0.5;
        Self {
            min: center - half,
            max: center + half,
            blend_distance,
            post,
        }
    }

    /// How much this volume's settings count at `at`: 1 inside the box,
    /// falling smoothly to 0 at [`blend_distance`](Self::blend_distance)
    /// from it.
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::render::{Post, PostVolume};
    /// let v = PostVolume::new(Vec3::ZERO, Vec3::splat(2.0), 2.0, Post::default());
    /// assert_eq!(v.weight(vec3(0.5, 0.0, 0.0)), 1.0);
    /// assert_eq!(v.weight(vec3(2.0, 0.0, 0.0)), 0.5);
    /// assert_eq!(v.weight(vec3(3.0, 0.0, 0.0)), 0.0);
    /// ```
    #[must_use]
    pub fn weight(&self, at: Vec3) -> f32 {
        let outside = (self.min - at).max(at - self.max).max(Vec3::ZERO).length();
        if outside <= 0.0 {
            return 1.0;
        }
        let blend = self.blend_distance;
        if !(blend.is_finite() && blend > 0.0) || outside >= blend {
            return 0.0;
        }
        let x = 1.0 - outside / blend;
        x * x * (3.0 - 2.0 * x)
    }

    /// `base` with `volumes` blended over it by their [`weight`](Self::weight)
    /// at `at` (the camera's position), in order: where volumes overlap,
    /// the later one wins, so list a room inside a building after the
    /// building. See [`Post::lerp`] for how settings blend.
    #[must_use]
    pub fn blend(base: &Post, volumes: &[PostVolume], at: Vec3) -> Post {
        volumes
            .iter()
            .fold(*base, |post, volume| post.lerp(&volume.post, volume.weight(at)))
    }
}

/// `window` scaled by `scale` (clamped to the allowed range), at least 1 and
/// at most `limit` pixels per side (0: no limit).
pub(crate) fn scaled_size(window: (u32, u32), scale: f32, limit: u32) -> (u32, u32) {
    let scale = if scale.is_nan() {
        1.0
    } else {
        scale.clamp(MIN_RENDER_SCALE, MAX_RENDER_SCALE)
    };
    let limit = if limit == 0 { u32::MAX } else { limit };
    let side = |v: u32| ((v as f32 * scale).round() as u32).clamp(1, limit.max(1));
    (side(window.0), side(window.1))
}

/// [`Post`] as the post shader reads it.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct PostUniform {
    /// x: exposure multiplier, y: bloom, z: contrast, w: saturation.
    pub grade: [f32; 4],
    /// White balance multipliers in LMS space; w: vignette.
    pub balance: [f32; 4],
    /// x: tone map, y: lookup table size (0 for none), z: bloom on (1/0),
    /// w: HDR output (1/0; see [`hdr`](Self::hdr)).
    pub mode: [u32; 4],
    /// x: bloom threshold, y: soft knee.
    pub bloom: [f32; 4],
    /// For the scaling pass: one output pixel in uv (x, y), the sharpness
    /// (z) and the scene's size over the output's (w).
    pub output: [f32; 4],
    /// Lens distortion: x theta (or its inverse when pinching), y sigma,
    /// z one over the scale, w 1 barrel, -1 pincushion, 0 off. See [`lens`].
    pub lens: [f32; 4],
    /// x: chromatic aberration amount (0 off).
    pub fringe: [f32; 4],
    /// Split toning: the shadows' tint (rgb) and the balance (w).
    pub split_shadows: [f32; 4],
    /// Split toning: the highlights' tint (rgb), and w 1 when on (0 off).
    pub split_highlights: [f32; 4],
    /// Depth of field: x focus distance, y sharp range, z largest blur
    /// (fraction of the scene's height; 0 off).
    pub dof: [f32; 4],
    /// The camera's near (x) and far (y) planes, to turn depth into
    /// distance; z: with HDR output (`mode.w` 1), the brightest the frame
    /// may be over SDR white.
    pub camera: [f32; 4],
    /// Auto exposure's meter: x min, y max (stops), z compensation, w
    /// center weight.
    pub meter: [f32; 4],
    /// Auto exposure's adaptation this frame: the most it may brighten (x)
    /// and darken (y), in stops.
    pub adapt: [f32; 4],
    /// Auto exposure: x on (1/0), y jump to the target this frame (1/0).
    pub auto: [u32; 4],
}

/// The lens distortion terms, as Unity's and other engines' "lens
/// distortion" effects compute them: an angle `theta` that grows with the
/// strength (capped at 160 degrees), and `sigma = 2 tan(theta / 2)`.
fn lens(post: &Post) -> [f32; 4] {
    let intensity = post.lens_distortion;
    if !intensity.is_finite() || intensity == 0.0 {
        return [0.0, 0.0, 1.0, 0.0];
    }
    let intensity = intensity.clamp(-1.0, 1.0);
    let amount = 1.6 * (intensity * 100.0).abs().max(1.0);
    let theta = amount.min(160.0).to_radians();
    let sigma = 2.0 * (theta * 0.5).tan();
    let scale = if post.lens_scale.is_finite() && post.lens_scale > 0.0 {
        post.lens_scale
    } else {
        1.0
    };
    [
        if intensity > 0.0 { theta } else { 1.0 / theta },
        sigma,
        1.0 / scale,
        if intensity > 0.0 { 1.0 } else { -1.0 },
    ]
}

impl PostUniform {
    pub fn new(post: &Post, lut_size: Option<u32>) -> Self {
        let [l, m, s] = white_balance(post.temperature, post.tint);
        let bloom = post.bloom.max(0.0);
        let split = &post.split_toning;
        let tint = |c: Color| {
            let v = |x: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.5 };
            [v(c.r), v(c.g), v(c.b)]
        };
        let [sr, sg, sb] = tint(split.shadows);
        let [hr, hg, hb] = tint(split.highlights);
        let balance = if split.balance.is_finite() {
            split.balance.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let dof = &post.depth_of_field;
        Self {
            grade: [
                post.exposure.exp2(),
                bloom,
                post.contrast.max(0.0),
                post.saturation.max(0.0),
            ],
            balance: [l, m, s, post.vignette.clamp(0.0, 1.0)],
            mode: [post.tonemap as u32, lut_size.unwrap_or(0), u32::from(bloom > 0.0), 0],
            bloom: [post.bloom_threshold.max(0.0), 0.5, 0.0, 0.0],
            output: [0.0; 4],
            lens: lens(post),
            fringe: [
                if post.chromatic_aberration.is_finite() {
                    post.chromatic_aberration.clamp(0.0, 1.0) * 0.05
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ],
            split_shadows: [sr, sg, sb, balance],
            split_highlights: [hr, hg, hb, if split.is_neutral() { 0.0 } else { 1.0 }],
            dof: if dof.is_on() {
                [
                    dof.focus_distance.max(0.0),
                    dof.focus_range.max(0.0),
                    dof.blur.clamp(0.0, DepthOfField::MAX_BLUR),
                    0.0,
                ]
            } else {
                [0.0; 4]
            },
            camera: [0.1, 1000.0, 0.0, 0.0],
            meter: if post.auto_exposure.enabled {
                post.auto_exposure.meter()
            } else {
                [0.0; 4]
            },
            adapt: [0.0; 4],
            auto: [u32::from(post.auto_exposure.enabled), 0, 0, 0],
        }
    }

    /// The same, for a frame `dt` seconds after the last one this camera
    /// drew: how far auto exposure may move, or with `jump` (its first
    /// frame, or after a reset) that it starts at the target.
    pub fn adapt(mut self, post: &Post, dt: f32, jump: bool) -> Self {
        if self.auto[0] != 0 {
            let [brighten, darken] = post.auto_exposure.steps(dt);
            self.adapt = [brighten, darken, 0.0, 0.0];
            self.auto[1] = u32::from(jump);
        }
        self
    }

    /// The same, for a camera with these near and far planes (depth of
    /// field reads distances from the depth buffer with them).
    pub fn camera(mut self, near: f32, far: f32) -> Self {
        self.camera = [near, far, 0.0, 0.0];
        self
    }

    /// The same, for HDR output reaching `headroom` times SDR white:
    /// the tone mapped highlights open up to it, and the scaling pass
    /// clamps to it instead of to white. `None` (or a headroom of 1 or less)
    /// keeps the SDR frame exactly as it was.
    pub fn hdr(mut self, headroom: Option<f32>) -> Self {
        if let Some(h) = headroom.filter(|h| h.is_finite() && *h > 1.0) {
            self.mode[3] = 1;
            self.camera[2] = h;
        }
        self
    }

    /// The same, for a scene drawn at `scene` pixels and scaled to
    /// `output` ones.
    pub fn scaled(mut self, post: &Post, scene: (u32, u32), output: (u32, u32)) -> Self {
        let sharpen = if post.sharpen.is_nan() {
            0.0
        } else {
            post.sharpen.clamp(0.0, 1.0)
        };
        self.output = [
            1.0 / output.0.max(1) as f32,
            1.0 / output.1.max(1) as f32,
            sharpen,
            scene.0 as f32 / output.0.max(1) as f32,
        ];
        self
    }
}

/// Multipliers in LMS (cone response) space that move the white point, as
/// Unity and others do it: shift along the daylight curve by `temperature`,
/// across it by `tint`.
fn white_balance(temperature: f32, tint: f32) -> [f32; 3] {
    let t1 = temperature.clamp(-1.0, 1.0) * 100.0 / 65.0;
    let t2 = tint.clamp(-1.0, 1.0) * 100.0 / 65.0;
    let x = 0.31271 - t1 * if t1 < 0.0 { 0.1 } else { 0.05 };
    let y = 2.87 * x - 3.0 * x * x - 0.275_095_07 + t2 * 0.05;
    let big_x = x / y;
    let big_z = (1.0 - x - y) / y;
    let lms = [
        0.7328 * big_x + 0.4296 - 0.1624 * big_z,
        -0.7036 * big_x + 1.6975 + 0.0061 * big_z,
        0.0030 * big_x + 0.0136 + 0.9834 * big_z,
    ];
    let d65 = [0.949_237, 1.035_42, 1.087_28];
    [d65[0] / lms[0], d65[1] / lms[1], d65[2] / lms[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msaa_is_a_post_setting_that_headless_runs_keep() {
        crate::game::Game::new("samples", 32, 32).run_headless(2, |ctx| {
            assert_eq!(ctx.scene3d.post.msaa, 4, "the default");
            // Without a GPU nothing is switched; any count, even one no GPU
            // takes, is kept as the setting and ignored.
            for samples in [0, 1, 2, 8] {
                ctx.scene3d.post.msaa = samples;
                assert_eq!(ctx.scene3d.post.msaa, samples);
            }
        });
    }

    #[test]
    fn scaled_sizes_round_clamp_and_never_reach_zero() {
        assert_eq!(scaled_size((1920, 1080), 1.0, 0), (1920, 1080));
        assert_eq!(scaled_size((1920, 1080), 0.5, 0), (960, 540));
        assert_eq!(scaled_size((1921, 1081), 0.5, 0), (961, 541));
        assert_eq!(scaled_size((100, 100), 0.01, 0), (25, 25), "clamped to the minimum");
        assert_eq!(scaled_size((100, 100), 9.0, 0), (200, 200), "clamped to the maximum");
        assert_eq!(scaled_size((100, 100), f32::NAN, 0), (100, 100));
        assert_eq!(scaled_size((100, 100), f32::INFINITY, 0), (200, 200));
        assert_eq!(scaled_size((1, 1), 0.25, 0), (1, 1));
        assert_eq!(scaled_size((0, 0), 1.0, 0), (1, 1));
        assert_eq!(
            scaled_size((4096, 4096), 2.0, 4096),
            (4096, 4096),
            "within the GPU's limit"
        );
    }

    #[test]
    fn the_scaling_uniform_holds_the_pixel_size_and_ratio() {
        let post = Post {
            sharpen: 2.0,
            ..Default::default()
        };
        let u = PostUniform::new(&post, None).scaled(&post, (960, 540), (1920, 1080));
        assert_eq!(u.output, [1.0 / 1920.0, 1.0 / 1080.0, 1.0, 0.5], "sharpness is clamped");
    }

    #[test]
    fn lens_terms_follow_the_strength_and_stay_off_at_zero() {
        let off = PostUniform::new(&Post::default(), None);
        assert_eq!((off.lens[3], off.fringe[0]), (0.0, 0.0));
        let bulge = PostUniform::new(
            &Post {
                lens_distortion: 0.5,
                chromatic_aberration: 2.0,
                ..Default::default()
            },
            None,
        );
        let theta = 80f32.to_radians();
        assert!((bulge.lens[0] - theta).abs() < 1e-6);
        assert!((bulge.lens[1] - 2.0 * (theta / 2.0).tan()).abs() < 1e-5);
        assert_eq!((bulge.lens[2], bulge.lens[3]), (1.0, 1.0));
        assert_eq!(bulge.fringe[0], 0.05, "clamped to 1");
        let pinch = PostUniform::new(
            &Post {
                lens_distortion: -0.26,
                lens_scale: 1.04,
                ..Default::default()
            },
            None,
        );
        let theta = (1.6f32 * 26.0).to_radians();
        assert!((pinch.lens[0] - 1.0 / theta).abs() < 1e-5);
        assert!((pinch.lens[2] - 1.0 / 1.04).abs() < 1e-6);
        assert_eq!(pinch.lens[3], -1.0);
        let strong = lens(&Post {
            lens_distortion: 1.0,
            ..Default::default()
        });
        assert!((strong[0] - 160f32.to_radians()).abs() < 1e-6, "capped at 160 degrees");
        assert_eq!(size_of::<PostUniform>() % 16, 0);
    }

    #[test]
    fn split_toning_and_depth_of_field_are_off_by_default_in_the_uniform() {
        let u = PostUniform::new(&Post::default(), None);
        assert_eq!(u.split_highlights[3], 0.0, "neutral tints: the pass skips split toning");
        assert_eq!(u.dof, [0.0; 4], "no depth of field");
        let mut post = Post::default();
        post.split_toning.shadows = Color::rgb(0.2, 0.4, f32::NAN);
        post.split_toning.balance = 3.0;
        post.depth_of_field.enabled = true;
        post.depth_of_field.blur = 1.0;
        post.depth_of_field.focus_distance = 4.0;
        let u = PostUniform::new(&post, None).camera(0.5, 200.0);
        assert_eq!(
            u.split_shadows,
            [0.2, 0.4, 0.5, 1.0],
            "a NaN is grey, the balance clamped"
        );
        assert_eq!(u.split_highlights[3], 1.0);
        assert_eq!(u.dof, [4.0, 0.0, DepthOfField::MAX_BLUR, 0.0], "blur clamped");
        assert_eq!(u.camera[..2], [0.5, 200.0]);
        post.depth_of_field.blur = 0.0;
        assert_eq!(PostUniform::new(&post, None).dof, [0.0; 4], "no blur is off");
    }

    #[test]
    fn depth_of_field_blur_grows_away_from_the_sharp_band_like_a_lens() {
        let dof = DepthOfField {
            enabled: true,
            focus_distance: 10.0,
            focus_range: 1.0,
            blur: 0.02,
        };
        for sharp in [9.0, 10.0, 11.0] {
            assert_eq!(dof.blur_at(sharp), 0.0, "{sharp} is in the sharp band");
        }
        assert!(dof.blur_at(12.0) > 0.0 && dof.blur_at(12.0) < dof.blur_at(15.0));
        assert_eq!(dof.blur_at(30.0), 0.02, "full blur well behind");
        assert_eq!(dof.blur_at(1000.0), 0.02, "and no more at the horizon");
        assert_eq!(dof.blur_at(2.0), 0.02, "full blur close up");
        let off = DepthOfField { enabled: false, ..dof };
        assert_eq!(off.blur_at(1000.0), 0.0);
    }

    fn look(exposure: f32) -> Post {
        Post {
            exposure,
            ..Default::default()
        }
    }

    #[test]
    fn lerp_blends_amounts_and_switches_choices_halfway() {
        let a = Post {
            vignette: 0.4,
            ..Default::default()
        };
        let b = Post {
            exposure: 2.0,
            split_toning: SplitToning {
                shadows: Color::rgb(0.0, 0.5, 1.0),
                ..Default::default()
            },
            tonemap: Tonemap::AgX,
            render_scale: 0.5,
            msaa: 2,
            ..Default::default()
        };
        let q = a.lerp(&b, 0.25);
        assert_eq!(q.exposure, 0.5);
        assert!((q.vignette - 0.3).abs() < 1e-6);
        assert_eq!(q.split_toning.shadows, Color::rgb(0.375, 0.5, 0.625));
        assert_eq!(
            (q.tonemap, q.render_scale, q.msaa),
            (Tonemap::Aces, 1.0, 4),
            "before halfway"
        );
        let q = a.lerp(&b, 0.5);
        assert_eq!(
            (q.tonemap, q.render_scale, q.msaa),
            (Tonemap::AgX, 0.5, 2),
            "from halfway"
        );
        assert_eq!(a.lerp(&b, -1.0), a, "clamped");
        assert_eq!(a.lerp(&b, 7.0), b, "clamped");
        assert_eq!(a.lerp(&b, f32::NAN), a, "NaN counts as 0");
    }

    #[test]
    fn lerp_fades_an_effect_that_one_end_has_off() {
        let off = Post::default();
        let on = Post {
            depth_of_field: DepthOfField {
                enabled: true,
                focus_distance: 4.0,
                focus_range: 1.0,
                blur: 0.02,
            },
            ambient_occlusion: AmbientOcclusion {
                enabled: true,
                radius: 2.0,
                intensity: 0.8,
                ..Default::default()
            },
            ..Default::default()
        };
        let q = off.lerp(&on, 0.25);
        assert!(q.depth_of_field.enabled && q.ambient_occlusion.enabled);
        assert_eq!(q.depth_of_field.blur, 0.005, "a quarter of the blur");
        assert_eq!(q.depth_of_field.focus_distance, 4.0, "the focus of the end that has it");
        assert!((q.ambient_occlusion.intensity - 0.2).abs() < 1e-6);
        assert_eq!(q.ambient_occlusion.radius, 2.0);
        let back = on.lerp(&off, 0.75);
        assert_eq!(back.depth_of_field.blur, q.depth_of_field.blur, "the same either way");
        assert_eq!(off.lerp(&on, 0.0), off, "fully off at its own end");
    }

    #[test]
    fn walking_through_a_volume_blends_without_jumps() {
        let base = look(0.0);
        let volumes = [
            PostVolume::new(Vec3::new(0.0, 0.0, 0.0), Vec3::splat(10.0), 5.0, look(2.0)),
            // A smaller room inside it, listed after: it wins there.
            PostVolume::new(Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0), 1.0, look(-1.0)),
        ];
        let mut last = PostVolume::blend(&base, &volumes, Vec3::new(-20.0, 0.0, 0.0)).exposure;
        assert_eq!(last, 0.0, "outside everything");
        let step = 0.05;
        let mut x = -20.0 + step;
        while x <= 0.0 {
            let now = PostVolume::blend(&base, &volumes, Vec3::new(x, 0.0, 0.0)).exposure;
            // The blend is smooth: no step is bigger than the slope of
            // the fastest fade allows (3/2 over its distance, per unit).
            assert!(
                (now - last).abs() <= 3.0 * 1.5 * step + 1e-4,
                "a jump at {x}: {last} -> {now}"
            );
            last = now;
            x += step;
        }
        assert_eq!(
            PostVolume::blend(&base, &volumes, Vec3::new(-4.0, 0.0, 0.0)).exposure,
            2.0
        );
        assert_eq!(PostVolume::blend(&base, &volumes, Vec3::ZERO).exposure, -1.0);
        assert_eq!(PostVolume::blend(&base, &[], Vec3::ZERO), base);
    }

    #[test]
    fn a_volume_without_blend_distance_switches_at_its_faces() {
        let v = PostVolume::new(Vec3::ZERO, Vec3::splat(2.0), 0.0, look(1.0));
        assert_eq!(v.weight(Vec3::new(1.0, 0.0, 0.0)), 1.0, "on the face counts as inside");
        assert_eq!(v.weight(Vec3::new(1.001, 0.0, 0.0)), 0.0);
        let nan = PostVolume {
            blend_distance: f32::NAN,
            ..v
        };
        assert_eq!(nan.weight(Vec3::new(1.5, 0.0, 0.0)), 0.0);
        let corner = PostVolume::new(Vec3::ZERO, Vec3::splat(2.0), 2.0, look(1.0));
        let diagonal = corner.weight(Vec3::new(2.0, 2.0, 1.0));
        assert!(
            diagonal < corner.weight(Vec3::new(2.0, 1.0, 1.0)),
            "distance to the box, not per axis"
        );
    }

    #[test]
    fn auto_exposure_is_off_in_the_uniform_until_enabled() {
        let off = PostUniform::new(&Post::default(), None).adapt(&Post::default(), 0.5, true);
        assert_eq!((off.meter, off.adapt, off.auto), ([0.0; 4], [0.0; 4], [0; 4]));
        let mut post = Post::default();
        post.auto_exposure = AutoExposure {
            enabled: true,
            min: -2.0,
            max: f32::NAN,
            compensation: 0.5,
            center_weight: 3.0,
            ..Default::default()
        };
        let u = PostUniform::new(&post, None).adapt(&post, 0.5, false);
        assert_eq!(
            u.meter,
            [-2.0, 4.0, 0.5, 1.0],
            "a NaN max is the default, the weight clamped"
        );
        assert_eq!(u.adapt, [0.5, 1.5, 0.0, 0.0], "half a second at 1 and 3 stops a second");
        assert_eq!(u.auto, [1, 0, 0, 0]);
        assert_eq!(
            PostUniform::new(&post, None).adapt(&post, 0.5, true).auto[1],
            1,
            "a jump"
        );
        post.auto_exposure.max = -5.0;
        assert_eq!(PostUniform::new(&post, None).meter[1], -2.0, "a max below min is min");
    }

    #[test]
    fn auto_exposure_steps_hold_at_zero_and_jump_at_infinity() {
        let steps = |brighten, darken, dt| {
            AutoExposure {
                brighten_speed: brighten,
                darken_speed: darken,
                ..Default::default()
            }
            .steps(dt)
        };
        assert_eq!(steps(2.0, 4.0, 0.25), [0.5, 1.0]);
        assert_eq!(steps(0.0, -1.0, 0.25), [0.0, 0.0], "0 or below holds");
        assert_eq!(
            steps(f32::NAN, 1.0, f32::NAN),
            [0.0, 0.0],
            "NaN holds; a NaN frame moves nothing"
        );
        assert_eq!(
            steps(f32::INFINITY, 1.0, 0.0),
            [f32::MAX, 0.0],
            "infinite jumps, even at dt 0"
        );
        assert_eq!(steps(1.0, 1.0, -1.0), [0.0, 0.0], "time doesn't run backwards");
    }

    #[test]
    fn auto_exposure_targets_middle_grey_within_its_range() {
        let auto = AutoExposure {
            enabled: true,
            ..Default::default()
        };
        assert!((auto.target(0.09) - 1.0).abs() < 1e-5);
        assert!((auto.target(0.36) + 1.0).abs() < 1e-5);
        assert_eq!(auto.target(0.0), 4.0, "black: held at max");
        assert_eq!(auto.target(1e9), -4.0, "blinding: held at min");
        let wide = AutoExposure {
            min: -40.0,
            max: 40.0,
            ..auto
        };
        assert!(wide.target(0.0).is_finite(), "the meter is held within its stops");
    }

    #[test]
    fn lerp_fades_auto_exposure_in_by_its_range() {
        let off = Post::default();
        let on = Post {
            auto_exposure: AutoExposure {
                enabled: true,
                min: -2.0,
                max: 6.0,
                brighten_speed: f32::INFINITY,
                ..Default::default()
            },
            ..Default::default()
        };
        let q = off.lerp(&on, 0.25);
        assert!(q.auto_exposure.enabled, "on through the blend");
        assert_eq!(
            (q.auto_exposure.min, q.auto_exposure.max),
            (-0.5, 1.5),
            "a quarter of the range"
        );
        assert_eq!(q.auto_exposure.darken_speed, 3.0, "the settings of the end that has it");
        assert_eq!(
            q.auto_exposure.brighten_speed,
            f32::INFINITY,
            "an infinite speed isn't blended"
        );
        let back = on.lerp(&off, 0.75);
        assert_eq!(back.auto_exposure, q.auto_exposure, "the same either way");
        assert_eq!(off.lerp(&on, 0.0), off, "fully off at its own end");
        let half = on.lerp(
            &Post {
                auto_exposure: AutoExposure {
                    brighten_speed: 1.0,
                    ..on.auto_exposure
                },
                ..on
            },
            0.5,
        );
        assert_eq!(half.auto_exposure.brighten_speed, 1.0, "switched halfway");
    }

    #[test]
    fn neutral_white_balance_changes_nothing() {
        for v in white_balance(0.0, 0.0) {
            assert!((v - 1.0).abs() < 0.01, "{v}");
        }
    }

    #[test]
    fn warm_white_balance_boosts_long_wavelengths() {
        let [l, _, s] = white_balance(0.5, 0.0);
        assert!(l > s, "warm: {l} vs {s}");
        let [l, _, s] = white_balance(-0.5, 0.0);
        assert!(l < s, "cool: {l} vs {s}");
    }

    #[test]
    fn neutral_lut_corners_are_black_and_white() {
        let (w, h, px) = Post::neutral_lut(4);
        assert_eq!((w, h, px.len()), (16, 4, 16 * 4 * 4));
        assert_eq!(&px[..4], &[0, 0, 0, 255]);
        // Last square (blue full), last row (green full), last column (red full).
        let last = ((h - 1) * w + w - 1) as usize * 4;
        assert_eq!(&px[last..last + 4], &[255, 255, 255, 255]);
    }
}
