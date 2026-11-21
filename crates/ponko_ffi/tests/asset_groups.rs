//! Asset groups from C: declare, start, progress, failure with the
//! failing path, cancel, the bytes of your own formats, and a loading
//! state.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::ptr::null_mut;

use PonkoErrorKind::*;
use PonkoStatus::*;

/// A background memory source with `files`.
unsafe fn add_files(ctx: *mut PonkoCtx, id: &CStr, files: &[(&CStr, &[u8])]) {
    unsafe {
        let source = ponko_memory_source_new(id.as_ptr());
        for (name, bytes) in files {
            assert_eq!(
                ponko_memory_source_insert(source, name.as_ptr(), bytes.as_ptr(), bytes.len()),
                PONKO_OK
            );
        }
        assert_eq!(ponko_ctx_add_background_memory_source(ctx, source), PONKO_OK);
        assert_eq!(
            ponko_ctx_add_background_memory_source(ctx, source),
            PONKO_INVALID_ARGUMENT,
            "an id twice"
        );
        ponko_memory_source_free(source);
    }
}

unsafe fn state(ctx: *mut PonkoCtx, id: u32) -> u32 {
    let mut state = 99;
    assert_eq!(unsafe { ponko_ctx_asset_group_state(ctx, id, &mut state) }, PONKO_OK);
    state
}

unsafe extern "C" fn keep(user: *mut c_void, data: *const u8, len: usize) -> u32 {
    let out = unsafe { &mut *user.cast::<Vec<u8>>() };
    out.extend_from_slice(unsafe { std::slice::from_raw_parts(data, len) });
    PONKO_OK as u32
}

unsafe extern "C" fn refuse(_: *mut c_void, _: *const u8, _: usize) -> u32 {
    PONKO_INVALID_ARGUMENT as u32
}

#[test]
fn a_c_group_loads_in_the_background_with_progress() {
    run_ctx(|ctx| unsafe {
        add_files(
            ctx,
            c"level",
            &[
                (c"level.bin", &[1, 2, 3, 4]),
                (c"tiles.png", &PIXEL_PNG),
                (c"rules.json", b"{}"),
            ],
        );
        let group = ponko_asset_group_new(c"level".as_ptr());
        assert!(!group.is_null());
        for (kind, path) in [
            (PonkoAssetGroupKind::PONKO_GROUP_BYTES, c"level.bin"),
            (PonkoAssetGroupKind::PONKO_GROUP_TEXTURE, c"tiles.png"),
            (PonkoAssetGroupKind::PONKO_GROUP_BYTES, c"rules.json"),
        ] {
            assert_eq!(ponko_asset_group_add(group, kind as u32, path.as_ptr()), PONKO_OK);
        }
        assert_eq!(ponko_asset_group_add(group, 42, c"x".as_ptr()), PONKO_INVALID_ARGUMENT);
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(id, 1);
        let mut found = 0;
        assert_eq!(ponko_ctx_find_asset_group(ctx, c"level".as_ptr(), &mut found), PONKO_OK);
        assert_eq!(found, id);
        assert_eq!(ponko_ctx_find_asset_group(ctx, c"other".as_ptr(), &mut found), PONKO_OK);
        assert_eq!(found, 0);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_LOADING as u32);
        assert_eq!(
            ponko_ctx_set_asset_group_frame_budget(ctx, -1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_set_asset_group_frame_budget(ctx, 3.0), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_READY as u32);
        let mut progress = PonkoAssetGroupProgress::default();
        assert_eq!(ponko_ctx_asset_group_progress(ctx, id, &mut progress), PONKO_OK);
        assert_eq!((progress.loaded, progress.total), (3, 3));
        assert_eq!(progress.bytes_total, 4 + PIXEL_PNG.len() as u64 + 2);
        assert!(progress.bytes_known && progress.fraction == 1.0);
        assert_eq!(ponko_ctx_asset_group_error(ctx, id), PONKO_OK, "it didn't fail");
        let mut bytes: Vec<u8> = Vec::new();
        assert_eq!(
            ponko_ctx_take_asset_group_bytes(ctx, id, c"level.bin".as_ptr(), Some(keep), (&raw mut bytes).cast()),
            PONKO_OK
        );
        assert_eq!(bytes, [1, 2, 3, 4]);
        assert_eq!(
            ponko_ctx_take_asset_group_bytes(ctx, id, c"level.bin".as_ptr(), Some(keep), (&raw mut bytes).cast()),
            PONKO_NOT_FOUND,
            "taken once"
        );
        assert_eq!(
            ponko_ctx_take_asset_group_bytes(ctx, id, c"rules.json".as_ptr(), Some(refuse), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_DECODE);
        // The texture is what ponko_ctx_load_texture now returns.
        let mut texture = std::mem::zeroed::<PonkoTexture>();
        assert_eq!(
            ponko_ctx_load_texture(ctx, c"tiles.png".as_ptr(), &mut texture),
            PONKO_OK
        );
        let mut stats = PonkoAssetGroupStats::default();
        assert_eq!(ponko_ctx_asset_group_stats(ctx, &mut stats), PONKO_OK);
        assert_eq!(stats.frame_budget_ms, 3.0);
        assert!(stats.steps > 0);
    });
}

#[test]
fn a_failed_c_group_names_the_failing_path_and_bad_ids_are_refused() {
    run_ctx(|ctx| unsafe {
        let group = ponko_asset_group_new(c"broken".as_ptr());
        assert_eq!(
            ponko_asset_group_add(
                group,
                PonkoAssetGroupKind::PONKO_GROUP_SOUND as u32,
                c"sfx/none.wav".as_ptr()
            ),
            PONKO_OK
        );
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_FAILED as u32);
        assert_eq!(ponko_ctx_asset_group_error(ctx, id), PONKO_NOT_FOUND);
        assert_eq!(ponko_last_error_kind(), PONKO_ERROR_NOT_FOUND);
        assert_eq!(last_path(), "sfx/none.wav");
        let mut s = 0;
        for bad in [0, id + 1] {
            assert_eq!(ponko_ctx_asset_group_state(ctx, bad, &mut s), PONKO_INVALID_ARGUMENT);
        }
        assert!(last_error().contains("unknown asset group id"), "{}", last_error());
        assert_eq!(
            ponko_ctx_start_asset_group(ctx, null_mut(), &mut id),
            PONKO_INVALID_ARGUMENT
        );
        assert!(ponko_asset_group_new(std::ptr::null()).is_null());
    });
}

#[test]
fn a_cancelled_c_group_stays_cancelled() {
    run_ctx(|ctx| unsafe {
        add_files(ctx, c"mem", &[(c"a.bin", b"a")]);
        let group = ponko_asset_group_new(c"one".as_ptr());
        assert_eq!(
            ponko_asset_group_add(group, PonkoAssetGroupKind::PONKO_GROUP_BYTES as u32, c"a.bin".as_ptr()),
            PONKO_OK
        );
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(ponko_ctx_cancel_asset_group(ctx, id), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_CANCELLED as u32);
        // An unstarted group is freed, not started.
        ponko_asset_group_free(ponko_asset_group_new(c"unused".as_ptr()));
    });
}

#[test]
fn a_failed_c_group_restarts_and_loads_only_what_failed() {
    run_ctx(|ctx| unsafe {
        add_files(ctx, c"first", &[(c"a.bin", b"a"), (c"tiles.png", &PIXEL_PNG)]);
        let group = ponko_asset_group_new(c"level".as_ptr());
        for (kind, path) in [
            (PonkoAssetGroupKind::PONKO_GROUP_BYTES, c"a.bin"),
            (PonkoAssetGroupKind::PONKO_GROUP_TEXTURE, c"tiles.png"),
            (PonkoAssetGroupKind::PONKO_GROUP_BYTES, c"late.bin"),
        ] {
            assert_eq!(ponko_asset_group_add(group, kind as u32, path.as_ptr()), PONKO_OK);
        }
        let mut id = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut id), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_FAILED as u32);
        assert_eq!(last_path_of_group(ctx, id), "late.bin");
        let mut progress = PonkoAssetGroupProgress::default();
        assert_eq!(ponko_ctx_asset_group_progress(ctx, id, &mut progress), PONKO_OK);
        assert_eq!(progress.loaded, 2);
        assert_eq!(progress.bytes_uploaded, 4, "one 1 x 1 RGBA texture");

        // The missing file arrives; the restart reads only it.
        add_files(ctx, c"second", &[(c"late.bin", b"late")]);
        assert_eq!(ponko_ctx_restart_asset_group(ctx, id), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_LOADING as u32);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_READY as u32);
        let mut bytes: Vec<u8> = Vec::new();
        for path in [c"a.bin", c"late.bin"] {
            assert_eq!(
                ponko_ctx_take_asset_group_bytes(ctx, id, path.as_ptr(), Some(keep), (&raw mut bytes).cast()),
                PONKO_OK
            );
        }
        assert_eq!(bytes, b"alate");
        let mut stats = PonkoAssetGroupStats::default();
        assert_eq!(ponko_ctx_asset_group_stats(ctx, &mut stats), PONKO_OK);
        assert_eq!(stats.total_upload_bytes, 4);
        assert!(stats.total_upload_ms >= 0.0 && stats.max_upload_ms >= stats.upload_ms);
        // Restarting a ready group does nothing; a bad id is refused.
        assert_eq!(ponko_ctx_restart_asset_group(ctx, id), PONKO_OK);
        assert_eq!(state(ctx, id), PonkoAssetGroupState::PONKO_GROUP_READY as u32);
        assert_eq!(ponko_ctx_restart_asset_group(ctx, id + 1), PONKO_INVALID_ARGUMENT);
        // Cancelling a failed group makes it cancelled.
        let group = ponko_asset_group_new(c"broken".as_ptr());
        assert_eq!(
            ponko_asset_group_add(
                group,
                PonkoAssetGroupKind::PONKO_GROUP_BYTES as u32,
                c"none.bin".as_ptr()
            ),
            PONKO_OK
        );
        let mut broken = 0;
        assert_eq!(ponko_ctx_start_asset_group(ctx, group, &mut broken), PONKO_OK);
        assert_eq!(ponko_ctx_finish_uploads(ctx), PONKO_OK);
        assert_eq!(state(ctx, broken), PonkoAssetGroupState::PONKO_GROUP_FAILED as u32);
        assert_eq!(ponko_ctx_cancel_asset_group(ctx, broken), PONKO_OK);
        assert_eq!(state(ctx, broken), PonkoAssetGroupState::PONKO_GROUP_CANCELLED as u32);
    });
}

/// The path group `id`'s error names.
unsafe fn last_path_of_group(ctx: *mut PonkoCtx, id: u32) -> String {
    assert_ne!(unsafe { ponko_ctx_asset_group_error(ctx, id) }, PONKO_OK);
    last_path()
}

const LOADING: u32 = 1;
const PLAYING: u32 = 2;

unsafe extern "C" fn level(_: *mut PonkoCtx, group: *mut PonkoAssetGroup, _: *mut c_void) {
    let kind = PonkoAssetGroupKind::PONKO_GROUP_BYTES as u32;
    assert_eq!(
        unsafe { ponko_asset_group_add(group, kind, c"level.bin".as_ptr()) },
        PONKO_OK
    );
}

unsafe extern "C" fn files(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe { add_files(ctx, c"mem", &[(c"level.bin", &[9; 16])]) };
}

unsafe extern "C" fn record(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Vec<u32>>() };
    let mut s = 0;
    assert_eq!(unsafe { ponko_ctx_state(ctx, &mut s) }, PONKO_OK);
    seen.push(s);
    // A frame takes a little time, so the worker gets to read.
    std::thread::sleep(std::time::Duration::from_millis(2));
}

#[test]
fn a_c_loading_state_leaves_when_its_group_is_ready() {
    let mut seen: Vec<u32> = Vec::new();
    let mut world = null_mut();
    unsafe {
        let g = new_game();
        assert_eq!(
            ponko_game_loading_state(g, LOADING, c"level".as_ptr(), Some(level), null_mut(), PLAYING),
            PONKO_INVALID_ARGUMENT,
            "no state machine yet"
        );
        assert_eq!(ponko_game_state(g, LOADING), PONKO_OK);
        assert_eq!(
            ponko_game_loading_state(g, LOADING, c"level".as_ptr(), None, null_mut(), PLAYING),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_game_loading_state(g, LOADING, c"level".as_ptr(), Some(level), null_mut(), PLAYING),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_sim_system(g, c"record".as_ptr(), 1, Some(record), (&raw mut seen).cast()),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_run_headless(g, 200, Some(files), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert_eq!(seen.first(), Some(&LOADING));
    assert_eq!(seen.last(), Some(&PLAYING), "{seen:?}");
}
