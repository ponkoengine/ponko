//! Query filters from C: which entities a query visits by the components
//! they have or lack, with OR over alternatives and AND groups inside an OR.
//! The runtime form of the Rust `With`, `Without` and `Or` filters and
//! tuples of them (`query.filter::<F>()`): a `PonkoFilter` is built from
//! component ids, and `ponko_query_filtered` runs a query with it.
//!
//! The entities visited are the ones the Rust query with the same filter
//! visits, and the limits are the same: at most 8 different `with` and 8
//! different `without` components at the top, and an `any` expands to ANDed
//! clauses (4 at most, with 16 component predicates between them).
use std::ffi::c_void;

use ponko_ecs::{FilterError, FilterShape, FilterTree, MAX_FILTER_DEPTH, World, compile_tree};

use crate::changes::query_with_filter;
use crate::{
    Failure, PonkoQueryFn, PonkoQueryOptions, PonkoStatus, PonkoWorld, access, boundary, invalid, invalid_owned,
};

/// A query filter built from component ids; see `ponko_filter_with`,
/// `ponko_filter_without`, `ponko_filter_all` and `ponko_filter_any`. A
/// filter is a value that doesn't depend on a world: build it once, run it
/// in any number of queries (`ponko_query_filtered`), free it with
/// `ponko_filter_free`. Building one from other filters copies them, so
/// those can be freed at once.
/// ```
/// use ponko_ffi::*;
/// unsafe {
///     let filter = ponko_filter_with(1);
///     assert!(!filter.is_null());
///     ponko_filter_free(filter);
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PonkoFilter(Node);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    With(u32),
    Without(u32),
    All(Vec<Node>),
    Any(Vec<Node>),
}

impl Node {
    /// How deep the tree nests; an empty group is a leaf, as for
    /// `DynFilter`.
    fn depth(&self) -> usize {
        match self {
            Node::With(_) | Node::Without(_) => 0,
            Node::All(parts) | Node::Any(parts) if parts.is_empty() => 0,
            Node::All(parts) | Node::Any(parts) => 1 + parts.iter().map(Node::depth).max().unwrap_or(0),
        }
    }
}

impl FilterTree for Node {
    fn shape(&self) -> FilterShape<'_, Self> {
        match self {
            Node::With(id) => FilterShape::With(*id),
            Node::Without(id) => FilterShape::Without(*id),
            Node::All(parts) => FilterShape::All(parts),
            Node::Any(parts) => FilterShape::Or(parts),
        }
    }
}

fn limit(error: FilterError) -> Failure {
    match error {
        FilterError::UnknownComponent(_) => invalid("a filter's component id is not registered in this world"),
        other => invalid_owned(other.to_string()),
    }
}

/// The runtime filter of `filter` in `world`, compiled: unknown component
/// ids and the filter limits are errors. Allocates nothing.
pub(crate) fn compile(world: &World, filter: &PonkoFilter) -> Result<ponko_ecs::CompiledFilter, Failure> {
    compile_tree(&filter.0, |raw| world.component_at(raw as usize)).map_err(limit)
}

fn make(node: Node) -> Result<*mut PonkoFilter, Failure> {
    if node.depth() > MAX_FILTER_DEPTH {
        return Err(invalid("filters nest at most 16 levels deep"));
    }
    Ok(Box::into_raw(Box::new(PonkoFilter(node))))
}

/// Builds a filter from a node, or null (see `ponko_last_error`).
fn built(node: impl FnOnce() -> Result<Node, Failure>) -> *mut PonkoFilter {
    let mut result = std::ptr::null_mut();
    boundary(|| {
        result = make(node()?)?;
        Ok(())
    });
    result
}

/// Copies the filters `parts` points to.
unsafe fn copy_parts(parts: *const *const PonkoFilter, count: usize) -> Result<Vec<Node>, Failure> {
    if count == 0 {
        return Err(invalid(
            "a filter needs at least one part: ponko_filter_always matches everything, ponko_filter_never nothing",
        ));
    }
    if parts.is_null() {
        return Err(invalid("filter parts are null"));
    }
    // SAFETY: the caller supplies `count` readable pointers.
    let parts = unsafe { std::slice::from_raw_parts(parts, count) };
    parts
        .iter()
        .map(|&part| {
            // SAFETY: each is null or a live filter.
            unsafe { part.as_ref() }
                .map(|part| part.0.clone())
                .ok_or_else(|| invalid("a filter part is null"))
        })
        .collect()
}

/// A filter that holds for entities that have component `component`. Null
/// on failure. Free it with `ponko_filter_free`. The id is looked up when a
/// query uses the filter.
///
/// Rust: `With<T>`.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free.
///
/// ```
/// unsafe { ponko_ffi::ponko_filter_free(ponko_ffi::ponko_filter_with(7)) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_filter_with(component: u32) -> *mut PonkoFilter {
    built(|| Ok(Node::With(component)))
}

/// A filter that holds for entities that don't have component `component`.
/// Null on failure.
///
/// Rust: `Without<T>`.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free.
///
/// ```
/// unsafe { ponko_ffi::ponko_filter_free(ponko_ffi::ponko_filter_without(7)) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_filter_without(component: u32) -> *mut PonkoFilter {
    built(|| Ok(Node::Without(component)))
}

/// A filter that holds for every entity: no condition. Null on failure.
///
/// Rust: `()`, the empty tuple.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free.
///
/// ```
/// unsafe { ponko_ffi::ponko_filter_free(ponko_ffi::ponko_filter_always()) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_filter_always() -> *mut PonkoFilter {
    built(|| Ok(Node::All(Vec::new())))
}

/// A filter that holds for no entity: an `any` of nothing. Null on failure.
///
/// Rust: `Or<()>`.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free.
///
/// ```
/// unsafe { ponko_ffi::ponko_filter_free(ponko_ffi::ponko_filter_never()) };
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_filter_never() -> *mut PonkoFilter {
    built(|| Ok(Node::Any(Vec::new())))
}

/// A filter that holds when every one of the `count` filters in `parts`
/// holds: an AND group, which inside `ponko_filter_any` is one arm. The
/// parts are copied. Null, with the reason in `ponko_last_error`, for
/// `count` 0 (use `ponko_filter_always`), a null part, or groups nested
/// more than 16 deep (`always` and `never` are leaves, not levels). A
/// component named twice is one slot, and `with` and `without` of one
/// component match nothing, as for the Rust tuple.
///
/// Rust: a tuple of filters, `(A, B)`.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free. The parts stay the caller's (they are copied).
///
/// ```
/// use ponko_ffi::*;
/// unsafe {
///     let (a, b) = (ponko_filter_with(1), ponko_filter_without(2));
///     let both = ponko_filter_all([a.cast_const(), b.cast_const()].as_ptr(), 2);
///     assert!(!both.is_null());
///     assert!(ponko_filter_all(std::ptr::null(), 0).is_null());
///     for f in [a, b, both] {
///         ponko_filter_free(f);
///     }
/// }
/// ```
/// # Safety
/// `parts` points to `count` filters from this library (or is null with
/// `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_filter_all(parts: *const *const PonkoFilter, count: usize) -> *mut PonkoFilter {
    built(|| {
        // SAFETY: forwarded caller contract.
        Ok(Node::All(unsafe { copy_parts(parts, count) }?))
    })
}

/// A filter that holds when at least one of the `count` filters in `parts`
/// holds. Each part is an arm: a `with`, a `without`, an AND group
/// (`ponko_filter_all`) or another `any`. The parts are copied. Null, with
/// the reason in `ponko_last_error`, for `count` 0 (use `ponko_filter_never`),
/// a null part, or groups nested more than 16 deep.
///
/// A query takes at most 4 clauses, with 16 component predicates between
/// them: `any` of ANDs is expanded, so `(a AND b) OR c` is two clauses
/// (`(a OR c) AND (b OR c)`) of two predicates each. `ponko_query_filtered`
/// and `ponko_filter_check` report a filter over the limits.
///
/// Rust: `Or<(A, B)>`.
///
/// Ownership: the returned filter is the caller's (null on failure); free it
/// with ponko_filter_free. The parts stay the caller's (they are copied).
///
/// ```
/// use ponko_ffi::*;
/// unsafe {
///     let (a, b) = (ponko_filter_with(1), ponko_filter_with(2));
///     let either = ponko_filter_any([a.cast_const(), b.cast_const()].as_ptr(), 2);
///     assert!(!either.is_null());
///     assert!(ponko_filter_any(std::ptr::null(), 0).is_null());
///     for f in [a, b, either] {
///         ponko_filter_free(f);
///     }
/// }
/// ```
/// # Safety
/// `parts` points to `count` filters from this library (or is null with
/// `count` 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_filter_any(parts: *const *const PonkoFilter, count: usize) -> *mut PonkoFilter {
    // SAFETY: forwarded caller contract.
    built(|| Ok(Node::Any(unsafe { copy_parts(parts, count) }?)))
}

/// Frees a filter; null is ignored. Filters built from it are unaffected.
/// ```
/// unsafe { ponko_ffi::ponko_filter_free(std::ptr::null_mut()) };
/// ```
/// # Safety
/// Null, or an owned filter not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_filter_free(filter: *mut PonkoFilter) {
    if !filter.is_null() {
        // SAFETY: the caller transfers the owned filter exactly once.
        drop(unsafe { Box::from_raw(filter) });
    }
}

/// Checks that a query could use `filter` in this world: every component id
/// is registered, and the filter is within the limits (see
/// `ponko_filter_any`). `PONKO_INVALID_ARGUMENT` otherwise, with the reason
/// in `ponko_last_error`. `ponko_query_filtered` makes the same check.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let unknown = ponko_filter_with(99);
///     assert_eq!(ponko_filter_check(w, unknown), PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_filter_free(unknown);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` is a live handle; `filter` a live filter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_filter_check(world: *mut PonkoWorld, filter: *const PonkoFilter) -> PonkoStatus {
    boundary(|| {
        // SAFETY: the caller supplies a live filter or null.
        let filter = unsafe { filter.as_ref() }.ok_or_else(|| invalid("filter is null"))?;
        // SAFETY: the caller supplies a live handle.
        unsafe { access(world, |w, _| compile(w, filter).map(drop)) }
    })
}

/// `ponko_query_with` over the entities that also pass `filter`: it needs
/// no column, so the components it names can be left out of `components`.
/// A null `filter` is no filter, null `options` the defaults (see
/// `ponko_query_with`; `options->parallel` runs on all cores, as
/// `ponko_query_par`). Nothing runs, and the status says why, for a
/// component id of the filter that this world hasn't registered, or a filter
/// over the limits (see `ponko_filter_any`).
///
/// Rust: `world.query::<(..)>().filter::<F>()`.
///
/// Ownership: `user` stays the caller's and is only passed to `callback` during
/// this call; the engine keeps no pointer to it after returning.
///
/// ```
/// use ponko_ffi::*;
/// unsafe extern "C" fn count(chunk: *const PonkoChunk, user: *mut std::ffi::c_void) {
///     unsafe { *user.cast::<usize>() += (*chunk).count };
/// }
/// let w = ponko_world_new();
/// unsafe {
///     let (mut a, mut b, mut e, zero) = (0, 0, 0, 0u32);
///     ponko_component_register(w, c"A".as_ptr(), 4, 4, &mut a);
///     ponko_component_register(w, c"B".as_ptr(), 4, 4, &mut b);
///     let value = |id| PonkoValue { component: id, data: (&raw const zero).cast(), size: 4 };
///     ponko_spawn(w, &value(a), 1, &mut e);
///     ponko_spawn(w, &value(b), 1, &mut e);
///     ponko_spawn(w, [value(a), value(b)].as_ptr(), 2, &mut e);
///
///     // `a` or `b`, but not both: `Or<((With<A>, Without<B>), (Without<A>, With<B>))>`.
///     let (with_a, no_a) = (ponko_filter_with(a), ponko_filter_without(a));
///     let (with_b, no_b) = (ponko_filter_with(b), ponko_filter_without(b));
///     let a_only = ponko_filter_all([with_a.cast_const(), no_b.cast_const()].as_ptr(), 2);
///     let b_only = ponko_filter_all([no_a.cast_const(), with_b.cast_const()].as_ptr(), 2);
///     let xor = ponko_filter_any([a_only.cast_const(), b_only.cast_const()].as_ptr(), 2);
///
///     let mut rows = 0usize;
///     let status = ponko_query_filtered(w, std::ptr::null(), 0, xor, std::ptr::null(), Some(count), (&raw mut rows).cast());
///     assert_eq!((status, rows), (PonkoStatus::PONKO_OK, 2));
///     for f in [with_a, no_a, with_b, no_b, a_only, b_only, xor] {
///         ponko_filter_free(f);
///     }
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// As `ponko_query_with`; `filter` is null or a live filter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_query_filtered(
    world: *mut PonkoWorld,
    components: *const u32,
    count: usize,
    filter: *const PonkoFilter,
    options: *const PonkoQueryOptions,
    callback: PonkoQueryFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract; the filter is null or live.
    unsafe {
        query_with_filter(
            world,
            components,
            count,
            filter.as_ref(),
            options,
            ponko_ecs::QueryCache::Auto,
            callback,
            user,
        )
    }
}
