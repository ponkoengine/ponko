//! Asset groups from C: assets declared together and loaded in the
//! background with progress, for loading screens (Rust's `AssetGroup`,
//! `Assets::start_group` and the `group_*` calls, `Game::loading_state`),
//! and background asset sources (`Assets::add_background_source`).
use std::ffi::{c_char, c_void};

use ponko::assets::{AssetGroup, GroupId, GroupProgress, GroupState, GroupStats};

use crate::asset_sources::{PonkoAssetDecodeFn, PonkoMemorySource};
use crate::ctx::{ctx_access, out, text, with_ctx};
use crate::game::{build, ok};
use crate::{PonkoCtx, PonkoGame, PonkoStatus, boundary, engine_error, invalid, invalid_owned};

/// A set of assets to load together (Rust's `AssetGroup`): make one with
/// `ponko_asset_group_new`, add assets, start it with
/// `ponko_ctx_start_asset_group` (which takes it) or free it with
/// `ponko_asset_group_free`.
/// ```
/// let group = unsafe { ponko_ffi::ponko_asset_group_new(c"level".as_ptr()) };
/// unsafe { ponko_ffi::ponko_asset_group_free(group) };
/// ```
pub struct PonkoAssetGroup(pub(crate) Option<AssetGroup>);

/// Where an asset group is (Rust's `GroupState`), from
/// `ponko_ctx_asset_group_state`; pass it as a `uint32_t`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetGroupState::PONKO_GROUP_READY as u32, 1);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAssetGroupState {
    /// Its assets are being read, decoded and uploaded.
    PONKO_GROUP_LOADING = 0,
    /// Every asset is loaded.
    PONKO_GROUP_READY = 1,
    /// An asset failed (`ponko_ctx_asset_group_error` says which and why).
    PONKO_GROUP_FAILED = 2,
    /// Cancelled before it was ready.
    PONKO_GROUP_CANCELLED = 3,
}

/// How far an asset group got (Rust's `GroupProgress`), from
/// `ponko_ctx_asset_group_progress`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetGroupProgress::default().total, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoAssetGroupProgress {
    /// Where the group is (a `PonkoAssetGroupState`).
    pub state: u32,
    /// Assets loaded.
    pub loaded: usize,
    /// Assets the group declares.
    pub total: usize,
    /// Bytes of the assets' files read so far.
    pub bytes_read: u64,
    /// Bytes of all the assets' files, as far as they are known.
    pub bytes_total: u64,
    /// Whether `bytes_total` counts every asset.
    pub bytes_known: bool,
    /// Bytes of the assets loaded, out of `bytes_total`.
    pub bytes_loaded: u64,
    /// How much is done, from 0 to 1, for a progress bar; 1 once ready.
    pub fraction: f32,
    /// Milliseconds the game thread spent uploading the group's textures
    /// to the GPU.
    pub upload_ms: f32,
    /// Bytes of the group's textures uploaded to the GPU.
    pub bytes_uploaded: u64,
}

impl From<GroupProgress> for PonkoAssetGroupProgress {
    fn from(p: GroupProgress) -> Self {
        Self {
            state: state_to_c(p.state),
            loaded: p.loaded,
            total: p.total,
            bytes_read: p.bytes_read,
            bytes_total: p.bytes_total,
            bytes_known: p.bytes_known,
            bytes_loaded: p.bytes_loaded,
            fraction: p.fraction(),
            upload_ms: p.upload_ms,
            bytes_uploaded: p.bytes_uploaded,
        }
    }
}

/// What the game thread's asset group step cost (Rust's `GroupStats`),
/// from `ponko_ctx_asset_group_stats`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetGroupStats::default().steps, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PonkoAssetGroupStats {
    /// The time budget of each frame's step, in milliseconds.
    pub frame_budget_ms: f32,
    /// Frames that ran the step.
    pub steps: u64,
    /// Milliseconds the last step took.
    pub step_ms: f32,
    /// The longest step, in milliseconds.
    pub max_step_ms: f32,
    /// Steps that took longer than the budget.
    pub over_budget: u64,
    /// Bytes the last step uploaded.
    pub frame_bytes: u64,
    /// Milliseconds the last step spent uploading textures to the GPU.
    pub upload_ms: f32,
    /// The most milliseconds one step spent uploading.
    pub max_upload_ms: f32,
    /// Milliseconds every step spent uploading, since the game started.
    pub total_upload_ms: f64,
    /// Bytes every step uploaded, since the game started.
    pub total_upload_bytes: u64,
}

impl From<GroupStats> for PonkoAssetGroupStats {
    fn from(s: GroupStats) -> Self {
        Self {
            frame_budget_ms: s.frame_budget_ms,
            steps: s.steps,
            step_ms: s.step_ms,
            max_step_ms: s.max_step_ms,
            over_budget: s.over_budget,
            frame_bytes: s.frame_bytes,
            upload_ms: s.upload_ms,
            max_upload_ms: s.max_upload_ms,
            total_upload_ms: s.total_upload_ms,
            total_upload_bytes: s.total_upload_bytes,
        }
    }
}

fn state_to_c(state: GroupState) -> u32 {
    (match state {
        GroupState::Ready => PonkoAssetGroupState::PONKO_GROUP_READY,
        GroupState::Failed => PonkoAssetGroupState::PONKO_GROUP_FAILED,
        GroupState::Cancelled => PonkoAssetGroupState::PONKO_GROUP_CANCELLED,
        _ => PonkoAssetGroupState::PONKO_GROUP_LOADING,
    }) as u32
}

/// The group `id` names in `c`, or `PONKO_INVALID_ARGUMENT`.
pub(crate) fn group_id(c: &ponko::prelude::Ctx, id: u32) -> Result<GroupId, crate::Failure> {
    (id as usize)
        .checked_sub(1)
        .and_then(|i| c.assets.groups().nth(i))
        .ok_or_else(|| invalid("unknown asset group id: use the id ponko_ctx_start_asset_group gave"))
}

/// An empty asset group called `name` (copied). Null on bad arguments
/// (see `ponko_last_error`).
///
/// Ownership: the returned group is the caller's (null on failure); free it
/// with ponko_asset_group_free. ponko_ctx_start_asset_group takes it over
/// instead.
///
/// ```
/// assert!(unsafe { ponko_ffi::ponko_asset_group_new(std::ptr::null()) }.is_null());
/// ```
/// # Safety
/// `name` null or NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_asset_group_new(name: *const c_char) -> *mut PonkoAssetGroup {
    let mut result = std::ptr::null_mut();
    boundary(|| {
        // SAFETY: caller supplies a NUL-terminated name.
        let name = unsafe { text(name)? };
        result = Box::into_raw(Box::new(PonkoAssetGroup(Some(AssetGroup::new(name)))));
        Ok(())
    });
    result
}

/// Frees a group that wasn't started; null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_asset_group_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// `group` null or from `ponko_asset_group_new`, not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_asset_group_free(group: *mut PonkoAssetGroup) {
    if !group.is_null() {
        // SAFETY: caller passes an owned handle, once.
        drop(unsafe { Box::from_raw(group) });
    }
}

/// How `ponko_asset_group_add` adds an asset; pass it as a `uint32_t`.
/// ```
/// assert_eq!(ponko_ffi::PonkoAssetGroupKind::PONKO_GROUP_BYTES as u32, 4);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoAssetGroupKind {
    /// A texture as `ponko_ctx_load_texture` loads it, which then returns it.
    PONKO_GROUP_TEXTURE = 0,
    /// A texture streamed under the texture residency budget
    /// (`ponko_ctx_stream_texture`).
    PONKO_GROUP_STREAMED_TEXTURE = 1,
    /// A mesh file streamed under the mesh residency budget
    /// (`ponko_ctx_load_mesh` then returns it).
    PONKO_GROUP_MESH = 2,
    /// A sound, decoded on a worker (`ponko_ctx_load_sound` then returns it).
    PONKO_GROUP_SOUND = 3,
    /// The file's bytes, for your own decoder
    /// (`ponko_ctx_take_asset_group_bytes`).
    PONKO_GROUP_BYTES = 4,
}

/// Adds asset `path` (copied) to `group`, loaded as `kind` (a
/// `PonkoAssetGroupKind`) says.
/// ```
/// let group = unsafe { ponko_ffi::ponko_asset_group_new(c"level".as_ptr()) };
/// assert_eq!(unsafe { ponko_ffi::ponko_asset_group_add(group, 0, c"tiles.png".as_ptr()) } as u32, 0);
/// assert_eq!(unsafe { ponko_ffi::ponko_asset_group_add(group, 99, c"x".as_ptr()) } as u32, 1);
/// unsafe { ponko_ffi::ponko_asset_group_free(group) };
/// ```
/// # Safety
/// `group` null or live; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_asset_group_add(
    group: *mut PonkoAssetGroup,
    kind: u32,
    path: *const c_char,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller supplies a live handle and a NUL-terminated path.
        let group = unsafe { group.as_mut() }.ok_or_else(|| invalid("asset group is null"))?;
        let path = unsafe { text(path)? };
        let declared = group
            .0
            .take()
            .ok_or_else(|| invalid("asset group was already started"))?;
        let (declared, result) = match kind {
            0 => (declared.texture(path), Ok(())),
            1 => (declared.streamed_texture(path), Ok(())),
            2 => (declared.mesh(path), Ok(())),
            3 => (declared.sound(path), Ok(())),
            4 => (declared.bytes(path), Ok(())),
            _ => (declared, Err(invalid("unknown asset group kind"))),
        };
        group.0 = Some(declared);
        result
    })
}

/// Starts loading `group` in the background (Rust's
/// `Assets::start_group`) and writes its id (numbered from 1) to `*out_id`.
/// The engine takes the group: don't free or use it afterwards (also when
/// this fails).
///
/// Ownership: `group` is taken over (also on failure): the engine frees it;
/// don't use or free it after.
///
/// ```
/// let mut id = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_start_asset_group(std::ptr::null_mut(), std::ptr::null_mut(), &mut id) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `group` null or from `ponko_asset_group_new`; output writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_start_asset_group(
    ctx: *mut PonkoCtx,
    group: *mut PonkoAssetGroup,
    out_id: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        if group.is_null() {
            return Err(invalid("asset group is null"));
        }
        // SAFETY: caller passes an owned handle, once.
        let group = unsafe { Box::from_raw(group) };
        let declared = group.0.ok_or_else(|| invalid("asset group was already started"))?;
        // SAFETY: forwarded caller contract.
        unsafe { ctx_access(ctx, |c| out(out_id, c.assets.start_group(declared).index())) }
    })
}

/// The id of the last group started with `name`, or 0 if none
/// (Rust's `Assets::group`).
/// ```
/// let mut id = 7;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_find_asset_group(std::ptr::null_mut(), c"level".as_ptr(), &mut id) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `name` NUL-terminated; output writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_find_asset_group(
    ctx: *mut PonkoCtx,
    name: *const c_char,
    out_id: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let name = text(name)?;
            out(out_id, c.assets.group(name).map_or(0, GroupId::index))
        })
    })
}

/// Where group `id` is: a `PonkoAssetGroupState` in `*out_state`.
/// ```
/// let mut state = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_group_state(std::ptr::null_mut(), 1, &mut state) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; output writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_group_state(ctx: *mut PonkoCtx, id: u32, out_state: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            out(out_state, state_to_c(c.assets.group_state(id)))
        })
    })
}

/// How far group `id` got, in `*out_progress`.
/// ```
/// let mut p = ponko_ffi::PonkoAssetGroupProgress::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_group_progress(std::ptr::null_mut(), 1, &mut p) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; output writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_group_progress(
    ctx: *mut PonkoCtx,
    id: u32,
    out_progress: *mut PonkoAssetGroupProgress,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            out(out_progress, c.assets.group_progress(id).into())
        })
    })
}

/// Why group `id` failed: `PONKO_OK` if it didn't; otherwise the status of
/// its first error (`PONKO_NOT_FOUND` for a missing file, else
/// `PONKO_INVALID_ARGUMENT`), with `ponko_last_error()`,
/// `ponko_last_error_kind()` and `ponko_last_error_path()` (the failing
/// asset) set as any failed load sets them.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_group_error(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_group_error(ctx: *mut PonkoCtx, id: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            match c.assets.group_error(id) {
                Some(error) => Err(engine_error(error.clone())),
                None => Ok(()),
            }
        })
    })
}

/// Stops loading group `id` (Rust's `Assets::cancel_group`): what loaded
/// stays loaded; nothing more is read. A failed group is cancelled too,
/// dropping what it kept for a restart.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_cancel_asset_group(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_cancel_asset_group(ctx: *mut PonkoCtx, id: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            c.assets.cancel_group(id);
            Ok(())
        })
    })
}

/// Loads the rest of a failed or cancelled group `id` (Rust's
/// `Assets::restart_group`): only the assets that had not loaded are read
/// again. Does nothing to a group that is loading or ready.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_restart_asset_group(std::ptr::null_mut(), 1) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_restart_asset_group(ctx: *mut PonkoCtx, id: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let id = group_id(c, id)?;
            c.assets.restart_group(id);
            Ok(())
        })
    })
}

/// Hands the bytes of asset `path` of group `id` (added as
/// `PONKO_GROUP_BYTES`) to `decode`, once: they are taken, so a second call
/// is `PONKO_NOT_FOUND`, as is a call before the asset loaded. A decoder
/// that returns anything but `PONKO_OK` makes this `PONKO_INVALID_ARGUMENT`
/// (`PONKO_ERROR_DECODE`). `decode` runs outside the engine's borrow of the
/// context, so it may call `ponko_ctx_*`.
///
/// Ownership: `user` stays the caller's and is only passed to `decode` during
/// this call; the engine keeps no pointer to it after returning.
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_take_asset_group_bytes(std::ptr::null_mut(), 1, c"a.bin".as_ptr(), None, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` NUL-terminated; `decode` takes `user`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_take_asset_group_bytes(
    ctx: *mut PonkoCtx,
    id: u32,
    path: *const c_char,
    decode: PonkoAssetDecodeFn,
    user: *mut c_void,
) -> PonkoStatus {
    boundary(|| {
        let decode = decode.ok_or_else(|| invalid("decode callback is null"))?;
        // SAFETY: caller supplies a NUL-terminated path.
        let path = unsafe { text(path)? };
        // SAFETY: forwarded caller contract; the borrow ends before `decode`.
        let bytes = unsafe {
            ctx_access(ctx, |c| {
                let id = group_id(c, id)?;
                c.assets.take_group_value::<Vec<u8>>(id, path).ok_or_else(|| {
                    engine_error(
                        ponko::prelude::Error::new(
                            ponko::prelude::ErrorKind::NotFound,
                            format!("asset group {} has no bytes for \"{path}\" (not loaded yet, already taken, or not added as PONKO_GROUP_BYTES)", id.index()),
                        )
                        .with_path(path),
                    )
                })
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
                format!("\"{path}\" could not be decoded: the decoder returned status {status}"),
            )
            .with_path(path),
        ))
    })
}

/// Sets how long the game thread may spend on asset groups each frame, in
/// milliseconds (default 4; Rust's `Assets::set_group_frame_budget`). A
/// negative or non-finite value is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_asset_group_frame_budget(std::ptr::null_mut(), 2.0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_asset_group_frame_budget(ctx: *mut PonkoCtx, milliseconds: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if !(milliseconds.is_finite() && milliseconds >= 0.0) {
                return Err(invalid("the budget must be 0 or more milliseconds"));
            }
            c.assets.set_group_frame_budget(milliseconds);
            Ok(())
        })
    })
}

/// What the game thread's asset group step cost, in `*out_stats`.
/// ```
/// let mut s = ponko_ffi::PonkoAssetGroupStats::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_asset_group_stats(std::ptr::null_mut(), &mut s) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; output writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_asset_group_stats(
    ctx: *mut PonkoCtx,
    out_stats: *mut PonkoAssetGroupStats,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_stats, c.assets.group_stats().into())) })
}

/// Adds a copy of `source` as a background source (Rust's
/// `Assets::add_background_source`): asset groups and streaming read it on
/// worker threads. The handle stays yours. A source id this game already
/// has is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_add_background_memory_source(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `source` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_add_background_memory_source(
    ctx: *mut PonkoCtx,
    source: *const PonkoMemorySource,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let source = source.as_ref().ok_or_else(|| invalid("memory source is null"))?;
        ctx_access(ctx, |c| {
            let id = ponko::assets::AssetSource::id(source.source());
            if c.assets.has_source(id) {
                return Err(invalid_owned(format!(
                    "an asset source called \"{id}\" was already added"
                )));
            }
            c.assets.add_background_source(source.source().clone());
            Ok(())
        })
    })
}

/// Fills a new asset group for `ponko_game_loading_state`: add its assets
/// with `ponko_asset_group_add` (don't start or free it). `ctx` and
/// `group` are valid until it returns.
/// ```
/// let none: ponko_ffi::PonkoAssetGroupBuildFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoAssetGroupBuildFn =
    Option<unsafe extern "C" fn(ctx: *mut PonkoCtx, group: *mut PonkoAssetGroup, user: *mut c_void)>;

/// Makes state `loading` a loading screen (Rust's `Game::loading_state`):
/// entering it starts a group called `name` (copied), filled by `build`;
/// once that group is ready the game moves to state `then`. Leaving
/// `loading` before then cancels the group; a failed group keeps the game
/// in `loading`. Find the group with `ponko_ctx_find_asset_group(name)`
/// to draw its progress. Without `ponko_game_state` first:
/// `PONKO_INVALID_ARGUMENT`.
///
/// Ownership: `user` stays the caller's; the engine passes it to `build_fn`
/// each time the state is entered and never frees it. It must stay valid until
/// the game is freed (or its run returns).
///
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_loading_state(std::ptr::null_mut(), 1, c"level".as_ptr(), None, std::ptr::null_mut(), 2) } as u32, 1);
/// ```
/// # Safety
/// Live game; `name` NUL-terminated; `build` and `user` valid while the game runs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_loading_state(
    game: *mut PonkoGame,
    loading: u32,
    name: *const c_char,
    build_fn: PonkoAssetGroupBuildFn,
    user: *mut c_void,
    then: u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        build(game, |g| {
            let Some(f) = build_fn else {
                return (g, Err(invalid("build callback is null")));
            };
            let name = match text(name) {
                Ok(name) => name.to_owned(),
                Err(e) => return (g, Err(e)),
            };
            if !g.has_state::<u32>() {
                return (
                    g,
                    Err(invalid("the game has no state machine: call ponko_game_state first")),
                );
            }
            let make = move |ctx: &mut ponko::prelude::Ctx| {
                let mut group = PonkoAssetGroup(Some(AssetGroup::new(name.as_str())));
                // SAFETY: the caller promised `f` and `user` stay valid while the game runs.
                with_ctx(ctx, |c| f(c, &mut group, user));
                group.0.unwrap_or_else(|| AssetGroup::new(name.as_str()))
            };
            ok(g.loading_state(loading, make, then))
        })
    }
}
