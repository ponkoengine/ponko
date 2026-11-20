//! Debug drawing from C (Rust `ctx.debug`, `ponko::debug`): lines, shapes
//! and labels in 2D and 3D, kept until the system that drew them runs again
//! (or for `PonkoDebugStyle::seconds`), and built-in views of colliders,
//! navmeshes, bounds, lights and the hierarchy. While drawing is off (the
//! default in release builds of the engine) every call returns at once.
//! The shape calls work inside a `ponko_query` callback, like commands:
//! a shape for each entity the query visits (not in `ponko_query_par`).

use std::ffi::c_char;

use ponko::debug::{DebugDraw, DebugShape, DebugView};
use ponko::prelude::{Color, Ctx, Quat, Vec2, Vec3};

use crate::ctx::{ctx_access, debug_draw, out, text};
use crate::descriptors::{Versioned, read_desc, write_desc};
use crate::three::{PonkoCamera3D, PonkoQuat, PonkoVec3, camera_from_c, quat_from_c};
use crate::types::{PonkoColor, PonkoDrawSpace, PonkoVec2, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid};

/// How a shape is drawn (Rust `DebugShape`'s settings). Start from
/// `ponko_debug_style_default()`; pass null for the defaults.
/// ```
/// let s = ponko_ffi::ponko_debug_style_default();
/// assert_eq!((s.seconds, s.width, s.size, s.on_top, s.screen), (0.0, 1.0, 14.0, false, false));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoDebugStyle {
    /// Keep the shape this many seconds of frame time; 0 (the default)
    /// keeps it until the system that drew it runs again.
    pub seconds: f32,
    /// Line width in pixels. Default 1.
    pub width: f32,
    /// Label height in pixels. Default 14.
    pub size: f32,
    /// Draw 3D lines over nearer surfaces, and 2D lines over the UI.
    pub on_top: bool,
    /// 2D points are window pixels from the top-left corner instead of
    /// world units (ignored by 3D shapes).
    pub screen: bool,
}

/// The default style: until the system runs again, 1 pixel wide, labels
/// 14 pixels high, depth-tested, in the world.
#[unsafe(no_mangle)]
pub extern "C" fn ponko_debug_style_default() -> PonkoDebugStyle {
    PonkoDebugStyle {
        seconds: 0.0,
        width: 1.0,
        size: 14.0,
        on_top: false,
        screen: false,
    }
}

/// Current version of [`PonkoDebugStyleEx`].
pub const PONKO_DEBUG_STYLE_EX_VERSION: u32 = 1;

/// Versioned style for 2D debug drawing. Set the header and call
/// `ponko_debug_style_ex_defaults` before customizing its fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoDebugStyleEx {
    /// Caller allocation size in bytes.
    pub struct_size: u32,
    /// Set to `PONKO_DEBUG_STYLE_EX_VERSION`.
    pub version: u32,
    /// Lifetime in frame seconds. Zero keeps the shape until its system runs again.
    pub seconds: f32,
    /// Line width in pixels.
    pub width: f32,
    /// Label height in pixels.
    pub size: f32,
    /// Draw over the UI.
    pub on_top: bool,
    /// Coordinate space (a `PonkoDrawSpace` value).
    pub space: u32,
}

impl Default for PonkoDebugStyleEx {
    fn default() -> Self {
        let style = ponko_debug_style_default();
        Self {
            struct_size: size_of::<Self>() as u32,
            version: PONKO_DEBUG_STYLE_EX_VERSION,
            seconds: style.seconds,
            width: style.width,
            size: style.size,
            on_top: style.on_top,
            space: PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD as u32,
        }
    }
}

impl Versioned for PonkoDebugStyleEx {
    const NAME: &'static str = "PonkoDebugStyleEx";
    const VERSION: u32 = PONKO_DEBUG_STYLE_EX_VERSION;

    fn len_of(version: u32) -> Option<usize> {
        (version == 1).then_some(std::mem::offset_of!(Self, space) + size_of::<u32>())
    }
}

/// Writes the default 2D style, preserving the caller's header and trailing bytes.
/// # Safety
/// `style` null or writable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_debug_style_ex_defaults(style: *mut PonkoDebugStyleEx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { write_desc(style, &PonkoDebugStyleEx::default()) })
}

unsafe fn with_style_ex(
    style: *const PonkoDebugStyleEx,
    draw: impl FnOnce(&PonkoDebugStyle) -> PonkoStatus,
) -> PonkoStatus {
    // SAFETY: the caller passes null or a readable descriptor.
    let style = match unsafe { read_desc(style, PonkoDebugStyleEx::default()) } {
        Ok(style) => style,
        Err(err) => return boundary(|| Err(err)),
    };
    let space = match style.space {
        n if n == PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD as u32 => PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD,
        n if n == PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN as u32 => PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN,
        _ => return boundary(|| Err(invalid("space is not a PonkoDrawSpace"))),
    };
    draw(&ponko_debug_style_with_space(
        PonkoDebugStyle {
            seconds: style.seconds,
            width: style.width,
            size: style.size,
            on_top: style.on_top,
            screen: false,
        },
        space,
    ))
}

/// Returns `style` with its 2D points in `space` (Rust `DebugShape::space`),
/// every other setting as it was: the same `PonkoDrawSpace` sprite draws
/// take (`ponko_draw_params_with_space`). 3D shapes stay in the 3D world.
/// ```
/// use ponko_ffi::*;
/// let s = ponko_debug_style_with_space(ponko_debug_style_default(), PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN);
/// assert!(s.screen);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_debug_style_with_space(mut style: PonkoDebugStyle, space: PonkoDrawSpace) -> PonkoDebugStyle {
    style.screen = space == PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN;
    style
}

/// The built-in views of `ponko_ctx_debug_show` (Rust `DebugView`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoDebugView {
    /// 2D and 3D physics colliders (green, sensors yellow).
    PONKO_DEBUG_VIEW_COLLIDERS = 0,
    /// The navmesh's polygon edges.
    PONKO_DEBUG_VIEW_NAVMESH = 1,
    /// Model bounding spheres and sprite rectangles.
    PONKO_DEBUG_VIEW_BOUNDS = 2,
    /// The reach of point and spot lights, 2D and 3D.
    PONKO_DEBUG_VIEW_LIGHTS = 3,
    /// Each attached entity's axes, and a line to its parent.
    PONKO_DEBUG_VIEW_HIERARCHY = 4,
}

/// What is drawn now (Rust `DebugStats`): lines and labels in the buffer,
/// not counting the built-in views, which are added while a frame is drawn.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoDebugStats {
    /// 2D lines.
    pub lines_2d: usize,
    /// 3D lines.
    pub lines_3d: usize,
    /// Text labels.
    pub labels: usize,
}

fn view_from_c(view: u32) -> Result<DebugView, Failure> {
    Ok(match view {
        0 => DebugView::Colliders,
        1 => DebugView::NavMesh,
        2 => DebugView::Bounds,
        3 => DebugView::Lights,
        4 => DebugView::Hierarchy,
        _ => return Err(invalid("view is not a PonkoDebugView")),
    })
}

fn v2(v: PonkoVec2) -> Vec2 {
    to_c(v)
}

fn v3(v: PonkoVec3) -> Vec3 {
    to_c(v)
}

fn rgba(c: PonkoColor) -> Color {
    to_c(c)
}

/// Reads the style (null is the default).
unsafe fn style_from_c(style: *const PonkoDebugStyle) -> PonkoDebugStyle {
    if style.is_null() {
        ponko_debug_style_default()
    } else {
        // SAFETY: the caller passes null or a live style.
        unsafe { *style }
    }
}

fn styled(shape: DebugShape<'_>, s: &PonkoDebugStyle) {
    let mut shape = shape.width(s.width).size(s.size).seconds(s.seconds);
    if s.on_top {
        shape = shape.on_top();
    }
    if s.screen {
        shape.screen();
    }
}

/// Runs `f` on the context behind the boundary.
unsafe fn draw(ctx: *mut PonkoCtx, f: impl FnOnce(&mut Ctx) -> Result<(), Failure>) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, f) })
}

macro_rules! shape {
    ($ctx:ident, $style:ident, |$d:ident| $body:expr) => {{
        // SAFETY: forwarded caller contract (style null or live).
        let s = unsafe { style_from_c($style) };
        // SAFETY: forwarded caller contract (ctx null or live).
        boundary(|| unsafe {
            debug_draw($ctx, |$d: &mut DebugDraw| {
                styled($body, &s);
                Ok(())
            })
        })
    }};
}

/// Turns debug drawing on or off (Rust `DebugDraw::set_enabled`). Off, it
/// forgets everything drawn and every call returns at once; the built-in
/// views stay chosen.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_debug_set_enabled(std::ptr::null_mut(), true) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_set_enabled(ctx: *mut PonkoCtx, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.debug.set_enabled(on);
            Ok(())
        })
    })
}

/// Whether debug drawing is on.
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_enabled(ctx: *mut PonkoCtx, out_on: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_on, c.debug.enabled())) })
}

/// Forgets everything drawn so far, timed shapes included.
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_clear(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.debug.clear();
            Ok(())
        })
    })
}

/// Turns a built-in view (`PonkoDebugView`) on or off; it draws every frame
/// while debug drawing is on.
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_show(ctx: *mut PonkoCtx, view: u32, on: bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.debug.show(view_from_c(view)?, on);
            Ok(())
        })
    })
}

/// Whether a built-in view (`PonkoDebugView`) is on.
/// # Safety
/// `ctx` null or live; `out_on` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_showing(ctx: *mut PonkoCtx, view: u32, out_on: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_on, c.debug.showing(view_from_c(view)?))) })
}

/// Sets how far from the 3D camera (world units) the colliders view draws
/// the triangles of collision meshes and terrain (Rust
/// `DebugDraw::set_collider_distance`; default 50). Below 0 or not finite
/// is taken as 0.
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_set_collider_distance(ctx: *mut PonkoCtx, distance: f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.debug.set_collider_distance(distance);
            Ok(())
        })
    })
}

/// How far from the 3D camera the colliders view draws mesh triangles.
/// # Safety
/// `ctx` null or live; `out_distance` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_collider_distance(ctx: *mut PonkoCtx, out_distance: *mut f32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe { ctx_access(ctx, |c| out(out_distance, c.debug.collider_distance())) })
}

/// How many lines and labels are drawn now (Rust `DebugDraw::stats`).
/// # Safety
/// `ctx` null or live; `out_stats` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_stats(ctx: *mut PonkoCtx, out_stats: *mut PonkoDebugStats) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let s = c.debug.stats();
            out(
                out_stats,
                PonkoDebugStats {
                    lines_2d: s.lines_2d,
                    lines_3d: s.lines_3d,
                    labels: s.labels,
                },
            )
        })
    })
}

/// A 2D line from `a` to `b` (world units, or pixels with `style->screen`).
/// ```
/// use ponko_ffi::*;
/// let (a, b) = (PonkoVec2 { x: 0.0, y: 0.0 }, PonkoVec2 { x: 1.0, y: 0.0 });
/// let red = PonkoColor { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
/// let status = unsafe { ponko_ctx_debug_line2d(std::ptr::null_mut(), a, b, red, std::ptr::null()) };
/// assert_eq!(status, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_line2d(
    ctx: *mut PonkoCtx,
    a: PonkoVec2,
    b: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.line(v2(a), v2(b), rgba(color)))
}

/// A 2D line from `origin` along `direction` (its length is the line's).
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_ray2d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.ray(v2(origin), v2(direction), rgba(color)))
}

/// A 2D arrow from `from` to `to`, its head at `to`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_arrow2d(
    ctx: *mut PonkoCtx,
    from: PonkoVec2,
    to: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.arrow(v2(from), v2(to), rgba(color)))
}

/// A 2D rectangle `size` wide and high around `center`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_box2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    size: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.aabb(v2(center), v2(size), rgba(color)))
}

/// A 2D circle.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_circle2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.circle(v2(center), radius, rgba(color)))
}

/// A 2D capsule: the points within `radius` of the segment from `a` to `b`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_capsule2d(
    ctx: *mut PonkoCtx,
    a: PonkoVec2,
    b: PonkoVec2,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.capsule(v2(a), v2(b), radius, rgba(color)))
}

/// 2D lines through `count` points, back to the first when `closed`.
/// # Safety
/// `ctx` null or live; `points` holds `count` values (null when 0);
/// `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_polyline2d(
    ctx: *mut PonkoCtx,
    points: *const PonkoVec2,
    count: usize,
    closed: bool,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    if count > 0 && points.is_null() {
        return boundary(|| Err(invalid("points is null")));
    }
    let points: &[Vec2] = if count == 0 {
        &[]
    } else {
        // SAFETY: PonkoVec2 and Vec2 are both two f32s; the caller passes
        // `count` live points.
        unsafe { std::slice::from_raw_parts(points.cast(), count) }
    };
    shape!(ctx, style, |d| d.polyline(points, closed, rgba(color)))
}

/// A 2D grid of `cells` by `cells` squares of side `cell` around `center`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_grid2d(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    cell: f32,
    cells: u32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.grid(v2(center), cell, cells, rgba(color)))
}

/// A text label whose top-left corner is at the 2D point `at`.
/// # Safety
/// `ctx` null or live; `label` NUL-terminated UTF-8; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_text2d(
    ctx: *mut PonkoCtx,
    at: PonkoVec2,
    label: *const c_char,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let label = match unsafe { text(label) } {
        Ok(label) => label,
        Err(e) => return boundary(|| Err(e)),
    };
    shape!(ctx, style, |d| d.text(v2(at), label, rgba(color)))
}

/// Like `ponko_ctx_debug_line2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_line2d_ex(
    ctx: *mut PonkoCtx,
    a: PonkoVec2,
    b: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_line2d(ctx, a, b, color, s)) }
}

/// Like `ponko_ctx_debug_ray2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_ray2d_ex(
    ctx: *mut PonkoCtx,
    origin: PonkoVec2,
    direction: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_ray2d(ctx, origin, direction, color, s)) }
}

/// Like `ponko_ctx_debug_arrow2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_arrow2d_ex(
    ctx: *mut PonkoCtx,
    from: PonkoVec2,
    to: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_arrow2d(ctx, from, to, color, s)) }
}

/// Like `ponko_ctx_debug_box2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_box2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    size: PonkoVec2,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_box2d(ctx, center, size, color, s)) }
}

/// Like `ponko_ctx_debug_circle2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_circle2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_circle2d(ctx, center, radius, color, s)) }
}

/// Like `ponko_ctx_debug_capsule2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_capsule2d_ex(
    ctx: *mut PonkoCtx,
    a: PonkoVec2,
    b: PonkoVec2,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_capsule2d(ctx, a, b, radius, color, s)) }
}

/// Like `ponko_ctx_debug_polyline2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size. `points` holds `count` values (null when 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_polyline2d_ex(
    ctx: *mut PonkoCtx,
    points: *const PonkoVec2,
    count: usize,
    closed: bool,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        with_style_ex(style, |s| {
            ponko_ctx_debug_polyline2d(ctx, points, count, closed, color, s)
        })
    }
}

/// Like `ponko_ctx_debug_grid2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_grid2d_ex(
    ctx: *mut PonkoCtx,
    center: PonkoVec2,
    cell: f32,
    cells: u32,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_grid2d(ctx, center, cell, cells, color, s)) }
}

/// Like `ponko_ctx_debug_text2d`, with a versioned 2D style.
/// # Safety
/// `ctx` null or live; `style` null or readable for its initialized header's stated size. `label` is NUL-terminated UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_text2d_ex(
    ctx: *mut PonkoCtx,
    at: PonkoVec2,
    label: *const c_char,
    color: PonkoColor,
    style: *const PonkoDebugStyleEx,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { with_style_ex(style, |s| ponko_ctx_debug_text2d(ctx, at, label, color, s)) }
}

/// A 3D line from `a` to `b`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_line3d(
    ctx: *mut PonkoCtx,
    a: PonkoVec3,
    b: PonkoVec3,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.line(v3(a), v3(b), rgba(color)))
}

/// A 3D line from `origin` along `direction` (its length is the line's).
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_ray3d(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    direction: PonkoVec3,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.ray(v3(origin), v3(direction), rgba(color)))
}

/// A 3D arrow from `from` to `to`, its head at `to`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_arrow3d(
    ctx: *mut PonkoCtx,
    from: PonkoVec3,
    to: PonkoVec3,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.arrow(v3(from), v3(to), rgba(color)))
}

/// A 3D box of `size` around `center`, turned by `rotation`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_box3d(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    size: PonkoVec3,
    rotation: PonkoQuat,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    let rotation: Quat = quat_from_c(rotation);
    shape!(ctx, style, |d| d.obb(v3(center), v3(size), rotation, rgba(color)))
}

/// A 3D sphere: three rings, one around each axis.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_sphere3d(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.sphere(v3(center), radius, rgba(color)))
}

/// A 3D capsule: the points within `radius` of the segment from `a` to `b`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_capsule3d(
    ctx: *mut PonkoCtx,
    a: PonkoVec3,
    b: PonkoVec3,
    radius: f32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.capsule(v3(a), v3(b), radius, rgba(color)))
}

/// 3D lines through `count` points, back to the first when `closed`.
/// # Safety
/// `ctx` null or live; `points` holds `count` values (null when 0);
/// `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_polyline3d(
    ctx: *mut PonkoCtx,
    points: *const PonkoVec3,
    count: usize,
    closed: bool,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    if count > 0 && points.is_null() {
        return boundary(|| Err(invalid("points is null")));
    }
    let points: &[Vec3] = if count == 0 {
        &[]
    } else {
        // SAFETY: PonkoVec3 and Vec3 are both three f32s; the caller passes
        // `count` live points.
        unsafe { std::slice::from_raw_parts(points.cast(), count) }
    };
    shape!(ctx, style, |d| d.polyline(points, closed, rgba(color)))
}

/// A grid on the ground (the XZ plane) of `cells` by `cells` squares of
/// side `cell` around `center`.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_grid3d(
    ctx: *mut PonkoCtx,
    center: PonkoVec3,
    cell: f32,
    cells: u32,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    shape!(ctx, style, |d| d.grid(v3(center), cell, cells, rgba(color)))
}

/// What a 3D camera sees: its near and far rectangles and the edges
/// between them. `camera` null draws the scene's camera (the window's
/// aspect is used either way).
/// # Safety
/// `ctx` null or live; `camera` and `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_frustum3d(
    ctx: *mut PonkoCtx,
    camera: *const PonkoCamera3D,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let s = unsafe { style_from_c(style) };
    // SAFETY: forwarded caller contract.
    unsafe {
        draw(ctx, |c: &mut Ctx| {
            let base = c.scene3d.camera;
            let cam = if camera.is_null() {
                base
            } else {
                camera_from_c(base, &*camera)?
            };
            styled(c.debug.frustum(&cam, rgba(color)), &s);
            Ok(())
        })
    }
}

/// A text label whose top-left corner is where the 3D point `at` is on the
/// screen (none while it is behind the camera).
/// # Safety
/// `ctx` null or live; `label` NUL-terminated UTF-8; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_text3d(
    ctx: *mut PonkoCtx,
    at: PonkoVec3,
    label: *const c_char,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let label = match unsafe { text(label) } {
        Ok(label) => label,
        Err(e) => return boundary(|| Err(e)),
    };
    shape!(ctx, style, |d| d.text(v3(at), label, rgba(color)))
}

/// A text label at the 3D point `at` hidden behind nearer 3D surfaces
/// (Rust `DebugShape::depth_tested`): drawn in the 3D frame facing the
/// camera, `style->size` pixels high, its top-left corner at `at`. Uses
/// `style`'s size and seconds; it is depth-tested whatever `style->on_top` says.
/// # Safety
/// `ctx` null or live; `label` NUL-terminated UTF-8; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_text3d_depth_tested(
    ctx: *mut PonkoCtx,
    at: PonkoVec3,
    label: *const c_char,
    color: PonkoColor,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    let label = match unsafe { text(label) } {
        Ok(label) => label,
        Err(e) => return boundary(|| Err(e)),
    };
    // SAFETY: forwarded caller contract (style null or live).
    let s = unsafe { style_from_c(style) };
    // SAFETY: forwarded caller contract (ctx null or live).
    boundary(|| unsafe {
        debug_draw(ctx, |d: &mut DebugDraw| {
            d.text(v3(at), label, rgba(color))
                .size(s.size)
                .seconds(s.seconds)
                .depth_tested();
            Ok(())
        })
    })
}

/// The axes of a transform: x red, y green, z blue, each `length` long.
/// # Safety
/// `ctx` null or live; `style` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_debug_axes3d(
    ctx: *mut PonkoCtx,
    at: PonkoVec3,
    rotation: PonkoQuat,
    length: f32,
    style: *const PonkoDebugStyle,
) -> PonkoStatus {
    let rotation: Quat = quat_from_c(rotation);
    shape!(ctx, style, |d| d.axes(v3(at), rotation, length))
}

#[cfg(test)]
mod style_ex_tests {
    use super::*;

    #[test]
    fn descriptor_space_reaches_the_legacy_draw_path() {
        let mut style = PonkoDebugStyleEx {
            seconds: 0.2,
            width: 2.0,
            size: 17.0,
            on_top: true,
            ..Default::default()
        };
        for space in [
            PonkoDrawSpace::PONKO_DRAW_SPACE_WORLD,
            PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN,
        ] {
            style.space = space as u32;
            let status = unsafe {
                with_style_ex(&style, |draw| {
                    assert_eq!(draw.screen, space == PonkoDrawSpace::PONKO_DRAW_SPACE_SCREEN);
                    assert_eq!(
                        (draw.seconds, draw.width, draw.size, draw.on_top),
                        (0.2, 2.0, 17.0, true)
                    );
                    PonkoStatus::PONKO_OK
                })
            };
            assert_eq!(status, PonkoStatus::PONKO_OK);
        }
    }
}
