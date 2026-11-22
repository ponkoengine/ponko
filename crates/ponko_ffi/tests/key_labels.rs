//! Key labels on the player's keyboard layout through the C API (gl#237):
//! a key's and a glyph's label, the change edge, and a layout set from C,
//! in a headless run of three ticks (each its own frame).
use ponko_ffi::*;
use std::cell::Cell;
use std::ffi::{CStr, c_char, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::{null, null_mut};

use PonkoKey::*;
use PonkoStatus::*;

type Ctx = *mut PonkoCtx;

struct Run {
    frame: Cell<u32>,
    failure: Option<String>,
}

unsafe extern "C" fn system(ctx: Ctx, user: *mut c_void) {
    let run = unsafe { &mut *user.cast::<Run>() };
    let frame = run.frame.get();
    run.frame.set(frame + 1);
    if run.failure.is_some() {
        return;
    }
    if let Err(payload) = catch_unwind(AssertUnwindSafe(|| unsafe { frame_body(ctx, frame) })) {
        let message = payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()));
        run.failure = Some(format!(
            "frame {frame}: {}",
            message.unwrap_or_else(|| "panicked".into())
        ));
    }
}

/// Reads a string through a `(buffer, capacity, out_len)` call.
unsafe fn read(call: impl Fn(*mut c_char, usize, *mut usize) -> PonkoStatus) -> String {
    let mut len = 0;
    assert_eq!(call(null_mut(), 0, &mut len), PONKO_OK);
    let mut buffer = vec![0u8; len + 1];
    assert_eq!(call(buffer.as_mut_ptr().cast(), buffer.len(), &mut len), PONKO_OK);
    unsafe { CStr::from_ptr(buffer.as_ptr().cast()) }
        .to_string_lossy()
        .into_owned()
}

unsafe fn label(ctx: Ctx, key: PonkoKey) -> String {
    unsafe { read(|b, c, l| ponko_ctx_key_label(ctx, key as u32, b, c, l)) }
}

unsafe fn changed(ctx: Ctx) -> bool {
    let mut changed = true;
    assert_eq!(unsafe { ponko_ctx_key_labels_changed(ctx, &mut changed) }, PONKO_OK);
    changed
}

fn entry(key: PonkoKey, c: char) -> PonkoKeyLabel {
    PonkoKeyLabel {
        key: key as u32,
        character: c as u32,
    }
}

unsafe fn frame_body(ctx: Ctx, frame: u32) {
    unsafe {
        match frame {
            0 => {
                // Headless: a US keyboard until something says otherwise.
                assert!(!changed(ctx));
                assert_eq!(label(ctx, PONKO_KEY_W), "W");
                assert_eq!(label(ctx, PONKO_KEY_SPACE), "Space");
                assert_eq!(label(ctx, PONKO_KEY_LEFT_SHIFT), "Left Shift");
                // A French keyboard, as a "keyboard layout" option sets it.
                let azerty = [
                    entry(PONKO_KEY_W, 'z'),
                    entry(PONKO_KEY_A, 'q'),
                    entry(PONKO_KEY_Q, 'a'),
                    entry(PONKO_KEY_NUM_2, 'é'),
                    entry(PONKO_KEY_SPACE, 'x'), // not a character key: ignored
                ];
                assert_eq!(ponko_ctx_set_key_labels(ctx, azerty.as_ptr(), azerty.len()), PONKO_OK);
                assert!(changed(ctx), "told at once, for this frame");
                assert_eq!(label(ctx, PONKO_KEY_W), "Z");
                assert_eq!(label(ctx, PONKO_KEY_NUM_2), "É");
                assert_eq!(label(ctx, PONKO_KEY_SPACE), "Space");
                let w = PonkoGlyph {
                    kind: PonkoGlyphKind::PONKO_GLYPH_KEY as u32,
                    family: 0,
                    code: PONKO_KEY_W as u32,
                };
                assert_eq!(read(|b, c, l| ponko_ctx_glyph_label(ctx, w, b, c, l)), "Z");
                assert_eq!(
                    read(|b, c, l| ponko_glyph_label(w, b, c, l)),
                    "W",
                    "the US label stays there"
                );
            }
            1 => {
                assert!(!changed(ctx), "an edge: one frame only");
                assert_eq!(label(ctx, PONKO_KEY_W), "Z", "kept");
                // A bad entry changes nothing.
                let bad = [
                    entry(PONKO_KEY_W, 'y'),
                    PonkoKeyLabel {
                        key: 9999,
                        character: 'a' as u32,
                    },
                ];
                assert_eq!(
                    ponko_ctx_set_key_labels(ctx, bad.as_ptr(), bad.len()),
                    PONKO_INVALID_ARGUMENT
                );
                let surrogate = [PonkoKeyLabel {
                    key: PONKO_KEY_W as u32,
                    character: 0xD800,
                }];
                assert_eq!(
                    ponko_ctx_set_key_labels(ctx, surrogate.as_ptr(), 1),
                    PONKO_INVALID_ARGUMENT
                );
                assert_eq!(ponko_ctx_set_key_labels(ctx, null(), 1), PONKO_INVALID_ARGUMENT);
                assert!(!changed(ctx));
                assert_eq!(label(ctx, PONKO_KEY_W), "Z");
                let mut len = 0;
                assert_eq!(
                    ponko_ctx_key_label(ctx, 9999, null_mut(), 0, &mut len),
                    PONKO_INVALID_ARGUMENT
                );
                // A short buffer gets the length and an empty string.
                let mut small = [b'?' as c_char; 1];
                assert_eq!(
                    ponko_ctx_key_label(ctx, PONKO_KEY_SPACE as u32, small.as_mut_ptr(), 1, &mut len),
                    PONKO_OK
                );
                assert_eq!((len, small[0]), (5, 0));
                // Back to a US keyboard.
                assert_eq!(ponko_ctx_set_key_labels(ctx, null(), 0), PONKO_OK);
                assert!(changed(ctx));
            }
            _ => {
                assert!(!changed(ctx));
                assert_eq!(label(ctx, PONKO_KEY_W), "W");
            }
        }
    }
}

#[test]
fn c_reads_key_labels_and_hears_when_they_change() {
    let mut run = Run {
        frame: Cell::new(0),
        failure: None,
    };
    unsafe {
        let game = ponko_game_new(c"key labels".as_ptr(), 1, 1);
        let user = (&mut run as *mut Run).cast();
        assert_eq!(
            ponko_game_sim_system(game, c"labels".as_ptr(), 1, Some(system), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(ponko_game_run_headless(game, 3, None, null_mut(), &mut world), PONKO_OK);
        ponko_world_free(world);
    }
    if let Some(failure) = run.failure {
        panic!("{failure}");
    }
    assert_eq!(run.frame.get(), 3, "the system ran once a tick");
}
