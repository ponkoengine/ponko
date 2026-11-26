//! The player's files from C (Rust's `ctx.storage`): saves and settings in
//! the per-user data folder (browser storage on the web), written safely,
//! and save slots for the world.
use std::ffi::c_char;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::game::{PonkoGame, build, ok};
use crate::{Failure, PonkoStatus, boundary, engine_error, invalid, write_text};

/// Names the game for `ctx.storage`: its files go in the folder `app_id`
/// inside the player's data folder (Windows `%APPDATA%`, macOS
/// `~/Library/Application Support`, Linux `$XDG_DATA_HOME` or
/// `~/.local/share`; `PONKO_DATA_DIR` replaces it). Without one the game
/// keeps no files. `app_id`: 1 to 128 letters, digits, `-`, `_` and `.`,
/// not starting with `.`.
/// # Safety
/// `game` null or live; `app_id` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_app_id(std::ptr::null_mut(), c"crabs".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_app_id(game: *mut PonkoGame, app_id: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let app_id = match unsafe { text(app_id) } {
        Ok(id) => id,
        Err(e) => return boundary(|| Err(e)),
    };
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            if ponko::storage::is_app_id(app_id) {
                ok(g.app_id(app_id))
            } else {
                (
                    g,
                    Err(invalid(
                        "app id must be 1 to 128 letters, digits, '-', '_' or '.', not starting with '.'",
                    )),
                )
            }
        })
    }
}

/// Keeps `ctx.storage`'s files in the folder `dir` itself (tests, portable
/// installs), instead of the app id's folder in the player's data folder.
/// # Safety
/// `game` null or live; `dir` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_data_dir(std::ptr::null_mut(), c"saves".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_data_dir(game: *mut PonkoGame, dir: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let dir = match unsafe { text(dir) } {
        Ok(dir) => dir.to_string(),
        Err(e) => return boundary(|| Err(e)),
    };
    // SAFETY: forwarded caller contract.
    unsafe { build(game, |g| ok(g.data_dir(dir))) }
}

/// Writes `len` bytes as the stored file `name`, safely: the old file stays
/// whole until the new one is complete (temporary file, then rename), and is
/// kept as its backup. Fails (kind `PONKO_ERROR_UNSUPPORTED`) when the game
/// has no app id.
/// # Safety
/// Live ctx; `name` NUL-terminated; `bytes` readable for `len` bytes (may be
/// null when `len` is 0).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_write(std::ptr::null_mut(), c"a".as_ptr(), std::ptr::null(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_write(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    bytes: *const u8,
    len: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let bytes = slice(bytes, len)?;
        ctx_access(ctx, |c| c.storage.write(text(name)?, bytes).map_err(engine_error))
    })
}

/// Reads the stored file `name` into `buffer`: `*out_len` is always its
/// size; the bytes are copied only when they fit in `capacity` (call with a
/// bigger buffer otherwise). `PONKO_NOT_FOUND` if there is no such file.
/// # Safety
/// Live ctx; `name` NUL-terminated; `buffer` writable for `capacity` bytes
/// (or null with capacity 0); `out_len` writable.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_read(std::ptr::null_mut(), c"a".as_ptr(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_read(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    buffer: *mut u8,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let bytes = ctx_access(ctx, |c| c.storage.read(text(name)?).map_err(engine_error))?;
        copy_out(buffer, capacity, out_len, &bytes)
    })
}

/// `ponko_ctx_storage_read` for the file as it was before its last write
/// (the backup); `PONKO_NOT_FOUND` if it was never written over.
/// # Safety
/// As `ponko_ctx_storage_read`.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_read_backup(std::ptr::null_mut(), c"a".as_ptr(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_read_backup(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    buffer: *mut u8,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let bytes = ctx_access(ctx, |c| c.storage.read_backup(text(name)?).map_err(engine_error))?;
        copy_out(buffer, capacity, out_len, &bytes)
    })
}

/// Sets `*out_exists` to whether the stored file `name` exists.
/// # Safety
/// Live ctx; `name` NUL-terminated; `out_exists` writable.
/// ```
/// let mut e = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_exists(std::ptr::null_mut(), c"a".as_ptr(), &mut e) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_exists(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    out_exists: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let exists = ctx_access(ctx, |c| Ok(c.storage.exists(text(name)?)))?;
        out(out_exists, exists)
    })
}

/// Whether the game can keep files at all (Rust's `ctx.storage.is_available()`),
/// to `out_available`: `false` without an app id or data folder, or on a
/// desktop with no home folder; every other storage call then fails with
/// kind `PONKO_ERROR_UNSUPPORTED`. A game that runs without saving checks
/// this instead of treating that failure as a damaged file.
/// # Safety
/// `ctx` null or live; `out_available` writable.
/// ```
/// let mut a = true;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_is_available(std::ptr::null_mut(), &mut a) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_is_available(ctx: *mut PonkoCtx, out_available: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let available = ctx_access(ctx, |c| Ok(c.storage.is_available()))?;
        out(out_available, available)
    })
}

/// Every stored file's name, sorted, one per line (`\n` between them), as
/// NUL-terminated text: `*out_len` is always the length; the text is copied
/// only when it fits.
/// # Safety
/// Live ctx; `buffer` writable for `capacity` bytes (or null with capacity
/// 0); `out_len` writable.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_list(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_list(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let names = ctx_access(ctx, |c| c.storage.list().map_err(engine_error))?;
        write_text(buffer, capacity, out_len, &names.join("\n"))
    })
}

/// Deletes the stored file `name` and its backup; a missing file is fine.
/// # Safety
/// Live ctx; `name` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_delete(std::ptr::null_mut(), c"a".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_delete(ctx: *mut PonkoCtx, name: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.storage.delete(text(name)?).map_err(engine_error)) })
}

/// The folder the stored files are in, as NUL-terminated text (`*out_len`
/// always its length, copied when it fits). Empty on the web and when the
/// game has no app id.
/// # Safety
/// Live ctx; `buffer` writable for `capacity` bytes (or null with capacity
/// 0); `out_len` writable.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_storage_dir(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_storage_dir(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let dir = ctx_access(ctx, |c| {
            Ok(c.storage
                .dir()
                .map(|d| d.to_string_lossy().into_owned())
                .unwrap_or_default())
        })?;
        write_text(buffer, capacity, out_len, &dir)
    })
}

/// Saves the world's reflected components and resources in the save slot
/// `slot` (the stored file `saves/<slot>.json`), safely.
/// # Safety
/// Live ctx; `slot` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_save_world(std::ptr::null_mut(), c"slot1".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_save_world(ctx: *mut PonkoCtx, slot: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.storage.save_world(&c.world, text(slot)?).map_err(engine_error)
        })
    })
}

/// Replaces the world with the save slot `slot` (as `ponko_ctx_load_scene`
/// does with a scene). `PONKO_NOT_FOUND` if there is no such slot; refused
/// while a lockstep session runs. On an error the world is unchanged.
/// # Safety
/// Live ctx; `slot` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_world(std::ptr::null_mut(), c"slot1".as_ptr()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_world(ctx: *mut PonkoCtx, slot: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| c.load_world(text(slot)?).map_err(engine_error)) })
}

/// `len` bytes at `bytes` (null is fine for none).
unsafe fn slice<'a>(bytes: *const u8, len: usize) -> Result<&'a [u8], Failure> {
    if len == 0 {
        return Ok(&[]);
    }
    if bytes.is_null() {
        return Err(invalid("bytes is null"));
    }
    // SAFETY: caller supplies `len` readable bytes.
    Ok(unsafe { std::slice::from_raw_parts(bytes, len) })
}

/// Writes the size always and the bytes when they fit.
unsafe fn copy_out(buffer: *mut u8, capacity: usize, out_len: *mut usize, bytes: &[u8]) -> Result<(), Failure> {
    if out_len.is_null() || (buffer.is_null() && capacity != 0) {
        return Err(invalid("output is null"));
    }
    // SAFETY: validated pointers; the caller supplies the stated capacity.
    unsafe {
        out_len.write(bytes.len());
        if bytes.len() <= capacity && !bytes.is_empty() {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
        }
    }
    Ok(())
}
