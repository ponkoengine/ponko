//! Required components from C: `ponko_component_require`. The rules are
//! Rust's (`ponko_ecs::Requires`); a C requirement copies a value's bytes.
use std::ffi::c_void;

use crate::{Kind, PonkoStatus, PonkoWorld, access, boundary, invalid};

/// Makes component `component` require component `required` in this world:
/// from now on every spawn and set (from C or Rust) that gives an entity
/// `component` also gives it `required` if it lacks one, as a copy of the
/// bytes at `default_value` (the registered size of `required`, copied by
/// this call). What `required` requires comes too; values the spawn gives,
/// or the entity has, win. Entities that have `component` already are left
/// as they are. Calling it again for the same pair replaces the value.
///
/// `required` must be a plain value: a component registered from C, or a
/// built-in one C sees as its own bytes (`PonkoTransform2D`,
/// `PonkoVelocity`, ...); a converted one (`PonkoSprite`, `PonkoRigidBody`,
/// ...) is `PONKO_INVALID_ARGUMENT`. The engine's own requirements (a body
/// brings a transform and a velocity, a collider a fixed body) apply to C
/// spawns without this call.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let (mut enemy, mut health) = (0, 0);
///     ponko_component_register(w, c"Enemy".as_ptr(), 0, 1, &mut enemy);
///     ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut health);
///     let full = 100.0f32;
///     let value = (&raw const full).cast();
///     assert_eq!(ponko_component_require(w, enemy, health, value), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_component_require(w, enemy, 99, value), PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` null or a live handle; `default_value` readable for the
/// registered size of `required` (may be null when that is 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_component_require(
    world: *mut PonkoWorld,
    component: u32,
    required: u32,
    default_value: *const c_void,
) -> PonkoStatus {
    boundary(|| {
        // SAFETY: caller supplies a live handle; ids and kinds are checked.
        unsafe {
            access(world, |w, _| {
                let (of, _) = crate::component(w, component)?;
                let (needed, kind) = crate::component(w, required)?;
                if !matches!(kind, Kind::Plain) {
                    return Err(invalid(
                        "required component must be a plain value (registered from C, or a built-in such as PonkoTransform2D); require converted built-ins from Rust",
                    ));
                }
                let size = w.component_info(needed).layout().size();
                if default_value.is_null() && size != 0 {
                    return Err(invalid("default value is null"));
                }
                // SAFETY: `needed` is plain (no destructor: `component`
                // checks), and the caller's value is `size` readable bytes.
                w.raw().require_dynamic(of, needed, default_value.cast());
                Ok(())
            })
        }
    })
}
