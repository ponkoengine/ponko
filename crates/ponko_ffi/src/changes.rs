//! Change detection from C: which components were added, changed or removed.
//!
//! Tracking is off for every component until `ponko_component_track_changes`
//! (or a `changed`/`added` filter) asks for it. A reader keeps a tick and
//! asks what happened after it; in a game, each system's tick is kept for it
//! (`PONKO_SINCE_LAST_RUN`).
use std::ffi::c_void;

use ponko_ecs::{ChunkQuery, Entity, Tick};

use crate::filters::{PonkoFilter, compile};
use crate::{
    Failure, ParallelQuery, PonkoChunk, PonkoQueryFn, PonkoStatus, PonkoWorld, access, boundary, chunk_in_parallel,
    error, invalid, query_input, resolve,
};

/// A `since` value: what happened since the calling system last ran (the
/// world's changes-since tick, `ponko_world_changes_since`). Outside
/// systems that tick is 0 unless set: everything.
pub const PONKO_SINCE_LAST_RUN: u64 = u64::MAX;

/// How `ponko_query_with` runs a query. Start from
/// `ponko_query_options_default()`. Bit `i` of each mask is about the
/// `i`-th component of the query.
/// ```
/// let options = ponko_ffi::ponko_query_options_default();
/// assert_eq!((options.writes, options.changed, options.added), (u32::MAX, 0, 0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoQueryOptions {
    /// Columns the callback may write; their rows count as changed, written
    /// or not. The other columns must only be read. Default: all.
    pub writes: u32,
    /// Only visit rows where these columns changed after `since`.
    pub changed: u32,
    /// Only visit rows where these columns were added after `since`. At
    /// most 8 `changed` and `added` bits together.
    pub added: u32,
    /// `false`: a row must pass every `changed` and `added` bit; `true`:
    /// one is enough.
    pub any: bool,
    /// Run the callback on all CPU cores at once, as `ponko_query_par` does.
    pub parallel: bool,
    /// The reader's tick: `PONKO_SINCE_LAST_RUN` (the default), or a tick
    /// from `ponko_world_advance_change_tick`. 0 is before everything.
    pub since: u64,
}

/// The default query options: every column may be written, every row is
/// visited, on the calling thread.
/// ```
/// assert!(!ponko_ffi::ponko_query_options_default().parallel);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_query_options_default() -> PonkoQueryOptions {
    PonkoQueryOptions {
        writes: u32::MAX,
        changed: 0,
        added: 0,
        any: false,
        parallel: false,
        since: PONKO_SINCE_LAST_RUN,
    }
}

fn since(raw: u64) -> Option<Tick> {
    (raw != PONKO_SINCE_LAST_RUN).then(|| Tick::from_bits(raw))
}

fn component_id(w: &ponko_ecs::World, id: u32) -> Result<ponko_ecs::ComponentId, Failure> {
    w.component_at(id as usize)
        .ok_or_else(|| invalid("component id is not registered in this world"))
}

fn output<T>(out: *mut T) -> Result<*mut T, Failure> {
    if out.is_null() {
        Err(invalid("output is null"))
    } else {
        Ok(out)
    }
}

/// Starts recording when values of component `id` are added, changed and
/// removed. Until then a component keeps no change data and costs nothing.
/// Values that exist already count as added and changed now. Tracking stays
/// on; calling this again does nothing.
///
/// What counts as a change: `ponko_spawn` and `ponko_set` (added too when
/// the entity had no such component), every column of `ponko_query` and
/// `ponko_query_par` (they may be written), the `writes` columns of
/// `ponko_query_with`, and `ponko_mark_changed`. `ponko_get` doesn't.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut id = 0;
///     ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut id);
///     assert_eq!(ponko_component_track_changes(w, id), PonkoStatus::PONKO_OK);
///     assert_eq!(ponko_component_track_changes(w, 99), PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_component_track_changes(world: *mut PonkoWorld, id: u32) -> PonkoStatus {
    // SAFETY: caller supplies a live handle; the id is checked.
    boundary(|| unsafe {
        access(world, |w, _| {
            let id = component_id(w, id)?;
            w.raw().track_changes_dynamic(id);
            Ok(())
        })
    })
}

/// The world's current change tick: changes made now are stamped with it.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut tick = 0;
///     assert_eq!(ponko_world_change_tick(w, &mut tick), PonkoStatus::PONKO_OK);
///     assert_eq!(tick, 1);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world and valid writable output, or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_change_tick(world: *mut PonkoWorld, out_tick: *mut u64) -> PonkoStatus {
    boundary(|| {
        let out = output(out_tick)?;
        // SAFETY: caller provides writable output and a live handle.
        unsafe {
            access(world, |w, _| {
                *out = w.change_tick().to_bits();
                Ok(())
            })
        }
    })
}

/// Ends the current change tick and writes it to `out_tick`; later changes
/// get a newer one. A reader that isn't a game system keeps its place this
/// way: look (passing its last tick as `since`), then keep this tick for
/// next time. Game systems don't need it: the game advances the tick after
/// every system.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let (mut ended, mut now) = (0, 0);
///     assert_eq!(ponko_world_advance_change_tick(w, &mut ended), PonkoStatus::PONKO_OK);
///     ponko_world_change_tick(w, &mut now);
///     assert_eq!((ended, now), (1, 2));
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world and valid writable output, or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_advance_change_tick(world: *mut PonkoWorld, out_tick: *mut u64) -> PonkoStatus {
    boundary(|| {
        let out = output(out_tick)?;
        // SAFETY: caller provides writable output and a live handle.
        unsafe {
            access(world, |w, _| {
                *out = w.advance_change_tick().to_bits();
                Ok(())
            })
        }
    })
}

/// The tick `PONKO_SINCE_LAST_RUN` stands for. Inside a game system: the
/// tick that system last finished at. Otherwise 0 (everything) unless set
/// with `ponko_world_set_changes_since`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut tick = 9;
///     assert_eq!(ponko_world_changes_since(w, &mut tick), PonkoStatus::PONKO_OK);
///     assert_eq!(tick, 0);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world and valid writable output, or null pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_changes_since(world: *mut PonkoWorld, out_tick: *mut u64) -> PonkoStatus {
    boundary(|| {
        let out = output(out_tick)?;
        // SAFETY: caller provides writable output and a live handle.
        unsafe {
            access(world, |w, _| {
                *out = w.changes_since().to_bits();
                Ok(())
            })
        }
    })
}

/// Sets the tick `PONKO_SINCE_LAST_RUN` stands for. A game sets it before
/// each system, so only code that drives a world itself needs this.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     assert_eq!(ponko_world_set_changes_since(w, 5), PonkoStatus::PONKO_OK);
///     let mut tick = 0;
///     ponko_world_changes_since(w, &mut tick);
///     assert_eq!(tick, 5);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_set_changes_since(world: *mut PonkoWorld, tick: u64) -> PonkoStatus {
    // SAFETY: caller supplies a live handle.
    boundary(|| unsafe {
        access(world, |w, _| {
            w.set_changes_since(Tick::from_bits(tick));
            Ok(())
        })
    })
}

/// Marks component `id` of `entity` as changed now, for a write the world
/// can't see. `PONKO_NOT_FOUND` if the entity is dead or has no such
/// component. Does nothing for a component that isn't tracked.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     assert_eq!(ponko_mark_changed(w, 42, 0), PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// `world` must be null or a live handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_mark_changed(world: *mut PonkoWorld, entity: u64, id: u32) -> PonkoStatus {
    // SAFETY: caller supplies a live handle; the id is checked.
    boundary(|| unsafe {
        access(world, |w, _| {
            let id = component_id(w, id)?;
            if w.raw().mark_changed_dynamic(Entity::from_bits(entity), id) {
                Ok(())
            } else {
                Err(error(PonkoStatus::PONKO_NOT_FOUND, "entity or component not found"))
            }
        })
    })
}

/// Lists the entities that lost tracked component `id` after `since` (it
/// was removed, or the entity despawned), oldest first: writes how many
/// there are to `out_count`, and up to `capacity` of them to
/// `out_entities`. Call it with capacity 0 to size a buffer.
///
/// An entity is listed once per removal, and may have the component again
/// by now. In a game, removals are kept until every system has run past them.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let (mut id, mut e, mut n) = (0, 0, 9);
///     ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut id);
///     // Not tracked yet: removals of it aren't recorded.
///     let status = ponko_removed(w, id, 0, std::ptr::null_mut(), 0, &mut n);
///     assert_eq!(status, PonkoStatus::PONKO_INVALID_ARGUMENT);
///     ponko_component_track_changes(w, id);
///     let hp = 5u32;
///     let value = PonkoValue { component: id, data: (&raw const hp).cast(), size: 4 };
///     ponko_spawn(w, &value, 1, &mut e);
///     ponko_despawn(w, e);
///     let mut gone = [0u64; 4];
///     assert_eq!(ponko_removed(w, id, 0, gone.as_mut_ptr(), 4, &mut n), PonkoStatus::PONKO_OK);
///     assert_eq!((n, gone[0]), (1, e));
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world; `out_entities` must hold `capacity` writable ids (it may be
/// null when `capacity` is 0); `out_count` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_removed(
    world: *mut PonkoWorld,
    id: u32,
    since: u64,
    out_entities: *mut u64,
    capacity: usize,
    out_count: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        let out_count = output(out_count)?;
        if out_entities.is_null() && capacity != 0 {
            return Err(invalid("entity buffer is null"));
        }
        // SAFETY: caller provides a live handle and writable outputs.
        unsafe {
            access(world, |w, _| {
                let id = component_id(w, id)?;
                if !w.component_info(id).tracks_changes() {
                    return Err(invalid(
                        "removals of this component aren't tracked: call ponko_component_track_changes first",
                    ));
                }
                let since = since_or_world(w, since);
                let removed = w.raw_ref().removed_dynamic(id, since);
                for (i, e) in removed.iter().take(capacity).enumerate() {
                    out_entities.add(i).write(e.to_bits());
                }
                *out_count = removed.len();
                Ok(())
            })
        }
    })
}

/// `RawWorld::query_chunks`.
///
/// A `changed` or `added` filter starts tracking its component here: that
/// is what the C API promises, while the Rust API asks for tracking first.
fn visit(w: &mut ponko_ecs::World, query: ChunkQuery<'_>, filtered: u32, f: impl FnMut(ponko_ecs::Chunk<'_>)) {
    for (i, &id) in query.ids().iter().enumerate() {
        if filtered >> i & 1 != 0 {
            w.raw().track_changes_dynamic(id);
        }
    }
    w.raw().query_chunks(query, f);
}

fn since_or_world(w: &ponko_ecs::World, raw: u64) -> Tick {
    since(raw).unwrap_or(w.changes_since())
}

/// `ponko_query` with options: which columns the callback writes (only
/// those count as changed), which rows it wants (`changed` / `added` since a
/// tick), and whether to run on all cores. Null options are the defaults,
/// which make it `ponko_query`.
///
/// With a `changed` or `added` filter the callback gets runs of consecutive
/// rows that pass, instead of whole archetypes: `count` rows, columns
/// starting at the run's first row, and `first` counting the rows visited
/// before it. A filter starts tracking its component.
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
///     let (mut id, mut e, mut tick) = (0, 0, 0);
///     ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut id);
///     ponko_component_track_changes(w, id);
///     let hp = 5u32;
///     let value = PonkoValue { component: id, data: (&raw const hp).cast(), size: 4 };
///     ponko_spawn(w, &value, 1, &mut e);
///     ponko_spawn(w, &value, 1, &mut e);
///     ponko_world_advance_change_tick(w, &mut tick);
///     ponko_mark_changed(w, e, id);
///
///     // Read-only, and only the rows that changed after `tick`.
///     let mut options = ponko_query_options_default();
///     options.writes = 0;
///     options.changed = 1;
///     options.since = tick;
///     let mut rows = 0usize;
///     let status = ponko_query_with(w, &id, 1, &options, Some(count), (&raw mut rows).cast());
///     assert_eq!((status, rows), (PonkoStatus::PONKO_OK, 1));
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// As `ponko_query`; `options` must be null or readable; with
/// `options->parallel`, as `ponko_query_par`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_query_with(
    world: *mut PonkoWorld,
    components: *const u32,
    count: usize,
    options: *const PonkoQueryOptions,
    callback: PonkoQueryFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    unsafe {
        query_with_filter(
            world,
            components,
            count,
            None,
            options,
            ponko_ecs::QueryCache::Auto,
            callback,
            user,
        )
    }
}

/// `ponko_query_with`, over the entities that also pass `filter`, using
/// the world's archetype index as `cache` says.
///
/// # Safety
/// As `ponko_query_with`.
#[allow(clippy::too_many_arguments)]
pub(crate) unsafe fn query_with_filter(
    world: *mut PonkoWorld,
    components: *const u32,
    count: usize,
    filter: Option<&PonkoFilter>,
    options: *const PonkoQueryOptions,
    cache: ponko_ecs::QueryCache,
    callback: PonkoQueryFn,
    user: *mut c_void,
) -> PonkoStatus {
    boundary(|| {
        let input = query_input(components, count)?;
        let callback = callback.ok_or_else(|| invalid("query callback is null"))?;
        let options = if options.is_null() {
            ponko_query_options_default()
        } else {
            // SAFETY: the caller supplies readable options.
            unsafe { *options }
        };
        let used = if count == 32 { u32::MAX } else { (1 << count) - 1 };
        if (options.changed & used).count_ones() + (options.added & used).count_ones() > 8 {
            return Err(invalid("a query takes at most 8 changed and added filters"));
        }
        // SAFETY: caller supplies live handle, input ids and a callable callback.
        unsafe {
            access(world, |w, handle| {
                let (resolved, sizes) = resolve(w, input)?;
                let compiled = filter.map(|filter| compile(w, filter)).transpose()?;
                let query = ChunkQuery::new(&resolved[..count])
                    .writes(options.writes)
                    .changed(options.changed)
                    .added(options.added)
                    .match_any(options.any)
                    .since(since(options.since))
                    .filter(compiled.as_ref())
                    .cache(cache);
                let filtered = options.changed | options.added;
                let mut first = 0;
                if options.parallel {
                    let _commands_off = ParallelQuery::begin(handle);
                    let threads = rayon::current_num_threads().max(1);
                    visit(w, query, filtered, |chunk| {
                        chunk_in_parallel(&chunk, &sizes, first, threads, callback, user);
                        first += chunk.entities.len();
                    });
                } else {
                    visit(w, query, filtered, |chunk| {
                        let view = PonkoChunk {
                            count: chunk.entities.len(),
                            column_count: count,
                            columns: chunk.columns.as_ptr().cast(),
                            entities: chunk.entities.as_ptr().cast(),
                            first,
                        };
                        first += view.count;
                        callback(&view, user);
                    });
                }
                Ok(())
            })
        }
    })
}
