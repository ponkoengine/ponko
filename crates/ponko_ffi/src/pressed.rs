//! "Which input was pressed this frame": the C side of
//! `Input::pressed_this_frame`, for rebinding screens.
use crate::actions::{PonkoActionBinding, PonkoActionSource};
use crate::ctx::ctx_access;
use crate::{PonkoCtx, PonkoStatus, boundary, invalid};
use ponko::prelude::{AxisBinding, ButtonBinding, PressedInput};

/// The axis threshold `ponko_ctx_pressed_this_frame` uses for `0.0`:
/// Rust's `PRESS_THRESHOLD`.
/// ```
/// assert_eq!(ponko_ffi::PONKO_PRESS_THRESHOLD, ponko::actions::PRESS_THRESHOLD);
/// ```
pub const PONKO_PRESS_THRESHOLD: f32 = 0.5;

/// One press as the binding description `ponko_action_map_set` takes: kind
/// key, mouse, gamepad button or gamepad axis (with the default deadzone).
pub(crate) fn describe(pressed: PressedInput) -> Option<PonkoActionBinding> {
    let blank = PonkoActionBinding {
        kind: 0,
        pad: 0,
        first: 0,
        second: 0,
        third: 0,
        fourth: 0,
        deadzone: 0.0,
    };
    Some(match pressed {
        PressedInput::Button(ButtonBinding::Key(k)) => PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_KEY as u32,
            first: k as u32,
            ..blank
        },
        PressedInput::Button(ButtonBinding::Mouse(b)) => PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_MOUSE as u32,
            first: b as u32,
            ..blank
        },
        PressedInput::Button(ButtonBinding::Gamepad { pad, button }) => PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_GAMEPAD_BUTTON as u32,
            pad: pad.slot() as u32,
            first: button as u32,
            ..blank
        },
        PressedInput::Axis(AxisBinding::Gamepad { pad, axis, deadzone }) => PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_GAMEPAD_AXIS as u32,
            pad: pad.slot() as u32,
            first: axis as u32,
            deadzone,
            ..blank
        },
        PressedInput::Button(ButtonBinding::Gesture(g)) => PonkoActionBinding {
            kind: PonkoActionSource::PONKO_ACTION_GESTURE as u32,
            first: crate::touch::gesture_to_c(g),
            ..blank
        },
        // A press kind this ABI version cannot describe is left out.
        _ => return None,
    })
}

/// Lists every key, mouse button, gamepad button, gamepad axis and touch
/// gesture that went down this frame (Rust's `ctx.input.pressed_this_frame()`), as
/// `PonkoActionBinding`s: kinds `PONKO_ACTION_KEY`, `PONKO_ACTION_MOUSE`,
/// `PONKO_ACTION_GAMEPAD_BUTTON`, `PONKO_ACTION_GAMEPAD_AXIS` (with the
/// default deadzone) and `PONKO_ACTION_GESTURE`, ready for
/// `ponko_action_map_set` (kind 0 for the buttons and gestures, 1 for the
/// axis). Order: keys, mouse buttons, each gamepad slot's buttons, axes,
/// then gestures; the same every run.
///
/// `axis_threshold` is how far a stick or trigger must move to count, in
/// `0..=1`; `0.0` means `PONKO_PRESS_THRESHOLD`. `out_count` receives the
/// number of presses; at most `capacity` of them are written to `out`, so a
/// short buffer loses the later ones and `*out_count > capacity` says so.
/// `out` may be null with `capacity` 0 to just count. A negative or NaN
/// threshold is `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_pressed_this_frame(std::ptr::null_mut(), 0.0, std::ptr::null_mut(), 0, &mut 0) } as u32, 0);
/// ```
/// # Safety
/// Context must be live or null; `out` writable for `capacity` bindings (or
/// null with capacity 0); `out_count` writable; no overlapping pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_pressed_this_frame(
    ctx: *mut PonkoCtx,
    axis_threshold: f32,
    out: *mut PonkoActionBinding,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context and independent outputs.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_count.is_null() || (out.is_null() && capacity != 0) {
                return Err(invalid("output is null"));
            }
            if axis_threshold.is_nan() || axis_threshold < 0.0 {
                return Err(invalid("axis threshold must be 0 or positive"));
            }
            let threshold = if axis_threshold == 0.0 {
                PONKO_PRESS_THRESHOLD
            } else {
                axis_threshold
            };
            let mut count = 0usize;
            for binding in c
                .input
                .pressed_this_frame_with_threshold(threshold)
                .filter_map(describe)
            {
                if count < capacity {
                    // SAFETY: `count < capacity`, and the caller owns that many slots.
                    out.add(count).write(binding);
                }
                count += 1;
            }
            crate::ctx::out(out_count, count)
        })
    })
}
