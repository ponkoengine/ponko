//! Per-map texture coordinates used by the built-in material shaders.

use super::TextureTransform;

/// A material texture whose coordinates can be overridden independently.
///
/// Occlusion and metallic-roughness can use separate coordinates even
/// when they live in the same packed ORM image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum TextureMap {
    /// Base color and alpha coverage.
    BaseColor,
    /// Emissive color.
    Emissive,
    /// Tangent-space normal.
    Normal,
    /// Metallic and roughness (blue and green of the ORM image).
    MetallicRoughness,
    /// Ambient occlusion (red of the ORM image).
    Occlusion,
    /// Clear coat amount.
    Clearcoat,
    /// Clear coat roughness.
    ClearcoatRoughness,
    /// Clear coat normal.
    ClearcoatNormal,
    /// Sheen color.
    SheenColor,
    /// Sheen roughness.
    SheenRoughness,
    /// Specular strength.
    Specular,
    /// Specular color.
    SpecularColor,
    /// Anisotropy.
    Anisotropy,
    /// Iridescence amount.
    Iridescence,
    /// Iridescence thickness.
    IridescenceThickness,
    /// Specular transmission amount.
    Transmission,
    /// Volume thickness.
    Thickness,
    /// Diffuse transmission color.
    DiffuseTransmissionColor,
    /// Diffuse transmission amount.
    DiffuseTransmission,
    /// Volume scatter color.
    ScatterColor,
    /// Volume scatter strength.
    Scatter,
}

impl TextureMap {
    /// All map kinds, in the order used for the four-transform limit.
    pub const ALL: [Self; 21] = [
        Self::BaseColor,
        Self::Emissive,
        Self::Normal,
        Self::MetallicRoughness,
        Self::Occlusion,
        Self::Clearcoat,
        Self::ClearcoatRoughness,
        Self::ClearcoatNormal,
        Self::SheenColor,
        Self::SheenRoughness,
        Self::Specular,
        Self::SpecularColor,
        Self::Anisotropy,
        Self::Iridescence,
        Self::IridescenceThickness,
        Self::Transmission,
        Self::Thickness,
        Self::DiffuseTransmissionColor,
        Self::DiffuseTransmission,
        Self::ScatterColor,
        Self::Scatter,
    ];

    pub(crate) const CORE_COUNT: usize = Self::Clearcoat as usize;
}

/// Per-map overrides of [`MaterialDesc::texture_transform`](super::MaterialDesc::texture_transform).
///
/// Each map defaults to the shared transform and the mesh's primary
/// texture coordinates. Overrides are drawn by the built-in shaders.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct TextureTransformOverrides {
    transforms: [TextureTransform; TextureMap::ALL.len()],
    mask: u32,
    uv1: u32,
}

impl TextureTransformOverrides {
    /// The override for a map, or `None` if it inherits the material's transform.
    pub fn get(&self, map: TextureMap) -> Option<TextureTransform> {
        (self.mask & (1 << map as usize) != 0).then_some(self.transforms[map as usize])
    }

    /// Sets an override, or clears it with `None`.
    pub fn set(&mut self, map: TextureMap, transform: Option<TextureTransform>) {
        let bit = 1 << map as usize;
        self.transforms[map as usize] = transform.unwrap_or_default();
        self.mask = (self.mask & !bit) | (u32::from(transform.is_some()) * bit);
    }

    /// Whether the map reads the mesh's second texture coordinates.
    pub fn uses_uv1(&self, map: TextureMap) -> bool {
        self.uv1 & (1 << map as usize) != 0
    }

    /// Selects the mesh's secondary (`true`) or primary (`false`) coordinates.
    pub fn set_uv1(&mut self, map: TextureMap, uses_uv1: bool) {
        let bit = 1 << map as usize;
        self.uv1 = (self.uv1 & !bit) | (u32::from(uses_uv1) * bit);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.uv1 == 0 && self.mask == 0
    }

    pub(crate) fn is_finite(&self) -> bool {
        self.transforms.iter().all(TextureTransform::is_finite)
    }
}

/// Which channels of a packed ORM image represent actual texture references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OrmMaps {
    #[default]
    Both,
    MetallicRoughness,
    Occlusion,
}

/// A deduplicated table. Entry zero is the fallback when the table is full.
#[derive(Default)]
pub(super) struct TransformTable {
    transforms: [TextureTransform; 4],
    count: usize,
}

impl TransformTable {
    pub(super) fn insert(&mut self, transform: TextureTransform) -> Option<usize> {
        if let Some(i) = self.transforms[..self.count].iter().position(|t| *t == transform) {
            return Some(i);
        }
        let i = self.count;
        *self.transforms.get_mut(i)? = transform;
        self.count += 1;
        Some(i)
    }

    pub(super) fn rows(&self, i: usize) -> [[f32; 4]; 2] {
        uv_rows(self.transforms[i])
    }
}

/// Two uniform rows of an affine texture transform.
pub(super) fn uv_rows(transform: TextureTransform) -> [[f32; 4]; 2] {
    let m = transform.matrix();
    [
        [m.x_axis.x, m.y_axis.x, m.z_axis.x, 0.0],
        [m.x_axis.y, m.y_axis.y, m.z_axis.y, 0.0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    #[test]
    fn table_deduplicates_and_limits_distinct_transforms() {
        let mut table = TransformTable::default();
        let transform = |x| TextureTransform::IDENTITY.with_offset(Vec2::new(x, 0.0));
        for i in 0..4 {
            assert_eq!(table.insert(transform(i as f32)), Some(i));
        }
        assert_eq!(table.insert(transform(2.0)), Some(2));
        assert_eq!(table.insert(transform(4.0)), None);
        assert_eq!(table.rows(0)[0], [1.0, 0.0, 0.0, 0.0]);
    }
}
