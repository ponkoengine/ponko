//! Reading GPU buffers and render targets back on the CPU, from C (Rust
//! `Graphics::read_gpu_buffer`, `read_render_target` and `readback`).
use ponko::__ffi;
use ponko::prelude::ErrorKind;
use ponko::render::GpuBuffer;

use crate::ctx::{ctx_access, out};
use crate::three::render_target_from_c;
use crate::{PonkoCtx, PonkoGpuBuffer, PonkoStatus, PonkoTexture, boundary, invalid, invalid_owned};

/// A read of GPU memory back to the CPU that is under way (Rust
/// `Readback`): from `ponko_ctx_read_gpu_buffer` or
/// `ponko_ctx_read_render_target`, answered by `ponko_ctx_readback` on a
/// later frame. A plain id.
/// ```
/// let r = ponko_ffi::PonkoReadback { id: 0 };
/// assert_eq!(r.id, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoReadback {
    /// Engine-internal id.
    pub id: u32,
}

/// Asks for elements `start` to `end` (not included) of `buffer` back on
/// the CPU, as they are once this frame's compute passes have ran (Rust
/// `Graphics::read_gpu_buffer`); read the answer with `ponko_ctx_readback`
/// on a later frame. The values are not deterministic: never feed them to
/// simulation systems, replays, rollback or lockstep. A freed buffer, or an
/// empty range or one past its end, is an invalid argument.
/// ```
/// let b = ponko_ffi::PonkoGpuBuffer { id: 0 };
/// let status = unsafe { ponko_ffi::ponko_ctx_read_gpu_buffer(std::ptr::null_mut(), b, 0, 1, std::ptr::null_mut()) };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_read` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_read_gpu_buffer(
    ctx: *mut PonkoCtx,
    buffer: PonkoGpuBuffer,
    start: u32,
    end: u32,
    out_read: *mut PonkoReadback,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_read.is_null() {
                return Err(invalid("output is null"));
            }
            let b = __ffi::from_id::<GpuBuffer>(buffer.id);
            let len = c.gfx.gpu_buffer_len(b).ok_or_else(|| invalid("invalid GPU buffer"))?;
            if start >= end || end > len {
                return Err(invalid(
                    "invalid GPU buffer read range: needs start < end <= the buffer's length",
                ));
            }
            let read = c.gfx.read_gpu_buffer(b, start..end);
            out(
                out_read,
                PonkoReadback {
                    id: __ffi::readback_id(read),
                },
            )
        })
    })
}

/// Asks for the `width` x `height` texels at (`x`, `y`) (from the top left)
/// of render target `target` back on the CPU (Rust
/// `Graphics::read_render_target`); read the answer with
/// `ponko_ctx_readback` on a later frame, row by row from the top, as
/// linear RGBA. A texture that isn't a live render target, or an empty
/// rectangle or one past its edge, is an invalid argument.
/// ```
/// let t = ponko_ffi::PonkoTexture { id: 0, width: 4, height: 4 };
/// let status = unsafe { ponko_ffi::ponko_ctx_read_render_target(std::ptr::null_mut(), t, 0, 0, 1, 1, std::ptr::null_mut()) };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_read` writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_read_render_target(
    ctx: *mut PonkoCtx,
    target: PonkoTexture,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    out_read: *mut PonkoReadback,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_read.is_null() {
                return Err(invalid("output is null"));
            }
            let target = render_target_from_c(c, target)?;
            let fits = |at: u32, size: u32, room: u32| size > 0 && at.checked_add(size).is_some_and(|e| e <= room);
            if !fits(x, width, target.width()) || !fits(y, height, target.height()) {
                return Err(invalid(
                    "invalid render target read: the rectangle must be at least 1 x 1 and fit in the target",
                ));
            }
            let read = c
                .gfx
                .read_render_target(target, ponko::math::uvec2(x, y), ponko::math::uvec2(width, height));
            out(
                out_read,
                PonkoReadback {
                    id: __ffi::readback_id(read),
                },
            )
        })
    })
}

/// The answer to `read` (Rust `Graphics::readback`). While the GPU is still
/// working on it (always on the frame it was asked for), returns
/// `PONKO_OK` with `*out_ready` false. Once it arrived: `*out_ready` true,
/// `*out_len` the number of values (one per buffer element or texel), and
/// the first `capacity` of them copied into `values`; pass a null `values`
/// and 0 to ask only the length. A read that can't be answered returns
/// `PONKO_UNSUPPORTED` (no GPU runs compute passes here: headless, or a GPU
/// buffer on WebGL2) with `*out_ready` true. The answer stays for the rest
/// of the frame it arrived in; later, `read` is used up and an invalid
/// argument.
/// ```
/// let r = ponko_ffi::PonkoReadback { id: 0 };
/// let (mut len, mut ready) = (0, false);
/// let status = unsafe { ponko_ffi::ponko_ctx_readback(std::ptr::null_mut(), r, std::ptr::null_mut(), 0, &mut len, &mut ready) };
/// assert_eq!(status as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `values` holds `capacity` writable arrays (null
/// allowed for zero); `out_len` and `out_ready` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_readback(
    ctx: *mut PonkoCtx,
    read: PonkoReadback,
    values: *mut [f32; 4],
    capacity: usize,
    out_len: *mut usize,
    out_ready: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_len.is_null() || out_ready.is_null() || (capacity > 0 && values.is_null()) {
                return Err(invalid("invalid readback output"));
            }
            *out_len = 0;
            *out_ready = false;
            match c.gfx.readback(__ffi::readback(read.id)) {
                None => Ok(()),
                Some(Ok(answer)) => {
                    let n = answer.len().min(capacity);
                    if n > 0 {
                        let dst = std::slice::from_raw_parts_mut(values, n);
                        for (d, v) in dst.iter_mut().zip(answer) {
                            *d = v.to_array();
                        }
                    }
                    *out_len = answer.len();
                    *out_ready = true;
                    Ok(())
                }
                Some(Err(e)) => {
                    let unsupported = e.kind() == ErrorKind::Unsupported;
                    *out_ready = unsupported;
                    let mut failure = invalid_owned(e);
                    if unsupported {
                        failure.0 = PonkoStatus::PONKO_UNSUPPORTED;
                    }
                    Err(failure)
                }
            }
        })
    })
}
