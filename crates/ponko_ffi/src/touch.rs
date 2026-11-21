//! Touch input (gl#61): the C side of Rust's touches (`InputEvent::Touch`,
//! `Input::touches`), gestures (`Input::gesture_events`, `drag_delta`,
//! `pinch_scale`, `rotation_delta`, `GestureSettings`), the touch device
//! switch, on-screen controls (`Input::set_touch_controls` and friends) and the
//! input scenario's touch step. Gesture bindings for actions are the
//! `PONKO_ACTION_GESTURE`, `PONKO_ACTION_GESTURE_AXIS` and `PONKO_ACTION_DRAG`
//! sources of `PonkoActionBinding`.
use std::ffi::c_char;

use ponko::actions::InputEvent;
use ponko::input::{Gesture, GestureAxis, GestureSettings, TouchPhase};
use ponko::scenario::InputScenario;
use ponko::ui::touch::{TouchControls, TouchShow};

use crate::ctx::{ctx_access, out, text};
use crate::scenario::{PonkoInputScenario, edit};
use crate::types::{PonkoVec2, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid};

/// Where a finger is in its life (Rust's `TouchPhase`).
/// ```
/// assert_eq!(ponko_ffi::PonkoTouchPhase::PONKO_TOUCH_CANCELLED as u32, 3);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoTouchPhase {
    /// The finger touched the screen.
    PONKO_TOUCH_STARTED = 0,
    /// It moved.
    PONKO_TOUCH_MOVED = 1,
    /// It lifted.
    PONKO_TOUCH_ENDED = 2,
    /// The system took it away (no gesture comes of it).
    PONKO_TOUCH_CANCELLED = 3,
}

/// A gesture (Rust's `Gesture`), for `PONKO_ACTION_GESTURE` bindings and
/// `PonkoGestureEvent`.
/// ```
/// assert_eq!(ponko_ffi::PonkoGesture::PONKO_GESTURE_SWIPE_DOWN as u32, 9);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoGesture {
    /// A short touch that lifts near where it started.
    PONKO_GESTURE_TAP = 0,
    /// A second tap soon after a tap, near it.
    PONKO_GESTURE_DOUBLE_TAP = 1,
    /// A touch held still: held until it lifts.
    PONKO_GESTURE_LONG_PRESS = 2,
    /// One finger moving: held until it lifts.
    PONKO_GESTURE_DRAG = 3,
    /// Two fingers spreading or closing: held until one lifts.
    PONKO_GESTURE_PINCH = 4,
    /// Two fingers turning: held until one lifts.
    PONKO_GESTURE_ROTATE = 5,
    /// A quick drag to the left, when the finger lifts.
    PONKO_GESTURE_SWIPE_LEFT = 6,
    /// A quick drag to the right.
    PONKO_GESTURE_SWIPE_RIGHT = 7,
    /// A quick drag up.
    PONKO_GESTURE_SWIPE_UP = 8,
    /// A quick drag down.
    PONKO_GESTURE_SWIPE_DOWN = 9,
}

/// A gesture's amount this frame (Rust's `GestureAxis`), for
/// `PONKO_ACTION_GESTURE_AXIS` bindings.
/// ```
/// assert_eq!(ponko_ffi::PonkoGestureAxis::PONKO_GESTURE_AXIS_DRAG_Y as u32, 3);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoGestureAxis {
    /// How much a pinch spread this frame (`0.1`: 10 % farther apart).
    PONKO_GESTURE_AXIS_PINCH = 0,
    /// How far two fingers turned this frame, radians, clockwise positive.
    PONKO_GESTURE_AXIS_ROTATE = 1,
    /// A drag's movement across this frame, window pixels.
    PONKO_GESTURE_AXIS_DRAG_X = 2,
    /// A drag's movement down this frame, window pixels.
    PONKO_GESTURE_AXIS_DRAG_Y = 3,
}

/// When on-screen controls are shown (Rust's `TouchShow`).
/// ```
/// assert_eq!(ponko_ffi::PonkoTouchShow::PONKO_TOUCH_SHOW_AUTO as u32, 0);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoTouchShow {
    /// On touch devices only (the default).
    PONKO_TOUCH_SHOW_AUTO = 0,
    /// Always.
    PONKO_TOUCH_SHOW_ALWAYS = 1,
    /// Never.
    PONKO_TOUCH_SHOW_NEVER = 2,
}

/// What a `PonkoTouchControl` is.
/// ```
/// assert_eq!(ponko_ffi::PonkoTouchControlKind::PONKO_TOUCH_CORNER_BUTTON as u32, 3);
/// ```
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoTouchControlKind {
    /// A stick at the bottom left, for a vector or scalar action.
    PONKO_TOUCH_STICK = 0,
    /// A stick at the bottom right.
    PONKO_TOUCH_RIGHT_STICK = 1,
    /// A round button at the bottom right, for a button action.
    PONKO_TOUCH_BUTTON = 2,
    /// A small button in the top right corner.
    PONKO_TOUCH_CORNER_BUTTON = 3,
}

/// One on-screen control for `ponko_ctx_set_touch_controls`.
/// ```
/// let c = ponko_ffi::PonkoTouchControl { kind: 0, action: c"move".as_ptr(), label: std::ptr::null() };
/// assert_eq!(c.kind, 0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PonkoTouchControl {
    /// A `PonkoTouchControlKind` value.
    pub kind: u32,
    /// The action it drives, NUL-terminated UTF-8, read during the call.
    pub action: *const c_char,
    /// A button's label (null: none); sticks ignore it.
    pub label: *const c_char,
}

/// One finger on the screen (Rust's `Touch`).
/// ```
/// let t = ponko_ffi::PonkoTouch::default();
/// assert!(!t.claimed);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PonkoTouch {
    /// The id the platform gave it.
    pub id: u64,
    /// Where it is, window pixels.
    pub position: PonkoVec2,
    /// Where it touched down.
    pub start: PonkoVec2,
    /// How far it moved this frame.
    pub delta: PonkoVec2,
    /// It is on an on-screen control (no gesture comes of it).
    pub claimed: bool,
}

/// A gesture that happened this frame (Rust's `GestureEvent`).
/// ```
/// let e = ponko_ffi::PonkoGestureEvent::default();
/// assert_eq!(e.gesture, 0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PonkoGestureEvent {
    /// A `PonkoGesture` value.
    pub gesture: u32,
    /// Where, window pixels.
    pub position: PonkoVec2,
}

/// The thresholds gestures are recognized with (Rust's `GestureSettings`):
/// distances in window pixels, times in seconds of game time.
/// ```
/// let s = ponko_ffi::PonkoGestureSettings::default();
/// assert_eq!(s.tap_distance, 0.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PonkoGestureSettings {
    /// How far a tap or a long press may move.
    pub tap_distance: f32,
    /// How long a tap may last.
    pub tap_time: f32,
    /// How soon after a tap a second one is a double tap.
    pub double_tap_time: f32,
    /// How near the first a double tap's second tap must be.
    pub double_tap_distance: f32,
    /// How long a touch is held still to be a long press.
    pub long_press_time: f32,
    /// How long a swipe is at least.
    pub swipe_distance: f32,
    /// How long a swipe lasts at most.
    pub swipe_time: f32,
    /// How far two fingers' distance changes to start a pinch.
    pub pinch_distance: f32,
    /// How far two fingers turn to start a rotation, radians.
    pub rotate_angle: f32,
}

/// The gesture a C value names.
pub(crate) fn gesture_from_c(value: u32) -> Result<Gesture, Failure> {
    Gesture::ALL
        .get(value as usize)
        .copied()
        .ok_or_else(|| invalid("unknown gesture"))
}

/// A gesture's C value.
pub(crate) fn gesture_to_c(gesture: Gesture) -> u32 {
    Gesture::ALL.iter().position(|g| *g == gesture).unwrap_or(0) as u32
}

/// The gesture axis a C value names.
pub(crate) fn gesture_axis_from_c(value: u32) -> Result<GestureAxis, Failure> {
    Ok(match value {
        0 => GestureAxis::Pinch,
        1 => GestureAxis::Rotate,
        2 => GestureAxis::DragX,
        3 => GestureAxis::DragY,
        _ => return Err(invalid("unknown gesture axis")),
    })
}

/// A gesture axis's C value, `None` for one this ABI does not know.
pub(crate) fn gesture_axis_to_c(axis: GestureAxis) -> Option<u32> {
    Some(match axis {
        GestureAxis::Pinch => 0,
        GestureAxis::Rotate => 1,
        GestureAxis::DragX => 2,
        GestureAxis::DragY => 3,
        _ => return None,
    })
}

/// A gesture source's scale from a binding's `deadzone` field (0 means 1).
pub(crate) fn scale_from_c(value: f32) -> Result<f32, Failure> {
    if !value.is_finite() {
        return Err(invalid("a gesture scale must be finite"));
    }
    Ok(if value == 0.0 { 1.0 } else { value })
}

fn phase_from_c(value: u32) -> Result<TouchPhase, Failure> {
    Ok(match value {
        0 => TouchPhase::Started,
        1 => TouchPhase::Moved,
        2 => TouchPhase::Ended,
        3 => TouchPhase::Cancelled,
        _ => return Err(invalid("unknown touch phase")),
    })
}

/// The touch event the C arguments describe, checked.
fn touch_event(id: u64, phase: u32, position: PonkoVec2) -> Result<InputEvent, Failure> {
    let position: ponko::prelude::Vec2 = to_c(position);
    if !position.is_finite() {
        return Err(invalid("a touch position must be finite"));
    }
    Ok(InputEvent::Touch {
        id,
        phase: phase_from_c(phase)?,
        position,
    })
}

/// Writes up to `capacity` of `items` to `out` and their number to
/// `out_count` (the `ponko_ctx_pressed_this_frame` convention).
unsafe fn write_list<T: Copy>(
    items: impl ExactSizeIterator<Item = T>,
    out_list: *mut T,
    capacity: usize,
    out_count: *mut usize,
) -> Result<(), Failure> {
    if out_count.is_null() || (out_list.is_null() && capacity != 0) {
        return Err(invalid("output is null"));
    }
    let count = items.len();
    for (i, item) in items.take(capacity).enumerate() {
        // SAFETY: caller supplies `capacity` writable elements.
        unsafe { out_list.add(i).write(item) };
    }
    // SAFETY: checked non-null; caller supplies a writable count.
    unsafe { out(out_count, count) }
}

/// Feeds a finger's event, as the window does (Rust `InputEvent::Touch`):
/// `phase` is a `PonkoTouchPhase`, `position` window pixels, `id` the
/// finger's id (a later finger takes a new one). Gestures, actions bound to
/// them and on-screen controls follow. Not a finite point or an unknown
/// phase: `PONKO_INVALID_ARGUMENT`.
/// ```
/// let at = ponko_ffi::PonkoVec2 { x: 10.0, y: 20.0 };
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_feed_touch(std::ptr::null_mut(), 1, 0, at) } as u32, 0);
/// ```
/// # Safety
/// Context must be live in its callback, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_feed_touch(
    ctx: *mut PonkoCtx,
    id: u64,
    phase: u32,
    position: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: caller supplies callback context.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.feed(touch_event(id, phase, position)?);
            Ok(())
        })
    })
}

/// Lists the fingers on the screen (Rust `Input::touches`), oldest first.
/// `*out_count` receives how many there are; at most `capacity` are written
/// to `out` (null with capacity 0 just counts).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touches(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut 0) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `out` writable for `capacity` touches (or null with
/// capacity 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touches(
    ctx: *mut PonkoCtx,
    out_touches: *mut PonkoTouch,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let touches = c.input.touches().iter().map(|t| PonkoTouch {
                id: t.id(),
                position: to_c(t.position()),
                start: to_c(t.start()),
                delta: to_c(t.delta()),
                claimed: t.claimed(),
            });
            write_list(touches, out_touches, capacity, out_count)
        })
    })
}

/// Lists the gestures that happened this frame (Rust
/// `Input::gesture_events`), in order, as `ponko_ctx_touches` lists.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gesture_events(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut 0) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `out` writable for `capacity` events (or null with
/// capacity 0); `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gesture_events(
    ctx: *mut PonkoCtx,
    out_events: *mut PonkoGestureEvent,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let events = c.input.gesture_events().iter().map(|e| PonkoGestureEvent {
                gesture: gesture_to_c(e.gesture),
                position: to_c(e.position),
            });
            write_list(events, out_events, capacity, out_count)
        })
    })
}

/// This frame's change of the drag, the pinch and the rotation: `out_drag`
/// in window pixels (Rust `Input::drag_delta`), `out_pinch_scale` as a
/// factor, 1 with no pinch (`pinch_scale`), `out_rotation` in radians,
/// clockwise positive (`rotation_delta`). Any output may be null.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gesture_deltas(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; each output null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gesture_deltas(
    ctx: *mut PonkoCtx,
    out_drag: *mut PonkoVec2,
    out_pinch_scale: *mut f32,
    out_rotation: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract; null outputs are skipped.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if !out_drag.is_null() {
                out(out_drag, to_c(c.input.drag_delta()))?;
            }
            if !out_pinch_scale.is_null() {
                out(out_pinch_scale, c.input.pinch_scale())?;
            }
            if !out_rotation.is_null() {
                out(out_rotation, c.input.rotation_delta())?;
            }
            Ok(())
        })
    })
}

/// Whether this is a touch device (Rust `Input::touch_device`): the
/// platform has a touch screen, or a finger was seen.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touch_device(std::ptr::null_mut(), &mut false) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `out_touch` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touch_device(ctx: *mut PonkoCtx, out_touch: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_touch, c.input.touch_device())) })
}

/// Says whether this is a touch device (Rust `Input::set_touch_device`):
/// on-screen controls shown `PONKO_TOUCH_SHOW_AUTO` follow it.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_device(std::ptr::null_mut(), true) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_device(ctx: *mut PonkoCtx, touch: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.set_touch_device(touch);
            Ok(())
        })
    })
}

/// Reads the gesture thresholds (Rust `Input::gesture_settings`).
/// ```
/// let mut s = ponko_ffi::PonkoGestureSettings::default();
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_gesture_settings(std::ptr::null_mut(), &mut s) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `out_settings` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_gesture_settings(
    ctx: *mut PonkoCtx,
    out_settings: *mut PonkoGestureSettings,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = c.input.gesture_settings();
            out(
                out_settings,
                PonkoGestureSettings {
                    tap_distance: s.tap_distance,
                    tap_time: s.tap_time,
                    double_tap_time: s.double_tap_time,
                    double_tap_distance: s.double_tap_distance,
                    long_press_time: s.long_press_time,
                    swipe_distance: s.swipe_distance,
                    swipe_time: s.swipe_time,
                    pinch_distance: s.pinch_distance,
                    rotate_angle: s.rotate_angle,
                },
            )
        })
    })
}

/// Sets the gesture thresholds (Rust `Input::set_gesture_settings`). Read
/// them first and change what you need. A negative or non-finite value:
/// `PONKO_INVALID_ARGUMENT`, nothing changed.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_gesture_settings(std::ptr::null_mut(), std::ptr::null()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `settings` readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_gesture_settings(
    ctx: *mut PonkoCtx,
    settings: *const PonkoGestureSettings,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = settings.as_ref().ok_or_else(|| invalid("settings is null"))?;
            let all = [
                s.tap_distance,
                s.tap_time,
                s.double_tap_time,
                s.double_tap_distance,
                s.long_press_time,
                s.swipe_distance,
                s.swipe_time,
                s.pinch_distance,
                s.rotate_angle,
            ];
            if all.iter().any(|v| !v.is_finite() || *v < 0.0) {
                return Err(invalid("gesture settings must be finite and not negative"));
            }
            c.input.set_gesture_settings(GestureSettings {
                tap_distance: s.tap_distance,
                tap_time: s.tap_time,
                double_tap_time: s.double_tap_time,
                double_tap_distance: s.double_tap_distance,
                long_press_time: s.long_press_time,
                swipe_distance: s.swipe_distance,
                swipe_time: s.swipe_time,
                pinch_distance: s.pinch_distance,
                rotate_angle: s.rotate_angle,
            });
            Ok(())
        })
    })
}

/// Puts on-screen controls on the screen (Rust `Input::set_touch_controls`),
/// replacing any: `count` controls, each driving its named action while a
/// finger holds it. `show` is a `PonkoTouchShow`; with `over_screens` false
/// (the usual) they step aside while a UI screen is open, so its buttons
/// take taps. Bad kinds or strings: `PONKO_INVALID_ARGUMENT`, nothing
/// changed.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_controls(std::ptr::null_mut(), std::ptr::null(), 0, 0, false) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `controls` readable for `count` elements (or null
/// with count 0), their strings NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_controls(
    ctx: *mut PonkoCtx,
    controls: *const PonkoTouchControl,
    count: usize,
    show: u32,
    over_screens: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let set = controls_from_c(controls, count, show, over_screens)?;
            c.input.set_touch_controls(set);
            Ok(())
        })
    })
}

/// The Rust set of `count` C controls, checked.
///
/// # Safety
/// `controls` readable for `count` elements (or null with count 0), their
/// strings NUL-terminated.
pub(crate) unsafe fn controls_from_c(
    controls: *const PonkoTouchControl,
    count: usize,
    show: u32,
    over_screens: bool,
) -> Result<TouchControls, Failure> {
    if controls.is_null() && count != 0 {
        return Err(invalid("controls is null"));
    }
    let list = if count == 0 {
        &[]
    } else {
        // SAFETY: caller contract.
        unsafe { std::slice::from_raw_parts(controls, count) }
    };
    let mut set = TouchControls::new().show(show_from_c(show)?).over_screens(over_screens);
    for control in list {
        // SAFETY: caller contract: NUL-terminated strings.
        let action = unsafe { text(control.action)? };
        let label = if control.label.is_null() {
            ""
        } else {
            // SAFETY: as above.
            unsafe { text(control.label)? }
        };
        set = match control.kind {
            0 => set.stick(action),
            1 => set.right_stick(action),
            2 => set.button(action, label),
            3 => set.corner_button(action, label),
            _ => return Err(invalid("unknown touch control kind")),
        };
    }
    Ok(set)
}

fn show_from_c(value: u32) -> Result<TouchShow, Failure> {
    Ok(match value {
        0 => TouchShow::Auto,
        1 => TouchShow::Always,
        2 => TouchShow::Never,
        _ => return Err(invalid("unknown touch show")),
    })
}

/// Takes the on-screen controls away (Rust `Input::set_touch_controls` with an empty set).
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_remove_touch_controls(std::ptr::null_mut()) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_touch_controls(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.set_touch_controls(TouchControls::new());
            Ok(())
        })
    })
}

/// Changes when the on-screen controls show (Rust
/// `Input::set_touch_controls_show`): `show` is a `PonkoTouchShow`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_controls_show(std::ptr::null_mut(), 1) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_controls_show(ctx: *mut PonkoCtx, show: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.input.set_touch_controls_show(show_from_c(show)?);
            Ok(())
        })
    })
}

/// Shows or hides the control for `action` (Rust
/// `Input::set_touch_control_shown`): a hidden one takes no touches. No such
/// control: `PONKO_NOT_FOUND`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_set_touch_control_shown(std::ptr::null_mut(), c"pause".as_ptr(), false) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `action` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_touch_control_shown(
    ctx: *mut PonkoCtx,
    action: *const c_char,
    shown: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let action = text(action)?;
            if c.input.touch_control_center(action).is_none() {
                return Err(error(
                    PonkoStatus::PONKO_NOT_FOUND,
                    "no on-screen control for that action",
                ));
            }
            c.input.set_touch_control_shown(action, shown);
            Ok(())
        })
    })
}

/// Where the control for `action` is, window pixels (Rust
/// `Input::touch_control_center`): what a test touches. No such control:
/// `PONKO_NOT_FOUND`.
/// ```
/// let mut at = ponko_ffi::PonkoVec2::default();
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_touch_control_center(std::ptr::null_mut(), c"jump".as_ptr(), &mut at) } as u32, 0);
/// ```
/// # Safety
/// Context live or null; `action` NUL-terminated; `out_center` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_touch_control_center(
    ctx: *mut PonkoCtx,
    action: *const c_char,
    out_center: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let at = c
                .input
                .touch_control_center(text(action)?)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no on-screen control for that action"))?;
            out(out_center, to_c(at))
        })
    })
}

/// Adds a finger's event at simulation tick `tick` (as
/// `ponko_ctx_feed_touch`). The JSON step is `touch`.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// let at = ponko_ffi::PonkoVec2 { x: 40.0, y: 50.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_touch(s, 2, 1, 0, at) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_touch(
    scenario: *mut PonkoInputScenario,
    tick: u64,
    id: u64,
    phase: u32,
    position: PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            let event = touch_event(id, phase, position)?;
            Ok(move |s: InputScenario| s.at(tick, event))
        })
    }
}

/// Feeds a trackpad gesture, as a Mac's trackpad does (Rust
/// `InputEvent::Trackpad`): `gesture` is `PONKO_GESTURE_PINCH` (`amount`:
/// how much it spread, 0.1 for 10 %), `PONKO_GESTURE_ROTATE` (`amount` in
/// radians, clockwise positive) or `PONKO_GESTURE_DOUBLE_TAP` (`amount`
/// unused); `phase` (a `PonkoTouchPhase`) says whether it begins, goes on
/// or ends. It feeds the same gestures and amounts as two fingers would,
/// at the mouse pointer, and makes no touches and no touch device. Other
/// gestures are ignored. An unknown gesture or phase, or an amount that
/// isn't finite: `PONKO_INVALID_ARGUMENT`.
/// ```
/// assert_ne!(unsafe { ponko_ffi::ponko_ctx_feed_trackpad(std::ptr::null_mut(), 4, 0, 0.25) } as u32, 0);
/// ```
/// # Safety
/// Context live or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_feed_trackpad(
    ctx: *mut PonkoCtx,
    gesture: u32,
    phase: u32,
    amount: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let event = trackpad_event(gesture, phase, amount)?;
        ctx_access(ctx, |c| {
            c.input.feed(event);
            Ok(())
        })
    })
}

fn trackpad_event(gesture: u32, phase: u32, amount: f32) -> Result<InputEvent, Failure> {
    if !amount.is_finite() {
        return Err(invalid("a trackpad amount must be finite"));
    }
    Ok(InputEvent::Trackpad {
        gesture: gesture_from_c(gesture)?,
        phase: phase_from_c(phase)?,
        amount,
    })
}

/// Adds a trackpad gesture at simulation tick `tick` (as
/// `ponko_ctx_feed_trackpad`). The JSON step is `trackpad`.
/// ```
/// let s = ponko_ffi::ponko_input_scenario_new();
/// assert_eq!(unsafe { ponko_ffi::ponko_input_scenario_trackpad(s, 2, 4, 0, 0.25) } as u32, 0);
/// unsafe { ponko_ffi::ponko_input_scenario_free(s) };
/// ```
/// # Safety
/// `scenario` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_input_scenario_trackpad(
    scenario: *mut PonkoInputScenario,
    tick: u64,
    gesture: u32,
    phase: u32,
    amount: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        edit(scenario, || {
            let event = trackpad_event(gesture, phase, amount)?;
            Ok(move |s: InputScenario| s.at(tick, event))
        })
    }
}
