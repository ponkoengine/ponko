//! The player's files from C: stored files written safely, read back in the
//! next run, listed and deleted; save slots; a game without an app id.
use ponko_ffi::*;
use std::ffi::{CString, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

/// A fresh, empty folder for one test.
fn temp_dir(name: &str) -> CString {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-storage-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    CString::new(dir.to_str().unwrap()).unwrap()
}

fn run(dir: Option<&CString>, setup: unsafe extern "C" fn(*mut PonkoCtx, *mut c_void), user: *mut c_void) {
    unsafe {
        let game = ponko_game_new(c"storage".as_ptr(), 1, 1);
        if let Some(dir) = dir {
            assert_eq!(ponko_game_data_dir(game, dir.as_ptr()), PONKO_OK);
        }
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

unsafe extern "C" fn write_files(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let old = b"v1";
        let new = b"version 2";
        assert_eq!(
            ponko_ctx_storage_write(ctx, c"saves/a.txt".as_ptr(), old.as_ptr(), old.len()),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_storage_write(ctx, c"saves/a.txt".as_ptr(), new.as_ptr(), new.len()),
            PONKO_OK
        );
        assert_eq!(ponko_ctx_storage_write(ctx, c"empty".as_ptr(), null(), 0), PONKO_OK);
        assert_eq!(ponko_ctx_save_world(ctx, c"slot1".as_ptr()), PONKO_OK);
        // Unsafe names are refused.
        assert_eq!(
            ponko_ctx_storage_write(ctx, c"../up".as_ptr(), old.as_ptr(), old.len()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_INVALID);
    }
}

unsafe extern "C" fn read_files(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        // The size first, then the bytes.
        let mut len = 0;
        assert_eq!(
            ponko_ctx_storage_read(ctx, c"saves/a.txt".as_ptr(), null_mut(), 0, &mut len),
            PONKO_OK
        );
        assert_eq!(len, 9);
        let mut buffer = vec![0u8; len];
        assert_eq!(
            ponko_ctx_storage_read(
                ctx,
                c"saves/a.txt".as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut len
            ),
            PONKO_OK
        );
        assert_eq!(buffer, b"version 2");
        let mut backup = [0u8; 8];
        assert_eq!(
            ponko_ctx_storage_read_backup(ctx, c"saves/a.txt".as_ptr(), backup.as_mut_ptr(), 8, &mut len),
            PONKO_OK
        );
        assert_eq!(&backup[..len], b"v1");
        assert_eq!(
            ponko_ctx_storage_read(ctx, c"missing".as_ptr(), null_mut(), 0, &mut len),
            PONKO_NOT_FOUND
        );

        let mut list = [0 as std::ffi::c_char; 128];
        assert_eq!(
            ponko_ctx_storage_list(ctx, list.as_mut_ptr(), list.len(), &mut len),
            PONKO_OK
        );
        let names = std::ffi::CStr::from_ptr(list.as_ptr()).to_str().unwrap();
        assert_eq!(names, "empty\nsaves/a.txt\nsaves/slot1.json");

        let mut exists = false;
        assert_eq!(ponko_ctx_storage_exists(ctx, c"empty".as_ptr(), &mut exists), PONKO_OK);
        assert!(exists);
        assert_eq!(ponko_ctx_storage_delete(ctx, c"empty".as_ptr()), PONKO_OK);
        assert_eq!(ponko_ctx_storage_exists(ctx, c"empty".as_ptr(), &mut exists), PONKO_OK);
        assert!(!exists);

        assert_eq!(ponko_ctx_storage_dir(ctx, null_mut(), 0, &mut len), PONKO_OK);
        assert!(len > 0, "a folder on the desktop");
        let mut available = false;
        assert_eq!(ponko_ctx_storage_is_available(ctx, &mut available), PONKO_OK);
        assert!(available, "a data folder keeps files");
        assert_eq!(ponko_ctx_storage_is_available(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_load_world(ctx, c"slot1".as_ptr()), PONKO_OK);
        assert_eq!(ponko_ctx_load_world(ctx, c"nothing".as_ptr()), PONKO_NOT_FOUND);
    }
}

#[test]
fn stored_files_written_in_one_run_are_read_back_in_the_next() {
    let dir = temp_dir("runs");
    run(Some(&dir), write_files, null_mut());
    run(Some(&dir), read_files, null_mut());
    let _ = std::fs::remove_dir_all(dir.to_str().unwrap());
}

unsafe extern "C" fn without_app_id(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let mut len = 7;
        assert_eq!(ponko_ctx_storage_dir(ctx, null_mut(), 0, &mut len), PONKO_OK);
        assert_eq!(len, 0, "no folder");
        let mut available = true;
        assert_eq!(ponko_ctx_storage_is_available(ctx, &mut available), PONKO_OK);
        assert!(!available, "no app id, no files");
        assert_eq!(
            ponko_ctx_storage_write(ctx, c"a".as_ptr(), null(), 0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_UNSUPPORTED);
    }
}

#[test]
fn a_game_without_an_app_id_keeps_no_files() {
    run(None, without_app_id, null_mut());
}

#[test]
fn a_bad_app_id_is_refused_and_leaves_the_game_usable() {
    unsafe {
        let game = ponko_game_new(c"storage".as_ptr(), 1, 1);
        assert_eq!(ponko_game_app_id(game, c"../x".as_ptr()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_game_app_id(game, null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_game_app_id(game, c"com.example.crabs".as_ptr()), PONKO_OK);
        assert_eq!(ponko_game_data_dir(game, null()), PONKO_INVALID_ARGUMENT);
        ponko_game_free(game);
    }
}
