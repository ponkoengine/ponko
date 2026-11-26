//! Material values that clips animate ([`AnimatedProperty::Material`]),
//! put on the materials just before a frame is drawn.
//!
//! Each animated material keeps the description the game or file gave it
//! (its base): every frame starts from the base, sets what the clips keyed
//! (mixed with the base by the weight they leave over) and updates the
//! material only when that differs from what was written last. A material
//! no clip keys any more goes back to its base. A material the game
//! updated meanwhile takes that as its new base.

use glam::{Vec2, Vec4};
use ponko_ecs::World;

use super::clip::any_material_clips;
use super::{AnimatedProperty, Material, MaterialDesc, MaterialProperty, Skeleton, linear};
use crate::color::Color;
use crate::gfx::Graphics;

/// What a glTF file says of a material, beyond its description, that its
/// animated values combine with: the description's emissive color is the
/// file's emissive factor times its strength, and its alpha the file's
/// times what transmission lets through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MaterialFactors {
    /// Linear RGB, before the strength.
    pub emissive: [f32; 3],
    pub emissive_strength: f32,
    /// What the base color's alpha is multiplied by.
    pub alpha_scale: f32,
}

impl MaterialFactors {
    /// The factors a description stands for on its own: its emissive color
    /// at strength 1, its alpha as it is.
    fn of(desc: &MaterialDesc) -> Self {
        let [r, g, b, _] = linear(desc.emissive);
        Self {
            emissive: [r, g, b],
            emissive_strength: 1.0,
            alpha_scale: 1.0,
        }
    }
}

/// One material clips animate.
#[derive(Debug, Clone)]
pub(crate) struct AnimatedMaterial {
    material: Material,
    base: MaterialDesc,
    factors: MaterialFactors,
    /// What the animation wrote last (`None`: the base is on).
    written: Option<MaterialDesc>,
    /// This frame's description and factors, while clips key it.
    next: Option<(MaterialDesc, MaterialFactors)>,
}

/// sRGB from linear, as glTF factors become a [`Color`].
fn srgb(linear: f32) -> f32 {
    super::gltf::to_srgb(linear.max(0.0))
}

/// `property`'s value in `desc` (and `factors`), in the components the
/// clip keys it in.
fn value_of(desc: &MaterialDesc, factors: &MaterialFactors, property: MaterialProperty) -> Vec4 {
    let rgb = |c: Color| {
        let [r, g, b, _] = linear(c);
        Vec4::new(r, g, b, 0.0)
    };
    let layers = &desc.layers;
    let x = |v: f32| Vec4::new(v, 0.0, 0.0, 0.0);
    let xy = |v: Vec2| Vec4::new(v.x, v.y, 0.0, 0.0);
    match property {
        MaterialProperty::Color => {
            let [r, g, b, a] = linear(desc.color);
            Vec4::new(r, g, b, a / factors.alpha_scale.max(1e-6))
        }
        MaterialProperty::Emissive => Vec4::from((glam::Vec3::from(factors.emissive), 0.0)),
        MaterialProperty::EmissiveStrength => x(factors.emissive_strength),
        MaterialProperty::Roughness => x(desc.roughness),
        MaterialProperty::Metallic => x(desc.metallic),
        MaterialProperty::NormalStrength => x(desc.normal_strength),
        MaterialProperty::AlphaCutoff => x(desc.alpha_cutoff),
        MaterialProperty::TextureOffset => xy(desc.texture_transform.offset),
        MaterialProperty::TextureRotation => x(desc.texture_transform.rotation),
        MaterialProperty::TextureScale => xy(desc.texture_transform.scale),
        MaterialProperty::Clearcoat => x(layers.clearcoat),
        MaterialProperty::ClearcoatRoughness => x(layers.clearcoat_roughness),
        MaterialProperty::SheenColor => rgb(layers.sheen_color),
        MaterialProperty::SheenRoughness => x(layers.sheen_roughness),
        MaterialProperty::Specular => x(layers.specular),
        MaterialProperty::SpecularColor => rgb(layers.specular_color),
        MaterialProperty::Ior => x(layers.ior),
        MaterialProperty::Anisotropy => x(layers.anisotropy),
        MaterialProperty::AnisotropyRotation => x(layers.anisotropy_rotation),
        MaterialProperty::Iridescence => x(layers.iridescence),
        MaterialProperty::IridescenceIor => x(layers.iridescence_ior),
        MaterialProperty::IridescenceThinnest => x(layers.iridescence_thickness[0]),
        MaterialProperty::IridescenceThickest => x(layers.iridescence_thickness[1]),
    }
}

/// Sets `property` to `v` in `desc` (and `factors`), each value kept in
/// the range the material takes.
fn set(desc: &mut MaterialDesc, factors: &mut MaterialFactors, property: MaterialProperty, v: Vec4) {
    let unit = |v: f32| v.clamp(0.0, 1.0);
    let color = |v: Vec4| Color::rgb(srgb(v.x), srgb(v.y), srgb(v.z));
    let layers = &mut desc.layers;
    match property {
        MaterialProperty::Color => {
            desc.color = Color::rgba(srgb(v.x), srgb(v.y), srgb(v.z), unit(v.w * factors.alpha_scale));
        }
        MaterialProperty::Emissive => factors.emissive = [v.x, v.y, v.z].map(|c| c.max(0.0)),
        MaterialProperty::EmissiveStrength => factors.emissive_strength = v.x.max(0.0),
        MaterialProperty::Roughness => desc.roughness = unit(v.x),
        MaterialProperty::Metallic => desc.metallic = unit(v.x),
        MaterialProperty::NormalStrength => desc.normal_strength = v.x,
        MaterialProperty::AlphaCutoff => desc.alpha_cutoff = v.x.max(0.0),
        MaterialProperty::TextureOffset => desc.texture_transform.offset = v.truncate().truncate(),
        MaterialProperty::TextureRotation => desc.texture_transform.rotation = v.x,
        MaterialProperty::TextureScale => desc.texture_transform.scale = v.truncate().truncate(),
        MaterialProperty::Clearcoat => layers.clearcoat = unit(v.x),
        MaterialProperty::ClearcoatRoughness => layers.clearcoat_roughness = unit(v.x),
        MaterialProperty::SheenColor => layers.sheen_color = color(v),
        MaterialProperty::SheenRoughness => layers.sheen_roughness = unit(v.x),
        MaterialProperty::Specular => layers.specular = unit(v.x),
        MaterialProperty::SpecularColor => layers.specular_color = color(v),
        MaterialProperty::Ior => layers.ior = v.x.max(1.0),
        MaterialProperty::Anisotropy => layers.anisotropy = v.x.clamp(-1.0, 1.0),
        MaterialProperty::AnisotropyRotation => layers.anisotropy_rotation = v.x,
        MaterialProperty::Iridescence => layers.iridescence = unit(v.x),
        MaterialProperty::IridescenceIor => layers.iridescence_ior = v.x.max(1.0),
        MaterialProperty::IridescenceThinnest => layers.iridescence_thickness[0] = v.x.max(0.0),
        MaterialProperty::IridescenceThickest => layers.iridescence_thickness[1] = v.x.max(0.0),
    }
}

impl AnimatedMaterial {
    /// The description `desc` with the emissive color of `factors`.
    fn finished(mut desc: MaterialDesc, factors: &MaterialFactors) -> MaterialDesc {
        let [r, g, b] = factors.emissive.map(|c| srgb(c * factors.emissive_strength));
        desc.emissive = Color::rgb(r, g, b);
        desc
    }
}

impl Graphics {
    /// Puts this frame's animated material values (what the clips sampled
    /// into the world's skeletons) on their materials. Nothing to do while
    /// no live clip animates a material and none was animated.
    pub(crate) fn apply_animated_materials(&mut self, world: &mut World) {
        let assets = &mut self.assets3d;
        if !any_material_clips() && assets.animated_materials.is_empty() {
            return;
        }
        let descs = &assets.material_descs;
        let factors_of = &assets.material_factors;
        let animated = &mut assets.animated_materials;
        world.query::<&Skeleton>().for_each(|skeleton| {
            if !skeleton.morphs.keys_materials() {
                return;
            }
            for (target, sum, coverage) in skeleton.morphs.properties() {
                let AnimatedProperty::Material(material, property) = target else {
                    continue;
                };
                let Some(&now) = descs.get(&material.id) else {
                    continue; // freed, or not this game's
                };
                let state = animated.entry(material.id).or_insert_with(|| AnimatedMaterial {
                    material,
                    base: now,
                    factors: factors_of
                        .get(&material.id)
                        .copied()
                        .unwrap_or_else(|| MaterialFactors::of(&now)),
                    written: None,
                    next: None,
                });
                let (desc, factors) = state.next.get_or_insert_with(|| {
                    // Updated by the game since the animation wrote it: the
                    // game's is the new base.
                    if state.written.is_some_and(|w| w != now) || (state.written.is_none() && state.base != now) {
                        state.base = now;
                        state.factors = MaterialFactors::of(&now);
                    }
                    (state.base, state.factors)
                });
                let value = sum + value_of(desc, factors, property) * (1.0 - coverage);
                set(desc, factors, property, value);
            }
        });
        let writes = &mut assets.animated_writes;
        writes.clear();
        for state in animated.values_mut() {
            let desc = match state.next.take() {
                Some((desc, factors)) => AnimatedMaterial::finished(desc, &factors),
                None if state.written.is_some() => state.base,
                None => continue,
            };
            if state.written.unwrap_or(state.base) != desc {
                writes.push((state.material, desc));
            }
            state.written = (desc != state.base).then_some(desc);
        }
        // Materials back on their base need no state: none is kept for a
        // game whose clips stopped animating materials.
        animated.retain(|_, s| s.written.is_some());
        let mut writes = std::mem::take(&mut self.assets3d.animated_writes);
        for (material, desc) in writes.drain(..) {
            // Out-of-range values were clamped; a description the material
            // still refuses keeps what it had.
            let _ = self.try_update_material(material, desc);
        }
        self.assets3d.animated_writes = writes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_property_reads_back_what_it_sets() {
        let mut desc = MaterialDesc::default();
        let mut factors = MaterialFactors::of(&desc);
        for (property, v) in [
            (MaterialProperty::Color, Vec4::new(0.25, 0.5, 1.0, 0.5)),
            (MaterialProperty::Emissive, Vec4::new(1.0, 0.5, 0.0, 0.0)),
            (MaterialProperty::EmissiveStrength, Vec4::X * 3.0),
            (MaterialProperty::Roughness, Vec4::X * 0.25),
            (MaterialProperty::Metallic, Vec4::X * 0.75),
            (MaterialProperty::NormalStrength, Vec4::X * 2.0),
            (MaterialProperty::AlphaCutoff, Vec4::X * 0.25),
            (MaterialProperty::TextureOffset, Vec4::new(0.5, -0.25, 0.0, 0.0)),
            (MaterialProperty::TextureRotation, Vec4::X * 1.5),
            (MaterialProperty::TextureScale, Vec4::new(2.0, 3.0, 0.0, 0.0)),
            (MaterialProperty::Clearcoat, Vec4::X * 0.5),
            (MaterialProperty::ClearcoatRoughness, Vec4::X * 0.25),
            (MaterialProperty::SheenColor, Vec4::new(1.0, 0.0, 0.5, 0.0)),
            (MaterialProperty::SheenRoughness, Vec4::X * 0.5),
            (MaterialProperty::Specular, Vec4::X * 0.5),
            (MaterialProperty::SpecularColor, Vec4::new(0.0, 1.0, 0.5, 0.0)),
            (MaterialProperty::Ior, Vec4::X * 1.33),
            (MaterialProperty::Anisotropy, Vec4::X * -0.5),
            (MaterialProperty::AnisotropyRotation, Vec4::X * 0.5),
            (MaterialProperty::Iridescence, Vec4::X * 0.5),
            (MaterialProperty::IridescenceIor, Vec4::X * 1.8),
            (MaterialProperty::IridescenceThinnest, Vec4::X * 150.0),
            (MaterialProperty::IridescenceThickest, Vec4::X * 600.0),
        ] {
            set(&mut desc, &mut factors, property, v);
            let back = value_of(&desc, &factors, property);
            assert!(back.abs_diff_eq(v, 1e-4), "{property:?}: set {v}, read {back}");
        }
    }

    #[test]
    fn out_of_range_values_are_kept_in_range() {
        let mut desc = MaterialDesc::default();
        let mut factors = MaterialFactors::of(&desc);
        set(&mut desc, &mut factors, MaterialProperty::Roughness, Vec4::X * 3.0);
        set(&mut desc, &mut factors, MaterialProperty::Ior, Vec4::X * 0.5);
        set(&mut desc, &mut factors, MaterialProperty::Color, Vec4::new(1.0, 1.0, 1.0, 2.0));
        assert_eq!((desc.roughness, desc.layers.ior, desc.color.a), (1.0, 1.0, 1.0));
        assert!(desc.layers.problem().is_none());
    }

    #[test]
    fn the_emissive_color_is_the_factor_times_the_strength() {
        let factors = MaterialFactors {
            emissive: [0.5, 0.25, 0.0],
            emissive_strength: 2.0,
            alpha_scale: 1.0,
        };
        let desc = AnimatedMaterial::finished(MaterialDesc::default(), &factors);
        let [r, g, b, _] = linear(desc.emissive);
        assert!((r - 1.0).abs() < 1e-4 && (g - 0.5).abs() < 1e-4 && b == 0.0, "{r} {g} {b}");
    }
}
