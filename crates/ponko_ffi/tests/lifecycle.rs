//! A phone's app lifecycle from C: suspend, resume and low-memory events
//! (`ponko_ctx_window_feed_lifecycle`, read with `ponko_ctx_window_event_ex`),
//! `ponko_ctx_window_suspended`, Back (`ponko_ctx_window_back_requested`,
//! `ponko_ctx_window_cancel_back`), the on-screen keyboard's inset and the
//! audio session category, all as Rust's.
mod common;
use common::*;
use ponko_ffi::*;
use std::ffi::c_char;
use std::ptr::null_mut;

use PonkoStatus::*;

unsafe fn kinds(ctx: *mut PonkoCtx) -> Vec<u32> {
    unsafe {
        let mut n = 0;
        assert_eq!(ponko_ctx_window_event_count(ctx, &mut n), PONKO_OK);
        (0..n)
            .map(|i| {
                let mut e = PonkoWindowEventEx::default();
                let mut path = [0 as c_char; 8];
                let mut len = 0;
                assert_eq!(
                    ponko_ctx_window_event_ex(ctx, i, &mut e, path.as_mut_ptr(), path.len(), &mut len),
                    PONKO_OK
                );
                e.kind
            })
            .collect()
    }
}

#[test]
fn suspend_and_resume_are_events_and_a_state_that_changes_once_each() {
    use PonkoWindowEventKind::*;
    unsafe {
        run_ctx(|ctx| {
            let mut suspended = true;
            assert_eq!(ponko_ctx_window_suspended(ctx, &mut suspended), PONKO_OK);
            assert!(!suspended, "a game starts in front");
            for kind in [PONKO_WINDOW_EVENT_SUSPENDED, PONKO_WINDOW_EVENT_SUSPENDED] {
                assert_eq!(ponko_ctx_window_feed_lifecycle(ctx, kind as u32), PONKO_OK);
            }
            assert_eq!(ponko_ctx_window_suspended(ctx, &mut suspended), PONKO_OK);
            assert!(suspended);
            for kind in [PONKO_WINDOW_EVENT_LOW_MEMORY, PONKO_WINDOW_EVENT_RESUMED] {
                assert_eq!(ponko_ctx_window_feed_lifecycle(ctx, kind as u32), PONKO_OK);
            }
            assert_eq!(ponko_ctx_window_suspended(ctx, &mut suspended), PONKO_OK);
            assert!(!suspended);
            assert_eq!(
                kinds(ctx),
                [5, 7, 6],
                "the repeated suspend sends nothing; the kinds are the enum's"
            );
        });
    }
}

#[test]
fn a_back_press_is_an_event_the_game_can_cancel() {
    unsafe {
        run_ctx(|ctx| {
            let mut back = true;
            assert_eq!(ponko_ctx_window_back_requested(ctx, &mut back), PONKO_OK);
            assert!(!back);
            let kind = PonkoWindowEventKind::PONKO_WINDOW_EVENT_BACK_REQUESTED as u32;
            assert_eq!(ponko_ctx_window_feed_lifecycle(ctx, kind), PONKO_OK);
            assert_eq!(kinds(ctx), [8]);
            assert_eq!(ponko_ctx_window_back_requested(ctx, &mut back), PONKO_OK);
            assert!(back, "the app would leave after this frame");
            assert_eq!(ponko_ctx_window_cancel_back(ctx), PONKO_OK);
            assert_eq!(ponko_ctx_window_back_requested(ctx, &mut back), PONKO_OK);
            assert!(!back, "canceled: the game handles Back");
            assert_eq!(ponko_ctx_window_back_requested(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
        });
        let mut back = false;
        assert_eq!(
            ponko_ctx_window_back_requested(null_mut(), &mut back),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_window_cancel_back(null_mut()), PONKO_INVALID_ARGUMENT);
    }
}

#[test]
fn lifecycle_calls_refuse_other_kinds_null_outputs_and_null_contexts() {
    unsafe {
        run_ctx(|ctx| {
            for kind in [0, 4, 9, u32::MAX] {
                assert_eq!(ponko_ctx_window_feed_lifecycle(ctx, kind), PONKO_INVALID_ARGUMENT);
            }
            assert!(kinds(ctx).is_empty());
            assert_eq!(ponko_ctx_window_suspended(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
            assert_eq!(ponko_ctx_window_keyboard_inset(ctx, null_mut()), PONKO_INVALID_ARGUMENT);
            assert_eq!(
                ponko_ctx_audio_session_category(ctx, null_mut()),
                PONKO_INVALID_ARGUMENT
            );
        });
        let (mut flag, mut inset, mut category) = (false, 0.0, 0);
        assert_eq!(
            ponko_ctx_window_suspended(null_mut(), &mut flag),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_window_keyboard_inset(null_mut(), &mut inset),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_audio_session_category(null_mut(), &mut category),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_ctx_window_feed_lifecycle(null_mut(), 5), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_ctx_window_feed_keyboard_inset(null_mut(), 1.0),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_audio_set_session_category(null_mut(), 0),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[test]
fn the_keyboard_inset_is_what_was_fed_and_nonsense_counts_as_none() {
    unsafe {
        run_ctx(|ctx| {
            let mut inset = -1.0;
            assert_eq!(ponko_ctx_window_keyboard_inset(ctx, &mut inset), PONKO_OK);
            assert_eq!(inset, 0.0, "no keyboard at the start");
            assert_eq!(ponko_ctx_window_feed_keyboard_inset(ctx, 301.0), PONKO_OK);
            assert_eq!(ponko_ctx_window_keyboard_inset(ctx, &mut inset), PONKO_OK);
            assert_eq!(inset, 301.0);
            for nonsense in [-4.0, f32::NAN, f32::INFINITY] {
                assert_eq!(ponko_ctx_window_feed_keyboard_inset(ctx, nonsense), PONKO_OK);
                assert_eq!(ponko_ctx_window_keyboard_inset(ctx, &mut inset), PONKO_OK);
                assert_eq!(inset, 0.0, "{nonsense}");
            }
        });
    }
}

#[test]
fn the_audio_session_category_is_kept_and_unknown_ones_are_refused() {
    use PonkoAudioSessionCategory::*;
    unsafe {
        run_ctx(|ctx| {
            let mut category = u32::MAX;
            for wanted in [
                PONKO_AUDIO_SESSION_AMBIENT,
                PONKO_AUDIO_SESSION_PLAYBACK,
                PONKO_AUDIO_SESSION_SOLO_AMBIENT,
            ] {
                assert_eq!(ponko_ctx_audio_set_session_category(ctx, wanted as u32), PONKO_OK);
                assert_eq!(ponko_ctx_audio_session_category(ctx, &mut category), PONKO_OK);
                assert_eq!(category, wanted as u32);
            }
            assert_eq!(ponko_ctx_audio_set_session_category(ctx, 3), PONKO_INVALID_ARGUMENT);
            assert_eq!(ponko_ctx_audio_session_category(ctx, &mut category), PONKO_OK);
            assert_eq!(category, PONKO_AUDIO_SESSION_SOLO_AMBIENT as u32, "the old one stays");
        });
    }
}
