use std::ffi::{CStr, c_void};
use std::ptr::{null, null_mut};

use ponko_ffi::{PonkoConsoleType::*, PonkoStatus::*, *};

#[derive(Default)]
struct Log {
    calls: usize,
    output: String,
}

unsafe extern "C" fn remember(value: *const PonkoConsoleValue, user: *mut c_void) {
    unsafe {
        let value = &*value;
        assert_eq!(value.kind, PONKO_CONSOLE_TEXT as u32);
        let bytes = std::slice::from_raw_parts(value.text.cast(), value.text_len);
        (*user.cast::<Log>()).output = String::from_utf8(bytes.to_vec()).unwrap();
    }
}

unsafe extern "C" fn echo(
    ctx: *mut PonkoCtx,
    args: *const PonkoConsoleValue,
    count: usize,
    reply: *mut PonkoConsoleReply,
    user: *mut c_void,
) {
    unsafe {
        let args = std::slice::from_raw_parts(args, count);
        assert_eq!(args[0].kind, PONKO_CONSOLE_INTEGER as u32);
        assert_eq!(args[0].integer, -3);
        assert_eq!(args[1].kind, PONKO_CONSOLE_TEXT as u32);
        assert_eq!(
            std::slice::from_raw_parts(args[1].text.cast::<u8>(), args[1].text_len),
            b"red gem"
        );
        assert_eq!(args[2].kind, PONKO_CONSOLE_FLOAT as u32);
        assert_eq!(args[2].number, 1.5);
        assert_eq!(args[3].kind, PONKO_CONSOLE_BOOL as u32);
        assert!(args[3].flag);
        assert_eq!(
            ponko_ctx_console_value(ctx, c"label".as_ptr(), Some(remember), user),
            PONKO_OK
        );
        (*user.cast::<Log>()).calls += 1;
        assert_eq!(
            ponko_console_reply_text(reply, args[1].text, args[1].text_len, false),
            PONKO_OK
        );
    }
}

unsafe extern "C" fn start(ctx: *mut PonkoCtx, user: *mut c_void) {
    unsafe {
        assert_eq!(
            ponko_ctx_console_value(ctx, c"absent".as_ptr(), Some(remember), user),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_feed_input(ctx, 0, 0, PonkoKey::PONKO_KEY_F8 as u32, 1.0),
            PONKO_OK
        );
    }
}

unsafe extern "C" fn type_command(ctx: *mut PonkoCtx, _: *mut c_void) {
    if unsafe { ponko_ctx_sim_tick(ctx) } == 1 {
        unsafe {
            assert_eq!(
                ponko_ctx_ui_insert_text(ctx, c"echo -3 \"red gem\" 1.5 true".as_ptr()),
                PONKO_OK
            );
            assert_eq!(
                ponko_ctx_feed_input(ctx, 0, 0, PonkoKey::PONKO_KEY_ENTER as u32, 1.0),
                PONKO_OK
            );
        }
    }
}

#[test]
fn native_console_copies_registration_and_dispatches_typed_values_on_headless_frames() {
    let mut log = Log::default();
    let user = (&mut log as *mut Log).cast();
    unsafe {
        let game = ponko_game_new(c"Native console".as_ptr(), 640, 480);
        assert_eq!(ponko_game_headless_frames(game, true), PONKO_OK);
        assert_eq!(ponko_game_developer_console(game, true), PONKO_OK);
        assert_eq!(ponko_game_console_key(game, u32::MAX), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_game_console_key(game, PonkoKey::PONKO_KEY_F8 as u32), PONKO_OK);
        let bad = ponko_game_console_command(game, c"bad".as_ptr(), c"Bad".as_ptr(), null(), 1, Some(echo), user);
        assert_eq!(bad, PONKO_INVALID_ARGUMENT);
        assert!(
            CStr::from_ptr(ponko_last_error())
                .to_str()
                .unwrap()
                .contains("console input")
        );
        let mut text = *b"Ready";
        let initial = PonkoConsoleValue {
            kind: PONKO_CONSOLE_TEXT as u32,
            text: text.as_ptr().cast(),
            text_len: text.len(),
            ..Default::default()
        };
        assert_eq!(
            ponko_game_console_variable(game, c"label".as_ptr(), c"Label".as_ptr(), &initial),
            PONKO_OK
        );
        text[0] = b'X';
        assert_eq!(text[0], b'X');
        let args = [
            PonkoConsoleArg {
                name: c"count".as_ptr(),
                kind: PONKO_CONSOLE_INTEGER as u32,
            },
            PonkoConsoleArg {
                name: c"item".as_ptr(),
                kind: PONKO_CONSOLE_TEXT as u32,
            },
            PonkoConsoleArg {
                name: c"scale".as_ptr(),
                kind: PONKO_CONSOLE_FLOAT as u32,
            },
            PonkoConsoleArg {
                name: c"on".as_ptr(),
                kind: PONKO_CONSOLE_BOOL as u32,
            },
        ];
        assert_eq!(
            ponko_game_console_command(
                game,
                c"echo".as_ptr(),
                c"Echo".as_ptr(),
                args.as_ptr(),
                args.len(),
                Some(echo),
                user
            ),
            PONKO_OK
        );
        assert_eq!(
            ponko_game_system(game, c"typing".as_ptr(), Some(type_command), null_mut()),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(game, 2, Some(start), user, &mut world),
            PONKO_OK
        );
        assert_eq!(ponko_world_free(world), PONKO_OK);
    }
    assert_eq!(log.calls, 1);
    assert_eq!(log.output, "Ready");
}
