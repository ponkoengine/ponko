//! Retained UI on surfaces in the 3D world from C (Rust `UiPanel3D`,
//! `Graphics::create_ui_panel`, `Ui::set_pointer_ray`).
//!
//! `ponko_ctx_create_ui_panel` gives an entity a panel: its own UI, drawn
//! every frame into a render target that a glowing quad shows. Build that UI
//! with the `ponko_ctx_ui_*` calls, passing the entity as their `target`
//! (`PONKO_UI_SCREEN` is the screen's own UI). The mouse works on the nearest
//! panel its ray hits, in the panel's pixels, unless the screen's UI has it.

use ponko::render::Model;
use ponko::{
    d3::UiPanel as UiPanel3D,
    prelude::{Ctx, Vec3},
};

use crate::ctx::{ctx_access, out};
use crate::particles::entity;
use crate::types::{PonkoTexture, PonkoVec2, texture_to_c, to_c};
use crate::{Failure, PonkoCtx, PonkoStatus, PonkoVec3, boundary, error, invalid, invalid_owned};

fn panel(c: &mut Ctx, bits: u64) -> Result<&mut UiPanel3D, Failure> {
    let e = entity(c, bits)?;
    c.world
        .get_mut::<UiPanel3D>(e)
        .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity has no UI panel"))
}

/// Gives `entity` a UI panel `width` x `height` pixels, `size` meters wide
/// and high in the world, and the model that shows it: a quad in the entity's
/// XY plane, centered on it and facing +Z, with a glowing, alpha-blended
/// material. The entity needs a 3D transform to be drawn and hit. This
/// replaces the entity's model and any panel it had. Build the panel's UI with
/// the `ponko_ctx_ui_*` calls, giving the entity as their `target`. Writes the
/// panel's texture (a render target) to `out_texture` unless it is null.
/// ```
/// let size = ponko_ffi::PonkoVec2 { x: 1.0, y: 0.5 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_create_ui_panel(std::ptr::null_mut(), 0, 256, 128, size, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_texture` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_create_ui_panel(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    width: u32,
    height: u32,
    size: PonkoVec2,
    out_texture: *mut PonkoTexture,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            let size: ponko::prelude::Vec2 = to_c(size);
            let (panel, model): (UiPanel3D, Model) =
                c.gfx.try_create_ui_panel(width, height, size).map_err(invalid_owned)?;
            let texture = panel.target.texture();
            c.world.insert(e, panel);
            c.world.insert(e, model);
            if out_texture.is_null() {
                Ok(())
            } else {
                out(out_texture, texture_to_c(texture))
            }
        })
    })
}

/// Removes `entity`'s UI panel and its UI. Its model stays: despawn the entity
/// or remove the model to hide the quad.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_remove_ui_panel(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_remove_ui_panel(ctx: *mut PonkoCtx, entity_bits: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let e = entity(c, entity_bits)?;
            c.world.remove::<UiPanel3D>(e);
            Ok(())
        })
    })
}

/// Whether the mouse and keyboard can use `entity`'s panel (on by default).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_ui_panel_interactive(std::ptr::null_mut(), 0, false) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_ui_panel_interactive(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    interactive: bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            panel(c, entity_bits)?.interactive = interactive;
            Ok(())
        })
    })
}

/// Where the mouse (or the ray from `ponko_ctx_set_ui_pointer_ray`) is on
/// `entity`'s panel this frame: `out_over` says whether it is over it, and
/// `out_pixel` is the spot in the panel's pixels (zero when it isn't).
/// ```
/// let (mut over, mut at) = (false, Default::default());
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_panel_pointer(std::ptr::null_mut(), 0, &mut over, &mut at) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_panel_pointer(
    ctx: *mut PonkoCtx,
    entity_bits: u64,
    out_over: *mut bool,
    out_pixel: *mut PonkoVec2,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_over.is_null() || out_pixel.is_null() {
            return Err(invalid("output is null"));
        }
        ctx_access(ctx, |c| {
            let at = panel(c, entity_bits)?.pointer();
            // SAFETY: both outputs checked non-null above; the caller guarantees they are writable.
            out_over.write(at.is_some());
            out_pixel.write(to_c(at.unwrap_or_default()));
            Ok(())
        })
    })
}

/// Makes panels use the ray from `origin` along `direction` (in the world)
/// instead of the camera's ray through the mouse, from this frame on: aim
/// with the camera's forward ray while the cursor is locked, or a controller's
/// in VR. Clear it with `ponko_ctx_clear_ui_pointer_ray`. A zero or
/// non-finite `direction` fails with `PONKO_INVALID_ARGUMENT`.
/// ```
/// let v = ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: -1.0 };
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_set_ui_pointer_ray(std::ptr::null_mut(), Default::default(), v) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_set_ui_pointer_ray(
    ctx: *mut PonkoCtx,
    origin: PonkoVec3,
    direction: PonkoVec3,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let (origin, direction): (Vec3, Vec3) = (to_c(origin), to_c(direction));
        if !origin.is_finite() || !direction.is_finite() || direction == Vec3::ZERO {
            return Err(invalid("ray needs a finite origin and a nonzero, finite direction"));
        }
        ctx_access(ctx, |c| {
            c.ui.set_pointer_ray(Some((origin, direction)));
            Ok(())
        })
    })
}

/// Goes back to the camera's ray through the mouse for panels.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_clear_ui_pointer_ray(std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_clear_ui_pointer_ray(ctx: *mut PonkoCtx) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            c.ui.set_pointer_ray(None);
            Ok(())
        })
    })
}
