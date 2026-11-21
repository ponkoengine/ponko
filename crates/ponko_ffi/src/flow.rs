//! Flow fields from C: `PonkoNavGrid` (walking costs) and `PonkoFlowField`
//! (the way to the nearest goal from every cell).
use ponko::flow::FlowField;
use ponko::flow::NavGrid;
use ponko::prelude::Vec2;

use crate::ctx::out;
use crate::types::*;
use crate::{Failure, PonkoStatus, boundary, error, invalid};

/// A grid of walking costs: 1 is normal ground, up to 255 is slower, 0
/// (`PONKO_NAV_WALL`) can't be entered.
pub struct PonkoNavGrid(NavGrid);
/// The way to the nearest goal from every cell of a nav grid. Queries only
/// read, so they're safe from `ponko_query_par` threads.
pub struct PonkoFlowField(FlowField);

/// A cell that can't be entered.
pub const PONKO_NAV_WALL: u8 = 0;
const _: () = assert!(PONKO_NAV_WALL == NavGrid::WALL);

/// A read-only view of a flow field for inline lookups (ponko.hpp's
/// `flow_view`). Valid until the field is computed again or freed.
/// ```
/// assert_eq!(std::mem::size_of::<ponko_ffi::PonkoFlowView>() % std::mem::size_of::<usize>(), 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoFlowView {
    /// Per padded cell, an index into `directions`; cell (x, y) is at
    /// `(y + 1) * stride + x + 1`. Null before the first compute.
    pub cells: *const u8,
    /// The 9 directions: 8 unit vectors and (index 8) zero.
    pub directions: *const PonkoVec2,
    /// Cells across and down.
    pub width: u32,
    /// Cells down.
    pub height: u32,
    /// Row length of `cells`.
    pub stride: usize,
    /// World position of cell (0, 0)'s top-left corner.
    pub origin: PonkoVec2,
    /// Side of a cell in world units.
    pub cell_size: f32,
}

/// Makes a `width` x `height` grid of normal ground with cells `cell_size`
/// world units wide. Returns null (see `ponko_last_error`) if a side is 0,
/// it has more than 2^28 cells, or `cell_size` isn't positive.
///
/// Ownership: the returned grid is the caller's (null on failure); free it with
/// ponko_nav_free.
///
/// ```
/// let nav = ponko_ffi::ponko_nav_new(16, 16, 8.0);
/// assert!(!nav.is_null());
/// unsafe { ponko_ffi::ponko_nav_free(nav); }
/// assert!(ponko_ffi::ponko_nav_new(0, 16, 8.0).is_null());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_nav_new(width: u32, height: u32, cell_size: f32) -> *mut PonkoNavGrid {
    let mut nav = std::ptr::null_mut();
    let status = boundary(|| {
        if width == 0 || height == 0 || (width as u64) * (height as u64) > 1 << 28 {
            return Err(invalid("a nav grid needs 1 to 2^28 cells"));
        }
        if !(cell_size > 0.0 && cell_size.is_finite()) {
            return Err(invalid("cell size must be positive"));
        }
        nav = Box::into_raw(Box::new(PonkoNavGrid(NavGrid::new(width, height, cell_size))));
        Ok(())
    });
    if status == PonkoStatus::PONKO_OK {
        nav
    } else {
        std::ptr::null_mut()
    }
}

/// Frees a nav grid. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_nav_free(std::ptr::null_mut()); }
/// ```
/// # Safety
/// `nav` must be null or live, and not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_free(nav: *mut PonkoNavGrid) {
    if !nav.is_null() {
        // SAFETY: caller passes a grid from ponko_nav_new, once.
        drop(unsafe { Box::from_raw(nav) });
    }
}

unsafe fn nav_mut<'a>(nav: *mut PonkoNavGrid) -> Result<&'a mut NavGrid, Failure> {
    // SAFETY: caller supplies a live grid with no concurrent use.
    unsafe { nav.as_mut() }
        .map(|n| &mut n.0)
        .ok_or_else(|| invalid("nav grid is null"))
}

/// Moves the grid: `origin` is where cell (0, 0)'s top-left corner is in the world.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_set_origin(std::ptr::null_mut(), Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `nav` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_set_origin(nav: *mut PonkoNavGrid, origin: PonkoVec2) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        unsafe { nav_mut(nav)? }.origin = to_c(origin);
        Ok(())
    })
}

/// The grid's costs, row by row (`width * height` bytes), writable in place.
/// Valid until the grid is freed.
///
/// Ownership: `out_costs` receives a view of bytes the grid owns (writable in
/// place), valid until the grid is freed; never free it.
///
/// ```
/// use ponko_ffi::*;
/// let nav = ponko_nav_new(4, 2, 1.0);
/// let (mut costs, mut len) = (std::ptr::null_mut(), 0);
/// unsafe {
///     assert_eq!(ponko_nav_costs(nav, &mut costs, &mut len), PonkoStatus::PONKO_OK);
///     assert_eq!(len, 8);
///     *costs.add(5) = PONKO_NAV_WALL;
///     ponko_nav_free(nav);
/// }
/// ```
/// # Safety
/// `nav` must be null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_costs(
    nav: *mut PonkoNavGrid,
    out_costs: *mut *mut u8,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_costs.is_null() || out_len.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract; outputs checked non-null.
        unsafe {
            let costs = nav_mut(nav)?.costs_mut();
            out_len.write(costs.len());
            out_costs.write(costs.as_mut_ptr());
        }
        Ok(())
    })
}

/// An empty flow field (every direction zero until computed).
///
/// Ownership: the returned flow field is the caller's (null on failure); free
/// it with ponko_flow_free.
///
/// ```
/// let f = ponko_ffi::ponko_flow_new();
/// unsafe { ponko_ffi::ponko_flow_free(f); }
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_flow_new() -> *mut PonkoFlowField {
    Box::into_raw(Box::new(PonkoFlowField(FlowField::new())))
}

/// Frees a flow field. Null is ignored.
/// ```
/// unsafe { ponko_ffi::ponko_flow_free(std::ptr::null_mut()); }
/// ```
/// # Safety
/// `flow` must be null or live, and not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_flow_free(flow: *mut PonkoFlowField) {
    if !flow.is_null() {
        // SAFETY: caller passes a field from ponko_flow_new, once.
        drop(unsafe { Box::from_raw(flow) });
    }
}

/// Recomputes `flow` for `count` goals (world points; ones outside the grid
/// or on walls are skipped). Reuses its memory. Views taken before expire.
/// ```
/// use ponko_ffi::*;
/// let (nav, flow) = (ponko_nav_new(8, 8, 1.0), ponko_flow_new());
/// let goal = PonkoVec2 { x: 7.5, y: 0.5 };
/// unsafe {
///     assert_eq!(ponko_flow_compute(flow, nav, &goal, 1), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_flow_direction(flow, PonkoVec2 { x: 0.5, y: 0.5 }), PonkoVec2 { x: 1.0, y: 0.0 });
///     ponko_flow_free(flow);
///     ponko_nav_free(nav);
/// }
/// ```
/// # Safety
/// Live handles, `flow` not read meanwhile; `goals` holds `count` points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_flow_compute(
    flow: *mut PonkoFlowField,
    nav: *const PonkoNavGrid,
    goals: *const PonkoVec2,
    count: usize,
) -> PonkoStatus {
    boundary(|| {
        if goals.is_null() && count > 0 {
            return Err(invalid("goals is null"));
        }
        // SAFETY: caller supplies live handles and `count` goals.
        unsafe {
            let flow = flow.as_mut().ok_or_else(|| invalid("flow field is null"))?;
            let nav = nav.as_ref().ok_or_else(|| invalid("nav grid is null"))?;
            let goals: &[Vec2] = if count == 0 {
                &[]
            } else {
                // PonkoVec2 and Vec2 have the same layout (checked in types.rs).
                std::slice::from_raw_parts(goals.cast::<Vec2>(), count)
            };
            flow.0.compute(&nav.0, goals);
        }
        Ok(())
    })
}

/// Unit direction toward the nearest goal from `pos`, or zero (at a goal,
/// on a wall, outside, unreachable, or for a null field).
/// ```
/// let d = unsafe { ponko_ffi::ponko_flow_direction(std::ptr::null(), Default::default()) };
/// assert_eq!(d, Default::default());
/// ```
/// # Safety
/// `flow` must be null or live, and not being computed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_flow_direction(flow: *const PonkoFlowField, pos: PonkoVec2) -> PonkoVec2 {
    // SAFETY: forwarded caller contract.
    match unsafe { flow.as_ref() } {
        Some(f) => to_c(f.0.direction_at(to_c(pos))),
        None => PonkoVec2::default(),
    }
}

/// Length of the shortest way from `pos` to a goal in world units (slow
/// ground counts as longer). `PONKO_NOT_FOUND` if no goal can be reached.
/// ```
/// let mut d = 0.0;
/// assert_eq!(unsafe { ponko_ffi::ponko_flow_distance(std::ptr::null(), Default::default(), &mut d) } as u32, 1);
/// ```
/// # Safety
/// `flow` must be null or live, and not being computed; `out_distance` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_flow_distance(
    flow: *const PonkoFlowField,
    pos: PonkoVec2,
    out_distance: *mut f32,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let f = unsafe { flow.as_ref() }.ok_or_else(|| invalid("flow field is null"))?;
        match f.0.distance_at(to_c(pos)) {
            // SAFETY: forwarded caller contract.
            Some(d) => unsafe { out(out_distance, d) },
            None => Err(error(PonkoStatus::PONKO_NOT_FOUND, "no goal can be reached from here")),
        }
    })
}

/// A view for inline lookups; see `PonkoFlowView`.
/// ```
/// use ponko_ffi::*;
/// let flow = ponko_flow_new();
/// let mut v = std::mem::MaybeUninit::uninit();
/// unsafe {
///     assert_eq!(ponko_flow_view(flow, v.as_mut_ptr()), PonkoStatus::PONKO_OK);
///     assert!(v.assume_init().cells.is_null(), "not computed yet");
///     ponko_flow_free(flow);
/// }
/// ```
/// # Safety
/// `flow` must be null or live; `out_view` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_flow_view(flow: *const PonkoFlowField, out_view: *mut PonkoFlowView) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let f = unsafe { flow.as_ref() }.ok_or_else(|| invalid("flow field is null"))?;
        let v = f.0.raw_view();
        let view = PonkoFlowView {
            cells: if v.dir.is_empty() {
                std::ptr::null()
            } else {
                v.dir.as_ptr()
            },
            // Vec2 and PonkoVec2 have the same layout.
            directions: v.directions.as_ptr().cast(),
            width: v.size.x,
            height: v.size.y,
            stride: v.stride,
            origin: to_c(v.origin),
            cell_size: v.cell_size,
        };
        // SAFETY: forwarded caller contract.
        unsafe { out(out_view, view) }
    })
}
