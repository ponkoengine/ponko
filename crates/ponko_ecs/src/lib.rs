//! # ponko_ecs
//!
//! The entity-component-system behind Ponko, built for simulations
//! with millions of entities.
//!
//! - **Entities** are cheap handles ([`Entity`]).
//! - **Components** are plain Rust types that opt in with
//!   `#[derive(Component)]` (or `impl Component for Type {}`).
//! - **Queries** loop over every entity that has a set of components, as fast
//!   as a hand-written loop over arrays.
//! - **Resources** are global values stored in the world.
//! - **Commands** record changes to apply after a query finishes.
//!
//! ```
//! use ponko_ecs::*;
//!
//! #[derive(Component)]
//! struct Pos(f32, f32);
//! #[derive(Component)]
//! struct Vel(f32, f32);
//! struct Gravity(f32); // a resource: no derive
//!
//! let mut world = World::new();
//! world.insert_resource(Gravity(-9.8));
//! for i in 0..1000 {
//!     world.spawn((Pos(i as f32, 0.0), Vel(0.0, 0.0)));
//! }
//!
//! world
//!     .query::<&mut Vel>()
//!     .with_resources::<&Gravity>()
//!     .for_each(|v, gravity| v.1 += gravity.0);
//! world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
//!     p.0 += v.0;
//!     p.1 += v.1;
//! });
//! ```
//!
//! Under the hood every component, Rust or not, is a runtime-registered column
//! (name, size, alignment, drop). Rust types are registered on first use;
//! C/C++ code uses [`RawWorld::register_component`] and [`RawWorld::for_each_chunk`]
//! (see [`World::raw`]).
//!
//! Most games use this crate through [`ponko`](https://docs.rs/ponko/), whose
//! systems get the world as `ctx.world`; the guide's
//! [ECS chapter](https://ponko.dev/guide/ecs.html) explains it with examples.
//! The main types: [`World`] (entities, components, resources), the query
//! builder from [`World::query`], [`Commands`] for deferred changes, and
//! [`Component`] with its derive.
//!
//! # Component hooks
//!
//! Functions a world calls when a component arrives on an entity, is
//! replaced, or leaves it, registered per component type with
//! [`World::on_add`], [`World::on_replace`] and [`World::on_remove`]. A
//! hook runs inside the operation that caused it, so a lookup table kept
//! next to the world (an index by name, a registry of tagged entities) is
//! never out of date, not even between two systems.
//!
//! | Hook | Runs | Where the value is |
//! |---|---|---|
//! | [`on_add`](World::on_add) | after `T` arrived on an entity that didn't have one: `spawn` (every form), `insert` | in the world: [`HookWorld::get`] |
//! | [`on_replace`](World::on_replace) | before `insert` overwrites the entity's `T` | the old one in the world, the new one passed to the hook |
//! | [`on_remove`](World::on_remove) | before `T` leaves: `remove`, `despawn`, `clear`, `reset` | still in the world |
//!
//! ## What a hook may do
//!
//! A hook gets a [`HookWorld`]: the world without structural changes. It
//! reads and writes components and resources and runs queries, but it can't
//! spawn, despawn, insert or remove right away: those go to
//! [`HookWorld::commands`], which the world applies when the call that ran
//! the hook is done (a `spawn`, a `despawn`, a whole `apply`, `spawn_batch`
//! or `clear`), before that call returns. Commands that run more hooks are
//! applied after them, first recorded first, until none are left: a hook
//! that keeps recording commands that run it again never ends.
//!
//! So nothing a hook does can move or drop the entity it is called for
//! while the world is in the middle of changing it. Hooks never run on two
//! threads at once: structural changes need the world exclusively (`&mut
//! World`), which parallel queries and view systems don't have; their
//! commands are applied later, one buffer after another.
//!
//! ## Order
//!
//! Hooks run in an order that depends only on what the program did: those
//! of one operation in the order of the components' ids (the order this
//! world first saw each component type), several hooks of one component in
//! the order they were registered. A spawn runs `on_add` hooks after the
//! entity is complete; a despawn runs `on_remove` hooks while it still is.
//!
//! ## Cost
//!
//! Hooks are looked up when a structural change starts, never in queries.
//! A typed spawn checks nothing: the bundles whose archetype has `on_add`
//! hooks are marked where the world caches each bundle's archetype, and
//! every other bundle spawns through the same code as in a world without
//! hooks. A despawn, `insert` or `remove` checks one flag of the world;
//! once a hook is registered it also looks at one byte per archetype or
//! one entry per component, so components without hooks keep their speed.
//! A hook call costs a few nanoseconds besides the hook's own work.
//! Measurements: `crates/ponko_bench/HOOKS.md`.
//!
//! Dropping a world runs no hooks. Change detection is separate and still
//! works the same: hooks are for work that must happen at once, change
//! detection ([`World::track_changes`]) for work done once per tick over
//! everything that changed.

#![deny(missing_docs)]
// docs.rs: "Available on crate feature X only" on feature-gated items.
#![cfg_attr(docsrs, feature(doc_cfg))]

// `#[derive(Component)]` names the trait `::ponko_ecs::Component`, in this
// crate too (and in its doctests, where `crate` would be the doctest).
extern crate self as ponko_ecs;

mod raw;
pub use raw::{RawWorld, RawWorldRef};

/// Seals the traits this crate implements for its own types only
/// ([`QueryData`], [`ReadOnlyQueryData`], [`Bundle`], [`ResourceSet`],
/// [`ResourceTuple`]): each has a `SEAL` item of this type, and code outside
/// the crate can't write one because it can't name the type. Public but not
/// exported, like the `Filters` that seal the filter traits.
///
/// An impl that has everything but the seal doesn't compile:
///
/// ```compile_fail
/// use ponko_ecs::{ResourceTuple, World};
/// struct Mine;
/// impl ResourceTuple for Mine {
///     type Refs<'w> = ();
///     fn fetch(_: &mut World) -> Self::Refs<'_> {}
/// }
/// ```
///
/// and neither does one that tries to name it:
///
/// ```compile_fail
/// use ponko_ecs::{ResourceTuple, World};
/// struct Mine;
/// impl ResourceTuple for Mine {
///     const SEAL: ponko_ecs::seal::Seal = ponko_ecs::seal::Seal;
///     type Refs<'w> = ();
///     fn fetch(_: &mut World) -> Self::Refs<'_> {}
/// }
/// ```
mod seal {
    #[doc(hidden)]
    pub struct Seal;
}
mod access;
mod borrow;
mod bundle;
mod change;
mod commands;
mod entity;
mod hooks;
#[cfg(feature = "parallel")]
mod par;
mod query;
mod required;
mod slots;
mod snapshot;
mod storage;
mod view;
mod world;

pub use access::AccessLog;
pub use borrow::{Lookup, QueryLookup, QueryResources, ReadOnlyQueryData, ResourceSet, ResourceTuple};
pub use bundle::{Bundle, Component};
pub use change::{Added, Changed, ChunkQuery, Tick};
pub use commands::Commands;
pub use entity::Entity;
pub use hooks::{DynamicHook, Hook, HookWorld, ReplaceHook};
// Web builds with threads only: how the engine runs parallel loops started
// on the page's own thread, which may not block (see `par`).
#[cfg(all(feature = "parallel", target_family = "wasm", target_feature = "atomics"))]
pub use par::{WebRunner, set_web_runner};
/// `#[derive(Component)]`: `impl Component for Type {}`. See [`Component`](trait@Component).
#[cfg(feature = "derive")]
pub use ponko_macros::Component;
pub use query::{
    CompiledFilter, DynFilter, FilterError, FilterShape, FilterTree, MAX_FILTER_DEPTH, Or, OrTerm, OrTerms, Query,
    QueryCache, QueryCachePolicy, QueryCacheStats, QueryData, QueryFilter, QueryIter, SingleError, With, Without,
    compile_tree,
};
pub use required::Requires;
pub use snapshot::{SnapshotError, WorldSnapshot};
/// What `#[derive(Component)]` uses to notice a `Clone` type. Not part of the API.
#[doc(hidden)]
pub mod __snapshot {
    pub use crate::snapshot::{CloneFn, NotClone, Probe, ViaClone};
}
pub use storage::{Archetype, ComponentId, ComponentInfo, DropFn};
pub use view::{QueryRef, ViewGroup, ViewQuery, ViewState, WorldView};
pub use world::{Chunk, World};
