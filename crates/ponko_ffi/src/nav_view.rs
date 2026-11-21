//! Navmesh reads inside C loops: `ponko_ctx_nav_view` hands out a read-only
//! view of the 3D navmesh before a `ponko_query`, and the
//! `ponko_nav_view_*` calls read through it without the ctx, so they work
//! inside `ponko_query` and `ponko_query_par` callbacks, where ctx calls
//! are refused. `ponko_nav_view_sample_positions` snaps a whole array of
//! points in one call.
use ponko::d3::NavMesh as NavMesh3D;

use crate::ctx::{ctx_access, out};
use crate::three::PonkoVec3;
use crate::types::to_c;
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid};

/// The navmesh a `PonkoNavView` reads (Rust `d3::NavMesh`), opaque: only
/// the `ponko_nav_view_*` calls look inside.
pub struct PonkoNavMesh {
    _opaque: [u8; 0],
}

/// A read-only view of the game's 3D navmesh, from `ponko_ctx_nav_view`.
/// It stays valid until the system that took it returns, and only until a
/// call bakes the navmesh again (`ponko_ctx_bake_navmesh`); carving and
/// obstacles change what it finds, not whether it is valid. The calls that
/// read it touch nothing else, so they are safe inside `ponko_query` and
/// from `ponko_query_par` threads.
/// ```
/// let v = ponko_ffi::PonkoNavView { navmesh: std::ptr::null() };
/// assert!(v.navmesh.is_null());
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoNavView {
    /// The navmesh, owned by the game's world.
    pub navmesh: *const PonkoNavMesh,
}

// The view is read from many threads at once (`ponko_query_par`).
const _: () = {
    const fn sync<T: Sync>() {}
    sync::<NavMesh3D>();
};

unsafe fn mesh<'a>(view: *const PonkoNavView) -> Result<&'a NavMesh3D, Failure> {
    // SAFETY: caller contract: null or a live view.
    let view = unsafe { view.as_ref() }.ok_or_else(|| invalid("view is null"))?;
    // SAFETY: caller contract: the view is still valid (see PonkoNavView),
    // and its pointer came from the navmesh resource (`ponko_ctx_nav_view`).
    unsafe { view.navmesh.cast::<NavMesh3D>().as_ref() }.ok_or_else(|| invalid("view has no navmesh"))
}

/// A view of the game's 3D navmesh for reads inside loops (see
/// `PonkoNavView`). Take it before the loop: it is a ctx call, refused
/// inside one. No navmesh baked: `PONKO_NOT_FOUND`.
///
/// Ownership: the navmesh stays the game's; the view borrows it until the
/// system returns or the navmesh is baked again. `out_view` is the
/// caller's, written during the call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_nav_view(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_view` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_nav_view(ctx: *mut PonkoCtx, out_view: *mut PonkoNavView) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            if out_view.is_null() {
                return Err(invalid("output is null"));
            }
            let nav = c
                .world
                .get_resource::<NavMesh3D>()
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no navmesh: bake one first"))?;
            out(
                out_view,
                PonkoNavView {
                    navmesh: std::ptr::from_ref(nav).cast(),
                },
            )
        })
    })
}

/// The nearest walkable point within `max_distance` of `point`, through a
/// view (as `ponko_ctx_navmesh_sample_position`); `*out_found` is false
/// when there is none. Safe inside loops and from `ponko_query_par`.
///
/// Ownership: `view` is read and the outputs written during the call only.
/// ```
/// let p = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_view_sample_position(std::ptr::null(), p, 1.0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `view` null or valid (see `PonkoNavView`); outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_view_sample_position(
    view: *const PonkoNavView,
    point: PonkoVec3,
    max_distance: f32,
    out_point: *mut PonkoVec3,
    out_found: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        if out_point.is_null() || out_found.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract; outputs checked non-null.
        unsafe {
            match mesh(view)?.sample_position(to_c(point), max_distance) {
                Some(p) => {
                    out_point.write(to_c(p));
                    out(out_found, true)
                }
                None => out(out_found, false),
            }
        }
    })
}

/// Snaps `count` points to the navmesh in one call: `out_points[i]` is the
/// nearest walkable point within `max_distance` of `points[i]` and
/// `out_found[i]` whether there was one (`out_points[i]` is then
/// `points[i]` unchanged). `points` and `out_points` may be the same array.
/// Safe inside loops and from `ponko_query_par`.
///
/// Ownership: the arrays are the caller's, read and written during the
/// call only.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_view_sample_positions(std::ptr::null(), std::ptr::null(), 0, 1.0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `view` null or valid; `points`, `out_points` and `out_found` hold
/// `count` entries each (or are null with `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_view_sample_positions(
    view: *const PonkoNavView,
    points: *const PonkoVec3,
    count: usize,
    max_distance: f32,
    out_points: *mut PonkoVec3,
    out_found: *mut bool,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: forwarded caller contract.
        let nav = unsafe { mesh(view)? };
        if count == 0 {
            return Ok(());
        }
        if points.is_null() || out_points.is_null() || out_found.is_null() {
            return Err(invalid("an array is null"));
        }
        for i in 0..count {
            // SAFETY: caller contract: `count` entries each; reading entry
            // `i` before writing it lets the arrays be the same.
            unsafe {
                let p = points.add(i).read();
                let found = nav.sample_position(to_c(p), max_distance);
                out_points.add(i).write(found.map_or(p, to_c));
                out_found.add(i).write(found.is_some());
            }
        }
        Ok(())
    })
}

/// A path from `from` to `to` through a view (as
/// `ponko_ctx_navmesh_find_path`): writes up to `capacity` corners and the
/// full count to `*out_len`, 0 when there is no path. Safe inside loops and
/// from `ponko_query_par`.
///
/// Ownership: `view` is read and the outputs written during the call only.
/// ```
/// let p = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_nav_view_find_path(std::ptr::null(), p, p, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `view` null or valid; `out_corners` writable for `capacity` points (or
/// null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_nav_view_find_path(
    view: *const PonkoNavView,
    from: PonkoVec3,
    to: PonkoVec3,
    out_corners: *mut PonkoVec3,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_corners.is_null() && capacity > 0 {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let path = mesh(view)?.find_path(to_c(from), to_c(to)).unwrap_or_default();
            for (i, p) in path.iter().take(capacity).enumerate() {
                out_corners.add(i).write(to_c(*p));
            }
            out(out_len, path.len())
        }
    })
}
