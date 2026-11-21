//! Data-only mods from C (Rust's `ponko::mods`): mount mod packs in a load
//! order, after checking them all, and ask which mods are on and what they
//! added.
use std::ffi::c_char;

use ponko::mods::{ModPack, ModSet, Mods};

use crate::ctx::{ctx_access, text};
use crate::perf::write_text;
use crate::{PonkoCtx, PonkoStatus, boundary, engine_error, error, invalid};

/// Mounts the mod packs at `paths` (`count` file paths, each a pack made by
/// `ponko pack` from a folder with `mod.json` at its root), in that load
/// order moved only as far as each manifest's `requires` and `after` need
/// (Rust's `ModSet::mount`). From then on assets load from the mods first,
/// the last mod with a name winning, then from the game's own files.
///
/// Every mod is checked before any is mounted: a bad one is refused whole
/// and nothing is mounted. `ponko_last_error()` then names the mod, the
/// file and the reason (`mod "frost", file "prefabs/frost.json": component
/// "Frosty" isn't one this game has`), `ponko_last_error_path()` the file.
/// A required mod missing, or a component or resource the game doesn't
/// reflect, is `PONKO_NOT_FOUND`; every other problem (a damaged pack, a
/// cycle in the order, a name like `../x`, a file over the limits, a field
/// that doesn't fit) is `PONKO_INVALID_ARGUMENT`. Mount in setup, after
/// reflecting the game's components; mods mount once per game.
///
/// Ownership: `paths` and its strings stay the caller's; they are read
/// during the call only.
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mount_mods(std::ptr::null_mut(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `paths` readable for `count` NUL-terminated strings
/// (or null with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mount_mods(
    ctx: *mut PonkoCtx,
    paths: *const *const c_char,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if paths.is_null() && count != 0 {
            return Err(invalid("paths is null"));
        }
        let mut set = ModSet::new();
        for i in 0..count {
            let path = text(*paths.add(i))?;
            let pack = ModPack::open(path).map_err(|e| engine_error(e.into()))?;
            set.add(pack);
        }
        ctx_access(ctx, |c| set.mount(c).map_err(|e| engine_error(e.into())))
    })
}

/// How many mods are mounted (0 before `ponko_ctx_mount_mods`) to
/// `*out_count`.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_count(std::ptr::null_mut(), &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_count(ctx: *mut PonkoCtx, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_count.is_null() {
            return Err(invalid("out_count is null"));
        }
        let n = ctx_access(ctx, |c| {
            Ok(c.world.get_resource::<Mods>().map_or(0, |m| m.list().len()))
        })?;
        out_count.write(n);
        Ok(())
    })
}

/// The id of mounted mod `index`, in load order (Rust's `Mods::list`).
/// Writes it to `buffer` (NUL-terminated when it fits in `capacity`) and
/// its length to `*out_len`; call with a null buffer and capacity 0 to learn
/// the length. An index past the end is `PONKO_NOT_FOUND`.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_id(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_id(
    ctx: *mut PonkoCtx,
    index: usize,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        check_output(buffer, capacity, out_len)?;
        ctx_access(ctx, |c| {
            let id = c
                .world
                .get_resource::<Mods>()
                .and_then(|m| m.list().get(index))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no mod at that index"))?;
            write_text(buffer, capacity, out_len, &id.id);
            Ok(())
        })
    })
}

/// The id of the mod asset `name` loads from (Rust's `Mods::provider`): the
/// last mod in load order that has it. `PONKO_NOT_FOUND` if no mod has it
/// (it loads from the game's own files). Output as `ponko_ctx_mod_id`.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_provider(std::ptr::null_mut(), c"a.json".as_ptr(), std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated; `buffer` writable for
/// `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_provider(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let name = text(name)?;
        check_output(buffer, capacity, out_len)?;
        ctx_access(ctx, |c| {
            let id = c
                .world
                .get_resource::<Mods>()
                .and_then(|m| m.provider(name))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no mod has that file"))?;
            write_text(buffer, capacity, out_len, id);
            Ok(())
        })
    })
}

/// File `index` of the mods' files whose names start with `prefix`, sorted,
/// each name once (Rust's `Mods::files_under`): how a game finds what mods
/// added (`"enemies/"`). Output as `ponko_ctx_mod_id`; an index past the
/// last such file is `PONKO_NOT_FOUND`, so a loop from 0 lists them all.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_file_under(std::ptr::null_mut(), c"enemies/".as_ptr(), 0, std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `prefix` NUL-terminated; `buffer` writable for
/// `capacity` bytes (or null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_file_under(
    ctx: *mut PonkoCtx,
    prefix: *const c_char,
    index: usize,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let prefix = text(prefix)?;
        check_output(buffer, capacity, out_len)?;
        ctx_access(ctx, |c| {
            let file = c
                .world
                .get_resource::<Mods>()
                .and_then(|m| m.files_under(prefix).nth(index))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no more mod files under that prefix"))?;
            write_text(buffer, capacity, out_len, file);
            Ok(())
        })
    })
}

/// The mounted mods' fingerprint (Rust's `Mods::fingerprint`) to
/// `*out_fingerprint`, 0 when none are mounted: lockstep peers and replays
/// compare it.
/// ```
/// let mut f = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mods_fingerprint(std::ptr::null_mut(), &mut f) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_fingerprint` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mods_fingerprint(ctx: *mut PonkoCtx, out_fingerprint: *mut u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_fingerprint.is_null() {
            return Err(invalid("out_fingerprint is null"));
        }
        let f = ctx_access(ctx, |c| Ok(c.world.get_resource::<Mods>().map_or(0, Mods::fingerprint)))?;
        out_fingerprint.write(f);
        Ok(())
    })
}

fn check_output(buffer: *mut c_char, capacity: usize, out_len: *mut usize) -> Result<(), crate::Failure> {
    if out_len.is_null() || (buffer.is_null() && capacity > 0) {
        return Err(invalid("buffer or out_len is null"));
    }
    Ok(())
}
