//! `ponko_game_restore_window_settings`: a game opens with the window
//! settings the player saved last time, before setup runs.
mod common;
use common::last_error;
use ponko_ffi::*;
use std::ffi::CString;

use PonkoStatus::*;

fn size_seen(dir: &CString, restore: Option<bool>, change: Option<PonkoVec2>) -> PonkoVec2 {
    let mut seen = PonkoVec2::default();
    unsafe {
        let game = ponko_game_new(c"restore".as_ptr(), 800, 600);
        assert_eq!(ponko_game_hot_reload(game, false), PONKO_OK);
        assert_eq!(ponko_game_data_dir(game, dir.as_ptr()), PONKO_OK);
        if let Some(on) = restore {
            assert_eq!(ponko_game_restore_window_settings(game, on), PONKO_OK);
        }
        let world = common::run_game(game, 0, |ctx| {
            let mut props = PonkoWindowPropertiesEx::default();
            assert_eq!(
                ponko_ctx_window_properties_ex(ctx, &mut props),
                PONKO_OK,
                "{}",
                last_error()
            );
            seen = props.size;
            if let Some(size) = change {
                assert_eq!(ponko_ctx_window_set_size(ctx, size), PONKO_OK);
                assert_eq!(ponko_ctx_window_save_settings(ctx), PONKO_OK, "{}", last_error());
            }
        });
        ponko_world_free(world);
    }
    seen
}

#[test]
fn the_saved_window_settings_are_there_before_setup_when_restored() {
    let dir = std::env::temp_dir().join(format!("ponko-ffi-restore-window-{}", std::process::id()));
    let c_dir = CString::new(dir.to_string_lossy().as_bytes()).unwrap();
    let big = PonkoVec2 { x: 1024.0, y: 768.0 };
    // The first run saves the player's choice.
    let first = size_seen(&c_dir, None, Some(big));
    assert_eq!((first.x, first.y), (800.0, 600.0));
    // Restored, the next run starts with it; not restored, with the game's size.
    let restored = size_seen(&c_dir, Some(true), None);
    assert_eq!((restored.x, restored.y), (1024.0, 768.0));
    let plain = size_seen(&c_dir, Some(false), None);
    assert_eq!((plain.x, plain.y), (800.0, 600.0));
    let _ = std::fs::remove_dir_all(dir);
    // A null game is refused.
    assert_eq!(
        unsafe { ponko_game_restore_window_settings(std::ptr::null_mut(), true) },
        PONKO_INVALID_ARGUMENT
    );
}
