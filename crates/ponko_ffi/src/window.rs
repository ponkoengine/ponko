//! Typed window, display and clipboard bindings. No operation creates a window.
#![deny(missing_docs)]
use crate::ctx::{ctx_access, out, text};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, engine_error, invalid};
use ponko::math::{IVec2, UVec2, Vec2};
use ponko::window::{Cursor, CursorGrab, CursorIcon, VideoMode, WindowEvent, WindowImage, WindowMode};
use std::{
    ffi::c_char,
    mem::{offset_of, size_of},
};
/// How the window is shown (`PonkoWindowModeEx::kind`). Unknown values are rejected.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoWindowModeKind {
    /// An ordinary desktop window.
    PONKO_WINDOW_MODE_WINDOWED = 0,
    /// Borderless fullscreen on a monitor (or the current one).
    PONKO_WINDOW_MODE_BORDERLESS = 1,
    /// Exclusive fullscreen in an advertised video mode of a monitor.
    PONKO_WINDOW_MODE_EXCLUSIVE = 2,
}
/// Cursor capture (`ponko_ctx_window_set_cursor_grab`). Unknown values are rejected.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoCursorGrab {
    /// No capture.
    PONKO_CURSOR_GRAB_NONE = 0,
    /// Kept inside the window.
    PONKO_CURSOR_GRAB_CONFINED = 1,
    /// Locked in place for relative motion (falls back to confined where needed).
    PONKO_CURSOR_GRAB_LOCKED = 2,
}
/// What a `PonkoWindowEventEx` reports.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoWindowEventKind {
    /// Keyboard focus changed; `value` is the new focus.
    PONKO_WINDOW_EVENT_FOCUSED = 0,
    /// Minimized state changed; `value` is the new state.
    PONKO_WINDOW_EVENT_MINIMIZED = 1,
    /// The OS asked to close the window (cancel with `ponko_ctx_window_cancel_close`).
    PONKO_WINDOW_EVENT_CLOSE_REQUESTED = 2,
    /// A file was dropped on the window; its path is written to the path buffer.
    PONKO_WINDOW_EVENT_FILE_DROPPED = 3,
    /// The DPI scale changed; `scale_factor` is the new one.
    PONKO_WINDOW_EVENT_SCALE_FACTOR_CHANGED = 4,
}
/// System cursors (`ponko_ctx_window_set_cursor_icon`), the CSS cursor set. Unknown values are rejected.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoCursorIcon {
    /// The Default system cursor.
    PONKO_CURSOR_DEFAULT = 0,
    /// The ContextMenu system cursor.
    PONKO_CURSOR_CONTEXT_MENU = 1,
    /// The Help system cursor.
    PONKO_CURSOR_HELP = 2,
    /// The Pointer system cursor.
    PONKO_CURSOR_POINTER = 3,
    /// The Progress system cursor.
    PONKO_CURSOR_PROGRESS = 4,
    /// The Wait system cursor.
    PONKO_CURSOR_WAIT = 5,
    /// The Cell system cursor.
    PONKO_CURSOR_CELL = 6,
    /// The Crosshair system cursor.
    PONKO_CURSOR_CROSSHAIR = 7,
    /// The Text system cursor.
    PONKO_CURSOR_TEXT = 8,
    /// The VerticalText system cursor.
    PONKO_CURSOR_VERTICAL_TEXT = 9,
    /// The Alias system cursor.
    PONKO_CURSOR_ALIAS = 10,
    /// The Copy system cursor.
    PONKO_CURSOR_COPY = 11,
    /// The Move system cursor.
    PONKO_CURSOR_MOVE = 12,
    /// The NoDrop system cursor.
    PONKO_CURSOR_NO_DROP = 13,
    /// The NotAllowed system cursor.
    PONKO_CURSOR_NOT_ALLOWED = 14,
    /// The Grab system cursor.
    PONKO_CURSOR_GRAB = 15,
    /// The Grabbing system cursor.
    PONKO_CURSOR_GRABBING = 16,
    /// The EResize system cursor.
    PONKO_CURSOR_E_RESIZE = 17,
    /// The NResize system cursor.
    PONKO_CURSOR_N_RESIZE = 18,
    /// The NeResize system cursor.
    PONKO_CURSOR_NE_RESIZE = 19,
    /// The NwResize system cursor.
    PONKO_CURSOR_NW_RESIZE = 20,
    /// The SResize system cursor.
    PONKO_CURSOR_S_RESIZE = 21,
    /// The SeResize system cursor.
    PONKO_CURSOR_SE_RESIZE = 22,
    /// The SwResize system cursor.
    PONKO_CURSOR_SW_RESIZE = 23,
    /// The WResize system cursor.
    PONKO_CURSOR_W_RESIZE = 24,
    /// The EwResize system cursor.
    PONKO_CURSOR_EW_RESIZE = 25,
    /// The NsResize system cursor.
    PONKO_CURSOR_NS_RESIZE = 26,
    /// The NeswResize system cursor.
    PONKO_CURSOR_NESW_RESIZE = 27,
    /// The NwseResize system cursor.
    PONKO_CURSOR_NWSE_RESIZE = 28,
    /// The ColResize system cursor.
    PONKO_CURSOR_COL_RESIZE = 29,
    /// The RowResize system cursor.
    PONKO_CURSOR_ROW_RESIZE = 30,
    /// The AllScroll system cursor.
    PONKO_CURSOR_ALL_SCROLL = 31,
    /// The ZoomIn system cursor.
    PONKO_CURSOR_ZOOM_IN = 32,
    /// The ZoomOut system cursor.
    PONKO_CURSOR_ZOOM_OUT = 33,
    /// The DndAsk system cursor.
    PONKO_CURSOR_DND_ASK = 34,
    /// The AllResize system cursor.
    PONKO_CURSOR_ALL_RESIZE = 35,
}
/// Sentinel for an unspecified/unavailable monitor index.
pub const PONKO_WINDOW_NO_MONITOR: u32 = u32::MAX;
/// Current PonkoWindowVideoModeEx descriptor version.
pub const PONKO_WINDOW_VIDEO_MODE_EX_VERSION: u32 = 1;
/// Versioned window video mode descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowVideoModeEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_VIDEO_MODE_EX_VERSION.
    pub version: u32,
    /// Physical pixel width.
    pub width: u32,
    /// Physical pixel height.
    pub height: u32,
    /// Refresh rate in millihertz.
    pub refresh_millihertz: u32,
    /// Color depth in bits.
    pub bit_depth: u32,
}
impl Versioned for PonkoWindowVideoModeEx {
    const NAME: &'static str = "PonkoWindowVideoModeEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, bit_depth) + size_of::<u32>())
    }
}
impl Default for PonkoWindowVideoModeEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            width: Default::default(),
            height: Default::default(),
            refresh_millihertz: Default::default(),
            bit_depth: Default::default(),
        }
    }
}
/// Initialize PonkoWindowVideoModeEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_video_mode_ex_defaults(desc: *mut PonkoWindowVideoModeEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowVideoModeEx::default()) })
}
/// Current PonkoWindowModeEx descriptor version.
pub const PONKO_WINDOW_MODE_EX_VERSION: u32 = 1;
/// Versioned window mode descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowModeEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_MODE_EX_VERSION.
    pub version: u32,
    /// A PonkoWindowModeKind value.
    pub kind: u32,
    /// Monitor index, or PONKO_WINDOW_NO_MONITOR for current display.
    pub monitor: u32,
    /// Exclusive physical width.
    pub width: u32,
    /// Exclusive physical height.
    pub height: u32,
    /// Exclusive refresh rate.
    pub refresh_millihertz: u32,
    /// Exclusive color depth.
    pub bit_depth: u32,
}
impl Versioned for PonkoWindowModeEx {
    const NAME: &'static str = "PonkoWindowModeEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, bit_depth) + size_of::<u32>())
    }
}
impl Default for PonkoWindowModeEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            kind: Default::default(),
            monitor: PONKO_WINDOW_NO_MONITOR,
            width: Default::default(),
            height: Default::default(),
            refresh_millihertz: Default::default(),
            bit_depth: Default::default(),
        }
    }
}
/// Initialize PonkoWindowModeEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_mode_ex_defaults(desc: *mut PonkoWindowModeEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowModeEx::default()) })
}
/// Current PonkoWindowPropertiesEx descriptor version.
pub const PONKO_WINDOW_PROPERTIES_EX_VERSION: u32 = 1;
/// Versioned window properties descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowPropertiesEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_PROPERTIES_EX_VERSION.
    pub version: u32,
    /// Requested logical client size.
    pub size: crate::PonkoVec2,
    /// Requested physical desktop X.
    pub position_x: i32,
    /// Requested physical desktop Y.
    pub position_y: i32,
    /// Whether position is set.
    pub has_position: bool,
    /// Minimum logical client size.
    pub min_size: crate::PonkoVec2,
    /// Maximum logical client size.
    pub max_size: crate::PonkoVec2,
    /// Whether minimum size is set.
    pub has_min_size: bool,
    /// Whether maximum size is set.
    pub has_max_size: bool,
    /// Requested topmost level.
    pub always_on_top: bool,
    /// Requested desktop decorations.
    pub decorations: bool,
}
impl Versioned for PonkoWindowPropertiesEx {
    const NAME: &'static str = "PonkoWindowPropertiesEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, decorations) + size_of::<bool>())
    }
}
impl Default for PonkoWindowPropertiesEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            size: crate::PonkoVec2 { x: 800.0, y: 600.0 },
            position_x: Default::default(),
            position_y: Default::default(),
            has_position: Default::default(),
            min_size: Default::default(),
            max_size: Default::default(),
            has_min_size: Default::default(),
            has_max_size: Default::default(),
            always_on_top: Default::default(),
            decorations: true,
        }
    }
}
/// Initialize PonkoWindowPropertiesEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_properties_ex_defaults(desc: *mut PonkoWindowPropertiesEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowPropertiesEx::default()) })
}
/// Current PonkoWindowImageEx descriptor version.
pub const PONKO_WINDOW_IMAGE_EX_VERSION: u32 = 1;
/// Versioned window image descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowImageEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_IMAGE_EX_VERSION.
    pub version: u32,
    /// Physical image width.
    pub width: u32,
    /// Physical image height.
    pub height: u32,
    /// Copied RGBA8 pixels, null only for zero length.
    pub rgba: *const u8,
    /// Number of bytes at rgba.
    pub rgba_len: usize,
    /// Custom cursor hotspot X in image pixels; ignored for icons.
    pub hotspot_x: u32,
    /// Custom cursor hotspot Y in image pixels; ignored for icons.
    pub hotspot_y: u32,
}
impl Versioned for PonkoWindowImageEx {
    const NAME: &'static str = "PonkoWindowImageEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, hotspot_y) + size_of::<u32>())
    }
}
impl Default for PonkoWindowImageEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            width: Default::default(),
            height: Default::default(),
            rgba: std::ptr::null(),
            rgba_len: Default::default(),
            hotspot_x: Default::default(),
            hotspot_y: Default::default(),
        }
    }
}
/// Initialize PonkoWindowImageEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_image_ex_defaults(desc: *mut PonkoWindowImageEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowImageEx::default()) })
}
/// Current PonkoWindowMonitorEx descriptor version.
pub const PONKO_WINDOW_MONITOR_EX_VERSION: u32 = 1;
/// Versioned window monitor descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowMonitorEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_MONITOR_EX_VERSION.
    pub version: u32,
    /// Physical desktop origin X.
    pub position_x: i32,
    /// Physical desktop origin Y.
    pub position_y: i32,
    /// Physical display width.
    pub width: u32,
    /// Physical display height.
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub scale_factor: f32,
    /// Current refresh, or zero if unavailable.
    pub refresh_millihertz: u32,
}
impl Versioned for PonkoWindowMonitorEx {
    const NAME: &'static str = "PonkoWindowMonitorEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, refresh_millihertz) + size_of::<u32>())
    }
}
impl Default for PonkoWindowMonitorEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            position_x: Default::default(),
            position_y: Default::default(),
            width: Default::default(),
            height: Default::default(),
            scale_factor: 1.0,
            refresh_millihertz: Default::default(),
        }
    }
}
/// Initialize PonkoWindowMonitorEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_monitor_ex_defaults(desc: *mut PonkoWindowMonitorEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowMonitorEx::default()) })
}
/// Current PonkoWindowStateEx descriptor version.
pub const PONKO_WINDOW_STATE_EX_VERSION: u32 = 1;
/// Versioned window state descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowStateEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_STATE_EX_VERSION.
    pub version: u32,
    /// Observed physical client width.
    pub width: u32,
    /// Observed physical client height.
    pub height: u32,
    /// Physical pixels per logical pixel.
    pub scale_factor: f32,
    /// Observed focus.
    pub focused: bool,
    /// Observed minimized state.
    pub minimized: bool,
    /// Current monitor index, or PONKO_WINDOW_NO_MONITOR.
    pub monitor: u32,
}
impl Versioned for PonkoWindowStateEx {
    const NAME: &'static str = "PonkoWindowStateEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, monitor) + size_of::<u32>())
    }
}
impl Default for PonkoWindowStateEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            width: Default::default(),
            height: Default::default(),
            scale_factor: 1.0,
            focused: Default::default(),
            minimized: Default::default(),
            monitor: PONKO_WINDOW_NO_MONITOR,
        }
    }
}
/// Initialize PonkoWindowStateEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_state_ex_defaults(desc: *mut PonkoWindowStateEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowStateEx::default()) })
}
/// Current PonkoWindowEventEx descriptor version.
pub const PONKO_WINDOW_EVENT_EX_VERSION: u32 = 1;
/// Versioned window event descriptor. Initialize size/version, then defaults.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoWindowEventEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to PONKO_WINDOW_EVENT_EX_VERSION.
    pub version: u32,
    /// A PonkoWindowEventKind value.
    pub kind: u32,
    /// Focus/minimized value, ignored for other kinds.
    pub value: bool,
    /// New DPI scale, only for ScaleFactorChanged.
    pub scale_factor: f32,
}
impl Versioned for PonkoWindowEventEx {
    const NAME: &'static str = "PonkoWindowEventEx";
    const VERSION: u32 = 1;
    fn len_of(v: u32) -> Option<usize> {
        (v == 1).then_some(offset_of!(Self, scale_factor) + size_of::<f32>())
    }
}
impl Default for PonkoWindowEventEx {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            version: 1,
            kind: Default::default(),
            value: Default::default(),
            scale_factor: Default::default(),
        }
    }
}
/// Initialize PonkoWindowEventEx, preserving the caller header and unknown trailing bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_window_event_ex_defaults(desc: *mut PonkoWindowEventEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { write_desc(desc, &PonkoWindowEventEx::default()) })
}

fn mode_from(d: PonkoWindowModeEx) -> Result<WindowMode, Failure> {
    let monitor = (d.monitor != PONKO_WINDOW_NO_MONITOR).then_some(d.monitor as usize);
    Ok(match d.kind {
        0 => WindowMode::Windowed,
        1 => WindowMode::Borderless(monitor),
        2 => WindowMode::Exclusive(
            monitor
                .ok_or_else(|| invalid("exclusive fullscreen needs a monitor; enumerate connected monitors first"))?,
            VideoMode::new(
                d.width,
                d.height,
                d.refresh_millihertz,
                d.bit_depth
                    .try_into()
                    .map_err(|_| invalid("bit depth exceeds u16; select an advertised video mode"))?,
            ),
        ),
        _ => return Err(invalid("unknown window mode kind; use PonkoWindowModeKind")),
    })
}
fn mode_to(m: WindowMode) -> PonkoWindowModeEx {
    let mut d = PonkoWindowModeEx::default();
    match m {
        WindowMode::Windowed => {}
        WindowMode::Borderless(i) => {
            d.kind = 1;
            d.monitor = i.map_or(PONKO_WINDOW_NO_MONITOR, |i| i as u32);
        }
        WindowMode::Exclusive(i, v) => {
            d.kind = 2;
            d.monitor = i as u32;
            d.width = v.width;
            d.height = v.height;
            d.refresh_millihertz = v.refresh_millihertz;
            d.bit_depth = v.bit_depth.into();
        }
        _ => {}
    }
    d
}
unsafe fn image_from(d: PonkoWindowImageEx) -> Result<WindowImage, Failure> {
    if d.rgba.is_null() && d.rgba_len != 0 {
        return Err(invalid("RGBA image pointer is null; supply width * height * 4 bytes"));
    }
    if (d.width as usize)
        .checked_mul(d.height as usize)
        .and_then(|n| n.checked_mul(4))
        != Some(d.rgba_len)
        || d.width == 0
        || d.height == 0
    {
        return Err(invalid(
            "RGBA image dimensions/length disagree; supply width * height * 4 bytes",
        ));
    }
    // SAFETY: caller guarantees readable, non-null pixels for rgba_len bytes; checked above.
    Ok(WindowImage::new(
        d.width,
        d.height,
        unsafe { std::slice::from_raw_parts(d.rgba, d.rgba_len) }.to_vec(),
    ))
}
/// An engine error as a status: PONKO_NOT_FOUND for a monitor or mode that is gone,
/// PONKO_UNSUPPORTED for what this platform cannot do (the web, no clipboard),
/// PONKO_INVALID_ARGUMENT otherwise. `ponko_last_error_kind` gives the kind either way.
fn window_error(e: ponko::error::Error) -> Failure {
    let unsupported = e.kind() == ponko::error::ErrorKind::Unsupported;
    let mut failure = engine_error(e);
    if unsupported {
        failure.0 = PonkoStatus::PONKO_UNSUPPORTED;
    }
    failure
}
fn monitor_gone() -> ponko::error::Error {
    ponko::error::Error::new(
        ponko::error::ErrorKind::NotFound,
        "no monitor at that index (disconnected?); call ponko_ctx_window_monitor_count again and pick a listed one",
    )
}
fn cursor_icon(value: u32) -> Result<CursorIcon, Failure> {
    CursorIcon::ALL
        .get(value as usize)
        .copied()
        .ok_or_else(|| invalid("unknown cursor icon; use PonkoCursorIcon"))
}
/// Read the observed fullscreen mode.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_mode_ex(ctx: *mut PonkoCtx, desc: *mut PonkoWindowModeEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| write_desc(desc, &mode_to(c.window.mode()))) })
}
/// Switch fullscreen mode; web supports only windowed/borderless-current-display.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_mode_ex(
    ctx: *mut PonkoCtx,
    desc: *const PonkoWindowModeEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let d = read_desc(desc, PonkoWindowModeEx::default())?;
        let mode = mode_from(d)?;
        ctx_access(ctx, |c| c.window.set_mode(mode).map_err(window_error))
    })
}
/// Read requested properties; title and fullscreen have their own calls.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_properties_ex(
    ctx: *mut PonkoCtx,
    desc: *mut PonkoWindowPropertiesEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let v = c.window.settings();
            let d = PonkoWindowPropertiesEx {
                size: crate::PonkoVec2 {
                    x: v.size.x,
                    y: v.size.y,
                },
                position_x: v.position.unwrap_or_default().x,
                position_y: v.position.unwrap_or_default().y,
                has_position: v.position.is_some(),
                min_size: crate::PonkoVec2 {
                    x: v.min_size.unwrap_or_default().x,
                    y: v.min_size.unwrap_or_default().y,
                },
                max_size: crate::PonkoVec2 {
                    x: v.max_size.unwrap_or_default().x,
                    y: v.max_size.unwrap_or_default().y,
                },
                has_min_size: v.min_size.is_some(),
                has_max_size: v.max_size.is_some(),
                always_on_top: v.always_on_top,
                decorations: v.decorations,
                ..Default::default()
            };
            write_desc(desc, &d)
        })
    })
}
/// Sets the window title (copied UTF-8). On the web the document title is left alone.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_title(ctx: *mut PonkoCtx, value: *const c_char) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let value = text(value)?;
        ctx_access(ctx, |c| c.window.set_title(value).map_err(window_error))
    })
}
/// Requests a logical client size; both sides positive and finite. The OS or page may adjust it.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_size(ctx: *mut PonkoCtx, size: crate::PonkoVec2) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.window.set_size(Vec2::new(size.x, size.y)).map_err(window_error)
        })
    })
}
/// Moves the window to a physical desktop position (can be negative). PONKO_UNSUPPORTED on the web.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_position(ctx: *mut PonkoCtx, x: i32, y: i32) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.set_position(IVec2::new(x, y)).map_err(window_error)) })
}
/// Sets the minimum and maximum logical client size; a null pointer removes that limit. PONKO_UNSUPPORTED on the web.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_resize_limits(
    ctx: *mut PonkoCtx,
    min: *const crate::PonkoVec2,
    max: *const crate::PonkoVec2,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let min = min.as_ref().map(|v| Vec2::new(v.x, v.y));
        let max = max.as_ref().map(|v| Vec2::new(v.x, v.y));
        ctx_access(ctx, |c| c.window.set_resize_limits(min, max).map_err(window_error))
    })
}
/// Keeps the window above others (true) or not. PONKO_UNSUPPORTED on the web.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_always_on_top(ctx: *mut PonkoCtx, value: bool) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.set_always_on_top(value).map_err(window_error)) })
}
/// Shows or hides the title bar and borders. PONKO_UNSUPPORTED on the web.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_decorations(ctx: *mut PonkoCtx, value: bool) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.set_decorations(value).map_err(window_error)) })
}
/// Sets the window icon from copied RGBA8 pixels; null restores the default (hotspot fields are ignored). PONKO_UNSUPPORTED on the web.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_icon_ex(
    ctx: *mut PonkoCtx,
    desc: *const PonkoWindowImageEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let image = if desc.is_null() {
            None
        } else {
            Some(image_from(read_desc(desc, PonkoWindowImageEx::default())?)?)
        };
        ctx_access(ctx, |c| c.window.set_icon(image).map_err(window_error))
    })
}
/// Shows a system cursor (a PonkoCursorIcon value).
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_cursor_icon(ctx: *mut PonkoCtx, icon: u32) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let icon = cursor_icon(icon)?;
        ctx_access(ctx, |c| c.window.set_cursor(Cursor::System(icon)).map_err(window_error))
    })
}
/// Shows a custom cursor from copied RGBA8 pixels; the hotspot must lie inside the image and each side be at most 65535.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_cursor_image_ex(
    ctx: *mut PonkoCtx,
    desc: *const PonkoWindowImageEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let d = read_desc(desc, PonkoWindowImageEx::default())?;
        let cursor = Cursor::Custom(image_from(d)?, UVec2::new(d.hotspot_x, d.hotspot_y));
        ctx_access(ctx, |c| c.window.set_cursor(cursor).map_err(window_error))
    })
}
/// Shows or hides the cursor, independently of capture.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_cursor_visible(ctx: *mut PonkoCtx, value: bool) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.set_cursor_visible(value).map_err(window_error)) })
}
/// Releases, confines or locks the cursor (a PonkoCursorGrab value). Lock falls back to confinement where the OS needs it; on the web it needs a user gesture.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_set_cursor_grab(ctx: *mut PonkoCtx, grab: u32) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let grab = match grab {
            0 => CursorGrab::None,
            1 => CursorGrab::Confined,
            2 => CursorGrab::Locked,
            _ => return Err(invalid("unknown cursor grab; use PonkoCursorGrab")),
        };
        ctx_access(ctx, |c| c.window.set_cursor_grab(grab).map_err(window_error))
    })
}
/// Whether the OS asked to close the window and no system canceled it yet this frame.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_close_requested(ctx: *mut PonkoCtx, value: *mut bool) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        if value.is_null() {
            return Err(invalid("close-request output is null"));
        }
        ctx_access(ctx, |c| out(value, c.window.close_requested()))
    })
}
/// Cancels the pending OS close request, so the game keeps running. `ponko_ctx_quit` still quits.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_cancel_close(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.window.cancel_close();
            Ok(())
        })
    })
}
/// Saves the requested window settings (mode, size, position, title, limits, flags) to `window.json` in the game's player storage.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_save_settings(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.save_settings(&mut c.storage).map_err(window_error)) })
}
/// Opens the first window with the player's saved settings (Rust
/// `Game::restore_window_settings`): `window.json` from the game's storage
/// (`ponko_ctx_window_save_settings` writes it) gives the window's mode,
/// size, position and flags before it exists, so it doesn't show at its
/// default size first. The game needs an app id (`ponko_game_app_id`) or a
/// data folder; with no saved file, or one that can't be used, the window
/// opens with the defaults. A headless run applies the settings to the
/// context's window before setup. Off by default.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_game_restore_window_settings(std::ptr::null_mut(), true) } as u32, 1);
/// ```
/// # Safety
/// `game` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_restore_window_settings(game: *mut crate::PonkoGame, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { crate::game::build(game, |g| crate::game::ok(g.restore_window_settings(on))) }
}
/// Restores `window.json` from player storage, if there is one. A stored monitor or exclusive mode that is gone falls back to windowed (the reason is logged).
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_restore_settings(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe { ctx_access(ctx, |c| c.window.restore_settings(&c.storage).map_err(window_error)) })
}
/// Writes the number of connected monitors (0 headless). Indices are valid until monitors change.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_monitor_count(ctx: *mut PonkoCtx, count: *mut usize) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        if count.is_null() {
            return Err(invalid("monitor count output is null"));
        }
        ctx_access(ctx, |c| out(count, c.window.monitors().len()))
    })
}
/// Writes how many exclusive fullscreen modes monitor `monitor` advertises. NOT_FOUND if it is disconnected.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_video_mode_count(
    ctx: *mut PonkoCtx,
    monitor: u32,
    count: *mut usize,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        if count.is_null() {
            return Err(invalid("mode count output is null"));
        }
        ctx_access(ctx, |c| {
            let modes = c.window.exclusive_modes(monitor as usize).map_err(window_error)?;
            out(count, modes.len())
        })
    })
}
/// Writes how many window events (PonkoWindowEventEx) this system can read this frame.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_event_count(ctx: *mut PonkoCtx, count: *mut usize) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        if count.is_null() {
            return Err(invalid("event count output is null"));
        }
        ctx_access(ctx, |c| out(count, c.events.peek::<WindowEvent>().len()))
    })
}
/// Read requested title as UTF-8; length excludes NUL; retry with length+1 bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_title(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    length: *mut usize,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            crate::write_text(buffer, capacity, length, &c.window.settings().title)
        })
    })
}
/// Read a connected display; refresh indices after hot-plug.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_monitor_ex(
    ctx: *mut PonkoCtx,
    index: u32,
    desc: *mut PonkoWindowMonitorEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let monitors = c.window.monitors();
            let m = monitors
                .get(index as usize)
                .ok_or_else(|| window_error(monitor_gone()))?;
            write_desc(
                desc,
                &PonkoWindowMonitorEx {
                    position_x: m.position.x,
                    position_y: m.position.y,
                    width: m.size.x,
                    height: m.size.y,
                    scale_factor: m.scale_factor,
                    refresh_millihertz: m.refresh_millihertz.unwrap_or(0),
                    ..Default::default()
                },
            )
        })
    })
}
/// Read monitor name as UTF-8, empty if not exposed; standard text buffer convention.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_monitor_name(
    ctx: *mut PonkoCtx,
    index: u32,
    buffer: *mut c_char,
    capacity: usize,
    length: *mut usize,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let monitors = c.window.monitors();
            let m = monitors
                .get(index as usize)
                .ok_or_else(|| window_error(monitor_gone()))?;
            crate::write_text(buffer, capacity, length, m.name.as_deref().unwrap_or(""))
        })
    })
}
/// Read one advertised exclusive mode.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_video_mode_ex(
    ctx: *mut PonkoCtx,
    monitor: u32,
    index: u32,
    desc: *mut PonkoWindowVideoModeEx,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let modes = c.window.exclusive_modes(monitor as usize).map_err(window_error)?;
            let v = modes
                .get(index as usize)
                .ok_or_else(|| invalid("video mode index is unavailable; refresh the mode list"))?;
            write_desc(
                desc,
                &PonkoWindowVideoModeEx {
                    width: v.width,
                    height: v.height,
                    refresh_millihertz: v.refresh_millihertz,
                    bit_depth: v.bit_depth.into(),
                    ..Default::default()
                },
            )
        })
    })
}
/// Read observed state; use mode_ex for observed fullscreen mode.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_state_ex(ctx: *mut PonkoCtx, desc: *mut PonkoWindowStateEx) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let v = c.window.state();
            write_desc(
                desc,
                &PonkoWindowStateEx {
                    width: v.physical_size.x,
                    height: v.physical_size.y,
                    scale_factor: v.scale_factor,
                    focused: v.focused,
                    minimized: v.minimized,
                    monitor: v.monitor.map_or(PONKO_WINDOW_NO_MONITOR, |i| i as u32),
                    ..Default::default()
                },
            )
        })
    })
}
/// Read a non-consuming lifecycle event and optional dropped path. Path length excludes NUL; retry with length+1 bytes.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_window_event_ex(
    ctx: *mut PonkoCtx,
    index: usize,
    desc: *mut PonkoWindowEventEx,
    path: *mut c_char,
    capacity: usize,
    length: *mut usize,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let _ = read_desc(desc, PonkoWindowEventEx::default())?;
        if length.is_null() || (capacity != 0 && path.is_null()) {
            return Err(invalid("event path output is null"));
        }
        ctx_access(ctx, |c| {
            let event = c
                .events
                .peek::<WindowEvent>()
                .get(index)
                .ok_or_else(|| invalid("window event index is unavailable; read event count first"))?;
            let mut d = PonkoWindowEventEx::default();
            let mut dropped = None;
            match event {
                WindowEvent::Focused(v) => {
                    d.kind = 0;
                    d.value = *v;
                }
                WindowEvent::Minimized(v) => {
                    d.kind = 1;
                    d.value = *v;
                }
                WindowEvent::CloseRequested => d.kind = 2,
                WindowEvent::FileDropped(p) => {
                    d.kind = 3;
                    dropped = Some(p.to_string_lossy());
                }
                WindowEvent::ScaleFactorChanged(v) => {
                    d.kind = 4;
                    d.scale_factor = *v;
                }
                _ => return Err(invalid("window event kind requires a newer native library")),
            }
            crate::write_text(path, capacity, length, dropped.as_deref().unwrap_or(""))?;
            write_desc(desc, &d)
        })
    })
}
/// Write UTF-8 clipboard text; web submits asynchronously and requires permission/activation.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clipboard_write(ctx: *mut PonkoCtx, value: *const c_char) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        let value = text(value)?;
        ctx_access(ctx, |c| c.window.clipboard_write(value).map_err(window_error))
    })
}
/// Reads clipboard text as UTF-8 with the usual text-buffer rule: `length` gets the length without
/// the NUL, the text is written only if `capacity > length`, so call with capacity 0 first and
/// again with length + 1 bytes (the clipboard is read again; if it grew, `length` says so).
/// `available` is false headless, with no text on the clipboard, or while a browser read is
/// pending; `length` is then 0. Errors (no clipboard, permission denied) leave the buffer alone.
/// # Safety
/// Contexts must be null or live outside queries. Descriptor headers must be initialized;
/// descriptors and buffers must be valid for their stated sizes, output pointers writable,
/// input strings NUL-terminated. No pointer is retained.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clipboard_read(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    length: *mut usize,
    available: *mut bool,
) -> PonkoStatus {
    // SAFETY: all pointer use follows the caller contract; inputs are checked before mutation.
    boundary(|| unsafe {
        if length.is_null() || available.is_null() || (capacity != 0 && buffer.is_null()) {
            return Err(invalid(
                "clipboard output is null; supply writable length/available and a valid buffer",
            ));
        }
        ctx_access(ctx, |c| {
            let text = c.window.clipboard_read().map_err(window_error)?;
            crate::write_text(buffer, capacity, length, text.as_deref().unwrap_or(""))?;
            out(available, text.is_some())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ponko::error::Error;
    use ponko::window::{Monitor, WindowBackend, WindowRequest, WindowService, WindowState};
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct Fake {
        state: WindowState,
        monitors: bool,
        clipboard: Option<String>,
        web: bool,
    }
    struct Backend(Rc<RefCell<Fake>>);
    impl WindowBackend for Backend {
        fn request(&mut self, r: &WindowRequest) -> Result<(), Error> {
            if self.0.borrow().web && matches!(r, WindowRequest::Position(_)) {
                return Err(Error::new(ponko::error::ErrorKind::Unsupported, "cannot move a canvas"));
            }
            if let WindowRequest::Mode(m) = r {
                self.0.borrow_mut().state.mode = m.clone();
            }
            Ok(())
        }
        fn state(&self) -> WindowState {
            self.0.borrow().state.clone()
        }
        fn monitors(&self) -> Vec<Monitor> {
            if !self.0.borrow().monitors {
                return Vec::new();
            }
            let mut m = Monitor::default();
            m.name = Some("Left".into());
            m.position = IVec2::new(-1920, 0);
            m.size = UVec2::new(1920, 1080);
            m.scale_factor = 1.5;
            m.refresh_millihertz = Some(144_000);
            m.modes = vec![
                VideoMode::new(1280, 720, 60_000, 32),
                VideoMode::new(1920, 1080, 144_000, 32),
            ];
            vec![m]
        }
        fn clipboard_read(&mut self) -> Result<Option<String>, Error> {
            Ok(self.0.borrow().clipboard.clone())
        }
        fn clipboard_write(&mut self, text: &str) -> Result<(), Error> {
            self.0.borrow_mut().clipboard = Some(text.into());
            Ok(())
        }
    }

    fn run(body: impl FnOnce(&mut ponko::prelude::Ctx, Rc<RefCell<Fake>>)) {
        let mut body = Some(body);
        ponko::prelude::Game::new("window-ffi", 1, 1).run_headless(0, |ctx| {
            let fake = Rc::new(RefCell::new(Fake {
                monitors: true,
                ..Default::default()
            }));
            ctx.window = WindowService::with_backend(Backend(fake.clone()));
            (body.take().unwrap())(ctx, fake);
        });
    }

    #[test]
    fn monitors_modes_and_names_are_read_through_typed_descriptors() {
        run(|ctx, fake| {
            fake.borrow_mut().state.monitor = Some(0);
            crate::ctx::with_ctx(ctx, |c| unsafe {
                let mut n = 0;
                assert_eq!(ponko_ctx_window_monitor_count(c, &mut n), PonkoStatus::PONKO_OK);
                assert_eq!(n, 1);
                let mut m = PonkoWindowMonitorEx::default();
                assert_eq!(ponko_ctx_window_monitor_ex(c, 0, &mut m), PonkoStatus::PONKO_OK);
                assert_eq!(
                    (m.position_x, m.width, m.scale_factor, m.refresh_millihertz),
                    (-1920, 1920, 1.5, 144_000)
                );
                let mut name = [0 as c_char; 16];
                let mut len = 0;
                assert_eq!(
                    ponko_ctx_window_monitor_name(c, 0, name.as_mut_ptr(), name.len(), &mut len),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!(std::ffi::CStr::from_ptr(name.as_ptr()).to_str().unwrap(), "Left");
                assert_eq!(ponko_ctx_window_video_mode_count(c, 0, &mut n), PonkoStatus::PONKO_OK);
                assert_eq!(n, 2);
                let mut v = PonkoWindowVideoModeEx::default();
                assert_eq!(ponko_ctx_window_video_mode_ex(c, 0, 1, &mut v), PonkoStatus::PONKO_OK);
                assert_eq!(
                    (v.width, v.height, v.refresh_millihertz, v.bit_depth),
                    (1920, 1080, 144_000, 32)
                );
                assert_eq!(
                    ponko_ctx_window_video_mode_ex(c, 0, 2, &mut v),
                    PonkoStatus::PONKO_INVALID_ARGUMENT
                );
                assert_eq!(ponko_ctx_window_monitor_ex(c, 1, &mut m), PonkoStatus::PONKO_NOT_FOUND);
                assert_eq!(
                    ponko_ctx_window_video_mode_count(c, 1, &mut n),
                    PonkoStatus::PONKO_NOT_FOUND
                );

                // An advertised exclusive mode round-trips through the mode descriptor.
                let mut mode = PonkoWindowModeEx {
                    kind: PonkoWindowModeKind::PONKO_WINDOW_MODE_EXCLUSIVE as u32,
                    monitor: 0,
                    width: v.width,
                    height: v.height,
                    refresh_millihertz: v.refresh_millihertz,
                    bit_depth: v.bit_depth,
                    ..Default::default()
                };
                assert_eq!(ponko_ctx_window_set_mode_ex(c, &mode), PonkoStatus::PONKO_OK);
                let mut now = PonkoWindowModeEx::default();
                assert_eq!(ponko_ctx_window_mode_ex(c, &mut now), PonkoStatus::PONKO_OK);
                assert_eq!(
                    (now.kind, now.monitor, now.width, now.refresh_millihertz),
                    (2, 0, 1920, 144_000)
                );
                let mut state = PonkoWindowStateEx::default();
                assert_eq!(ponko_ctx_window_state_ex(c, &mut state), PonkoStatus::PONKO_OK);
                assert_eq!(state.monitor, 0);
                mode.kind = 7;
                assert_eq!(
                    ponko_ctx_window_set_mode_ex(c, &mode),
                    PonkoStatus::PONKO_INVALID_ARGUMENT
                );
            });
        });
    }

    #[test]
    fn platform_restrictions_return_unsupported_with_the_engine_message() {
        run(|ctx, fake| {
            fake.borrow_mut().web = true;
            crate::ctx::with_ctx(ctx, |c| unsafe {
                assert_eq!(ponko_ctx_window_set_position(c, 10, 10), PonkoStatus::PONKO_UNSUPPORTED);
                assert_eq!(
                    crate::ponko_last_error_kind(),
                    crate::PonkoErrorKind::PONKO_ERROR_UNSUPPORTED
                );
                assert_eq!(
                    ponko_ctx_window_set_title(c, c"still fine".as_ptr()),
                    PonkoStatus::PONKO_OK
                );
            });
        });
    }

    #[test]
    fn window_events_are_readable_from_c_without_consuming_them() {
        run(|ctx, _| {
            ctx.events.send(WindowEvent::Focused(true));
            ctx.events.send(WindowEvent::FileDropped("levels/one.map".into()));
            ctx.events.send(WindowEvent::ScaleFactorChanged(2.0));
            crate::ctx::with_ctx(ctx, |c| unsafe {
                let mut n = 0;
                assert_eq!(ponko_ctx_window_event_count(c, &mut n), PonkoStatus::PONKO_OK);
                assert_eq!(n, 3);
                let mut e = PonkoWindowEventEx::default();
                let mut path = [0 as c_char; 64];
                let mut len = 0;
                assert_eq!(
                    ponko_ctx_window_event_ex(c, 0, &mut e, path.as_mut_ptr(), path.len(), &mut len),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!((e.kind, e.value, len), (0, true, 0));
                assert_eq!(
                    ponko_ctx_window_event_ex(c, 1, &mut e, path.as_mut_ptr(), path.len(), &mut len),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!(e.kind, PonkoWindowEventKind::PONKO_WINDOW_EVENT_FILE_DROPPED as u32);
                assert_eq!(
                    std::ffi::CStr::from_ptr(path.as_ptr()).to_str().unwrap(),
                    "levels/one.map"
                );
                assert_eq!(
                    ponko_ctx_window_event_ex(c, 2, &mut e, path.as_mut_ptr(), path.len(), &mut len),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!((e.kind, e.scale_factor), (4, 2.0));
            });
            assert_eq!(
                ctx.events.peek::<WindowEvent>().len(),
                3,
                "reading from C does not consume"
            );
        });
    }

    #[test]
    fn clipboard_reads_into_a_caller_buffer_after_a_size_probe() {
        run(|ctx, fake| {
            crate::ctx::with_ctx(ctx, |c| unsafe {
                let (mut len, mut available) = (9, true);
                assert_eq!(
                    ponko_ctx_clipboard_read(c, std::ptr::null_mut(), 0, &mut len, &mut available),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!((len, available), (0, false), "nothing on the clipboard yet");
                assert_eq!(
                    ponko_ctx_clipboard_write(c, c"hello 世界".as_ptr()),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!(
                    ponko_ctx_clipboard_read(c, std::ptr::null_mut(), 0, &mut len, &mut available),
                    PonkoStatus::PONKO_OK
                );
                assert!(available);
                let mut small = vec![b'x' as c_char; len];
                assert_eq!(
                    ponko_ctx_clipboard_read(c, small.as_mut_ptr(), small.len(), &mut len, &mut available),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!(small[0], 0, "a buffer without room for the NUL gets an empty string");
                let mut buffer = vec![0 as c_char; len + 1];
                assert_eq!(
                    ponko_ctx_clipboard_read(c, buffer.as_mut_ptr(), buffer.len(), &mut len, &mut available),
                    PonkoStatus::PONKO_OK
                );
                assert_eq!(
                    std::ffi::CStr::from_ptr(buffer.as_ptr()).to_str().unwrap(),
                    "hello 世界"
                );
            });
            assert_eq!(fake.borrow().clipboard.as_deref(), Some("hello 世界"));
        });
    }
}
