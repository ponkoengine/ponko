//! HDR display output from C: `PonkoHdrOutput` through
//! `ponko_game_hdr_output`, `ponko_ctx_hdr_output` and
//! `ponko_ctx_set_hdr_output`, and `ponko_ctx_hdr_status`, as Rust's
//! `Game::hdr_output`, `Graphics::hdr_output` / `set_hdr_output` and
//! `hdr_status`. Headless: no window, so the status is SDR with its reason.
mod common;
use common::*;
use ponko_ffi::*;

use PonkoStatus::*;

fn defaults() -> PonkoHdrOutput {
    // SAFETY: a plain C struct, all zeros before its header is set.
    let mut h: PonkoHdrOutput = unsafe { std::mem::zeroed() };
    (h.struct_size, h.version) = (size_of::<PonkoHdrOutput>() as u32, PONKO_HDR_OUTPUT_VERSION);
    // SAFETY: a writable descriptor with its header set.
    assert_eq!(unsafe { ponko_hdr_output_defaults(&mut h) }, PONKO_OK);
    h
}

fn read(ctx: *mut PonkoCtx) -> PonkoHdrOutput {
    // SAFETY: as above.
    let mut h: PonkoHdrOutput = unsafe { std::mem::zeroed() };
    (h.struct_size, h.version) = (size_of::<PonkoHdrOutput>() as u32, PONKO_HDR_OUTPUT_VERSION);
    // SAFETY: a live context and a writable descriptor.
    assert_eq!(unsafe { ponko_ctx_hdr_output(ctx, &mut h) }, PONKO_OK);
    h
}

#[test]
fn hdr_output_defaults_are_off_with_a_200_nit_paper_white() {
    let h = defaults();
    assert_eq!((h.enabled, h.paper_white_nits, h.peak_nits), (false, 200.0, 0.0));
    run_ctx(|ctx| assert_eq!(read(ctx), h, "a new game starts with the defaults"));
}

#[test]
fn the_game_option_reaches_the_running_game_and_settings_change_while_it_runs() {
    let mut h = defaults();
    h.enabled = true;
    h.paper_white_nits = 250.0;
    let game = new_game();
    // SAFETY: a live game and a readable descriptor.
    assert_eq!(unsafe { ponko_game_hdr_output(game, &h) }, PONKO_OK);
    let world = run_game(game, 0, |ctx| unsafe {
        assert_eq!(read(ctx), h);
        let mut status: PonkoHdrStatus = std::mem::zeroed();
        (status.struct_size, status.version) = (size_of::<PonkoHdrStatus>() as u32, PONKO_HDR_STATUS_VERSION);
        assert_eq!(ponko_ctx_hdr_status(ctx, &mut status), PONKO_OK);
        assert!(!status.active && !status.available);
        assert_eq!(status.encoding, PonkoHdrEncoding::PONKO_HDR_ENCODING_SDR);
        assert_eq!(status.reason, PonkoHdrOffReason::PONKO_HDR_OFF_HEADLESS);
        assert_eq!((status.headroom, status.display_peak_nits), (1.0, 0.0));

        let mut now = read(ctx);
        now.peak_nits = 800.0;
        assert_eq!(ponko_ctx_set_hdr_output(ctx, &now), PONKO_OK);
        assert_eq!(read(ctx).peak_nits, 800.0);
        // A NaN or negative peak is refused and changes nothing.
        for bad in [f32::NAN, -1.0] {
            now.peak_nits = bad;
            assert_eq!(ponko_ctx_set_hdr_output(ctx, &now), PONKO_INVALID_ARGUMENT);
            assert_eq!(read(ctx).peak_nits, 800.0);
        }
        assert_eq!(ponko_ctx_set_hdr_output(ctx, std::ptr::null()), PONKO_INVALID_ARGUMENT);
        assert_eq!(ponko_ctx_hdr_status(ctx, std::ptr::null_mut()), PONKO_INVALID_ARGUMENT);
    });
    // SAFETY: the world the run returned, freed once.
    unsafe { ponko_world_free(world) };
}

#[test]
fn a_refused_game_option_leaves_the_game_as_it_was() {
    let mut h = defaults();
    h.enabled = true;
    h.peak_nits = -5.0;
    let game = new_game();
    // SAFETY: a live game and a readable descriptor.
    assert_eq!(unsafe { ponko_game_hdr_output(game, &h) }, PONKO_INVALID_ARGUMENT);
    let world = run_game(game, 0, |ctx| assert!(!read(ctx).enabled, "still the defaults"));
    // SAFETY: as above.
    unsafe { ponko_world_free(world) };
}

#[test]
fn an_unknown_version_is_refused() {
    let mut h = defaults();
    h.version = PONKO_HDR_OUTPUT_VERSION + 1;
    // SAFETY: a writable descriptor.
    assert_eq!(unsafe { ponko_hdr_output_defaults(&mut h) }, PONKO_INVALID_ARGUMENT);
}

#[test]
fn hdr10_metadata_settings_round_trip_and_bad_values_are_refused() {
    let h = defaults();
    assert_eq!((h.max_content_light_nits, h.max_frame_average_nits), (0.0, 0.0));
    run_ctx(|ctx| unsafe {
        let mut now = read(ctx);
        now.enabled = true;
        now.max_content_light_nits = 900.0;
        now.max_frame_average_nits = 250.0;
        // SAFETY: a live context and readable descriptors.
        assert_eq!(ponko_ctx_set_hdr_output(ctx, &now), PONKO_OK);
        let back = read(ctx);
        assert_eq!(
            (back.enabled, back.max_content_light_nits, back.max_frame_average_nits),
            (true, 900.0, 250.0)
        );
        for bad in [f32::NAN, -1.0] {
            let mut wrong = now;
            wrong.max_frame_average_nits = bad;
            assert_eq!(ponko_ctx_set_hdr_output(ctx, &wrong), PONKO_INVALID_ARGUMENT);
            assert_eq!(read(ctx).max_frame_average_nits, 250.0, "unchanged");
        }
        // A version 1 caller (no metadata fields) keeps the metadata as set.
        let mut v1 = now;
        v1.version = 1;
        v1.peak_nits = 700.0;
        v1.max_content_light_nits = -99.0; // past its version: not read
        assert_eq!(ponko_ctx_set_hdr_output(ctx, &v1), PONKO_OK);
        let back = read(ctx);
        assert_eq!((back.peak_nits, back.max_content_light_nits), (700.0, 900.0));
        // Headless: nothing was sent to a display.
        let mut status: PonkoHdrStatus = std::mem::zeroed();
        (status.struct_size, status.version) = (size_of::<PonkoHdrStatus>() as u32, PONKO_HDR_STATUS_VERSION);
        assert_eq!(ponko_ctx_hdr_status(ctx, &mut status), PONKO_OK);
        assert!(!status.metadata_sent);
        assert_eq!((status.max_mastering_nits, status.max_content_light_nits), (0.0, 0.0));
    });
}
