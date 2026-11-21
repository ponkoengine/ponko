//! UI groups fading as one image from C: `ponko_ctx_ui_set_fade_as_group`
//! and `ponko_ctx_ui_fades_as_group`, as Rust's `Ui::set_fade_as_group`.
mod common;
use common::*;
use ponko_ffi::*;

use PonkoStatus::*;

#[test]
fn a_node_fades_as_a_group_once_set_and_piece_by_piece_by_default() {
    run_ctx(|ctx| unsafe {
        let (mut menu, mut card) = (PONKO_UI_NONE, PONKO_UI_NONE);
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, std::ptr::null(), &mut menu),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, menu, std::ptr::null(), &mut card),
            PONKO_OK
        );
        let mut on = true;
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, menu, &mut on),
            PONKO_OK
        );
        assert!(!on, "piece by piece by default");
        assert_eq!(
            ponko_ctx_ui_set_fade_as_group(ctx, PONKO_UI_SCREEN, menu, true),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, menu, &mut on),
            PONKO_OK
        );
        assert!(on);
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, card, &mut on),
            PONKO_OK
        );
        assert!(!on, "its children are in the group, not groups of their own");
        assert_eq!(
            ponko_ctx_ui_set_fade_as_group(ctx, PONKO_UI_SCREEN, menu, false),
            PONKO_OK
        );
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, menu, &mut on),
            PONKO_OK
        );
        assert!(!on);
        assert_eq!(
            ponko_ctx_ui_set_fade_as_group(ctx, PONKO_UI_SCREEN, 9999, true),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, 9999, &mut on),
            PONKO_NOT_FOUND
        );
        assert_eq!(
            ponko_ctx_ui_fades_as_group(ctx, PONKO_UI_SCREEN, menu, std::ptr::null_mut()),
            PONKO_INVALID_ARGUMENT
        );
    });
}
