//! Video from C: the test clips (64 x 64, 10 frames a second, 2 seconds;
//! a gray quarter whose level says which frame it is) loaded, played,
//! sought, looped and freed through the C functions, in a headless setup.
use std::ffi::c_void;
use std::ptr::null_mut;

use PonkoStatus::*;
use ponko_ffi::*;

/// Runs `f` with a ctx, in the setup of a headless game.
fn with_ctx(f: fn(*mut PonkoCtx)) {
    unsafe extern "C" fn setup(ctx: *mut PonkoCtx, user: *mut c_void) {
        let f: fn(*mut PonkoCtx) = unsafe { std::mem::transmute(user) };
        f(ctx);
    }
    unsafe {
        let game = ponko_game_new(c"video".as_ptr(), 64, 64);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 0, Some(setup), f as *mut c_void, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
}

#[cfg(feature = "video")]
mod with_the_feature {
    use super::*;
    use std::ptr::null;

    const CLIP: &[u8] = include_bytes!("../../../tests/fixtures/video/clip.webm");
    const SILENT: &[u8] = include_bytes!("../../../tests/fixtures/video/silent.webm");
    const VP9: &[u8] = include_bytes!("../../../tests/fixtures/video/vp9.webm");

    unsafe fn from(ctx: *mut PonkoCtx, bytes: &[u8]) -> (PonkoStatus, *mut PonkoVideo) {
        let mut v = null_mut();
        let s = unsafe { ponko_ctx_video_from_bytes(ctx, bytes.as_ptr().cast(), bytes.len(), &mut v) };
        (s, v)
    }

    fn state(v: *const PonkoVideo) -> PonkoVideoState {
        let mut s: PonkoVideoState = unsafe { std::mem::zeroed() };
        s.struct_size = size_of::<PonkoVideoState>() as u32;
        s.version = PONKO_VIDEO_STATE_VERSION;
        assert_eq!(unsafe { ponko_video_state(v, &mut s) }, PONKO_OK);
        s
    }

    /// Which frame is shown, from the gray quarter's level.
    fn frame_number(v: *const PonkoVideo) -> i32 {
        let (mut p, mut n) = (null(), 0usize);
        assert_eq!(unsafe { ponko_video_frame_rgba(v, &mut p, &mut n) }, PONKO_OK);
        assert_eq!(n, 64 * 64 * 4);
        let rgba = unsafe { std::slice::from_raw_parts(p, n) };
        let level = i32::from(rgba[(48 * 64 + 48) * 4]);
        (level - 20 + 5).div_euclid(10)
    }

    fn live(ctx: *mut PonkoCtx) -> (usize, usize) {
        let mut c: PonkoLiveCounts = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { ponko_ctx_live_counts(ctx, &mut c) }, PONKO_OK);
        (c.textures, c.sounds)
    }

    #[test]
    fn a_c_video_plays_seeks_and_stops_on_its_last_frame() {
        with_ctx(|ctx| unsafe {
            let before = live(ctx);
            let (s, v) = from(ctx, CLIP);
            assert_eq!(s, PONKO_OK);
            let st = state(v);
            assert_eq!((st.width, st.height), (64, 64));
            assert_eq!((st.texture.width, st.texture.height), (64, 64));
            assert!((st.seconds - 2.0).abs() < 0.05, "{}", st.seconds);
            assert!(!st.playing && !st.finished && !st.looping && st.has_sound);
            assert_eq!((st.time, st.frame_time, st.volume), (0.0, 0.0, 1.0));
            assert_eq!(frame_number(v), 0);
            // Paused, advancing does nothing.
            assert_eq!(ponko_ctx_video_advance(ctx, v, 0.5), PONKO_OK);
            assert_eq!(frame_number(v), 0);

            assert_eq!(ponko_ctx_video_play(ctx, v), PONKO_OK);
            assert_eq!(ponko_ctx_video_advance(ctx, v, 0.55), PONKO_OK);
            let st = state(v);
            assert!(st.playing && (st.time - 0.55).abs() < 1e-4 && (st.frame_time - 0.5).abs() < 1e-4);
            assert_eq!(frame_number(v), 5);

            assert_eq!(ponko_ctx_video_seek(ctx, v, 1.25), PONKO_OK);
            assert_eq!(frame_number(v), 12);
            assert_eq!(ponko_ctx_video_pause(ctx, v), PONKO_OK);
            assert!(!state(v).playing);

            // Played to the end: it stops on the last frame; play starts over.
            assert_eq!(ponko_ctx_video_play(ctx, v), PONKO_OK);
            assert_eq!(ponko_ctx_video_advance(ctx, v, 5.0), PONKO_OK);
            let st = state(v);
            assert!(st.finished && !st.playing);
            assert_eq!(frame_number(v), 19);
            assert_eq!(ponko_ctx_video_play(ctx, v), PONKO_OK);
            assert_eq!(frame_number(v), 0);
            assert_eq!(state(v).time, 0.0);

            // update moves it by the frame's time: a headless setup has none.
            assert_eq!(ponko_ctx_video_update(ctx, v), PONKO_OK);

            assert_eq!(ponko_ctx_free_video(ctx, v), PONKO_OK);
            assert_eq!(live(ctx), before, "its texture and sound are freed");
        });
    }

    #[test]
    fn a_looping_c_video_starts_over_and_keeps_playing() {
        with_ctx(|ctx| unsafe {
            let (_, v) = from(ctx, CLIP);
            assert_eq!(ponko_video_set_looping(v, true), PONKO_OK);
            assert_eq!(ponko_ctx_video_play(ctx, v), PONKO_OK);
            assert_eq!(ponko_ctx_video_advance(ctx, v, 2.35), PONKO_OK);
            let st = state(v);
            assert!(st.looping && st.playing && !st.finished);
            assert!((st.time - 0.35).abs() < 0.06, "{}", st.time);
            assert_eq!(frame_number(v), 3);
            assert_eq!(ponko_ctx_free_video(ctx, v), PONKO_OK);
        });
    }

    #[test]
    fn a_c_videos_sound_takes_its_bus_and_volume() {
        with_ctx(|ctx| unsafe {
            let (_, v) = from(ctx, CLIP);
            let mut bus = PonkoAudioBus::default();
            assert_eq!(ponko_ctx_audio_bus(ctx, c"cutscene".as_ptr(), &mut bus), PONKO_OK);
            assert_eq!(ponko_ctx_video_set_bus(ctx, v, bus), PONKO_OK);
            assert_eq!(ponko_ctx_video_set_volume(ctx, v, 0.5), PONKO_OK);
            let st = state(v);
            assert_eq!((st.bus, st.volume), (bus, 0.5));
            assert!(st.has_sound && (st.sound.seconds - 2.0).abs() < 0.05);
            assert_eq!(ponko_ctx_video_set_volume(ctx, v, f32::NAN), PONKO_OK);
            assert_eq!(state(v).volume, 1.0, "a volume that isn't a number is 1");
            assert_eq!(ponko_ctx_free_video(ctx, v), PONKO_OK);

            let (s, quiet) = from(ctx, SILENT);
            assert_eq!(s, PONKO_OK);
            assert!(!state(quiet).has_sound);
            assert_eq!(ponko_ctx_free_video(ctx, quiet), PONKO_OK);
        });
    }

    #[test]
    fn c_videos_load_from_asset_sources_and_bad_files_are_errors() {
        with_ctx(|ctx| unsafe {
            let source = ponko_memory_source_new(c"films".as_ptr());
            assert_eq!(
                ponko_memory_source_insert(source, c"intro.webm".as_ptr(), CLIP.as_ptr(), CLIP.len()),
                PONKO_OK
            );
            assert_eq!(ponko_ctx_add_memory_source(ctx, source), PONKO_OK);
            ponko_memory_source_free(source);
            let mut v = null_mut();
            assert_eq!(ponko_ctx_load_video(ctx, c"intro.webm".as_ptr(), &mut v), PONKO_OK);
            assert_eq!(state(v).width, 64);
            assert_eq!(ponko_ctx_free_video(ctx, v), PONKO_OK);

            let mut none = null_mut();
            assert_eq!(
                ponko_ctx_load_video(ctx, c"missing.webm".as_ptr(), &mut none),
                PONKO_NOT_FOUND
            );
            assert_eq!(from(ctx, VP9).0, PONKO_UNSUPPORTED, "VP9 pictures are refused");
            assert_eq!(from(ctx, &CLIP[..CLIP.len() / 3]).0, PONKO_INVALID_ARGUMENT);
            assert_eq!(from(ctx, b"not a video").0, PONKO_INVALID_ARGUMENT);
            assert!(none.is_null());
            assert_eq!(ponko_ctx_free_video(ctx, null_mut()), PONKO_OK);
            assert_eq!(ponko_video_state(null(), null_mut()), PONKO_INVALID_ARGUMENT);
        });
    }
}

/// Without the feature, loading reports that the build has no video.
#[cfg(not(feature = "video"))]
#[test]
fn without_the_feature_videos_are_unsupported() {
    with_ctx(|ctx| unsafe {
        let mut v = null_mut();
        let bytes = [0u8; 16];
        assert_eq!(
            ponko_ctx_video_from_bytes(ctx, bytes.as_ptr().cast(), 16, &mut v),
            PONKO_UNSUPPORTED
        );
        assert_eq!(
            ponko_ctx_load_video(ctx, c"intro.webm".as_ptr(), &mut v),
            PONKO_UNSUPPORTED
        );
        assert!(v.is_null());
    });
}
