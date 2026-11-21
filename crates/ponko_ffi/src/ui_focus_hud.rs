//! The retained UI's focus set from code, and HUDs under the screens (Rust's
//! `Ui::set_focus`, `Ui::clear_focus`, `Ui::set_initial_focus`,
//! `Ui::show_hud`, `Ui::hide_hud` and `Ui::is_hud`).

use crate::ctx::out;
use crate::ui_retained::{existing, with_ui};
use crate::{PonkoCtx, PonkoStatus, boundary, invalid};

/// Moves the keyboard and gamepad focus to node `id`, as Tab or a click
/// would: Enter (or a gamepad's South button) then activates it. Only an
/// enabled, shown button, text input, toggle or slider on the top screen can
/// have it; anything else fails with `PONKO_INVALID_ARGUMENT` and leaves the
/// focus as it was. `PONKO_NOT_FOUND` for a node that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_focus(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_focus(ctx: *mut PonkoCtx, target: u64, id: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            if ui.set_focus(id) {
                Ok(())
            } else {
                Err(invalid(
                    "only an enabled, shown button, text input, toggle or slider on the top screen takes the focus",
                ))
            }
        })
    })
}

/// Takes the focus away: no node has it until Tab, an arrow key, a click or
/// `ponko_ctx_ui_set_focus`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_clear_focus(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_clear_focus(ctx: *mut PonkoCtx, target: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            ui.clear_focus();
            Ok(())
        })
    })
}

/// Marks node `id` as the one its screen opens on (or unmarks it, `on`
/// false): every `ponko_ctx_ui_push_screen` of the screen focuses it, so a
/// menu opens ready for Enter, and opens on it again when it is pushed
/// again. If several nodes of a screen are marked, the first (depth-first)
/// that can take the focus wins. `PONKO_NOT_FOUND` for a node that doesn't
/// exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_initial_focus(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_initial_focus(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    on: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_initial_focus(id, on);
            Ok(())
        })
    })
}

/// Makes pressing button `id` also close its screen (or stops it, `on`
/// false): a Back, Close or Resume button. A click or Enter on it sends its
/// `clicked` event and then pops its screen as Escape would, with a
/// `screen_popped` event (Rust `Ui::set_closes_screen`). `PONKO_NOT_FOUND`
/// for a node that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_closes_screen(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_closes_screen(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    on: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_closes_screen(id, on);
            Ok(())
        })
    })
}

/// Whether pressing node `id` closes its screen
/// (`ponko_ctx_ui_set_closes_screen`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_closes_screen(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_closes_screen(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_on: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_on, ui.closes_screen(id))
        })
    })
}

/// Lays root `id` out inside the window's safe area (clear of a phone's
/// notch, home bar and rounded corners), or in the whole window (`on`
/// false, the default): its sizes, fractions, anchors and `auto` margins then
/// count from the safe area's edges (Rust `Ui::set_inside_safe_area`). A
/// backdrop stays in the whole window so it reaches under the notch; a HUD or
/// a dialog goes inside. See `ponko_ctx_window_safe_area`.
/// `PONKO_NOT_FOUND` for a node that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_inside_safe_area(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_inside_safe_area(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    on: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_inside_safe_area(id, on);
            Ok(())
        })
    })
}

/// Whether root `id` is laid out inside the safe area
/// (`ponko_ctx_ui_set_inside_safe_area`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_inside_safe_area(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_inside_safe_area(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_on: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_on, ui.inside_safe_area(id))
        })
    })
}

/// Fades node `id` and everything inside it as one image (`on` true), or
/// piece by piece (false, the default; Rust `Ui::set_fade_as_group`).
/// While the group is partly transparent (its own opacity or an ancestor's)
/// it is drawn opaque into an offscreen layer the size of the window, shown
/// at the group's opacity: overlapping children no longer show through each
/// other. Nothing extra is drawn while it is opaque. World-space panels fade
/// piece by piece. `PONKO_NOT_FOUND` for a node that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_fade_as_group(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_fade_as_group(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    on: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_fade_as_group(id, on);
            Ok(())
        })
    })
}

/// Whether node `id` fades as one image (`ponko_ctx_ui_set_fade_as_group`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_fades_as_group(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_fades_as_group(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_on: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_on, ui.fades_as_group(id))
        })
    })
}

/// Shows the tree under a detached `root` as a HUD: display-only UI (a
/// score, a health bar) drawn below every screen. Escape (or a gamepad's
/// East button) never closes it, it takes no pointer or focus, and it
/// doesn't make `ponko_ctx_ui_wants_keyboard` true. Showing it again
/// changes nothing. `PONKO_INVALID_ARGUMENT` for a node with a parent or one
/// on the screen stack; `PONKO_NOT_FOUND` for a node that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_show_hud(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_show_hud(ctx: *mut PonkoCtx, target: u64, root: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let root = existing(ui, root)?;
            if ui.parent(root).is_some() {
                return Err(invalid("UI HUD must be a detached root"));
            }
            if ui.is_screen(root) {
                return Err(invalid("UI HUD is already a screen"));
            }
            ui.show_hud(root);
            Ok(())
        })
    })
}

/// Takes a HUD off the screen; its nodes stay, so it can be shown again.
/// Does nothing for a node that isn't a HUD. `PONKO_NOT_FOUND` for a node
/// that doesn't exist.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_hide_hud(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_hide_hud(ctx: *mut PonkoCtx, target: u64, root: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let root = existing(ui, root)?;
            ui.hide_hud(root);
            Ok(())
        })
    })
}

/// Writes whether node `id` is a HUD's root right now.
/// ```
/// let mut hud = false;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_is_hud(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, &mut hud) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_hud` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_is_hud(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    out_hud: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            out(out_hud, ui.is_hud(id))
        })
    })
}
