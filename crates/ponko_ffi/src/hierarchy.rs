//! The entity hierarchy from C: parents, ordered children, recursive
//! despawn and the update that places changed subtrees (`WorldHierarchy`
//! in Rust). The link itself is an attachment (`PonkoAttached`, or
//! `PonkoAttached3D` in 3D); `ponko_set_parent` makes one that keeps the
//! child where it is.
use ponko::prelude::{CommandsHierarchy, Entity, WorldHierarchy};

use crate::ctx::{PonkoCtx, collect, commands};
use crate::{PonkoStatus, PonkoWorld, access, boundary, engine_error, error, invalid};

/// Makes `child` hang off `parent`, last among its children, without moving
/// it (it gets a `PonkoAttached`, or a `PonkoAttached3D` if both have 3D
/// transforms). `PONKO_NOT_FOUND` if either is dead;
/// `PONKO_INVALID_ARGUMENT` if the link would make a loop or the two aren't
/// both 2D or both 3D.
/// # Safety
/// `world` must be null or a live handle.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { assert_eq!(ponko_set_parent(w, 1, 2), PonkoStatus::PONKO_NOT_FOUND); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_set_parent(world: *mut PonkoWorld, child: u64, parent: u64) -> PonkoStatus {
    // SAFETY: caller guarantees a live handle; access enforces exclusive use.
    boundary(|| unsafe {
        access(world, |w, _| {
            w.set_parent(Entity::from_bits(child), Entity::from_bits(parent))
                .map_err(engine_error)
        })
    })
}

/// Makes `child` a root where it is now (removes its attachment and takes it
/// out of its parent's children). Writes whether it had a parent to
/// `*out_had_parent`, which may be null.
/// # Safety
/// `world` must be null or a live handle; `out_had_parent` writable or null.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut had = true; assert_eq!(ponko_remove_parent(w, 1, &mut had), PonkoStatus::PONKO_OK); assert!(!had); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_remove_parent(
    world: *mut PonkoWorld,
    child: u64,
    out_had_parent: *mut bool,
) -> PonkoStatus {
    // SAFETY: caller guarantees a live handle and a writable or null output.
    boundary(|| unsafe {
        let had = access(world, |w, _| Ok(w.remove_parent(Entity::from_bits(child))))?;
        if !out_had_parent.is_null() {
            out_had_parent.write(had);
        }
        Ok(())
    })
}

/// Writes the entity `e` hangs off to `*out_parent`; `PONKO_NOT_FOUND` if it
/// has none (it is a root, or dead).
/// # Safety
/// `world` must be null or a live handle; `out_parent` writable.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut p = 0; assert_eq!(ponko_parent(w, 1, &mut p), PonkoStatus::PONKO_NOT_FOUND); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_parent(world: *mut PonkoWorld, e: u64, out_parent: *mut u64) -> PonkoStatus {
    boundary(|| {
        if out_parent.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: caller guarantees a live handle and a writable output.
        unsafe {
            let parent = access(world, |w, _| Ok(w.parent(Entity::from_bits(e))))?;
            let parent = parent.ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "the entity has no parent"))?;
            out_parent.write(parent.to_bits());
        }
        Ok(())
    })
}

/// Which entities [`list`] writes.
#[derive(Clone, Copy)]
enum Which {
    Children,
    Descendants,
    Ancestors,
}

/// Writes `e`'s children, descendants or ancestors to `out_entities`.
unsafe fn list(
    world: *mut PonkoWorld,
    e: u64,
    which: Which,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract; `collect` checks and caps every write.
    boundary(|| unsafe {
        access(world, |w, _| {
            let e = Entity::from_bits(e);
            let mut n = 0;
            collect(out_entities, capacity, out_count, &mut n, |sink| match which {
                Which::Children => w.children(e).for_each(|c| sink.push(c.to_bits())),
                Which::Descendants => w.descendants(e).for_each(|c| sink.push(c.to_bits())),
                Which::Ancestors => w.ancestors(e).for_each(|c| sink.push(c.to_bits())),
            })
        })
    })
}

/// Writes `e`'s children, in order, to `out_entities` (up to `capacity`)
/// and how many it has to `*out_count`. Call with capacity 0 to size a
/// buffer.
/// # Safety
/// `world` must be null or a live handle; `out_entities` writable for
/// `capacity` ids (null with 0); `out_count` writable.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut n = 9; assert_eq!(ponko_children(w, 1, std::ptr::null_mut(), 0, &mut n), PonkoStatus::PONKO_OK); assert_eq!(n, 0); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_children(
    world: *mut PonkoWorld,
    e: u64,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { list(world, e, Which::Children, out_entities, capacity, out_count) }
}

/// Like `ponko_children`, for everything under `e`: depth first, each
/// entity before its children, siblings in order.
/// # Safety
/// As `ponko_children`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut n = 9; assert_eq!(ponko_descendants(w, 1, std::ptr::null_mut(), 0, &mut n), PonkoStatus::PONKO_OK); assert_eq!(n, 0); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_descendants(
    world: *mut PonkoWorld,
    e: u64,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { list(world, e, Which::Descendants, out_entities, capacity, out_count) }
}

/// Like `ponko_children`, for the entities above `e`, its parent first.
/// # Safety
/// As `ponko_children`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut n = 9; assert_eq!(ponko_ancestors(w, 1, std::ptr::null_mut(), 0, &mut n), PonkoStatus::PONKO_OK); assert_eq!(n, 0); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ancestors(
    world: *mut PonkoWorld,
    e: u64,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { list(world, e, Which::Ancestors, out_entities, capacity, out_count) }
}

/// Moves `child` to position `index` among its parent's children (the last
/// if `index` is past the end). `PONKO_NOT_FOUND` if it has no parent.
/// # Safety
/// `world` must be null or a live handle.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { assert_eq!(ponko_set_child_index(w, 1, 0), PonkoStatus::PONKO_NOT_FOUND); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_set_child_index(world: *mut PonkoWorld, child: u64, index: usize) -> PonkoStatus {
    // SAFETY: caller guarantees a live handle; access enforces exclusive use.
    boundary(|| unsafe {
        access(world, |w, _| {
            if w.set_child_index(Entity::from_bits(child), index) {
                Ok(())
            } else {
                Err(error(PonkoStatus::PONKO_NOT_FOUND, "the entity has no parent"))
            }
        })
    })
}

/// Despawns `e` and everything under it, writing how many entities that
/// was to `*out_count` (which may be null). A dead `e` despawns nothing.
/// # Safety
/// `world` must be null or a live handle; `out_count` writable or null.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { let mut n = 9; assert_eq!(ponko_despawn_recursive(w, 1, &mut n), PonkoStatus::PONKO_OK); assert_eq!(n, 0); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_despawn_recursive(world: *mut PonkoWorld, e: u64, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: caller guarantees a live handle and a writable or null output.
    boundary(|| unsafe {
        let n = access(world, |w, _| Ok(w.despawn_recursive(Entity::from_bits(e))))?;
        if !out_count.is_null() {
            out_count.write(n);
        }
        Ok(())
    })
}

/// Despawns `entity` and everything under it after this system (Rust
/// `ctx.commands.despawn_recursive`): the subtree as it is when the
/// system's commands are applied. Does nothing if `entity` is dead by then.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_despawn_recursive_later(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// Works inside `ponko_query` callbacks (not `ponko_query_par`).
/// Applied after the system returns.
/// # Safety
/// `ctx` must be null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_despawn_recursive_later(ctx: *mut PonkoCtx, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        commands(ctx, |c| {
            c.despawn_recursive(Entity::from_bits(entity));
            Ok(())
        })
    })
}

/// Brings the hierarchy up to date now: children lists, despawns under
/// despawned parents, and the transforms of every subtree whose root moved
/// or whose links changed (what `ponko_game_follow_attachments`' system
/// does once per tick).
/// # Safety
/// `world` must be null or a live handle.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe { assert_eq!(ponko_update_hierarchy(w), PonkoStatus::PONKO_OK); ponko_world_free(w); }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_update_hierarchy(world: *mut PonkoWorld) -> PonkoStatus {
    // SAFETY: caller guarantees a live handle; access enforces exclusive use.
    boundary(|| unsafe {
        access(world, |w, _| {
            w.update_hierarchy();
            Ok(())
        })
    })
}
