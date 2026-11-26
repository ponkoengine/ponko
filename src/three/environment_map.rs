//! Linear HDR sources shared by scene lighting and reflection probes.
use std::sync::{Arc, OnceLock};

use glam::Vec3;
use image::Rgb32FImage;

use crate::error::{Error, ErrorKind};
use crate::gfx::Graphics;

use super::probe::cube_direction;

/// An HDR environment for [`SceneSettings::environment`](super::SceneSettings::environment).
/// Create one from a [panorama](Graphics::try_load_environment), six
/// [cube faces](Graphics::try_load_environment_cube), or [a neutral studio](Self::studio).
/// Cloning shares the image. The renderer prefilters it for reflections and
/// diffuse lighting when it is first used.
#[derive(Clone)]
pub struct EnvironmentMap(pub(crate) Arc<Vec<u16>>);

impl std::fmt::Debug for EnvironmentMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvironmentMap").finish_non_exhaustive()
    }
}

impl EnvironmentMap {
    /// A procedural, neutral studio with a broad ceiling light, two
    /// softboxes and dim fill. Requires no external image.
    #[must_use]
    pub fn studio() -> Self {
        static STUDIO: OnceLock<EnvironmentMap> = OnceLock::new();
        STUDIO
            .get_or_init(|| {
                let panels = [
                    (Vec3::new(-0.6, 1.0, 0.6).normalize(), 1.65, 9),
                    (Vec3::new(0.8, 0.55, 0.1).normalize(), 0.9, 18),
                    (Vec3::new(0.0, 0.8, -1.0).normalize(), 0.6, 12),
                ];
                Self::sample(|face, u, v| {
                    let d = cube_direction(face, u, v);
                    let fill = 0.045 + d.y.max(0.0) * 0.10;
                    let panels: f32 = panels
                        .iter()
                        .map(|&(axis, strength, sharpness)| strength * d.dot(axis).max(0.0).powi(sharpness))
                        .sum();
                    Vec3::splat(fill + panels)
                })
            })
            .clone()
    }

    fn sample(mut sample: impl FnMut(u32, f32, f32) -> Vec3) -> Self {
        let mut pixels = Vec::with_capacity(768 * 128 * 4);
        for y in 0..128 {
            for x in 0..768 {
                let c = sample(x / 128, ((x % 128) as f32 + 0.5) / 128.0, (y as f32 + 0.5) / 128.0);
                pixels.extend(
                    c.clamp(Vec3::ZERO, Vec3::splat(65504.0))
                        .to_array()
                        .map(|v| half::f16::from_f32(v).to_bits()),
                );
                pixels.push(half::f16::ONE.to_bits());
            }
        }
        Self(Arc::new(pixels))
    }
}

fn bilinear(image: &Rgb32FImage, u: f32, v: f32, wrap_x: bool) -> Vec3 {
    let (width, height) = (image.width() as i32, image.height() as i32);
    let (u, v) = (u * width as f32 - 0.5, v * height as f32 - 0.5);
    let (x, y) = (u.floor() as i32, v.floor() as i32);
    let sample = |x: i32, y: i32| {
        let x = if wrap_x {
            x.rem_euclid(width)
        } else {
            x.clamp(0, width - 1)
        };
        Vec3::from_array(image.get_pixel(x as u32, y.clamp(0, height - 1) as u32).0)
            .clamp(Vec3::ZERO, Vec3::splat(65504.0))
    };
    let (du, dv) = (u - u.floor(), v - v.floor());
    sample(x, y)
        .lerp(sample(x + 1, y), du)
        .lerp(sample(x, y + 1).lerp(sample(x + 1, y + 1), du), dv)
}

impl Graphics {
    /// Load a linear HDR or EXR environment image from the asset sources
    /// or `assets/`. The latitude-longitude image is 2:1, with +Y at the
    /// top and +X at the center.
    ///
    /// # Errors
    ///
    /// Returns the asset lookup/read error if it can't be read, [`Decode`](ErrorKind::Decode)
    /// for invalid image data or non-finite radiance, or [`Invalid`](ErrorKind::Invalid)
    /// for an image whose aspect ratio isn't 2:1.
    pub fn try_load_environment(&self, path: &str) -> Result<EnvironmentMap, Error> {
        let image = self.read_environment_image(path)?;
        if image.width() != image.height().saturating_mul(2) || image.height() == 0 {
            return Err(Error::new(ErrorKind::Invalid, "environment image must have 2:1 aspect ratio").with_path(path));
        }
        Ok(EnvironmentMap::sample(|face, u, v| {
            let d = cube_direction(face, u, v);
            let u = d.z.atan2(d.x) / std::f32::consts::TAU + 0.5;
            let v = 0.5 - d.y.asin() / std::f32::consts::PI;
            bilinear(&image, u, v, true)
        }))
    }

    /// Load a linear HDR or EXR cubemap as six images, in the order
    /// +X, -X, +Y, -Y, +Z, -Z. At each face center the directions to
    /// the right and up are respectively (-Z, +Y), (+Z, +Y), (+X, -Z),
    /// (+X, +Z), (+X, +Y) and (-X, +Y).
    ///
    /// # Errors
    ///
    /// Returns the asset lookup, read and decode errors of
    /// [`try_load_environment`](Self::try_load_environment), or
    /// [`Invalid`](ErrorKind::Invalid) if the faces aren't equal-size squares.
    pub fn try_load_environment_cube(&self, paths: [&str; 6]) -> Result<EnvironmentMap, Error> {
        let mut images = Vec::with_capacity(6);
        let mut side = None;
        for path in paths {
            let image = self.read_environment_image(path)?;
            let width = image.width();
            if width == 0 || width != image.height() || width != *side.get_or_insert(width) {
                return Err(
                    Error::new(ErrorKind::Invalid, "environment cube faces must be equal-size squares").with_path(path),
                );
            }
            images.push(image);
        }
        Ok(EnvironmentMap::sample(|face, u, v| {
            bilinear(&images[face as usize], u, v, false)
        }))
    }

    fn read_environment_image(&self, path: &str) -> Result<Rgb32FImage, Error> {
        let found = self.asset_sources.find(path)?;
        let origin = found.origin.clone();
        let bytes = found.into_bytes().map_err(|e| e.with_path(path))?;
        let image = image::load_from_memory(&bytes)
            .map_err(|e| Error::decode(path, format!("{} is not a valid image: {e}", origin.describe(path))))?
            .into_rgb32f();
        if image.pixels().any(|p| !Vec3::from_array(p.0).is_finite()) {
            return Err(Error::decode(path, "environment image contains non-finite radiance"));
        }
        Ok(image)
    }
}
