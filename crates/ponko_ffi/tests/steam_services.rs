//! Steam's store-side services from C without a running Steam: every call
//! does nothing and says false, so one build runs from any store. The
//! services' logic is tested in Rust against the fake (`tests/steam_services.rs`
//! of the engine); `ponko_steam_start` would start the real client and is
//! on the person's checklist.
use ponko_ffi::*;
use std::ffi::{CString, c_void};
use std::ptr::{null, null_mut};

use PonkoStatus::*;

#[test]
fn every_service_does_nothing_before_steam_starts() {
    unsafe {
        assert!(!ponko_steam_running());
        ponko_steam_update();
        assert!(!ponko_steam_unlock_achievement(c"FIRST_WIN".as_ptr()));
        assert!(!ponko_steam_clear_achievement(c"FIRST_WIN".as_ptr()));
        assert_eq!(ponko_steam_achievement(c"FIRST_WIN".as_ptr()), -1);
        assert!(!ponko_steam_unlock_achievement(null()), "null names are refused");
        assert!(!ponko_steam_set_stat_i32(c"wins".as_ptr(), 3));
        assert!(!ponko_steam_set_stat_f32(c"distance".as_ptr(), 1.5));
        let (mut i, mut f) = (7, 7.0);
        assert!(!ponko_steam_stat_i32(c"wins".as_ptr(), &mut i));
        assert!(!ponko_steam_stat_f32(c"distance".as_ptr(), &mut f));
        assert_eq!((i, f), (7, 7.0), "outputs untouched");
        assert!(!ponko_steam_store_stats());
        assert!(!ponko_steam_set_rich_presence(c"status".as_ptr(), c"Menu".as_ptr()));
        assert!(!ponko_steam_set_rich_presence(c"status".as_ptr(), null()));
        ponko_steam_clear_rich_presence();
        assert!(!ponko_steam_overlay_open());
        assert!(!ponko_steam_is_steam_deck());
        assert!(!ponko_steam_is_big_picture());
        assert!(!ponko_steam_show_text_input(0, c"Name".as_ptr(), c"".as_ptr(), 16));
        assert!(!ponko_steam_show_text_input(9, c"Name".as_ptr(), c"".as_ptr(), 16));
        assert!(!ponko_steam_show_floating_keyboard(4, 0, 0, 200, 30));
        assert!(!ponko_steam_cloud_enabled());
    }
}

#[test]
fn no_text_input_result_without_steam_and_outputs_are_checked() {
    unsafe {
        let (mut state, mut len) = (9, 9);
        let mut buf = [1 as std::ffi::c_char; 8];
        assert_eq!(
            ponko_steam_take_text_input(&mut state, buf.as_mut_ptr(), buf.len(), &mut len),
            PONKO_OK
        );
        assert_eq!((state, len, buf[0]), (PONKO_STEAM_TEXT_NONE, 0, 0));
        assert_eq!(
            ponko_steam_take_text_input(null_mut(), null_mut(), 0, &mut len),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_steam_take_text_input(&mut state, null_mut(), 4, &mut len),
            PONKO_INVALID_ARGUMENT
        );
        assert_ne!(PONKO_STEAM_TEXT_SUBMITTED, PONKO_STEAM_TEXT_CANCELLED);
    }
}

fn temp_dir(name: &str) -> CString {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-steam-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    CString::new(dir.to_str().unwrap()).unwrap()
}

unsafe extern "C" fn sync_without_steam(ctx: *mut PonkoCtx, _: *mut c_void) {
    unsafe {
        let bytes = b"{}";
        assert_eq!(
            ponko_ctx_storage_write(ctx, c"saves/a.json".as_ptr(), bytes.as_ptr(), bytes.len()),
            PONKO_OK
        );
        let files = [c"saves/".as_ptr(), c"settings.json".as_ptr()];
        let (mut down, mut up) = (9, 9);
        assert_eq!(
            ponko_ctx_steam_sync_cloud(ctx, files.as_ptr(), files.len(), &mut down, &mut up),
            PONKO_OK
        );
        assert_eq!((down, up), (0, 0), "the folder is the only copy");
        assert_eq!(
            ponko_ctx_steam_sync_cloud(ctx, null(), 1, null_mut(), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_steam_delete_cloud_file(ctx, c"saves/a.json".as_ptr()),
            PONKO_OK
        );
        let mut exists = true;
        assert_eq!(
            ponko_ctx_storage_exists(ctx, c"saves/a.json".as_ptr(), &mut exists),
            PONKO_OK
        );
        assert!(!exists, "the local file is deleted");
        assert_eq!(
            ponko_ctx_steam_delete_cloud_file(ctx, c"../up".as_ptr()),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn cloud_sync_without_steam_keeps_the_storage_folder_as_it_is() {
    let dir = temp_dir("sync");
    unsafe {
        let game = ponko_game_new(c"steam".as_ptr(), 1, 1);
        assert_eq!(ponko_game_data_dir(game, dir.as_ptr()), PONKO_OK);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(sync_without_steam), null_mut(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    let _ = std::fs::remove_dir_all(dir.to_str().unwrap());
}
