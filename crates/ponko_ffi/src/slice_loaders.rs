//! Sliced loaders from C (Rust `SliceLoader` and `AssetGroup::with_sliced`):
//! an asset group decodes a large file of the game's own a bounded piece
//! at a time, so a loading screen keeps its frame rate where there are no
//! worker threads (a browser), and on a worker where there are.
//!
//! The design (the guide's "Sliced loaders from C"): a Rust loader is a
//! value with `step` and `finish`; a C loader is three callbacks over a
//! state pointer of the game's own.
//!
//! - `start(user, bytes, len, &state)` gets the file's bytes (valid during
//!   the call) and makes the state: the parsed header, a cursor, the
//!   output being built. It runs again for each restart of a failed group.
//! - `step(user, state, &done)` does the next bounded piece of the work
//!   and sets `done` once it is all done. A status other than `PONKO_OK`
//!   from either fails the group with a decode error naming the asset.
//! - `free_state(user, state)` frees a state nobody took: the group failed
//!   or was cancelled, the game ended, or the value was never taken.
//! - Once done, the state is the value: `ponko_ctx_take_asset_group_sliced`
//!   hands the pointer to the game, which owns (and frees) it from then.
//!
//! `user` belongs to the loader: `free_user` (if set) runs once when the
//! group no longer needs it. `start`, `step` and `free_state` may run on a
//! worker thread, at the same time as other loaders' calls (never two of
//! one asset at once), so they touch only their own state and `user`, and
//! never call `ponko_ctx_*`.
//!
//! ```c
//! static uint32_t start(void* user, const uint8_t* bytes, size_t len, void** out_state) {
//!     Sum* s = calloc(1, sizeof *s); s->text = copy(bytes, len); *out_state = s; return PONKO_OK;
//! }
//! static uint32_t step(void* user, void* state, bool* out_done) {
//!     Sum* s = state; /* a thousand lines */ *out_done = s->at == s->len; return PONKO_OK;
//! }
//! PonkoSliceLoader loader = {.struct_size = sizeof loader, .version = PONKO_SLICE_LOADER_VERSION,
//!                            .start = start, .step = step, .free_state = free_sum};
//! ponko_asset_group_add_sliced(group, "numbers.txt", &loader);
//! ```

use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};
use std::sync::Arc;

use ponko::assets::SliceLoader;
use ponko::prelude::{Error, ErrorKind};

use crate::asset_groups::{PonkoAssetGroup, group_id};
use crate::component_hooks::PonkoFreeUserFn;
use crate::ctx::{ctx_access, out, text};
use crate::descriptors::{Versioned, read_desc};
use crate::{PonkoCtx, PonkoStatus, boundary, engine_error, invalid};

/// The version of `PonkoSliceLoader` this header describes.
pub const PONKO_SLICE_LOADER_VERSION: u32 = 1;

/// Makes a sliced loader's state from the file's bytes (valid during the
/// call only) and writes it to `*out_state`; returns `PONKO_OK`, or any
/// other status to fail the group.
/// ```
/// let none: ponko_ffi::PonkoSliceStartFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoSliceStartFn =
    Option<unsafe extern "C" fn(user: *mut c_void, bytes: *const u8, len: usize, out_state: *mut *mut c_void) -> u32>;

/// Does the next bounded piece of a sliced loader's work and sets
/// `*out_done` once it is all done; returns `PONKO_OK`, or any other status
/// to fail the group.
/// ```
/// let none: ponko_ffi::PonkoSliceStepFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoSliceStepFn =
    Option<unsafe extern "C" fn(user: *mut c_void, state: *mut c_void, out_done: *mut bool) -> u32>;

/// Frees a sliced loader's state that nobody took.
/// ```
/// let none: ponko_ffi::PonkoSliceFreeFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoSliceFreeFn = Option<unsafe extern "C" fn(user: *mut c_void, state: *mut c_void)>;

/// A versioned loader descriptor for `ponko_asset_group_add_sliced` or
/// `ponko_asset_group_add_decoder`. Set `struct_size`, `version`, and `start`.
/// Set `step` for a sliced loader and leave it null for a one-shot decoder.
///
/// Version 1: every field below.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoSliceLoader {
    /// `sizeof(PonkoSliceLoader)` as the caller compiled it.
    pub struct_size: u32,
    /// `PONKO_SLICE_LOADER_VERSION` as the caller compiled it.
    pub version: u32,
    /// Makes the state from the file's bytes.
    pub start: PonkoSliceStartFn,
    /// Does the next piece of sliced work. Null for a one-shot decoder.
    pub step: PonkoSliceStepFn,
    /// Frees a state nobody took; null if states need no freeing.
    pub free_state: PonkoSliceFreeFn,
    /// Passed to every callback.
    pub user: *mut c_void,
    /// Called once with `user` when the group no longer needs it; null for none.
    pub free_user: PonkoFreeUserFn,
}

const _: () = assert!(offset_of!(PonkoSliceLoader, struct_size) == 0 && offset_of!(PonkoSliceLoader, version) == 4);
impl Versioned for PonkoSliceLoader {
    const NAME: &'static str = "PonkoSliceLoader";
    const VERSION: u32 = PONKO_SLICE_LOADER_VERSION;
    fn len_of(version: u32) -> Option<usize> {
        match version {
            1 => Some(offset_of!(Self, free_user) + size_of::<PonkoFreeUserFn>()),
            _ => None,
        }
    }
}

/// The callbacks and `user` of one added asset, shared by its loads.
struct Callbacks {
    start: unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut *mut c_void) -> u32,
    step: PonkoSliceStepFn,
    free_state: PonkoSliceFreeFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
}

// SAFETY: the caller registering a loader promises the callbacks may run
// on any thread with `user` (see the registration's Ownership paragraph).
unsafe impl Send for Callbacks {}
// SAFETY: as above; the engine never runs two calls of one asset at once.
unsafe impl Sync for Callbacks {}

impl Callbacks {
    fn start(self: &Arc<Self>, bytes: &[u8]) -> Result<State, Error> {
        let mut ptr = std::ptr::null_mut();
        // SAFETY: the caller's callback uses these bytes only during this call.
        let status = unsafe { (self.start)(self.user, bytes.as_ptr(), bytes.len(), &mut ptr) };
        let state = State {
            ptr,
            callbacks: Arc::clone(self),
        };
        if status != PonkoStatus::PONKO_OK as u32 {
            return Err(if self.step.is_some() {
                failed("start", status)
            } else {
                Error::new(
                    ErrorKind::Decode,
                    format!("the one-shot decoder returned status {status}"),
                )
            });
        }
        Ok(state)
    }

    fn free(&self, state: *mut c_void) {
        if let (Some(free), false) = (self.free_state, state.is_null()) {
            // SAFETY: the caller's free function, with a state its `start` made.
            unsafe { free(self.user, state) };
        }
    }
}

impl Drop for Callbacks {
    fn drop(&mut self) {
        if let Some(free_user) = self.free_user {
            // SAFETY: the caller's function, once, with its own pointer.
            unsafe { free_user(self.user) };
        }
    }
}

/// A state the engine holds: freed with `free_state` unless taken.
struct State {
    ptr: *mut c_void,
    callbacks: Arc<Callbacks>,
}

// SAFETY: as `Callbacks`: the state is only handed to the caller's
// callbacks, one call at a time.
unsafe impl Send for State {}

impl Drop for State {
    fn drop(&mut self) {
        self.callbacks
            .free(std::mem::replace(&mut self.ptr, std::ptr::null_mut()));
    }
}

/// A finished state, waiting for `ponko_ctx_take_asset_group_sliced`.
struct SlicedValue(State);
struct DecodedValue(State);

fn failed(what: &str, status: u32) -> Error {
    Error::new(
        ErrorKind::Decode,
        format!("the sliced loader's {what} returned status {status}"),
    )
}

impl SliceLoader for State {
    type Value = SlicedValue;

    fn step(&mut self) -> Result<bool, Error> {
        let mut done = false;
        let step = self
            .callbacks
            .step
            .ok_or_else(|| Error::new(ErrorKind::Invalid, "a sliced loader needs step"))?;
        // SAFETY: the caller's step, with its own user and state.
        let status = unsafe { step(self.callbacks.user, self.ptr, &mut done) };
        if status != PonkoStatus::PONKO_OK as u32 {
            return Err(failed("step", status));
        }
        Ok(done)
    }

    fn finish(self) -> SlicedValue {
        SlicedValue(self)
    }
}

/// Adds asset `path` (copied) to `group`, decoded by the sliced `loader`
/// (Rust `AssetGroup::with_sliced`): once the file is read, `start` makes
/// a state from its bytes and `step` runs until it says it is done. Where
/// there are no worker threads, each frame's group step calls `step` within
/// its frame budget, so a large file is spread over several frames; with
/// workers, the whole load runs on a worker. Keep each `step` short: the
/// budget is checked between calls. Take the finished state with
/// `ponko_ctx_take_asset_group_sliced`. A null `start` or `step`:
/// `PONKO_INVALID_ARGUMENT`, nothing added (and `free_user` not called).
///
/// Ownership: `loader` and `path` are read during the call. `user` is
/// taken over by the group: it is passed to the callbacks until the group
/// is done with it, then to `free_user` (if set) once, on any thread. The
/// callbacks may run on worker threads while the game runs; they must not
/// call `ponko_ctx_*`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_asset_group_add_sliced(std::ptr::null_mut(), c"a".as_ptr(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `group` null or live; `path` NUL-terminated; `loader` null or a readable
/// descriptor with its header set, its callbacks safe to call with `user`
/// from any thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_asset_group_add_sliced(
    group: *mut PonkoAssetGroup,
    path: *const c_char,
    loader: *const PonkoSliceLoader,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { add_loader(group, path, loader, true) }
}

/// Adds a custom, one-shot decoder (Rust `AssetGroup::with`). Set `start`
/// and leave `step` null. Take the decoded value with
/// `ponko_ctx_take_asset_group_decoded`.
///
/// Ownership: the descriptor and path are read during this call. On success
/// the group owns `user` until it calls `free_user`, if set. It calls
/// `free_state` for a returned state nobody takes, including failure or
/// cancellation. Callbacks can run on any thread and must not call `ponko_ctx_*`.
///
/// # Safety
/// `group` null or live; `path` NUL-terminated; `loader` a readable descriptor;
/// its callbacks safe to call with `user` from any thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_asset_group_add_decoder(
    group: *mut PonkoAssetGroup,
    path: *const c_char,
    loader: *const PonkoSliceLoader,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { add_loader(group, path, loader, false) }
}

unsafe fn add_loader(
    group: *mut PonkoAssetGroup,
    path: *const c_char,
    loader: *const PonkoSliceLoader,
    sliced: bool,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller contract.
        let group = unsafe { group.as_mut() }.ok_or_else(|| invalid("asset group is null"))?;
        // SAFETY: caller contract.
        let path = unsafe { text(path)? };
        // SAFETY: caller contract.
        let d = unsafe {
            read_desc(
                loader,
                PonkoSliceLoader {
                    struct_size: size_of::<PonkoSliceLoader>() as u32,
                    version: PONKO_SLICE_LOADER_VERSION,
                    start: None,
                    step: None,
                    free_state: None,
                    user: std::ptr::null_mut(),
                    free_user: None,
                },
            )?
        };
        let start = d.start.ok_or_else(|| invalid("an asset loader needs start"))?;
        if sliced != d.step.is_some() {
            return Err(invalid(if sliced {
                "a sliced loader needs step"
            } else {
                "a one-shot decoder must leave step null"
            }));
        }
        let declared = group
            .0
            .take()
            .ok_or_else(|| invalid("asset group was already started"))?;
        let callbacks = Arc::new(Callbacks {
            start,
            step: d.step,
            free_state: d.free_state,
            user: d.user,
            free_user: d.free_user,
        });
        group.0 = Some(if sliced {
            declared.with_sliced(path, move |bytes| callbacks.start(&bytes))
        } else {
            declared.with(path, move |bytes| callbacks.start(bytes).map(DecodedValue))
        });
        Ok(())
    })
}

/// Takes the finished state of asset `path` of group `id` (added with
/// `ponko_asset_group_add_sliced`) into `*out_state`, once: a second call
/// is `PONKO_NOT_FOUND`, as is a call before the asset finished.
///
/// Ownership: `out_state` receives the loader's state, owned by the caller
/// from now on: the engine won't call `free_state` for it; free it yourself
/// (with the loader's own `free_state`, for example).
/// ```
/// let mut state = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_take_asset_group_sliced(std::ptr::null_mut(), 1, c"a".as_ptr(), &mut state) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` NUL-terminated; `out_state` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_take_asset_group_sliced(
    ctx: *mut PonkoCtx,
    id: u32,
    path: *const c_char,
    out_state: *mut *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { take_value(ctx, id, path, out_state, "sliced", |SlicedValue(state)| state) }
}

/// Takes the finished one-shot decoder value. Returns `PONKO_NOT_FOUND`
/// when the value is not ready or has already been taken.
///
/// Ownership: on success `out_state` receives a value the caller must free.
/// The engine stops managing that value.
///
/// # Safety
/// `ctx` null or live; `path` NUL-terminated; `out_state` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_take_asset_group_decoded(
    ctx: *mut PonkoCtx,
    id: u32,
    path: *const c_char,
    out_state: *mut *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { take_value(ctx, id, path, out_state, "decoded", |DecodedValue(state)| state) }
}

unsafe fn take_value<T: Send + 'static>(
    ctx: *mut PonkoCtx,
    id: u32,
    path: *const c_char,
    out_state: *mut *mut c_void,
    kind: &str,
    unwrap: impl FnOnce(T) -> State,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        if out_state.is_null() {
            return Err(invalid("out_state is null"));
        }
        let value = ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            c.assets.take_group_value::<T>(id, path).ok_or_else(|| {
                engine_error(
                    Error::new(
                        ErrorKind::NotFound,
                        format!("asset group {} has no finished {kind} value for \"{path}\" (not loaded yet, already taken, or a different loader)", id.index()),
                    )
                    .with_path(path),
                )
            })
        })?;
        let mut state = unwrap(value);
        out(out_state, std::mem::replace(&mut state.ptr, std::ptr::null_mut()))
    })
}
