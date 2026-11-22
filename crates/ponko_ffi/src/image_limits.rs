//! How big a picture the image decoders accept (Rust's `ImageLimits`): the
//! game's setting, and one picture decoded under limits of its own.
use std::ffi::c_char;

use ponko::assets::ImageLimits;

use crate::ctx::{ctx_access, out, text};
use crate::game::{build, ok};
use crate::types::{PonkoTexture, texture_to_c};
use crate::{PonkoCtx, PonkoGame, PonkoStatus, boundary, engine_error, invalid};

/// Sets how big a picture the image decoders accept, for the whole process
/// (Rust's `Game::image_limits`): the longest side in pixels and the most
/// bytes a decoded picture may take as RGBA8. Defaults: 16384 and
/// 536870912 (512 MiB). Every decoder reads them (PNG, JPEG and the other
/// image formats, KTX2, DDS, video, tilesets, sprite sheets); a picture past
/// them fails to load with `PONKO_INVALID_ARGUMENT` and
/// `ponko_last_error_kind() == PONKO_ERROR_LIMIT`, and the message names the
/// limits. Lower them for pictures from players or the network; raise them
/// for large source art. A game that never calls this leaves them as they
/// are. A picture wider than the GPU's own texture limit still fails at
/// upload. Both must be above 0.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_image_limits(std::ptr::null_mut(), 4096, 64 << 20) } as u32, 1);
/// ```
/// # Safety
/// `game` must be null or a live game.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_image_limits(game: *mut PonkoGame, max_side: u32, max_bytes: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            if max_side == 0 || max_bytes == 0 {
                return (g, Err(invalid("image limits must be above 0")));
            }
            ok(g.image_limits(ImageLimits::new(max_side, max_bytes)))
        })
    }
}

/// Changes the image limits from now on (Rust's `assets.set_image_limits`),
/// as `ponko_game_image_limits` does before the game starts. For the whole
/// process: they stay until set again. Pictures already loaded stay as they
/// are.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_image_limits(std::ptr::null_mut(), 4096, 64 << 20) } as u32, 1);
/// ```
/// # Safety
/// `ctx` must be null or the live context of the running system.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_image_limits(ctx: *mut PonkoCtx, max_side: u32, max_bytes: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if max_side == 0 || max_bytes == 0 {
                return Err(invalid("image limits must be above 0"));
            }
            c.assets.set_image_limits(ImageLimits::new(max_side, max_bytes));
            Ok(())
        })
    })
}

/// Writes the image limits in force (Rust's `assets.image_limits()`):
/// `*out_max_side` pixels and `*out_max_bytes` bytes of RGBA8.
/// ```
/// let (mut side, mut bytes) = (0u32, 0u64);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_image_limits(std::ptr::null(), &mut side, &mut bytes) } as u32, 1);
/// ```
/// # Safety
/// `ctx` must be null or the live context of the running system; both outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_image_limits(
    ctx: *const PonkoCtx,
    out_max_side: *mut u32,
    out_max_bytes: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_max_side.is_null() || out_max_bytes.is_null() {
                return Err(invalid("output is null"));
            }
            let limits = c.assets.image_limits();
            out(out_max_side, limits.max_side)?;
            out(out_max_bytes, limits.max_bytes)
        })
    })
}

/// Decodes one picture from `bytes` (`len` bytes of a PNG, JPEG, KTX2 or any
/// other format this build reads) into a texture under limits of its own,
/// `max_side` pixels a side and `max_bytes` of RGBA8, and not the game's
/// (Rust's `gfx.try_decode_texture`): for a picture you don't trust, without
/// changing what every other load follows. `name` is for messages. A picture
/// past the limits fails with `PONKO_INVALID_ARGUMENT` and
/// `ponko_last_error_kind() == PONKO_ERROR_LIMIT` before its pixels are
/// allocated; bytes that aren't a picture are `PONKO_ERROR_DECODE`.
/// `*out_texture` is unchanged on failure.
/// ```
/// let mut t = ponko_ffi::PonkoTexture { id: 0, width: 0, height: 0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_try_decode_texture(std::ptr::null_mut(), std::ptr::null(), 0, c"a.png".as_ptr(), 64, 1 << 20, &mut t) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `bytes` null or `len` readable bytes; `name`
/// NUL-terminated; `out_texture` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_try_decode_texture(
    ctx: *mut PonkoCtx,
    bytes: *const u8,
    len: usize,
    name: *const c_char,
    max_side: u32,
    max_bytes: u64,
    out_texture: *mut PonkoTexture,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            if out_texture.is_null() {
                return Err(invalid("output is null"));
            }
            if bytes.is_null() && len != 0 {
                return Err(invalid("bytes is null"));
            }
            let data: &[u8] = if len == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(bytes, len)
            };
            let limits = ImageLimits::new(max_side, max_bytes);
            let texture = c.gfx.try_decode_texture(data, name, limits).map_err(engine_error)?;
            out(out_texture, texture_to_c(texture))
        })
    })
}
