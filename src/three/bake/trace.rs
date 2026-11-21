//! The path tracer the baker gathers light with: the scene's triangles in
//! a [`Bvh`], what each surface is made of, the sun, lamps and sky.

use glam::Vec3;

use super::bvh::{Bvh, Tri};

/// What a model's surfaces are made of, in linear color.
#[derive(Clone, Copy, Debug)]
pub(super) struct Surface {
    pub albedo: Vec3,
    pub emissive: Vec3,
    pub double_sided: bool,
    /// Its albedo map, an index into `TraceScene::maps`.
    pub map: Option<u32>,
}

/// A point or spot light, as the renderer lights with it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Lamp {
    pub position: Vec3,
    /// Linear color times intensity.
    pub radiance: Vec3,
    pub range: f32,
    /// A spot light's forward direction and the cosines of its outer and
    /// inner half angles; `None` for a point light.
    pub cone: Option<(Vec3, f32, f32)>,
}

/// The sky's light from each direction: as the renderer's ambient light
/// without an atmosphere (sky above, ground below, an optional equator
/// color on the horizon), times the ambient intensity.
#[derive(Clone, Copy, Debug)]
pub(super) struct SkyLight {
    pub sky: Vec3,
    pub ground: Vec3,
    pub equator: Option<Vec3>,
}

impl SkyLight {
    pub fn radiance(&self, d: Vec3) -> Vec3 {
        match self.equator {
            Some(e) if d.y >= 0.0 => e.lerp(self.sky, d.y),
            Some(e) => e.lerp(self.ground, -d.y),
            None => self.ground.lerp(self.sky, d.y * 0.5 + 0.5),
        }
    }
}

pub(super) struct TraceScene {
    bvh: Bvh,
    /// Per triangle: which surface, its unit face normal, its corners'
    /// normals.
    pub tri_surface: Vec<u32>,
    pub tri_normal: Vec<Vec3>,
    pub tri_corner_normals: Vec<[Vec3; 3]>,
    pub surfaces: Vec<Surface>,
    /// Albedo maps, and each triangle's corners' texture coordinates.
    pub maps: Vec<std::sync::Arc<super::BakeTexture>>,
    pub tri_uvs: Vec<[glam::Vec2; 3]>,
    /// Unit direction towards the sun, and its light (linear color times
    /// intensity).
    pub sun: Option<(Vec3, Vec3)>,
    pub lamps: Vec<Lamp>,
    pub sky: SkyLight,
    /// How far rays start off the surface they leave.
    pub bias: f32,
    pub bounces: u32,
}

/// What a ray brought back: the light, and whether it hit the back of a
/// one-sided surface first (it started inside something).
pub(super) struct Incoming {
    pub light: Vec3,
    pub back: bool,
}

impl TraceScene {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tris: &[Tri],
        tri_surface: Vec<u32>,
        tri_corner_normals: Vec<[Vec3; 3]>,
        surfaces: Vec<Surface>,
        (maps, tri_uvs): (Vec<std::sync::Arc<super::BakeTexture>>, Vec<[glam::Vec2; 3]>),
        sun: Option<(Vec3, Vec3)>,
        lamps: Vec<Lamp>,
        sky: SkyLight,
        bounces: u32,
    ) -> Self {
        let tri_normal = tris.iter().map(|t| t.e1.cross(t.e2).normalize_or_zero()).collect();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for t in tris {
            for p in [t.v0, t.v0 + t.e1, t.v0 + t.e2] {
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        let size = if tris.is_empty() {
            1.0
        } else {
            (hi - lo).length().max(1e-3)
        };
        Self {
            bvh: Bvh::new(tris),
            tri_surface,
            tri_normal,
            tri_corner_normals,
            surfaces,
            maps,
            tri_uvs,
            sun,
            lamps,
            sky,
            bias: size * 2e-5 + 1e-4,
            bounces,
        }
    }

    /// Whether anything is in the way from `origin` along unit `dir`
    /// within `distance`.
    pub fn occluded(&self, origin: Vec3, dir: Vec3, distance: f32) -> bool {
        self.bvh.occluded(origin, dir, distance)
    }

    /// The light arriving at `p` (facing `n`) straight from the sun and the
    /// lamps, with their shadows: irradiance, as `direct_light` in the mesh
    /// shader counts it (radiance times the cosine).
    pub fn direct(&self, p: Vec3, n: Vec3, rays: &mut u64) -> Vec3 {
        let mut sum = Vec3::ZERO;
        if let Some((l, radiance)) = self.sun {
            let c = n.dot(l);
            if c > 0.0 && {
                *rays += 1;
                !self.bvh.occluded(p, l, f32::INFINITY)
            } {
                sum += radiance * c;
            }
        }
        for lamp in &self.lamps {
            let delta = lamp.position - p;
            let d2 = delta.length_squared();
            if d2 >= lamp.range * lamp.range || d2 <= 0.0 {
                continue;
            }
            let d = d2.sqrt();
            let l = delta / d;
            let c = n.dot(l);
            if c <= 0.0 {
                continue;
            }
            let ratio = d2 / (lamp.range * lamp.range);
            let window = (1.0 - ratio * ratio).max(0.0);
            let mut attenuation = window * window / d2.max(1.0);
            if let Some((forward, outer, inner)) = lamp.cone {
                let angle = (-l).dot(forward);
                attenuation *= if inner == outer {
                    f32::from(angle >= outer)
                } else {
                    let t = ((angle - outer) / (inner.max(outer + 1e-6) - outer)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
            }
            if attenuation > 0.0 && {
                *rays += 1;
                !self.bvh.occluded(p, l, d - self.bias)
            } {
                sum += lamp.radiance * attenuation * c;
            }
        }
        sum
    }

    /// The light arriving at `origin` from direction `dir`, from a surface
    /// `depth` reflections away from where the gathering started.
    pub fn incoming(&self, origin: Vec3, dir: Vec3, depth: u32, rng: &mut Rng) -> Incoming {
        rng.rays += 1;
        let Some(hit) = self.bvh.closest(origin, dir, f32::INFINITY) else {
            return Incoming {
                light: self.sky.radiance(dir),
                back: false,
            };
        };
        let i = hit.tri as usize;
        let surface = self.surfaces[self.tri_surface[i] as usize];
        let mut ng = self.tri_normal[i];
        let mut flip = false;
        if ng.dot(dir) > 0.0 {
            if !surface.double_sided {
                return Incoming {
                    light: Vec3::ZERO,
                    back: true,
                };
            }
            ng = -ng;
            flip = true;
        }
        let mut light = surface.emissive;
        let albedo = match surface.map {
            Some(map) => {
                let [a, b, c] = self.tri_uvs[i];
                let uv = a * (1.0 - hit.u - hit.v) + b * hit.u + c * hit.v;
                surface.albedo * self.maps[map as usize].sample(uv)
            }
            None => surface.albedo,
        };
        if depth < self.bounces && albedo != Vec3::ZERO {
            let [n0, n1, n2] = self.tri_corner_normals[i];
            let mut n = (n0 * (1.0 - hit.u - hit.v) + n1 * hit.u + n2 * hit.v).normalize_or(ng);
            if flip {
                n = -n;
            }
            if n.dot(ng) <= 0.0 {
                n = ng;
            }
            let p = origin + dir * hit.t + ng * self.bias;
            let direct = self.direct(p, n, &mut rng.rays);
            let bounce = self.incoming(p, cosine_sample(n, rng.next(), rng.next()), depth + 1, rng);
            light += albedo * (direct / std::f32::consts::PI + bounce.light);
        }
        Incoming { light, back: false }
    }
}

/// A direction around unit `n`, more of them nearer `n` (by the cosine),
/// from two numbers in 0..1.
pub(super) fn cosine_sample(n: Vec3, u1: f32, u2: f32) -> Vec3 {
    let r = u1.sqrt();
    let phi = std::f32::consts::TAU * u2;
    let (t, b) = n.any_orthonormal_pair();
    (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()).normalize_or(n)
}

/// A direction on the whole sphere, evenly, from two numbers in 0..1.
pub(super) fn sphere_sample(u1: f32, u2: f32) -> Vec3 {
    let z = 1.0 - 2.0 * u1;
    let r = (1.0 - z * z).max(0.0).sqrt();
    let phi = std::f32::consts::TAU * u2;
    Vec3::new(r * phi.cos(), z, r * phi.sin())
}

/// Sample `i` of a two-dimensional low-discrepancy sequence (R2), shifted
/// by `offset` (each texel its own), so few samples cover the hemisphere
/// evenly.
pub(super) fn r2(i: u32, offset: (f32, f32)) -> (f32, f32) {
    const A1: f64 = 0.754_877_666_246_692_8;
    const A2: f64 = 0.569_840_290_998_053_3;
    let x = (f64::from(offset.0) + A1 * f64::from(i)).fract() as f32;
    let y = (f64::from(offset.1) + A2 * f64::from(i)).fract() as f32;
    (x, y)
}

/// A small random number generator (SplitMix64), seeded per texel so the
/// result doesn't depend on which thread bakes it.
pub(super) struct Rng {
    state: u64,
    /// Rays traced with it so far.
    pub rays: u64,
}

impl Rng {
    pub fn new(seed: u64, a: u64, b: u64) -> Self {
        let mut r = Self {
            state: seed ^ a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
            rays: 0,
        };
        r.step();
        r
    }

    fn step(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in 0..1.
    pub fn next(&mut self) -> f32 {
        (self.step() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_samples_stay_above_the_surface_and_average_two_thirds_up() {
        let n = Vec3::new(0.3, 0.9, -0.2).normalize();
        let mut sum = 0.0;
        for i in 0..4096 {
            let (u1, u2) = r2(i, (0.1, 0.7));
            let d = cosine_sample(n, u1, u2);
            assert!(d.dot(n) >= -1e-5);
            assert!((d.length() - 1.0).abs() < 1e-4);
            sum += d.dot(n);
        }
        // The mean cosine of a cosine-weighted hemisphere is 2/3.
        assert!((sum / 4096.0 - 2.0 / 3.0).abs() < 0.01, "{}", sum / 4096.0);
    }
}
