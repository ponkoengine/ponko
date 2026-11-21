//! Resources from C: one value per world, found by name (a score, settings,
//! a random seed), stored as plain bytes.
use std::alloc::Layout;
use std::ffi::{CStr, c_char, c_void};

use ponko::prelude::WorldReflect;

use crate::{Failure, PonkoStatus, PonkoWorld, access, boundary, error, invalid};
use crate::{PonkoField, check_fields, fields_from_c};

fn resource_name<'a>(name: *const c_char) -> Result<&'a str, Failure> {
    if name.is_null() {
        return Err(invalid("name is null"));
    }
    // SAFETY: caller supplies a NUL-terminated name.
    let name = unsafe { CStr::from_ptr(name) }
        .to_str()
        .map_err(|_| invalid("name is not UTF-8"))?;
    if name.is_empty() {
        return Err(invalid("name is empty"));
    }
    Ok(name)
}

fn resource_layout(size: usize, alignment: usize) -> Result<Layout, Failure> {
    let layout = Layout::from_size_align(size, alignment).map_err(|_| invalid("invalid resource layout"))?;
    if !size.is_multiple_of(alignment) {
        return Err(invalid("resource size must be a multiple of alignment"));
    }
    Ok(layout)
}

fn same_layout(world: &ponko::prelude::World, name: &str, layout: Layout) -> Result<(), Failure> {
    match world.raw_ref().resource_layout_dynamic(name) {
        Some(existing) if existing != layout => Err(invalid("resource exists with a different size or alignment")),
        _ => Ok(()),
    }
}

/// Sets the resource called `name` to a copy of `size` bytes at `data`,
/// adding it if the world has none. A resource keeps its size and alignment.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// let score: u32 = 7;
/// let mut out: u32 = 0;
/// unsafe {
///     let size = std::mem::size_of::<u32>();
///     assert_eq!(ponko_resource_set(w, c"Score".as_ptr(), (&raw const score).cast(), size, 4), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_resource_get(w, c"Score".as_ptr(), (&raw mut out).cast(), size), PonkoStatus::PONKO_OK);
///     assert_eq!(out, 7);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or live; `name` NUL-terminated; `data` must hold
/// `size` readable bytes of a plain value (no pointers it owns).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_resource_set(
    world: *mut PonkoWorld,
    name: *const c_char,
    data: *const c_void,
    size: usize,
    alignment: usize,
) -> PonkoStatus {
    boundary(|| {
        let name = resource_name(name)?;
        let layout = resource_layout(size, alignment)?;
        if data.is_null() {
            return Err(invalid("data is null"));
        }
        // SAFETY: access validates null and guards the live handle.
        unsafe {
            access(world, |w, _| {
                same_layout(w, name, layout)?;
                // SAFETY: the layout matches, `data` holds `size` bytes of a plain value.
                w.raw().insert_resource_dynamic(name, layout, data.cast());
                Ok(())
            })
        }
    })
}

/// Copies the resource called `name` into `out` (`size` bytes, which must be
/// its size). `PONKO_NOT_FOUND` if the world has none.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// let mut out: u32 = 0;
/// unsafe {
///     let status = ponko_resource_get(w, c"Score".as_ptr(), (&raw mut out).cast(), 4);
///     assert_eq!(status, PonkoStatus::PONKO_NOT_FOUND);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or live; `name` NUL-terminated; `out` must hold
/// `size` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_resource_get(
    world: *mut PonkoWorld,
    name: *const c_char,
    out: *mut c_void,
    size: usize,
) -> PonkoStatus {
    boundary(|| {
        let name = resource_name(name)?;
        if out.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: access validates null and guards the live handle.
        unsafe {
            access(world, |w, _| {
                let layout = w
                    .raw_ref()
                    .resource_layout_dynamic(name)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "resource not found"))?;
                if layout.size() != size {
                    return Err(invalid("resource copy size does not match"));
                }
                let from = w.raw_ref().get_resource_dynamic(name).expect("layout implies presence");
                // SAFETY: both hold `size` bytes; the world's copy is separate memory.
                std::ptr::copy_nonoverlapping(from, out.cast::<u8>(), size);
                Ok(())
            })
        }
    })
}

/// Removes the resource called `name`; `PONKO_NOT_FOUND` if there is none.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// let seed: u64 = 42;
/// unsafe {
///     ponko_resource_set(w, c"Seed".as_ptr(), (&raw const seed).cast(), 8, 8);
///     assert_eq!(ponko_resource_remove(w, c"Seed".as_ptr()), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_resource_remove(w, c"Seed".as_ptr()), PonkoStatus::PONKO_NOT_FOUND);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or live; `name` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_resource_remove(world: *mut PonkoWorld, name: *const c_char) -> PonkoStatus {
    boundary(|| {
        let name = resource_name(name)?;
        // SAFETY: access validates null and guards the live handle.
        unsafe {
            access(world, |w, _| {
                if w.raw().remove_resource_dynamic(name) {
                    Ok(())
                } else {
                    Err(error(PonkoStatus::PONKO_NOT_FOUND, "resource not found"))
                }
            })
        }
    })
}

/// Makes the resource called `name` readable and writable as JSON by tools
/// and agents, and saved by `ponko_world_save`, by describing its fields.
/// It doesn't have to exist yet: a tool setting it adds it, with bytes no
/// field covers zero. Describing it again replaces the fields. A name a
/// Rust resource is reflected under returns `PONKO_PANIC`, saying so.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// let field = PonkoField { name: c"red".as_ptr(), kind: PonkoFieldType::PONKO_FIELD_U32 as u32, offset: 0 };
/// unsafe {
///     assert_eq!(ponko_resource_reflect(w, c"Score".as_ptr(), 8, 4, &field, 1), PonkoStatus::PONKO_OK);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or live; `name` NUL-terminated; `fields` must hold
/// `len` fields with valid names. Any field values must make a valid value
/// of the resource (true for plain C structs of these types).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_resource_reflect(
    world: *mut PonkoWorld,
    name: *const c_char,
    size: usize,
    alignment: usize,
    fields: *const PonkoField,
    len: usize,
) -> PonkoStatus {
    boundary(|| {
        let name = resource_name(name)?;
        let layout = resource_layout(size, alignment)?;
        // SAFETY: forwarded from the caller.
        let list = unsafe { fields_from_c(fields, len)? };
        check_fields(&list, size, "resource")?;
        // SAFETY: access validates null and guards the live handle.
        unsafe {
            access(world, |w, _| {
                same_layout(w, name, layout)?;
                // SAFETY: fields checked; the caller promised field values make valid values.
                w.reflect_resource_fields(name, layout, &list);
                Ok(())
            })
        }
    })
}

/// What `ponko_resource_scope` calls: `world` is the world the scope was
/// opened on, without the resource, and `value` the resource's bytes,
/// readable and writable until it returns.
/// ```
/// let none: ponko_ffi::PonkoResourceScopeFn = None;
/// assert!(none.is_none());
/// ```
pub type PonkoResourceScopeFn =
    Option<unsafe extern "C" fn(world: *mut PonkoWorld, value: *mut c_void, user: *mut c_void)>;

/// The bytes of a resource taken out of the world, in memory aligned for it.
struct Taken {
    bytes: Vec<u8>,
    offset: usize,
    layout: Layout,
}

impl Taken {
    fn new(layout: Layout) -> Self {
        let bytes = vec![0; layout.size() + layout.align()];
        let offset = bytes.as_ptr().align_offset(layout.align());
        Self { bytes, offset, layout }
    }
    fn ptr(&mut self) -> *mut u8 {
        // SAFETY: `offset + size` is within the buffer (it has `align` spare bytes).
        unsafe { self.bytes.as_mut_ptr().add(self.offset) }
    }
}

/// Lends the resource called `name` and the world next to it to `scope`
/// (Rust's `World::resource_scope`): `scope(world, value, user)` gets a
/// pointer to the resource's bytes to read and change in place, and the
/// world, to query, spawn and use other resources meanwhile. It is the
/// native form of the borrows Rust writes as `resource_scope`,
/// `resources_mut` and `Query::with_resources`: run a query inside the scope
/// to use the resource in its loop, and nest scopes for several resources.
///
/// While the scope runs, the resource is not in the world: looking it up
/// finds nothing, and a nested scope for the same name returns
/// `PONKO_NOT_FOUND`. When `scope` returns, the resource is back with its
/// changes, replacing anything set under that name meanwhile.
/// `PONKO_NOT_FOUND` if there is no such resource; `size` must be its size.
/// `scope` must not free the world.
///
/// Ownership: `user` stays the caller's and is only passed to `scope` during
/// this call; the engine keeps no pointer to it after returning.
///
/// ```
/// use ponko_ffi::*;
/// use std::ffi::c_void;
/// unsafe extern "C" fn add_one(_world: *mut PonkoWorld, value: *mut c_void, _user: *mut c_void) {
///     unsafe { *value.cast::<u32>() += 1 };
/// }
/// let w = ponko_world_new();
/// let mut score: u32 = 7;
/// unsafe {
///     ponko_resource_set(w, c"Score".as_ptr(), (&raw const score).cast(), 4, 4);
///     assert_eq!(ponko_resource_scope(w, c"Score".as_ptr(), 4, Some(add_one), std::ptr::null_mut()), PonkoStatus::PONKO_OK);
///     ponko_resource_get(w, c"Score".as_ptr(), (&raw mut score).cast(), 4);
///     ponko_world_free(w);
/// }
/// assert_eq!(score, 8);
/// ```
/// # Safety
/// `world` must be null or live; `name` NUL-terminated; `scope` callable
/// with `user`, returning normally.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_resource_scope(
    world: *mut PonkoWorld,
    name: *const c_char,
    size: usize,
    scope: PonkoResourceScopeFn,
    user: *mut c_void,
) -> PonkoStatus {
    boundary(|| {
        let name = resource_name(name)?;
        let scope = scope.ok_or_else(|| invalid("scope callback is null"))?;
        // Take the resource out, under the world's busy flag.
        // SAFETY: access validates null and guards the live handle.
        let mut taken = unsafe {
            access(world, |w, _| {
                let layout = w
                    .raw_ref()
                    .resource_layout_dynamic(name)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "resource not found"))?;
                if layout.size() != size {
                    return Err(invalid("resource size does not match"));
                }
                let mut taken = Taken::new(layout);
                let from = w.raw_ref().get_resource_dynamic(name).expect("layout implies presence");
                // SAFETY: both hold `size` bytes; the copy is separate memory.
                std::ptr::copy_nonoverlapping(from, taken.ptr(), size);
                w.raw().remove_resource_dynamic(name);
                Ok(taken)
            })?
        };
        // The world is free while the scope runs, as in Rust.
        // SAFETY: the caller's callback takes these arguments and returns normally.
        unsafe { scope(world, taken.ptr().cast(), user) };
        // SAFETY: as above; the world outlives the scope (it must not free it).
        unsafe {
            access(world, |w, _| {
                w.raw().remove_resource_dynamic(name);
                let (layout, value) = (taken.layout, taken.ptr());
                // SAFETY: `value` holds a value of `layout`, aligned for it.
                w.raw().insert_resource_dynamic(name, layout, value);
                Ok(())
            })
        }
    })
}
