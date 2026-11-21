//! The archetype index for repeated queries, from C: the per-query hint
//! (`ponko_query_cached`), the world's policy with its off switch
//! (`ponko_world_set_query_cache_policy`) and what the index holds
//! (`ponko_world_query_cache_stats`).
//!
//! C queries use the index without any of this: a world with many
//! archetypes indexes the queries that repeat by itself, whichever language
//! runs them. The entities a query visits, and their order, never depend on
//! the index.
//!
//! Rust: `Query::cached` / `Query::uncached`, `World::set_query_cache_policy`
//! and `World::query_cache_stats`.
use std::ffi::c_void;

use ponko_ecs::{QueryCache, QueryCachePolicy};

use crate::changes::query_with_filter;
use crate::{PonkoFilter, PonkoQueryFn, PonkoQueryOptions, PonkoStatus, PonkoWorld, access, boundary, invalid};

/// `ponko_query_cached`: the world's policy decides whether this query
/// uses the archetype index. What every other query function does.
pub const PONKO_QUERY_CACHE_AUTO: u32 = 0;
/// `ponko_query_cached`: index this query on its first use, however many
/// archetypes it matches.
pub const PONKO_QUERY_CACHE_ALWAYS: u32 = 1;
/// `ponko_query_cached`: test every archetype for this query.
pub const PONKO_QUERY_CACHE_NEVER: u32 = 2;

/// When a world indexes the archetypes of repeated queries. Start from
/// `ponko_query_cache_policy_default()` or `ponko_query_cache_policy_off()`.
///
/// A query is indexed without a hint once the world has `min_archetypes`
/// archetypes, the same query (components and filters) ran `promote_after`
/// times, and it matches at most `max_match_percent` percent of the
/// archetypes; the index keeps at most `max_signatures` queries and
/// `max_index_bytes` bytes of archetype numbers.
/// ```
/// let policy = ponko_ffi::ponko_query_cache_policy_default();
/// assert!(policy.min_archetypes < usize::MAX);
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PonkoQueryCachePolicy {
    /// Fewest archetypes in the world for any query to use the index.
    /// `SIZE_MAX` turns the index off (`ponko_query_cache_policy_off()`).
    pub min_archetypes: usize,
    /// Most queries remembered; unused ones are dropped when it is reached.
    pub max_signatures: usize,
    /// Most bytes of archetype numbers kept, over all queries.
    pub max_index_bytes: usize,
    /// Runs of a query before it is indexed without a hint.
    pub promote_after: u32,
    /// Without a hint, a query is indexed only while it matches at most
    /// this share of the archetypes, in percent. A world keeps at most 100:
    /// anything above is lowered to it.
    pub max_match_percent: u8,
    /// Room for later fields; leave it zero, as the two functions that make
    /// a policy do. Anything else is `PONKO_INVALID_ARGUMENT`.
    pub reserved: [u8; 19],
}

/// What a world's archetype index holds: see
/// `ponko_world_query_cache_stats`.
/// ```
/// let stats = ponko_ffi::PonkoQueryCacheStats::default();
/// assert_eq!((stats.signatures, stats.bytes), (0, 0));
/// ```
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PonkoQueryCacheStats {
    /// Queries remembered, indexed or not.
    pub signatures: usize,
    /// Queries that have an index.
    pub indexed: usize,
    /// Archetype numbers held, over all queries.
    pub indices: usize,
    /// Heap bytes the index owns.
    pub bytes: usize,
    /// Queries answered from an index so far.
    pub hits: u64,
    /// Queries dropped to make room so far.
    pub evictions: u64,
}

fn to_c(policy: QueryCachePolicy) -> PonkoQueryCachePolicy {
    PonkoQueryCachePolicy {
        min_archetypes: policy.min_archetypes,
        max_signatures: policy.max_signatures,
        max_index_bytes: policy.max_index_bytes,
        promote_after: policy.promote_after,
        max_match_percent: policy.max_match_percent,
        reserved: [0; 19],
    }
}

/// The default policy of a world.
/// ```
/// let policy = ponko_ffi::ponko_query_cache_policy_default();
/// assert_eq!((policy.promote_after, policy.max_match_percent), (2, 50));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_query_cache_policy_default() -> PonkoQueryCachePolicy {
    to_c(QueryCachePolicy::new())
}

/// The policy that turns the archetype index off: every query tests every
/// archetype, hinted or not, and the world keeps no index memory.
/// ```
/// let policy = ponko_ffi::ponko_query_cache_policy_off();
/// assert_eq!(policy.min_archetypes, usize::MAX);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_query_cache_policy_off() -> PonkoQueryCachePolicy {
    to_c(QueryCachePolicy::off())
}

/// Sets when `world` indexes the archetypes of repeated queries, and
/// forgets what was indexed so far. `ponko_query_cache_policy_off()` opts
/// the whole world out. Query results never depend on the policy. A
/// `max_match_percent` above 100 is stored as 100, as in Rust.
///
/// Rust: `World::set_query_cache_policy`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let off = ponko_query_cache_policy_off();
///     assert_eq!(ponko_world_set_query_cache_policy(w, &off), PonkoStatus::PONKO_OK);
///     let mut now = ponko_query_cache_policy_default();
///     assert_eq!(ponko_world_query_cache_policy(w, &mut now), PonkoStatus::PONKO_OK);
///     assert_eq!(now, off);
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world; `policy` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_set_query_cache_policy(
    world: *mut PonkoWorld,
    policy: *const PonkoQueryCachePolicy,
) -> PonkoStatus {
    boundary(|| {
        if policy.is_null() {
            return Err(invalid("policy is null"));
        }
        // SAFETY: the caller supplies a readable policy.
        let from = unsafe { *policy };
        if from.reserved != [0; 19] {
            return Err(invalid("reserved must be zero"));
        }
        let mut policy = QueryCachePolicy::new();
        policy.min_archetypes = from.min_archetypes;
        policy.max_signatures = from.max_signatures;
        policy.max_index_bytes = from.max_index_bytes;
        policy.promote_after = from.promote_after;
        policy.max_match_percent = from.max_match_percent;
        // SAFETY: the caller supplies a live handle.
        unsafe {
            access(world, |w, _| {
                w.set_query_cache_policy(policy);
                Ok(())
            })
        }
    })
}

/// The policy `world` uses: the default one, or the one set last.
///
/// Rust: `World::query_cache_policy`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut policy = ponko_query_cache_policy_off();
///     assert_eq!(ponko_world_query_cache_policy(w, &mut policy), PonkoStatus::PONKO_OK);
///     assert_eq!(policy, ponko_query_cache_policy_default());
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world; `out_policy` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_query_cache_policy(
    world: *mut PonkoWorld,
    out_policy: *mut PonkoQueryCachePolicy,
) -> PonkoStatus {
    boundary(|| {
        if out_policy.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: the caller supplies a live handle and a writable output.
        unsafe {
            access(world, |w, _| {
                out_policy.write(to_c(w.query_cache_policy()));
                Ok(())
            })
        }
    })
}

/// What the archetype index of `world` holds now: how many queries it
/// remembers and has indexed, its memory, and how often it was used.
///
/// Rust: `World::query_cache_stats`.
/// ```
/// use ponko_ffi::*;
/// let w = ponko_world_new();
/// unsafe {
///     let mut stats = PonkoQueryCacheStats { signatures: 9, ..Default::default() };
///     assert_eq!(ponko_world_query_cache_stats(w, &mut stats), PonkoStatus::PONKO_OK);
///     assert_eq!(stats, PonkoQueryCacheStats::default());
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// Live world; `out_stats` must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_world_query_cache_stats(
    world: *mut PonkoWorld,
    out_stats: *mut PonkoQueryCacheStats,
) -> PonkoStatus {
    boundary(|| {
        if out_stats.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: the caller supplies a live handle and a writable output.
        unsafe {
            access(world, |w, _| {
                let stats = w.query_cache_stats();
                out_stats.write(PonkoQueryCacheStats {
                    signatures: stats.signatures,
                    indexed: stats.indexed,
                    indices: stats.indices,
                    bytes: stats.bytes,
                    hits: stats.hits,
                    evictions: stats.evictions,
                });
                Ok(())
            })
        }
    })
}

/// `ponko_query_filtered` (`filter` and `options` may be null), saying
/// whether this query uses the world's archetype index: `cache` is
/// `PONKO_QUERY_CACHE_AUTO` (the world's policy decides, as in every other
/// query function), `PONKO_QUERY_CACHE_ALWAYS` (index it now) or
/// `PONKO_QUERY_CACHE_NEVER` (test every archetype). The chunks are the
/// same for all three; any other value is `PONKO_INVALID_ARGUMENT`.
///
/// Rust: `query.cached()` and `query.uncached()`.
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
///     let (mut id, mut e, hp) = (0, 0, 5u32);
///     ponko_component_register(w, c"Health".as_ptr(), 4, 4, &mut id);
///     let value = PonkoValue { component: id, data: (&raw const hp).cast(), size: 4 };
///     ponko_spawn(w, &value, 1, &mut e);
///     for cache in [PONKO_QUERY_CACHE_AUTO, PONKO_QUERY_CACHE_ALWAYS, PONKO_QUERY_CACHE_NEVER] {
///         let mut rows = 0usize;
///         let status = ponko_query_cached(
///             w, &id, 1, std::ptr::null(), std::ptr::null(), cache, Some(count), (&raw mut rows).cast(),
///         );
///         assert_eq!((status, rows), (PonkoStatus::PONKO_OK, 1));
///     }
///     ponko_world_free(w);
/// }
/// ```
/// # Safety
/// As `ponko_query_filtered`.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_query_cached(
    world: *mut PonkoWorld,
    components: *const u32,
    count: usize,
    filter: *const PonkoFilter,
    options: *const PonkoQueryOptions,
    cache: u32,
    callback: PonkoQueryFn,
    user: *mut c_void,
) -> PonkoStatus {
    let cache = match cache {
        PONKO_QUERY_CACHE_AUTO => QueryCache::Auto,
        PONKO_QUERY_CACHE_ALWAYS => QueryCache::Always,
        PONKO_QUERY_CACHE_NEVER => QueryCache::Never,
        _ => {
            return boundary(|| {
                Err(invalid(
                    "cache is PONKO_QUERY_CACHE_AUTO, PONKO_QUERY_CACHE_ALWAYS or PONKO_QUERY_CACHE_NEVER",
                ))
            });
        }
    };
    // SAFETY: forwarded caller contract; the filter is null or live.
    unsafe {
        query_with_filter(
            world,
            components,
            count,
            filter.as_ref(),
            options,
            cache,
            callback,
            user,
        )
    }
}
