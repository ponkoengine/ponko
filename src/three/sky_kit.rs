//! The sky kit ([`SkyKit`]): a ready-made sky on the sky shader hook, with
//! raymarched clouds, a moon with phases and a star field, and the
//! [`DayNight`] driver that moves the sun and the moon through the day.
//! The CPU half: the settings, what the shaders read of them (`frame_params`)
//! and the driver's arithmetic. The GPU half is `sky_kit.wgsl`,
//! `sky_kit_clouds.wgsl` and `sky_kit_shadow.wgsl`, run by `render/clouds.rs`.

use glam::{Vec2, Vec3};

use crate::color::Color;
use crate::render::Lighting;

use super::{SceneSettings, Sky};

/// A ready-made sky: the physical atmosphere, clouds with wind, a moon
/// with phases and a star field, set as [`Sky::kit`] (`ctx.scene3d.sky.kit`).
///
/// It is a sky shader the engine brings along (see [`SkyShader`](crate::render::SkyShader)),
/// so it reaches everything a sky shader does: the background, the fog's
/// color, reflections and the ambient light. On top of that:
///
/// - **Clouds** ([`Clouds`]) are raymarched through a layer of the sky at
///   a quarter of the screen's pixels and smoothed over frames (temporal
///   reprojection), with the coverage, kind, wind, self-shadowing and
///   silver lining set there. [`CloudQuality`] picks how finely they are
///   marched, or a flat 2D cloud layer drawn into the sky itself.
/// - **Cloud shadows** fall on the ground: a small map of how much sunlight
///   the clouds let through is drawn each frame and every lit surface
///   reads it.
/// - **The fog** takes the clouds' colors (it reads the sky behind it),
///   and its glow toward the sun dims where clouds cover the sun.
/// - **The moon** ([`Moon`]) is lit by its phase, with a soft halo, and the
///   **stars** ([`Sky::stars`] sets how bright) turn with the time of day.
///
/// The sun is [`Lighting::sun_direction`]: with the kit, the sun's light
/// reddens near the horizon and goes out below it, as with the built-in
/// atmosphere. [`DayNight`] moves the sun and the moon through a day.
///
/// ```no_run
/// # use ponko::prelude::*;
/// use ponko::render::{CloudQuality, SkyKit};
/// # fn f(ctx: &mut Ctx) {
/// let mut kit = SkyKit::default();
/// kit.clouds.coverage = 0.6;
/// kit.clouds.wind = vec2(12.0, 4.0); // m/s
/// kit.quality = CloudQuality::High;
/// ctx.scene3d.sky.kit = Some(kit);
/// # }
/// ```
///
/// # Cost
///
/// Nothing while [`Sky::kit`] is `None`: no passes, textures or shaders are
/// made. With it: the sky shader, one 128 x 128 cloud shadow pass, and with
/// a volumetric [`CloudQuality`] a cloud march at a quarter of the frame's
/// pixels, one temporal resolve and one composite over the sky's pixels
/// (measured times per preset are in [`CloudQuality`]). On a WebGL2-class
/// GPU the clouds are always the flat layer.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SkyKit {
    /// The clouds: how many, what kind, the wind.
    pub clouds: Clouds,
    /// The moon: where, its phase, its size.
    pub moon: Moon,
    /// How finely the clouds are drawn. Default [`CloudQuality::Medium`].
    pub quality: CloudQuality,
}

impl Default for SkyKit {
    fn default() -> Self {
        Self {
            clouds: Clouds::default(),
            moon: Moon::default(),
            quality: CloudQuality::Medium,
        }
    }
}

impl SkyKit {
    /// The kit with `quality`.
    ///
    /// ```
    /// use ponko::render::{CloudQuality, SkyKit};
    /// let kit = SkyKit::default().with_quality(CloudQuality::Flat);
    /// assert!(!kit.quality.is_volumetric());
    /// ```
    #[must_use]
    pub fn with_quality(mut self, quality: CloudQuality) -> Self {
        self.quality = quality;
        self
    }

    /// The kit with `coverage` of the sky clouded (0 to 1).
    ///
    /// ```
    /// use ponko::render::SkyKit;
    /// assert_eq!(SkyKit::default().with_coverage(0.9).clouds.coverage, 0.9);
    /// ```
    #[must_use]
    pub fn with_coverage(mut self, coverage: f32) -> Self {
        self.clouds.coverage = coverage;
        self
    }
}

/// The sky kit's clouds ([`SkyKit::clouds`]). Distances are in world units,
/// taken to be meters.
///
/// ```
/// use ponko::render::Clouds;
/// let mut storm = Clouds::default();
/// storm.coverage = 0.85;
/// storm.kind = 1.0; // towering
/// storm.self_shadow = 1.6; // dark undersides
/// assert!(storm.coverage > Clouds::default().coverage);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Clouds {
    /// How much of the sky is clouded, 0 (clear) to 1 (overcast). Default
    /// 0.5.
    pub coverage: f32,
    /// What kind of clouds: 0 flat, layered sheets (stratus), 1 tall heaps
    /// with round tops (cumulus), or between. Default 0.5.
    pub kind: f32,
    /// Which way and how fast the clouds drift, in world units per second
    /// along X and Z. Default (6, 2).
    pub wind: Vec2,
    /// How far the clouds have drifted on top of what the wind did: set it
    /// to put them somewhere (the same on every machine of a networked
    /// game). Default zero.
    pub offset: Vec2,
    /// The height of the layer's base. Default 1500.
    pub altitude: f32,
    /// How deep the layer is. Default 1500.
    pub thickness: f32,
    /// How thick the clouds are inside: more is whiter in the sun and
    /// darker in shade. Default 1.
    pub density: f32,
    /// How much the clouds shadow themselves, 0 (evenly lit) up: more
    /// gives darker undersides and deeper folds. Default 1.
    pub self_shadow: f32,
    /// How brightly cloud edges glow looking toward the sun or the moon,
    /// 0 (not) to 1. Default 0.6.
    pub silver_lining: f32,
    /// How dark the clouds' shadows on the ground are, 0 (none, and no
    /// shadow pass) to 1. Default 0.8.
    pub shadows: f32,
}

impl Default for Clouds {
    fn default() -> Self {
        Self {
            coverage: 0.5,
            kind: 0.5,
            wind: Vec2::new(6.0, 2.0),
            offset: Vec2::ZERO,
            altitude: 1500.0,
            thickness: 1500.0,
            density: 1.0,
            self_shadow: 1.0,
            silver_lining: 0.6,
            shadows: 0.8,
        }
    }
}

/// The sky kit's moon ([`SkyKit::moon`]).
///
/// ```
/// use ponko::render::Moon;
/// let full = Moon { phase: 0.5, ..Moon::default() };
/// assert!((full.illumination() - 1.0).abs() < 1e-6);
/// assert!(Moon { phase: 0.0, ..Moon::default() }.illumination() < 1e-6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moon {
    /// Which way moonlight travels (like [`Lighting::sun_direction`]); `None`
    /// (the default): the moon follows the sun across the sky, as far
    /// behind it as its [`phase`](Self::phase) says (with the sun when new,
    /// opposite it when full).
    pub direction: Option<Vec3>,
    /// Where it is in its cycle: 0 new, 0.25 first quarter (lit on the
    /// right), 0.5 full, 0.75 last quarter, 1 new again. Default 0.5.
    pub phase: f32,
    /// Its size in the sky, in degrees across. Default 2 (the real moon is
    /// about 0.5, which looks small on a screen).
    pub size: f32,
    /// How bright it is, 0 for none. Default 1.
    pub brightness: f32,
}

impl Default for Moon {
    fn default() -> Self {
        Self {
            direction: None,
            phase: 0.5,
            size: 2.0,
            brightness: 1.0,
        }
    }
}

impl Moon {
    /// The share of the moon's face that is lit at its phase: 0 new, 1 full.
    #[must_use]
    pub fn illumination(&self) -> f32 {
        0.5 - 0.5 * (self.phase * std::f32::consts::TAU).cos()
    }
}

/// How finely the sky kit draws its clouds ([`SkyKit::quality`]).
///
/// The volumetric presets march the cloud layer at half the frame's width
/// and height (a quarter of its pixels), from a different offset each
/// frame, and blend each frame with the last one moved to where the camera
/// now looks (temporal reprojection), so the steps' noise averages out.
/// They differ in how many steps a ray takes through the clouds and toward
/// the sun. What the kit adds to a frame's GPU time at 1920 x 1080 (the
/// whole frame's, against the same frame with the built-in atmosphere),
/// measured offscreen on an Apple M4 Max (Metal) while the machine was
/// busy with other work: three runs of 40 frames each, cloud cover 0.5,
/// looking just above the horizon (`cloud_presets_gpu_time` in the engine's
/// tests):
///
/// | Preset | Steps (view, sun) | Kit's GPU time, median per run |
/// |---|---|---|
/// | `Flat` | none: a 2D layer in the sky shader | 0.46, 0.48, 0.60 ms |
/// | `Low` | 24, 2 | 1.02, 0.84, 0.79 ms |
/// | `Medium` | 48, 4 | 2.18, 1.83, 1.84 ms |
/// | `High` | 96, 6 | 3.88, 5.63, 9.88 ms (the march alone 3.9 to 4.3 ms; the rest is the busy machine) |
///
/// The cloud march itself scales with the steps (0.78, 1.7 to 1.8 and 3.9
/// to 4.3 ms); the cloud shadow map takes about 0.03 ms, the temporal
/// resolve 0.06 ms.
///
/// On a WebGL2-class GPU every preset draws as `Flat`.
///
/// ```
/// use ponko::render::CloudQuality;
/// assert_eq!(CloudQuality::default(), CloudQuality::Medium);
/// assert!(CloudQuality::High.steps().0 > CloudQuality::Low.steps().0);
/// assert_eq!(CloudQuality::Flat.steps(), (0, 0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CloudQuality {
    /// A flat cloud layer drawn into the sky shader: no cloud passes but
    /// the cloud shadows. What WebGL2 draws.
    Flat,
    /// Volumetric, 24 steps per ray and 2 toward the sun.
    Low,
    /// Volumetric, 48 steps per ray and 4 toward the sun.
    #[default]
    Medium,
    /// Volumetric, 96 steps per ray and 6 toward the sun.
    High,
}

impl CloudQuality {
    /// Every preset, cheapest first.
    pub const ALL: [CloudQuality; 4] = [
        CloudQuality::Flat,
        CloudQuality::Low,
        CloudQuality::Medium,
        CloudQuality::High,
    ];

    /// Whether the clouds are raymarched (not the flat layer).
    #[must_use]
    pub fn is_volumetric(self) -> bool {
        self != CloudQuality::Flat
    }

    /// Steps per ray through the clouds and toward the sun (zero for the
    /// flat layer).
    #[must_use]
    pub fn steps(self) -> (u32, u32) {
        match self {
            CloudQuality::Flat => (0, 0),
            CloudQuality::Low => (24, 2),
            CloudQuality::Medium => (48, 4),
            CloudQuality::High => (96, 6),
        }
    }

    /// Whether each frame marches a quarter of the cloud image's pixels (one
    /// of every 2 x 2 block, a different one each frame) and fills in the
    /// rest from the frames before: `Low` and `Medium`, the presets for
    /// mid-range GPUs. `High` marches every pixel each frame.
    ///
    /// ```
    /// use ponko::render::CloudQuality;
    /// assert!(CloudQuality::Medium.interleaved());
    /// assert!(!CloudQuality::High.interleaved());
    /// ```
    #[must_use]
    pub fn interleaved(self) -> bool {
        matches!(self, CloudQuality::Low | CloudQuality::Medium)
    }

    /// The preset's name: `flat`, `low`, `medium` or `high`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            CloudQuality::Flat => "flat",
            CloudQuality::Low => "low",
            CloudQuality::Medium => "medium",
            CloudQuality::High => "high",
        }
    }
}

/// A day and night cycle for the sky kit: moves the sun through the day
/// ([`Sky::sun_direction_at`]), the moon through its phases, and dims the
/// night light with the moon. Hold one in the game and call
/// [`update`](Self::update) each frame (or [`advance`](Self::advance) and
/// [`apply`](Self::apply) to drive it yourself).
///
/// ```no_run
/// # use ponko::prelude::*;
/// use ponko::render::DayNight;
/// let mut day = DayNight::new(7.0); // seven in the morning
/// day.hours_per_second = 0.2; // a day in two minutes
/// Game::new("day", 1280, 720)
///     .system(move |ctx: &mut Ctx| day.update(ctx))
///     .run(|_| {});
/// ```
///
/// [`apply`](Self::apply) turns the kit on if it is off. Turning
/// [`Lighting::shadow_sun_step`] up a little (0.01) keeps the sun's
/// shadows still between steps while the sun moves.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DayNight {
    /// The time of day, 0 to 24.
    pub hour: f32,
    /// Whole days gone by: the moon's phase moves on each day.
    pub day: u32,
    /// How fast the day goes: game hours per real second. Default 0.1 (a
    /// day in four minutes); 0 holds the time.
    pub hours_per_second: f32,
    /// Days from one new moon to the next. Default 29.5, as the real moon;
    /// fewer shows the phases sooner.
    pub days_per_moon: f32,
    /// The moon's phase on day 0 at midnight (see [`Moon::phase`]).
    /// Default 0.5 (full).
    pub first_phase: f32,
    /// The sun's color (before the atmosphere reddens it near the
    /// horizon). Default as [`Lighting`]'s.
    pub sun_color: Color,
    /// The sun's brightness (before the atmosphere dims it near the
    /// horizon). Default as [`Lighting`]'s.
    pub sun_intensity: f32,
    /// The night's light under a full moon; a new moon leaves a fifth of
    /// it. Default as [`Sky::night_light`].
    pub night_light: Color,
}

impl DayNight {
    /// A cycle starting at `hour` o'clock on day 0.
    ///
    /// ```
    /// use ponko::render::DayNight;
    /// let day = DayNight::new(12.0);
    /// assert_eq!((day.hour, day.day), (12.0, 0));
    /// ```
    #[must_use]
    pub fn new(hour: f32) -> Self {
        let lighting = Lighting::default();
        Self {
            hour: hour.rem_euclid(24.0),
            day: 0,
            hours_per_second: 0.1,
            days_per_moon: 29.5,
            first_phase: 0.5,
            sun_color: lighting.sun_color,
            sun_intensity: lighting.sun_intensity,
            night_light: Sky::default().night_light,
        }
    }

    /// Moves the clock on by `seconds` of real time
    /// ([`hours_per_second`](Self::hours_per_second) each), counting the
    /// days.
    ///
    /// ```
    /// use ponko::render::DayNight;
    /// let mut day = DayNight::new(23.0);
    /// day.hours_per_second = 1.0;
    /// day.advance(2.0);
    /// assert_eq!(day.day, 1);
    /// assert!((day.hour - 1.0).abs() < 1e-4);
    /// ```
    pub fn advance(&mut self, seconds: f32) {
        let hours = self.hour + seconds.max(0.0) * self.hours_per_second.max(0.0);
        if !hours.is_finite() {
            return;
        }
        let days = (hours / 24.0).floor();
        self.day = self.day.saturating_add(days as u32);
        self.hour = hours - days * 24.0;
    }

    /// The moon's phase now (see [`Moon::phase`]).
    ///
    /// ```
    /// use ponko::render::DayNight;
    /// let mut day = DayNight::new(0.0);
    /// day.first_phase = 0.0;
    /// day.days_per_moon = 8.0;
    /// day.day = 4;
    /// assert!((day.moon_phase() - 0.5).abs() < 1e-6, "full after half a cycle");
    /// ```
    #[must_use]
    pub fn moon_phase(&self) -> f32 {
        let days = self.day as f32 + self.hour / 24.0;
        (self.first_phase + days / self.days_per_moon.max(0.01)).rem_euclid(1.0)
    }

    /// Sets `scene` to this time: the sun's direction, color and brightness,
    /// the moon's phase (and the kit on, if it was off), and the night
    /// light for the moon.
    ///
    /// ```
    /// use ponko::render::{DayNight, SceneSettings};
    /// let mut scene = SceneSettings::default();
    /// DayNight::new(12.0).apply(&mut scene);
    /// assert!(scene.sky.kit.is_some());
    /// assert!(scene.lighting.sun_direction.y < -0.8, "noon: the sun is high");
    /// ```
    pub fn apply(&self, scene: &mut SceneSettings) {
        let lighting = &mut scene.lighting;
        lighting.sun_direction = Sky::sun_direction_at(self.hour);
        lighting.sun_color = self.sun_color;
        lighting.sun_intensity = self.sun_intensity;
        let sky = &mut scene.sky;
        let kit = sky.kit.get_or_insert_with(SkyKit::default);
        kit.moon.phase = self.moon_phase();
        let moonlight = 0.2 + 0.8 * kit.moon.illumination();
        let n = self.night_light;
        sky.night_light = Color::rgba(n.r * moonlight, n.g * moonlight, n.b * moonlight, n.a);
    }

    /// [`advance`](Self::advance) by this frame's time, then
    /// [`apply`](Self::apply) to `ctx.scene3d`: the whole cycle, as a system.
    pub fn update(&mut self, ctx: &mut crate::ctx::Ctx) {
        self.advance(ctx.time.dt());
        self.apply(&mut ctx.scene3d);
    }
}

/// The axis the sky turns around in [`Sky::sun_direction_at`]'s day: the
/// sun's path is the circle at right angles to it (the celestial pole,
/// 30 degrees up toward +Z).
pub(crate) fn sky_pole() -> Vec3 {
    let tilt = 30f32.to_radians();
    Vec3::new(0.0, tilt.sin(), tilt.cos())
}

/// `v` turned by `angle` (radians) around unit axis `axis`.
fn turned(v: Vec3, axis: Vec3, angle: f32) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - c)
}

/// How far around the sky the sun is (its hour angle, radians): 0 rising in
/// the east, π/2 highest, π setting. The stars turn by it.
pub(crate) fn hour_angle(to_sun: Vec3) -> f32 {
    let pole = sky_pole();
    let east = Vec3::X;
    let up = pole.cross(east);
    to_sun.dot(up).atan2(to_sun.dot(east))
}

/// The side of the cloud shadow map, in texels (a square in the IBL atlas).
pub(crate) const CLOUD_SHADOW_TEXELS: u32 = 128;
/// How far the cloud shadow map reaches across, in world units.
pub(crate) const CLOUD_SHADOW_EXTENT: f32 = 8000.0;

impl SkyKit {
    /// The direction toward the moon, for the sun toward `to_sun`.
    pub(crate) fn to_moon(self, to_sun: Vec3) -> Vec3 {
        match self.moon.direction.and_then(|d| (-d).try_normalize()) {
            Some(d) => d,
            None => turned(to_sun, sky_pole(), -self.moon.phase * std::f32::consts::TAU),
        }
    }

    /// Where the cloud shadow map is centered (on the cloud layer's base,
    /// under the camera along the sun), snapped to its texels so the
    /// shadows don't crawl as the camera moves.
    pub(crate) fn shadow_center(&self, to_sun: Vec3, camera: Vec3) -> Vec2 {
        let up = to_sun.y.max(0.05);
        let rise = (self.clouds.altitude - camera.y).max(0.0) / up;
        // Far behind a low sun is no use: the map covers the near ground.
        let reach = (rise * to_sun.length()).min(CLOUD_SHADOW_EXTENT * 0.4);
        let flat = Vec2::new(to_sun.x, to_sun.z);
        let center = Vec2::new(camera.x, camera.z) + flat.normalize_or_zero() * reach * flat.length() / to_sun.length();
        let texel = CLOUD_SHADOW_EXTENT / CLOUD_SHADOW_TEXELS as f32;
        (center / texel).round() * texel
    }

    /// What the kit's shaders read (`frame.sky_kit` in WGSL), for the sun
    /// toward `to_sun`, the camera at `camera`, clouds blown `drift` so far,
    /// and whether the background's clouds come from the cloud passes
    /// (`volumetric`) or the flat layer.
    ///
    /// - 0: toward the moon (xyz), its phase;
    /// - 1: coverage, kind, drift (x, z);
    /// - 2: base height, thickness, density, self-shadowing;
    /// - 3: silver lining, the stars' turn (radians), cos of the moon's
    ///   radius, the moon's brightness;
    /// - 4: the shadow map's center (x, z), 1 / its extent, shadow strength
    ///   (0: no cloud shadows);
    /// - 5: 1 when the cloud passes draw the background's clouds, 1 (the
    ///   kit is on), the stars' brightness ([`Sky::stars`]), 0.
    pub(crate) fn frame_params(
        &self,
        to_sun: Vec3,
        camera: Vec3,
        drift: Vec2,
        volumetric: bool,
        stars: f32,
    ) -> [[f32; 4]; 6] {
        let c = &self.clouds;
        let moon = self.to_moon(to_sun);
        let center = self.shadow_center(to_sun, camera);
        let drift = drift + c.offset;
        let radius = (self.moon.size.clamp(0.05, 30.0) * 0.5).to_radians();
        let shadows = if self.casts_shadows() { c.shadows.min(1.0) } else { 0.0 };
        [
            [moon.x, moon.y, moon.z, self.moon.phase.rem_euclid(1.0)],
            [c.coverage.clamp(0.0, 1.0), c.kind.clamp(0.0, 1.0), drift.x, drift.y],
            [
                c.altitude,
                c.thickness.max(1.0),
                c.density.max(0.0),
                c.self_shadow.max(0.0),
            ],
            [
                c.silver_lining.clamp(0.0, 1.0),
                hour_angle(to_sun),
                radius.cos(),
                self.moon.brightness.max(0.0),
            ],
            [center.x, center.y, 1.0 / CLOUD_SHADOW_EXTENT, shadows],
            [f32::from(volumetric), 1.0, stars.max(0.0), 0.0],
        ]
    }

    /// Whether the cloud shadow map is drawn and read.
    pub(crate) fn casts_shadows(&self) -> bool {
        self.clouds.shadows > 0.0 && self.clouds.coverage > 0.0 && self.clouds.density > 0.0
    }

    /// Sixteen values that change what the sky texture (fog, reflections,
    /// ambient light) shows: the reflections are baked again when they
    /// change. The drift is counted in steps of 2 km, so drifting clouds
    /// rebake rarely.
    pub(crate) fn bake_key(&self, drift: Vec2) -> [f32; 16] {
        let c = &self.clouds;
        let drift = (drift + c.offset) / 2000.0;
        let moon = self.moon.direction.unwrap_or(Vec3::ZERO);
        [
            c.coverage,
            c.kind,
            c.altitude,
            c.thickness,
            c.density,
            c.self_shadow,
            c.silver_lining,
            drift.x.round(),
            drift.y.round(),
            self.moon.phase,
            self.moon.size,
            self.moon.brightness,
            moon.x,
            moon.y,
            moon.z,
            f32::from(self.moon.direction.is_some()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_moon_rises_as_the_sun_sets_and_a_new_moon_stays_with_the_sun() {
        let mut kit = SkyKit::default();
        let to_sun = -Sky::sun_direction_at(18.0);
        kit.moon.phase = 0.5;
        let full = kit.to_moon(to_sun);
        assert!((full + to_sun).length() < 1e-4, "opposite the sun: {full}");
        kit.moon.phase = 0.0;
        assert!((kit.to_moon(to_sun) - to_sun).length() < 1e-4);
        // First quarter: highest at sunset, a quarter of the sky behind.
        kit.moon.phase = 0.25;
        let quarter = kit.to_moon(to_sun);
        assert!(quarter.y > 0.8, "{quarter}");
        // A direction of its own wins.
        kit.moon.direction = Some(Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(kit.to_moon(to_sun), Vec3::Y);
    }

    #[test]
    fn the_stars_turn_with_the_sun_once_a_day() {
        let angle = |h: f32| hour_angle(-Sky::sun_direction_at(h));
        assert!(angle(6.0).abs() < 1e-4);
        assert!((angle(12.0) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        assert!((angle(18.0).abs() - std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn the_cloud_shadow_map_follows_the_camera_in_whole_texels() {
        let kit = SkyKit::default();
        let to_sun = Vec3::Y;
        let texel = CLOUD_SHADOW_EXTENT / CLOUD_SHADOW_TEXELS as f32;
        let a = kit.shadow_center(to_sun, Vec3::new(10.0, 0.0, 0.0));
        let b = kit.shadow_center(to_sun, Vec3::new(20.0, 0.0, 0.0));
        assert_eq!(a, b, "a small step keeps the map still");
        let far = kit.shadow_center(to_sun, Vec3::new(1000.0, 0.0, 0.0));
        assert!((far.x / texel - (far.x / texel).round()).abs() < 1e-3);
        assert!((far.x - 1000.0).abs() <= texel);
        // A low sun from +X: the clouds shading the camera are toward the sun.
        let low = kit.shadow_center(Vec3::new(1.0, 0.5, 0.0).normalize(), Vec3::ZERO);
        assert!(low.x > 1000.0, "{low}");
    }

    #[test]
    fn no_cloud_shadows_without_clouds_or_strength() {
        let mut kit = SkyKit::default();
        assert!(kit.casts_shadows());
        let params = kit.frame_params(Vec3::Y, Vec3::ZERO, Vec2::ZERO, true, 1.0);
        assert!(params[4][3] > 0.0 && params[5][0] == 1.0);
        kit.clouds.coverage = 0.0;
        assert!(!kit.casts_shadows());
        assert_eq!(kit.frame_params(Vec3::Y, Vec3::ZERO, Vec2::ZERO, false, 1.0)[4][3], 0.0);
        kit.clouds.coverage = 0.5;
        kit.clouds.shadows = 0.0;
        assert!(!kit.casts_shadows());
    }

    #[test]
    fn slow_drift_changes_the_bake_key_only_every_two_kilometers() {
        let kit = SkyKit::default();
        assert_eq!(kit.bake_key(Vec2::ZERO), kit.bake_key(Vec2::new(500.0, 0.0)));
        assert_ne!(kit.bake_key(Vec2::ZERO), kit.bake_key(Vec2::new(3000.0, 0.0)));
    }

    #[test]
    fn the_day_night_cycle_counts_days_and_moves_the_moon() {
        let mut day = DayNight::new(22.0);
        day.hours_per_second = 2.0;
        day.days_per_moon = 4.0;
        day.first_phase = 0.0;
        let before = day.moon_phase();
        day.advance(2.0); // four hours: 2 o'clock the next day
        assert_eq!(day.day, 1);
        assert!((day.hour - 2.0).abs() < 1e-4);
        assert!(day.moon_phase() > before);
        let mut scene = SceneSettings::default();
        day.apply(&mut scene);
        assert_eq!(scene.lighting.sun_direction, Sky::sun_direction_at(2.0));
        let kit = scene.sky.kit.expect("apply turns the kit on");
        assert_eq!(kit.moon.phase, day.moon_phase());
        // A new moon leaves less night light than a full one.
        let dim = scene.sky.night_light;
        day.first_phase = 0.5 - day.moon_phase();
        day.apply(&mut scene);
        assert!(
            scene.sky.night_light.b > dim.b,
            "{:?} vs {dim:?}",
            scene.sky.night_light
        );
        // Held time.
        day.hours_per_second = 0.0;
        let hour = day.hour;
        day.advance(100.0);
        assert_eq!(day.hour, hour);
    }
}
