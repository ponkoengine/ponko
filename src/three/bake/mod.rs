//! Baked lighting: the light that bounces around a scene that doesn't move,
//! worked out ahead of time on the CPU and stored in lightmaps (for the
//! models that keep still) and a probe volume (for the ones that move
//! through it). See [`bake_lighting`].
//!
//! Only the indirect light is baked: the sky's light (shadowed by what's
//! around), light bounced off the surfaces around (the sun's, the lamps'
//! and the sky's, up to [`LightBakeSettings::bounces`] times) and the light
//! of glowing surfaces. The sun and the lamps still light everything
//! directly every frame, with their real-time shadows, so moving models
//! still cast shadows on baked ones.

mod bvh;
mod pack;
mod trace;
mod uv;

use glam::{Mat3, Mat4, UVec3, Vec2, Vec3};
use rayon::prelude::*;

use crate::color::Color;
use crate::error::{Error, ErrorKind};
use crate::three::{Lighting, MaterialDesc, MeshData, PointLight3D, Sky, SpotLight3D, linear};
use bvh::Tri;
use trace::{Lamp, Rng, SkyLight, Surface, TraceScene};

/// How [`bake_lighting`] bakes: how fine, how many rays, how many bounces.
///
/// ```
/// let fast = ponko::render::LightBakeSettings::default().with_samples(32).with_texels_per_unit(2.0);
/// assert_eq!(fast.bounces, 3);
/// ```
///
/// New fields may be added in any release: build one with [`Default`] and
/// the `with_*` methods.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LightBakeSettings {
    /// Lightmap texels per world unit along a surface: 4 is a texel every
    /// 25 cm in a world of meters. Each model's lightmap gets as many as
    /// its surface area needs at this density (up to a page). Default 4.
    pub texels_per_unit: f32,
    /// Rays gathered per texel and per probe: more is less noisy and
    /// slower (the time grows with it). Default 128.
    pub samples: u32,
    /// How many times light bounces off surfaces on its way to a texel: 0
    /// is the sky's light alone (and glowing surfaces), 1 adds the light of
    /// lit surfaces, and so on. Default 3.
    pub bounces: u32,
    /// The lightmap pages' largest width and height, in texels: 64 to
    /// 2048. Models' lightmaps are packed onto as many pages as they need,
    /// up to 16; a bake that fits on one smaller page (a power of two from
    /// 64) gets that instead. Default 1024.
    pub page_size: u32,
    /// Also bakes the direction most light comes from, per texel, so normal
    /// maps still shape baked light (twice the memory). Default on.
    pub directional: bool,
    /// The probe volume's spacing, in world units: a grid of probes around
    /// every model in the bake lights the models that have no lightmap
    /// (moving ones, instance batches). 0: no probes. Default 2.
    pub probe_spacing: f32,
    /// Smooths each lightmap's noise, keeping edges between surfaces facing
    /// different ways. Default on.
    pub denoise: bool,
    /// Where the random numbers start: the same seed bakes the same result
    /// on any machine and any number of threads. Default 0.
    pub seed: u64,
}

impl Default for LightBakeSettings {
    fn default() -> Self {
        Self {
            texels_per_unit: 4.0,
            samples: 128,
            bounces: 3,
            page_size: 1024,
            directional: true,
            probe_spacing: 2.0,
            denoise: true,
            seed: 0,
        }
    }
}

impl LightBakeSettings {
    with_fields! {
        /// Sets [`texels_per_unit`](Self::texels_per_unit).
        texels_per_unit: f32 => with_texels_per_unit;
        /// Sets [`samples`](Self::samples).
        samples: u32 => with_samples;
        /// Sets [`bounces`](Self::bounces).
        bounces: u32 => with_bounces;
        /// Sets [`page_size`](Self::page_size).
        page_size: u32 => with_page_size;
        /// Sets [`directional`](Self::directional).
        directional: bool => with_directional;
        /// Sets [`probe_spacing`](Self::probe_spacing).
        probe_spacing: f32 => with_probe_spacing;
        /// Sets [`denoise`](Self::denoise).
        denoise: bool => with_denoise;
        /// Sets [`seed`](Self::seed).
        seed: u64 => with_seed;
    }
}

/// One model in a [`LightBakeScene`]: its triangles where it stands in the
/// world, and what its surface is like.
///
/// ```
/// # use ponko::prelude::*;
/// let mut floor = ponko::render::MeshData::plane(10.0);
/// floor.generate_lightmap_uvs(4.0);
/// let model = ponko::render::BakeModel::new(&floor, Mat4::IDENTITY).with_albedo(Color::rgb(0.6, 0.6, 0.6));
/// assert!(model.lightmapped);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BakeModel {
    /// Corner positions and normals in the world, and lightmap
    /// coordinates (the mesh's [`uvs1`](MeshData::uvs1)).
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs1: Vec<Vec2>,
    indices: Vec<u32>,
    /// Its color (sRGB, as [`MaterialDesc::color`]): how much light it
    /// bounces. Textures aren't read: for a textured surface, set it to
    /// the texture's average color times the material's. Default white.
    pub albedo: Color,
    /// Light it gives off itself (as [`MaterialDesc::emissive`]), which
    /// lights what's around it in the bake. Default black.
    pub emissive: Color,
    /// Both faces bounce light (as [`MaterialDesc::double_sided`]); a ray
    /// that reaches the back of a one-sided surface has started inside
    /// something, and its texel is filled from its neighbours. Default off.
    pub double_sided: bool,
    /// Whether it gets a lightmap. Off: it only blocks and bounces light
    /// for the others (a model that moves later, or one lit by the probes).
    /// Default on.
    pub lightmapped: bool,
}

impl BakeModel {
    /// `mesh` placed by `transform` (a [`d3::Transform`](crate::d3::Transform)'s
    /// [`matrix`](crate::d3::Transform::matrix)). A lightmapped model's mesh
    /// needs lightmap coordinates: [`MeshData::uvs1`] from its file, or
    /// [`MeshData::generate_lightmap_uvs`]. The mesh the model is drawn
    /// with must have the same ones.
    #[must_use]
    pub fn new(mesh: &MeshData, transform: Mat4) -> Self {
        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        let normals = mesh.normals_or_computed();
        Self {
            positions: mesh.positions.iter().map(|&p| transform.transform_point3(p)).collect(),
            normals: normals
                .iter()
                .map(|&n| (normal_matrix * n).normalize_or_zero())
                .collect(),
            uvs1: mesh.uvs1.clone(),
            indices: mesh.indices.clone(),
            albedo: Color::WHITE,
            emissive: Color::rgb(0.0, 0.0, 0.0),
            double_sided: false,
            lightmapped: true,
        }
    }

    /// Takes the albedo, emissive color and sides from a material.
    #[must_use]
    pub fn with_material(mut self, material: &MaterialDesc) -> Self {
        self.albedo = material.color;
        self.emissive = material.emissive;
        self.double_sided = material.double_sided;
        self
    }

    with_fields! {
        /// Sets [`albedo`](Self::albedo).
        albedo: Color => with_albedo;
        /// Sets [`emissive`](Self::emissive).
        emissive: Color => with_emissive;
        /// Sets [`double_sided`](Self::double_sided).
        double_sided: bool => with_double_sided;
        /// Sets [`lightmapped`](Self::lightmapped).
        lightmapped: bool => with_lightmapped;
    }

    fn triangles(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        let n = self.positions.len();
        self.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|i| i as usize))
            .filter(move |t| t.iter().all(|&i| i < n))
    }

    fn area(&self) -> f32 {
        self.triangles()
            .map(|[a, b, c]| {
                let p = &self.positions;
                (p[b] - p[a]).cross(p[c] - p[a]).length() * 0.5
            })
            .sum()
    }

    fn uv_area(&self) -> f32 {
        self.triangles()
            .map(|[a, b, c]| {
                let u = &self.uvs1;
                ((u[b] - u[a]).perp_dot(u[c] - u[a]) * 0.5).abs()
            })
            .sum()
    }
}

/// Baked lighting's GPU layers (see `BakedLighting::gpu_layers`) and
/// their side, as the renderer takes them.
pub(crate) type BakedLayers = std::sync::Arc<(u32, Vec<Vec<u16>>)>;

/// What [`bake_lighting`] bakes: the models, and the light they're lit by.
///
/// ```
/// # use ponko::prelude::*;
/// let lighting = ponko::render::Lighting::default();
/// let sky = ponko::render::Sky::default();
/// let mut scene = ponko::render::LightBakeScene::new(&lighting, &sky);
/// let mut floor = ponko::render::MeshData::plane(4.0);
/// floor.generate_lightmap_uvs(2.0);
/// let index = scene.add(ponko::render::BakeModel::new(&floor, Mat4::IDENTITY));
/// assert_eq!(index, 0);
/// ```
#[derive(Debug, Clone)]
pub struct LightBakeScene {
    models: Vec<BakeModel>,
    lamps: Vec<Lamp>,
    sun: Option<(Vec3, Vec3)>,
    sky: SkyLight,
}

impl LightBakeScene {
    /// No models yet, lit by `lighting`'s sun and the sky's ambient light
    /// as the renderer draws it: the sky, ground (and equator) colors of
    /// `sky` times [`Lighting::ambient_intensity`], also with an atmosphere
    /// (whose own light the bake doesn't model).
    #[must_use]
    pub fn new(lighting: &Lighting, sky: &Sky) -> Self {
        let rgb = |c: Color| {
            let [r, g, b, _] = linear(c);
            Vec3::new(r, g, b)
        };
        let ambient = lighting.ambient_intensity.max(0.0);
        let towards = -lighting.sun_direction.normalize_or_zero();
        let sun_light = rgb(lighting.sun_color) * lighting.sun_intensity.max(0.0);
        Self {
            models: Vec::new(),
            lamps: Vec::new(),
            sun: (towards != Vec3::ZERO && sun_light != Vec3::ZERO).then_some((towards, sun_light)),
            sky: SkyLight {
                sky: rgb(sky.sky_color) * ambient,
                ground: rgb(sky.ground_color) * ambient,
                equator: sky.equator_color.map(|c| rgb(c) * ambient),
            },
        }
    }

    /// Adds a model; returns its index, which [`BakedLighting::lightmap`]
    /// takes.
    pub fn add(&mut self, model: BakeModel) -> usize {
        self.models.push(model);
        self.models.len() - 1
    }

    /// Adds a point light at `position`, as the renderer lights with it.
    pub fn add_point_light(&mut self, position: Vec3, light: &PointLight3D) {
        let [r, g, b, _] = linear(light.color);
        self.lamps.push(Lamp {
            position,
            radiance: Vec3::new(r, g, b) * light.intensity.max(0.0),
            range: light.range.max(0.0),
            cone: None,
        });
    }

    /// Adds a spot light at `position` shining along `forward` (its
    /// transform's local -Z), as the renderer lights with it.
    pub fn add_spot_light(&mut self, position: Vec3, forward: Vec3, light: &SpotLight3D) {
        let [r, g, b, _] = linear(light.color);
        let valid = 0.0 <= light.inner_angle
            && light.inner_angle <= light.outer_angle
            && light.outer_angle < std::f32::consts::FRAC_PI_2;
        self.lamps.push(Lamp {
            position,
            radiance: Vec3::new(r, g, b) * if valid { light.intensity.max(0.0) } else { 0.0 },
            range: light.range.max(0.0),
            cone: Some((
                forward.normalize_or(Vec3::NEG_Z),
                light.outer_angle.cos(),
                light.inner_angle.cos(),
            )),
        });
    }

    /// Adds every point and spot light in `world` (with a
    /// [`d3::Transform`](crate::d3::Transform)) where it is now.
    pub fn add_lights_from(&mut self, world: &mut ponko_ecs::World) {
        use crate::three::Transform3D;
        for (t, light) in world.query::<(&Transform3D, &PointLight3D)>().iter() {
            self.add_point_light(t.position, light);
        }
        for (t, light) in world.query::<(&Transform3D, &SpotLight3D)>().iter() {
            self.add_spot_light(t.position, t.rotation * Vec3::NEG_Z, light);
        }
    }

    /// The models added so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether no model was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

/// Where a model's lightmap is in [`BakedLighting`]: a rectangle of a page,
/// in texels from its top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightmapRect {
    /// Which page.
    pub page: u32,
    /// Its top left texel.
    pub x: u32,
    /// Its top left texel's row.
    pub y: u32,
    /// Its width and height in texels.
    pub width: u32,
    /// See `width`.
    pub height: u32,
}

/// Draws a [`Model`](crate::render::Model) with its lightmap from the
/// baked lighting set with
/// [`Graphics::set_baked_lighting`](crate::gfx::Graphics::set_baked_lighting):
/// the model's index in the [`LightBakeScene`] it was baked in (get it from
/// [`BakedLighting::lightmap`]). The model's mesh must have the lightmap
/// coordinates it was baked with. Without baked lighting set, or with an
/// index it doesn't have, the model is drawn as without the component.
///
/// ```
/// # use ponko::prelude::*;
/// let mut world = World::new();
/// world.spawn((ponko::d3::Transform::default(), ponko::render::Lightmapped(0)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, ponko_ecs::Component)]
#[repr(transparent)]
pub struct Lightmapped(pub u32);

/// How a bake went.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct BakeStats {
    /// Wall-clock time the bake took, in seconds.
    pub seconds: f64,
    /// Lightmap texels on surfaces (gathered), and the ones of those found
    /// inside other geometry and filled from their neighbours instead.
    pub texels: u64,
    /// See `texels`.
    pub inside_texels: u64,
    /// Probes in the volume.
    pub probes: u64,
    /// Rays traced, shadow rays included.
    pub rays: u64,
}

/// The probe volume: a grid of probes, each the light arriving from every
/// direction as an order-1 spherical harmonic of irradiance over pi.
#[derive(Debug, Clone, PartialEq)]
struct Probes {
    origin: Vec3,
    spacing: f32,
    counts: UVec3,
    /// Per probe: the constant term (rgb), then the x, y and z terms (rgb
    /// each): irradiance over pi toward normal n is c0 + cx n.x + cy n.y + cz n.z.
    coefficients: Vec<[f32; 12]>,
}

/// The result of [`bake_lighting`]: lightmap pages (half floats), where
/// each model's lightmap is, the probe volume. Draw with it through
/// [`Graphics::set_baked_lighting`](crate::gfx::Graphics::set_baked_lighting);
/// keep it in a file with [`to_bytes`](Self::to_bytes) and
/// [`from_bytes`](Self::from_bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct BakedLighting {
    page_size: u32,
    /// Per page: irradiance over pi (rgb, a: 1 where baked), and with
    /// `directional`, the light's mean direction (xyz, its length how much
    /// it comes from that way).
    light: Vec<Vec<[u16; 4]>>,
    direction: Vec<Vec<[u16; 4]>>,
    rects: Vec<Option<LightmapRect>>,
    probes: Option<Probes>,
    stats: BakeStats,
}

/// The most lightmap pages a bake makes.
pub const MAX_LIGHTMAP_PAGES: u32 = 16;
/// Texels at the start of the first layer the renderer reads before the
/// rectangle table (see [`BakedLighting::gpu_layers`]).
const HEADER_TEXELS: u32 = 8;
/// What the renderer finds in the first texel of a baked lighting array
/// (no texture array of images has a negative value).
const SIGNATURE: f32 = -1.0;
const MAGIC: &[u8; 8] = b"PONKOLIT";
const VERSION: u32 = 1;

impl BakedLighting {
    /// The pages' width and height in texels.
    #[must_use]
    pub fn page_size(&self) -> u32 {
        self.page_size
    }

    /// How many lightmap pages there are.
    #[must_use]
    pub fn pages(&self) -> u32 {
        self.light.len() as u32
    }

    /// Whether it has the light's direction per texel.
    #[must_use]
    pub fn directional(&self) -> bool {
        !self.direction.is_empty()
    }

    /// The component that draws model `index` of the bake (its index in
    /// the [`LightBakeScene`]) with its lightmap; `None` for a model that
    /// isn't lightmapped.
    #[must_use]
    pub fn lightmap(&self, index: usize) -> Option<Lightmapped> {
        self.rects.get(index)?.map(|_| Lightmapped(index as u32))
    }

    /// Where model `index`'s lightmap is.
    #[must_use]
    pub fn rect(&self, index: usize) -> Option<LightmapRect> {
        *self.rects.get(index)?
    }

    /// The baked light at texel (`x`, `y`) of `page`: irradiance over pi
    /// (what a white surface there shows), and whether the texel was baked
    /// (its own or filled from a neighbour).
    #[must_use]
    pub fn texel(&self, page: u32, x: u32, y: u32) -> Option<(Vec3, bool)> {
        let t = self.light.get(page as usize)?.get((y * self.page_size + x) as usize)?;
        let [r, g, b, a] = t.map(f16_to_f32);
        Some((Vec3::new(r, g, b), a > 0.5))
    }

    /// The light the probe volume gives a surface at `position` facing
    /// `normal`: irradiance over pi, or `None` outside the volume (or
    /// without one).
    #[must_use]
    pub fn probe_light(&self, position: Vec3, normal: Vec3) -> Option<Vec3> {
        let p = self.probes.as_ref()?;
        let local = (position - p.origin) / p.spacing;
        let last = (p.counts - 1).as_vec3();
        if local.min_element() < 0.0 || (local - last).max_element() > 0.0 {
            return None;
        }
        let base = local.floor().min(last - 1.0).max(Vec3::ZERO);
        let t = local - base;
        let mut sum = Vec3::ZERO;
        for corner in 0..8u32 {
            let o = UVec3::new(corner & 1, (corner >> 1) & 1, corner >> 2);
            let weight = Vec3::select(o.cmpeq(UVec3::ONE), t, Vec3::ONE - t);
            let cell = base.as_uvec3() + o;
            let c = &p.coefficients[(cell.x + p.counts.x * (cell.y + p.counts.y * cell.z)) as usize];
            sum += probe_irradiance(c, normal) * (weight.x * weight.y * weight.z);
        }
        Some(sum)
    }

    /// How the bake went.
    #[must_use]
    pub fn stats(&self) -> BakeStats {
        self.stats
    }

    /// The GPU memory it takes when set with
    /// [`Graphics::set_baked_lighting`](crate::gfx::Graphics::set_baked_lighting),
    /// in bytes: a header layer, then each page's light and directions,
    /// 8 bytes a texel.
    #[must_use]
    pub fn gpu_bytes(&self) -> u64 {
        let layers = 1 + self.light.len() + self.direction.len();
        u64::from(self.page_size) * u64::from(self.page_size) * 8 * layers as u64
    }

    /// The texture array the renderer reads (RGBA half floats, each layer
    /// `page_size` square): layer 0 holds a header (texel 0: the signature
    /// -1, the pages, 1 when directional; texels 1 and 2: the probe grid's
    /// origin in two parts and its spacing; texel 3: its counts and the row
    /// its probes start at), from texel 8 two texels per model (x, y,
    /// width, height; page), then three texels per probe. Then the pages'
    /// light, then their directions.
    pub(crate) fn gpu_layers(&self) -> Result<Vec<Vec<u16>>, Error> {
        let side = self.page_size;
        let area = (side * side) as usize;
        let mut header = vec![[0u16; 4]; area];
        let set = |header: &mut Vec<[u16; 4]>, at: usize, v: [f32; 4]| header[at] = v.map(f32_to_f16);
        set(
            &mut header,
            0,
            [
                SIGNATURE,
                self.pages() as f32,
                f32::from(u8::from(self.directional())),
                1.0,
            ],
        );
        for (i, rect) in self.rects.iter().enumerate() {
            let at = HEADER_TEXELS as usize + 2 * i;
            if at + 1 >= area {
                return Err(Error::new(
                    ErrorKind::Limit,
                    format!(
                        "baked lighting: {} models don't fit in a {side} page's table; bake with a larger page_size",
                        self.rects.len()
                    ),
                ));
            }
            if let Some(r) = rect {
                set(
                    &mut header,
                    at,
                    [r.x as f32, r.y as f32, r.width as f32, r.height as f32],
                );
                set(&mut header, at + 1, [r.page as f32, 0.0, 0.0, 0.0]);
            }
        }
        if let Some(p) = &self.probes {
            let row = (HEADER_TEXELS as usize + 2 * self.rects.len()).div_ceil(side as usize);
            let count = p.coefficients.len();
            if row * side as usize + 3 * count > area {
                return Err(Error::new(
                    ErrorKind::Limit,
                    format!(
                        "baked lighting: {count} probes don't fit in a {side} page; bake with a larger probe_spacing or page_size"
                    ),
                ));
            }
            let hi = p.origin.to_array().map(|v| f16_to_f32(f32_to_f16(v)));
            set(&mut header, 1, [hi[0], hi[1], hi[2], p.spacing]);
            set(
                &mut header,
                2,
                [p.origin.x - hi[0], p.origin.y - hi[1], p.origin.z - hi[2], 0.0],
            );
            set(
                &mut header,
                3,
                [p.counts.x as f32, p.counts.y as f32, p.counts.z as f32, row as f32],
            );
            for (i, c) in p.coefficients.iter().enumerate() {
                let at = row * side as usize + 3 * i;
                set(&mut header, at, [c[0], c[1], c[2], c[3]]);
                set(&mut header, at + 1, [c[4], c[5], c[6], c[7]]);
                set(&mut header, at + 2, [c[8], c[9], c[10], c[11]]);
            }
        }
        let flat = |layer: &[[u16; 4]]| layer.iter().flatten().copied().collect::<Vec<u16>>();
        let mut layers = vec![flat(&header)];
        layers.extend(self.light.iter().map(|l| flat(l)));
        layers.extend(self.direction.iter().map(|l| flat(l)));
        Ok(layers)
    }

    /// The bake as bytes, to keep in a file and load with
    /// [`from_bytes`](Self::from_bytes) (or
    /// [`Graphics::load_baked_lighting`](crate::gfx::Graphics::load_baked_lighting)).
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + self.gpu_bytes() as usize);
        let u = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
        let f = |out: &mut Vec<u8>, v: f32| out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(MAGIC);
        u(&mut out, VERSION);
        u(&mut out, self.page_size);
        u(&mut out, self.pages());
        u(&mut out, u32::from(self.directional()));
        u(&mut out, self.rects.len() as u32);
        for rect in &self.rects {
            match rect {
                Some(r) => {
                    for v in [1, r.page, r.x, r.y, r.width, r.height] {
                        u(&mut out, v);
                    }
                }
                None => u(&mut out, 0),
            }
        }
        match &self.probes {
            Some(p) => {
                u(&mut out, 1);
                for v in [p.origin.x, p.origin.y, p.origin.z, p.spacing] {
                    f(&mut out, v);
                }
                for v in p.counts.to_array() {
                    u(&mut out, v);
                }
                for c in &p.coefficients {
                    for &v in c {
                        f(&mut out, v);
                    }
                }
            }
            None => u(&mut out, 0),
        }
        for page in self.light.iter().chain(&self.direction) {
            for t in page {
                for v in t {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        out
    }

    /// Reads what [`to_bytes`](Self::to_bytes) wrote.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Decode`] for bytes that aren't baked lighting (or are
    /// cut short, or of another version).
    ///
    /// ```
    /// let err = ponko::render::BakedLighting::from_bytes(b"not a bake").unwrap_err();
    /// assert_eq!(err.kind(), ponko::prelude::ErrorKind::Decode);
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bad = |why: &str| Error::new(ErrorKind::Decode, format!("baked lighting: {why}"));
        let mut r = Reader { bytes, at: 0 };
        if r.take(8)? != MAGIC {
            return Err(bad("not a baked lighting file (made by BakedLighting::to_bytes)"));
        }
        if r.u32()? != VERSION {
            return Err(bad("made by another version of Ponko; bake it again"));
        }
        let page_size = r.u32()?;
        let pages = r.u32()?;
        let directional = r.u32()? != 0;
        let count = r.u32()?;
        if !(1..=4096).contains(&page_size) || pages > MAX_LIGHTMAP_PAGES || count > 1 << 24 {
            return Err(bad("its sizes are out of range"));
        }
        let mut rects = Vec::with_capacity(count.min(1 << 16) as usize);
        for _ in 0..count {
            rects.push(if r.u32()? != 0 {
                Some(LightmapRect {
                    page: r.u32()?,
                    x: r.u32()?,
                    y: r.u32()?,
                    width: r.u32()?,
                    height: r.u32()?,
                })
            } else {
                None
            });
        }
        let probes = if r.u32()? != 0 {
            let origin = Vec3::new(r.f32()?, r.f32()?, r.f32()?);
            let spacing = r.f32()?;
            let counts = UVec3::new(r.u32()?, r.u32()?, r.u32()?);
            let n = u64::from(counts.x) * u64::from(counts.y) * u64::from(counts.z);
            if n > 1 << 22 || counts.min_element() < 2 || !(spacing.is_finite() && spacing > 0.0) {
                return Err(bad("its probe grid is out of range"));
            }
            let mut coefficients = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let mut c = [0.0; 12];
                for v in &mut c {
                    *v = r.f32()?;
                }
                coefficients.push(c);
            }
            Some(Probes {
                origin,
                spacing,
                counts,
                coefficients,
            })
        } else {
            None
        };
        let area = (page_size * page_size) as usize;
        let mut read_pages = |n: u32| -> Result<Vec<Vec<[u16; 4]>>, Error> {
            (0..n)
                .map(|_| {
                    let b = r.take(area * 8)?;
                    Ok(b.as_chunks::<8>()
                        .0
                        .iter()
                        .map(|t| std::array::from_fn(|k| u16::from_le_bytes([t[2 * k], t[2 * k + 1]])))
                        .collect())
                })
                .collect()
        };
        let light = read_pages(pages)?;
        let direction = if directional { read_pages(pages)? } else { Vec::new() };
        if rects
            .iter()
            .flatten()
            .any(|r| r.page >= pages || r.x + r.width > page_size || r.y + r.height > page_size)
        {
            return Err(bad("a lightmap is outside its page"));
        }
        Ok(Self {
            page_size,
            light,
            direction,
            rects,
            probes,
            stats: BakeStats::default(),
        })
    }
}

/// Reads [`BakedLighting::to_bytes`]'s little-endian values in order.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let s = self
            .bytes
            .get(self.at..self.at.saturating_add(n))
            .ok_or_else(|| Error::new(ErrorKind::Decode, "baked lighting: the data ends too soon"))?;
        self.at += n;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }
}

impl crate::gfx::Graphics {
    /// Draws with `baked` lighting from now on (`None`: without any).
    /// Models with a [`Lightmapped`] component take their indirect light
    /// from their lightmap; every other model inside the probe volume takes
    /// it from the probes (where real-time global illumination doesn't
    /// already light it). Each model's material must leave its third
    /// custom texture array empty: that slot (`custom_array2`) carries the
    /// baked lighting, so a material that uses it (terrain) isn't lit by it.
    /// Setting new baked lighting replaces the old.
    ///
    /// GPU memory: [`BakedLighting::gpu_bytes`]. Works on every GPU tier,
    /// WebGL2 too. Headless, nothing is drawn.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let scene = ponko::render::LightBakeScene::new(&ctx.scene3d.lighting, &ctx.scene3d.sky);
    ///     let settings = ponko::render::LightBakeSettings::default().with_page_size(64);
    ///     let baked = ponko::render::bake_lighting(&scene, &settings).unwrap();
    ///     ctx.gfx.set_baked_lighting(Some(&baked)).unwrap();
    ///     ctx.gfx.set_baked_lighting(None).unwrap();
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Limit`] when its pages are larger than this GPU's
    /// textures or its tables don't fit its first layer (the message says
    /// which setting to change).
    pub fn set_baked_lighting(&mut self, baked: Option<&BakedLighting>) -> Result<(), Error> {
        let layers = match baked {
            Some(b) => Some(std::sync::Arc::new((b.page_size(), b.gpu_layers()?))),
            None => None,
        };
        if let (Some(layers), Some(renderer)) = (&layers, self.renderer.as_ref())
            && !self.headless
        {
            let (max, max_layers) = renderer.max_texture_array_size();
            if layers.0 > max || layers.1.len() as u32 > max_layers {
                return Err(Error::new(
                    ErrorKind::Limit,
                    format!(
                        "set_baked_lighting: {} layers of {} x {} are more than this GPU takes ({max} x {max}, {max_layers} layers); \
                         bake with a smaller page_size",
                        layers.1.len(),
                        layers.0,
                        layers.0
                    ),
                ));
            }
        }
        self.assets3d.baked_upload = Some(layers);
        self.upload_baked_lighting();
        Ok(())
    }

    /// Hands baked lighting set with [`set_baked_lighting`](Self::set_baked_lighting)
    /// to the renderer, once there is one.
    pub(crate) fn upload_baked_lighting(&mut self) {
        if self.headless || self.renderer.is_none() {
            return;
        }
        if let (Some(upload), Some(renderer)) = (self.assets3d.baked_upload.take(), self.renderer.as_mut()) {
            renderer.set_baked_lighting(upload.as_deref().map(|(side, layers)| (*side, layers.as_slice())));
        }
    }

    /// Reads baked lighting saved with [`BakedLighting::to_bytes`] from the
    /// [asset sources](crate::assets::Assets::add_source) or `assets/`, for
    /// [`set_baked_lighting`](Self::set_baked_lighting).
    ///
    /// # Panics
    ///
    /// If the file is missing, can't be read or isn't baked lighting (the
    /// message names it); [`try_load_baked_lighting`](Self::try_load_baked_lighting)
    /// returns the error instead.
    #[must_use]
    pub fn load_baked_lighting(&self, path: &str) -> BakedLighting {
        self.try_load_baked_lighting(path).unwrap_or_else(|e| panic!("{e}"))
    }

    /// [`load_baked_lighting`](Self::load_baked_lighting) with errors as
    /// values.
    ///
    /// ```
    /// # use ponko::prelude::*;
    /// Game::new("t", 1, 1).run_headless(0, |ctx| {
    ///     let err = ctx.gfx.try_load_baked_lighting("no/such.lighting").unwrap_err();
    ///     assert_eq!(err.kind(), ErrorKind::NotFound);
    /// });
    /// ```
    ///
    /// # Errors
    ///
    /// [`ErrorKind::NotFound`] or [`Io`](ErrorKind::Io) when the file can't
    /// be read, [`Decode`](ErrorKind::Decode) when it isn't baked lighting,
    /// each with the path.
    pub fn try_load_baked_lighting(&self, path: &str) -> Result<BakedLighting, Error> {
        let bytes = self
            .asset_sources
            .find(path)?
            .into_bytes()
            .map_err(|e| e.with_path(path))?;
        BakedLighting::from_bytes(&bytes).map_err(|e| e.with_path(path))
    }
}

/// Irradiance over pi toward `n` from a probe's coefficients.
fn probe_irradiance(c: &[f32; 12], n: Vec3) -> Vec3 {
    let v = |i: usize| Vec3::new(c[i], c[i + 1], c[i + 2]);
    (v(0) + v(3) * n.x + v(6) * n.y + v(9) * n.z).max(Vec3::ZERO)
}

/// One lightmap texel on a surface: where it is, which way the surface
/// faces there (its face, and its smoothed normal), whose it is.
#[derive(Clone, Copy)]
struct Texel {
    position: Vec3,
    face: Vec3,
    normal: Vec3,
    model: u32,
    /// Whether the texel's center is on the triangle (not just near it).
    exact: bool,
}

/// What gathering found at a texel: irradiance over pi, the mean
/// direction the light came from (weighted by brightness), whether it's
/// inside geometry.
#[derive(Clone, Copy, Default)]
struct Gathered {
    light: Vec3,
    direction: Vec3,
    inside: bool,
    rays: u64,
}

/// Bakes the indirect light of `scene` into lightmaps and a probe volume,
/// on all cores. A scene of a few hundred square units at the default
/// settings bakes in seconds; the time grows with the texels (surface area
/// times `texels_per_unit` squared) times the samples times the bounces.
///
/// ```
/// # use ponko::prelude::*;
/// let mut lighting = ponko::render::Lighting::default();
/// lighting.sun_intensity = 0.0;
/// let mut sky = ponko::render::Sky::default();
/// sky.sky_color = Color::rgb(0.5, 0.5, 0.5);
/// sky.ground_color = Color::rgb(0.5, 0.5, 0.5);
/// let mut scene = ponko::render::LightBakeScene::new(&lighting, &sky);
/// let mut floor = ponko::render::MeshData::plane(2.0);
/// floor.generate_lightmap_uvs(2.0);
/// let i = scene.add(ponko::render::BakeModel::new(&floor, Mat4::IDENTITY));
/// let settings = ponko::render::LightBakeSettings::default().with_samples(16).with_page_size(64);
/// let baked = ponko::render::bake_lighting(&scene, &settings).unwrap();
/// // Nothing above an open floor: it takes the sky's light, as without a bake.
/// let rect = baked.rect(i).unwrap();
/// let (light, baked_here) = baked.texel(rect.page, rect.x + rect.width / 2, rect.y + rect.height / 2).unwrap();
/// assert!(baked_here);
/// assert!((light.x - 0.214).abs() < 0.01, "{light}"); // 0.5 sRGB is 0.214 linear
/// ```
///
/// # Errors
///
/// [`ErrorKind::Invalid`] for settings out of range or a lightmapped model
/// without lightmap coordinates (the message names it and the fix),
/// [`ErrorKind::Limit`] when the lightmaps need more than
/// [`MAX_LIGHTMAP_PAGES`] pages.
pub fn bake_lighting(scene: &LightBakeScene, settings: &LightBakeSettings) -> Result<BakedLighting, Error> {
    // On the frame pool (on the web, its workers, not the page's thread),
    // as parallel queries run: the bake's parallel loops start inside.
    crate::frame_pool::run(|| {
        crate::profile_scope!("three/shared_bake");
        bake(scene, settings)
    })
}

fn bake(scene: &LightBakeScene, settings: &LightBakeSettings) -> Result<BakedLighting, Error> {
    let start = std::time::Instant::now();
    let invalid = |m: String| Error::new(ErrorKind::Invalid, format!("bake_lighting: {m}"));
    let tpu = settings.texels_per_unit;
    if !(tpu.is_finite() && tpu > 0.0) {
        return Err(invalid(format!("texels_per_unit must be above 0 (got {tpu})")));
    }
    if settings.samples == 0 {
        return Err(invalid("samples must be at least 1".into()));
    }
    let side = settings.page_size;
    if !(64..=2048).contains(&side) {
        return Err(invalid(format!("page_size must be 64 to 2048 (got {side})")));
    }
    if !(settings.probe_spacing.is_finite() && settings.probe_spacing >= 0.0) {
        return Err(invalid(format!(
            "probe_spacing must be 0 or more (got {})",
            settings.probe_spacing
        )));
    }
    for (m, model) in scene.models.iter().enumerate() {
        if model.lightmapped && model.uvs1.len() != model.positions.len() {
            return Err(invalid(format!(
                "model {m} is lightmapped but its mesh has no lightmap coordinates (uvs1); \
                 call MeshData::generate_lightmap_uvs before making the mesh and the BakeModel, \
                 or give the glTF file a second UV set"
            )));
        }
    }
    let (trace, tris, first_tri) = trace_scene(scene, settings.bounces);

    // Each lightmapped model's rectangle: as many texels as its area needs.
    let mut sized = Vec::new();
    for (m, model) in scene.models.iter().enumerate() {
        if !model.lightmapped {
            continue;
        }
        let uv_area = model.uv_area();
        let area = model.area();
        if uv_area <= 1e-9 || area <= 0.0 {
            continue;
        }
        let s = ((area * tpu * tpu / uv_area).sqrt().ceil() as u32).max(4);
        sized.push((m, s));
    }
    // The smallest page that holds everything (and the table of the
    // models and probes), or as many of the largest as it takes.
    let grid = (settings.probe_spacing > 0.0 && !tris.is_empty()).then(|| probe_grid(&tris, settings.probe_spacing));
    let probe_count = grid.map_or(0, |(_, _, c)| (c.x * c.y * c.z) as usize);
    let table = HEADER_TEXELS as usize + 2 * scene.models.len();
    let fits = |side: u32| {
        let row = table.div_ceil(side as usize) * side as usize;
        sized.iter().all(|&(_, s)| s + 2 <= side) && row + 3 * probe_count <= (side * side) as usize
    };
    let side = std::iter::successors(Some(64u32), |s| Some(s * 2))
        .take_while(|&s| s < settings.page_size)
        .find(|&s| fits(s) && pack::pack_pages(&sizes_for(&sized, s), s).1 <= 1)
        .unwrap_or(settings.page_size);
    let sized: Vec<(usize, u32)> = sized.into_iter().map(|(m, s)| (m, s.min(side - 2))).collect();
    let (places, pages) = pack::pack_pages(&sizes_for(&sized, side), side);
    if pages > MAX_LIGHTMAP_PAGES {
        return Err(Error::new(
            ErrorKind::Limit,
            format!(
                "bake_lighting: the lightmaps need {pages} pages of {side} x {side}, more than {MAX_LIGHTMAP_PAGES}; \
                 bake with a larger page_size or fewer texels_per_unit"
            ),
        ));
    }
    let mut rects = vec![None; scene.models.len()];
    for (k, &(m, s)) in sized.iter().enumerate() {
        let (x, y, page) = places[k];
        rects[m] = Some(LightmapRect {
            page,
            x: x + 1,
            y: y + 1,
            width: s,
            height: s,
        });
    }

    // The texels on each model's surface.
    let area = (side * side) as usize;
    let mut texels: Vec<Vec<Option<Texel>>> = (0..pages).map(|_| vec![None; area]).collect();
    for (m, model) in scene.models.iter().enumerate() {
        let Some(rect) = rects[m] else { continue };
        let page = &mut texels[rect.page as usize];
        for (k, [a, b, c]) in model.triangles().enumerate() {
            let ng = trace.tri_normal[first_tri[m] + k];
            if ng == Vec3::ZERO {
                continue;
            }
            rasterize(model, [a, b, c], ng, m as u32, rect, side, page);
        }
    }

    // Gathering, texel by texel on all cores (each texel's random numbers
    // are its own, so the result doesn't depend on the threads).
    let jobs: Vec<(u32, u32, Texel)> = texels
        .iter()
        .enumerate()
        .flat_map(|(p, page)| {
            page.iter()
                .enumerate()
                .filter_map(move |(i, t)| t.map(|t| (p as u32, i as u32, t)))
        })
        .collect();
    let samples = settings.samples;
    let gathered: Vec<Gathered> = jobs
        .par_iter()
        .map(|&(p, i, t)| gather(&trace, &t, samples, settings.seed, u64::from(p) << 32 | u64::from(i)))
        .collect();
    let mut stats = BakeStats {
        texels: jobs.len() as u64,
        ..Default::default()
    };
    let mut light: Vec<Vec<Option<(Vec3, Vec3)>>> = (0..pages).map(|_| vec![None; area]).collect();
    for (&(p, i, _), g) in jobs.iter().zip(&gathered) {
        stats.rays += g.rays;
        if g.inside {
            stats.inside_texels += 1;
        } else {
            light[p as usize][i as usize] = Some((g.light, g.direction));
        }
    }
    // Smoothing, then filling the texels inside geometry and around each
    // chart from their neighbours.
    for (p, page) in light.iter_mut().enumerate() {
        if settings.denoise {
            denoise(&trace, page, &texels[p], side, tpu);
        }
        dilate(page, &texels[p], side);
    }
    let to_half = |v: Vec3, a: f32| [v.x, v.y, v.z, a].map(f32_to_f16);
    let light_pages: Vec<Vec<[u16; 4]>> = light
        .iter()
        .map(|page| {
            page.iter()
                .map(|t| t.map_or([0; 4], |(l, _)| to_half(l.max(Vec3::ZERO), 1.0)))
                .collect()
        })
        .collect();
    let direction_pages = if settings.directional {
        light
            .iter()
            .map(|page| {
                page.iter()
                    .map(|t| t.map_or([0; 4], |(_, d)| to_half(d, 1.0)))
                    .collect()
            })
            .collect()
    } else {
        Vec::new()
    };
    let probes = grid.map(|grid| bake_probes(&trace, grid, settings, &mut stats));
    stats.seconds = start.elapsed().as_secs_f64();
    Ok(BakedLighting {
        page_size: side,
        light: light_pages,
        direction: direction_pages,
        rects,
        probes,
        stats,
    })
}

/// Every model's triangles in one tree, with what they are made of, and
/// the lights: what rays are traced against. Also the triangles, and each
/// model's first triangle among them.
fn trace_scene(scene: &LightBakeScene, bounces: u32) -> (TraceScene, Vec<Tri>, Vec<usize>) {
    let rgb = |c: Color| {
        let [r, g, b, _] = linear(c);
        Vec3::new(r, g, b)
    };
    let mut tris = Vec::new();
    let mut tri_surface = Vec::new();
    let mut corner_normals = Vec::new();
    let mut surfaces = Vec::with_capacity(scene.models.len());
    let mut first_tri = Vec::with_capacity(scene.models.len());
    for (m, model) in scene.models.iter().enumerate() {
        surfaces.push(Surface {
            albedo: rgb(model.albedo).clamp(Vec3::ZERO, Vec3::ONE),
            emissive: rgb(model.emissive).max(Vec3::ZERO),
            double_sided: model.double_sided,
        });
        first_tri.push(tris.len());
        for [a, b, c] in model.triangles() {
            let p = &model.positions;
            let tri = Tri::new(p[a], p[b], p[c]);
            // Degenerate triangles stay, so indices line up with the
            // model's triangles; nothing hits them.
            let n = &model.normals;
            corner_normals.push(if tri.e1.cross(tri.e2).length_squared() <= 0.0 {
                [Vec3::ZERO; 3]
            } else {
                [n[a], n[b], n[c]]
            });
            tris.push(tri);
            tri_surface.push(m as u32);
        }
    }
    let trace = TraceScene::new(
        &tris,
        tri_surface,
        corner_normals,
        surfaces,
        scene.sun,
        scene.lamps.clone(),
        scene.sky,
        bounces,
    );
    (trace, tris, first_tri)
}

/// The scene as a camera at `eye` sees it through `view_proj`, path traced
/// with `samples` rays a pixel: linear color, row by row from the top.
/// What baked lighting and realtime global illumination are measured
/// against.
#[cfg(test)]
pub(crate) fn reference_image(
    scene: &LightBakeScene,
    eye: Vec3,
    view_proj: Mat4,
    size: (u32, u32),
    samples: u32,
    bounces: u32,
) -> Vec<Vec3> {
    let (trace, _, _) = trace_scene(scene, bounces);
    let inverse = view_proj.inverse();
    (0..size.0 * size.1)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % size.0, i / size.0);
            let mut rng = Rng::new(0x5EED, u64::from(i), 7);
            let mut sum = Vec3::ZERO;
            for _ in 0..samples {
                let ndc = Vec2::new(
                    (x as f32 + rng.next()) / size.0 as f32 * 2.0 - 1.0,
                    1.0 - (y as f32 + rng.next()) / size.1 as f32 * 2.0,
                );
                let far = inverse * ndc.extend(0.5).extend(1.0);
                let dir = (far.truncate() / far.w - eye).normalize();
                sum += trace.incoming(eye, dir, 0, &mut rng).light;
            }
            sum / samples as f32
        })
        .collect()
}

/// Marks the texels of triangle `[a, b, c]` of `model` in its `rect`: the
/// ones whose center is on it, and (where no triangle has one) the ones it
/// touches, at its point nearest their center.
fn rasterize(
    model: &BakeModel,
    [a, b, c]: [usize; 3],
    face: Vec3,
    m: u32,
    rect: LightmapRect,
    side: u32,
    page: &mut [Option<Texel>],
) {
    let origin = Vec2::new(rect.x as f32, rect.y as f32);
    let size = Vec2::new(rect.width as f32, rect.height as f32);
    let q = [a, b, c].map(|i| origin + model.uvs1[i] * size);
    let twice = (q[1] - q[0]).perp_dot(q[2] - q[0]);
    if twice.abs() < 1e-12 {
        return;
    }
    let lo = q[0].min(q[1]).min(q[2]).floor() - Vec2::ONE;
    let hi = q[0].max(q[1]).max(q[2]).ceil() + Vec2::ONE;
    let x0 = (lo.x.max(rect.x as f32)) as u32;
    let y0 = (lo.y.max(rect.y as f32)) as u32;
    let x1 = (hi.x as u32).min(rect.x + rect.width);
    let y1 = (hi.y as u32).min(rect.y + rect.height);
    let bary = |p: Vec2| -> Vec3 {
        let w1 = (p - q[0]).perp_dot(q[2] - q[0]) / twice;
        let w2 = (q[1] - q[0]).perp_dot(p - q[0]) / twice;
        Vec3::new(1.0 - w1 - w2, w1, w2)
    };
    for y in y0..y1 {
        for x in x0..x1 {
            let center = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let mut w = bary(center);
            let exact = w.min_element() >= -1e-4;
            let slot = &mut page[(y * side + x) as usize];
            if !exact {
                if slot.is_some() {
                    continue;
                }
                // Near enough to touch the texel: half its diagonal.
                let near = closest_on_triangle(center, q);
                if near.distance(center) > std::f32::consts::FRAC_1_SQRT_2 {
                    continue;
                }
                w = bary(near).max(Vec3::ZERO);
                w /= w.x + w.y + w.z;
            } else if slot.is_some_and(|t| t.exact) {
                continue;
            }
            let p = &model.positions;
            let n = &model.normals;
            let normal = (n[a] * w.x + n[b] * w.y + n[c] * w.z).normalize_or(face);
            *slot = Some(Texel {
                position: p[a] * w.x + p[b] * w.y + p[c] * w.z,
                face,
                normal: if normal.dot(face) > 0.0 { normal } else { face },
                model: m,
                exact,
            });
        }
    }
}

/// The point of triangle `q` nearest `p` (in 2D).
fn closest_on_triangle(p: Vec2, q: [Vec2; 3]) -> Vec2 {
    let inside = {
        let s = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
        let (d0, d1, d2) = (s(q[0], q[1]), s(q[1], q[2]), s(q[2], q[0]));
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    };
    if inside {
        return p;
    }
    let on_edge = |a: Vec2, b: Vec2| {
        let d = b - a;
        let t = ((p - a).dot(d) / d.length_squared().max(1e-12)).clamp(0.0, 1.0);
        a + d * t
    };
    [on_edge(q[0], q[1]), on_edge(q[1], q[2]), on_edge(q[2], q[0])]
        .into_iter()
        .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
        .unwrap_or(p)
}

/// Irradiance over pi at a texel: the mean light of `samples` rays over the
/// hemisphere, more toward the normal (by the cosine).
fn gather(trace: &TraceScene, t: &Texel, samples: u32, seed: u64, key: u64) -> Gathered {
    let mut rng = Rng::new(seed, key, 1);
    let offset = (rng.next(), rng.next());
    let origin = t.position + t.face * trace.bias;
    let (mut sum, mut toward, mut weight, mut back) = (Vec3::ZERO, Vec3::ZERO, 0.0f32, 0u32);
    for i in 0..samples {
        let (u1, u2) = trace::r2(i, offset);
        let mut dir = trace::cosine_sample(t.normal, u1, u2);
        if dir.dot(t.face) <= 0.0 {
            // Below the face (a smoothed normal leaning over): mirror it up.
            dir = (dir - t.face * (2.0 * dir.dot(t.face))).normalize_or(t.face);
        }
        let got = trace.incoming(origin, dir, 0, &mut rng);
        back += u32::from(got.back);
        sum += got.light;
        let lum = got.light.dot(Vec3::new(0.2126, 0.7152, 0.0722));
        toward += dir * lum;
        weight += lum;
    }
    Gathered {
        light: sum / samples as f32,
        direction: if weight > 0.0 { toward / weight } else { t.normal },
        // A quarter of the rays reaching the back of something: the texel
        // is inside it (under a wall standing on the floor).
        inside: back * 4 > samples,
        rays: rng.rays,
    }
}

/// Smooths baked texels with their neighbours on the same model that face
/// about the same way, are nearby in the world and in sight of each other
/// (5x5, Gaussian).
fn denoise(trace: &TraceScene, page: &mut [Option<(Vec3, Vec3)>], texels: &[Option<Texel>], side: u32, tpu: f32) {
    let src: Vec<Option<(Vec3, Vec3)>> = page.to_vec();
    let reach = 3.0 / tpu;
    let s = side as i32;
    page.par_chunks_mut(side as usize).enumerate().for_each(|(y, row)| {
        let y = y as i32;
        for x in 0..s {
            let i = (y * s + x) as usize;
            let (Some(_), Some(t)) = (src[i], texels[i]) else {
                continue;
            };
            let from = t.position + t.face * trace.bias;
            let (mut l, mut d, mut w_sum) = (Vec3::ZERO, Vec3::ZERO, 0.0);
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= s || ny >= s {
                        continue;
                    }
                    let j = (ny * s + nx) as usize;
                    let (Some((lj, dj)), Some(tj)) = (src[j], texels[j]) else {
                        continue;
                    };
                    if tj.model != t.model || tj.position.distance(t.position) > reach {
                        continue;
                    }
                    let facing = tj.normal.dot(t.normal);
                    if facing < 0.8 {
                        continue;
                    }
                    // Not across something standing between them (a thin
                    // wall on a floor).
                    if j != i {
                        let to = tj.position + tj.face * trace.bias - from;
                        let distance = to.length();
                        if distance > 0.0 && trace.occluded(from, to / distance, distance) {
                            continue;
                        }
                    }
                    let w = (-((dx * dx + dy * dy) as f32) / 4.5).exp() * facing.powi(8);
                    l += lj * w;
                    d += dj * w;
                    w_sum += w;
                }
            }
            row[x as usize] = Some((l / w_sum, d / w_sum));
        }
    });
}

/// Fills the texels that have no light yet (inside geometry, or the
/// gutter around each chart) from their neighbours, round by round, until
/// every texel on a surface and the two around it have some: whatever a
/// filtered lookup on a surface can read.
fn dilate(page: &mut [Option<(Vec3, Vec3)>], texels: &[Option<Texel>], side: u32) {
    let s = side as i32;
    let around = |x: i32, y: i32| {
        [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)]
            .into_iter()
            .map(move |(dx, dy)| (x + dx, y + dy))
            .filter(move |&(nx, ny)| nx >= 0 && ny >= 0 && nx < s && ny < s)
            .map(move |(nx, ny)| (ny * s + nx) as usize)
    };
    // The texels to fill: on a surface, and two rings around those.
    let mut needed: Vec<bool> = texels.iter().map(Option::is_some).collect();
    for _ in 0..2 {
        let grown = needed.clone();
        for y in 0..s {
            for x in 0..s {
                let i = (y * s + x) as usize;
                if !grown[i] && around(x, y).any(|j| grown[j]) {
                    needed[i] = true;
                }
            }
        }
    }
    let mut missing: Vec<usize> = (0..page.len()).filter(|&i| needed[i] && page[i].is_none()).collect();
    while !missing.is_empty() {
        let src = page.to_vec();
        let before = missing.len();
        // Every empty texel next to a full one takes their mean, even
        // outside `needed`: a few more texels of gutter cost nothing.
        for y in 0..s {
            for x in 0..s {
                let i = (y * s + x) as usize;
                if src[i].is_some() {
                    continue;
                }
                let (mut l, mut d, mut n) = (Vec3::ZERO, Vec3::ZERO, 0);
                for j in around(x, y) {
                    if let Some((lj, dj)) = src[j] {
                        l += lj;
                        d += dj;
                        n += 1;
                    }
                }
                if n > 0 {
                    page[i] = Some((l / n as f32, d / n as f32));
                }
            }
        }
        missing.retain(|&i| page[i].is_none());
        if missing.len() == before {
            // Nothing on the page has light (every texel is inside
            // something): no light, then.
            for i in missing.drain(..) {
                page[i] = Some((Vec3::ZERO, Vec3::Y));
            }
        }
    }
}

/// Where the probes go: the lowest one, their spacing (`spacing`, or
/// wider if more than 32768 would be needed) and how many along x, y, z.
fn probe_grid(tris: &[Tri], spacing: f32) -> (Vec3, f32, UVec3) {
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for t in tris {
        for p in [t.v0, t.v0 + t.e1, t.v0 + t.e2] {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let mut spacing = spacing;
    let counts = loop {
        // Past the models by about a spacing on every side.
        let c = ((hi - lo) / spacing).ceil().as_uvec3() + 3;
        if u64::from(c.x) * u64::from(c.y) * u64::from(c.z) <= 32768 {
            break c;
        }
        spacing *= 1.25;
    };
    // From three quarters of a spacing below the models' box: models
    // often stand on whole coordinates, and a probe on a surface would see
    // half of it from behind.
    (lo - spacing * 0.75, spacing, counts)
}

/// The rectangles to pack for lightmaps `sized` texels square: a texel of
/// gutter on every side.
fn sizes_for(sized: &[(usize, u32)], side: u32) -> Vec<(u32, u32)> {
    sized
        .iter()
        .map(|&(_, s)| (s.min(side - 2) + 2, s.min(side - 2) + 2))
        .collect()
}

/// The probe volume over every model: probes `probe_spacing` apart (wider
/// if more than 32768 would be needed), each gathering light from the whole
/// sphere into its coefficients. Probes inside geometry take their
/// neighbours' light.
fn bake_probes(
    trace: &TraceScene,
    (origin, spacing, counts): (Vec3, f32, UVec3),
    settings: &LightBakeSettings,
    stats: &mut BakeStats,
) -> Probes {
    let total = (counts.x * counts.y * counts.z) as usize;
    let samples = settings.samples.max(16) * 2;
    let results: Vec<(Option<[f32; 12]>, u64)> = (0..total)
        .into_par_iter()
        .map(|i| {
            let i = i as u32;
            let cell = UVec3::new(i % counts.x, (i / counts.x) % counts.y, i / (counts.x * counts.y));
            let at = origin + cell.as_vec3() * spacing;
            let mut rng = Rng::new(settings.seed, u64::from(i), 2);
            let offset = (rng.next(), rng.next());
            let (mut c0, mut cx, mut cy, mut cz, mut back) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0);
            for s in 0..samples {
                let (u1, u2) = trace::r2(s, offset);
                let d = trace::sphere_sample(u1, u2);
                let got = trace.incoming(at, d, 0, &mut rng);
                back += u32::from(got.back);
                c0 += got.light;
                cx += got.light * d.x;
                cy += got.light * d.y;
                cz += got.light * d.z;
            }
            if back * 4 > samples {
                return (None, rng.rays);
            }
            // Radiance's order-1 harmonics, then irradiance over pi from
            // them: E/pi = L00 Y00 + 2/3 L1m Y1m (n), with Y00 = 0.282095
            // and Y1m = 0.488603 (x, y, z): over the sphere's 4 pi.
            let k = 4.0 * std::f32::consts::PI / samples as f32;
            let l0 = c0 * k * 0.282_095;
            let l1 = [cx, cy, cz].map(|c| c * k * 0.488_603);
            let e0 = l0 * 0.282_095;
            let e1 = l1.map(|c| c * 0.488_603 * (2.0 / 3.0));
            (
                Some([
                    e0.x, e0.y, e0.z, e1[0].x, e1[0].y, e1[0].z, e1[1].x, e1[1].y, e1[1].z, e1[2].x, e1[2].y, e1[2].z,
                ]),
                rng.rays,
            )
        })
        .collect();
    stats.probes = total as u64;
    stats.rays += results.iter().map(|r| r.1).sum::<u64>();
    let mut coefficients: Vec<Option<[f32; 12]>> = results.into_iter().map(|r| r.0).collect();
    // Probes inside geometry: their neighbours' mean, spreading inward.
    let at = |c: UVec3| (c.x + counts.x * (c.y + counts.y * c.z)) as usize;
    for _ in 0..counts.max_element() {
        let src = coefficients.clone();
        let mut left = false;
        for i in 0..total as u32 {
            if src[i as usize].is_some() {
                continue;
            }
            let cell = UVec3::new(i % counts.x, (i / counts.x) % counts.y, i / (counts.x * counts.y));
            let (mut sum, mut n) = ([0.0f32; 12], 0);
            for axis in 0..3 {
                for step in [-1i32, 1] {
                    let mut o = cell.as_ivec3();
                    o[axis] += step;
                    if o.min_element() < 0 || o.cmpge(counts.as_ivec3()).any() {
                        continue;
                    }
                    if let Some(c) = src[at(o.as_uvec3())] {
                        for (s, v) in sum.iter_mut().zip(c) {
                            *s += v;
                        }
                        n += 1;
                    }
                }
            }
            if n > 0 {
                coefficients[i as usize] = Some(sum.map(|s| s / n as f32));
            } else {
                left = true;
            }
        }
        if !left {
            break;
        }
    }
    Probes {
        origin,
        spacing,
        counts,
        coefficients: coefficients.into_iter().map(Option::unwrap_or_default).collect(),
    }
}

/// `x` as a half float (rounded to nearest, overflow to infinity).
fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = 1u32 << (shift - 1);
        let rounded = (m + half - 1 + ((m >> shift) & 1)) >> shift;
        return sign | rounded as u16;
    }
    let rounded = man + 0xfff + ((man >> 13) & 1);
    let (e, m) = if rounded & 0x80_0000 != 0 {
        (e + 1, 0)
    } else {
        (e, rounded >> 13)
    };
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    sign | ((e as u16) << 10) | m as u16
}

/// A half float's value.
fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let man = f32::from(h & 0x3ff);
    sign * match exp {
        0 => man * (2.0f32).powi(-24),
        0x1f => {
            if man == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + man / 1024.0) * (2.0f32).powi(exp - 15),
    }
}

#[cfg(test)]
mod tests;
