//! Component hooks from C: `ponko_component_on_add`, `_on_replace` and
//! `_on_remove` (Rust `World::on_add`, `on_replace`, `on_remove`), and what
//! a hook may do while it runs (`ponko_hook_*`).
//!
//! A hook runs inside the operation that triggered it (a spawn, set,
//! remove, despawn, from C or Rust), while the world is busy with it: the
//! world's own calls return `PONKO_BUSY` there. The `PonkoHookWorld` it gets
//! reads and writes components in place, and records structural changes as
//! commands, applied before the call that ran the hook returns: the same
//! rules as Rust's `HookWorld`.
use std::ffi::c_void;
use std::sync::Arc;

use ponko_ecs::{ComponentId, Entity, HookWorld};

use crate::ctx::{Owned, report};
use crate::{
    Failure, Kind, PonkoStatus, PonkoValue, PonkoWorld, access, boundary, c_size, error, insert_value, invalid,
    spawn_values_as, value_layout,
};

/// The world as a hook sees it, valid only while the hook runs. Pass it to
/// the `ponko_hook_*` calls.
pub struct PonkoHookWorld {
    /// The `HookWorld` of the running hook, lifetime erased.
    world: *mut c_void,
}

impl PonkoHookWorld {
    /// # Safety
    /// `self` was made by `call` for a hook that is still running.
    unsafe fn hook_world(&mut self) -> &mut HookWorld<'static> {
        // SAFETY: `call` stored a `&mut HookWorld` that outlives the hook.
        unsafe { &mut *self.world.cast::<HookWorld<'static>>() }
    }
}

/// Which hook runs (`PonkoHookEvent::kind`).
/// ```
/// assert_eq!(ponko_ffi::PonkoHookKind::PONKO_HOOK_REMOVE as u32, 2);
/// ```
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoHookKind {
    /// The component arrived on an entity that had none.
    PONKO_HOOK_ADD = 0,
    /// A set is about to overwrite the entity's component.
    PONKO_HOOK_REPLACE = 1,
    /// The component is about to leave the entity (remove, despawn, clear).
    PONKO_HOOK_REMOVE = 2,
}

/// What a hook is called for. Its pointers are valid until the hook returns.
/// ```
/// let e = ponko_ffi::PonkoHookEvent { entity: 1, component: 0, kind: 0, value: std::ptr::null(), new_value: std::ptr::null(), size: 0 };
/// assert_eq!(e.entity, 1);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct PonkoHookEvent {
    /// The entity.
    pub entity: u64,
    /// The component's id.
    pub component: u32,
    /// Which hook (a `PonkoHookKind`).
    pub kind: u32,
    /// The component in the world: the new value for an add, the old one
    /// for a replace or a remove. `size` bytes, read-only.
    pub value: *const c_void,
    /// A replace: the value about to be written (not in the world yet).
    /// Null for add and remove.
    pub new_value: *const c_void,
    /// The component's size in bytes.
    pub size: usize,
}

/// A C hook: the world (for `ponko_hook_*`), what happened, and the `user`
/// pointer given when it was registered. Must return normally.
/// ```
/// let hook: ponko_ffi::PonkoHookFn = None;
/// assert!(hook.is_none());
/// ```
pub type PonkoHookFn = Option<unsafe extern "C" fn(*mut PonkoHookWorld, *const PonkoHookEvent, *mut c_void)>;

/// Called once with the hook's `user` pointer when the world that holds the
/// hook is freed: C++ frees its function object there.
/// ```
/// let free: ponko_ffi::PonkoFreeUserFn = None;
/// assert!(free.is_none());
/// ```
pub type PonkoFreeUserFn = Option<unsafe extern "C" fn(*mut c_void)>;

/// A registered C hook and its data; frees the data with the world.
struct CHook {
    hook: unsafe extern "C" fn(*mut PonkoHookWorld, *const PonkoHookEvent, *mut c_void),
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
    kind: PonkoHookKind,
    component: u32,
    size: usize,
}

// SAFETY: hooks run on the thread that changes the world's structure (the
// game's thread; never during a parallel query). The caller promises its
// `user` data may be used from there and freed from where the world is
// freed (the documented contract of `ponko_component_on_add`).
unsafe impl Send for CHook {}
// SAFETY: as above; the hook is only called, never changed.
unsafe impl Sync for CHook {}

impl Drop for CHook {
    fn drop(&mut self) {
        if let Some(free) = self.free_user {
            // SAFETY: the caller's contract: `free_user(user)` is called
            // exactly once, when the world is done with the hook.
            unsafe { free(self.user) };
        }
    }
}

impl CHook {
    fn call(&self, world: &mut HookWorld<'_>, e: Entity, id: ComponentId, new: *const u8) {
        let value = world.get_dynamic(e, id).unwrap_or(std::ptr::null());
        let event = PonkoHookEvent {
            entity: e.to_bits(),
            component: self.component,
            kind: self.kind as u32,
            value: value.cast(),
            new_value: new.cast(),
            size: self.size,
        };
        let mut handle = PonkoHookWorld {
            world: std::ptr::from_mut(world).cast(),
        };
        // SAFETY: the caller's contract for the hook; `handle` and `event`
        // live until it returns.
        unsafe { (self.hook)(&mut handle, &event, self.user) };
    }
}

unsafe fn register(
    world: *mut PonkoWorld,
    component: u32,
    kind: PonkoHookKind,
    hook: PonkoHookFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
) -> PonkoStatus {
    boundary(|| {
        let hook = hook.ok_or_else(|| invalid("hook is null"))?;
        // SAFETY: caller supplies a live handle.
        unsafe {
            access(world, |w, _| {
                let (id, k) = crate::component(w, component)?;
                if !k.is_plain() {
                    return Err(invalid(
                        "hooks from C take plain components (registered from C, or built-ins C sees as their own bytes); hook converted built-ins from Rust",
                    ));
                }
                let c = Arc::new(CHook {
                    hook,
                    user,
                    free_user,
                    kind,
                    component,
                    size: w.component_info(id).layout().size(),
                });
                let dynamic: ponko_ecs::DynamicHook = Arc::new(move |hw, e, id, new| c.call(hw, e, id, new));
                match kind {
                    PonkoHookKind::PONKO_HOOK_ADD => w.raw().on_add_dynamic(id, dynamic),
                    PonkoHookKind::PONKO_HOOK_REPLACE => w.raw().on_replace_dynamic(id, dynamic),
                    PonkoHookKind::PONKO_HOOK_REMOVE => w.raw().on_remove_dynamic(id, dynamic),
                }
                Ok(())
            })
        }
    })
}

/// Calls `hook` whenever `component` arrives on an entity that had none:
/// every spawn and every set that adds it, from C, C++ or Rust (Rust
/// `World::on_add`). It runs once the entity is complete, so
/// `event->value` is the new value. Several hooks of a component run in
/// the order they were registered; they stay for the life of the world.
///
/// `component` must be a plain value (registered from C, or a built-in C
/// sees as its own bytes, like `PonkoTransform3D`); a converted built-in
/// (`PonkoSprite`, bodies, colliders) is `PONKO_INVALID_ARGUMENT`.
///
/// Inside the hook the world is busy: use the `PonkoHookWorld` it gets
/// (`ponko_hook_get`, `ponko_hook_write`, `ponko_hook_*_later`).
///
/// Ownership: the world keeps `hook` and `user` for its whole life and
/// calls `free_user(user)` (if not null) once when it is freed; `user` must
/// stay valid until then. The hook runs on the thread that changes the
/// world (the game's thread), never during a parallel query.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut id = 0;
///     ponko_component_register(w, c"Coin".as_ptr(), 4, 4, &mut id);
///     assert_eq!(ponko_component_on_add(w, id, None, std::ptr::null_mut(), None), PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` null or live; `hook` and `free_user` callable as described.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_component_on_add(
    world: *mut PonkoWorld,
    component: u32,
    hook: PonkoHookFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe { register(world, component, PonkoHookKind::PONKO_HOOK_ADD, hook, user, free_user) }
}

/// Calls `hook` whenever a set is about to overwrite an entity's
/// `component` (Rust `World::on_replace`): `event->value` is the old value,
/// still in the world, and `event->new_value` the one about to be written.
/// Writes through query columns or `ponko_hook_write` are not replacements
/// and run no hook. Otherwise as `ponko_component_on_add`.
///
/// Ownership: as `ponko_component_on_add`: the world keeps `hook` and `user`
/// and calls `free_user(user)` once when it is freed.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_component_on_replace(std::ptr::null_mut(), 0, None, std::ptr::null_mut(), None) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// As `ponko_component_on_add`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_component_on_replace(
    world: *mut PonkoWorld,
    component: u32,
    hook: PonkoHookFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        register(
            world,
            component,
            PonkoHookKind::PONKO_HOOK_REPLACE,
            hook,
            user,
            free_user,
        )
    }
}

/// Calls `hook` whenever `component` is about to leave an entity: a
/// remove, a despawn (which runs the hooks of all the entity's components,
/// then removes it) or clearing the world (Rust `World::on_remove`).
/// `event->value` is the value that goes, still in place. Otherwise as
/// `ponko_component_on_add`.
///
/// Ownership: as `ponko_component_on_add`: the world keeps `hook` and `user`
/// and calls `free_user(user)` once when it is freed.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_component_on_remove(std::ptr::null_mut(), 0, None, std::ptr::null_mut(), None) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// As `ponko_component_on_add`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_component_on_remove(
    world: *mut PonkoWorld,
    component: u32,
    hook: PonkoHookFn,
    user: *mut c_void,
    free_user: PonkoFreeUserFn,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        register(
            world,
            component,
            PonkoHookKind::PONKO_HOOK_REMOVE,
            hook,
            user,
            free_user,
        )
    }
}

/// Runs `f` with the hook world behind `hw`.
///
/// # Safety
/// `hw` null or the handle a running hook got.
unsafe fn with_hook<R>(
    hw: *mut PonkoHookWorld,
    f: impl FnOnce(&mut HookWorld<'static>) -> Result<R, Failure>,
) -> Result<R, Failure> {
    // SAFETY: caller contract.
    let hw = unsafe { hw.as_mut() }.ok_or_else(|| invalid("hook world is null"))?;
    // SAFETY: caller contract: the hook is still running.
    f(unsafe { hw.hook_world() })
}

/// Copies component `component` of `entity` into `out_data` (`size` bytes,
/// its exact registered C size), from inside a hook: any entity, any
/// component C can read (`ponko_get`'s rules). `PONKO_NOT_FOUND` if the
/// entity lacks it.
///
/// Ownership: `hw` is borrowed from the running hook; `out_data` is the
/// caller's, written during the call only.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_get(std::ptr::null_mut(), 0, 0, std::ptr::null_mut(), 0) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got; `out_data` writable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_get(
    hw: *mut PonkoHookWorld,
    entity: u64,
    component: u32,
    out_data: *mut c_void,
    size: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            let (id, kind) = value_layout(w.world(), component, out_data, size)?;
            let src = w
                .get_dynamic(Entity::from_bits(entity), id)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity or component not found"))?;
            match kind {
                Kind::Plain => std::ptr::copy_nonoverlapping(src, out_data.cast(), size),
                Kind::Mirror(m) => (m.to_c)(src, out_data),
            }
            Ok(())
        })
    })
}

/// Overwrites `entity`'s `component` in place from inside a hook (`size`
/// bytes, its registered size): no structural change, so the entity must
/// have it already (`PONKO_NOT_FOUND` otherwise; add it with
/// `ponko_hook_set_later`). Counts as a change for change detection and
/// runs no hook. Plain components only, as `ponko_component_on_add`.
///
/// Ownership: `hw` is borrowed from the running hook; `data` is read
/// during the call only.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_write(std::ptr::null_mut(), 0, 0, std::ptr::null(), 0) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got; `data` readable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_write(
    hw: *mut PonkoHookWorld,
    entity: u64,
    component: u32,
    data: *const c_void,
    size: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            let (id, kind) = value_layout(w.world(), component, data, size)?;
            if !kind.is_plain() {
                return Err(invalid(
                    "ponko_hook_write takes plain components; set converted built-ins later",
                ));
            }
            let dst = w
                .get_dynamic_mut(Entity::from_bits(entity), id)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity or component not found"))?;
            debug_assert_eq!(size, c_size(w.world(), id, kind));
            std::ptr::copy(data.cast::<u8>(), dst, size);
            Ok(())
        })
    })
}

/// Spawns an entity with copies of `values` once the operation that ran
/// the hook is done (before the call that made it returns). Writes the new
/// entity's id to `out_entity` (may be null) at once; if the values turn
/// out bad when applied, the spawn is skipped with a message on stderr and
/// the id stays dead.
///
/// Ownership: `hw` is borrowed from the running hook; `values` and their
/// bytes are copied during the call; `out_entity` is the caller's.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_spawn_later(std::ptr::null_mut(), std::ptr::null(), 0, std::ptr::null_mut()) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got; `values` readable for `count`
/// values (each with its bytes); `out_entity` null or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_spawn_later(
    hw: *mut PonkoHookWorld,
    values: *const PonkoValue,
    count: usize,
    out_entity: *mut u64,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            if values.is_null() && count != 0 {
                return Err(invalid("values is null"));
            }
            let owned = if count == 0 {
                Owned::copy(&[])?
            } else {
                Owned::copy(std::slice::from_raw_parts(values, count))?
            };
            let e = w.commands().spawn_with(move |w, e| {
                // SAFETY: the values point into `owned`, which lives until this returns.
                match spawn_values_as(w, &owned.values(), Some(e)) {
                    Ok(_) => true,
                    Err(f) => {
                        report("ponko_hook_spawn_later", f);
                        false
                    }
                }
            });
            if !out_entity.is_null() {
                out_entity.write(e.to_bits());
            }
            Ok(())
        })
    })
}

/// Adds or replaces a component of `entity` once the operation that ran
/// the hook is done (that runs the component's own hooks then).
///
/// Ownership: `hw` is borrowed from the running hook; `value` and its
/// bytes are copied during the call.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_set_later(std::ptr::null_mut(), 0, std::ptr::null()) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got; `value` readable with its bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_set_later(
    hw: *mut PonkoHookWorld,
    entity: u64,
    value: *const PonkoValue,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            if value.is_null() {
                return Err(invalid("value is null"));
            }
            let owned = Owned::copy(std::slice::from_raw_parts(value, 1))?;
            w.commands().add(move |w| {
                let v = owned.values()[0];
                let applied = value_layout(w, v.component, v.data, v.size)
                    // SAFETY: layout validated against the world just now.
                    .and_then(|(id, kind)| insert_value(w, Entity::from_bits(entity), id, kind, v.data));
                if let Err(f) = applied {
                    report("ponko_hook_set_later", f);
                }
            });
            Ok(())
        })
    })
}

/// Removes `component` from `entity` once the operation that ran the hook
/// is done.
///
/// Ownership: `hw` is borrowed from the running hook.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_remove_later(std::ptr::null_mut(), 0, 0) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_remove_later(hw: *mut PonkoHookWorld, entity: u64, component: u32) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            let (id, _) = crate::component(w.world(), component)?;
            w.commands().add(move |w| {
                w.raw().remove_dynamic(Entity::from_bits(entity), id);
            });
            Ok(())
        })
    })
}

/// Despawns `entity` once the operation that ran the hook is done (its
/// components' `on_remove` hooks run then).
///
/// Ownership: `hw` is borrowed from the running hook.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_despawn_later(std::ptr::null_mut(), 0) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_despawn_later(hw: *mut PonkoHookWorld, entity: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            w.commands().despawn(Entity::from_bits(entity));
            Ok(())
        })
    })
}

/// Whether `entity` is alive, from inside a hook (the entity the hook runs
/// for always is).
///
/// Ownership: `hw` is borrowed from the running hook; `out_alive` is the
/// caller's.
/// ```
/// use ponko_ffi::*;
/// assert_eq!(unsafe { ponko_hook_is_alive(std::ptr::null_mut(), 0, std::ptr::null_mut()) }, PonkoStatus::PONKO_INVALID_ARGUMENT);
/// ```
/// # Safety
/// `hw` null or the handle a running hook got; `out_alive` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_hook_is_alive(
    hw: *mut PonkoHookWorld,
    entity: u64,
    out_alive: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        with_hook(hw, |w| {
            if out_alive.is_null() {
                return Err(invalid("output is null"));
            }
            out_alive.write(w.is_alive(Entity::from_bits(entity)));
            Ok(())
        })
    })
}
