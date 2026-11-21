//! Asset sources from C: where `load_*` calls find an asset's bytes besides
//! the `assets/` folder (Rust's `AssetSource`, `MemorySource` and
//! `Assets::add_source`), and `ponko_ctx_load_asset_with`, which hands an
//! asset's bytes to your own decoder (Rust's `Assets::load_with`).
use std::cell::Cell;
use std::ffi::{CString, c_char, c_void};
use std::rc::Rc;

use ponko::assets::{AssetSource, MemorySource};

use crate::ctx::{ctx_access, text};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, invalid, invalid_owned};

/// Reads asset `name` for a source added with `ponko_ctx_add_asset_source`.
/// It returns a `PonkoStatus` value (as `uint32_t`, like every enum here):
/// `PONKO_OK` with `*data` and `*len` set to the bytes (copied by the
/// engine before the next call into this source; `*data` may be null when
/// `*len` is 0), `PONKO_NOT_FOUND` if this source doesn't have `name` (the
/// next source is asked), or any other status for a read error, which the
/// load reports as `PONKO_ERROR_IO`. Runs on the game thread, inside the
/// load call that needs it; it must not call back into the engine.
/// ```
/// let none: ponko_ffi::PonkoAssetReadFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoAssetReadFn =
    Option<unsafe extern "C" fn(user: *mut c_void, name: *const c_char, data: *mut *const u8, len: *mut usize) -> u32>;

/// Whether a source has asset `name`, without reading it. Optional: without
/// it, the engine reads the asset to find out.
/// ```
/// let none: ponko_ffi::PonkoAssetContainsFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoAssetContainsFn = Option<unsafe extern "C" fn(user: *mut c_void, name: *const c_char) -> bool>;

/// Called once with the source's `user` pointer when the engine drops the
/// source (when the game ends). Optional.
/// ```
/// let none: ponko_ffi::PonkoUserFreeFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoUserFreeFn = Option<unsafe extern "C" fn(user: *mut c_void)>;

/// Decodes an asset's bytes for `ponko_ctx_load_asset_with`: `data` holds
/// `len` bytes until it returns. Return `PONKO_OK` (a `PonkoStatus` value,
/// as `uint32_t`), or any other status if
/// the bytes aren't what you expected (the load then fails with
/// `PONKO_ERROR_DECODE`). It may call other `ponko_ctx_*` functions, for
/// example to create a texture from the pixels it decoded.
/// ```
/// let none: ponko_ffi::PonkoAssetDecodeFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoAssetDecodeFn = Option<unsafe extern "C" fn(user: *mut c_void, data: *const u8, len: usize) -> u32>;

/// A source whose reads are C callbacks.
struct CSource {
    id: String,
    read: unsafe extern "C" fn(*mut c_void, *const c_char, *mut *const u8, *mut usize) -> u32,
    contains: PonkoAssetContainsFn,
    free: PonkoUserFreeFn,
    user: *mut c_void,
    /// Set once the engine took the source: before that, dropping it (a
    /// rejected source) leaves `user` to the caller.
    owned: Rc<Cell<bool>>,
}

impl CSource {
    fn c_name(name: &str) -> std::io::Result<CString> {
        CString::new(name).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "asset name contains NUL"))
    }
}

impl AssetSource for CSource {
    fn id(&self) -> &str {
        &self.id
    }

    fn read(&self, name: &str) -> std::io::Result<Vec<u8>> {
        let c_name = Self::c_name(name)?;
        let mut data: *const u8 = std::ptr::null();
        let mut len = 0usize;
        // SAFETY: the caller of ponko_ctx_add_asset_source promised a callback
        // that takes these arguments and returns normally.
        let status = unsafe { (self.read)(self.user, c_name.as_ptr(), &mut data, &mut len) };
        const OK: u32 = PonkoStatus::PONKO_OK as u32;
        const NOT_FOUND: u32 = PonkoStatus::PONKO_NOT_FOUND as u32;
        match status {
            OK if len == 0 => Ok(Vec::new()),
            OK if data.is_null() => Err(std::io::Error::other(format!(
                "asset source \"{}\" returned no bytes for \"{name}\"",
                self.id
            ))),
            // SAFETY: the callback promised `len` readable bytes at `data`,
            // valid until the next call into this source.
            OK => Ok(unsafe { std::slice::from_raw_parts(data, len) }.to_vec()),
            NOT_FOUND => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("\"{name}\" is not in asset source \"{}\"", self.id),
            )),
            other => Err(std::io::Error::other(format!(
                "asset source \"{}\" could not read \"{name}\" (status {})",
                self.id, other
            ))),
        }
    }

    fn contains(&self, name: &str) -> bool {
        match self.contains {
            Some(contains) => match Self::c_name(name) {
                // SAFETY: as in `read`.
                Ok(c_name) => unsafe { contains(self.user, c_name.as_ptr()) },
                Err(_) => false,
            },
            None => !matches!(self.read(name), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
        }
    }
}

impl Drop for CSource {
    fn drop(&mut self) {
        if let (true, Some(free)) = (self.owned.get(), self.free) {
            // SAFETY: called once, when the engine drops the source it took.
            unsafe { free(self.user) };
        }
    }
}

/// Refuses an id that one of the game's sources already has, before
/// `add_source` would panic on it.
fn unique_source(c: &ponko::prelude::Ctx, id: &str) -> Result<(), Failure> {
    if c.assets.has_source(id) {
        return Err(invalid_owned(format!(
            "an asset source called \"{id}\" was already added"
        )));
    }
    Ok(())
}

fn source_id<'a>(id: *const c_char) -> Result<&'a str, Failure> {
    // SAFETY: caller supplies a NUL-terminated id.
    let id = unsafe { text(id)? };
    if id.is_empty() {
        return Err(invalid("an asset source needs a non-empty id"));
    }
    Ok(id)
}

/// Adds an asset source whose reads are your callbacks: `load_*` calls ask
/// the added sources first, newest last, then `assets/`. `id` names the
/// source (copied; non-empty and unique among the game's sources) in cache
/// keys and error messages. Assets from a source are cached by its id and
/// the name, and never hot reloaded. Add sources in setup, before loading
/// what they hold.
///
/// On success the engine owns `user`: it calls `free_user(user)` (if not
/// null) when the source is dropped. On failure it never calls it. An id
/// that a source of this game already has is `PONKO_INVALID_ARGUMENT` (Rust's
/// `add_source` panics on it).
///
/// Ownership: on success the engine owns `user`: it calls `free_user(user)` (if
/// not null) when the source is dropped. On failure it never calls it, and
/// `user` stays the caller's.
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_add_asset_source(std::ptr::null_mut(), c"pak".as_ptr(), None, None, None, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `id` NUL-terminated; the callbacks valid for the
/// whole game, with `user` as their first argument.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_add_asset_source(
    ctx: *mut PonkoCtx,
    id: *const c_char,
    read: PonkoAssetReadFn,
    contains: PonkoAssetContainsFn,
    free_user: PonkoUserFreeFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = source_id(id)?;
            let read = read.ok_or_else(|| invalid("read callback is null"))?;
            unique_source(c, id)?;
            let owned = Rc::new(Cell::new(false));
            c.assets.add_source(CSource {
                id: id.to_owned(),
                read,
                contains,
                free: free_user,
                user,
                owned: Rc::clone(&owned),
            });
            owned.set(true);
            Ok(())
        })
    })
}

/// Files in memory, to add as an asset source (Rust's `MemorySource`):
/// assets compiled into the program, generated at startup, or made up in a
/// test. Free it with `ponko_memory_source_free`.
/// ```
/// let source = unsafe { ponko_ffi::ponko_memory_source_new(c"embedded".as_ptr()) };
/// unsafe { ponko_ffi::ponko_memory_source_free(source) };
/// ```
pub struct PonkoMemorySource(MemorySource);

impl PonkoMemorySource {
    /// The source it holds.
    pub(crate) fn source(&self) -> &MemorySource {
        &self.0
    }
}

/// An empty memory source called `id` (copied; non-empty). Null on bad
/// arguments (see `ponko_last_error`).
///
/// Ownership: the returned source is the caller's (null on failure); free it
/// with ponko_memory_source_free.
///
/// ```
/// assert!(unsafe { ponko_ffi::ponko_memory_source_new(c"".as_ptr()) }.is_null());
/// ```
/// # Safety
/// `id` is null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_memory_source_new(id: *const c_char) -> *mut PonkoMemorySource {
    let mut result = std::ptr::null_mut();
    boundary(|| {
        let id = source_id(id)?;
        result = Box::into_raw(Box::new(PonkoMemorySource(MemorySource::new(id))));
        Ok(())
    });
    result
}

/// Frees a memory source; null is ignored. Sources added to a game are
/// copies, so this doesn't affect them.
/// ```
/// unsafe { ponko_ffi::ponko_memory_source_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `source` null or from `ponko_memory_source_new`, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_memory_source_free(source: *mut PonkoMemorySource) {
    if !source.is_null() {
        // SAFETY: caller passes an owned handle, once.
        drop(unsafe { Box::from_raw(source) });
    }
}

/// Adds (or replaces) file `name` with a copy of `len` bytes at `data`
/// (`data` may be null when `len` is 0).
/// ```
/// let source = unsafe { ponko_ffi::ponko_memory_source_new(c"embedded".as_ptr()) };
/// let rules = b"{\"speed\": 30}";
/// assert_eq!(unsafe { ponko_ffi::ponko_memory_source_insert(source, c"rules.json".as_ptr(), rules.as_ptr(), rules.len()) } as u32, 0);
/// unsafe { ponko_ffi::ponko_memory_source_free(source) };
/// ```
/// # Safety
/// `source` null or live; `name` NUL-terminated; `data` readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_memory_source_insert(
    source: *mut PonkoMemorySource,
    name: *const c_char,
    data: *const u8,
    len: usize,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller supplies a live handle.
        let source = unsafe { source.as_mut() }.ok_or_else(|| invalid("memory source is null"))?;
        // SAFETY: caller supplies a NUL-terminated name.
        let name = unsafe { text(name)? };
        let bytes = if len == 0 {
            Vec::new()
        } else if data.is_null() {
            return Err(invalid("data is null"));
        } else {
            // SAFETY: caller supplies `len` readable bytes.
            unsafe { std::slice::from_raw_parts(data, len) }.to_vec()
        };
        source.0.insert(name, bytes);
        Ok(())
    })
}

/// Adds a copy of `source` to the game's asset sources (see
/// `ponko_ctx_add_asset_source`); the handle stays yours to change or free.
/// A source id this game already has is `PONKO_INVALID_ARGUMENT` (Rust's
/// `add_source` panics on it).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_add_memory_source(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `source` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_add_memory_source(
    ctx: *mut PonkoCtx,
    source: *const PonkoMemorySource,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let source = source.as_ref().ok_or_else(|| invalid("memory source is null"))?;
        ctx_access(ctx, |c| {
            unique_source(c, source.0.id())?;
            c.assets.add_source(source.0.clone());
            Ok(())
        })
    })
}

/// Reads asset `name` (from the added sources, then `assets/`) and hands its
/// bytes to `decode`: your own asset format, with no registration (Rust's
/// `Assets::load_with`). Nothing is cached or watched; each call reads again.
/// A missing asset is `PONKO_NOT_FOUND`; a read error, or a decoder that
/// returns anything but `PONKO_OK`, `PONKO_INVALID_ARGUMENT`. Either way
/// `ponko_last_error_kind()` (`PONKO_ERROR_NOT_FOUND`, `_IO`, `_DECODE`) and
/// `ponko_last_error_path()` (`name`) say more. `decode` runs after the read,
/// outside the engine's borrow of the context, so it may call `ponko_ctx_*`.
///
/// Ownership: `user` stays the caller's and is only passed to `decode` during
/// this call; the engine keeps no pointer to it after returning.
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_asset_with(std::ptr::null_mut(), c"a.bin".as_ptr(), None, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated; `decode` takes `user`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_asset_with(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    decode: PonkoAssetDecodeFn,
    user: *mut c_void,
) -> PonkoStatus {
    boundary(|| {
        let decode = decode.ok_or_else(|| invalid("decode callback is null"))?;
        // SAFETY: caller supplies a NUL-terminated name.
        let name = unsafe { text(name)? };
        // SAFETY: forwarded caller contract; the borrow ends before `decode`.
        let bytes = unsafe {
            ctx_access(ctx, |c| {
                c.assets
                    .load_with(name, |bytes| Ok(bytes.to_vec()))
                    .map_err(engine_error)
            })?
        };
        // SAFETY: the caller's decoder takes `user` and these bytes.
        let status = unsafe { decode(user, bytes.as_ptr(), bytes.len()) };
        if status == PonkoStatus::PONKO_OK as u32 {
            return Ok(());
        }
        Err(engine_error(
            ponko::prelude::Error::new(
                ponko::prelude::ErrorKind::Decode,
                format!("\"{name}\" could not be decoded: the decoder returned status {status}"),
            )
            .with_path(name),
        ))
    })
}
