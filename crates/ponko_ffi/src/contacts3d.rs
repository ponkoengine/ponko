//! Contact modification from C (the `physics3d` feature): one-way surfaces,
//! conveyor belts and a custom contact hook (Rust `ContactModifier3D`,
//! `Physics3D::set_contact_hook`, `ContactPair3D`). Without the feature the
//! ctx and world calls fail with an error naming it; the constructors still
//! work.
#![cfg_attr(not(feature = "physics3d"), allow(unused_variables))]

use std::ffi::c_void;

use crate::three::PonkoVec3;
use crate::{PonkoCtx, PonkoStatus, PonkoWorld};

/// The built-in `ContactModifier3D` component (Rust `ContactModifier3D`):
/// changes how a body's contacts behave. Put it on an entity with a
/// collider; the one-way side and the conveyor velocity are in that body's
/// own frame, so they turn with it. Bodies with this component pay a little:
/// their contacts are rebuilt every step. Start from
/// `ponko_contact_modifier3d_one_way`, `_conveyor`, `_hook` or `_default`.
/// Not a query column.
/// ```
/// let m = ponko_ffi::ponko_contact_modifier3d_default();
/// assert!(!m.has_one_way && !m.hook);
/// assert_eq!(m.one_way_angle, 1.2);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PonkoContactModifier3D {
    /// Whether `one_way` is used.
    pub has_one_way: bool,
    /// When `has_one_way`: contacts count only where the other body touches
    /// the side this (local) direction points to; from anywhere else it
    /// passes through. A body already partly inside keeps passing until it
    /// is out.
    pub one_way: PonkoVec3,
    /// How far from `one_way` a contact's direction may lean and still
    /// count, in radians (0 or more). Default 1.2 (about 70°).
    pub one_way_angle: f32,
    /// How fast the surface moves the things on it, in its own frame (m/s);
    /// the part along the surface counts. Zero for none.
    pub surface_velocity: PonkoVec3,
    /// Whether this body's contacts go to the hook set with
    /// `ponko_ctx_set_contact_hook3d`.
    pub hook: bool,
}

/// A modifier that does nothing: no one-way side (angle 1.2), no surface
/// velocity, no hook.
/// ```
/// let m = ponko_ffi::ponko_contact_modifier3d_default();
/// assert_eq!(m.surface_velocity.x, 0.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_contact_modifier3d_default() -> PonkoContactModifier3D {
    PonkoContactModifier3D {
        has_one_way: false,
        one_way: PonkoVec3::default(),
        one_way_angle: 1.2,
        surface_velocity: PonkoVec3::default(),
        hook: false,
    }
}

/// Solid only from the side `up` (a local direction) points to: things pass
/// up through a platform like this and land on top.
/// ```
/// let up = ponko_ffi::PonkoVec3 { x: 0.0, y: 1.0, z: 0.0 };
/// let m = ponko_ffi::ponko_contact_modifier3d_one_way(up);
/// assert!(m.has_one_way && m.one_way.y == 1.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_contact_modifier3d_one_way(up: PonkoVec3) -> PonkoContactModifier3D {
    PonkoContactModifier3D {
        has_one_way: true,
        one_way: up,
        ..ponko_contact_modifier3d_default()
    }
}

/// A surface moving what touches it at `velocity` (in its own frame, m/s),
/// like a conveyor belt.
/// ```
/// let v = ponko_ffi::PonkoVec3 { x: 2.0, y: 0.0, z: 0.0 };
/// assert_eq!(ponko_ffi::ponko_contact_modifier3d_conveyor(v).surface_velocity.x, 2.0);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_contact_modifier3d_conveyor(velocity: PonkoVec3) -> PonkoContactModifier3D {
    PonkoContactModifier3D {
        surface_velocity: velocity,
        ..ponko_contact_modifier3d_default()
    }
}

/// A modifier whose contacts go to the hook set with
/// `ponko_ctx_set_contact_hook3d`.
/// ```
/// assert!(ponko_ffi::ponko_contact_modifier3d_hook().hook);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_contact_modifier3d_hook() -> PonkoContactModifier3D {
    PonkoContactModifier3D {
        hook: true,
        ..ponko_contact_modifier3d_default()
    }
}

/// The id of the built-in `ContactModifier3D` component
/// (`PonkoContactModifier3D`), registering it if needed.
/// # Safety
/// Live handle and writable output.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut id = 99; assert_eq!(ponko_contact_modifier3d_component(w, &mut id), PonkoStatus::PONKO_OK); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_modifier3d_component(world: *mut PonkoWorld, out_id: *mut u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::component(world, out_id) }
}

/// The contacts between two bodies in one step, as the contact hook sees
/// them (Rust `ContactPair3D`). Opaque: it is valid only during the hook call
/// it was passed to. Changes affect this step only; the hook runs again the
/// next.
/// ```
/// let none: Option<&ponko_ffi::PonkoContactPair3D> = None;
/// assert!(none.is_none());
/// ```
pub struct PonkoContactPair3D {
    _private: (),
}

/// The contact hook (see `ponko_ctx_set_contact_hook3d`). `pair` is valid
/// only until the hook returns. The hook must return normally (no C++
/// exceptions, no `longjmp`) and must not call any `ponko_ctx_*` or
/// `ponko_world_*` function: it runs inside the physics step.
/// ```
/// let hook: ponko_ffi::PonkoContactHook3D = None;
/// assert!(hook.is_none());
/// ```
pub type PonkoContactHook3D = Option<unsafe extern "C" fn(pair: *mut PonkoContactPair3D, user: *mut c_void)>;

/// Sets the custom contact hook (Rust `Physics3D::set_contact_hook`): each
/// step, before the solver uses them, `hook(pair, user)` is called for the
/// contacts of every body whose `PonkoContactModifier3D::hook` is set. It
/// can drop contacts or points, change friction and bounciness, or set a
/// surface velocity (the `ponko_contact_pair3d_*` functions). Replaces an
/// earlier hook. A NULL `hook` clears it.
///
/// The hook runs on the thread that steps physics, during this ctx's own
/// step: it sees only the pair, so keep anything else it needs behind `user`.
/// It must return normally and must not call `ponko_ctx_*` or
/// `ponko_world_*` functions.
///
/// Ownership: `user` stays the caller's; the engine passes it to `hook` and
/// never frees it. It must stay valid until the hook is replaced or cleared, or
/// the game ends.
///
/// # Safety
/// `ctx` null or live, outside query callbacks. `hook` and `user` must stay
/// valid until the hook is replaced or cleared, or the game ends.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_contact_hook3d(std::ptr::null_mut(), None, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_contact_hook3d(
    ctx: *mut PonkoCtx,
    hook: PonkoContactHook3D,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_hook(ctx, hook, user) }
}

/// The two bodies of `pair` (Rust `ContactPair3D::a` and `b`).
/// # Safety
/// `pair` is the pointer a contact hook was just given; outputs writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_entities(p, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_entities(
    pair: *mut PonkoContactPair3D,
    out_a: *mut u64,
    out_b: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::entities(pair, out_a, out_b) }
}

/// The contact direction in the world, pointing from `a` toward `b` (Rust
/// `ContactPair3D::normal`).
/// # Safety
/// `pair` is the pointer a contact hook was just given; output writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_normal(p, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_normal(
    pair: *mut PonkoContactPair3D,
    out_normal: *mut PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::normal(pair, out_normal) }
}

/// How many contact points there are (0 once ignored; Rust
/// `ContactPair3D::point_count`).
/// # Safety
/// `pair` is the pointer a contact hook was just given; output writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_point_count(p, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_point_count(
    pair: *mut PonkoContactPair3D,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::point_count(pair, out_count) }
}

/// Contact point `index` in the world (on `a`'s surface), and how far the
/// two bodies are apart there (negative: overlapping; Rust
/// `ContactPair3D::point`). `PONKO_NOT_FOUND` if there is no such point.
/// # Safety
/// `pair` is the pointer a contact hook was just given; outputs writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_point(p, 0, std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_point(
    pair: *mut PonkoContactPair3D,
    index: usize,
    out_point: *mut PonkoVec3,
    out_distance: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::point(pair, index, out_point, out_distance) }
}

/// Drops every contact this step: the bodies pass through each other (Rust
/// `ContactPair3D::ignore`).
/// # Safety
/// `pair` is the pointer a contact hook was just given.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_ignore(std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_ignore(pair: *mut PonkoContactPair3D) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::ignore(pair) }
}

/// Drops contact point `index` (Rust `ContactPair3D::remove_point`; later
/// points may change places). An index past the end does nothing.
/// # Safety
/// `pair` is the pointer a contact hook was just given.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_remove_point(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_remove_point(pair: *mut PonkoContactPair3D, index: usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::remove_point(pair, index) }
}

/// The friction used for this pair (by default from both colliders; Rust
/// `ContactPair3D::friction`).
/// # Safety
/// `pair` is the pointer a contact hook was just given; output writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_friction(p, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_friction(
    pair: *mut PonkoContactPair3D,
    out_friction: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::friction(pair, out_friction) }
}

/// Changes the friction for this pair, this step (Rust
/// `ContactPair3D::set_friction`). Must be finite; below 0 counts as 0.
/// # Safety
/// `pair` is the pointer a contact hook was just given.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_set_friction(std::ptr::null_mut(), 0.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_set_friction(
    pair: *mut PonkoContactPair3D,
    friction: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_friction(pair, friction) }
}

/// The bounciness used for this pair (Rust `ContactPair3D::restitution`).
/// # Safety
/// `pair` is the pointer a contact hook was just given; output writable.
/// ```
/// let p = std::ptr::null_mut();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_restitution(p, std::ptr::null_mut()) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_restitution(
    pair: *mut PonkoContactPair3D,
    out_restitution: *mut f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::restitution(pair, out_restitution) }
}

/// Changes the bounciness for this pair, this step (Rust
/// `ContactPair3D::set_restitution`). Must be finite; below 0 counts as 0.
/// # Safety
/// `pair` is the pointer a contact hook was just given.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_set_restitution(std::ptr::null_mut(), 0.0) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_set_restitution(
    pair: *mut PonkoContactPair3D,
    restitution: f32,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_restitution(pair, restitution) }
}

/// Makes `b` slide along `a`'s surface at `velocity` (world, m/s; the part
/// along the surface counts), as on a conveyor belt (Rust
/// `ContactPair3D::set_surface_velocity`). Must be finite.
/// # Safety
/// `pair` is the pointer a contact hook was just given.
/// ```
/// let v = ponko_ffi::PonkoVec3::default();
/// assert_eq!(unsafe { ponko_ffi::ponko_contact_pair3d_set_surface_velocity(std::ptr::null_mut(), v) } as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_contact_pair3d_set_surface_velocity(
    pair: *mut PonkoContactPair3D,
    velocity: PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { imp::set_surface_velocity(pair, velocity) }
}

#[cfg(feature = "physics3d")]
mod imp {
    use super::*;
    use crate::ctx::{ctx_access, out};
    use crate::types::to_c;
    use crate::{CMirror, Failure, Kind, Mirror, boundary, builtin, error, invalid, mirror};
    use ponko::d3::ContactModifier as ContactModifier3D;
    use ponko::d3::ContactPair as ContactPair3D;
    use ponko::prelude::Vec3;

    impl CMirror for ContactModifier3D {
        type C = PonkoContactModifier3D;
        fn from_c(c: &PonkoContactModifier3D) -> Result<Self, Failure> {
            let one_way: Vec3 = to_c(c.one_way);
            let surface_velocity: Vec3 = to_c(c.surface_velocity);
            if !one_way.is_finite() || !surface_velocity.is_finite() {
                return Err(invalid("contact modifier vectors must be finite"));
            }
            if !(c.one_way_angle >= 0.0 && c.one_way_angle.is_finite()) {
                return Err(invalid("one_way_angle must be finite and 0 or more"));
            }
            Ok(ContactModifier3D {
                one_way: c.has_one_way.then_some(one_way),
                one_way_angle: c.one_way_angle,
                surface_velocity,
                hook: c.hook,
            })
        }
        fn to_c(&self) -> PonkoContactModifier3D {
            PonkoContactModifier3D {
                has_one_way: self.one_way.is_some(),
                one_way: to_c(self.one_way.unwrap_or(Vec3::ZERO)),
                one_way_angle: self.one_way_angle,
                surface_velocity: to_c(self.surface_velocity),
                hook: self.hook,
            }
        }
    }

    static MODIFIER: Mirror = mirror::<ContactModifier3D>();

    /// How `ContactModifier3D` is seen from C (see `physics3d::kind`).
    pub(crate) fn kind(t: std::any::TypeId) -> Option<Kind> {
        (t == std::any::TypeId::of::<ContactModifier3D>()).then_some(Kind::Mirror(&MODIFIER))
    }

    pub(super) unsafe fn component(world: *mut PonkoWorld, out_id: *mut u32) -> PonkoStatus {
        // SAFETY: forwarded caller contract.
        unsafe { builtin::<ContactModifier3D>(world, out_id) }
    }

    /// The hook and its user pointer, so the closure that physics keeps
    /// (which must be `Send + Sync`) can hold them.
    struct Hook {
        f: unsafe extern "C" fn(*mut PonkoContactPair3D, *mut c_void),
        user: *mut c_void,
    }
    // SAFETY: physics calls the hook only from inside `Ctx`'s own step, on
    // the thread that runs it (rapier's `parallel` feature is off), and the
    // `Ctx` that owns the closure never leaves the game's thread. The caller
    // of `ponko_ctx_set_contact_hook3d` promised `user` stays usable by
    // that thread. So the pointer is never used from two threads at once.
    unsafe impl Send for Hook {}
    // SAFETY: as for `Send`: calls never overlap.
    unsafe impl Sync for Hook {}
    impl Hook {
        fn call(&self, pair: &mut ContactPair3D) {
            let raw: *mut PonkoContactPair3D = (pair as *mut ContactPair3D).cast();
            // SAFETY: `pair` outlives the call; the caller of
            // ponko_ctx_set_contact_hook3d promised `f` and `user` stay valid.
            unsafe { (self.f)(raw, self.user) }
        }
    }

    pub(super) unsafe fn set_hook(ctx: *mut PonkoCtx, hook: PonkoContactHook3D, user: *mut c_void) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            unsafe {
                ctx_access(ctx, |c| {
                    match hook {
                        Some(f) => {
                            let hook = Hook { f, user };
                            c.physics3d.set_contact_hook(move |pair| hook.call(pair));
                        }
                        None => c.physics3d.clear_contact_hook(),
                    }
                    Ok(())
                })
            }
        })
    }

    /// The pair behind a pointer a hook was given.
    /// # Safety
    /// `pair` null, or the pointer passed to the running hook.
    unsafe fn pair<'p>(pair: *mut PonkoContactPair3D) -> Result<&'p mut ContactPair3D<'p, 'p>, Failure> {
        if pair.is_null() {
            return Err(invalid("pair is null"));
        }
        // SAFETY: `Hook::call` made this pointer from a live `ContactPair3D`.
        Ok(unsafe { &mut *pair.cast::<ContactPair3D>() })
    }

    fn finite(x: f32, what: &'static str) -> Result<f32, Failure> {
        if x.is_finite() { Ok(x) } else { Err(invalid(what)) }
    }

    pub(super) unsafe fn entities(p: *mut PonkoContactPair3D, out_a: *mut u64, out_b: *mut u64) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            // SAFETY: forwarded caller contract.
            unsafe {
                out(out_a, pair.a.to_bits())?;
                out(out_b, pair.b.to_bits())
            }
        })
    }

    pub(super) unsafe fn normal(p: *mut PonkoContactPair3D, out_normal: *mut PonkoVec3) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_normal, to_c(pair.normal())) }
        })
    }

    pub(super) unsafe fn point_count(p: *mut PonkoContactPair3D, out_count: *mut usize) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_count, pair.point_count()) }
        })
    }

    pub(super) unsafe fn point(
        p: *mut PonkoContactPair3D,
        index: usize,
        out_point: *mut PonkoVec3,
        out_distance: *mut f32,
    ) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            let (point, distance) = pair
                .point(index)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no such contact point"))?;
            // SAFETY: forwarded caller contract.
            unsafe {
                out(out_point, to_c(point))?;
                out(out_distance, distance)
            }
        })
    }

    pub(super) unsafe fn ignore(p: *mut PonkoContactPair3D) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            unsafe { pair(p) }?.ignore();
            Ok(())
        })
    }

    pub(super) unsafe fn remove_point(p: *mut PonkoContactPair3D, index: usize) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            unsafe { pair(p) }?.remove_point(index);
            Ok(())
        })
    }

    pub(super) unsafe fn friction(p: *mut PonkoContactPair3D, out_friction: *mut f32) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_friction, pair.friction()) }
        })
    }

    pub(super) unsafe fn set_friction(p: *mut PonkoContactPair3D, friction: f32) -> PonkoStatus {
        boundary(|| {
            let friction = finite(friction, "friction must be finite")?;
            // SAFETY: forwarded caller contract.
            unsafe { pair(p) }?.set_friction(friction);
            Ok(())
        })
    }

    pub(super) unsafe fn restitution(p: *mut PonkoContactPair3D, out_restitution: *mut f32) -> PonkoStatus {
        boundary(|| {
            // SAFETY: forwarded caller contract.
            let pair = unsafe { pair(p) }?;
            // SAFETY: forwarded caller contract.
            unsafe { out(out_restitution, pair.restitution()) }
        })
    }

    pub(super) unsafe fn set_restitution(p: *mut PonkoContactPair3D, restitution: f32) -> PonkoStatus {
        boundary(|| {
            let restitution = finite(restitution, "restitution must be finite")?;
            // SAFETY: forwarded caller contract.
            unsafe { pair(p) }?.set_restitution(restitution);
            Ok(())
        })
    }

    pub(super) unsafe fn set_surface_velocity(p: *mut PonkoContactPair3D, velocity: PonkoVec3) -> PonkoStatus {
        boundary(|| {
            let velocity: Vec3 = to_c(velocity);
            if !velocity.is_finite() {
                return Err(invalid("surface velocity must be finite"));
            }
            // SAFETY: forwarded caller contract.
            unsafe { pair(p) }?.set_surface_velocity(velocity);
            Ok(())
        })
    }
}

#[cfg(not(feature = "physics3d"))]
mod imp {
    //! Without the feature: every call fails, saying why.
    use super::*;
    use crate::{Failure, boundary, invalid};

    fn off() -> Result<(), Failure> {
        Err(invalid("ponko_ffi was built without the `physics3d` feature"))
    }
    pub(crate) fn kind(_: std::any::TypeId) -> Option<crate::Kind> {
        None
    }
    pub(super) unsafe fn component(_: *mut PonkoWorld, _: *mut u32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_hook(_: *mut PonkoCtx, _: PonkoContactHook3D, _: *mut c_void) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn entities(_: *mut PonkoContactPair3D, _: *mut u64, _: *mut u64) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn normal(_: *mut PonkoContactPair3D, _: *mut PonkoVec3) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn point_count(_: *mut PonkoContactPair3D, _: *mut usize) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn point(_: *mut PonkoContactPair3D, _: usize, _: *mut PonkoVec3, _: *mut f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn ignore(_: *mut PonkoContactPair3D) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn remove_point(_: *mut PonkoContactPair3D, _: usize) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn friction(_: *mut PonkoContactPair3D, _: *mut f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_friction(_: *mut PonkoContactPair3D, _: f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn restitution(_: *mut PonkoContactPair3D, _: *mut f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_restitution(_: *mut PonkoContactPair3D, _: f32) -> PonkoStatus {
        boundary(off)
    }
    pub(super) unsafe fn set_surface_velocity(_: *mut PonkoContactPair3D, _: PonkoVec3) -> PonkoStatus {
        boundary(off)
    }
}

pub(crate) use imp::kind as contact_kind;
