//! Versioned descriptors: settings structs that can grow within an ABI
//! version (STABILITY.md, "Versioned descriptors").
//!
//! The ABI 2 structs (`PonkoMaterialDesc`, `PonkoNavBakeSettings`,
//! `PonkoLighting`, ...) have fixed layouts: a release can't add a field to
//! them. Each `...Ex` struct here starts with `uint32_t struct_size` and
//! `uint32_t version`, then holds its ABI 2 struct (so old code converts by
//! assignment), then the fields added since. The caller sets both header
//! fields to what its header says (`sizeof` and the struct's
//! `PONKO_..._VERSION`), usually with `= {.struct_size = sizeof d, .version = PONKO_..._VERSION}` and the
//! struct's `_defaults` call, and the library:
//!
//! - reads (or, for outputs, writes) only the fields of that version, so a
//!   program built against an older header keeps working with a newer
//!   library: the fields added after its version get their defaults;
//! - refuses a `version` of 0, a `version` newer than it knows (the program
//!   needs a newer library), and a `struct_size` smaller than that version's
//!   fields, with `PONKO_INVALID_ARGUMENT`;
//! - never keeps a pointer into the struct or anything it points to past the
//!   call: what it needs, it copies.
//!
//! Collision descriptors (`PonkoColliderEx`, `PonkoQueryFilterEx`) are in
//! `collision_ex.rs`.

use std::mem::{offset_of, size_of};

use ponko::render::{Lighting, MaterialLayers, Sky, TextureMap, TextureTransform};

use crate::ctx::{ctx_access, out};
use crate::nav3d::{PonkoNavBakeSettings, PonkoNavSources, ponko_ctx_bake_navmesh, ponko_nav_bake_settings_default};
use crate::stencil::{PonkoStencil, ponko_stencil_default, stencil_from_c};
use crate::three::{
    PonkoLighting, PonkoMaterial, PonkoMaterialDesc, lighting_to_c, material_from_c, ponko_ctx_set_lighting,
    ponko_material_desc,
};
use crate::types::{PonkoColor, PonkoTexture, texture_from_c, texture_to_c, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid, invalid_owned};

/// A struct with the versioned-descriptor header (`struct_size`, `version`
/// as its first two `u32` fields).
pub(crate) trait Versioned: Copy {
    /// Its C name, for error messages.
    const NAME: &'static str;
    /// The newest version this library knows.
    const VERSION: u32;
    /// How many bytes from the start version `version` covers (the header
    /// included), up to the end of its last field: not `sizeof`, whose tail
    /// padding a later field may take. `None` for versions it doesn't know.
    fn len_of(version: u32) -> Option<usize>;
}

/// The size and version a caller put in a descriptor's header, checked:
/// returns how many bytes of it to read or write.
///
/// # Safety
/// `ptr` non-null and readable for at least the 8 header bytes.
unsafe fn checked_len<T: Versioned>(ptr: *const T) -> Result<usize, Failure> {
    let words = ptr.cast::<u32>();
    // SAFETY: caller contract: the two header words are readable.
    let (size, version) = unsafe { (words.read(), words.add(1).read()) };
    if version == 0 {
        return Err(invalid_owned(format!(
            "{}: version is 0; set it to PONKO_..._VERSION from the header (and struct_size to its sizeof)",
            T::NAME
        )));
    }
    let Some(len) = T::len_of(version) else {
        return Err(invalid_owned(format!(
            "{}: version {version} is newer than this library knows ({}): the program was built against a newer ponko.h",
            T::NAME,
            T::VERSION
        )));
    };
    if (size as usize) < len {
        return Err(invalid_owned(format!(
            "{}: struct_size {size} is smaller than version {version}'s {len} bytes",
            T::NAME
        )));
    }
    Ok(len)
}

/// Reads a caller's descriptor: the fields of its version over `defaults`
/// (which gives the fields added since). Null is an error.
///
/// # Safety
/// `ptr` null or readable for `struct_size` bytes, with valid field values.
pub(crate) unsafe fn read_desc<T: Versioned>(ptr: *const T, defaults: T) -> Result<T, Failure> {
    if ptr.is_null() {
        return Err(invalid_owned(format!("{} is null", T::NAME)));
    }
    // SAFETY: non-null; caller contract.
    let len = unsafe { checked_len(ptr)? };
    let mut value = defaults;
    debug_assert!(len <= size_of::<T>());
    // SAFETY: `len` is at most `size_of::<T>()` (a `len_of` covers T's own
    // fields) and at most `struct_size`, which the caller says is readable;
    // the two don't overlap (`value` is a local).
    unsafe { std::ptr::copy_nonoverlapping(ptr.cast::<u8>(), (&raw mut value).cast::<u8>(), len) };
    Ok(value)
}

/// Writes `value` into a caller's descriptor: the fields of the caller's
/// version, leaving its header (and anything past that version) alone.
///
/// # Safety
/// `ptr` null or writable for `struct_size` bytes, its header readable.
pub(crate) unsafe fn write_desc<T: Versioned>(ptr: *mut T, value: &T) -> Result<(), Failure> {
    if ptr.is_null() {
        return Err(invalid_owned(format!("{} output is null", T::NAME)));
    }
    // SAFETY: non-null; caller contract.
    let len = unsafe { checked_len(ptr)? };
    const HEADER: usize = 8;
    // SAFETY: as in `read_desc`, the other way round; the header stays.
    unsafe {
        std::ptr::copy_nonoverlapping(
            std::ptr::from_ref(value).cast::<u8>().add(HEADER),
            ptr.cast::<u8>().add(HEADER),
            len - HEADER,
        );
    }
    Ok(())
}

/// The end of field `$f` of `$t` (its offset plus its size).
macro_rules! end_of {
    ($t:ty, $f:ident: $ft:ty) => {
        offset_of!($t, $f) + size_of::<$ft>()
    };
}

/// The header both fields of which every descriptor starts with.
macro_rules! assert_header {
    ($t:ty) => {
        const _: () = assert!(offset_of!($t, struct_size) == 0 && offset_of!($t, version) == 4);
    };
}

// ---- material ---------------------------------------------------------------

/// The version of `PonkoMaterialDescEx` this header describes.
pub const PONKO_MATERIAL_DESC_EX_VERSION: u32 = 5;

/// Layers over a material's surface (Rust `MaterialLayers`): clear coat,
/// sheen, specular strength and color, index of refraction, anisotropy and
/// iridescence, as glTF's material extensions describe them.
/// `ponko_material_desc_ex_defaults` sets every layer off. Each `has_*`
/// says whether the map after it is used. Maps are read in glTF's channels
/// (see the Rust docs); a material reads at most two distinct layer images,
/// and layers need the built-in shader (`has_shader` false).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoMaterialLayers {
    /// Clear coat amount, 0 (none) to 1.
    pub clearcoat: f32,
    /// Clear coat roughness, 0 to 1.
    pub clearcoat_roughness: f32,
    /// Whether `clearcoat_map` is used.
    pub has_clearcoat_map: bool,
    /// Multiplies `clearcoat` by its red channel.
    pub clearcoat_map: PonkoTexture,
    /// Whether `clearcoat_roughness_map` is used.
    pub has_clearcoat_roughness_map: bool,
    /// Multiplies `clearcoat_roughness` by its green channel.
    pub clearcoat_roughness_map: PonkoTexture,
    /// Whether `clearcoat_normal_map` is used.
    pub has_clearcoat_normal_map: bool,
    /// The clear coat's own normal map.
    pub clearcoat_normal_map: PonkoTexture,
    /// How strong the clear coat's normal map is (1).
    pub clearcoat_normal_strength: f32,
    /// Sheen color (sRGB); black is none.
    pub sheen_color: PonkoColor,
    /// Sheen roughness, 0 to 1.
    pub sheen_roughness: f32,
    /// Whether `sheen_color_map` is used.
    pub has_sheen_color_map: bool,
    /// Multiplies `sheen_color` by its red, green and blue.
    pub sheen_color_map: PonkoTexture,
    /// Whether `sheen_roughness_map` is used.
    pub has_sheen_roughness_map: bool,
    /// Multiplies `sheen_roughness` by its alpha.
    pub sheen_roughness_map: PonkoTexture,
    /// A non-metal's reflection strength, 0 to 1 (1).
    pub specular: f32,
    /// Tints a non-metal's reflection (white).
    pub specular_color: PonkoColor,
    /// Whether `specular_map` is used.
    pub has_specular_map: bool,
    /// Multiplies `specular` by its alpha.
    pub specular_map: PonkoTexture,
    /// Whether `specular_color_map` is used.
    pub has_specular_color_map: bool,
    /// Multiplies `specular_color` by its red, green and blue.
    pub specular_color_map: PonkoTexture,
    /// Index of refraction, 1 or more (1.5).
    pub ior: f32,
    /// Highlight stretch, -1 to 1 (0: none).
    pub anisotropy: f32,
    /// Turns the stretch from the texture's u direction, in radians.
    pub anisotropy_rotation: f32,
    /// Whether `anisotropy_map` is used.
    pub has_anisotropy_map: bool,
    /// Direction (red, green) and strength (blue).
    pub anisotropy_map: PonkoTexture,
    /// Thin-film amount, 0 (none) to 1.
    pub iridescence: f32,
    /// The film's index of refraction (1.3).
    pub iridescence_ior: f32,
    /// The film's thickness in nanometers: where the thickness map is
    /// black, and where it is white (100, 400).
    pub iridescence_thickness: [f32; 2],
    /// Whether `iridescence_map` is used.
    pub has_iridescence_map: bool,
    /// Multiplies `iridescence` by its red channel.
    pub iridescence_map: PonkoTexture,
    /// Whether `iridescence_thickness_map` is used.
    pub has_iridescence_thickness_map: bool,
    /// Places the thickness by its green channel.
    pub iridescence_thickness_map: PonkoTexture,
}

fn layers_to_c(l: &MaterialLayers) -> PonkoMaterialLayers {
    let map = |t: Option<ponko::prelude::Texture>| (t.is_some(), t.map_or(NO_TEXTURE, texture_to_c));
    let (has_clearcoat_map, clearcoat_map) = map(l.clearcoat_map);
    let (has_clearcoat_roughness_map, clearcoat_roughness_map) = map(l.clearcoat_roughness_map);
    let (has_clearcoat_normal_map, clearcoat_normal_map) = map(l.clearcoat_normal_map);
    let (has_sheen_color_map, sheen_color_map) = map(l.sheen_color_map);
    let (has_sheen_roughness_map, sheen_roughness_map) = map(l.sheen_roughness_map);
    let (has_specular_map, specular_map) = map(l.specular_map);
    let (has_specular_color_map, specular_color_map) = map(l.specular_color_map);
    let (has_anisotropy_map, anisotropy_map) = map(l.anisotropy_map);
    let (has_iridescence_map, iridescence_map) = map(l.iridescence_map);
    let (has_iridescence_thickness_map, iridescence_thickness_map) = map(l.iridescence_thickness_map);
    PonkoMaterialLayers {
        clearcoat: l.clearcoat,
        clearcoat_roughness: l.clearcoat_roughness,
        has_clearcoat_map,
        clearcoat_map,
        has_clearcoat_roughness_map,
        clearcoat_roughness_map,
        has_clearcoat_normal_map,
        clearcoat_normal_map,
        clearcoat_normal_strength: l.clearcoat_normal_strength,
        sheen_color: to_c(l.sheen_color),
        sheen_roughness: l.sheen_roughness,
        has_sheen_color_map,
        sheen_color_map,
        has_sheen_roughness_map,
        sheen_roughness_map,
        specular: l.specular,
        specular_color: to_c(l.specular_color),
        has_specular_map,
        specular_map,
        has_specular_color_map,
        specular_color_map,
        ior: l.ior,
        anisotropy: l.anisotropy,
        anisotropy_rotation: l.anisotropy_rotation,
        has_anisotropy_map,
        anisotropy_map,
        iridescence: l.iridescence,
        iridescence_ior: l.iridescence_ior,
        iridescence_thickness: l.iridescence_thickness,
        has_iridescence_map,
        iridescence_map,
        has_iridescence_thickness_map,
        iridescence_thickness_map,
    }
}

fn layers_from_c(l: &PonkoMaterialLayers) -> MaterialLayers {
    let map = |has: bool, t: PonkoTexture| has.then(|| texture_from_c(t));
    let mut out = MaterialLayers::NONE
        .with_clearcoat(l.clearcoat, l.clearcoat_roughness)
        .with_clearcoat_map(map(l.has_clearcoat_map, l.clearcoat_map))
        .with_clearcoat_roughness_map(map(l.has_clearcoat_roughness_map, l.clearcoat_roughness_map))
        .with_clearcoat_normal_map(map(l.has_clearcoat_normal_map, l.clearcoat_normal_map))
        .with_sheen(to_c(l.sheen_color), l.sheen_roughness)
        .with_sheen_color_map(map(l.has_sheen_color_map, l.sheen_color_map))
        .with_sheen_roughness_map(map(l.has_sheen_roughness_map, l.sheen_roughness_map))
        .with_specular(l.specular, to_c(l.specular_color))
        .with_specular_map(map(l.has_specular_map, l.specular_map))
        .with_specular_color_map(map(l.has_specular_color_map, l.specular_color_map))
        .with_ior(l.ior)
        .with_anisotropy(l.anisotropy, l.anisotropy_rotation)
        .with_anisotropy_map(map(l.has_anisotropy_map, l.anisotropy_map))
        .with_iridescence_map(map(l.has_iridescence_map, l.iridescence_map))
        .with_iridescence_thickness_map(map(l.has_iridescence_thickness_map, l.iridescence_thickness_map));
    out.clearcoat_normal_strength = l.clearcoat_normal_strength;
    out.iridescence = l.iridescence;
    out.iridescence_ior = l.iridescence_ior;
    out.iridescence_thickness = l.iridescence_thickness;
    out
}

/// Light through a material's surface and the medium behind it (Rust
/// `Transmission`, glTF's transmission, volume, dispersion and diffuse
/// transmission extensions). `ponko_material_desc_ex_defaults` sets it off:
/// no transmission, an infinite (clear) attenuation distance and white tints.
/// Each `has_*` says whether the map after it is used. The maps count
/// towards the material's two distinct layer images, and transmission needs
/// the built-in shader (`has_shader` false).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoMaterialTransmission {
    /// Share of dielectric light transmitted through the surface, 0 (none) to 1.
    pub factor: f32,
    /// Whether `map` is used.
    pub has_map: bool,
    /// Multiplies `factor` by its red channel.
    pub map: PonkoTexture,
    /// Volume thickness in mesh units; 0 is a thin wall.
    pub thickness: f32,
    /// Whether `thickness_map` is used.
    pub has_thickness_map: bool,
    /// Multiplies `thickness` by its green channel.
    pub thickness_map: PonkoTexture,
    /// World-space distance at which white light takes on `attenuation_color`
    /// (infinity: clear).
    pub attenuation_distance: f32,
    /// The volume's tint (sRGB, alpha unused; white).
    pub attenuation_color: PonkoColor,
    /// Color separation in a volume, glTF's 20 / Abbe number (0: none).
    pub dispersion: f32,
    /// Share of the remaining diffuse light scattered through the surface from
    /// the other side, 0 (none) to 1.
    pub diffuse: f32,
    /// Whether `diffuse_map` is used.
    pub has_diffuse_map: bool,
    /// Multiplies `diffuse` by its alpha.
    pub diffuse_map: PonkoTexture,
    /// Diffuse transmitted tint (sRGB, alpha unused; white).
    pub diffuse_color: PonkoColor,
    /// Whether `diffuse_color_map` is used.
    pub has_diffuse_color_map: bool,
    /// Multiplies `diffuse_color` by its red, green and blue.
    pub diffuse_color_map: PonkoTexture,
    /// Share of clear transmission redirected into an approximate scattering
    /// lobe, 0 (none) to 1.
    pub scatter: f32,
    /// Whether `scatter_map` is used.
    pub has_scatter_map: bool,
    /// Multiplies `scatter` by its alpha.
    pub scatter_map: PonkoTexture,
    /// Scattered light's tint (sRGB, alpha unused; white).
    pub scatter_color: PonkoColor,
    /// Whether `scatter_color_map` is used.
    pub has_scatter_color_map: bool,
    /// Multiplies `scatter_color` by its red, green and blue.
    pub scatter_color_map: PonkoTexture,
    /// Bias of the scattering toward forward (positive) or backward
    /// (negative) light, -1 to 1 (0).
    pub scatter_anisotropy: f32,
}

fn transmission_to_c(t: &ponko::render::Transmission) -> PonkoMaterialTransmission {
    let map = |t: Option<ponko::prelude::Texture>| (t.is_some(), t.map_or(NO_TEXTURE, texture_to_c));
    let (has_map, map_) = map(t.map);
    let (has_thickness_map, thickness_map) = map(t.thickness_map);
    let (has_diffuse_map, diffuse_map) = map(t.diffuse_map);
    let (has_diffuse_color_map, diffuse_color_map) = map(t.diffuse_color_map);
    let (has_scatter_map, scatter_map) = map(t.scatter_map);
    let (has_scatter_color_map, scatter_color_map) = map(t.scatter_color_map);
    PonkoMaterialTransmission {
        factor: t.factor,
        has_map,
        map: map_,
        thickness: t.thickness,
        has_thickness_map,
        thickness_map,
        attenuation_distance: t.attenuation_distance,
        attenuation_color: to_c(t.attenuation_color),
        dispersion: t.dispersion,
        diffuse: t.diffuse,
        has_diffuse_map,
        diffuse_map,
        diffuse_color: to_c(t.diffuse_color),
        has_diffuse_color_map,
        diffuse_color_map,
        scatter: t.scatter,
        has_scatter_map,
        scatter_map,
        scatter_color: to_c(t.scatter_color),
        has_scatter_color_map,
        scatter_color_map,
        scatter_anisotropy: t.scatter_anisotropy,
    }
}

fn transmission_from_c(t: &PonkoMaterialTransmission) -> ponko::render::Transmission {
    let map = |has: bool, t: PonkoTexture| has.then(|| texture_from_c(t));
    // `Transmission` is `#[non_exhaustive]`: start from its defaults.
    let mut out = ponko::render::Transmission::NONE;
    out.factor = t.factor;
    out.map = map(t.has_map, t.map);
    out.thickness = t.thickness;
    out.thickness_map = map(t.has_thickness_map, t.thickness_map);
    out.attenuation_distance = t.attenuation_distance;
    out.attenuation_color = to_c(t.attenuation_color);
    out.dispersion = t.dispersion;
    out.diffuse = t.diffuse;
    out.diffuse_map = map(t.has_diffuse_map, t.diffuse_map);
    out.diffuse_color = to_c(t.diffuse_color);
    out.diffuse_color_map = map(t.has_diffuse_color_map, t.diffuse_color_map);
    out.scatter = t.scatter;
    out.scatter_map = map(t.has_scatter_map, t.scatter_map);
    out.scatter_color = to_c(t.scatter_color);
    out.scatter_color_map = map(t.has_scatter_color_map, t.scatter_color_map);
    out.scatter_anisotropy = t.scatter_anisotropy;
    out
}

const NO_TEXTURE: PonkoTexture = PonkoTexture {
    id: 0,
    width: 0,
    height: 0,
};

/// Map indices and mask positions for `PonkoTextureTransformOverrides`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoTextureMap {
    /// Base color and alpha coverage.
    PONKO_TEXTURE_MAP_BASE_COLOR = 0,
    /// Emissive color.
    PONKO_TEXTURE_MAP_EMISSIVE = 1,
    /// Tangent-space normal.
    PONKO_TEXTURE_MAP_NORMAL = 2,
    /// Metallic and roughness in the packed ORM map.
    PONKO_TEXTURE_MAP_METALLIC_ROUGHNESS = 3,
    /// Occlusion in the packed ORM map.
    PONKO_TEXTURE_MAP_OCCLUSION = 4,
    /// Clear coat amount.
    PONKO_TEXTURE_MAP_CLEARCOAT = 5,
    /// Clear coat roughness.
    PONKO_TEXTURE_MAP_CLEARCOAT_ROUGHNESS = 6,
    /// Clear coat normal.
    PONKO_TEXTURE_MAP_CLEARCOAT_NORMAL = 7,
    /// Sheen color.
    PONKO_TEXTURE_MAP_SHEEN_COLOR = 8,
    /// Sheen roughness.
    PONKO_TEXTURE_MAP_SHEEN_ROUGHNESS = 9,
    /// Specular strength.
    PONKO_TEXTURE_MAP_SPECULAR = 10,
    /// Specular color.
    PONKO_TEXTURE_MAP_SPECULAR_COLOR = 11,
    /// Anisotropy.
    PONKO_TEXTURE_MAP_ANISOTROPY = 12,
    /// Iridescence amount.
    PONKO_TEXTURE_MAP_IRIDESCENCE = 13,
    /// Iridescence thickness.
    PONKO_TEXTURE_MAP_IRIDESCENCE_THICKNESS = 14,
    /// Transmission amount.
    PONKO_TEXTURE_MAP_TRANSMISSION = 15,
    /// Volume thickness.
    PONKO_TEXTURE_MAP_THICKNESS = 16,
    /// Diffuse transmission color.
    PONKO_TEXTURE_MAP_DIFFUSE_TRANSMISSION_COLOR = 17,
    /// Diffuse transmission amount.
    PONKO_TEXTURE_MAP_DIFFUSE_TRANSMISSION = 18,
    /// Volume scatter color.
    PONKO_TEXTURE_MAP_SCATTER_COLOR = 19,
    /// Volume scatter strength.
    PONKO_TEXTURE_MAP_SCATTER = 20,
}

/// Number of entries in version 5's per-map overrides.
pub const PONKO_TEXTURE_MAP_COUNT: usize = 21;

/// Affine texture coordinates, applied in glTF order (scale, turn, offset).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoTextureTransform {
    /// Final offset in UV units.
    pub offset: [f32; 2],
    /// Rotation in radians, in glTF's convention.
    pub rotation: f32,
    /// Scale along U and V.
    pub scale: [f32; 2],
}

impl PonkoTextureTransform {
    const IDENTITY: Self = Self {
        offset: [0.0; 2],
        rotation: 0.0,
        scale: [1.0; 2],
    };

    fn to_rust(self) -> TextureTransform {
        TextureTransform::IDENTITY
            .with_offset(self.offset.into())
            .with_rotation(self.rotation)
            .with_scale(self.scale.into())
    }
}

/// Per-map overrides for a built-in material shader. Indices are
/// `PonkoTextureMap` values. Up to four distinct transforms are drawn.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoTextureTransformOverrides {
    /// Bit per map that uses its entry in `transforms`, else the shared transform.
    pub transform_mask: u32,
    /// Bit per map that uses the mesh's second texture coordinates.
    pub uv1_mask: u32,
    /// Overrides for the maps selected by `transform_mask`.
    pub transforms: [PonkoTextureTransform; PONKO_TEXTURE_MAP_COUNT],
}

impl PonkoTextureTransformOverrides {
    const NONE: Self = Self {
        transform_mask: 0,
        uv1_mask: 0,
        transforms: [PonkoTextureTransform::IDENTITY; PONKO_TEXTURE_MAP_COUNT],
    };
}

/// What a material is made from, in a layout that can grow (Rust
/// `MaterialDesc`): `PonkoMaterialDesc` plus the fields added since ABI 2.
/// Set `struct_size` and `version`, then fill it with
/// `ponko_material_desc_ex_defaults`:
///
/// ```c
/// PonkoMaterialDescEx d = {.struct_size = sizeof d, .version = PONKO_MATERIAL_DESC_EX_VERSION};
/// ponko_material_desc_ex_defaults(&d);
/// d.desc.color = (PonkoColor){1, 0, 0, 1};
/// ponko_ctx_create_material_ex(ctx, &d, &material);
/// ```
///
/// Version 1: `desc`, `has_stencil`, `stencil`. Version 2 adds
/// `masked_tint`, version 3 `unlit` and `layers`, version 4 `transmission`.
/// Version 5 adds the shared texture transform and per-map overrides.
/// ```
/// use ponko_ffi::*;
/// let mut d = PonkoMaterialDescEx { struct_size: size_of::<PonkoMaterialDescEx>() as u32, version: PONKO_MATERIAL_DESC_EX_VERSION, ..unsafe { std::mem::zeroed() } };
/// assert_eq!(unsafe { ponko_material_desc_ex_defaults(&mut d) }, PonkoStatus::PONKO_OK);
/// assert_eq!((d.desc.roughness, d.has_stencil, d.masked_tint, d.unlit), (0.6, false, false, false));
/// assert_eq!((d.layers.clearcoat, d.layers.ior, d.layers.specular), (0.0, 1.5, 1.0));
/// assert_eq!((d.transmission.factor, d.transmission.attenuation_distance), (0.0, f32::INFINITY));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoMaterialDescEx {
    /// `sizeof(PonkoMaterialDescEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_MATERIAL_DESC_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoMaterialDesc`).
    pub desc: PonkoMaterialDesc,
    /// Whether `stencil` is used (as `ponko_ctx_create_material_with_stencil`).
    pub has_stencil: bool,
    /// The stencil state, with `has_stencil`.
    pub stencil: PonkoStencil,
    /// The model's colour tints only where the vertex colour's alpha is 1
    /// (0: not at all); that alpha is then a mask, not opacity (Rust
    /// `MaterialDesc::masked_tint`). Version 2.
    pub masked_tint: bool,
    /// Version 3: drawn without lighting (`KHR_materials_unlit`).
    pub unlit: bool,
    /// Version 3: layers over the surface (all off by default).
    pub layers: PonkoMaterialLayers,
    /// Version 4: light through the surface and the medium behind it (off by
    /// default).
    pub transmission: PonkoMaterialTransmission,
    /// Version 5: the transform all maps inherit by default (identity).
    pub texture_transform: PonkoTextureTransform,
    /// Version 5: independent transforms and coordinate sets.
    pub texture_transform_overrides: PonkoTextureTransformOverrides,
}
assert_header!(PonkoMaterialDescEx);

impl Versioned for PonkoMaterialDescEx {
    const NAME: &'static str = "PonkoMaterialDescEx";
    const VERSION: u32 = PONKO_MATERIAL_DESC_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, stencil: PonkoStencil)),
            2 => Some(end_of!(Self, masked_tint: bool)),
            3 => Some(end_of!(Self, layers: PonkoMaterialLayers)),
            4 => Some(end_of!(Self, transmission: PonkoMaterialTransmission)),
            5 => Some(end_of!(Self, texture_transform_overrides: PonkoTextureTransformOverrides)),
            _ => None,
        }
    }
}

fn material_defaults() -> PonkoMaterialDescEx {
    PonkoMaterialDescEx {
        struct_size: size_of::<PonkoMaterialDescEx>() as u32,
        version: PONKO_MATERIAL_DESC_EX_VERSION,
        desc: ponko_material_desc(PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }),
        has_stencil: false,
        stencil: ponko_stencil_default(),
        masked_tint: false,
        unlit: false,
        layers: layers_to_c(&MaterialLayers::NONE),
        transmission: transmission_to_c(&ponko::render::Transmission::NONE),
        texture_transform: PonkoTextureTransform::IDENTITY,
        texture_transform_overrides: PonkoTextureTransformOverrides::NONE,
    }
}

/// Fills the fields of `desc`'s version with the defaults: a plain white
/// material (as `ponko_material_desc` of white), no stencil, the model's
/// colour over all of it. `struct_size`
/// and `version` must be set first.
/// # Safety
/// `desc` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_material_desc_ex_defaults(desc: *mut PonkoMaterialDescEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(desc, &material_defaults()) })
}

/// The Rust material a versioned C one describes, checked.
fn material_ex(c: &ponko::prelude::Ctx, d: &PonkoMaterialDescEx) -> Result<ponko::render::MaterialDesc, Failure> {
    let stencil = if d.has_stencil {
        Some(stencil_from_c(&d.stencil)?)
    } else {
        None
    };
    let mut desc = material_from_c(c, &d.desc)?
        .with_stencil(stencil)
        .with_masked_tint(d.masked_tint)
        .with_unlit(d.unlit)
        .with_layers(layers_from_c(&d.layers).with_transmission(transmission_from_c(&d.transmission)))
        .with_texture_transform(d.texture_transform.to_rust());
    let overrides = &d.texture_transform_overrides;
    if (overrides.transform_mask | overrides.uv1_mask) >> PONKO_TEXTURE_MAP_COUNT != 0 {
        return Err(invalid("texture transform overrides contain an unknown map bit"));
    }
    for map in TextureMap::ALL {
        let i = map as usize;
        let mask = 1 << i;
        if overrides.transform_mask & mask != 0 {
            desc.texture_transform_overrides
                .set(map, Some(overrides.transforms[i].to_rust()));
        }
        desc.texture_transform_overrides
            .set_uv1(map, overrides.uv1_mask & mask != 0);
    }
    Ok(desc)
}

/// Puts a material on the GPU, from a versioned description (as
/// `ponko_ctx_create_material`, plus a stencil state with `has_stencil`,
/// from version 2 `masked_tint`, from version 3 `unlit` and `layers`, from
/// version 4 `transmission`, and from version 5 texture transforms). Layers out of range, with a
/// custom shader or reading more than two images are refused with
/// `PONKO_INVALID_ARGUMENT`.
/// # Safety
/// `ctx` null or live; `desc` null or a readable descriptor; output writable.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_create_material_ex(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_material_ex(
    ctx: *mut PonkoCtx,
    desc: *const PonkoMaterialDescEx,
    out_material: *mut PonkoMaterial,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let d = read_desc(desc, material_defaults())?;
            if out_material.is_null() {
                return Err(invalid("output is null"));
            }
            let desc = material_ex(c, &d)?;
            let material = c.gfx.try_create_material(desc).map_err(crate::engine_error)?;
            out(out_material, to_c(material))
        })
    })
}

/// Changes a material from a versioned description (as
/// `ponko_ctx_update_material`; without `has_stencil` its stencil comes off).
/// # Safety
/// `ctx` null or live; `desc` null or a readable descriptor.
/// ```
/// let m = ponko_ffi::PonkoMaterial { id: 0 };
/// let s = unsafe { ponko_ffi::ponko_ctx_update_material_ex(std::ptr::null_mut(), m, std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_update_material_ex(
    ctx: *mut PonkoCtx,
    material: PonkoMaterial,
    desc: *const PonkoMaterialDescEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let d = read_desc(desc, material_defaults())?;
            let material: ponko::render::Material = to_c(material);
            if !c.gfx.is_live(material) {
                return Err(invalid("that material wasn't made by this game"));
            }
            let desc = material_ex(c, &d)?;
            c.gfx.try_update_material(material, desc).map_err(crate::engine_error)
        })
    })
}

// ---- navmesh bake -----------------------------------------------------------

/// The version of `PonkoNavBakeSettingsEx` this header describes.
pub const PONKO_NAV_BAKE_SETTINGS_EX_VERSION: u32 = 1;

/// How a navmesh is baked, in a layout that can grow (Rust
/// `NavBakeSettings`): `PonkoNavBakeSettings` plus the fields added since
/// ABI 2 (none yet). Set `struct_size` and `version`, then
/// `ponko_nav_bake_settings_ex_defaults`.
///
/// Version 1: `settings`.
/// ```
/// use ponko_ffi::*;
/// let mut s = PonkoNavBakeSettingsEx { struct_size: size_of::<PonkoNavBakeSettingsEx>() as u32, version: 1, settings: ponko_nav_bake_settings_default() };
/// s.settings.agent_height = 0.0;
/// assert_eq!(unsafe { ponko_nav_bake_settings_ex_defaults(&mut s) }, PonkoStatus::PONKO_OK);
/// assert_eq!(s.settings.agent_height, 1.8);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoNavBakeSettingsEx {
    /// `sizeof(PonkoNavBakeSettingsEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_NAV_BAKE_SETTINGS_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoNavBakeSettings`).
    pub settings: PonkoNavBakeSettings,
}
assert_header!(PonkoNavBakeSettingsEx);

impl Versioned for PonkoNavBakeSettingsEx {
    const NAME: &'static str = "PonkoNavBakeSettingsEx";
    const VERSION: u32 = PONKO_NAV_BAKE_SETTINGS_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, settings: PonkoNavBakeSettings)),
            _ => None,
        }
    }
}

fn nav_defaults() -> PonkoNavBakeSettingsEx {
    PonkoNavBakeSettingsEx {
        struct_size: size_of::<PonkoNavBakeSettingsEx>() as u32,
        version: PONKO_NAV_BAKE_SETTINGS_EX_VERSION,
        settings: ponko_nav_bake_settings_default(),
    }
}

/// Fills the fields of `settings`' version with the default bake (as
/// `ponko_nav_bake_settings_default`). `struct_size` and `version` must be
/// set first.
/// # Safety
/// `settings` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_bake_settings_ex_defaults(settings: *mut PonkoNavBakeSettingsEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(settings, &nav_defaults()) })
}

/// Bakes `sources` into this context's navmesh with versioned settings (as
/// `ponko_ctx_bake_navmesh`; `settings` null: the defaults).
/// # Safety
/// `ctx` null or live; `sources` null or live; `settings` null or a readable
/// descriptor.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_bake_navmesh_ex(std::ptr::null_mut(), std::ptr::null(), std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_bake_navmesh_ex(
    ctx: *mut PonkoCtx,
    sources: *const PonkoNavSources,
    settings: *const PonkoNavBakeSettingsEx,
) -> PonkoStatus {
    let mut s = nav_defaults();
    if !settings.is_null() {
        // SAFETY: forwarded caller contract.
        let status = boundary(|| unsafe {
            s = read_desc(settings, nav_defaults())?;
            Ok(())
        });
        if status != PonkoStatus::PONKO_OK {
            return status;
        }
    }
    // SAFETY: forwarded caller contract; `s.settings` is a local.
    unsafe { ponko_ctx_bake_navmesh(ctx, sources, &s.settings) }
}

// ---- lighting ---------------------------------------------------------------

/// The version of `PonkoLightingEx` this header describes.
pub const PONKO_LIGHTING_EX_VERSION: u32 = 1;

/// The sun and the sky's light, in a layout that can grow (Rust
/// `ctx.scene3d.lighting`): `PonkoLighting` plus the fields added since
/// ABI 2. Read it with `ponko_ctx_lighting_ex` (set `struct_size` and
/// `version` first), change it, and give it back with
/// `ponko_ctx_set_lighting_ex`.
///
/// Version 1: `lighting`, `shadow_sun_size`, `shadow_sun_step`.
/// ```
/// use ponko_ffi::*;
/// let mut l: PonkoLightingEx = unsafe { std::mem::zeroed() };
/// (l.struct_size, l.version) = (size_of::<PonkoLightingEx>() as u32, PONKO_LIGHTING_EX_VERSION);
/// assert_eq!(unsafe { ponko_lighting_ex_defaults(&mut l) }, PonkoStatus::PONKO_OK);
/// assert!((l.shadow_sun_size - 0.53).abs() < 1e-6);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoLightingEx {
    /// `sizeof(PonkoLightingEx)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_LIGHTING_EX_VERSION` as the caller compiled it.
    pub version: u32,
    /// Everything ABI 2 has (`PonkoLighting`).
    pub lighting: PonkoLighting,
    /// How large the sun is for its shadows' soft edges, in degrees across,
    /// 0 to 10: with the contact-hardening filter (`shadow_filter` 4) a
    /// shadow's edge widens by about `2 * tan(shadow_sun_size / 2)` per unit
    /// of distance from its caster; the other filters ignore it. Default
    /// 0.53, the real sun. Apart from `PonkoSky::sun_size`, the disc drawn
    /// in the sky.
    pub shadow_sun_size: f32,
    /// Holds the sun's direction for its cached shadows, which then change
    /// in steps of this many radians instead of shimmering every frame
    /// (0: off, the default; 0.01 to 0.03 suits a day cycle). Lighting and
    /// the sky still use the exact direction.
    pub shadow_sun_step: f32,
}
assert_header!(PonkoLightingEx);

impl Versioned for PonkoLightingEx {
    const NAME: &'static str = "PonkoLightingEx";
    const VERSION: u32 = PONKO_LIGHTING_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, shadow_sun_step: f32)),
            _ => None,
        }
    }
}

fn lighting_ex_of(l: &Lighting, sky: &Sky) -> PonkoLightingEx {
    PonkoLightingEx {
        struct_size: size_of::<PonkoLightingEx>() as u32,
        version: PONKO_LIGHTING_EX_VERSION,
        lighting: lighting_to_c(l, sky),
        shadow_sun_size: l.shadow_sun_size,
        shadow_sun_step: l.shadow_sun_step,
    }
}

/// Fills the fields of `lighting`'s version with the engine's default
/// lighting (what a new game starts with). `struct_size` and `version` must
/// be set first.
/// # Safety
/// `lighting` null or a writable descriptor with its header set.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_lighting_ex_defaults(lighting: *mut PonkoLightingEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(lighting, &lighting_ex_of(&Lighting::default(), &Sky::default())) })
}

/// Reads the sun and sky light into the fields of `out_lighting`'s version
/// (set its `struct_size` and `version` first).
/// # Safety
/// `ctx` null or live; `out_lighting` null or a writable descriptor with its
/// header set.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_lighting_ex(std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_lighting_ex(ctx: *mut PonkoCtx, out_lighting: *mut PonkoLightingEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            write_desc(out_lighting, &lighting_ex_of(&c.scene3d.lighting, &c.scene3d.sky))
        })
    })
}

/// Changes the sun and sky light (the checks of `ponko_ctx_set_lighting`,
/// plus `shadow_sun_size` 0 to 10 and `shadow_sun_step` 0 or more). Fields
/// newer than the caller's version keep their current values. Nothing
/// changes when it fails.
/// # Safety
/// `ctx` null or live; `lighting` null or a readable descriptor.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_lighting_ex(std::ptr::null_mut(), std::ptr::null()) };
/// assert_eq!(s, ponko_ffi::PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_lighting_ex(
    ctx: *mut PonkoCtx,
    lighting: *const PonkoLightingEx,
) -> PonkoStatus {
    let mut l = None;
    // SAFETY: forwarded caller contract.
    let status = boundary(|| unsafe {
        ctx_access(ctx, |c| {
            // Fields past the caller's version keep the current values.
            let current = lighting_ex_of(&c.scene3d.lighting, &c.scene3d.sky);
            let read = read_desc(lighting, current)?;
            if !(0.0..=10.0).contains(&read.shadow_sun_size) {
                return Err(invalid("shadow_sun_size must be 0 to 10 degrees"));
            }
            if !(read.shadow_sun_step.is_finite() && read.shadow_sun_step >= 0.0) {
                return Err(invalid("shadow_sun_step must be 0 or more"));
            }
            l = Some(read);
            Ok(())
        })
    });
    let Some(l) = l.filter(|_| status == PonkoStatus::PONKO_OK) else {
        return status;
    };
    // SAFETY: forwarded caller contract; `l.lighting` is a local.
    let status = unsafe { ponko_ctx_set_lighting(ctx, &l.lighting) };
    if status != PonkoStatus::PONKO_OK {
        return status;
    }
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.scene3d.lighting.shadow_sun_size = l.shadow_sun_size;
            c.scene3d.lighting.shadow_sun_step = l.shadow_sun_step;
            Ok(())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_map_indices_agree_with_the_rust_descriptor() {
        assert_eq!(PONKO_TEXTURE_MAP_COUNT, TextureMap::ALL.len());
        for (native, rust) in [
            PonkoTextureMap::PONKO_TEXTURE_MAP_BASE_COLOR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_EMISSIVE,
            PonkoTextureMap::PONKO_TEXTURE_MAP_NORMAL,
            PonkoTextureMap::PONKO_TEXTURE_MAP_METALLIC_ROUGHNESS,
            PonkoTextureMap::PONKO_TEXTURE_MAP_OCCLUSION,
            PonkoTextureMap::PONKO_TEXTURE_MAP_CLEARCOAT,
            PonkoTextureMap::PONKO_TEXTURE_MAP_CLEARCOAT_ROUGHNESS,
            PonkoTextureMap::PONKO_TEXTURE_MAP_CLEARCOAT_NORMAL,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SHEEN_COLOR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SHEEN_ROUGHNESS,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SPECULAR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SPECULAR_COLOR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_ANISOTROPY,
            PonkoTextureMap::PONKO_TEXTURE_MAP_IRIDESCENCE,
            PonkoTextureMap::PONKO_TEXTURE_MAP_IRIDESCENCE_THICKNESS,
            PonkoTextureMap::PONKO_TEXTURE_MAP_TRANSMISSION,
            PonkoTextureMap::PONKO_TEXTURE_MAP_THICKNESS,
            PonkoTextureMap::PONKO_TEXTURE_MAP_DIFFUSE_TRANSMISSION_COLOR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_DIFFUSE_TRANSMISSION,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SCATTER_COLOR,
            PonkoTextureMap::PONKO_TEXTURE_MAP_SCATTER,
        ]
        .into_iter()
        .zip(TextureMap::ALL)
        {
            assert_eq!(native as usize, rust as usize);
        }
    }

    #[test]
    fn version_one_covers_every_field_and_no_tail_padding() {
        // A version's length ends at its last field: the header plus fields.
        assert_eq!(
            PonkoLightingEx::len_of(1),
            Some(offset_of!(PonkoLightingEx, shadow_sun_step) + 4)
        );
        assert!(PonkoMaterialDescEx::len_of(1).unwrap() <= size_of::<PonkoMaterialDescEx>());
        assert_eq!(
            PonkoMaterialDescEx::len_of(2),
            Some(offset_of!(PonkoMaterialDescEx, masked_tint) + 1)
        );
        assert_eq!(
            PonkoMaterialDescEx::len_of(3),
            Some(offset_of!(PonkoMaterialDescEx, layers) + size_of::<PonkoMaterialLayers>())
        );
        assert_eq!(
            PonkoMaterialDescEx::len_of(4),
            Some(offset_of!(PonkoMaterialDescEx, transmission) + size_of::<PonkoMaterialTransmission>())
        );
        assert_eq!(
            PonkoMaterialDescEx::len_of(5),
            Some(
                offset_of!(PonkoMaterialDescEx, texture_transform_overrides)
                    + size_of::<PonkoTextureTransformOverrides>()
            )
        );
        assert_eq!(PonkoMaterialDescEx::len_of(6), None);
        assert!(PonkoNavBakeSettingsEx::len_of(1).unwrap() <= size_of::<PonkoNavBakeSettingsEx>());
        assert_eq!(PonkoLightingEx::len_of(2), None);
    }
}
