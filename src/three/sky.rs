//! The sky behind the 3D world and the fog in front of it ([`Sky`]), and
//! the CPU half of the atmosphere: how much sunlight gets through the air
//! (the sun's color low in the sky) and the light the sky gives the scene.
//! The GPU half, which draws the sky, is `sky.wgsl`; both use the same
//! model and constants.

use glam::{Vec3, Vec4};

use crate::color::Color;
use crate::render::Lighting;

use super::linear;

/// The sky behind the 3D world, the light it gives the scene, and the fog
/// in front of it: [`SceneSettings::sky`](crate::render::SceneSettings::sky)
/// (`ctx.scene3d.sky`).
///
/// The sky is one of two kinds, chosen by [`atmosphere`](Self::atmosphere):
///
/// - **Atmosphere** (the default): the sky is worked out from the sun in
///   [`Lighting`]: blue by day, orange and red when the sun is low, dark with
///   stars at night. The sun itself turns orange near the horizon and sets,
///   and the sky's own light lights the scene. One world unit is taken to be
///   a meter.
/// - **Flat** (`atmosphere` off): the background is the clear color
///   ([`Graphics::clear`](crate::gfx::Graphics::clear)), the ambient light
///   comes from [`sky_color`](Self::sky_color),
///   [`ground_color`](Self::ground_color) and
///   [`equator_color`](Self::equator_color), and the fog is
///   [`fog_color`](Self::fog_color).
///
/// Those four colors are only for the flat sky; the atmosphere makes its own.
///
/// Or a sky of the game's own: a [`SkyShader`] in [`shader`](Self::shader)
/// (stylized gradients, painted clouds, a night sky) takes the place of
/// both. The engine uses it wherever it looks at the sky: the background,
/// the fog's color, reflections and the ambient light, and what reflection
/// probes capture. The engine brings one ready-made: the sky kit
/// ([`kit`](Self::kit), [`SkyKit`](crate::render::SkyKit)), the atmosphere
/// with raymarched clouds, a moon with phases and stars.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// // Late evening, a little hazy, with fog lying in the valleys.
/// ctx.scene3d.lighting.sun_direction = ponko::render::Sky::sun_direction_at(18.5);
/// ctx.scene3d.sky.haze = 3.0;
/// ctx.scene3d.sky.fog_density = 0.01;
/// ctx.scene3d.sky.fog_height = 5.0;
/// // Or a flat, stylized sky: a background color and ambient colors.
/// ctx.gfx.clear(Color::hex(0x9CC4E4));
/// ctx.scene3d.sky.atmosphere = false;
/// ctx.scene3d.sky.sky_color = Color::hex(0x9CC4E4);
/// # }
/// ```
///
/// # Defaults and cost
///
/// [`Default`] is the physical atmosphere on, a haze of 1, a sun disc of 1.5
/// degrees, stars at 1, and fog and light shafts off. Fog costs nothing while
/// its density is 0. Light shafts cost 16 shadow map taps per pixel and need
/// the sun's shadows. A [`SkyShader`] runs your code wherever the engine
/// looks at the sky (background, fog color, reflections, ambient light and
/// what reflection probes capture).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Sky {
    /// Which kind of sky: on, a physical sky lit by the sun that lights the
    /// scene too; off, a flat sky (the clear color behind the world, the
    /// ambient light from [`sky_color`](Self::sky_color),
    /// [`ground_color`](Self::ground_color) and
    /// [`equator_color`](Self::equator_color)). Default on.
    pub atmosphere: bool,
    /// Flat sky only: light from the sky, on surfaces facing up. With an
    /// atmosphere, the sky's own light is used instead. Default a grey blue.
    pub sky_color: Color,
    /// Flat sky only: light bounced off the ground, on surfaces facing down.
    /// Default a dark brown.
    pub ground_color: Color,
    /// Flat sky only: light from the horizon, on surfaces facing sideways:
    /// with it, the ambient light goes from `ground_color` below through this
    /// to `sky_color` above (a three-color gradient); without it (`None`, the
    /// default), straight from ground to sky.
    pub equator_color: Option<Color>,
    /// Dust and water in the air, from 0 (none) up: more makes the sky
    /// paler, the glow around the sun bigger and sunsets redder. Default 1.
    pub haze: f32,
    /// The sun's size in the sky, in degrees across. Default 1.5 (the real
    /// sun is about 0.5, which looks small on a screen).
    pub sun_size: f32,
    /// How bright the stars are at night, 0 for none. Default 1.
    pub stars: f32,
    /// Light at night, when the sun is down: moonlight and starlight.
    /// Default a dim blue.
    pub night_light: Color,
    /// How thick the fog is near the ground: the share of light it takes
    /// away per world unit. 0 (the default) is no fog; 0.002 hazes the far
    /// distance; 0.05 is thick.
    pub fog_density: f32,
    /// The height the fog lies at: below it, it's thickest. Default 0.
    pub fog_height: f32,
    /// How quickly the fog thins out above `fog_height`, in world units:
    /// it halves about every 0.7 times this. Default 50.
    pub fog_falloff: f32,
    /// Flat sky only: the fog's color. With an atmosphere, the fog takes the
    /// sky's color behind it. Default a pale grey-blue.
    pub fog_color: Color,
    /// Distance fog: none closer than `fog_start` world units, all fog
    /// past `fog_end`, and a straight blend between. It hides the far end
    /// of a big world whatever the height. Off while `fog_end` is not
    /// past `fog_start` (default 0 and 0). The sky itself takes none.
    pub fog_start: f32,
    /// Where the distance fog hides everything. Default 0 (off).
    pub fog_end: f32,
    /// How much sunlight the fog scatters toward the camera, so it glows
    /// when looking toward the sun: 0 (the default) none, 0.1 a soft glow.
    pub fog_sun_glow: f32,
    /// How tight that glow is around the sun, 0 (all around) to just
    /// under 1 (a small bright halo). Default 0.7.
    pub fog_sun_focus: f32,
    /// Light shafts: how much shadows cut the fog's sun glow, from 0 (the
    /// default, off) to 1. Where the fog is in a tree's or a house's
    /// shadow it stops glowing, so beams of sunlight show between them.
    /// It needs fog, a `fog_sun_glow` and the sun's shadows, and reaches
    /// as far as they do. It costs 16 shadow map taps per pixel, and it is
    /// grainy without [`AntiAliasing::Taa`](crate::render::AntiAliasing::Taa) in
    /// `ctx.scene3d.post`, which smooths it out.
    pub light_shafts: f32,
    /// A sky of the game's own, in place of the atmosphere and the flat
    /// sky (see [`SkyShader`]): its light is the background, the fog's
    /// color and the reflections, and the ambient light unless
    /// [`authored_ambient`](Self::authored_ambient). The sun keeps
    /// [`Lighting`]'s color and intensity as authored (the atmosphere's
    /// reddening is off), [`sun_size`](Self::sun_size) and
    /// [`stars`](Self::stars) still draw the sun's disk and the stars, and
    /// all of the fog settings apply. `None` (the default): the atmosphere or
    /// the flat sky, as [`atmosphere`](Self::atmosphere) says.
    pub shader: Option<SkyShader>,
    /// Four values the sky shader reads as `frame.sky_params[0]` to `[3]`
    /// (a time of day, cloud cover, colors). The reflections, the ambient
    /// light and the fog follow a change within a few frames: they are
    /// rebuilt from the sky at most every
    /// [`SkyShader::REBUILD_FRAMES`] frames, while the background shows it
    /// at once. Default all zero.
    pub shader_params: [Vec4; 4],
    /// With a [`shader`](Self::shader): the ambient light comes from the
    /// authored [`sky_color`](Self::sky_color),
    /// [`ground_color`](Self::ground_color) and
    /// [`equator_color`](Self::equator_color), as for the flat sky, while
    /// the background, the fog and the reflections still show the shader's
    /// sky. Default off: the ambient light is the sky's own light.
    pub authored_ambient: bool,
    /// The sky kit: a ready-made sky with clouds, a moon and stars, on the
    /// sky shader hook (see [`SkyKit`](crate::render::SkyKit)). It takes the
    /// place of the atmosphere and the flat sky like a [`shader`](Self::shader),
    /// which wins if both are set. `None` (the default) costs nothing.
    pub kit: Option<crate::render::SkyKit>,
}

/// A sky shader: the game's own sky, made with
/// [`Graphics::create_sky_shader`](crate::gfx::Graphics::create_sky_shader) or
/// [`Graphics::load_sky_shader`](crate::gfx::Graphics::load_sky_shader); a
/// cheap handle. Show it by setting [`Sky::shader`].
///
/// The WGSL defines one function:
///
/// ```wgsl
/// fn sky(dir: vec3<f32>) -> vec3<f32>
/// ```
///
/// the sky's light (linear, before exposure and tone mapping) seen in unit
/// direction `dir` (world space, away from the camera), without the sun's
/// disk, which the engine adds. It runs for every background pixel, and for
/// the sky texture that fog, reflections and the ambient light read. It can
/// read `frame` (`frame.sun_dir`, `frame.sun_color`, `frame.time`,
/// [`Sky::shader_params`] as `frame.sky_params`) and the engine's helpers
/// (`hash2`, `noise2`, `fbm2`, `PI`); it declares no bindings of its own.
/// `drawing_background()` is true while it draws the background and false
/// for the sky texture: sharp detail the fog and the ambient light don't
/// need (stars, a moon's face) can be left out of the texture.
///
/// What the fog, reflections and ambient light show is rebuilt when the
/// sun, [`Lighting`] or [`Sky::shader_params`] change, not as `frame.time`
/// goes by: drive slow changes (a time of day) through the parameters, and
/// keep time-based motion (drifting clouds) small, since only the
/// background shows it.
///
/// ```no_run
/// # use ponko::prelude::*;
/// # fn f(ctx: &mut Ctx) {
/// let sunset = ctx.gfx.create_sky_shader("sunset", "
///     fn sky(dir: vec3<f32>) -> vec3<f32> {
///         let up = clamp(dir.y, 0.0, 1.0);
///         return mix(frame.sky_params[0].rgb, frame.sky_params[1].rgb, sqrt(up));
///     }");
/// ctx.scene3d.sky.shader = Some(sunset);
/// ctx.scene3d.sky.shader_params[0] = vec4(1.0, 0.45, 0.2, 0.0); // horizon
/// ctx.scene3d.sky.shader_params[1] = vec4(0.15, 0.2, 0.6, 0.0); // overhead
/// ctx.scene3d.sky.fog_density = 0.01; // the fog takes the sky's colors
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SkyShader {
    pub(crate) id: u32,
}

impl SkyShader {
    /// The fewest frames between two rebuilds of the fog's, the reflections'
    /// and the ambient light's sky from a sky shader: changing
    /// [`Sky::shader_params`] every frame costs one rebuild (a few
    /// milliseconds of GPU time) this often, not every frame.
    pub const REBUILD_FRAMES: u32 = 6;
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            atmosphere: true,
            sky_color: Color::hex(0x869BB0),
            ground_color: Color::hex(0x4D4135),
            equator_color: None,
            haze: 1.0,
            sun_size: 1.5,
            stars: 1.0,
            night_light: Color::hex(0x2C3A56),
            fog_density: 0.0,
            fog_height: 0.0,
            fog_falloff: 50.0,
            fog_color: Color::hex(0xB8C4D0),
            fog_start: 0.0,
            fog_end: 0.0,
            fog_sun_glow: 0.0,
            fog_sun_focus: 0.7,
            light_shafts: 0.0,
            shader: None,
            shader_params: [Vec4::ZERO; 4],
            authored_ambient: false,
            kit: None,
        }
    }
}

impl Sky {
    /// Which way sunlight travels at `hour` o'clock (0 to 24), for a day
    /// and night cycle: the sun rises in the east (+X) at 6, is highest at
    /// noon (60° up, to the south, -Z) and sets in the west at 18.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// assert!(ponko::render::Sky::sun_direction_at(12.0).y < -0.8, "noon: from high up");
    /// assert!(ponko::render::Sky::sun_direction_at(0.0).y > 0.0, "midnight: from below the ground");
    /// ```
    pub fn sun_direction_at(hour: f32) -> Vec3 {
        let angle = (hour - 6.0) / 12.0 * std::f32::consts::PI;
        let tilt = 30f32.to_radians();
        let sun = Vec3::new(angle.cos(), angle.sin() * tilt.cos(), -angle.sin() * tilt.sin());
        -sun
    }
}

// The atmosphere, in kilometers (as in sky.wgsl).
const GROUND: f32 = 6360.0;
const TOP: f32 = 6460.0;
const RAYLEIGH: Vec3 = Vec3::new(5.802e-3, 13.558e-3, 33.1e-3);
const RAYLEIGH_HEIGHT: f32 = 8.0;
const MIE_SCATTER: f32 = 3.996e-3;
const MIE_EXTINCTION: f32 = 4.44e-3;
const MIE_HEIGHT: f32 = 1.2;
const OZONE: Vec3 = Vec3::new(0.650e-3, 1.881e-3, 0.085e-3);
/// How bright the sky is for the sun's brightness: makes up for the light
/// scattered more than once, which the model leaves out. In sky.wgsl too.
pub(crate) const SKY_GAIN: f32 = 4.0;
/// How much of the light reaching the ground it reflects.
const GROUND_ALBEDO: f32 = 0.25;

/// How much light the air takes away per kilometer, at height `h` (km).
fn extinction(h: f32, haze: f32) -> Vec3 {
    let rayleigh = RAYLEIGH * (-h / RAYLEIGH_HEIGHT).exp();
    let mie = MIE_EXTINCTION * haze * (-h / MIE_HEIGHT).exp();
    let ozone = OZONE * (1.0 - (h - 25.0).abs() / 15.0).max(0.0);
    rayleigh + Vec3::splat(mie) + ozone
}

/// Distance from `origin` along `dir` to the sphere of `radius` around the
/// planet's center, from inside it.
fn exit_distance(origin: Vec3, dir: Vec3, radius: f32) -> f32 {
    let b = origin.dot(dir);
    let c = origin.length_squared() - radius * radius;
    -b + (b * b - c).max(0.0).sqrt()
}

/// Whether a ray from `origin` along `dir` hits the ground.
fn hits_ground(origin: Vec3, dir: Vec3) -> bool {
    let b = origin.dot(dir);
    let c = origin.length_squared() - GROUND * GROUND;
    b < 0.0 && b * b - c >= 0.0
}

/// The share of sunlight (per color) that reaches `origin` from `sun`
/// (towards the sun): 0 when the planet is in the way.
fn transmittance(origin: Vec3, sun: Vec3, haze: f32) -> Vec3 {
    if hits_ground(origin, sun) {
        return Vec3::ZERO;
    }
    const STEPS: usize = 16;
    let step = exit_distance(origin, sun, TOP) / STEPS as f32;
    let mut depth = Vec3::ZERO;
    for i in 0..STEPS {
        let p = origin + sun * (step * (i as f32 + 0.5));
        depth += extinction(p.length() - GROUND, haze) * step;
    }
    (-depth).exp()
}

/// Sky light from direction `dir` (towards the sky), for a sun of
/// brightness 1 in direction `sun` (towards the sun): single scattering.
fn radiance(dir: Vec3, sun: Vec3, haze: f32) -> Vec3 {
    const STEPS: usize = 12;
    let origin = Vec3::new(0.0, GROUND + 0.002, 0.0);
    let length = if hits_ground(origin, dir) {
        let b = origin.dot(dir);
        -b - (b * b - origin.length_squared() + GROUND * GROUND).max(0.0).sqrt()
    } else {
        exit_distance(origin, dir, TOP)
    };
    let step = length / STEPS as f32;
    let mu = dir.dot(sun);
    let rayleigh_phase = 3.0 / (16.0 * std::f32::consts::PI) * (1.0 + mu * mu);
    let g = 0.8f32;
    let mie_phase = 3.0 / (8.0 * std::f32::consts::PI) * ((1.0 - g * g) * (1.0 + mu * mu))
        / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * mu).powf(1.5));
    let mut depth = Vec3::ZERO;
    let mut light = Vec3::ZERO;
    for i in 0..STEPS {
        let p = origin + dir * (step * (i as f32 + 0.5));
        let h = p.length() - GROUND;
        let ext = extinction(h, haze);
        depth += ext * step * 0.5;
        let scatter = RAYLEIGH * (-h / RAYLEIGH_HEIGHT).exp() * rayleigh_phase
            + Vec3::splat(MIE_SCATTER * haze * (-h / MIE_HEIGHT).exp() * mie_phase);
        light += scatter * (-depth).exp() * transmittance(p, sun, haze) * step;
        depth += ext * step * 0.5;
    }
    light
}

/// The sun and ambient light the atmosphere gives: `lighting` with the
/// sun's color dimmed and reddened by the air it comes through, and its sky
/// and ground light worked out from the sky. Colors are linear.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct SkyLight {
    /// The sun's light at the ground (color times intensity, linear).
    pub sun: Vec3,
    /// Light from above and from below, before `ambient_intensity`.
    pub sky: Vec3,
    pub ground: Vec3,
}

impl SkyLight {
    /// The light as `lighting` and `sky` give it, with no atmosphere: the
    /// sun's own color and the sky's flat colors (the flat sky, and a sky
    /// shader's authored sun).
    pub fn authored(lighting: &Lighting, sky: &Sky) -> Self {
        let rgb = |c| {
            let [r, g, b, _] = linear(c);
            Vec3::new(r, g, b)
        };
        Self {
            sun: rgb(lighting.sun_color) * lighting.sun_intensity,
            sky: rgb(sky.sky_color),
            ground: rgb(sky.ground_color),
        }
    }

    /// Works it out; about 50 µs, so worth keeping while the sun and sky
    /// stay put.
    pub fn new(lighting: &Lighting, sky: &Sky) -> Self {
        let to_sun = (-lighting.sun_direction).try_normalize().unwrap_or(Vec3::Y);
        let [r, g, b, _] = linear(lighting.sun_color);
        let sun_out = Vec3::new(r, g, b) * lighting.sun_intensity;
        let ground = Vec3::new(0.0, GROUND + 0.002, 0.0);
        let haze = sky.haze.max(0.0);
        let sun = sun_out * transmittance(ground, to_sun, haze);
        // Cosine-weighted average of the sky over the upper half: four
        // rings of eight directions.
        let mut total = Vec3::ZERO;
        let mut weights = 0.0;
        for ring in 0..4 {
            let up = (ring as f32 + 0.5) / 4.0; // sin of the elevation
            let around = (1.0 - up * up).sqrt();
            for k in 0..8 {
                let a = k as f32 / 8.0 * std::f32::consts::TAU;
                let dir = Vec3::new(around * a.cos(), up, around * a.sin());
                total += radiance(dir, to_sun, haze) * up;
                weights += up;
            }
        }
        let [nr, ng, nb, _] = linear(sky.night_light);
        let night = Vec3::new(nr, ng, nb);
        let sky_light = total / weights * sun_out * SKY_GAIN + night;
        let ground_light = GROUND_ALBEDO * (sun * to_sun.y.max(0.0) / std::f32::consts::PI + sky_light);
        Self {
            sun,
            sky: sky_light,
            ground: ground_light,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(hour: f32) -> SkyLight {
        let lighting = Lighting {
            sun_direction: Sky::sun_direction_at(hour),
            sun_color: Color::WHITE,
            ..Default::default()
        };
        SkyLight::new(&lighting, &Sky::default())
    }

    #[test]
    fn the_sun_reddens_as_it_sets_and_is_gone_at_night() {
        let noon = light(12.0);
        let evening = light(17.8);
        let night = light(0.0);
        assert!(noon.sun.z > 0.7 * noon.sun.x, "white-ish at noon: {}", noon.sun);
        assert!(
            evening.sun.x > 2.0 * evening.sun.z,
            "red-orange low down: {}",
            evening.sun
        );
        assert!(evening.sun.x < noon.sun.x);
        assert_eq!(night.sun, Vec3::ZERO);
    }

    #[test]
    fn the_sky_is_blue_by_day_and_dark_at_night() {
        let noon = light(12.0);
        assert!(noon.sky.z > noon.sky.x, "blue: {}", noon.sky);
        let night = light(0.0);
        assert!(
            night.sky.length() < noon.sky.length() / 3.0,
            "{} vs {}",
            night.sky,
            noon.sky
        );
        assert!(night.sky.length() > 0.0, "night light");
        let lighting = Lighting {
            sun_direction: Sky::sun_direction_at(0.0),
            ..Default::default()
        };
        let moonless = Sky {
            night_light: Color::rgb(0.0, 0.0, 0.0),
            ..Default::default()
        };
        let dark = SkyLight::new(&lighting, &moonless).sky;
        assert!(dark.length() < noon.sky.length() / 100.0, "{dark} vs {}", noon.sky);
    }

    #[test]
    fn the_sun_crosses_the_sky_east_to_west() {
        let to_sun = |h| -Sky::sun_direction_at(h);
        assert!(to_sun(6.0).y.abs() < 1e-5 && to_sun(6.0).x > 0.99, "rises in the east");
        assert!(
            (to_sun(12.0).y - 30f32.to_radians().cos()).abs() < 1e-5,
            "60° up at noon"
        );
        assert!(to_sun(18.0).x < -0.99, "sets in the west");
    }
}
