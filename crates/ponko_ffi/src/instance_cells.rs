//! Instance cells (Rust `InstanceCell`): the instances of one spatial cell
//! of an instance batch, added, replaced and freed on their own, without
//! uploading the batch's other instances again. A native game builds a
//! cell's matrices on its own threads and adds the cell from the game's.
use crate::ctx::{ctx_access, out};
use crate::three::{PonkoInstanceBatch, instance_arrays};
use crate::types::PonkoColor;
use crate::{PonkoCtx, PonkoStatus, boundary, invalid};
use ponko::__ffi;
use ponko::render::{InstanceBatch, InstanceCell};

/// One cell of an instance batch (Rust `InstanceCell`). A handle; free it
/// with `ponko_ctx_free_instance_cell`. Freeing its batch frees it too.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PonkoInstanceCell {
    /// Opaque cell identifier.
    pub id: u32,
}

/// Adds a cell of `count` column-major affine matrices to `batch` and
/// uploads it now (Rust `Graphics::add_instance_cell`). Zero colours use the
/// batch's tint; otherwise `color_count` equals `count` (sRGB). A freed
/// batch, mismatched counts or a non-affine matrix is an error.
/// ```
/// let mut cell = ponko_ffi::PonkoInstanceCell::default();
/// let status = unsafe {
///     ponko_ffi::ponko_ctx_add_instance_cell(
///         std::ptr::null_mut(), Default::default(), std::ptr::null(), 0, std::ptr::null(), 0, &mut cell)
/// };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// Context is live or null; nonempty arrays have their stated lengths;
/// output is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_add_instance_cell(
    ctx: *mut PonkoCtx,
    batch: PonkoInstanceBatch,
    matrices: *const [f32; 16],
    count: usize,
    colors: *const PonkoColor,
    color_count: usize,
    output: *mut PonkoInstanceCell,
) -> PonkoStatus {
    // SAFETY: forwarded array/context/output contracts; validation precedes slicing.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if output.is_null() {
                return Err(invalid("null instance cell output"));
            }
            let batch = __ffi::from_id::<InstanceBatch>(batch.id);
            if !c.gfx.is_live(batch) {
                return Err(invalid("invalid instance batch"));
            }
            let (transforms, colors) = instance_arrays(matrices, count, colors, color_count)?;
            let cell = c.gfx.add_instance_cell(batch, &transforms, &colors);
            out(output, PonkoInstanceCell { id: __ffi::id(cell) })
        })
    })
}

/// Replaces a cell's instances, as `ponko_ctx_add_instance_cell` takes
/// them, and uploads them now; the batch's other cells stay as they are
/// (Rust `Graphics::replace_instance_cell`). A freed cell is an error.
/// # Safety
/// Context is live or null; nonempty arrays have their stated lengths.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_replace_instance_cell(
    ctx: *mut PonkoCtx,
    cell: PonkoInstanceCell,
    matrices: *const [f32; 16],
    count: usize,
    colors: *const PonkoColor,
    color_count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded array/context contracts; validation precedes slicing.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let (transforms, colors) = instance_arrays(matrices, count, colors, color_count)?;
            if !c
                .gfx
                .replace_instance_cell(__ffi::from_id::<InstanceCell>(cell.id), &transforms, &colors)
            {
                return Err(invalid("invalid instance cell"));
            }
            Ok(())
        })
    })
}

/// How many instances a cell draws, in `*output` (Rust
/// `Graphics::instance_cell_len`). A freed cell is an error.
/// # Safety
/// Context is live or null; output is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_instance_cell_len(
    ctx: *mut PonkoCtx,
    cell: PonkoInstanceCell,
    output: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded context/output contracts.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let Some(len) = c.gfx.instance_cell_len(__ffi::from_id::<InstanceCell>(cell.id)) else {
                return Err(invalid("invalid instance cell"));
            };
            out(output, len)
        })
    })
}

/// Frees a cell: only its instances stop drawing, and its GPU memory goes
/// to later cells. Freeing it again does nothing.
/// # Safety
/// Context is live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_free_instance_cell(ctx: *mut PonkoCtx, cell: PonkoInstanceCell) -> PonkoStatus {
    // SAFETY: forwarded context contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.gfx.free(__ffi::from_id::<InstanceCell>(cell.id));
            Ok(())
        })
    })
}
