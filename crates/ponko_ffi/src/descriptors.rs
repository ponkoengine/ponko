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

use ponko::render::{Lighting, Sky};

use crate::ctx::{ctx_access, out};
use crate::nav3d::{PonkoNavBakeSettings, PonkoNavSources, ponko_ctx_bake_navmesh, ponko_nav_bake_settings_default};
use crate::stencil::{PonkoStencil, ponko_stencil_default, stencil_from_c};
use crate::three::{
    PonkoLighting, PonkoMaterial, PonkoMaterialDesc, lighting_to_c, material_from_c, ponko_ctx_set_lighting,
    ponko_material_desc,
};
use crate::types::{PonkoColor, to_c};
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
pub const PONKO_MATERIAL_DESC_EX_VERSION: u32 = 1;

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
/// Version 1: `desc`, `has_stencil`, `stencil`.
/// ```
/// use ponko_ffi::*;
/// let mut d = PonkoMaterialDescEx { struct_size: size_of::<PonkoMaterialDescEx>() as u32, version: PONKO_MATERIAL_DESC_EX_VERSION, ..unsafe { std::mem::zeroed() } };
/// assert_eq!(unsafe { ponko_material_desc_ex_defaults(&mut d) }, PonkoStatus::PONKO_OK);
/// assert_eq!((d.desc.roughness, d.has_stencil), (0.6, false));
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
}
assert_header!(PonkoMaterialDescEx);

impl Versioned for PonkoMaterialDescEx {
    const NAME: &'static str = "PonkoMaterialDescEx";
    const VERSION: u32 = PONKO_MATERIAL_DESC_EX_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(end_of!(Self, stencil: PonkoStencil)),
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
    }
}

/// Fills the fields of `desc`'s version with the defaults: a plain white
/// material (as `ponko_material_desc` of white), no stencil. `struct_size`
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
    Ok(material_from_c(c, &d.desc)?.with_stencil(stencil))
}

/// Puts a material on the GPU, from a versioned description (as
/// `ponko_ctx_create_material`, plus a stencil state with `has_stencil`).
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
            out(out_material, to_c(c.gfx.create_material(desc)))
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
            c.gfx.update_material(material, desc);
            Ok(())
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
    fn version_one_covers_every_field_and_no_tail_padding() {
        // A version's length ends at its last field: the header plus fields.
        assert_eq!(
            PonkoLightingEx::len_of(1),
            Some(offset_of!(PonkoLightingEx, shadow_sun_step) + 4)
        );
        assert!(PonkoMaterialDescEx::len_of(1).unwrap() <= size_of::<PonkoMaterialDescEx>());
        assert!(PonkoNavBakeSettingsEx::len_of(1).unwrap() <= size_of::<PonkoNavBakeSettingsEx>());
        assert_eq!(PonkoLightingEx::len_of(2), None);
    }
}
