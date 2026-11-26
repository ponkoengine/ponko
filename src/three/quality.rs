//! Graphics quality presets: ready-made sets of the settings that trade
//! looks for speed, written into `ctx.scene3d` so a game's options menu can
//! switch them at run time.

use super::collect::MAX_CASCADES;
use super::{
    AntiAliasing, Lighting, MAX_LIGHT_SHADOW_ATLAS, MAX_LIGHT_SHADOW_RESOLUTION, MAX_LIGHT_SHADOW_VIEWS,
    MAX_RENDER_SCALE, MAX_SHADOW_RESOLUTION, MIN_LIGHT_SHADOW_ATLAS, MIN_LIGHT_SHADOW_RESOLUTION, MIN_RENDER_SCALE,
    MIN_SHADOW_RESOLUTION, Post, SceneSettings, ShadowFilter,
};

/// A graphics quality preset: a ready-made set of the settings that decide
/// how heavy the 3D picture is (the resolution it is drawn at, shadows,
/// ambient occlusion, screen-space reflections, global illumination and
/// antialiasing). Apply one with [`SceneSettings::set_quality`]; it writes
/// the matching fields of [`SceneSettings::lighting`] and
/// [`SceneSettings::post`] and leaves the others alone. A preset holds no
/// settings of its own: change any of those fields afterwards and
/// [`SceneSettings::quality`] says `None` (custom).
///
/// | | `Low` | `Medium` | `High` | `Ultra` |
/// |---|---|---|---|---|
/// | `post.render_scale` | 0.67 | 1 | 1 | 1 |
/// | `post.sharpen` | 0.5 | 0 | 0 | 0 |
/// | `lighting.shadow_distance` | 30 | 50 | 80 | 120 |
/// | `lighting.shadow_cascades` | 2 | 3 | 4 | 4 |
/// | `lighting.shadow_resolution` | 1024 | 2048 | 2048 | 4096 |
/// | `lighting.shadow_refresh` | 4 | 2 | 2 | 1 |
/// | `lighting.shadow_filter` | `Sharp` | `Soft` | `ContactHardening` | `ContactHardening` |
/// | `lighting.light_shadow_views` | 0 | 4 | 8 | 24 |
/// | `lighting.light_shadow_resolution` | 512 | 512 | 1024 | 2048 |
/// | `lighting.light_shadow_atlas` | 1024 | 2048 | 4096 | 8192 |
/// | `post.ambient_occlusion` (slices, steps) | off | off | 2, 4 | 4, 8 |
/// | `post.ao_half_resolution` / `ao_temporal` | off | off | on | on |
/// | `post.ssr` | 0 | 0 | 1 | 1 |
/// | `post.ssr_thin_objects` | off | off | off | on |
/// | `lighting.gi.enabled` | off | off | off | on |
/// | `post.anti_aliasing` | none | FXAA | SMAA | SMAA |
///
/// Every preset turns shadows on (`lighting.shadows`).
///
/// ```
/// use ponko::prelude::*;
/// use ponko::render::GraphicsQuality;
/// Game::new("quality", 64, 64).run_headless(0, |ctx| {
///     ctx.scene3d.set_quality(GraphicsQuality::Medium);
///     assert_eq!(ctx.scene3d.quality(), Some(GraphicsQuality::Medium));
///     // Then change one thing: sharper shadows, the rest as it was.
///     ctx.scene3d.lighting.shadow_resolution = 4096;
///     assert_eq!(ctx.scene3d.quality(), None); // custom
/// });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GraphicsQuality {
    /// For weak GPUs: the scene is drawn at two thirds of the window's size
    /// and sharpened on the way up, two 1024² shadow cascades reach 30 units
    /// (the far one redrawn every fourth frame), point and spot lights cast
    /// no shadows, no AO, no post antialiasing.
    Low,
    /// Full resolution, three 2048² cascades reaching 50 units, four
    /// views of up to 512² for point and spot lights in a 2048² atlas, FXAA,
    /// no AO.
    Medium,
    /// Four 2048² cascades reaching 80 units with contact-hardening edges,
    /// eight views of up to 1024² for point and spot lights in a 4096² atlas,
    /// light AO (2 slices, 4 steps), screen-space reflections, SMAA. It
    /// leaves the thin-objects reflection trace ([`Post::ssr_thin_objects`])
    /// off, as it costs 0.3 to 2.3 ms a frame in busy scenes; set it after
    /// the preset where thin things in reflections matter.
    High,
    /// Four 4096² cascades (256 MB of shadow maps) reaching 120 units, all
    /// redrawn every frame, with contact-hardening edges, 24 views of up to 2048² for point and spot
    /// lights in an 8192² atlas (another 256 MB), finer AO (4 slices, 8
    /// steps), screen-space reflections that keep thin things whole without
    /// TAA ([`Post::ssr_thin_objects`]), global illumination, SMAA.
    Ultra,
}

impl GraphicsQuality {
    /// Every preset, from the lightest to the heaviest.
    /// ```
    /// use ponko::render::GraphicsQuality;
    /// assert_eq!(GraphicsQuality::ALL[0], GraphicsQuality::Low);
    /// ```
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Ultra];

    /// The settings this preset writes.
    fn preset(self) -> Preset {
        match self {
            Self::Low => Preset {
                render_scale: 0.67,
                sharpen: 0.5,
                shadows: true,
                shadow_distance: 30.0,
                shadow_cascades: 2,
                shadow_resolution: 1024,
                shadow_refresh: 4,
                shadow_filter: ShadowFilter::Sharp,
                light_shadow_views: 0,
                light_shadow_resolution: 512,
                light_shadow_atlas: 1024,
                ambient_occlusion: false,
                ao_half_resolution: false,
                ao_temporal: false,
                ao_slices: 2,
                ao_steps: 4,
                ssr: 0.0,
                ssr_thin_objects: false,
                gi: false,
                anti_aliasing: AntiAliasing::None,
            },
            Self::Medium => Preset {
                render_scale: 1.0,
                sharpen: 0.0,
                shadows: true,
                shadow_distance: 50.0,
                shadow_cascades: 3,
                shadow_resolution: 2048,
                shadow_refresh: 2,
                shadow_filter: ShadowFilter::Soft,
                light_shadow_views: 4,
                light_shadow_resolution: 512,
                light_shadow_atlas: 2048,
                ambient_occlusion: false,
                ao_half_resolution: false,
                ao_temporal: false,
                ao_slices: 2,
                ao_steps: 4,
                ssr: 0.0,
                ssr_thin_objects: false,
                gi: false,
                anti_aliasing: AntiAliasing::Fxaa,
            },
            Self::High => Preset {
                render_scale: 1.0,
                sharpen: 0.0,
                shadows: true,
                shadow_distance: 80.0,
                shadow_cascades: 4,
                shadow_resolution: 2048,
                shadow_refresh: 2,
                shadow_filter: ShadowFilter::ContactHardening,
                light_shadow_views: 8,
                light_shadow_resolution: 1024,
                light_shadow_atlas: 4096,
                ambient_occlusion: true,
                ao_half_resolution: true,
                ao_temporal: true,
                ao_slices: 2,
                ao_steps: 4,
                ssr: 1.0,
                ssr_thin_objects: false,
                gi: false,
                anti_aliasing: AntiAliasing::Smaa,
            },
            Self::Ultra => Preset {
                render_scale: 1.0,
                sharpen: 0.0,
                shadows: true,
                shadow_distance: 120.0,
                shadow_cascades: 4,
                shadow_resolution: 4096,
                shadow_refresh: 1,
                shadow_filter: ShadowFilter::ContactHardening,
                light_shadow_views: MAX_LIGHT_SHADOW_VIEWS as u32,
                light_shadow_resolution: 2048,
                light_shadow_atlas: 8192,
                ambient_occlusion: true,
                ao_half_resolution: true,
                ao_temporal: true,
                ao_slices: 4,
                ao_steps: 8,
                ssr: 1.0,
                ssr_thin_objects: true,
                gi: true,
                anti_aliasing: AntiAliasing::Smaa,
            },
        }
    }
}

/// The settings a preset writes, as the renderer applies them. Private: the
/// settings themselves live in [`Lighting`] and [`Post`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Preset {
    render_scale: f32,
    sharpen: f32,
    shadows: bool,
    shadow_distance: f32,
    shadow_cascades: u32,
    shadow_resolution: u32,
    shadow_refresh: u32,
    shadow_filter: ShadowFilter,
    light_shadow_views: u32,
    light_shadow_resolution: u32,
    light_shadow_atlas: u32,
    ambient_occlusion: bool,
    ao_half_resolution: bool,
    ao_temporal: bool,
    ao_slices: u32,
    ao_steps: u32,
    ssr: f32,
    ssr_thin_objects: bool,
    gi: bool,
    anti_aliasing: AntiAliasing,
}

impl Preset {
    /// The current settings, each brought into its allowed range as the
    /// renderer uses it (a NaN counts as the default).
    fn read(lighting: &Lighting, post: &Post) -> Self {
        let (dl, dp) = (Lighting::default(), Post::default());
        let float = |v: f32, lo: f32, hi: f32, fallback: f32| if v.is_nan() { fallback } else { v.clamp(lo, hi) };
        Self {
            render_scale: float(post.render_scale, MIN_RENDER_SCALE, MAX_RENDER_SCALE, dp.render_scale),
            sharpen: float(post.sharpen, 0.0, 1.0, dp.sharpen),
            shadows: lighting.shadows,
            shadow_distance: float(lighting.shadow_distance, 0.0, f32::MAX, dl.shadow_distance),
            shadow_cascades: lighting.shadow_cascades.clamp(1, MAX_CASCADES as u32),
            shadow_resolution: lighting
                .shadow_resolution
                .clamp(MIN_SHADOW_RESOLUTION, MAX_SHADOW_RESOLUTION),
            shadow_refresh: lighting.shadow_refresh.clamp(1, super::collect::MAX_SHADOW_REFRESH),
            shadow_filter: lighting.shadow_filter,
            light_shadow_views: lighting.light_shadow_views.min(MAX_LIGHT_SHADOW_VIEWS as u32),
            light_shadow_resolution: lighting
                .light_shadow_resolution
                .clamp(MIN_LIGHT_SHADOW_RESOLUTION, MAX_LIGHT_SHADOW_RESOLUTION),
            light_shadow_atlas: super::shadow_atlas::floor_pow2(
                lighting
                    .light_shadow_atlas
                    .clamp(MIN_LIGHT_SHADOW_ATLAS, MAX_LIGHT_SHADOW_ATLAS),
            ),
            ambient_occlusion: post.ambient_occlusion.enabled,
            ao_half_resolution: post.ao_half_resolution,
            ao_temporal: post.ao_temporal,
            ao_slices: post.ambient_occlusion.slices.clamp(1, 8),
            ao_steps: post.ambient_occlusion.steps.clamp(1, 16),
            ssr: float(post.ssr, 0.0, 1.0, dp.ssr),
            ssr_thin_objects: post.ssr_thin_objects,
            gi: lighting.gi.enabled,
            anti_aliasing: post.anti_aliasing,
        }
    }

    fn write(self, lighting: &mut Lighting, post: &mut Post) {
        post.render_scale = self.render_scale;
        post.sharpen = self.sharpen;
        post.anti_aliasing = self.anti_aliasing;
        post.ambient_occlusion.enabled = self.ambient_occlusion;
        post.ao_half_resolution = self.ao_half_resolution;
        post.ao_temporal = self.ao_temporal;
        post.ambient_occlusion.slices = self.ao_slices;
        post.ambient_occlusion.steps = self.ao_steps;
        post.ssr = self.ssr;
        post.ssr_thin_objects = self.ssr_thin_objects;
        lighting.gi.enabled = self.gi;
        lighting.shadows = self.shadows;
        lighting.shadow_distance = self.shadow_distance;
        lighting.shadow_cascades = self.shadow_cascades;
        lighting.shadow_resolution = self.shadow_resolution;
        lighting.shadow_refresh = self.shadow_refresh;
        lighting.shadow_filter = self.shadow_filter;
        lighting.light_shadow_views = self.light_shadow_views;
        lighting.light_shadow_resolution = self.light_shadow_resolution;
        lighting.light_shadow_atlas = self.light_shadow_atlas;
    }
}

impl SceneSettings {
    /// Switches the 3D picture to preset `quality`, from any frame, without
    /// restarting: writes the preset's fields of [`lighting`](Self::lighting)
    /// and [`post`](Self::post) (see the table at [`GraphicsQuality`]) and
    /// leaves the others (AO radius, tone mapping, ...) alone. It pins render
    /// scale and sun shadow resolution against automatic quality, including
    /// when their values are unchanged. The next frame draws the scene at
    /// the new resolution and with new shadow maps where their size or
    /// count changed: nothing is drawn from stale targets.
    ///
    /// ```no_run
    /// # use ponko::prelude::*;
    /// # fn f(ctx: &mut Ctx) {
    /// ctx.scene3d.set_quality(ponko::render::GraphicsQuality::Low);
    /// ctx.scene3d.post.ambient_occlusion.radius = 2.0; // a tweak on top
    /// # }
    /// ```
    pub fn set_quality(&mut self, quality: GraphicsQuality) {
        self.autotune.scale_pinned = true;
        self.autotune.shadow_pinned = true;
        quality.preset().write(&mut self.lighting, &mut self.post);
    }

    /// Which preset the current settings are, or `None` when some setting a
    /// preset writes was changed by hand (custom). Values outside their
    /// allowed ranges are compared as the renderer uses them (clamped).
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("quality", 64, 64).run_headless(0, |ctx| {
    ///     assert_eq!(ctx.scene3d.quality(), None, "the defaults are none of the presets");
    ///     ctx.scene3d.set_quality(ponko::render::GraphicsQuality::Ultra);
    ///     assert_eq!(ctx.scene3d.quality(), Some(ponko::render::GraphicsQuality::Ultra));
    /// });
    /// ```
    #[must_use]
    pub fn quality(&self) -> Option<GraphicsQuality> {
        let current = Preset::read(&self.lighting, &self.post);
        GraphicsQuality::ALL.into_iter().find(|q| q.preset() == current)
    }

    /// Brings every setting a preset writes (the table at
    /// [`GraphicsQuality`]) into the range the renderer draws it with: values
    /// outside it are clamped, a NaN becomes the default and the light
    /// shadow atlas rounds down to a power of two. The picture stays the
    /// same, as the renderer does this as it draws; use it before showing
    /// the values (an options menu) or saving them.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// Game::new("quality", 64, 64).run_headless(0, |ctx| {
    ///     ctx.scene3d.post.render_scale = 9.0;
    ///     ctx.scene3d.post.sharpen = f32::NAN;
    ///     ctx.scene3d.clamp_quality();
    ///     assert_eq!(ctx.scene3d.post.render_scale, ponko::render::Post::MAX_RENDER_SCALE);
    ///     assert_eq!(ctx.scene3d.post.sharpen, 0.0);
    /// });
    /// ```
    pub fn clamp_quality(&mut self) {
        (self.lighting, self.post) = self.clamped_quality();
    }

    /// The lighting and post settings with every field a preset writes as
    /// the renderer uses it (see [`clamp_quality`](Self::clamp_quality)).
    pub(crate) fn clamped_quality(&self) -> (Lighting, Post) {
        let (mut lighting, mut post) = (self.lighting, self.post);
        Preset::read(&self.lighting, &self.post).write(&mut lighting, &mut post);
        (lighting, post)
    }

    /// Applies the preset meant for the GPU in `caps`, fitted to it: `Low`
    /// with FXAA for a [`GpuTier::WebGl2`](crate::render::GpuTier::WebGl2)
    /// GPU (it has no multisampling), `Medium` for `Baseline` and `High` for
    /// `Desktop`; then [`fit_quality_to`](Self::fit_quality_to). None of the
    /// tier presets asks for a feature its tier lacks or a shadow map larger
    /// than its textures may be. `Ultra` is never picked: it is for GPUs
    /// with memory to spare, a choice for the player to make.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::render::{GpuTier, GraphicsQuality};
    /// Game::new("quality", 64, 64).run_headless(0, |ctx| {
    ///     ctx.scene3d.set_recommended_quality(&GpuTier::Desktop.report());
    ///     assert_eq!(ctx.scene3d.quality(), Some(GraphicsQuality::High));
    ///     ctx.scene3d.set_recommended_quality(&GpuTier::WebGl2.report());
    ///     assert_eq!(ctx.scene3d.post.anti_aliasing, ponko::render::AntiAliasing::Fxaa);
    ///     assert_eq!(ctx.scene3d.quality(), None, "low, with FXAA");
    /// });
    /// ```
    pub fn set_recommended_quality(&mut self, caps: &crate::render::GpuCapabilities) {
        use crate::render::GpuTier;
        let quality = match caps.tier {
            GpuTier::WebGl2 => GraphicsQuality::Low,
            GpuTier::Baseline => GraphicsQuality::Medium,
            GpuTier::Desktop => GraphicsQuality::High,
        };
        self.set_quality(quality);
        if caps.tier == GpuTier::WebGl2 {
            self.post.anti_aliasing = AntiAliasing::Fxaa;
        }
        self.fit_quality_to(caps);
    }

    /// Takes out of the current settings what the GPU in `caps` can't do, as
    /// the renderer would draw them anyway: TAA becomes FXAA, and ambient
    /// occlusion, screen-space reflections and global illumination go off
    /// where they're unavailable; contact-hardening sun shadows become
    /// `Softest` on WebGL2 (and any GL backend); shadow maps and the light shadow atlas
    /// shrink to the largest texture the GPU allows. Asking for more is
    /// never a GPU error (the renderer falls back by itself); fitting makes
    /// the settings say what is drawn, for an options menu.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// use ponko::render::{AntiAliasing, GpuTier, GraphicsQuality};
    /// Game::new("quality", 64, 64).run_headless(0, |ctx| {
    ///     ctx.scene3d.set_quality(GraphicsQuality::Ultra);
    ///     ctx.scene3d.fit_quality_to(&GpuTier::WebGl2.report());
    ///     let (l, p) = (&ctx.scene3d.lighting, &ctx.scene3d.post);
    ///     assert_eq!((p.anti_aliasing, p.ambient_occlusion.enabled, p.ssr, l.gi.enabled), (AntiAliasing::Smaa, false, 0.0, false));
    ///     assert_eq!((l.shadow_resolution, l.light_shadow_atlas), (2048, 2048));
    /// });
    /// ```
    pub fn fit_quality_to(&mut self, caps: &crate::render::GpuCapabilities) {
        use crate::render::GpuFeature as F;
        let (lighting, post) = (&mut self.lighting, &mut self.post);
        if post.anti_aliasing == AntiAliasing::Taa && !caps.available(F::TemporalAntialiasing) {
            post.anti_aliasing = AntiAliasing::Fxaa;
        }
        if !caps.available(F::AmbientOcclusion) {
            post.ambient_occlusion.enabled = false;
            post.ao_half_resolution = false;
            post.ao_temporal = false;
        }
        if !caps.available(F::ScreenSpaceReflections) {
            post.ssr = 0.0;
            post.ssr_thin_objects = false;
        }
        if !caps.available(F::GlobalIllumination) {
            lighting.gi.enabled = false;
        }
        if lighting.shadow_filter == ShadowFilter::ContactHardening && !contact_shadows_drawn(caps) {
            lighting.shadow_filter = ShadowFilter::Softest;
        }
        let max = caps.limits.max_texture_dimension_2d;
        if max > 0 {
            lighting.shadow_resolution = lighting.shadow_resolution.min(max.max(MIN_SHADOW_RESOLUTION));
            lighting.light_shadow_resolution = lighting
                .light_shadow_resolution
                .min(max.max(MIN_LIGHT_SHADOW_RESOLUTION));
            lighting.light_shadow_atlas =
                super::shadow_atlas::floor_pow2(lighting.light_shadow_atlas.min(max.max(MIN_LIGHT_SHADOW_ATLAS)));
        }
    }
}

/// Whether the GPU in `caps` draws `ShadowFilter::ContactHardening`: not
/// on the GL backend (WebGL2 and native OpenGL ES), whose shading language
/// can't read the shadow map's depths; the renderer draws `Softest` there.
fn contact_shadows_drawn(caps: &crate::render::GpuCapabilities) -> bool {
    caps.tier != crate::render::GpuTier::WebGl2 && caps.adapter.backend != "gl"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_get_heavier_from_low_to_ultra() {
        let p = GraphicsQuality::ALL.map(GraphicsQuality::preset);
        for w in p.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(a.render_scale <= b.render_scale);
            assert!(a.shadow_distance < b.shadow_distance);
            assert!(a.shadow_cascades <= b.shadow_cascades);
            assert!(a.shadow_resolution <= b.shadow_resolution);
            assert!(
                a.shadow_refresh >= b.shadow_refresh,
                "heavier redraws far shadows more often"
            );
            assert!(
                a.shadow_filter as u32 <= b.shadow_filter as u32,
                "heavier filters softer"
            );
            assert!(a.light_shadow_views < b.light_shadow_views);
            assert!(a.light_shadow_resolution <= b.light_shadow_resolution);
            assert!(a.light_shadow_atlas < b.light_shadow_atlas);
            assert!(a.ambient_occlusion <= b.ambient_occlusion);
            assert!(a.ao_slices * a.ao_steps <= b.ao_slices * b.ao_steps);
            assert!(a.ssr <= b.ssr);
            assert!(a.ssr_thin_objects <= b.ssr_thin_objects);
            assert!(a.gi <= b.gi);
            assert!(a.anti_aliasing as u32 <= b.anti_aliasing as u32);
        }
    }

    #[test]
    fn presets_are_valid_and_distinct() {
        for q in GraphicsQuality::ALL {
            let mut s = SceneSettings::default();
            s.set_quality(q);
            assert_eq!(
                Preset::read(&s.lighting, &s.post),
                q.preset(),
                "{q:?} is inside every range"
            );
            assert_eq!(s.quality(), Some(q));
        }
        assert_eq!(SceneSettings::default().quality(), None);
        let d = SceneSettings::default();
        let (l, p) = (Lighting::default(), Post::default());
        assert_eq!(Preset::read(&d.lighting, &d.post), Preset::read(&l, &p));
    }

    #[test]
    fn the_documented_values() {
        let [low, medium, high, ultra] = GraphicsQuality::ALL.map(GraphicsQuality::preset);
        assert_eq!((low.render_scale, low.sharpen, low.shadow_distance), (0.67, 0.5, 30.0));
        assert_eq!((low.shadow_cascades, low.shadow_resolution), (2, 1024));
        assert_eq!(low.shadow_refresh, 4);
        assert_eq!(ultra.shadow_refresh, 1);
        assert_eq!(
            (
                low.light_shadow_views,
                low.light_shadow_resolution,
                low.light_shadow_atlas
            ),
            (0, 512, 1024)
        );
        assert_eq!((low.ambient_occlusion, low.anti_aliasing), (false, AntiAliasing::None));
        assert_eq!(
            (low.shadow_filter, medium.shadow_filter, high.shadow_filter),
            (ShadowFilter::Sharp, ShadowFilter::Soft, ShadowFilter::ContactHardening)
        );
        assert_eq!(ultra.shadow_filter, ShadowFilter::ContactHardening);
        assert_eq!(
            (medium.shadow_distance, medium.shadow_cascades, medium.shadow_resolution),
            (50.0, 3, 2048)
        );
        assert_eq!(
            (
                medium.light_shadow_views,
                medium.light_shadow_resolution,
                medium.light_shadow_atlas
            ),
            (4, 512, 2048)
        );
        assert_eq!(
            (medium.ambient_occlusion, medium.anti_aliasing),
            (false, AntiAliasing::Fxaa)
        );
        assert_eq!(
            (high.shadow_distance, high.shadow_cascades, high.shadow_resolution),
            (80.0, 4, 2048)
        );
        assert_eq!(
            (
                high.light_shadow_views,
                high.light_shadow_resolution,
                high.light_shadow_atlas
            ),
            (8, 1024, 4096)
        );
        assert_eq!((high.ambient_occlusion, high.ao_slices, high.ao_steps), (true, 2, 4));
        assert!(high.ao_half_resolution && high.ao_temporal);
        assert_eq!(high.anti_aliasing, AntiAliasing::Smaa);
        assert_eq!((low.ssr, medium.ssr, high.ssr), (0.0, 0.0, 1.0));
        assert_eq!(
            [low, medium, high, ultra].map(|p| p.ssr_thin_objects),
            [false, false, false, true],
            "thin objects in reflections in Ultra only"
        );
        assert_eq!(
            (ultra.shadow_distance, ultra.shadow_cascades, ultra.shadow_resolution),
            (120.0, 4, 4096)
        );
        assert_eq!(
            (
                ultra.light_shadow_views,
                ultra.light_shadow_resolution,
                ultra.light_shadow_atlas
            ),
            (24, 2048, 8192)
        );
        assert_eq!((ultra.ao_slices, ultra.ao_steps, ultra.ssr), (4, 8, 1.0));
        assert!(ultra.gi && !high.gi);
    }

    #[test]
    fn out_of_range_and_nan_values_are_compared_as_the_renderer_uses_them() {
        let mut s = SceneSettings::default();
        s.set_quality(GraphicsQuality::Ultra);
        s.lighting.shadow_cascades = 9;
        s.lighting.shadow_resolution = 1 << 20;
        s.lighting.light_shadow_views = 99;
        s.post.ssr = 7.0;
        assert_eq!(s.quality(), Some(GraphicsQuality::Ultra), "clamped to Ultra's values");
        s.post.render_scale = 9.0;
        assert_eq!(Preset::read(&s.lighting, &s.post).render_scale, MAX_RENDER_SCALE);
        assert_eq!(s.quality(), None, "drawn at twice Ultra's resolution");
        let ultra_at_twice = s.clone();
        s.clamp_quality();
        assert_eq!(s.post.render_scale, MAX_RENDER_SCALE);
        assert_eq!((s.lighting.shadow_cascades, s.lighting.shadow_resolution), (4, 4096));
        assert_eq!((s.lighting.light_shadow_views, s.post.ssr), (24, 1.0));
        assert_eq!(
            Preset::read(&s.lighting, &s.post),
            Preset::read(&ultra_at_twice.lighting, &ultra_at_twice.post),
            "the same picture"
        );
        let mut s = SceneSettings::default();
        s.post.render_scale = 0.0;
        s.post.sharpen = f32::NAN;
        s.lighting.shadow_distance = -5.0;
        s.lighting.light_shadow_resolution = 1;
        s.lighting.light_shadow_atlas = 5000;
        s.post.ambient_occlusion.slices = 0;
        s.post.ambient_occlusion.steps = 99;
        let q = Preset::read(&s.lighting, &s.post);
        assert_eq!(q.render_scale, MIN_RENDER_SCALE);
        assert_eq!(q.sharpen, 0.0);
        assert_eq!(q.shadow_distance, 0.0);
        assert_eq!(q.light_shadow_resolution, 128);
        assert_eq!(q.light_shadow_atlas, 4096);
        assert_eq!((q.ao_slices, q.ao_steps), (1, 16));
    }

    #[test]
    fn applying_a_preset_and_reading_it_back_round_trips() {
        crate::game::Game::new("quality", 64, 64).run_headless(0, |ctx| {
            assert_eq!(ctx.scene3d.quality(), None);
            for q in [
                GraphicsQuality::Ultra,
                GraphicsQuality::Low,
                GraphicsQuality::High,
                GraphicsQuality::Medium,
            ] {
                ctx.scene3d.set_quality(q);
                assert_eq!(ctx.scene3d.quality(), Some(q));
                let p = q.preset();
                let s = &ctx.scene3d;
                assert_eq!(s.post.render_scale, p.render_scale);
                assert_eq!(s.lighting.shadow_resolution, p.shadow_resolution);
                assert_eq!(s.post.ambient_occlusion.enabled, p.ambient_occlusion);
                assert_eq!(s.post.ao_half_resolution, p.ao_half_resolution);
                assert_eq!(s.post.ao_temporal, p.ao_temporal);
                assert_eq!(s.post.ssr, p.ssr);
                assert_eq!(s.post.ssr_thin_objects, p.ssr_thin_objects);
                assert_eq!(s.lighting.gi.enabled, p.gi);
            }
        });
    }

    #[test]
    fn tweaks_after_a_preset_stick_and_make_it_custom() {
        crate::game::Game::new("quality", 64, 64).run_headless(0, |ctx| {
            ctx.scene3d.post.ambient_occlusion.radius = 3.5;
            ctx.scene3d.set_quality(GraphicsQuality::High);
            assert_eq!(
                ctx.scene3d.post.ambient_occlusion.radius, 3.5,
                "settings outside the preset stay"
            );
            ctx.scene3d.lighting.shadow_cascades = 2;
            assert_eq!(ctx.scene3d.quality(), None);
        });
    }

    #[test]
    fn tier_presets_ask_for_nothing_their_tier_lacks() {
        use crate::render::GpuFeature;
        use crate::render::GpuTier;
        for tier in GpuTier::ALL {
            let floor = tier.report();
            let mut s = SceneSettings::default();
            s.set_recommended_quality(&floor);
            let before = Preset::read(&s.lighting, &s.post);
            s.fit_quality_to(&floor);
            assert_eq!(before, Preset::read(&s.lighting, &s.post), "{tier}: already fitted");
            let (l, p) = (&s.lighting, &s.post);
            assert!(p.anti_aliasing != AntiAliasing::Taa || floor.available(GpuFeature::TemporalAntialiasing));
            assert!(!p.ambient_occlusion.enabled || floor.available(GpuFeature::AmbientOcclusion));
            assert!(p.ssr == 0.0 || floor.available(GpuFeature::ScreenSpaceReflections));
            assert!(!l.gi.enabled || floor.available(GpuFeature::GlobalIllumination));
            assert!(l.shadow_resolution <= floor.limits.max_texture_dimension_2d);
            assert!(l.light_shadow_atlas <= floor.limits.max_texture_dimension_2d);
        }
        let quality = |tier: GpuTier| {
            let mut s = SceneSettings::default();
            s.set_recommended_quality(&tier.report());
            s.quality()
        };
        assert_eq!(quality(GpuTier::Baseline), Some(GraphicsQuality::Medium));
        assert_eq!(quality(GpuTier::Desktop), Some(GraphicsQuality::High));
        assert_eq!(quality(GpuTier::WebGl2), None, "low with FXAA");
    }

    #[test]
    fn fitting_takes_out_only_what_the_gpu_lacks() {
        use crate::render::GpuTier;
        let ultra = |taa: bool| {
            let mut s = SceneSettings::default();
            s.set_quality(GraphicsQuality::Ultra);
            if taa {
                s.post.anti_aliasing = AntiAliasing::Taa;
            }
            s
        };
        let mut on_desktop = ultra(false);
        on_desktop.fit_quality_to(&GpuTier::Desktop.report());
        assert_eq!(on_desktop.quality(), Some(GraphicsQuality::Ultra));
        let mut on_baseline = ultra(true);
        on_baseline.fit_quality_to(&GpuTier::Baseline.report());
        let (l, p) = (&on_baseline.lighting, &on_baseline.post);
        assert_eq!(p.anti_aliasing, AntiAliasing::Taa, "baseline has TAA");
        assert!(p.ambient_occlusion.enabled && p.ssr == 1.0 && p.ssr_thin_objects);
        assert!(!l.gi.enabled, "no room for the GI probes at the baseline floor");
        assert_eq!(l.shadow_resolution, 2048);
        let mut on_webgl2 = ultra(true);
        on_webgl2.fit_quality_to(&GpuTier::WebGl2.report());
        let (l, p) = (&on_webgl2.lighting, &on_webgl2.post);
        assert_eq!(p.anti_aliasing, AntiAliasing::Fxaa);
        assert!(!p.ambient_occlusion.enabled && !p.ao_temporal && !p.ao_half_resolution);
        assert_eq!((p.ssr, p.ssr_thin_objects, l.gi.enabled), (0.0, false, false));
        assert_eq!(l.light_shadow_atlas, 2048);
        assert_eq!(l.shadow_cascades, 4, "untouched");
    }

    #[test]
    fn shadow_sun_size_sets_how_fast_contact_hardening_edges_widen() {
        let mut l = Lighting::default();
        assert_eq!(l.penumbra_per_distance(), 0.0, "the fixed filters ignore it");
        l.shadow_filter = ShadowFilter::ContactHardening;
        let real = l.penumbra_per_distance();
        assert!((real - 0.00925).abs() < 1e-4, "the real sun: {real} per unit");
        l.shadow_sun_size = 6.0;
        assert!((l.penumbra_per_distance() - 2.0 * 3f32.to_radians().tan()).abs() < 1e-6);
        l.shadow_sun_size = 90.0;
        let most = l.penumbra_per_distance();
        assert!(
            (most - 2.0 * 5f32.to_radians().tan()).abs() < 1e-6,
            "clamped to 10 degrees: {most}"
        );
        l.shadow_sun_size = -1.0;
        assert_eq!(l.penumbra_per_distance(), 0.0, "no smaller than a point");
        l.shadow_sun_size = f32::NAN;
        assert_eq!(l.penumbra_per_distance(), real, "a NaN counts as the default");
    }

    #[test]
    fn contact_hardening_shadows_are_off_by_default_and_on_in_high_and_ultra() {
        assert_eq!(Lighting::default().shadow_filter, ShadowFilter::Softest);
        for q in GraphicsQuality::ALL {
            let on = q.preset().shadow_filter == ShadowFilter::ContactHardening;
            assert_eq!(on, matches!(q, GraphicsQuality::High | GraphicsQuality::Ultra), "{q:?}");
        }
    }

    #[test]
    fn fitting_to_webgl2_falls_back_from_contact_hardening_to_the_fixed_filter() {
        use crate::render::GpuTier;
        let fitted = |caps: &crate::render::GpuCapabilities| {
            let mut s = SceneSettings::default();
            s.set_quality(GraphicsQuality::High);
            s.fit_quality_to(caps);
            s.lighting.shadow_filter
        };
        assert_eq!(fitted(&GpuTier::Desktop.report()), ShadowFilter::ContactHardening);
        assert_eq!(fitted(&GpuTier::Baseline.report()), ShadowFilter::ContactHardening);
        assert_eq!(fitted(&GpuTier::WebGl2.report()), ShadowFilter::Softest);
        let mut gl = GpuTier::Desktop.report();
        gl.adapter.backend = "gl".to_owned();
        assert_eq!(fitted(&gl), ShadowFilter::Softest, "native OpenGL ES draws Softest too");
        // Fitting changes nothing else about the filter.
        let mut s = SceneSettings::default();
        s.lighting.shadow_filter = ShadowFilter::Soft;
        s.fit_quality_to(&GpuTier::WebGl2.report());
        assert_eq!(s.lighting.shadow_filter, ShadowFilter::Soft);
    }
}
