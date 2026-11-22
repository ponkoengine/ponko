//! Code mods from C (Rust's `ponko::mods::with_code`, feature `mods-wasm`):
//! look up a function a mod's WebAssembly module exports, call it with
//! numbers under the same fuel and memory limits as from Rust, and choose
//! which entities and prefabs the mods' host functions reach. Built without
//! the feature, every call reports `PONKO_UNSUPPORTED`.
use std::ffi::c_char;

use crate::ctx::{ctx_access, out, text};
#[cfg(feature = "mods-wasm")]
use crate::engine_error;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid};

/// The type of a [`PonkoModValue`]: WebAssembly's four number types.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoModValueKind {
    /// `i32`, in `i`.
    PONKO_MOD_I32 = 0,
    /// `i64`, in `i` (an entity's bits, for example).
    PONKO_MOD_I64 = 1,
    /// `f32`, in `f`.
    PONKO_MOD_F32 = 2,
    /// `f64`, in `f`.
    PONKO_MOD_F64 = 3,
}

/// A number passed to or returned from a mod's function (Rust's
/// `ModValue`): integers in `i`, floats in `f`, by `kind`.
/// ```
/// let v = ponko_ffi::PonkoModValue { kind: ponko_ffi::PonkoModValueKind::PONKO_MOD_F32, i: 0, f: 1.5 };
/// assert_eq!(v.f, 1.5);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoModValue {
    /// Which number type this is.
    pub kind: PonkoModValueKind,
    /// The value of an `i32` or `i64`.
    pub i: i64,
    /// The value of an `f32` or `f64`.
    pub f: f64,
}

/// A function a code mod exports, found by [`ponko_ctx_mod_function`]: keep
/// it and pass it to [`ponko_ctx_mod_call`] (no name lookup per call).
/// Valid in the game that found it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoModFn {
    /// The function's number in this game, from 0.
    pub id: u32,
}

#[cfg(feature = "mods-wasm")]
mod on {
    use ponko::mods::{ModFn, ModValue};

    use super::{PonkoModValue, PonkoModValueKind as T};

    /// The functions C looked up, by `PonkoModFn::id`.
    #[derive(Default)]
    pub(super) struct Found(pub(super) Vec<(String, String, ModFn)>);

    pub(super) fn to_rust(v: &PonkoModValue) -> Option<ModValue> {
        Some(match v.kind {
            T::PONKO_MOD_I32 => ModValue::I32(i32::try_from(v.i).ok()?),
            T::PONKO_MOD_I64 => ModValue::I64(v.i),
            T::PONKO_MOD_F32 => ModValue::F32(v.f as f32),
            T::PONKO_MOD_F64 => ModValue::F64(v.f),
        })
    }

    pub(super) fn to_c(v: ModValue) -> PonkoModValue {
        let (kind, i, f) = match v {
            ModValue::I32(x) => (T::PONKO_MOD_I32, i64::from(x), 0.0),
            ModValue::I64(x) => (T::PONKO_MOD_I64, x, 0.0),
            ModValue::F32(x) => (T::PONKO_MOD_F32, 0, f64::from(x)),
            ModValue::F64(x) => (T::PONKO_MOD_F64, 0, x),
        };
        PonkoModValue { kind, i, f }
    }
}

/// Builds without the feature: every call says so.
#[cfg(not(feature = "mods-wasm"))]
fn not_built() -> Failure {
    error(
        PonkoStatus::PONKO_UNSUPPORTED,
        "code mods are not compiled in: ponko_ffi was built without the `mods-wasm` feature",
    )
}

/// Runs `f` with the mounted code mods, or fails: `PONKO_NOT_FOUND` when no
/// mounted mod has code, `PONKO_UNSUPPORTED` without the feature.
#[cfg(feature = "mods-wasm")]
unsafe fn with_code<R>(
    ctx: *mut PonkoCtx,
    f: impl FnOnce(&mut ponko::mods::ModCode, &mut on::Found) -> Result<R, Failure>,
) -> Result<R, Failure> {
    // SAFETY: forwarded caller contract.
    unsafe {
        ctx_access(ctx, |c| {
            let mut found = c.world.remove_resource::<on::Found>().unwrap_or_default();
            let out = ponko::mods::with_code(c, |code| f(code, &mut found));
            c.world.insert_resource(found);
            out.unwrap_or_else(|| Err(error(PonkoStatus::PONKO_NOT_FOUND, "no mounted mod has code")))
        })
    }
}

#[cfg(not(feature = "mods-wasm"))]
unsafe fn with_code<R>(
    ctx: *mut PonkoCtx,
    _: impl FnOnce(&mut (), &mut ()) -> Result<R, Failure>,
) -> Result<R, Failure> {
    // SAFETY: forwarded caller contract.
    unsafe { ctx_access(ctx, |_| Err(not_built())) }
}

/// Finds the function `name` that mod `mod_id` exports and writes a handle
/// to it to `*out_fn` (Rust's `ModCode::function`). Look a function up once
/// and keep the handle. `PONKO_NOT_FOUND` when no mounted mod has code, or
/// the mod has no such function; `PONKO_UNSUPPORTED` when the library was
/// built without the `mods-wasm` feature.
///
/// Ownership: `mod_id` and `name` stay the caller's; read during the call.
///
/// ```
/// let mut f = ponko_ffi::PonkoModFn::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_function(std::ptr::null_mut(), c"m".as_ptr(), c"f".as_ptr(), &mut f) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `mod_id` and `name` NUL-terminated; `out_fn` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_function(
    ctx: *mut PonkoCtx,
    mod_id: *const c_char,
    name: *const c_char,
    out_fn: *mut PonkoModFn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let (mod_id, name) = (text(mod_id)?, text(name)?);
        if out_fn.is_null() {
            return Err(invalid("out_fn is null"));
        }
        #[cfg(feature = "mods-wasm")]
        let id = with_code(ctx, |code, found| {
            if let Some(i) = found.0.iter().position(|(m, n, _)| m == mod_id && n == name) {
                return Ok(i);
            }
            let f = code
                .function(mod_id, name)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "the mod has no code or no such function"))?;
            found.0.push((mod_id.to_owned(), name.to_owned(), f));
            Ok(found.0.len() - 1)
        })?;
        #[cfg(not(feature = "mods-wasm"))]
        let id: usize = with_code(ctx, |_, _| {
            let _ = (mod_id, name);
            Ok(0)
        })?;
        let id = u32::try_from(id).map_err(|_| invalid("too many mod functions looked up"))?;
        out(out_fn, PonkoModFn { id })
    })
}

/// Calls mod function `f` with `count` numbers from `args`, with one call's
/// fuel (Rust's `ModCode::call`): the same fuel and memory limits as a Rust
/// game's (`CodeLimits::default()`: 10 million fuel, 16 MiB). Writes its
/// result to `*out_result` and whether it returned one to `*out_has_result`
/// (both optional: null when not wanted). A call that doesn't fit the
/// function's parameters, runs out of fuel or memory, or traps returns
/// `PONKO_INVALID_ARGUMENT` with the mod and the reason in
/// `ponko_last_error()`; the mod stays callable. An `f` this game didn't
/// find, or no mounted mod with code, is `PONKO_NOT_FOUND`;
/// `PONKO_UNSUPPORTED` without the `mods-wasm` feature.
///
/// Ownership: `args` stays the caller's; read during the call.
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_call(std::ptr::null_mut(), ponko_ffi::PonkoModFn::default(), std::ptr::null(), 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `args` readable for `count` values (or null with
/// `count` 0); `out_result` and `out_has_result` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_call(
    ctx: *mut PonkoCtx,
    f: PonkoModFn,
    args: *const PonkoModValue,
    count: usize,
    out_result: *mut PonkoModValue,
    out_has_result: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if args.is_null() && count != 0 {
            return Err(invalid("args is null"));
        }
        let args: &[PonkoModValue] = if count == 0 {
            &[]
        } else {
            std::slice::from_raw_parts(args, count)
        };
        #[cfg(feature = "mods-wasm")]
        let result = with_code(ctx, |code, found| {
            let (_, _, func) = found
                .0
                .get(f.id as usize)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "not a mod function this game found"))?;
            let mut values = [ponko::mods::ModValue::I32(0); 16];
            if args.len() > values.len() {
                return Err(invalid("more than 16 arguments"));
            }
            for (slot, a) in values.iter_mut().zip(args) {
                *slot = on::to_rust(a).ok_or_else(|| invalid("an i32 argument out of the i32 range"))?;
            }
            code.call(*func, &values[..args.len()])
                .map(|r| r.map(on::to_c))
                .map_err(|e| engine_error(e.into()))
        })?;
        #[cfg(not(feature = "mods-wasm"))]
        let result: Option<PonkoModValue> = with_code(ctx, |_, _| {
            let _ = (f, args);
            Ok(None)
        })?;
        if !out_has_result.is_null() {
            out_has_result.write(result.is_some());
        }
        if let (Some(r), false) = (result, out_result.is_null()) {
            out_result.write(r);
        }
        Ok(())
    })
}

/// Lets the mods' host functions `get_field` and `set_field` read and write
/// the reflected components of `entity` (bits from `ponko_entity_bits`; hand
/// them to the mod as an `i64`). A mod reaches no entity the game didn't
/// expose, apart from the roots of the prefabs it spawns (Rust's
/// `ModCode::expose`). `PONKO_NOT_FOUND` when no mounted mod has code;
/// `PONKO_UNSUPPORTED` without the `mods-wasm` feature.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_expose_entity(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_expose_entity(ctx: *mut PonkoCtx, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_code(ctx, |code, _| {
            #[cfg(feature = "mods-wasm")]
            code.expose(ponko::prelude::Entity::from_bits(entity));
            #[cfg(not(feature = "mods-wasm"))]
            let _ = (code, entity);
            Ok(())
        })
    })
}

/// Takes `entity` back from the mods (Rust's `ModCode::hide`); writes
/// whether it was exposed to `*out_was_exposed` (null when not wanted).
/// Statuses as [`ponko_ctx_mod_expose_entity`].
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_hide_entity(std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_was_exposed` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_hide_entity(
    ctx: *mut PonkoCtx,
    entity: u64,
    out_was_exposed: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        #[cfg(feature = "mods-wasm")]
        let was = with_code(ctx, |code, _| Ok(code.hide(ponko::prelude::Entity::from_bits(entity))))?;
        #[cfg(not(feature = "mods-wasm"))]
        let was = with_code(ctx, |_, _| {
            let _ = entity;
            Ok(false)
        })?;
        if !out_was_exposed.is_null() {
            out_was_exposed.write(was);
        }
        Ok(())
    })
}

/// Lets the mods spawn the prefab at `path` with the host function
/// `spawn_prefab` (the path they pass, the one `ponko_ctx_spawn_prefab`
/// takes; Rust's `ModCode::expose_prefab`). Statuses as
/// [`ponko_ctx_mod_expose_entity`].
///
/// Ownership: `path` stays the caller's; copied during the call.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_expose_prefab(std::ptr::null_mut(), c"prefabs/a.json".as_ptr()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_expose_prefab(ctx: *mut PonkoCtx, path: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        with_code(ctx, |code, _| {
            #[cfg(feature = "mods-wasm")]
            code.expose_prefab(path);
            #[cfg(not(feature = "mods-wasm"))]
            let _ = (code, path);
            Ok(())
        })
    })
}

/// Seeds every mod's host function `random` (Rust's
/// `ModCode::seed_random`): same seed, same sequence; each mod has its own.
/// Statuses as [`ponko_ctx_mod_expose_entity`].
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_mod_seed_random(std::ptr::null_mut(), 7) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_mod_seed_random(ctx: *mut PonkoCtx, seed: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_code(ctx, |code, _| {
            #[cfg(feature = "mods-wasm")]
            code.seed_random(seed);
            #[cfg(not(feature = "mods-wasm"))]
            let _ = (code, seed);
            Ok(())
        })
    })
}
