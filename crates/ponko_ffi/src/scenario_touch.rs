//! Touch scenario builders from C (Rust `InputScenario::tap`, `touch_hold`
//! and `touch_drag`): a whole finger in one call, each with an id of its
//! own, for headless tests of touch games and on-screen controls. Also the
//! step that feeds safe area insets (`InputScenario::safe_insets`), for
//! layouts on a notched phone.
use ponko::scenario::InputScenario;
use ponko::window::SafeInsets;

use crate::PonkoSafeInsets;
use crate::PonkoStatus;
use crate::invalid;
use crate::scenario::{PonkoInputScenario, edit};
use crate::types::{PonkoVec2, to_c};

fn finite(points: &[PonkoVec2]) -> Result<(), crate::Failure> {
    if points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        Ok(())
    } else {
        Err(invalid("touch positions must be finite"))
    }
}

/// Adds a tap at `at` (window pixels) on tick `tick`: a finger goes down
/// and lifts there within the tick, so it is a tap gesture, or a press of
/// the on-screen button under it (Rust `InputScenario::tap`). A
/// non-finite position is `PONKO_INVALID_ARGUMENT` and adds nothing.
///
/// Ownership: `scenario` stays the caller's; it is edited during the call
/// only.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// let at = ponko_ffi::PonkoVec2 { x: 100.0, y: 200.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_tap(s, 3, at) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_tap(
    scenario: *mut PonkoInputScenario,
    tick: u64,
    at: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            finite(&[at])?;
            Ok(move |s: InputScenario| s.tap(tick, to_c(at)))
        })
    }
}

/// Adds a finger held still at `at` from tick `from` until it lifts on
/// tick `to` (`from` if earlier): a held on-screen button, or a long press
/// when it lasts long enough (Rust `InputScenario::touch_hold`).
///
/// Ownership: `scenario` stays the caller's; it is edited during the call
/// only.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// let at = ponko_ffi::PonkoVec2 { x: 700.0, y: 500.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_touch_hold(s, 10, 50, at) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_touch_hold(
    scenario: *mut PonkoInputScenario,
    from: u64,
    to: u64,
    at: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            finite(&[at])?;
            Ok(move |s: InputScenario| s.touch_hold(from, to, to_c(at)))
        })
    }
}

/// Adds a finger that goes down at `start` on tick `from`, moves in a
/// straight line one step each tick, and lifts at `end` on tick `to`: a
/// drag or a swipe, or an on-screen stick pushed and let go (Rust
/// `InputScenario::touch_drag`).
///
/// Ownership: `scenario` stays the caller's; it is edited during the call
/// only.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// let (a, b) = (ponko_ffi::PonkoVec2 { x: 100.0, y: 400.0 }, ponko_ffi::PonkoVec2 { x: 160.0, y: 400.0 });
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_touch_drag(s, 5, 25, a, b) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_touch_drag(
    scenario: *mut PonkoInputScenario,
    from: u64,
    to: u64,
    start: PonkoVec2,
    end: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            finite(&[start, end])?;
            Ok(move |s: InputScenario| s.touch_drag(from, to, to_c(start), to_c(end)))
        })
    }
}

/// Adds a step that feeds the window's safe area insets on tick `tick`, as a
/// phone reports them (Rust `InputScenario::safe_insets`), before that
/// tick's simulation systems run: a layout for a notched phone is checked
/// headless, without a device. They stay until another step changes them. A
/// non-finite or negative inset is 0, as `ponko_ctx_window_feed_safe_insets`
/// takes it.
///
/// Ownership: `scenario` stays the caller's; it is edited during the call
/// only.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// let insets = ponko_ffi::PonkoSafeInsets { top: 0.0, right: 0.0, bottom: 21.0, left: 47.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_safe_insets(s, 0, insets) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_safe_insets(
    scenario: *mut PonkoInputScenario,
    tick: u64,
    insets: PonkoSafeInsets,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            Ok(move |s: InputScenario| s.safe_insets(tick, SafeInsets::from(insets)))
        })
    }
}
