//! Ribbons, lines and decals from C (Rust `Trail3D`, `Line3D`,
//! `MeshData::decal` and `Graphics::load_mesh_data`).
//!
//! Fill a `PonkoTrail3D` (start from `ponko_trail3d_new()`) or a
//! `PonkoLine3D` (start from `ponko_line3d_new()`), then give an entity with a
//! 3D transform one with `ponko_ctx_set_trail3d` / `ponko_ctx_set_line3d`.
//! Their curves and gradients are the particle keys (`PonkoParticleKey`,
//! `PonkoParticleColorKey`) and their blend a `PonkoParticleBlend3D`.

use std::ffi::c_char;

use ponko::d3::Line as Line3D;
use ponko::d3::Trail as Trail3D;
use ponko::prelude::{Color, Mat4, Vec3};
use ponko::render::MeshData;

use crate::ctx::{ctx_access, out, text};
use crate::particles::{
    PonkoParticleBlend3D, PonkoParticleColorKey, PonkoParticleKey, blend_from_c, curve_from_c, entity, finite,
    gradient_from_c, keys,
};
use crate::three::{PonkoMeshData, PonkoVec3};
use crate::types::{PonkoColor, PonkoTexture, texture_from_c, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, invalid, invalid_owned};

/// A ribbon left behind its entity as it moves (Rust `Trail3D`). Make one
/// with `ponko_trail3d_new`, change what you like, then give it to an
/// entity with `ponko_ctx_set_trail3d`. The key pointers are only read
/// during the call that takes the trail; null with a count of 0 keeps the
/// default curve.
/// ```
/// let t = ponko_ffi::ponko_trail3d_new(0.5, 0.1);
/// assert_eq!(t.max_points, 64);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoTrail3D {
    /// Seconds a point lasts before the ribbon lets go of it. Must be positive.
    pub lifetime: f32,
    /// Width in meters at the entity, narrowing to 0 at the end; used when
    /// `width_keys` is null.
    pub width: f32,
    /// Width in meters over age (0 the newest end, 1 the oldest); null to
    /// narrow from `width` to 0.
    pub width_keys: *const PonkoParticleKey,
    /// Number of `width_keys`.
    pub width_key_count: usize,
    /// Color over age (0 the newest end, 1 the oldest); null for white
    /// fading out.
    pub color_keys: *const PonkoParticleColorKey,
    /// Number of `color_keys`.
    pub color_key_count: usize,
    /// Use `texture` (u is each point's age over `lifetime`, v across);
    /// otherwise the ribbon is plain white. Default false.
    pub has_texture: bool,
    /// The ribbon image.
    pub texture: PonkoTexture,
    /// A `PonkoParticleBlend3D`. Default alpha.
    pub blend: u32,
    /// A new point is kept only after the entity moved this far (meters)
    /// from the last one. Default 0.05.
    pub min_distance: f32,
    /// At most this many points (at least 2); the oldest go first. Default 64.
    pub max_points: u32,
    /// While false, no new points are added and the ribbon fades out. Default true.
    pub emitting: bool,
}

/// A ribbon `width` meters wide (narrowing to 0 at its end) whose points last
/// `lifetime` seconds, white and alpha blended (Rust `Trail3D::new`).
/// ```
/// let t = ponko_ffi::ponko_trail3d_new(0.25, 0.05);
/// assert_eq!((t.lifetime, t.width, t.emitting), (0.25, 0.05, true));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_trail3d_new(lifetime: f32, width: f32) -> PonkoTrail3D {
    let t = Trail3D::new(lifetime, width);
    PonkoTrail3D {
        lifetime,
        width,
        width_keys: std::ptr::null(),
        width_key_count: 0,
        color_keys: std::ptr::null(),
        color_key_count: 0,
        has_texture: false,
        texture: PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        },
        blend: PonkoParticleBlend3D::PONKO_PARTICLES_ALPHA as u32,
        min_distance: t.min_distance,
        max_points: t.max_points as u32,
        emitting: t.emitting,
    }
}

/// Checks a C trail, so bad values are errors rather than panics.
///
/// # Safety
/// The key pointers are null or point to their counts of keys.
unsafe fn trail_from_c(t: &PonkoTrail3D) -> Result<Trail3D, Failure> {
    if !(t.lifetime.is_finite() && t.lifetime > 0.0) {
        return Err(invalid("trail lifetime must be positive"));
    }
    if !finite(&[t.width, t.min_distance]) || t.width < 0.0 || t.min_distance < 0.0 {
        return Err(invalid("trail width and min_distance must be finite and not negative"));
    }
    if t.max_points < 2 {
        return Err(invalid("trail max_points must be at least 2"));
    }
    // SAFETY: caller contract.
    let width_keys = unsafe { keys(t.width_keys, t.width_key_count, "width_keys")? };
    // SAFETY: as above.
    let color_keys = unsafe { keys(t.color_keys, t.color_key_count, "color_keys")? };
    if width_keys.iter().any(|k| k.value < 0.0) {
        return Err(invalid("trail widths must not be negative"));
    }
    let mut trail = Trail3D::new(t.lifetime, t.width);
    if let Some(width) = curve_from_c(width_keys)? {
        trail.width = width;
    }
    if let Some(color) = gradient_from_c(color_keys)? {
        trail.color = color;
    }
    trail.texture = t.has_texture.then(|| texture_from_c(t.texture));
    trail.blend = blend_from_c(t.blend)?;
    trail.min_distance = t.min_distance;
    trail.max_points = t.max_points as usize;
    trail.emitting = t.emitting;
    Ok(trail)
}

fn trail(c: &mut ponko::prelude::Ctx, bits: u64) -> Result<&mut Trail3D, Failure> {
    let e = entity(c, bits)?;
    c.world
        .get_mut::<Trail3D>(e)
        .ok_or_else(|| invalid("entity has no trail"))
}

/// Gives `entity` (which needs a 3D transform to draw) a new ribbon trail,
/// replacing any it had along with its points.
/// ```
/// let t = ponko_ffi::ponko_trail3d_new(0.5, 0.1);
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_trail3d(std::ptr::null_mut(), 0, &t) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `trail` null or valid, its key pointers as documented.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_trail3d(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    trail: *const PonkoTrail3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let t = trail.as_ref().ok_or_else(|| invalid("trail is null"))?;
            let trail = trail_from_c(t)?;
            c.world.insert(e, trail);
            Ok(())
        })
    })
}

/// How many points `entity`'s trail keeps now (Rust `Trail3D::point_count`).
/// Trails only record in visual frames, which headless runs skip.
/// ```
/// let mut n = 0;
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_trail3d_point_count(std::ptr::null_mut(), 0, &mut n) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_trail3d_point_count(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_count: *mut u32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let n = trail(c, entity_bits)?.point_count();
            out(out_count, n as u32)
        })
    })
}

/// Forgets every point of `entity`'s trail (Rust `Trail3D::clear`): after a
/// teleport, so no ribbon spans the jump.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_clear_trail3d(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clear_trail3d(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            trail(c, entity_bits)?.clear();
            Ok(())
        })
    })
}

/// Starts or stops `entity`'s trail adding points (Rust `Trail3D::emitting`);
/// while it's off, the ribbon fades out.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_trail3d_emitting(std::ptr::null_mut(), 0, false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_trail3d_emitting(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    emitting: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            trail(c, entity_bits)?.emitting = emitting;
            Ok(())
        })
    })
}

/// Removes `entity`'s trail and its points.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_trail3d(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_trail3d(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.remove::<Trail3D>(e);
            Ok(())
        })
    })
}

/// A line through points in its entity's space, drawn as a camera-facing
/// strip (Rust `Line3D`): beams, cables, ropes. Make one with
/// `ponko_line3d_new`, set `points` and `point_count`, then give it to an
/// entity with `ponko_ctx_set_line3d`, which copies the points; call it
/// again any time to change them.
/// ```
/// let l = ponko_ffi::ponko_line3d_new(0.1, ponko_ffi::PonkoColor { r: 1.0, g: 0.0, b: 0.0, a: 1.0 });
/// assert_eq!(l.point_count, 0);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoLine3D {
    /// `point_count` points; at least two to draw anything.
    pub points: *const PonkoVec3,
    /// Number of `points`.
    pub point_count: usize,
    /// Meters, scaled by the entity's largest scale.
    pub width: f32,
    /// Its color (and opacity) along its whole length.
    pub color: PonkoColor,
    /// Use `texture` (u along the line, v across); otherwise plain white.
    /// Default false.
    pub has_texture: bool,
    /// The line image.
    pub texture: PonkoTexture,
    /// A `PonkoParticleBlend3D`. Default alpha.
    pub blend: u32,
    /// Meters of line per repeat of `texture`; 0 stretches it once over the
    /// whole line. Default 0.
    pub texture_length: f32,
}

/// A `color` line `width` meters wide, alpha blended, with no points yet
/// (Rust `Line3D::new`).
/// ```
/// let l = ponko_ffi::ponko_line3d_new(0.1, ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
/// assert_eq!((l.width, l.texture_length), (0.1, 0.0));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_line3d_new(width: f32, color: PonkoColor) -> PonkoLine3D {
    PonkoLine3D {
        points: std::ptr::null(),
        point_count: 0,
        width,
        color,
        has_texture: false,
        texture: PonkoTexture {
            id: 0,
            width: 0,
            height: 0,
        },
        blend: PonkoParticleBlend3D::PONKO_PARTICLES_ALPHA as u32,
        texture_length: 0.0,
    }
}

/// Gives `entity` (which needs a 3D transform to draw) a line, replacing any
/// it had. The points are copied.
/// ```
/// let l = ponko_ffi::ponko_line3d_new(0.1, ponko_ffi::PonkoColor { r: 1.0, g: 1.0, b: 1.0, a: 1.0 });
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_line3d(std::ptr::null_mut(), 0, &l) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `line` null or valid, its `points` null or holding `point_count` points.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_line3d(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    line: *const PonkoLine3D,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let l = line.as_ref().ok_or_else(|| invalid("line is null"))?;
            if !finite(&[l.width, l.texture_length, l.color.r, l.color.g, l.color.b, l.color.a])
                || l.width < 0.0
                || l.texture_length < 0.0
            {
                return Err(invalid(
                    "line width and texture_length must be finite and not negative, its color finite",
                ));
            }
            let points = keys(l.points, l.point_count, "points")?;
            if points.iter().any(|p| !finite(&[p.x, p.y, p.z])) {
                return Err(invalid("line points must be finite"));
            }
            let mut line = Line3D::new(
                points.iter().map(|&p| to_c::<_, Vec3>(p)).collect(),
                l.width,
                to_c::<_, Color>(l.color),
            );
            line.texture = l.has_texture.then(|| texture_from_c(l.texture));
            line.blend = blend_from_c(l.blend)?;
            line.texture_length = l.texture_length;
            c.world.insert(e, line);
            Ok(())
        })
    })
}

/// Removes `entity`'s line.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_line3d(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_line3d(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.remove::<Line3D>(e);
            Ok(())
        })
    })
}

// ---- decals and mesh files ----------------------------------------------------

/// A decal (Rust `MeshData::decal`): the part of `data` inside a box, as new
/// mesh data of its own with an image projected straight onto it (bullet
/// holes, scorch marks, splats, signs). `projector` is 16 floats, column
/// after column, placing the box in the mesh's space: it maps the unit cube
/// (-0.5 to 0.5 on each axis) onto the box, so its scale is the decal's size
/// and depth. The image is projected along the box's -Z, its top along +Y;
/// only faces turned toward +Z get it. Each corner is lifted `lift` meters
/// along its normal so the decal doesn't flicker (a few millimeters is
/// typical). The result is empty (`index_count` 0 in
/// `ponko_mesh_data_view`) when the box misses the mesh; don't give an
/// empty one to `ponko_ctx_create_mesh`. Null (see `ponko_last_error`) if
/// `data` or `projector` is null or the projector isn't finite. Free the
/// result with `ponko_mesh_data_free`.
///
/// Ownership: the returned mesh data is the caller's (null on failure); free it
/// with ponko_mesh_data_free.
///
/// # Safety
/// `data` null or live; `projector` null or holds 16 floats.
/// ```
/// use ponko_ffi::*;
/// let floor = ponko_mesh_data_plane(4.0);
/// // A 0.5 m mark projected along the floor's -Y (box +Z turned up), 0.2 m deep.
/// let down = [0.5, 0.0, 0.0, 0.0,  0.0, 0.0, -0.2, 0.0,  0.0, 0.5, 0.0, 0.0,  1.0, 0.0, 0.0, 1.0];
/// let mark = unsafe { ponko_mesh_data_decal(floor, down.as_ptr(), 0.002) };
/// assert!(!mark.is_null());
/// unsafe { ponko_mesh_data_free(mark) };
/// assert!(unsafe { ponko_mesh_data_decal(floor, std::ptr::null(), 0.002) }.is_null());
/// unsafe { ponko_mesh_data_free(floor) };
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mesh_data_decal(
    data: *const PonkoMeshData,
    projector: *const f32,
    lift: f32,
) -> *mut PonkoMeshData {
    let mut result = std::ptr::null_mut();
    boundary(|| {
        // SAFETY: caller passes null or live mesh data.
        let Some(data) = (unsafe { data.as_ref() }) else {
            return Err(invalid("mesh data is null"));
        };
        if projector.is_null() {
            return Err(invalid("projector is null"));
        }
        // SAFETY: caller passes 16 floats.
        let projector = Mat4::from_cols_slice(unsafe { std::slice::from_raw_parts(projector, 16) });
        if !projector.is_finite() || !lift.is_finite() {
            return Err(invalid("projector and lift must be finite"));
        }
        result = Box::into_raw(Box::new(PonkoMeshData(data.0.decal(projector, lift))));
        Ok(())
    });
    result
}

/// Reads a glTF file's mesh onto the CPU (Rust `Graphics::load_mesh_data`), all
/// its parts in one and placed as in the file, without making anything on
/// the GPU: to cut decals from or read its corners. Loads the file each
/// call; keep the result. Free it with `ponko_mesh_data_free`. Fails if
/// the file is missing or isn't a glTF file with triangles.
///
/// Ownership: on success `out_data` receives an owned mesh data, the caller's;
/// free it with ponko_mesh_data_free.
///
/// # Safety
/// `ctx` null or live; `path` a NUL-terminated string; `out_data` writable.
/// ```
/// let mut d = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_mesh_data(std::ptr::null_mut(), c"a.glb".as_ptr(), &mut d) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_mesh_data(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    out_data: *mut *mut PonkoMeshData,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let path = text(path)?;
            if out_data.is_null() {
                return Err(invalid("output is null"));
            }
            let data: MeshData = c.gfx.try_load_mesh_data(path).map_err(invalid_owned)?;
            out(out_data, Box::into_raw(Box::new(PonkoMeshData(data))))
        })
    })
}
