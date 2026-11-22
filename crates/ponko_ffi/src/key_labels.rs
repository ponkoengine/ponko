//! Key labels on the player's keyboard layout (gl#237): the C side of
//! Rust's `Input::key_labels` (`KeyLabels::label`, `KeyLabels::glyph`),
//! `Input::key_labels_changed` and `Input::set_key_labels` with
//! `KeyLabels::from_layout`.
use std::ffi::c_char;

use ponko::input::KeyLabels;

use crate::controllers::{PonkoGlyph, glyph_from_c};
use crate::ctx::{ctx_access, out};
use crate::keys::key_from_c;
use crate::{PonkoCtx, PonkoStatus, boundary, invalid, write_text};

/// One key of a keyboard layout, for `ponko_ctx_set_key_labels`: the
/// character the key types without a modifier.
/// ```
/// let l = ponko_ffi::PonkoKeyLabel { key: ponko_ffi::PonkoKey::PONKO_KEY_W as u32, character: 'z' as u32 };
/// assert_eq!(l.character, 0x7A);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoKeyLabel {
    /// A `PonkoKey` (by its place on a US keyboard).
    pub key: u32,
    /// The Unicode code point it types (`'z'`); letters are labelled in
    /// upper case.
    pub character: u32,
}

/// What `key` (a `PonkoKey`) is labelled on the player's keyboard, as
/// UTF-8 (Rust's `ctx.input.key_labels().label(key)`): `"Z"` for
/// `PONKO_KEY_W` on a French keyboard, `"Space"` or `"Left Shift"` for
/// keys whose name doesn't depend on the layout. `out_len` gets the length
/// without the NUL; the text is written when it fits in `capacity`.
/// ```
/// let mut len = 0;
/// let s = unsafe { ponko_ffi::ponko_ctx_key_label(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut len) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes (or is null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_key_label(
    ctx: *mut PonkoCtx,
    key: u32,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let key = key_from_c(key).ok_or_else(|| invalid("unknown key"))?;
            write_text(buffer, capacity, out_len, c.input.key_labels().label(key))
        })
    })
}

/// A glyph's label on the player's keyboard layout (Rust's
/// `ctx.input.key_labels().glyph(glyph)`): `ponko_glyph_label` gives the US
/// labels, this one `"Z"` for the W key on a French keyboard. Buffer rules
/// as `ponko_ctx_key_label`.
/// ```
/// let g = ponko_ffi::PonkoGlyph::default();
/// let mut len = 0;
/// let s = unsafe { ponko_ffi::ponko_ctx_glyph_label(std::ptr::null_mut(), g, std::ptr::null_mut(), 0, &mut len) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` holds `capacity` bytes (or is null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_glyph_label(
    ctx: *mut PonkoCtx,
    glyph: PonkoGlyph,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let glyph = glyph_from_c(glyph)?;
            write_text(buffer, capacity, out_len, c.input.key_labels().glyph(glyph))
        })
    })
}

/// Whether the key labels changed this frame (the player switched layouts,
/// the window learned a key's label from a press, or the game set them):
/// redraw text prompts (Rust's `ctx.input.key_labels_changed()`).
/// ```
/// let mut changed = false;
/// let s = unsafe { ponko_ffi::ponko_ctx_key_labels_changed(std::ptr::null_mut(), &mut changed) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_changed` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_key_labels_changed(ctx: *mut PonkoCtx, out_changed: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_changed, c.input.key_labels_changed())) })
}

/// Replaces the key labels with a layout (Rust's
/// `ctx.input.set_key_labels(KeyLabels::from_layout(..))`): a US keyboard
/// with `count` keys relabelled by `labels` (null with 0: a US keyboard).
/// For a "keyboard layout" option, or a test. Keys the player presses later
/// still correct them, and a layout switch replaces them. Only the
/// character keys (letters, digits, punctuation) take a label; others and
/// control characters are ignored. An unknown key or a value that is not a
/// Unicode scalar fails and changes nothing.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_set_key_labels(std::ptr::null_mut(), std::ptr::null(), 0) };
/// assert_eq!(s as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `labels` holds `count` readable entries (or is null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_key_labels(
    ctx: *mut PonkoCtx,
    labels: *const PonkoKeyLabel,
    count: usize,
) -> PonkoStatus {
    boundary(|| {
        if labels.is_null() && count != 0 {
            return Err(invalid("labels is null"));
        }
        let entries = if count == 0 {
            &[][..]
        } else {
            // SAFETY: caller supplies `count` readable entries.
            unsafe { std::slice::from_raw_parts(labels, count) }
        };
        let mut layout = KeyLabels::us();
        for entry in entries {
            let key = key_from_c(entry.key).ok_or_else(|| invalid("unknown key"))?;
            let c =
                char::from_u32(entry.character).ok_or_else(|| invalid("character is not a Unicode scalar value"))?;
            layout.set(key, c);
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            ctx_access(ctx, |c| {
                c.input.set_key_labels(layout);
                Ok(())
            })
        }
    })
}
