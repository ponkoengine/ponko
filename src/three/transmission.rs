//! Light through a material's surface and the medium behind it.

use crate::color::Color;
use crate::texture::Texture;

/// The transmission layer of a built-in PBR material.
///
/// Clear transmission samples the opaque HDR scene and preserves specular
/// highlights. Diffuse transmission lights the surface from its back, as
/// for paper or foliage. Both are off by default. See
/// [`MaterialLayers`](super::MaterialLayers) for texture limits.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Transmission {
    /// Share of dielectric light transmitted through the surface (0 to 1).
    pub factor: f32,
    /// Modulates [`factor`](Self::factor) in the red channel (linear).
    pub map: Option<Texture>,
    /// Thickness in mesh units. Zero means an infinitely thin wall.
    pub thickness: f32,
    /// Modulates [`thickness`](Self::thickness) in the green channel (linear).
    pub thickness_map: Option<Texture>,
    /// World-space distance at which white transmitted light takes on
    /// [`attenuation_color`](Self::attenuation_color). Infinity is clear.
    pub attenuation_distance: f32,
    /// The attenuation tint (sRGB, alpha unused).
    pub attenuation_color: Color,
    /// Angular color separation in a volume (glTF's 20 / Abbe number).
    pub dispersion: f32,
    /// Share of the remaining dielectric diffuse light scattered through
    /// the surface from the other side (0 to 1).
    pub diffuse: f32,
    /// Modulates [`diffuse`](Self::diffuse) in the alpha channel.
    pub diffuse_map: Option<Texture>,
    /// Diffuse transmitted tint (sRGB, alpha unused).
    pub diffuse_color: Color,
    /// Modulates [`diffuse_color`](Self::diffuse_color) in RGB (sRGB).
    pub diffuse_color_map: Option<Texture>,
    /// Share of clear transmission redirected into an approximate
    /// scattering lobe (0 to 1).
    pub scatter: f32,
    /// Modulates [`scatter`](Self::scatter) in the alpha channel.
    pub scatter_map: Option<Texture>,
    /// Scattered light's tint (sRGB, alpha unused).
    pub scatter_color: Color,
    /// Modulates [`scatter_color`](Self::scatter_color) in RGB (sRGB).
    pub scatter_color_map: Option<Texture>,
    /// Bias toward forward (positive) or backward (negative) scattering.
    pub scatter_anisotropy: f32,
}

impl Default for Transmission {
    fn default() -> Self {
        Self::NONE
    }
}

impl Transmission {
    /// No transmission, with clear medium and neutral tints.
    pub const NONE: Self = Self {
        factor: 0.0,
        map: None,
        thickness: 0.0,
        thickness_map: None,
        attenuation_distance: f32::INFINITY,
        attenuation_color: Color::WHITE,
        dispersion: 0.0,
        diffuse: 0.0,
        diffuse_map: None,
        diffuse_color: Color::WHITE,
        diffuse_color_map: None,
        scatter: 0.0,
        scatter_map: None,
        scatter_color: Color::WHITE,
        scatter_color_map: None,
        scatter_anisotropy: 0.0,
    };

    /// Thin-walled, clear transmission of the given share (0 to 1).
    #[must_use]
    pub fn new(factor: f32) -> Self {
        Self { factor, ..Self::NONE }
    }

    /// Adds a volume of mesh-space thickness, with a world-space
    /// attenuation distance and sRGB attenuation tint.
    #[must_use]
    pub fn with_volume(mut self, thickness: f32, distance: f32, color: Color) -> Self {
        self.thickness = thickness;
        self.attenuation_distance = distance;
        self.attenuation_color = color;
        self
    }

    pub(crate) fn problem(&self) -> Option<String> {
        let unit =
            |name: &str, v: f32| (!(0.0..=1.0).contains(&v)).then(|| format!("{name} goes from 0 to 1 (got {v})"));
        unit("transmission", self.factor)
            .or_else(|| unit("diffuse transmission", self.diffuse))
            .or_else(|| unit("scatter strength", self.scatter))
            .or_else(|| {
                (!(self.thickness >= 0.0 && self.thickness.is_finite()))
                    .then(|| "volume thickness must be finite and nonnegative".to_string())
            })
            .or_else(|| {
                (self.attenuation_distance <= 0.0 || self.attenuation_distance.is_nan())
                    .then(|| "volume attenuation distance must be positive".to_string())
            })
            .or_else(|| {
                (!(self.dispersion >= 0.0 && self.dispersion.is_finite()))
                    .then(|| "dispersion must be finite and nonnegative".to_string())
            })
            .or_else(|| {
                (!(-1.0..=1.0).contains(&self.scatter_anisotropy))
                    .then(|| "scatter anisotropy goes from -1 to 1".to_string())
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::three::{LayerSlots, MaterialLayers};

    #[test]
    fn diffuse_transmission_color_and_alpha_can_share_an_image() {
        let packed = Texture::new(2, 1, 1);
        let thickness = Texture::new(3, 1, 1);
        let layer = Transmission {
            diffuse: 1.0,
            diffuse_map: Some(packed),
            diffuse_color_map: Some(packed),
            thickness_map: Some(thickness),
            ..Transmission::NONE
        };
        let layers = MaterialLayers::NONE.with_transmission(layer);
        let (slots, count) = LayerSlots::of(&layers, |_| true);
        assert_eq!(count, 2);
        assert_eq!(slots.images, [Some((3, false)), Some((2, true))]);
        assert_eq!(slots.packed >> 24 & 0b1111, 0b1010);
        assert_eq!(
            MaterialLayers::NONE
                .with_maps(layers.maps().map(|(map, _)| map))
                .transmission
                .diffuse_color_map,
            Some(packed),
        );
    }
}
