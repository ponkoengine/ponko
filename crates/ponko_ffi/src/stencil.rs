//! Stencil state on materials from C (Rust `Stencil`, `StencilFace`,
//! `StencilCompare`, `StencilOp`): start from a `ponko_stencil_*`
//! constructor, then make or change a material with
//! `ponko_ctx_create_material_with_stencil` or
//! `ponko_ctx_update_material_with_stencil`.

use ponko::render::{Material, Stencil, StencilCompare, StencilFace, StencilOp};

use crate::ctx::{ctx_access, out};
use crate::three::{PonkoMaterial, PonkoMaterialDesc, material_from_c};
use crate::types::to_c;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid};

/// How a stencil face compares the reference with the stored value
/// (`PonkoStencilFace::compare`, Rust `StencilCompare`).
/// ```
/// assert_eq!(ponko_ffi::PonkoStencilCompare::PONKO_STENCIL_ALWAYS as u32, 7);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoStencilCompare {
    /// Never passes.
    PONKO_STENCIL_NEVER,
    /// Passes where the reference is less than the stored value.
    PONKO_STENCIL_LESS,
    /// Passes where they are equal.
    PONKO_STENCIL_EQUAL,
    /// Passes where the reference is less than or equal to the stored value.
    PONKO_STENCIL_LESS_EQUAL,
    /// Passes where the reference is greater than the stored value.
    PONKO_STENCIL_GREATER,
    /// Passes where they differ.
    PONKO_STENCIL_NOT_EQUAL,
    /// Passes where the reference is greater than or equal to the stored value.
    PONKO_STENCIL_GREATER_EQUAL,
    /// Always passes: no test.
    PONKO_STENCIL_ALWAYS,
}

/// What a draw does to the stored stencil value (`PonkoStencilFace::fail`,
/// `depth_fail` and `pass`, Rust `StencilOp`).
/// ```
/// assert_eq!(ponko_ffi::PonkoStencilOp::PONKO_STENCIL_REPLACE as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoStencilOp {
    /// Leaves it.
    PONKO_STENCIL_KEEP,
    /// Sets it to 0.
    PONKO_STENCIL_ZERO,
    /// Sets it to the reference.
    PONKO_STENCIL_REPLACE,
    /// Flips its bits.
    PONKO_STENCIL_INVERT,
    /// Adds 1, stopping at 255.
    PONKO_STENCIL_INCREMENT_CLAMP,
    /// Takes 1 away, stopping at 0.
    PONKO_STENCIL_DECREMENT_CLAMP,
    /// Adds 1, from 255 back to 0.
    PONKO_STENCIL_INCREMENT_WRAP,
    /// Takes 1 away, from 0 back to 255.
    PONKO_STENCIL_DECREMENT_WRAP,
}

/// The stencil test and its operations for one side of the triangles
/// (Rust `StencilFace`). Each field holds a `PonkoStencilCompare` or
/// `PonkoStencilOp` value.
/// ```
/// let s = ponko_ffi::ponko_stencil_mark(1);
/// assert_eq!(s.front.pass, ponko_ffi::PonkoStencilOp::PONKO_STENCIL_REPLACE as u32);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoStencilFace {
    /// A `PonkoStencilCompare`.
    pub compare: u32,
    /// A `PonkoStencilOp`: what a pixel failing the stencil test does.
    pub fail: u32,
    /// A `PonkoStencilOp`: what a pixel passing the stencil test but failing the depth test does.
    pub depth_fail: u32,
    /// A `PonkoStencilOp`: what a pixel passing both does.
    pub pass: u32,
}

/// A material's stencil state (Rust `Stencil`): see the guide's "Stencil"
/// section for when the test runs and the order stencil materials keep.
/// Start from a `ponko_stencil_*` constructor.
/// ```
/// let s = ponko_ffi::ponko_stencil_default();
/// assert_eq!((s.read_mask, s.write_mask, s.order), (0xFF, 0xFF, 0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoStencil {
    /// The front faces' test and operations.
    pub front: PonkoStencilFace,
    /// The back faces' (double-sided materials).
    pub back: PonkoStencilFace,
    /// Where solid stencil materials draw among each other: lower first.
    pub order: i32,
    /// The value tested against, and written by `PONKO_STENCIL_REPLACE`.
    pub reference: u8,
    /// The stored bits the test reads.
    pub read_mask: u8,
    /// The stored bits the operations change.
    pub write_mask: u8,
}

fn compare_to_c(c: StencilCompare) -> u32 {
    c as u32
}

fn compare_from_c(c: u32) -> Result<StencilCompare, Failure> {
    use StencilCompare::*;
    Ok(match c {
        0 => Never,
        1 => Less,
        2 => Equal,
        3 => LessEqual,
        4 => Greater,
        5 => NotEqual,
        6 => GreaterEqual,
        7 => Always,
        _ => return Err(invalid("a stencil compare goes from 0 to 7 (PonkoStencilCompare)")),
    })
}

fn op_from_c(o: u32) -> Result<StencilOp, Failure> {
    use StencilOp::*;
    Ok(match o {
        0 => Keep,
        1 => Zero,
        2 => Replace,
        3 => Invert,
        4 => IncrementClamp,
        5 => DecrementClamp,
        6 => IncrementWrap,
        7 => DecrementWrap,
        _ => return Err(invalid("a stencil operation goes from 0 to 7 (PonkoStencilOp)")),
    })
}

fn face_to_c(f: &StencilFace) -> PonkoStencilFace {
    PonkoStencilFace {
        compare: compare_to_c(f.compare),
        fail: f.fail as u32,
        depth_fail: f.depth_fail as u32,
        pass: f.pass as u32,
    }
}

fn face_from_c(f: &PonkoStencilFace) -> Result<StencilFace, Failure> {
    Ok(StencilFace {
        compare: compare_from_c(f.compare)?,
        fail: op_from_c(f.fail)?,
        depth_fail: op_from_c(f.depth_fail)?,
        pass: op_from_c(f.pass)?,
    })
}

fn stencil_to_c(s: &Stencil) -> PonkoStencil {
    PonkoStencil {
        front: face_to_c(&s.front),
        back: face_to_c(&s.back),
        order: s.order,
        reference: s.reference,
        read_mask: s.read_mask,
        write_mask: s.write_mask,
    }
}

/// A C stencil as Rust's, checked.
pub(crate) fn stencil_from_c(s: &PonkoStencil) -> Result<Stencil, Failure> {
    Ok(Stencil::default()
        .with_front(face_from_c(&s.front)?)
        .with_back(face_from_c(&s.back)?)
        .with_order(s.order)
        .with_reference(s.reference)
        .with_read_mask(s.read_mask)
        .with_write_mask(s.write_mask))
}

/// A stencil that tests nothing and writes nothing (Rust `Stencil::default`).
/// ```
/// assert_eq!(ponko_ffi::ponko_stencil_default().reference, 0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stencil_default() -> PonkoStencil {
    stencil_to_c(&Stencil::default())
}

/// Draws everywhere and writes `reference` where it drew (Rust `Stencil::mark`).
/// ```
/// assert_eq!(ponko_ffi::ponko_stencil_mark(3).reference, 3);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stencil_mark(reference: u8) -> PonkoStencil {
    stencil_to_c(&Stencil::mark(reference))
}

/// Draws only where the stencil holds `reference` (Rust `Stencil::inside`).
/// ```
/// let s = ponko_ffi::ponko_stencil_inside(1);
/// assert_eq!(s.front.compare, ponko_ffi::PonkoStencilCompare::PONKO_STENCIL_EQUAL as u32);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stencil_inside(reference: u8) -> PonkoStencil {
    stencil_to_c(&Stencil::inside(reference))
}

/// Draws only where the stencil doesn't hold `reference` (Rust `Stencil::outside`).
/// ```
/// let s = ponko_ffi::ponko_stencil_outside(1);
/// assert_eq!(s.back.compare, ponko_ffi::PonkoStencilCompare::PONKO_STENCIL_NOT_EQUAL as u32);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stencil_outside(reference: u8) -> PonkoStencil {
    stencil_to_c(&Stencil::outside(reference))
}

/// Draws each pixel at most once, writing `reference` there (Rust `Stencil::once`).
/// ```
/// let s = ponko_ffi::ponko_stencil_once(1);
/// assert_eq!(s.front.pass, ponko_ffi::PonkoStencilOp::PONKO_STENCIL_REPLACE as u32);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_stencil_once(reference: u8) -> PonkoStencil {
    stencil_to_c(&Stencil::once(reference))
}

/// `ponko_ctx_create_material` with a stencil state (`stencil` null: none).
/// # Safety
/// `ctx` null or live; pointers valid or null.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_create_material_with_stencil(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_material_with_stencil(
    ctx: *mut PonkoCtx,
    desc: *const PonkoMaterialDesc,
    stencil: *const PonkoStencil,
    out_material: *mut PonkoMaterial,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(d) = desc.as_ref() else {
                return Err(invalid("material desc is null"));
            };
            if out_material.is_null() {
                return Err(invalid("output is null"));
            }
            let stencil = stencil.as_ref().map(stencil_from_c).transpose()?;
            let desc = material_from_c(c, d)?.with_stencil(stencil);
            out(out_material, to_c(c.gfx.create_material(desc)))
        })
    })
}

/// `ponko_ctx_update_material` with a stencil state (`stencil` null: none).
/// `ponko_ctx_update_material` takes a material's stencil off.
/// # Safety
/// `ctx` null or live; `desc` valid; `stencil` valid or null.
/// ```
/// let m = ponko_ffi::PonkoMaterial { id: 0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_update_material_with_stencil(std::ptr::null_mut(), m, std::ptr::null(), std::ptr::null()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_update_material_with_stencil(
    ctx: *mut PonkoCtx,
    material: PonkoMaterial,
    desc: *const PonkoMaterialDesc,
    stencil: *const PonkoStencil,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(d) = desc.as_ref() else {
                return Err(invalid("material desc is null"));
            };
            let material: Material = to_c(material);
            if !c.gfx.is_live(material) {
                return Err(invalid("that material wasn't made by this game"));
            }
            let stencil = stencil.as_ref().map(stencil_from_c).transpose()?;
            let desc = material_from_c(c, d)?.with_stencil(stencil);
            c.gfx.update_material(material, desc);
            Ok(())
        })
    })
}
