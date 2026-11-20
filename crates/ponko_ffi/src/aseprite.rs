//! Runtime Aseprite JSON Array loading and playback.

use std::ffi::c_char;

use ponko::d2::Sprite;
use ponko_ecs::Entity;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::types::{PonkoSprite, sprite_to_c};
use crate::{PonkoStatus, boundary, engine_error, error, invalid};

/// Loads an Aseprite export, returning the sprite for a timeline frame.
/// The context caches the atlas and owns its texture. `layer` is null for
/// a composite export or names the layer when `split_layers` is true.
/// A sprite is a snapshot of the current atlas layout.
///
/// # Safety
/// Live ctx, readable NUL-terminated `path` and `layer` (if non-null),
/// and writable `out_sprite`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_aseprite_frame(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    split_layers: bool,
    frame: u32,
    layer: *const c_char,
    out_sprite: *mut PonkoSprite,
) -> PonkoStatus {
    boundary(|| {
        if out_sprite.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let path = text(path)?;
            let layer = if layer.is_null() { None } else { Some(text(layer)?) };
            ctx_access(ctx, |c| {
                let atlas = c.gfx.try_load_aseprite(path, split_layers).map_err(engine_error)?;
                let sprite = atlas
                    .sprite(frame, layer)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "unknown Aseprite frame or layer"))?;
                out(out_sprite, sprite_to_c(&sprite))
            })
        }
    })
}

/// Starts an Aseprite tag on an existing entity, adding a sprite or
/// updating its image and region. An empty tag plays the whole timeline.
/// Use a null layer for a composite export. The built-in sprite animation
/// system advances the tag and uses the latest successful hot reload.
///
/// # Safety
/// Live ctx; `path`, `tag`, and non-null `layer` are readable NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_play_aseprite(
    ctx: *mut PonkoCtx,
    entity: u64,
    path: *const c_char,
    split_layers: bool,
    tag: *const c_char,
    layer: *const c_char,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        let tag = text(tag)?;
        let layer = if layer.is_null() { None } else { Some(text(layer)?) };
        ctx_access(ctx, |c| {
            let entity = Entity::from_bits(entity);
            if !c.world.is_alive(entity) {
                return Err(error(PonkoStatus::PONKO_NOT_FOUND, "no such entity"));
            }
            let atlas = c.gfx.try_load_aseprite(path, split_layers).map_err(engine_error)?;
            let animation = atlas.animation(tag, layer).map_err(engine_error)?;
            let frame = animation
                .sprite_at(0.0)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no Aseprite start frame"))?;
            if let Some(sprite) = c.world.get_mut::<Sprite>(entity) {
                sprite.texture = frame.texture;
                sprite.region = frame.region;
            } else {
                c.world.insert(entity, frame);
            }
            c.world.insert(entity, animation);
            Ok(())
        })
    })
}
