//! Albedo textures in the bake: a surface's color varies over it, and the
//! light it bounces with it.

use glam::{Vec2, Vec3};

use crate::error::{Error, ErrorKind};

/// The largest side an albedo texture keeps in a bake: bounced light is
/// soft, and a texture's average over a few centimeters is all it shows.
pub(crate) const MAX_SIDE: u32 = 128;

/// An albedo (base color) texture as the bake reads it: sRGB RGBA8 pixels
/// shrunk to at most 128 on a side and kept in linear color. Set it on a
/// [`BakeModel`](super::BakeModel) with
/// [`with_albedo_map`](super::BakeModel::with_albedo_map): the light the
/// model bounces is its color times the texture at the point a ray hits,
/// read through the mesh's first texture coordinates as the renderer reads
/// them (repeating past 0..1).
///
/// ```
/// // Two pixels: red on the left, blue on the right.
/// let map = ponko::render::BakeTexture::from_rgba(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
/// assert_eq!(map.size(), (2, 1));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BakeTexture {
    width: u32,
    height: u32,
    /// Linear color, row by row from the top.
    texels: Vec<Vec3>,
}

impl BakeTexture {
    /// The texture of `width` x `height` sRGB RGBA8 pixels (alpha ignored),
    /// averaged down by whole factors of two until no side is above 128.
    ///
    /// # Errors
    ///
    /// [`ErrorKind::Invalid`] for a size of 0 or `rgba` not
    /// `width * height * 4` bytes long.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Self, Error> {
        if width == 0 || height == 0 || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "BakeTexture::from_rgba: a {width} x {height} texture needs {} bytes of RGBA, got {}",
                    u64::from(width) * u64::from(height) * 4,
                    rgba.len()
                ),
            ));
        }
        let lut: Vec<f32> = (0..=255u8)
            .map(|v| {
                let c = f32::from(v) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            })
            .collect();
        let mut texels: Vec<Vec3> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| Vec3::new(lut[p[0] as usize], lut[p[1] as usize], lut[p[2] as usize]))
            .collect();
        let (mut w, mut h) = (width, height);
        while w > MAX_SIDE || h > MAX_SIDE {
            let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
            let mut next = vec![Vec3::ZERO; (nw * nh) as usize];
            for y in 0..nh {
                for x in 0..nw {
                    let mut sum = Vec3::ZERO;
                    let mut n = 0.0;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = (2 * x + dx, 2 * y + dy);
                        if sx < w && sy < h {
                            sum += texels[(sy * w + sx) as usize];
                            n += 1.0;
                        }
                    }
                    next[(y * nw + x) as usize] = sum / n;
                }
            }
            texels = next;
            (w, h) = (nw, nh);
        }
        Ok(Self {
            width: w,
            height: h,
            texels,
        })
    }

    /// Its width and height as kept (at most 128 each).
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The linear color at texture coordinates `uv` (0, 0 the top left
    /// corner), filtered between the four nearest texels, repeating.
    #[must_use]
    pub(crate) fn sample(&self, uv: Vec2) -> Vec3 {
        let (w, h) = (self.width as f32, self.height as f32);
        let p = Vec2::new(uv.x * w - 0.5, uv.y * h - 0.5);
        let base = p.floor();
        let t = p - base;
        let at = |x: f32, y: f32| {
            let xi = (x.rem_euclid(w) as u32).min(self.width - 1);
            let yi = (y.rem_euclid(h) as u32).min(self.height - 1);
            self.texels[(yi * self.width + xi) as usize]
        };
        // a + (b - a) t: exactly a where the texels are the same.
        let mix = |a: Vec3, b: Vec3, t: f32| a + (b - a) * t;
        let top = mix(at(base.x, base.y), at(base.x + 1.0, base.y), t.x);
        let bottom = mix(at(base.x, base.y + 1.0), at(base.x + 1.0, base.y + 1.0), t.x);
        mix(top, bottom, t.y)
    }

    /// Its texels in linear color, row by row (for the GPU baker).
    pub(crate) fn texels(&self) -> &[Vec3] {
        &self.texels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_texture_is_averaged_down_to_128_keeping_its_mean() {
        let (w, h) = (512u32, 300u32);
        let rgba: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if (i % w + i / w) % 2 == 0 {
                    [255, 255, 255, 255]
                } else {
                    [0, 0, 0, 255]
                }
            })
            .collect();
        let map = BakeTexture::from_rgba(w, h, &rgba).unwrap();
        assert_eq!(map.size(), (128, 75));
        // A checkerboard of white and black: half white everywhere.
        for t in map.texels() {
            assert!((t.x - 0.5).abs() < 0.02, "{t}");
        }
    }

    #[test]
    fn samples_filter_between_texels_and_repeat() {
        let map = BakeTexture::from_rgba(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
        assert!((map.sample(Vec2::new(0.25, 0.5)) - Vec3::X).length() < 1e-5);
        assert!((map.sample(Vec2::new(0.75, 0.5)) - Vec3::Z).length() < 1e-5);
        // Halfway, and past the edge where it repeats.
        assert!((map.sample(Vec2::new(0.5, 0.5)) - Vec3::new(0.5, 0.0, 0.5)).length() < 1e-5);
        assert!((map.sample(Vec2::new(1.25, 0.5)) - Vec3::X).length() < 1e-5);
        assert_eq!(
            BakeTexture::from_rgba(2, 2, &[0; 4]).unwrap_err().kind(),
            ErrorKind::Invalid
        );
    }
}
