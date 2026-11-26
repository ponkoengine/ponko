//! Layers over a material's base surface: clear coat, sheen, specular
//! strength and color, index of refraction, anisotropy, iridescence and transmission, as
//! glTF's material extensions describe them (`KHR_materials_clearcoat`,
//! `_sheen`, `_specular`, `_ior`, `_anisotropy`, `_iridescence`, `_transmission`).
//!
//! A material with any layer on is drawn by a second build of the built-in
//! shader that has the layers' code; every other material keeps the plain
//! build, which has none of it, so layers cost nothing where they're off.

use super::Transmission;
use crate::color::Color;
use crate::texture::Texture;

/// Layers over a [`MaterialDesc`](super::MaterialDesc)'s base surface, as
/// glTF's material extensions describe them. [`Default`] (and
/// [`MaterialLayers::NONE`]) has every layer off.
///
/// ```
/// # use ponko::prelude::*;
/// use ponko::render::{MaterialDesc, MaterialLayers};
/// // Car paint: a red base under a smooth clear coat.
/// let paint = MaterialDesc::color(Color::hex(0xA01010))
///     .with_roughness(0.5)
///     .with_layers(MaterialLayers::NONE.with_clearcoat(1.0, 0.05));
/// assert!(paint.layers.is_on());
/// ```
///
/// # Cost
///
/// A material with any layer on ([`is_on`](Self::is_on)) is drawn by a
/// second build of the built-in shader, made the first time such a material
/// is (a few milliseconds, once). Each layer adds work only to the pixels of
/// the materials that have it on: a clear coat is a second specular lobe and
/// reflection, sheen a fabric lobe, anisotropy a stretched specular lobe,
/// iridescence a thin-film color per pixel. Materials without layers are
/// drawn exactly as before.
///
/// # Maps
///
/// Maps are read in glTF's channels: the clear coat amount from red, its
/// roughness from green; the sheen color from red, green and blue (sRGB)
/// and its roughness from alpha; the specular color from red, green and
/// blue (sRGB) and its strength from alpha; the anisotropy direction from
/// red and green (-1 to 1, in the surface's texture directions) and its
/// strength from blue; the iridescence amount from red and its thickness
/// from green. See [`Transmission`] for the transmission map channels.
/// One image may serve several maps (glTF files often pack
/// them). A material reads **at most two distinct layer images**
/// ([`Graphics::try_create_material`](crate::gfx::Graphics::try_create_material)
/// says so otherwise); the same image as a sheen or specular color and as
/// another map counts twice, as it is read once as color and once as data.
///
/// Layers are drawn by the built-in shader: a material with a custom
/// `shader` can't have them, and on a layered material `custom_textures`
/// are not read (the layer maps take their slots).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MaterialLayers {
    /// How much clear coat: a thin, colorless, glossy layer over the base
    /// (car paint, varnish). 0 (the default) is none, 1 full.
    pub clearcoat: f32,
    /// The clear coat's roughness, 0 (mirror) to 1. Default 0.
    pub clearcoat_roughness: f32,
    /// Multiplies [`clearcoat`](Self::clearcoat) by its red channel.
    pub clearcoat_map: Option<Texture>,
    /// Multiplies [`clearcoat_roughness`](Self::clearcoat_roughness) by its green channel.
    pub clearcoat_roughness_map: Option<Texture>,
    /// The clear coat's own normal map (tangent space, as
    /// [`normal_map`](super::MaterialDesc::normal_map)). Without one the
    /// coat follows the mesh's surface, not the base's normal map.
    pub clearcoat_normal_map: Option<Texture>,
    /// How strong the clear coat's normal map is. Default 1.
    pub clearcoat_normal_strength: f32,
    /// The sheen's color: the soft glow of fabric fibers toward grazing
    /// angles (velvet, cloth). Black (the default) is no sheen.
    pub sheen_color: Color,
    /// How rough the sheen is, 0 to 1: rougher spreads it wider. Default 0.
    pub sheen_roughness: f32,
    /// Multiplies [`sheen_color`](Self::sheen_color) by its red, green and blue (sRGB).
    pub sheen_color_map: Option<Texture>,
    /// Multiplies [`sheen_roughness`](Self::sheen_roughness) by its alpha.
    pub sheen_roughness_map: Option<Texture>,
    /// How strong a non-metal's reflection is, 0 to 1. Default 1 (a plain
    /// material's).
    pub specular: f32,
    /// Tints a non-metal's reflection head-on. Default white.
    pub specular_color: Color,
    /// Multiplies [`specular`](Self::specular) by its alpha.
    pub specular_map: Option<Texture>,
    /// Multiplies [`specular_color`](Self::specular_color) by its red, green and blue (sRGB).
    pub specular_color_map: Option<Texture>,
    /// The index of refraction of a non-metal, which sets how much it
    /// reflects head-on: 1.5 (the default, 4%) is a plain material's, water
    /// is 1.33, diamond 2.4.
    pub ior: f32,
    /// How much the specular highlight stretches along one direction
    /// (brushed metal, hair), -1 to 1: positive along the direction, negative
    /// across it. 0 (the default) is none.
    pub anisotropy: f32,
    /// Turns the direction counter-clockwise from the texture's u direction,
    /// in radians. Default 0.
    pub anisotropy_rotation: f32,
    /// The direction (red, green: -1 to 1 along u and up the image) and a
    /// strength that multiplies [`anisotropy`](Self::anisotropy) (blue).
    pub anisotropy_map: Option<Texture>,
    /// How much a thin film's rainbow colors show (soap bubbles, oil,
    /// beetles). 0 (the default) is none, 1 full.
    pub iridescence: f32,
    /// The film's index of refraction. Default 1.3.
    pub iridescence_ior: f32,
    /// The film's thickness in nanometers: from the first (where the
    /// thickness map is black) to the second (where it is white, and
    /// everywhere without one). Default 100 to 400.
    pub iridescence_thickness: [f32; 2],
    /// Multiplies [`iridescence`](Self::iridescence) by its red channel.
    pub iridescence_map: Option<Texture>,
    /// Places the thickness between the two of
    /// [`iridescence_thickness`](Self::iridescence_thickness) by its green channel.
    pub iridescence_thickness_map: Option<Texture>,
    /// Specular and diffuse transmission, with optional volume attenuation.
    pub transmission: Transmission,
}

impl Default for MaterialLayers {
    fn default() -> Self {
        Self::NONE
    }
}

impl MaterialLayers {
    /// Every layer off: the material is drawn as without layers.
    pub const NONE: Self = Self {
        clearcoat: 0.0,
        clearcoat_roughness: 0.0,
        clearcoat_map: None,
        clearcoat_roughness_map: None,
        clearcoat_normal_map: None,
        clearcoat_normal_strength: 1.0,
        sheen_color: Color::rgb(0.0, 0.0, 0.0),
        sheen_roughness: 0.0,
        sheen_color_map: None,
        sheen_roughness_map: None,
        specular: 1.0,
        specular_color: Color::WHITE,
        specular_map: None,
        specular_color_map: None,
        ior: 1.5,
        anisotropy: 0.0,
        anisotropy_rotation: 0.0,
        anisotropy_map: None,
        iridescence: 0.0,
        iridescence_ior: 1.3,
        iridescence_thickness: [100.0, 400.0],
        iridescence_map: None,
        iridescence_thickness_map: None,
        transmission: Transmission::NONE,
    };

    /// A clear coat of `amount` (0 to 1) and `roughness` (0 to 1).
    #[must_use]
    pub fn with_clearcoat(mut self, amount: f32, roughness: f32) -> Self {
        self.clearcoat = amount;
        self.clearcoat_roughness = roughness;
        self
    }

    /// A sheen of `color` and `roughness` (0 to 1).
    #[must_use]
    pub fn with_sheen(mut self, color: Color, roughness: f32) -> Self {
        self.sheen_color = color;
        self.sheen_roughness = roughness;
        self
    }

    /// A non-metal's reflection: `strength` (0 to 1) and `color`.
    #[must_use]
    pub fn with_specular(mut self, strength: f32, color: Color) -> Self {
        self.specular = strength;
        self.specular_color = color;
        self
    }

    /// The index of refraction (1.5 is a plain material's).
    #[must_use]
    pub fn with_ior(mut self, ior: f32) -> Self {
        self.ior = ior;
        self
    }

    /// A highlight stretched by `strength` (-1 to 1), turned by `rotation`
    /// radians from the texture's u direction.
    #[must_use]
    pub fn with_anisotropy(mut self, strength: f32, rotation: f32) -> Self {
        self.anisotropy = strength;
        self.anisotropy_rotation = rotation;
        self
    }

    /// A thin film of `amount` (0 to 1), index of refraction `ior` and
    /// `thickness` in nanometers (where its map is white, or everywhere
    /// without one).
    #[must_use]
    pub fn with_iridescence(mut self, amount: f32, ior: f32, thickness: f32) -> Self {
        self.iridescence = amount;
        self.iridescence_ior = ior;
        self.iridescence_thickness[1] = thickness;
        self
    }

    with_fields! {
        /// Sets the specular and diffuse [`transmission`](Self::transmission) layer.
        transmission: Transmission => with_transmission;
        /// Sets [`clearcoat_map`](Self::clearcoat_map).
        clearcoat_map: Option<Texture> => with_clearcoat_map;
        /// Sets [`clearcoat_roughness_map`](Self::clearcoat_roughness_map).
        clearcoat_roughness_map: Option<Texture> => with_clearcoat_roughness_map;
        /// Sets [`clearcoat_normal_map`](Self::clearcoat_normal_map).
        clearcoat_normal_map: Option<Texture> => with_clearcoat_normal_map;
        /// Sets [`sheen_color_map`](Self::sheen_color_map).
        sheen_color_map: Option<Texture> => with_sheen_color_map;
        /// Sets [`sheen_roughness_map`](Self::sheen_roughness_map).
        sheen_roughness_map: Option<Texture> => with_sheen_roughness_map;
        /// Sets [`specular_map`](Self::specular_map).
        specular_map: Option<Texture> => with_specular_map;
        /// Sets [`specular_color_map`](Self::specular_color_map).
        specular_color_map: Option<Texture> => with_specular_color_map;
        /// Sets [`anisotropy_map`](Self::anisotropy_map).
        anisotropy_map: Option<Texture> => with_anisotropy_map;
        /// Sets [`iridescence_map`](Self::iridescence_map).
        iridescence_map: Option<Texture> => with_iridescence_map;
        /// Sets [`iridescence_thickness_map`](Self::iridescence_thickness_map).
        iridescence_thickness_map: Option<Texture> => with_iridescence_thickness_map;
    }

    /// Whether any layer is on (and the material is drawn by the layered
    /// build of the built-in shader).
    #[must_use]
    pub fn is_on(&self) -> bool {
        self.bits() != 0
    }

    /// Which layers are on, as the shader's bits ([`layer_bits`]).
    pub(crate) fn bits(&self) -> u32 {
        let mut bits = 0;
        if self.clearcoat > 0.0 {
            bits |= layer_bits::CLEARCOAT;
        }
        if self.sheen_color.r > 0.0 || self.sheen_color.g > 0.0 || self.sheen_color.b > 0.0 {
            bits |= layer_bits::SHEEN;
        }
        let white = self.specular_color.r >= 1.0 && self.specular_color.g >= 1.0 && self.specular_color.b >= 1.0;
        if self.specular != 1.0
            || !white
            || self.ior != 1.5
            || self.specular_map.is_some()
            || self.specular_color_map.is_some()
        {
            bits |= layer_bits::SPECULAR;
        }
        if self.anisotropy != 0.0 {
            bits |= layer_bits::ANISOTROPY;
        }
        if self.iridescence > 0.0 {
            bits |= layer_bits::IRIDESCENCE;
        }
        if self.transmission.factor > 0.0 || self.transmission.diffuse > 0.0 {
            bits |= layer_bits::TRANSMISSION;
        }
        bits
    }

    /// The layer maps in the shader's order ([`MapKind`]), each with
    /// whether it is read as color (sRGB) or data.
    pub(crate) fn maps(&self) -> [(Option<Texture>, MapKind); MAP_KINDS] {
        use MapKind as K;
        [
            (self.clearcoat_map, K::Clearcoat),
            (self.clearcoat_roughness_map, K::ClearcoatRoughness),
            (self.clearcoat_normal_map, K::ClearcoatNormal),
            (self.sheen_color_map, K::SheenColor),
            (self.sheen_roughness_map, K::SheenRoughness),
            (self.specular_map, K::Specular),
            (self.specular_color_map, K::SpecularColor),
            (self.anisotropy_map, K::Anisotropy),
            (self.iridescence_map, K::Iridescence),
            (self.iridescence_thickness_map, K::IridescenceThickness),
            (self.transmission.map, K::Transmission),
            (self.transmission.thickness_map, K::Thickness),
            (self.transmission.diffuse_color_map, K::DiffuseTransmissionColor),
            (self.transmission.diffuse_map, K::DiffuseTransmission),
            (self.transmission.scatter_color_map, K::ScatterColor),
            (self.transmission.scatter_map, K::Scatter),
        ]
    }

    /// Sets every layer map, in the shader's order ([`MapKind`]).
    pub(crate) fn with_maps(mut self, maps: [Option<Texture>; MAP_KINDS]) -> Self {
        let [
            a,
            b,
            c,
            d,
            e,
            f,
            g,
            h,
            i,
            j,
            transmission,
            thickness,
            diffuse_color,
            diffuse,
            scatter_color,
            scatter,
        ] = maps;
        self.clearcoat_map = a;
        self.clearcoat_roughness_map = b;
        self.clearcoat_normal_map = c;
        self.sheen_color_map = d;
        self.sheen_roughness_map = e;
        self.specular_map = f;
        self.specular_color_map = g;
        self.anisotropy_map = h;
        self.iridescence_map = i;
        self.iridescence_thickness_map = j;
        self.transmission.map = transmission;
        self.transmission.thickness_map = thickness;
        self.transmission.diffuse_map = diffuse;
        self.transmission.diffuse_color_map = diffuse_color;
        self.transmission.scatter_map = scatter;
        self.transmission.scatter_color_map = scatter_color;
        self
    }

    /// Checks the values: what's out of range, in words.
    pub(crate) fn problem(&self) -> Option<String> {
        let unit =
            |name: &str, v: f32| (!(0.0..=1.0).contains(&v)).then(|| format!("{name} goes from 0 to 1 (got {v})"));
        self.transmission
            .problem()
            .or_else(|| unit("clear coat", self.clearcoat))
            .or_else(|| unit("clear coat roughness", self.clearcoat_roughness))
            .or_else(|| unit("sheen roughness", self.sheen_roughness))
            .or_else(|| unit("specular", self.specular))
            .or_else(|| unit("iridescence", self.iridescence))
            .or_else(|| {
                (!(-1.0..=1.0).contains(&self.anisotropy))
                    .then(|| format!("anisotropy goes from -1 to 1 (got {})", self.anisotropy))
            })
            .or_else(|| {
                (!(self.ior >= 1.0 && self.ior.is_finite()))
                    .then(|| format!("the index of refraction is 1 or more (got {})", self.ior))
            })
            .or_else(|| {
                (!(self.iridescence_ior >= 1.0 && self.iridescence_ior.is_finite())).then(|| {
                    format!(
                        "the iridescence's index of refraction is 1 or more (got {})",
                        self.iridescence_ior
                    )
                })
            })
            .or_else(|| {
                let [a, b] = self.iridescence_thickness;
                (!(a >= 0.0 && b >= 0.0 && a.is_finite() && b.is_finite()))
                    .then(|| format!("the iridescence's thickness is 0 nanometers or more (got {a} to {b})"))
            })
    }
}

/// The game code of the layered build of the built-in shader: nothing of
/// its own. The engine composes this exact text with the layers' code on
/// (`compose_material`), as it composes `""` without.
pub(crate) const LAYERED_SHADER: &str = "// ponko: the built-in shader with material layers\n";

/// The shader's bits for each layer (`MaterialData.layer_info.x`).
pub(crate) mod layer_bits {
    pub const CLEARCOAT: u32 = 1;
    pub const SHEEN: u32 = 2;
    /// Specular strength and color, and the index of refraction.
    pub const SPECULAR: u32 = 4;
    pub const ANISOTROPY: u32 = 8;
    pub const IRIDESCENCE: u32 = 16;
    pub const TRANSMISSION: u32 = 32;
}

/// How many kinds of layer map there are.
pub(crate) const MAP_KINDS: usize = 16;

/// A kind of layer map, in the shader's order: map `k`'s slot is bits
/// `2k..2k + 2` of `MaterialData.layer_info.y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapKind {
    Clearcoat,
    ClearcoatRoughness,
    ClearcoatNormal,
    SheenColor,
    SheenRoughness,
    Specular,
    SpecularColor,
    Anisotropy,
    Iridescence,
    IridescenceThickness,
    Transmission,
    Thickness,
    DiffuseTransmissionColor,
    DiffuseTransmission,
    ScatterColor,
    Scatter,
}

impl MapKind {
    /// How the map's image is read: `Some(true)` as color (sRGB decoded),
    /// `Some(false)` as data, `None` either way (it reads only alpha,
    /// which sRGB leaves alone).
    pub(crate) fn srgb(self) -> Option<bool> {
        match self {
            MapKind::SheenColor
            | MapKind::SpecularColor
            | MapKind::DiffuseTransmissionColor
            | MapKind::ScatterColor => Some(true),
            MapKind::SheenRoughness | MapKind::Specular | MapKind::DiffuseTransmission | MapKind::Scatter => None,
            _ => Some(false),
        }
    }
}

/// Where a material's layer maps are bound: up to two images, each read
/// as color or as data, and each map's slot (0: none, 1 or 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct LayerSlots {
    /// The two images (texture ids), each with whether it is read as sRGB.
    pub images: [Option<(u32, bool)>; 2],
    /// Each map's slot, two bits per [`MapKind`].
    pub packed: u32,
}

impl LayerSlots {
    /// Places `layers`' maps (those `live` keeps) in at most two slots,
    /// first come first placed, and says how many distinct images all of
    /// them would need (more than two: the rest are left out).
    pub(crate) fn of(layers: &MaterialLayers, live: impl Fn(Texture) -> bool) -> (Self, usize) {
        Self::place(
            layers
                .maps()
                .map(|(map, kind)| (map.filter(|t| live(*t)).map(|t| t.id), kind)),
        )
    }

    /// [`of`](Self::of) for maps given as image ids (texture ids, or a
    /// file's image indices).
    pub(crate) fn place(maps: [(Option<u32>, MapKind); MAP_KINDS]) -> (Self, usize) {
        let mut slots = Self::default();
        let mut needed = 0;
        for (k, (map, kind)) in maps.into_iter().enumerate() {
            let Some(image) = map else {
                continue;
            };
            let found = slots.images.iter().position(|slot| {
                slot.is_some_and(|(id, srgb)| id == image && kind.srgb().is_none_or(|want| want == srgb))
            });
            let slot = match found {
                Some(slot) => slot,
                None => {
                    needed += 1;
                    match slots.images.iter().position(Option::is_none) {
                        Some(free) => {
                            slots.images[free] = Some((image, kind.srgb().unwrap_or(false)));
                            free
                        }
                        None => continue,
                    }
                }
            };
            slots.packed |= (slot as u32 + 1) << (2 * k);
        }
        (slots, needed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(id: u32) -> Texture {
        Texture::new(id, 1, 1)
    }

    #[test]
    fn no_layers_are_on_by_default() {
        assert!(!MaterialLayers::default().is_on());
        assert_eq!(MaterialLayers::NONE.bits(), 0);
        assert_eq!(MaterialLayers::NONE.with_ior(1.5).bits(), 0);
        assert_eq!(MaterialLayers::NONE.with_ior(1.33).bits(), layer_bits::SPECULAR);
        assert_eq!(
            MaterialLayers::NONE
                .with_clearcoat(1.0, 0.1)
                .with_anisotropy(-0.5, 0.0)
                .bits(),
            layer_bits::CLEARCOAT | layer_bits::ANISOTROPY
        );
        assert_eq!(
            MaterialLayers::NONE.with_sheen(Color::rgb(0.0, 0.0, 0.1), 0.5).bits(),
            layer_bits::SHEEN
        );
    }

    #[test]
    fn maps_of_one_image_share_a_slot_unless_read_as_color_and_as_data() {
        // glTF's packing: clear coat in red, its roughness in green, of one image.
        let packed = MaterialLayers::NONE
            .with_clearcoat(1.0, 1.0)
            .with_clearcoat_map(texture(7))
            .with_clearcoat_roughness_map(texture(7));
        let slots = LayerSlots::of(&packed, |_| true).0;
        assert_eq!(slots.images, [Some((7, false)), None]);
        assert_eq!(slots.packed, 0b0101);
        // Sheen color (sRGB) and roughness (alpha) of one image: one slot.
        let sheen = MaterialLayers::NONE
            .with_sheen(Color::WHITE, 1.0)
            .with_sheen_color_map(texture(3))
            .with_sheen_roughness_map(texture(3));
        let slots = LayerSlots::of(&sheen, |_| true).0;
        assert_eq!(slots.images, [Some((3, true)), None]);
        assert_eq!(slots.packed, (1 << 6) | (1 << 8));
        // The same image as color and as data: two slots.
        let both = sheen.with_clearcoat(1.0, 0.0).with_clearcoat_map(texture(3));
        let slots = LayerSlots::of(&both, |_| true).0;
        assert_eq!(slots.images, [Some((3, false)), Some((3, true))]);
        // A third image is one too many; a freed one isn't counted.
        let three = both.with_anisotropy(1.0, 0.0).with_anisotropy_map(texture(9));
        let (slots, needed) = LayerSlots::of(&three, |_| true);
        assert_eq!(needed, 3);
        assert_eq!(slots.packed >> 14 & 3, 0, "the third image is left out");
        let (slots, needed) = LayerSlots::of(&three, |t| t.id != 9);
        assert_eq!((needed, slots.packed >> 14 & 3), (2, 0));
    }

    #[test]
    fn out_of_range_values_are_named() {
        assert!(MaterialLayers::NONE.problem().is_none());
        let bad = MaterialLayers::NONE.with_clearcoat(2.0, 0.0);
        assert!(bad.problem().unwrap().contains("clear coat goes from 0 to 1"));
        let bad = MaterialLayers::NONE.with_ior(0.5);
        assert!(bad.problem().unwrap().contains("index of refraction"));
        let bad = MaterialLayers::NONE.with_anisotropy(-1.5, 0.0);
        assert!(bad.problem().unwrap().contains("anisotropy"));
    }
}
