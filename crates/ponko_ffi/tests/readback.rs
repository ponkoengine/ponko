//! Reading GPU buffers and render targets back, from C
//! (`Graphics::read_gpu_buffer`, `read_render_target`, `readback`):
//! headless, a read isn't ready on its frame, then fails as unsupported,
//! then is used up; bad arguments are refused.
mod common;

use PonkoStatus::*;
use common::{last_error, new_game, run_ctx, run_game};
use ponko_ffi::*;
use std::ffi::c_void;
use std::ptr::{null, null_mut};

/// What the system saw each tick: the status and whether it was ready.
struct Seen {
    read: PonkoReadback,
    answers: Vec<(PonkoStatus, bool)>,
}

unsafe extern "C" fn read_each_tick(ctx: *mut PonkoCtx, user: *mut c_void) {
    // SAFETY: `user` is the test's `Seen`, alive for the run.
    let seen = unsafe { &mut *user.cast::<Seen>() };
    let mut values = [[0.0f32; 4]; 2];
    let (mut len, mut ready) = (0, false);
    // SAFETY: live context; `values` holds 2 arrays.
    let status = unsafe { ponko_ctx_readback(ctx, seen.read, values.as_mut_ptr(), 2, &mut len, &mut ready) };
    seen.answers.push((status, ready));
}

#[test]
fn a_headless_read_is_unsupported_on_the_next_frame_then_used_up() {
    let mut seen = Seen {
        read: PonkoReadback::default(),
        answers: Vec::new(),
    };
    let game = new_game();
    unsafe {
        assert_eq!(
            ponko_game_add_system(
                game,
                PonkoPhase::PONKO_PHASE_SIM_LAST as u32,
                null(),
                1,
                Some(read_each_tick),
                (&raw mut seen).cast()
            ),
            PONKO_OK
        );
    }
    let read = &raw mut seen.read;
    let world = run_game(game, 3, |ctx| unsafe {
        let mut b = PonkoGpuBuffer { id: 0 };
        assert_eq!(ponko_ctx_create_gpu_buffer(ctx, 4, null(), 0, &mut b), PONKO_OK);
        assert_eq!(
            ponko_ctx_read_gpu_buffer(ctx, b, 1, 3, read),
            PONKO_OK,
            "{}",
            last_error()
        );
        let (mut len, mut ready) = (7, true);
        // Not on the frame it was asked for; only the length asked.
        assert_eq!(
            ponko_ctx_readback(ctx, *read, null_mut(), 0, &mut len, &mut ready),
            PONKO_OK
        );
        assert_eq!((len, ready), (0, false));
    });
    unsafe { ponko_world_free(world) };
    assert_eq!(
        seen.answers,
        [
            (PONKO_UNSUPPORTED, true),
            (PONKO_INVALID_ARGUMENT, false),
            (PONKO_INVALID_ARGUMENT, false)
        ]
    );
}

#[test]
fn bad_reads_are_refused() {
    run_ctx(|ctx| unsafe {
        let mut b = PonkoGpuBuffer { id: 0 };
        assert_eq!(ponko_ctx_create_gpu_buffer(ctx, 4, null(), 0, &mut b), PONKO_OK);
        let mut read = PonkoReadback::default();
        assert_eq!(
            ponko_ctx_read_gpu_buffer(ctx, b, 2, 2, &mut read),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_read_gpu_buffer(ctx, b, 0, 5, &mut read),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("read range"), "{}", last_error());
        assert_eq!(
            ponko_ctx_read_gpu_buffer(ctx, b, 0, 1, null_mut()),
            PONKO_INVALID_ARGUMENT
        );

        let mut target = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        assert_eq!(ponko_ctx_create_render_target(ctx, 8, 8, &mut target), PONKO_OK);
        assert_eq!(
            ponko_ctx_read_render_target(ctx, target, 6, 6, 2, 2, &mut read),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(
            ponko_ctx_read_render_target(ctx, target, 7, 0, 2, 1, &mut read),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(
            ponko_ctx_read_render_target(ctx, target, 0, 0, 0, 1, &mut read),
            PONKO_INVALID_ARGUMENT
        );
        let mut plain = PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        };
        let pixel = [255u8; 4];
        assert_eq!(
            ponko_ctx_create_texture(ctx, 1, 1, pixel.as_ptr(), 4, &mut plain),
            PONKO_OK,
            "{}",
            last_error()
        );
        assert_eq!(
            ponko_ctx_read_render_target(ctx, plain, 0, 0, 1, 1, &mut read),
            PONKO_INVALID_ARGUMENT
        );
        assert!(last_error().contains("isn't a render target"), "{}", last_error());

        let (mut len, mut ready) = (0, false);
        assert_eq!(
            ponko_ctx_readback(ctx, read, null_mut(), 1, &mut len, &mut ready),
            PONKO_INVALID_ARGUMENT,
            "values must be given for a capacity"
        );
        assert_eq!(
            ponko_ctx_readback(ctx, read, null_mut(), 0, null_mut(), &mut ready),
            PONKO_INVALID_ARGUMENT
        );
    });
    let mut read = PonkoReadback::default();
    assert_eq!(
        unsafe { ponko_ctx_read_gpu_buffer(null_mut(), PonkoGpuBuffer { id: 0 }, 0, 1, &mut read) },
        PONKO_INVALID_ARGUMENT
    );
}
