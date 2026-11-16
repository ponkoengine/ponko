//! `ponko_game_run` in a build without the `window` feature: the game can't
//! start on any machine, so it returns `PONKO_UNSUPPORTED` with kind
//! `PONKO_ERROR_UNSUPPORTED` instead of opening a window. (With the
//! feature, the same status comes from a missing GPU, device, window or
//! event loop; the mapping is tested in the library's own tests.)
#![cfg(not(feature = "window"))]
use ponko_ffi::*;
use std::ffi::CStr;
use std::ptr::null_mut;

#[test]
fn a_build_without_a_window_reports_the_game_unsupported() {
    unsafe {
        let game = ponko_game_new(c"no window".as_ptr(), 64, 64);
        assert_eq!(ponko_game_run(game, None, null_mut()), PonkoStatus::PONKO_UNSUPPORTED);
        assert_eq!(ponko_last_error_kind(), PonkoErrorKind::PONKO_ERROR_UNSUPPORTED);
        let message = CStr::from_ptr(ponko_last_error()).to_string_lossy();
        assert!(message.contains("window"), "{message}");
    }
}
