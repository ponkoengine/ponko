//! The [`World`]: owns every entity, component and resource.

use std::alloc::Layout;
use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::access::{AccessLog, Recorder};
use crate::bundle::{Bundle, Component};
use crate::commands::Commands;
use crate::entity::{Entities, Entity, Location};
use crate::query::{Query, QueryData};
use crate::storage::{Archetype, ComponentId, ComponentInfo, DropFn, TypeMap, drop_ptr};

/// The bit of [`BundleInfo::archetype`] (and of what `bundle_archetype`
/// returns) that marks a bundle whose archetype has a component with an
/// `on_add` hook, or that lacks a component it requires. No archetype
/// index reaches it, so the bounds check that indexing the archetypes
/// makes anyway sends such spawns to the hooked path, and other spawns
/// check nothing more: a separate test of a flag per spawn measured 2 to
/// 5 % slower one-by-one spawns (`crates/ponko_bench/HOOKS.md`).
const HOOKED_BUNDLE: usize = 1 << (usize::BITS - 1);

/// Writes `bundle` into a new row of `arch` for `e` and returns the row;
/// `cols` maps the bundle's elements to `arch`'s columns.
#[inline(always)]
fn write_row<B: Bundle>(
    arch: &mut Archetype,
    cols: &[usize],
    changes: &crate::change::Changes,
    pool: &mut crate::storage::BufferPool,
    bundle: B,
    e: Entity,
) -> usize {
    arch.reserve_row(changes, pool);
    let row = arch.len();
    // SAFETY: `reserve_row` made room for `row`.
    unsafe { write_bundle(arch, cols, bundle, row) };
    arch.entities.push(e);
    row
}

/// Moves the values of `bundle` into row `row` of `arch`; `cols` maps the
/// bundle's elements to `arch`'s columns.
///
/// # Safety
/// `row` is within `arch`'s capacity, and `cols` is the column map the
/// world cached for `B` in `arch`.
#[inline(always)]
unsafe fn write_bundle<B: Bundle>(arch: &mut Archetype, cols: &[usize], bundle: B, row: usize) {
    // SAFETY: the caller made room for `row` in every column; `cols` maps
    // each element to the column of its type, so `src` (a value of that
    // type, moved out of `bundle`) is copied into storage of its own layout,
    // whose values are `size` bytes apart.
    unsafe {
        bundle.write(&mut |i, src, size| {
            let c = &arch.columns[cols[i]];
            debug_assert_eq!(c.size(), size);
            std::ptr::copy_nonoverlapping(src, c.base().add(row * size), size);
        });
    }
}

/// Makes the values a bundle lacks and requires into row `row` of `arch`,
/// for entity `e`: `fills` lists their columns and constructors. If a
/// constructor panics, `e` goes back to `entities`; the values made before
/// it are leaked, never dropped twice.
///
/// # Safety
/// `row` is within `arch`'s capacity, and `fills` is the list the world
/// cached for a bundle that spawns into `arch`.
#[inline(always)]
unsafe fn fill_row(
    arch: &mut Archetype,
    fills: &[(usize, crate::hooks::Make)],
    row: usize,
    entities: &mut Entities,
    e: Entity,
) {
    if fills.is_empty() {
        return;
    }
    let unplaced = Unplaced { entities, e };
    for &(col, make) in fills {
        let c = &arch.columns[col];
        // SAFETY: the caller made room for `row`; `col` is the column of the
        // component `make` makes.
        unsafe { make.write(c.ptr(row), c.size()) };
    }
    std::mem::forget(unplaced);
}

/// [`World::placed`] on the two fields it changes, for loops that hold
/// other parts of the world meanwhile.
#[inline(always)]
fn placed(slots: &mut crate::slots::Slots, entities: &mut Entities, a: usize, row: usize, e: Entity) {
    if row == 0 {
        slots.filled(a);
    }
    entities.set(
        e,
        Location {
            archetype: a as u32,
            row: row as u32,
        },
    );
}

struct BundleInfo {
    /// The archetype (the bundle's components and those they require),
    /// with [`HOOKED_BUNDLE`] set if it has `on_add` hooks or the bundle
    /// lacks a required component.
    archetype: usize,
    /// Column index in the archetype for each tuple element.
    columns: Box<[usize]>,
    /// The columns of the archetype that the bundle doesn't fill, and how
    /// to make their required values; empty unless the bundle lacks a
    /// component it requires. Read only on the tagged path, next to
    /// `columns`, so filling gaps costs no second lookup.
    fills: Box<[(usize, crate::hooks::Make)]>,
    /// Whether the archetype has `on_add` hooks to run after a spawn.
    adds: bool,
}

/// One chunk of entities handed to [`RawWorld::for_each_chunk`](crate::RawWorld::for_each_chunk) or
/// [`RawWorld::query_chunks`](crate::RawWorld::query_chunks): `len` entities and one column pointer per
/// requested component, in request order.
#[non_exhaustive]
pub struct Chunk<'a> {
    /// The entities in this chunk; row `i` of each column belongs to `entities[i]`.
    pub entities: &'a [Entity],
    /// Column start pointers, each pointing at `entities.len()` packed values.
    pub columns: &'a [*mut u8],
}

impl Chunk<'_> {
    /// Number of entities (rows) in the chunk.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// `true` if the chunk has no rows (never true for chunks passed to callbacks).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }
}

/// Holds everything in the simulation: entities, their components, and
/// global resources.
///
/// ```
/// use ponko_ecs::*;
///
/// #[derive(Component)]
/// struct Pos(f32, f32);
/// #[derive(Component)]
/// struct Vel(f32, f32);
///
/// let mut world = World::new();
/// let crab = world.spawn((Pos(0.0, 0.0), Vel(1.0, 2.0)));
///
/// world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| {
///     p.0 += v.0;
///     p.1 += v.1;
/// });
///
/// assert_eq!(world.get::<Pos>(crab).unwrap().1, 2.0);
/// world.despawn(crab);
/// assert!(world.get::<Pos>(crab).is_none());
/// ```
pub struct World {
    pub(crate) infos: Vec<ComponentInfo>,
    by_type: TypeMap<ComponentId>,
    by_name: HashMap<String, ComponentId>,
    // Slots are never removed, but an empty archetype's slot may be
    // reclaimed and reused for another component set: see `slots.rs` for
    // what that means for the indices kept elsewhere.
    pub(crate) archetypes: Vec<Archetype>,
    pub(crate) query_matches: crate::query::MatchCache,
    /// Which archetypes hold entities (queries skip the others by it),
    /// which are empty, and the storage of reclaimed ones.
    pub(crate) slots: crate::slots::Slots,
    /// Reused work-plan storage of parallel queries.
    #[cfg(feature = "parallel")]
    pub(crate) par_plan: crate::query::ParPlan,
    pub(crate) archetype_by_ids: HashMap<Box<[ComponentId]>, usize>,
    pub(crate) entities: Entities,
    bundles: TypeMap<BundleInfo>,
    resources: TypeMap<Box<dyn Any + Send + Sync>>,
    /// Resources without a Rust type (the C and C++ path), by name.
    dynamic_resources: HashMap<String, RawValue>,
    scratch_ids: Vec<ComponentId>,
    /// Access recording for declared-access checks (debug builds only).
    pub(crate) recorder: Recorder,
    /// Change detection: the tick counter and the removal logs.
    pub(crate) changes: crate::change::Changes,
    /// Whether a component hook was ever registered: the one thing a
    /// despawn, insert or remove checks in a world without hooks.
    pub(crate) hooked: bool,
    /// Component hooks and the commands they recorded. Boxed: a world
    /// without hooks shouldn't be bigger for them.
    pub(crate) hooks: Box<crate::hooks::Hooks>,
    /// Unique per world, so a [`ViewState`](crate::ViewState) prepared on
    /// one world is never used to view another.
    pub(crate) uid: u64,
    /// Names the layout of the archetype slots (which component set is in
    /// which slot): changes whenever an archetype is made or reclaimed, to
    /// a number this world never had before. A snapshot restore with the
    /// same number skips laying the slots out again.
    pub(crate) layout: u64,
    /// The last number `layout` took; only grows.
    layouts: u64,
    /// Resources snapshots include ([`World::snapshot_resource`]).
    pub(crate) snapshot_resources: Vec<crate::snapshot::SnapshotResource>,
}

// Own the existing box so scoped access needs neither unsafe code nor a new
// allocation. Field borrows separate the callback's World and resource access.
struct ResourceScope<'a> {
    world: &'a mut World,
    id: TypeId,
    value: Option<Box<dyn Any + Send + Sync>>,
}

impl Drop for ResourceScope<'_> {
    fn drop(&mut self) {
        // Restore before dropping any replacement, even if its destructor panics.
        self.world.resources.insert(self.id, self.value.take().unwrap());
    }
}

/// One value with no Rust type: plain bytes in their own allocation, so a
/// pointer to it stays valid until the value is replaced or removed.
struct RawValue {
    ptr: std::ptr::NonNull<u8>,
    layout: Layout,
}

// SAFETY: a RawValue is plain bytes with no destructor and no shared state;
// the world hands out access to it under its own borrows.
unsafe impl Send for RawValue {}
unsafe impl Sync for RawValue {}

impl RawValue {
    /// A copy of `layout.size()` bytes at `data`.
    ///
    /// # Safety
    /// `data` points to `layout.size()` readable bytes (any pointer for size 0).
    unsafe fn copy_of(layout: Layout, data: *const u8) -> Self {
        let ptr = if layout.size() == 0 {
            // SAFETY: an alignment is never zero; nothing is read or written.
            unsafe { std::ptr::NonNull::new_unchecked(std::ptr::without_provenance_mut(layout.align())) }
        } else {
            // SAFETY: the size is non-zero.
            let p = unsafe { std::alloc::alloc(layout) };
            std::ptr::NonNull::new(p).unwrap_or_else(|| std::alloc::handle_alloc_error(layout))
        };
        if layout.size() != 0 {
            // SAFETY: the caller's bytes are readable; ours are fresh and don't overlap.
            unsafe { std::ptr::copy_nonoverlapping(data, ptr.as_ptr(), layout.size()) };
        }
        RawValue { ptr, layout }
    }
}

impl Drop for RawValue {
    fn drop(&mut self) {
        if self.layout.size() != 0 {
            // SAFETY: allocated in `copy_of` with this layout.
            unsafe { std::alloc::dealloc(self.ptr.as_ptr(), self.layout) };
        }
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// A number no other world of this process has. Code that remembers
    /// something about a world, such as a [`Tick`](crate::Tick) it last
    /// looked at, keeps this next to it to notice that the world was
    /// replaced: the new world's ticks start over and mean something else.
    ///
    /// ```
    /// use ponko_ecs::World;
    ///
    /// let (a, b) = (World::new(), World::new());
    /// assert_ne!(a.id(), b.id());
    /// assert_eq!(a.id(), a.id());
    /// ```
    #[inline]
    #[must_use]
    pub fn id(&self) -> u64 {
        self.uid
    }

    /// An empty world.
    ///
    /// ```
    /// use ponko_ecs::World;
    /// let world = World::new();
    /// assert!(world.is_empty());
    /// assert_eq!(world.len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        let mut w = World {
            infos: Vec::new(),
            by_type: TypeMap::default(),
            by_name: HashMap::new(),
            archetypes: Vec::new(),
            query_matches: Default::default(),
            slots: Default::default(),
            #[cfg(feature = "parallel")]
            par_plan: Default::default(),
            archetype_by_ids: HashMap::new(),
            entities: Entities::default(),
            bundles: TypeMap::default(),
            resources: TypeMap::default(),
            dynamic_resources: HashMap::new(),
            scratch_ids: Vec::new(),
            recorder: Recorder::default(),
            changes: Default::default(),
            hooked: false,
            hooks: Default::default(),
            uid: crate::view::next_world_uid(),
            layout: 0,
            layouts: 0,
            snapshot_resources: Vec::new(),
        };
        let empty = w.archetype_for(&[]); // archetype 0: entities with no components
        w.slots.pin(empty);
        w
    }

    // ---- entities ----------------------------------------------------------

    /// Creates an entity with the given components and returns its handle.
    ///
    /// Components come as a tuple; note the trailing comma for one component:
    /// `world.spawn((Health(10),))`.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Health(i32);
    /// #[derive(Component)]
    /// struct Player;
    /// let mut world = World::new();
    /// let e = world.spawn((Health(10), Player));
    /// assert_eq!(world.get::<Health>(e), Some(&Health(10)));
    /// assert!(world.has::<Player>(e));
    /// ```
    ///
    /// # Panics
    /// If the bundle names the same component type twice.
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        self.recorder.note(|l| l.structural = true);
        let a = self.bundle_archetype::<B>();
        let e = self.entities.alloc();
        self.spawn_in::<B>(a, bundle, e)
    }

    /// Spawns many entities of the same shape. Faster than calling `spawn` in a loop.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Coin(u32);
    /// let mut world = World::new();
    /// world.spawn_batch((0..100).map(|i| (Coin(i),)));
    /// assert_eq!(world.len(), 100);
    /// ```
    ///
    /// # Panics
    /// If the bundle names the same component type twice.
    pub fn spawn_batch<B: Bundle>(&mut self, bundles: impl IntoIterator<Item = B>) {
        self.recorder.note(|l| l.structural = true);
        let iter = bundles.into_iter();
        let a = self.bundle_archetype::<B>();
        let Some(arch) = self.archetypes.get_mut(a) else {
            // `a` is tagged (`HOOKED_BUNDLE`): the archetype has `on_add`
            // hooks, or the bundle lacks a component it requires.
            return self.spawn_batch_hooked::<B>(a & !HOOKED_BUNDLE, iter);
        };
        arch.reserve(iter.size_hint().0, &mut self.slots.buffers);
        // One lookup of the bundle's column map for the whole batch.
        let cols = &self.bundles[&TypeId::of::<B>()].columns;
        for b in iter {
            let e = self.entities.alloc();
            let row = write_row(arch, cols, &self.changes, &mut self.slots.buffers, b, e);
            placed(&mut self.slots, &mut self.entities, a, row, e);
        }
    }

    /// Spawns an entity with exactly the handle `e`, for restoring saved
    /// worlds so that components referring to entities stay valid. Returns
    /// `false` (dropping `bundle`) if `e` is taken, or was despawned in this
    /// world already (old handles stay dead).
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, World};
    ///
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Name(&'static str);
    ///
    /// let mut world = World::new();
    /// let saved = Entity::from_bits(7 | 3 << 32); // slot 7, generation 3
    /// assert!(world.spawn_at(saved, (Name("hello"),)));
    /// assert_eq!(world.get::<Name>(saved), Some(&Name("hello")));
    /// assert!(!world.spawn_at(saved, (Name("again"),))); // taken
    /// let fresh = world.spawn((Name("new"),)); // fresh handles never collide
    /// assert_ne!(fresh, saved);
    /// ```
    ///
    /// # Panics
    /// If the bundle names the same component type twice.
    pub fn spawn_at<B: Bundle>(&mut self, e: Entity, bundle: B) -> bool {
        self.recorder.note(|l| l.structural = true);
        if !self.entities.alloc_at(e) {
            return false;
        }
        let a = self.bundle_archetype::<B>();
        self.spawn_in::<B>(a, bundle, e);
        true
    }

    /// Memory held for entity data: component columns and handles of every
    /// archetype, including spare capacity and the buffers of reclaimed
    /// archetypes kept for new ones. Doesn't count resources or the
    /// world's bookkeeping (per-slot generation, archetype index).
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component)]
    /// struct Pos(f32, f32);
    /// let mut world = World::new();
    /// world.spawn_batch((0..1000).map(|i| (Pos(i as f32, 0.0),)));
    /// // 8 bytes of Pos + 8 bytes of entity handle each, at least.
    /// assert!(world.allocated_bytes() >= 1000 * 16);
    /// ```
    pub fn allocated_bytes(&self) -> usize {
        self.archetypes.iter().map(Archetype::allocated_bytes).sum::<usize>() + self.slots.kept_bytes()
    }

    /// Despawns every entity. Resources stay.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component)]
    /// struct Level(u8);
    ///
    /// let mut world = World::new();
    /// let e = world.spawn((Level(1),));
    /// world.clear();
    /// assert!(world.is_empty() && !world.is_alive(e));
    /// ```
    pub fn clear(&mut self) {
        self.recorder.note(|l| l.structural = true);
        let all: Vec<Entity> = self
            .archetypes
            .iter()
            .flat_map(|a| a.entities.iter().copied())
            .collect();
        if self.hooked {
            // The hooks' commands run after the last despawn.
            let held = self.hold_hooks();
            for e in all {
                held.world.despawn(e);
            }
            return;
        }
        for e in all {
            self.despawn(e);
        }
    }

    /// Despawns every entity **and forgets every entity id**, as if the world
    /// were new; resources and registered components stay. For everything
    /// but replacing the whole world, use [`clear`](Self::clear).
    ///
    /// **Old handles can match new entities afterwards.** Generations start
    /// over, so a handle kept from before the reset (in a component, a
    /// resource, a system's own state) may name an entity spawned after it,
    /// and `get`, `despawn` and the rest then act on that entity. That is
    /// the point of resetting: a save is loaded by resetting and spawning
    /// its entities again under their saved handles with
    /// [`spawn_at`](Self::spawn_at), so the handles stored inside the save
    /// stay valid. It also means every handle that isn't part of what is
    /// loaded must be dropped before, or looked up again after.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Level(u8);
    ///
    /// let mut world = World::new();
    /// let old = world.spawn((Level(1),));
    /// world.reset();
    /// assert!(!world.is_alive(old));
    /// // The first handle after a reset is the first handle of a new world:
    /// // the one `old` holds.
    /// let new = world.spawn((Level(2),));
    /// assert_eq!(new, old);
    /// assert_eq!(world.get::<Level>(old), Some(&Level(2)));
    ///
    /// // What it is for: the saved handle is free again.
    /// world.reset();
    /// assert!(world.spawn_at(old, (Level(3),)));
    /// ```
    pub fn reset(&mut self) {
        self.recorder.note(|l| l.structural = true);
        if self.hooked {
            // `on_remove` hooks run; what they record is dropped, since the
            // world starts over.
            let held = self.hold_hooks();
            held.world.clear();
            held.world.drop_hook_commands();
        } else {
            self.clear();
        }
        self.entities.reset();
    }

    /// Places `e`, allocated and without a location, with `bundle` in
    /// archetype `a` as `bundle_archetype` returned it, and runs the
    /// `on_add` hooks if it is tagged as having some.
    #[inline]
    fn spawn_in<B: Bundle>(&mut self, a: usize, bundle: B, e: Entity) -> Entity {
        let info = &self.bundles[&TypeId::of::<B>()];
        // A tagged index (`HOOKED_BUNDLE`) is past the end: the bounds check
        // that indexing made here sends it to the hooked path, with the
        // cache entry this lookup found.
        let Some(arch) = self.archetypes.get_mut(a) else {
            let info: *const BundleInfo = info;
            // SAFETY: `info` is `B`'s entry, and the bundle cache doesn't
            // change before `spawn_hooked` is done reading it.
            return unsafe { self.spawn_hooked::<B>(a & !HOOKED_BUNDLE, info, bundle, e) };
        };
        let row = write_row(arch, &info.columns, &self.changes, &mut self.slots.buffers, bundle, e);
        self.placed(a, row, e);
        e
    }

    /// [`spawn_in`](Self::spawn_in) of a tagged bundle (`a` untagged):
    /// places `e` with the components the bundle lacks and requires, then
    /// runs the archetype's `on_add` hooks and their commands. One row is
    /// reserved, the missing values are made into it, then the bundle's are
    /// moved in, as a plain spawn moves them.
    ///
    /// # Safety
    /// `info` points to `B`'s entry in this world's bundle cache. Nothing
    /// here changes the cache before the hooks run, and `info` is not read
    /// after that.
    #[cold]
    #[inline(never)]
    unsafe fn spawn_hooked<B: Bundle>(&mut self, a: usize, info: *const BundleInfo, bundle: B, e: Entity) -> Entity {
        // SAFETY: the caller's contract. The entry lives in the map's own
        // allocation, which nothing below touches until `adds` is read.
        let info = unsafe { &*info };
        let arch = &mut self.archetypes[a];
        arch.reserve_row(&self.changes, &mut self.slots.buffers);
        let row = arch.len();
        // SAFETY: `reserve_row` made room for `row`; `info` holds the fills
        // and the column map cached for `B` in this archetype.
        unsafe {
            fill_row(arch, &info.fills, row, &mut self.entities, e);
            write_bundle(arch, &info.columns, bundle, row);
        }
        arch.entities.push(e);
        let adds = info.adds;
        self.placed(a, row, e);
        if adds {
            self.run_spawn_hooks(a, e);
        }
        e
    }

    /// Records that `e` was written to `row` of archetype `a`.
    #[inline(always)]
    fn placed(&mut self, a: usize, row: usize, e: Entity) {
        placed(&mut self.slots, &mut self.entities, a, row, e);
    }

    /// [`spawn_batch`](Self::spawn_batch) of a tagged bundle (`a`
    /// untagged): each entity gets what it requires, its hooks run once it
    /// is placed, and their commands after the last.
    #[cold]
    #[inline(never)]
    fn spawn_batch_hooked<B: Bundle>(&mut self, a: usize, iter: impl Iterator<Item = B>) {
        self.archetypes[a].reserve(iter.size_hint().0, &mut self.slots.buffers);
        let info = &self.bundles[&TypeId::of::<B>()];
        if !info.adds {
            // Only gaps to fill, no hooks: one loop that keeps the
            // archetype and the cache entry at hand, like a plain batch.
            let arch = &mut self.archetypes[a];
            for b in iter {
                let e = self.entities.alloc();
                arch.reserve_row(&self.changes, &mut self.slots.buffers);
                let row = arch.len();
                // SAFETY: `reserve_row` made room for `row`; `info` holds
                // the fills and the column map cached for `B` here.
                unsafe {
                    fill_row(arch, &info.fills, row, &mut self.entities, e);
                    write_bundle(arch, &info.columns, b, row);
                }
                arch.entities.push(e);
                placed(&mut self.slots, &mut self.entities, a, row, e);
            }
            return;
        }
        let held = self.hold_hooks();
        // Hooks can't change the bundle cache, and their commands wait for
        // the end of the batch: the entry stays put for the whole loop.
        let info: *const BundleInfo = &held.world.bundles[&TypeId::of::<B>()];
        for b in iter {
            let e = held.world.entities.alloc();
            // SAFETY: `info` is `B`'s entry, unchanged until the batch ends.
            unsafe { held.world.spawn_hooked::<B>(a, info, b, e) };
        }
    }

    /// Removes an entity and drops its components. Returns `false` if it was
    /// already gone.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(1.0),));
    /// assert!(world.despawn(e));
    /// assert!(!world.despawn(e)); // already gone
    /// assert!(world.get::<Pos>(e).is_none()); // the old handle stays dead
    /// ```
    pub fn despawn(&mut self, e: Entity) -> bool {
        self.recorder.note(|l| l.structural = true);
        let Some(loc) = self.entities.get(e) else { return false };
        if self.hooked && self.hooks.removes(loc.archetype as usize) {
            return self.despawn_hooked(e, loc);
        }
        self.despawn_at(e, loc)
    }

    /// [`despawn`](Self::despawn) of an entity with `on_remove` hooks:
    /// they run first, while it is whole, then it goes, then their commands.
    #[cold]
    #[inline(never)]
    fn despawn_hooked(&mut self, e: Entity, loc: Location) -> bool {
        self.run_despawn_hooks(loc.archetype as usize, e);
        // Hooks can't change the world's structure, so `e` is where it was;
        // looked up again all the same.
        let gone = self.entities.get(e).is_some_and(|loc| self.despawn_at(e, loc));
        self.flush_hooks();
        gone
    }

    /// Despawns `e`, which is at `loc`: [`despawn`](Self::despawn) after the hooks.
    #[inline(always)]
    fn despawn_at(&mut self, e: Entity, loc: Location) -> bool {
        // Free the slot first: if a component's drop panics, the entity is already gone.
        self.entities.free(e);
        let a = loc.archetype as usize;
        // One bounds check for both uses.
        let archetype = &mut self.archetypes[a];
        if archetype.tracked() {
            self.changes.log_despawn(archetype, e, loc.row as usize);
        }
        let moved = unsafe { archetype.swap_remove_drop(loc.row as usize) };
        let Some(moved) = moved else {
            // The removed row was the last one; row 0 means it was the only
            // one. Deciding this from what `swap_remove_drop` returned, not
            // from the archetype's length, keeps the common path below free
            // of the bookkeeping (issue #448).
            std::hint::cold_path();
            if loc.row == 0 {
                self.despawned_last(a);
            }
            return true;
        };
        self.entities.set_row(moved, loc.row as usize);
        true
    }

    /// The rare end of [`despawn`](Self::despawn): archetype `a` lost its
    /// last entity.
    #[cold]
    #[inline(never)]
    fn despawned_last(&mut self, a: usize) {
        debug_assert!(self.archetypes[a].is_empty());
        self.slots.emptied(a);
        self.reclaim_if_over();
    }

    /// `true` if `e` hasn't been despawned.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Tag;
    /// let mut world = World::new();
    /// let e = world.spawn((Tag,));
    /// assert!(world.is_alive(e));
    /// world.despawn(e);
    /// assert!(!world.is_alive(e));
    /// ```
    #[must_use]
    pub fn is_alive(&self, e: Entity) -> bool {
        self.entities.get(e).is_some()
    }

    /// Number of live entities.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// world.spawn((Pos(0.0),));
    /// world.spawn((Pos(1.0),));
    /// assert_eq!(world.len(), 2);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// `true` if there are no live entities.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Tag;
    /// let mut world = World::new();
    /// assert!(world.is_empty());
    /// world.spawn((Tag,));
    /// assert!(!world.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Size of the entity slot table: one past the highest entity index this
    /// world has handed out or restored since it was created or
    /// [`reset`](Self::reset). Despawned slots stay in the table for reuse, so
    /// this never shrinks except on `reset`.
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, World};
    ///
    /// #[derive(Component)]
    /// struct Level(u8);
    ///
    /// let mut world = World::new();
    /// let a = world.spawn((Level(1),));
    /// world.spawn((Level(2),));
    /// world.despawn(a);
    /// assert_eq!((world.len(), world.entity_slots()), (1, 2));
    /// assert!(world.spawn_at(Entity::from_bits(9), (Level(3),)));
    /// assert_eq!(world.entity_slots(), 10);
    /// ```
    #[must_use]
    pub fn entity_slots(&self) -> usize {
        self.entities.slots()
    }

    // ---- single-entity access -----------------------------------------------

    /// Component `T` of entity `e`, or `None` if `e` is dead or doesn't have one.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(2.0),));
    /// assert_eq!(world.get::<Pos>(e), Some(&Pos(2.0)));
    /// world.despawn(e);
    /// assert_eq!(world.get::<Pos>(e), None);
    /// ```
    #[must_use]
    pub fn get<T: Component>(&self, e: Entity) -> Option<&T> {
        let id = self.component_id::<T>()?;
        self.recorder.note(|l| l.component(id, false));
        let ptr = self.locate(e, id)?;
        Some(unsafe { &*ptr.cast::<T>() })
    }

    /// Mutable version of [`get`](Self::get).
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(2.0),));
    /// world.get_mut::<Pos>(e).unwrap().0 += 1.0;
    /// assert_eq!(world.get::<Pos>(e), Some(&Pos(3.0)));
    /// ```
    pub fn get_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        let id = self.component_id::<T>()?;
        self.recorder.note(|l| l.component(id, true));
        let loc = self.entities.get(e)?;
        let arch = &self.archetypes[loc.archetype as usize];
        let col = arch.column_index(id)?;
        if arch.tracked() {
            // A mutable borrow counts as a change.
            arch.stamp(col, loc.row as usize, self.changes.stamp(), false);
        }
        let ptr = arch.columns[col].ptr(loc.row as usize);
        Some(unsafe { &mut *ptr.cast::<T>() })
    }

    /// `true` if `e` is alive and has component `T`.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// #[derive(Component)]
    /// struct Tag;
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(0.0),));
    /// assert!(world.has::<Pos>(e));
    /// assert!(!world.has::<Tag>(e));
    /// ```
    #[must_use]
    pub fn has<T: Component>(&self, e: Entity) -> bool {
        self.component_id::<T>().is_some_and(|id| self.locate(e, id).is_some())
    }

    /// Component `T` of several entities at once, all mutable: for work on
    /// a pair (or a few) entities that needs both sides, such as resolving a
    /// hit between two of them.
    ///
    /// `None` if any of them is dead or has no `T`, or if the same entity is
    /// named twice (two `&mut` to one value can't exist).
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    ///
    /// #[derive(Component)]
    /// struct Health(i32);
    /// let mut world = World::new();
    /// let a = world.spawn((Health(10),));
    /// let b = world.spawn((Health(4),));
    ///
    /// if let Some([a, b]) = world.get_many_mut::<Health, 2>([a, b]) {
    ///     std::mem::swap(&mut a.0, &mut b.0);
    /// }
    /// assert_eq!(world.get::<Health>(a).unwrap().0, 4);
    /// assert!(world.get_many_mut::<Health, 2>([a, a]).is_none());
    /// ```
    pub fn get_many_mut<T: Component, const N: usize>(&mut self, entities: [Entity; N]) -> Option<[&mut T; N]> {
        let id = self.component_id::<T>()?;
        for (i, &e) in entities.iter().enumerate() {
            if entities[..i].contains(&e) {
                return None;
            }
            self.locate(e, id)?;
        }
        let mut ptrs = [std::ptr::null_mut::<T>(); N];
        for (ptr, e) in ptrs.iter_mut().zip(entities) {
            *ptr = self.get_mut::<T>(e)?;
        }
        // SAFETY: the entities are alive and all different, so each pointer
        // is to its own row; they stay valid while `self` is borrowed.
        Some(ptrs.map(|ptr| unsafe { &mut *ptr }))
    }

    /// Adds component `value` to `e`, replacing any existing `T`.
    /// Returns `false` (and drops `value`) if `e` is dead.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Tag;
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Tag,));
    /// assert!(world.insert(e, Pos(1.0)));
    /// assert!(world.insert(e, Pos(2.0))); // replaces
    /// assert_eq!(world.get::<Pos>(e), Some(&Pos(2.0)));
    /// world.despawn(e);
    /// assert!(!world.insert(e, Pos(3.0))); // dead
    /// ```
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> bool {
        self.recorder.note(|l| l.structural = true);
        let id = self.register::<T>();
        let value = std::mem::ManuallyDrop::new(value);
        let ok = unsafe { self.insert_dynamic_inner(e, id, (&*value as *const T).cast()) };
        if !ok {
            drop(std::mem::ManuallyDrop::into_inner(value));
        }
        ok
    }

    /// Takes component `T` off `e` and returns it.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(1.0),));
    /// assert_eq!(world.remove::<Pos>(e), Some(Pos(1.0)));
    /// assert_eq!(world.remove::<Pos>(e), None);
    /// assert!(world.is_alive(e)); // only the component went
    /// ```
    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        self.recorder.note(|l| l.structural = true);
        let id = self.component_id::<T>()?;
        let loc = self.entities.get(e)?;
        let arch = &self.archetypes[loc.archetype as usize];
        let col = arch.column_index(id)?;
        if self.hooked && self.hooks.removes_component(id) {
            return self.remove_hooked::<T>(e, id);
        }
        let value = unsafe { arch.columns[col].ptr(loc.row as usize).cast::<T>().read() };
        self.move_without(e, loc, id);
        Some(value)
    }

    /// [`remove`](Self::remove) of a component with `on_remove` hooks: they
    /// run first, while `e` still has it, then their commands after it went.
    #[cold]
    #[inline(never)]
    fn remove_hooked<T: Component>(&mut self, e: Entity, id: ComponentId) -> Option<T> {
        self.run_hooks(id, e, crate::hooks::Kind::Remove);
        // Hooks can't change the world's structure; looked up again all the same.
        let value = self.locate(e, id).map(|ptr| {
            // SAFETY: `id` is `T`'s column, and the value is forgotten by
            // `move_without` right after, so it is owned once.
            let value = unsafe { ptr.cast::<T>().read() };
            if let Some(loc) = self.entities.get(e) {
                self.move_without(e, loc, id);
            }
            value
        });
        self.flush_hooks();
        value
    }

    /// Gives every entity in `entities` a clone of `value`, replacing any
    /// `T` it has, and returns how many were alive. The result is the same
    /// as [`insert`](Self::insert) on each entity in turn, but entities that
    /// move between the same two archetypes move together: the archetype
    /// change is looked up once, and when the batch is a whole archetype in
    /// its row order (the order a query visits it, or
    /// [`Archetype::entities`](crate::Archetype::entities) lists it), its
    /// columns are copied in one piece. Dead entities are skipped.
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, World};
    /// #[derive(Component)]
    /// struct Enemy;
    /// #[derive(Component, Clone)]
    /// struct Burning;
    /// let mut world = World::new();
    /// let enemies: Vec<Entity> = (0..100).map(|_| world.spawn((Enemy,))).collect();
    /// assert_eq!(world.insert_many(&enemies, Burning), 100);
    /// assert_eq!(world.query::<&Burning>().iter().count(), 100);
    /// ```
    pub fn insert_many<T: Component + Clone>(&mut self, entities: &[Entity], value: T) -> usize {
        self.recorder.note(|l| l.structural = true);
        let id = self.register::<T>();
        if self.hooked
            && (self.hooks.requires_component(id) || self.hooks.adds_component(id) || self.hooks.replaces_component(id))
        {
            // Hooks run per entity, between moves: one at a time.
            return entities.iter().filter(|&&e| self.insert(e, value.clone())).count();
        }
        let mut alive = 0;
        let mut i = 0;
        while i < entities.len() {
            let e = entities[i];
            let Some(loc) = self.entities.get(e) else {
                i += 1;
                continue;
            };
            let src = loc.archetype as usize;
            if self.archetypes[src].column_index(id).is_some() {
                self.insert(e, value.clone());
                alive += 1;
                i += 1;
                continue;
            }
            // The run: the entities from `i` on that are in `src` now.
            let run = i + self.run_in(&entities[i..], src);
            let dst = self.archetype_adding(src, id);
            let col = self.archetypes[dst]
                .column_index(id)
                .expect("invariant: the archetype that adding `id` leads to has `id`");
            let stamp = self.changes.stamp();
            if self.whole_archetype(src, &entities[i..run]) {
                // The clones first, into room past the end of `dst`: a
                // `clone` that panics leaks those made and leaves the world
                // as it was.
                let n = self.archetypes[src].len();
                let d = &mut self.archetypes[dst];
                d.reserve(n, &mut self.slots.buffers);
                let base = d.len();
                for row in base..base + n {
                    // SAFETY: column `col` holds `T`, and `row` is spare
                    // capacity that `move_all` makes live below.
                    unsafe { d.columns[col].ptr(row).cast::<T>().write(value.clone()) };
                }
                self.move_all(src, dst);
                let d = &self.archetypes[dst];
                if d.tracked() {
                    for row in base..base + n {
                        d.stamp(col, row, stamp, true);
                    }
                }
                self.reclaim_if_over();
            } else {
                for &e in &entities[i..run] {
                    match self.entities.get(e) {
                        Some(loc) if loc.archetype as usize == src => {
                            let v = value.clone();
                            let row = self.move_row(e, loc, dst);
                            self.put(dst, col, row, v, stamp);
                        }
                        // Named twice in the run: it has `T` by now.
                        _ => {
                            self.insert(e, value.clone());
                        }
                    }
                }
            }
            alive += run - i;
            i = run;
        }
        alive
    }

    /// Takes component `T` off every entity in `entities` that has it,
    /// dropping the values, and returns how many had it. The result is the
    /// same as [`remove`](Self::remove) on each entity in turn; entities
    /// move together as in [`insert_many`](Self::insert_many).
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, World};
    /// #[derive(Component)]
    /// struct Enemy;
    /// #[derive(Component, Clone)]
    /// struct Burning;
    /// let mut world = World::new();
    /// let enemies: Vec<Entity> = (0..100).map(|_| world.spawn((Enemy, Burning))).collect();
    /// assert_eq!(world.remove_many::<Burning>(&enemies[..40]), 40);
    /// assert_eq!(world.query::<&Burning>().iter().count(), 60);
    /// assert_eq!(world.remove_many::<Burning>(&enemies), 60); // the rest
    /// ```
    pub fn remove_many<T: Component>(&mut self, entities: &[Entity]) -> usize {
        self.recorder.note(|l| l.structural = true);
        let Some(id) = self.component_id::<T>() else {
            return 0;
        };
        if self.hooked && self.hooks.removes_component(id) {
            return entities.iter().filter(|&&e| self.remove::<T>(e).is_some()).count();
        }
        let tracked = self.infos[id.index()].tracked;
        let mut removed = 0;
        let mut i = 0;
        while i < entities.len() {
            let Some(loc) = self.entities.get(entities[i]) else {
                i += 1;
                continue;
            };
            let src = loc.archetype as usize;
            let Some(col) = self.archetypes[src].column_index(id) else {
                i += 1;
                continue;
            };
            let run = i + self.run_in(&entities[i..], src);
            let dst = self.archetype_removing(src, id);
            if self.whole_archetype(src, &entities[i..run]) {
                let n = self.archetypes[src].len();
                self.move_all(src, dst);
                // The values stay in `src`'s column, which no row owns now
                // and nothing reclaimed yet: dropped once each after the
                // move, so a `drop` that panics leaks the rest and leaves
                // the world sound.
                let c = &self.archetypes[src].columns[col];
                for row in 0..n {
                    // SAFETY: column `col` holds `T`; row `row` held a live
                    // value that the move forgot.
                    drop(unsafe { c.ptr(row).cast::<T>().read() });
                }
                self.reclaim_if_over();
                removed += n;
            } else {
                for &e in &entities[i..run] {
                    // Named twice in the run: it lacks `T` by now.
                    let Some(loc) = self.entities.get(e).filter(|l| l.archetype as usize == src) else {
                        continue;
                    };
                    if tracked && self.archetypes[src].tracked() {
                        self.changes.log_removal(id, e);
                    }
                    let c = &self.archetypes[src].columns[col];
                    // SAFETY: `e`'s value of `T`, owned once: the move forgets it.
                    let value = unsafe { c.ptr(loc.row as usize).cast::<T>().read() };
                    self.move_row(e, loc, dst);
                    drop(value);
                    removed += 1;
                }
            }
            i = run;
        }
        removed
    }

    /// How many entities at the start of `entities` are alive in archetype
    /// `src` (at least one: the caller found the first there).
    fn run_in(&self, entities: &[Entity], src: usize) -> usize {
        entities
            .iter()
            .position(|&e| self.entities.get(e).is_none_or(|l| l.archetype as usize != src))
            .unwrap_or(entities.len())
    }

    /// Whether `run` is every entity of archetype `src` in its row order,
    /// and `src` keeps no change stamps: then [`move_all`](Self::move_all)
    /// leaves the rows that moving them one by one would.
    fn whole_archetype(&self, src: usize, run: &[Entity]) -> bool {
        let arch = &self.archetypes[src];
        !arch.tracked() && arch.entities == run
    }

    /// Writes `value` into row `row` of column `col` of archetype `a`, a
    /// row a move left uninitialized, and stamps it as added.
    #[inline]
    fn put<T: Component>(&self, a: usize, col: usize, row: usize, value: T, stamp: u32) {
        let arch = &self.archetypes[a];
        // SAFETY: column `col` holds `T` (the caller found it by `T`'s id),
        // and its row `row` is uninitialized: the value is moved in once.
        unsafe { arch.columns[col].ptr(row).cast::<T>().write(value) };
        if arch.tracked() {
            arch.stamp(col, row, stamp, true);
        }
    }

    /// Moves every entity of archetype `src`, which keeps no change stamps,
    /// to the end of `dst`, in row order: the rows that moving them one by
    /// one in that order would leave. Shared columns are copied whole; the
    /// rest are forgotten, so the caller must have dropped or read them.
    /// Columns of `dst` that `src` lacks are left uninitialized for the
    /// caller to fill. Returns the first new row. `src`, now empty, is not
    /// reclaimed: the caller calls [`reclaim_if_over`](Self::reclaim_if_over)
    /// once done with its columns.
    fn move_all(&mut self, src: usize, dst: usize) -> usize {
        let (s, d) = two_mut(&mut self.archetypes, src, dst);
        debug_assert!(!s.tracked());
        let n = s.len();
        d.reserve(n, &mut self.slots.buffers);
        let base = d.len();
        let mut j = 0;
        for (i, col) in s.columns.iter().enumerate() {
            let id = s.ids[i];
            while j < d.ids.len() && d.ids[j] < id {
                j += 1;
            }
            if j < d.ids.len() && d.ids[j] == id {
                // SAFETY: rows `0..n` are live here; `dst` has room for `n`
                // more rows from `base`. The two buffers are different.
                unsafe { std::ptr::copy_nonoverlapping(col.ptr(0), d.columns[j].ptr(base), n * col.size()) };
            }
        }
        for (k, &e) in s.entities.iter().enumerate() {
            self.entities.set(
                e,
                Location {
                    archetype: dst as u32,
                    row: (base + k) as u32,
                },
            );
        }
        d.entities.extend_from_slice(&s.entities);
        s.entities.clear();
        if n > 0 {
            if base == 0 {
                self.slots.filled(dst);
            }
            self.slots.emptied(src);
        }
        base
    }

    // ---- queries -------------------------------------------------------------

    /// Loops over every entity that has the components in `Q`.
    ///
    /// `Q` is `&T`, `&mut T`, `Option<&T>`, `Option<&mut T>`, `Entity`, or a
    /// tuple of those. See [`Query`] for filters and iterators.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// world.spawn((Pos(1.0),));
    /// world.spawn((Pos(2.0),));
    /// world.query::<&mut Pos>().for_each(|p| p.0 *= 10.0);
    /// let sum: f32 = world.query::<&Pos>().iter().map(|p| p.0).sum();
    /// assert_eq!(sum, 30.0);
    /// ```
    ///
    /// # Panics
    /// If `Q` asks for the same component twice and at least once mutably.
    pub fn query<Q: QueryData>(&mut self) -> Query<'_, Q> {
        Query::new(self)
    }

    /// Loops over every entity that has the components in `Q`, reading only,
    /// from a shared world: for code that holds `&World`, such as a hash of
    /// the game state, a save, or a check in a test.
    ///
    /// `Q` is `&T`, `Option<&T>`, [`Entity`], or a tuple of those
    /// ([`ReadOnlyQueryData`](crate::ReadOnlyQueryData)); `&mut T` doesn't
    /// compile. Nothing is registered: a component the world has never seen
    /// matches no entity (and reads as `None` through `Option<&T>`). Several
    /// read-only queries can be held at once, and their items live as long
    /// as the world's borrow. See [`QueryRef`](crate::QueryRef).
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, World};
    /// #[derive(Component, Debug, PartialEq)]
    /// struct Pos(f32);
    /// #[derive(Component)]
    /// struct Hidden;
    ///
    /// fn total(world: &World) -> f32 {
    ///     world.query_ref::<&Pos>().iter().map(|p| p.0).sum()
    /// }
    /// let mut world = World::new();
    /// world.spawn((Pos(1.0),));
    /// let e = world.spawn((Pos(2.0),));
    /// assert_eq!(total(&world), 3.0);
    /// assert_eq!(world.query_ref::<(Entity, &Pos)>().get(e).map(|(_, p)| p), Some(&Pos(2.0)));
    /// assert_eq!(world.query_ref::<&Hidden>().count(), 0); // never seen: no match
    /// ```
    ///
    /// ```compile_fail
    /// # use ponko_ecs::{Component, World};
    /// # #[derive(Component)]
    /// # struct Pos(f32);
    /// let world = World::new();
    /// world.query_ref::<&mut Pos>(); // read-only
    /// ```
    pub fn query_ref<Q: crate::ReadOnlyQueryData>(&self) -> crate::QueryRef<'_, Q> {
        crate::QueryRef::new(self)
    }

    /// Reorders entities in memory so that, within each archetype that has
    /// component `T`, rows are sorted by `key(&T)` (ascending; ties keep no
    /// particular order). Handles and component values don't change, only
    /// the order queries visit them in.
    ///
    /// Use it to put entities that are processed together next to each
    /// other: sorting by a spatial cell key every few dozen ticks makes
    /// neighbor queries hit the CPU cache instead of main memory.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// for x in [3.0, 1.0, 2.0] {
    ///     world.spawn((Pos(x),));
    /// }
    /// world.sort_by::<Pos>(|p| p.0 as u64);
    /// let order: Vec<f32> = world.query::<&Pos>().iter().map(|p| p.0).collect();
    /// assert_eq!(order, [1.0, 2.0, 3.0]);
    /// ```
    pub fn sort_by<T: Component>(&mut self, key: impl Fn(&T) -> u64 + Sync) {
        self.recorder.note(|l| l.structural = true);
        let Some(id) = self.component_id::<T>() else { return };
        let mut keyed: Vec<(u64, u32)> = Vec::new();
        for a in 0..self.archetypes.len() {
            let arch = &self.archetypes[a];
            let Some(base) = arch.column_base(id) else { continue };
            let len = arch.len();
            if len < 2 {
                continue;
            }
            let values = unsafe { std::slice::from_raw_parts(base as *const T, len) };
            keyed.clear();
            keyed.extend(values.iter().enumerate().map(|(row, v)| (key(v), row as u32)));
            if keyed.windows(2).all(|w| w[0].0 <= w[1].0) {
                continue; // already in order
            }
            sort_keys(&mut keyed);
            let perm: Vec<u32> = keyed.iter().map(|&(_, row)| row).collect();
            let arch = &mut self.archetypes[a];
            arch.permute(&perm, self.changes.stamp());
            for (row, &e) in arch.entities.iter().enumerate() {
                self.entities.set_row(e, row);
            }
        }
    }

    // ---- resources -----------------------------------------------------------

    /// Stores a global value (game settings, a random generator, the score...),
    /// replacing any existing value of the same type.
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Score(1));
    /// world.insert_resource(Score(2)); // replaces
    /// assert_eq!(world.resource::<Score>().0, 2);
    /// ```
    pub fn insert_resource<R: Send + Sync + 'static>(&mut self, value: R) {
        self.note_resource::<R>(true);
        self.resources.insert(TypeId::of::<R>(), Box::new(value));
    }

    /// The resource of type `R`.
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Score(7));
    /// assert_eq!(world.resource::<Score>().0, 7);
    /// assert!(world.get_resource::<String>().is_none()); // the checking form
    /// ```
    ///
    /// # Panics
    /// If there is none; use [`get_resource`](Self::get_resource) to check.
    #[track_caller]
    #[must_use]
    pub fn resource<R: Send + Sync + 'static>(&self) -> &R {
        self.get_resource().unwrap_or_else(|| missing_resource::<R>())
    }

    /// Mutable version of [`resource`](Self::resource).
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Score(0));
    /// world.resource_mut::<Score>().0 += 5;
    /// assert_eq!(world.resource::<Score>().0, 5);
    /// ```
    ///
    /// # Panics
    /// If there is none; use [`get_resource_mut`](Self::get_resource_mut) to check.
    #[track_caller]
    pub fn resource_mut<R: Send + Sync + 'static>(&mut self) -> &mut R {
        self.get_resource_mut().unwrap_or_else(|| missing_resource::<R>())
    }

    /// Temporarily takes `R` out of the world, giving the callback mutable
    /// access to both the resource and the rest of the world.
    ///
    /// The original resource (including callback mutations) is restored on
    /// return or panic unwinding. It is absent from world resource lookups
    /// during the callback. A replacement inserted there is discarded when
    /// the original is restored. Scopes for different types may be nested.
    /// The existing box is reused: scoping itself does not allocate in steady
    /// state. Changes made by the callback may still allocate.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// struct Total(u32);
    /// #[derive(Component)]
    /// struct Count(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Total(0));
    /// world.spawn((Count(3),));
    /// world.resource_scope::<Total, _>(|world, total| {
    ///     world.query::<&mut Count>().for_each(|n| {
    ///         n.0 += 1;
    ///         total.0 += n.0;
    ///     });
    /// });
    /// assert_eq!(world.resource::<Total>().0, 4);
    /// ```
    ///
    /// # Panics
    /// If `R` is absent, including a nested scope for the same type unless a
    /// replacement was inserted. As with any destructor, a replacement whose
    /// destructor panics during unwinding causes an abort.
    #[track_caller]
    pub fn resource_scope<R: Send + Sync + 'static, T>(&mut self, f: impl FnOnce(&mut Self, &mut R) -> T) -> T {
        self.note_resource::<R>(true);
        let id = TypeId::of::<R>();
        let value = self.resources.remove(&id).unwrap_or_else(|| missing_resource::<R>());
        let mut scope = ResourceScope {
            world: self,
            id,
            value: Some(value),
        };
        f(scope.world, scope.value.as_mut().unwrap().downcast_mut::<R>().unwrap())
    }

    /// Several resources at once, all mutable: `R` is a tuple of 1 to 8
    /// different resource types, and the result is one `&mut` per type.
    ///
    /// ```
    /// use ponko_ecs::World;
    ///
    /// struct Menu(Vec<&'static str>);
    /// struct Session(&'static str);
    /// let mut world = World::new();
    /// world.insert_resource(Menu(Vec::new()));
    /// world.insert_resource(Session("lobby"));
    ///
    /// let (menu, session) = world.resources_mut::<(Menu, Session)>();
    /// menu.0.push(session.0);
    /// session.0 = "playing";
    /// assert_eq!(world.resource::<Menu>().0, ["lobby"]);
    /// ```
    ///
    /// It doesn't allocate.
    ///
    /// # Panics
    /// If a resource is missing, or `R` names the same type twice.
    #[track_caller]
    pub fn resources_mut<R: crate::borrow::ResourceTuple>(&mut self) -> R::Refs<'_> {
        R::fetch(self)
    }

    /// The resource of type `R`, if there is one.
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// assert!(world.get_resource::<Score>().is_none());
    /// world.insert_resource(Score(1));
    /// assert_eq!(world.get_resource::<Score>().map(|s| s.0), Some(1));
    /// ```
    #[must_use]
    pub fn get_resource<R: Send + Sync + 'static>(&self) -> Option<&R> {
        self.note_resource::<R>(false);
        self.resources
            .get(&TypeId::of::<R>())
            .map(|r| r.downcast_ref().unwrap())
    }

    /// Mutable version of [`get_resource`](Self::get_resource).
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Score(1));
    /// if let Some(score) = world.get_resource_mut::<Score>() {
    ///     score.0 += 1;
    /// }
    /// assert_eq!(world.resource::<Score>().0, 2);
    /// ```
    pub fn get_resource_mut<R: Send + Sync + 'static>(&mut self) -> Option<&mut R> {
        self.note_resource::<R>(true);
        self.resources
            .get_mut(&TypeId::of::<R>())
            .map(|r| r.downcast_mut().unwrap())
    }

    /// Removes and returns the resource of type `R`.
    ///
    /// ```
    /// use ponko_ecs::World;
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.insert_resource(Score(3));
    /// assert_eq!(world.remove_resource::<Score>().unwrap().0, 3);
    /// assert!(world.remove_resource::<Score>().is_none());
    /// ```
    pub fn remove_resource<R: Send + Sync + 'static>(&mut self) -> Option<R> {
        self.note_resource::<R>(true);
        self.resources
            .remove(&TypeId::of::<R>())
            .map(|r| *r.downcast().unwrap())
    }

    /// Implementation of [`RawWorld::insert_resource_dynamic`](crate::RawWorld::insert_resource_dynamic).
    #[track_caller]
    pub(crate) unsafe fn insert_resource_dynamic_inner(&mut self, name: &str, layout: Layout, value: *const u8) {
        self.recorder.note(|l| l.dynamic_resource(name));
        if let Some(old) = self.dynamic_resources.get(name) {
            assert!(
                old.layout == layout,
                "insert_resource_dynamic: `{name}` exists with a different size or alignment"
            );
        }
        // SAFETY: forwarded from the caller.
        let raw = unsafe { RawValue::copy_of(layout, value) };
        self.dynamic_resources.insert(name.to_string(), raw);
    }

    /// Implementation of [`RawWorldRef::get_resource_dynamic`](crate::RawWorldRef::get_resource_dynamic)
    /// and [`get_resource_dynamic_mut`](crate::RawWorld::get_resource_dynamic_mut):
    /// the value is its own allocation, so the pointer allows writing for
    /// callers that hold the world exclusively.
    #[must_use]
    pub(crate) fn get_resource_dynamic_inner(&self, name: &str) -> Option<*mut u8> {
        self.recorder.note(|l| l.dynamic_resource(name));
        self.dynamic_resources.get(name).map(|r| r.ptr.as_ptr())
    }

    /// Implementation of [`RawWorldRef::resource_layout_dynamic`](crate::RawWorldRef::resource_layout_dynamic).
    #[must_use]
    pub(crate) fn resource_layout_dynamic_inner(&self, name: &str) -> Option<Layout> {
        self.dynamic_resources.get(name).map(|r| r.layout)
    }

    /// Implementation of [`RawWorld::remove_resource_dynamic`](crate::RawWorld::remove_resource_dynamic).
    pub(crate) fn remove_resource_dynamic_inner(&mut self, name: &str) -> bool {
        self.recorder.note(|l| l.dynamic_resource(name));
        self.dynamic_resources.remove(name).is_some()
    }

    /// Moves a typed resource out of the map, unrecorded (for views).
    pub(crate) fn lend_resource(&mut self, id: TypeId) -> Option<Box<dyn Any + Send + Sync>> {
        self.resources.remove(&id)
    }

    /// Puts a resource moved out by `lend_resource` back.
    pub(crate) fn return_resource(&mut self, id: TypeId, value: Box<dyn Any + Send + Sync>) {
        self.resources.insert(id, value);
    }

    /// Records resource `R` as read or written, while recording.
    #[inline]
    fn note_resource<R: 'static>(&self, write: bool) {
        self.recorder
            .note(|l| l.resource(TypeId::of::<R>(), std::any::type_name::<R>(), write));
    }

    /// The boxes of the typed resources `ids` (`names` are their type
    /// names, for the panic messages of `caller`), all at once.
    #[track_caller]
    pub(crate) fn resource_boxes<const N: usize>(
        &mut self,
        caller: &str,
        ids: [TypeId; N],
        names: [&'static str; N],
    ) -> [&mut (dyn Any + Send + Sync); N] {
        for i in 0..N {
            if ids[..i].contains(&ids[i]) {
                resource_named_twice(caller, names[i]);
            }
            self.recorder.note(|l| l.resource(ids[i], names[i], true));
        }
        let mut i = 0;
        self.resources.get_disjoint_mut(ids.each_ref()).map(|found| {
            i += 1;
            match found {
                Some(found) => &mut **found,
                None => missing_resource_named(names[i - 1]),
            }
        })
    }

    /// Checks that every resource of `R` is present and named once, and
    /// records the access.
    #[track_caller]
    pub(crate) fn check_resources<R: crate::borrow::ResourceSet>(&self, caller: &str) {
        // A tuple holds 8; nested tuples can name more.
        const MAX: usize = 32;
        let mut seen = [TypeId::of::<()>(); MAX];
        let mut n = 0;
        R::declare(&mut |id, name, write| {
            if seen[..n].contains(&id) {
                resource_named_twice(caller, name);
            }
            if !self.resources.contains_key(&id) {
                missing_resource_named(name);
            }
            assert!(n < MAX, "{caller}: at most {MAX} resources");
            seen[n] = id;
            n += 1;
            self.recorder.note(|l| l.resource(id, name, write));
        });
    }

    /// Moves resource `R` out of the world, keeping its allocation.
    #[track_caller]
    pub(crate) fn take_resource_box<R: Send + Sync + 'static>(&mut self) -> Box<R> {
        let value = self
            .resources
            .remove(&TypeId::of::<R>())
            .unwrap_or_else(|| missing_resource::<R>());
        value
            .downcast()
            .unwrap_or_else(|_| unreachable!("resources are stored by their type"))
    }

    /// Puts back a resource taken with [`take_resource_box`](Self::take_resource_box).
    pub(crate) fn put_resource_box<R: Send + Sync + 'static>(&mut self, value: Box<R>) {
        self.resources.insert(TypeId::of::<R>(), value);
    }

    // ---- access recording ---------------------------------------------------------

    /// Starts recording which components and resources this world hands
    /// out, and whether its structure changes, until
    /// [`take_access_log`](Self::take_access_log). A new call restarts the log.
    ///
    /// Used to check that systems stay within the access they declare.
    /// Recording exists only in builds with debug assertions; in release
    /// builds this does nothing.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Pos(f32);
    /// struct Score(u32);
    /// let mut world = World::new();
    /// world.spawn((Pos(0.0),));
    /// world.insert_resource(Score(0));
    ///
    /// world.record_access();
    /// world.query::<&mut Pos>().for_each(|p| p.0 += 1.0);
    /// let _ = world.resource::<Score>().0;
    /// if let Some(log) = world.take_access_log() {
    ///     let pos = world.component_id::<Pos>().unwrap();
    ///     assert_eq!(log.components, [(pos, true)]);
    ///     assert_eq!(log.resources.len(), 1);
    ///     assert!(!log.structural);
    /// }
    /// ```
    pub fn record_access(&mut self) {
        self.recorder.start();
    }

    /// [`record_access`](Self::record_access), reusing `log`'s buffers: the
    /// log is emptied and filled again, so a checker that hands back the log
    /// it took last time records without allocating once the buffers are
    /// big enough.
    ///
    /// ```
    /// use ponko_ecs::{AccessLog, Component, World};
    /// #[derive(Component)]
    /// struct Pos(f32);
    ///
    /// let mut world = World::new();
    /// world.spawn((Pos(0.0),));
    /// let mut spare = AccessLog::default();
    /// for _ in 0..3 {
    ///     world.record_access_into(std::mem::take(&mut spare));
    ///     world.query::<&Pos>().for_each(|_| {});
    ///     if let Some(log) = world.take_access_log() {
    ///         assert_eq!(log.components.len(), 1, "only this round's access");
    ///         spare = log;
    ///     }
    /// }
    /// ```
    pub fn record_access_into(&mut self, log: AccessLog) {
        self.recorder.start_into(log);
    }

    /// Stops recording and returns what was accessed since
    /// [`record_access`](Self::record_access). `None` if the world wasn't
    /// recording, and always in release builds.
    pub fn take_access_log(&mut self) -> Option<AccessLog> {
        self.recorder.stop()
    }

    // ---- commands --------------------------------------------------------------

    /// An empty command buffer tied to this world: its
    /// [`spawn`](Commands::spawn) reserves the new entity's id here and
    /// returns it right away. Keep it and reuse it; applying empties it.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Ship;
    /// #[derive(Component)]
    /// struct Parent(Entity);
    ///
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// let parent = commands.spawn((Ship,));
    /// let child = commands.spawn((Parent(parent),));
    /// world.apply(&mut commands);
    /// assert_eq!(world.get::<Parent>(child).unwrap().0, parent);
    /// assert!(world.is_alive(parent));
    /// ```
    #[must_use]
    pub fn commands(&self) -> Commands {
        let mut commands = Commands::new();
        commands.bind(self);
        commands
    }

    /// Sizes the set of free slots the world keeps aside for
    /// [`Commands::spawn`], so deferred spawns reuse slots: call it once per
    /// frame (the engine does). It keeps as many as command buffers took in
    /// one frame lately, taken from the free list, and gives the rest back,
    /// so a game that stops spawning through commands doesn't keep them.
    /// "Lately" starts at two calls and doubles whenever buffers need more
    /// right after the set shrank, so a buffer that spawns a burst only now
    /// and then doesn't make the slot table grow either.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Spark;
    ///
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// // Each frame, sparks from the last one go and 100 new ones come.
    /// let mut sparks = Vec::new();
    /// for _ in 0..10 {
    ///     for e in sparks.drain(..) {
    ///         commands.despawn(e);
    ///     }
    ///     sparks.extend((0..100).map(|_| commands.spawn((Spark,))));
    ///     world.apply(&mut commands);
    ///     world.return_idle_ids();
    /// }
    /// // Then the game stops spawning through commands.
    /// for e in sparks {
    ///     world.despawn(e);
    /// }
    /// for _ in 0..2 {
    ///     world.return_idle_ids();
    /// }
    /// // The slots set aside for the next frame's sparks went back:
    /// // `world.spawn` reuses every slot.
    /// let slots = world.entity_slots();
    /// world.spawn_batch((0..slots).map(|_| (Spark,)));
    /// assert_eq!(world.entity_slots(), slots);
    /// ```
    pub fn return_idle_ids(&mut self) {
        self.entities.return_idle();
    }

    /// Spawns an entity at `e`, an id a [`Commands`] buffer of this world
    /// reserved. For the callbacks of `Commands::spawn_with`; everything
    /// else uses [`Commands::spawn`], which calls this.
    ///
    /// # Panics
    ///
    /// If `e` isn't a reserved, not yet spawned id of this world (nothing
    /// is changed then).
    #[doc(hidden)]
    pub fn spawn_reserved<B: Bundle>(&mut self, e: Entity, bundle: B) {
        self.recorder.note(|l| l.structural = true);
        // Everything that can fail first, so a panic leaves `e` reserved
        // for its command to release.
        let a = self.bundle_archetype::<B>();
        self.entities.claim(e);
        self.spawn_in::<B>(a, bundle, e);
    }

    /// Applies commands recorded while the world was busy (e.g. during a query).
    /// Leaves `commands` empty and ready to reuse, tied to this world (so
    /// its [`spawn`](Commands::spawn) reserves ids here from now on).
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Coin;
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// let coin = commands.spawn((Coin,));
    /// let other = commands.spawn((Coin,));
    /// commands.despawn(other);
    /// world.apply(&mut commands);
    /// assert!(world.is_alive(coin) && !world.is_alive(other));
    /// assert!(commands.is_empty(), "the buffer is ready to reuse");
    /// ```
    pub fn apply(&mut self, commands: &mut Commands) {
        self.recorder.note(|l| l.structural = true);
        if self.hooked {
            // Commands that hooks record run after all of these.
            let held = self.hold_hooks();
            commands.apply(held.world);
            return;
        }
        commands.apply(self);
    }

    // ---- components (also the C/C++ path) ---------------------------------------

    /// The id of Rust component `T`, registering it on first use.
    pub fn register<T: Component>(&mut self) -> ComponentId {
        if let Some(&id) = self.by_type.get(&TypeId::of::<T>()) {
            return id;
        }
        let drop = std::mem::needs_drop::<T>().then_some(drop_ptr::<T> as DropFn);
        let id = self.push_info(ComponentInfo {
            name: std::any::type_name::<T>().into(),
            layout: Layout::new::<T>(),
            drop,
            type_id: Some(TypeId::of::<T>()),
            tracked: false,
            clone: T::__snapshot_clone(),
        });
        self.by_type.insert(TypeId::of::<T>(), id);
        T::requires(&mut crate::required::Requires::new(self, id));
        id
    }

    /// Implementation of [`RawWorld::register_component`](crate::RawWorld::register_component).
    pub(crate) unsafe fn register_component_inner(
        &mut self,
        name: &str,
        size: usize,
        align: usize,
        drop: Option<DropFn>,
    ) -> ComponentId {
        let layout = Layout::from_size_align(size, align)
            .expect("invalid component size/alignment; use a nonzero power-of-two alignment and a size fitting Layout");
        if let Some(&id) = self.by_name.get(name) {
            let info = &self.infos[id.index()];
            assert!(
                info.layout == layout && info.type_id.is_none(),
                "component name `{name}` is already registered with a different layout; use its original layout or a unique name"
            );
            return id;
        }
        self.push_info(ComponentInfo {
            name: name.into(),
            layout,
            drop,
            type_id: None,
            tracked: false,
            clone: None,
        })
    }

    fn push_info(&mut self, info: ComponentInfo) -> ComponentId {
        let id = ComponentId(
            u32::try_from(self.infos.len())
                .expect("too many component types; reuse registered types instead of registering more"),
        );
        self.by_name.entry(info.name.clone()).or_insert(id);
        self.infos.push(info);
        id
    }

    /// The id of Rust component `T`, if it has been used in this world.
    #[inline]
    #[must_use]
    pub fn component_id<T: Component>(&self) -> Option<ComponentId> {
        self.by_type.get(&TypeId::of::<T>()).copied()
    }

    /// Implementation of [`RawWorldRef::component_by_name`](crate::RawWorldRef::component_by_name).
    #[must_use]
    pub(crate) fn component_by_name_inner(&self, name: &str) -> Option<ComponentId> {
        self.by_name.get(name).copied()
    }

    /// The component with number `index` ([`ComponentId::index`]), if registered.
    ///
    /// ```
    /// #[derive(ponko_ecs::Component)]
    /// struct Speed(f32);
    ///
    /// let mut world = ponko_ecs::World::new();
    /// world.spawn((Speed(1.5),));
    /// let id = world.component_id::<Speed>().unwrap();
    /// assert_eq!(world.component_at(id.index()), Some(id));
    /// assert_eq!(world.component_at(99), None);
    /// ```
    #[must_use]
    pub fn component_at(&self, index: usize) -> Option<ComponentId> {
        (index < self.infos.len()).then_some(ComponentId(index as u32))
    }

    /// What the world knows about component `id`.
    ///
    /// # Panics
    /// If `id` came from a different world.
    #[must_use]
    pub fn component_info(&self, id: ComponentId) -> &ComponentInfo {
        &self.infos[id.index()]
    }

    /// Every component type registered so far, indexed by [`ComponentId::index`].
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Pos(f32);
    /// let mut world = World::new();
    /// assert!(world.components().is_empty());
    /// let id = world.register::<Pos>();
    /// assert_eq!(world.components().len(), 1);
    /// assert_eq!(world.components()[id.index()].layout().size(), 4);
    /// ```
    #[must_use]
    pub fn components(&self) -> &[ComponentInfo] {
        &self.infos
    }

    /// Implementation of [`RawWorldRef::archetypes`](crate::RawWorldRef::archetypes).
    #[must_use]
    pub(crate) fn archetypes_inner(&self) -> &[Archetype] {
        &self.archetypes
    }

    /// The components entity `e` has (sorted by id), or `None` if it is dead.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Pos(f32);
    /// #[derive(Component)]
    /// struct Vel(f32);
    /// let mut world = World::new();
    /// let e = world.spawn((Pos(0.0), Vel(1.0)));
    /// let names: Vec<&str> = world.components_of(e).unwrap()
    ///     .iter()
    ///     .map(|&id| world.component_info(id).name())
    ///     .collect();
    /// assert_eq!(names.len(), 2);
    /// world.despawn(e);
    /// assert!(world.components_of(e).is_none());
    /// ```
    #[must_use]
    pub fn components_of(&self, e: Entity) -> Option<&[ComponentId]> {
        let loc = self.entities.get(e)?;
        Some(self.archetypes[loc.archetype as usize].component_ids())
    }

    /// Implementation of [`RawWorldRef::get_dynamic`](crate::RawWorldRef::get_dynamic)
    /// (`write: false`, and the caller hands the pointer out as `*const`) and
    /// [`get_dynamic_mut`](crate::RawWorld::get_dynamic_mut) (`write: true`,
    /// for callers that hold the world exclusively).
    #[must_use]
    pub(crate) fn get_dynamic_inner(&self, e: Entity, id: ComponentId, write: bool) -> Option<*mut u8> {
        self.recorder.note(|l| l.component(id, write));
        self.locate(e, id)
    }

    /// Where component `id` of `e` lives, without recording an access.
    pub(crate) fn locate(&self, e: Entity, id: ComponentId) -> Option<*mut u8> {
        let loc = self.entities.get(e)?;
        let arch = &self.archetypes[loc.archetype as usize];
        let col = arch.column_index(id)?;
        Some(arch.columns[col].ptr(loc.row as usize))
    }

    /// Implementation of [`RawWorld::spawn_dynamic`](crate::RawWorld::spawn_dynamic).
    pub(crate) unsafe fn spawn_dynamic_inner(&mut self, components: &[(ComponentId, *const u8)]) -> Entity {
        self.recorder.note(|l| l.structural = true);
        let a = self.dynamic_archetype(components, "spawn_dynamic");
        let e = self.entities.alloc();
        // SAFETY: forwarded caller contract; `e` was just allocated.
        unsafe { self.write_dynamic(a, e, components) };
        e
    }

    /// Implementation of [`RawWorld::spawn_at_dynamic`](crate::RawWorld::spawn_at_dynamic).
    pub(crate) unsafe fn spawn_at_dynamic_inner(&mut self, e: Entity, components: &[(ComponentId, *const u8)]) -> bool {
        self.recorder.note(|l| l.structural = true);
        let a = self.dynamic_archetype(components, "spawn_at_dynamic");
        if !self.entities.alloc_at(e) {
            return false;
        }
        // SAFETY: forwarded caller contract; `e` was just allocated.
        unsafe { self.write_dynamic(a, e, components) };
        true
    }

    /// Implementation of [`RawWorld::spawn_reserved_dynamic`](crate::RawWorld::spawn_reserved_dynamic).
    pub(crate) unsafe fn spawn_reserved_dynamic_inner(&mut self, e: Entity, components: &[(ComponentId, *const u8)]) {
        self.recorder.note(|l| l.structural = true);
        let a = self.dynamic_archetype(components, "spawn_reserved_dynamic");
        self.entities.claim(e);
        // SAFETY: forwarded caller contract; `e` was just claimed.
        unsafe { self.write_dynamic(a, e, components) };
    }

    /// The archetype holding exactly these components (panics on duplicates).
    fn dynamic_archetype(&mut self, components: &[(ComponentId, *const u8)], caller: &str) -> usize {
        let mut ids = std::mem::take(&mut self.scratch_ids);
        ids.clear();
        ids.extend(components.iter().map(|c| c.0));
        ids.sort_unstable();
        let unique = ids.windows(2).all(|w| w[0] != w[1]);
        let a = if unique { Some(self.archetype_for(&ids)) } else { None };
        self.scratch_ids = ids;
        a.unwrap_or_else(|| panic!("{caller}: the same component was given twice; remove the duplicate component"))
    }

    /// Appends freshly allocated `e` to archetype `a` with these values.
    ///
    /// # Safety
    /// `a` is the archetype of exactly these ids, `e` is allocated and has no
    /// location yet, and each pointer points to a valid value of its component.
    unsafe fn write_dynamic(&mut self, a: usize, e: Entity, components: &[(ComponentId, *const u8)]) {
        if self.hooked && self.hooks.requires(a) {
            // SAFETY: forwarded caller contract.
            return unsafe { self.write_dynamic_required(a, e, components) };
        }
        let arch = &mut self.archetypes[a];
        arch.reserve_row(&self.changes, &mut self.slots.buffers);
        let row = arch.len();
        for &(id, src) in components {
            let c = &arch.columns[arch.column_index(id).unwrap()];
            // SAFETY: the caller's value fits the column (same component), and
            // `row` is reserved spare capacity.
            unsafe { std::ptr::copy_nonoverlapping(src, c.ptr(row), c.size()) };
        }
        arch.entities.push(e);
        if row == 0 {
            self.slots.filled(a);
        }
        self.entities.set(
            e,
            Location {
                archetype: a as u32,
                row: row as u32,
            },
        );
        if self.hooked && self.hooks.adds(a) {
            self.run_spawn_hooks(a, e);
        }
    }

    /// [`write_dynamic`](Self::write_dynamic) where a component of `a`
    /// requires others: `e` goes to the archetype with the ones the given
    /// components lack, made first.
    ///
    /// # Safety
    /// As [`write_dynamic`](Self::write_dynamic).
    #[cold]
    #[inline(never)]
    unsafe fn write_dynamic_required(&mut self, a: usize, e: Entity, components: &[(ComponentId, *const u8)]) {
        let order: Vec<ComponentId> = components.iter().map(|c| c.0).collect();
        let mut missing = Vec::new();
        self.hooks.missing(&order, &self.archetypes[a].ids, &mut missing);
        let a = if missing.is_empty() {
            a
        } else {
            let mut ids = self.archetypes[a].ids.to_vec();
            ids.extend(missing.iter().map(|m| m.0));
            ids.sort_unstable();
            self.archetype_for(&ids)
        };
        let arch = &mut self.archetypes[a];
        arch.reserve_row(&self.changes, &mut self.slots.buffers);
        let row = arch.len();
        let column = |arch: &Archetype, id: ComponentId| {
            arch.column_index(id)
                .expect("invariant: the archetype was made with every component given and required")
        };
        // If a constructor panics, `e` goes back; the values made before
        // it are leaked, never dropped twice.
        let unplaced = Unplaced {
            entities: &mut self.entities,
            e,
        };
        for &(id, make) in &missing {
            let c = &arch.columns[column(arch, id)];
            // SAFETY: `row` is reserved spare capacity of the column of the
            // component `make` makes.
            unsafe { make.write(c.ptr(row), c.size()) };
        }
        std::mem::forget(unplaced);
        for &(id, src) in components {
            let c = &arch.columns[column(arch, id)];
            // SAFETY: as in `write_dynamic`.
            unsafe { std::ptr::copy_nonoverlapping(src, c.ptr(row), c.size()) };
        }
        arch.entities.push(e);
        self.placed(a, row, e);
        if self.hooks.adds(a) {
            self.run_spawn_hooks(a, e);
        }
    }

    /// Implementation of [`RawWorld::insert_dynamic`](crate::RawWorld::insert_dynamic).
    pub(crate) unsafe fn insert_dynamic_inner(&mut self, e: Entity, id: ComponentId, value: *const u8) -> bool {
        let Some(loc) = self.entities.get(e) else {
            self.recorder.note(|l| l.structural = true);
            return false;
        };
        let src = loc.archetype as usize;
        // Replacing a component in place is a write, not a structural change.
        // (Looked up inside, so release builds, which don't record, skip it.)
        self.recorder.note(|l| {
            if self.archetypes[src].column_index(id).is_some() {
                l.component(id, true);
            } else {
                l.structural = true;
            }
        });
        if let Some(col) = self.archetypes[src].column_index(id) {
            if self.hooked && self.hooks.replaces_component(id) {
                // SAFETY: forwarded caller contract.
                return unsafe { self.replace_hooked(e, id, value) };
            }
            let arch = &self.archetypes[src];
            let c = &arch.columns[col];
            let dst = c.ptr(loc.row as usize);
            unsafe {
                if let Some(d) = c.drop_fn() {
                    d(dst);
                }
                std::ptr::copy_nonoverlapping(value, dst, c.size());
            }
            if arch.tracked() {
                // Replacing a value changes it; it was added when it first arrived.
                arch.stamp(col, loc.row as usize, self.changes.stamp(), false);
            }
            return true;
        }
        // Read once: nothing below registers hooks or requirements.
        let hooked = self.hooked;
        if hooked && self.hooks.requires_component(id) {
            // SAFETY: forwarded caller contract; `e` lacks `id`.
            return unsafe { self.insert_arriving(e, loc, id, value) };
        }
        let dst = self.archetype_adding(src, id);
        let row = self.move_row(e, loc, dst);
        let arch = &self.archetypes[dst];
        let col = arch.column_index(id).unwrap();
        let c = &arch.columns[col];
        unsafe { std::ptr::copy_nonoverlapping(value, c.ptr(row), c.size()) };
        if arch.tracked() {
            arch.stamp(col, row, self.changes.stamp(), true);
        }
        if hooked && self.hooks.adds_component(id) {
            self.added_hooked(e, id);
        }
        true
    }

    /// The end of an insert that gave `e` component `id`, which has
    /// `on_add` hooks: runs them, then their commands.
    #[cold]
    #[inline(never)]
    fn added_hooked(&mut self, e: Entity, id: ComponentId) {
        self.run_hooks(id, e, crate::hooks::Kind::Add);
        self.flush_hooks();
    }

    /// An insert that gives `e` (at `loc`) component `id`, which it lacks,
    /// and which requires others: `e` gets `id` and
    /// the components it requires and `e` lacks in one move, then the
    /// `on_add` hooks of all of them run (in component-id order), then
    /// their commands.
    ///
    /// # Safety
    /// As [`insert_dynamic_inner`](Self::insert_dynamic_inner); `e` is
    /// alive at `loc` and lacks `id`.
    #[cold]
    #[inline(never)]
    unsafe fn insert_arriving(&mut self, e: Entity, loc: Location, id: ComponentId, value: *const u8) -> bool {
        let src = loc.archetype as usize;
        let mut missing = Vec::new();
        let mut ids = self.archetypes[src].ids.to_vec();
        ids.push(id);
        self.hooks.missing(&[id], &ids, &mut missing);
        // The required values first, so a constructor that panics leaves
        // `e` as it was (the values made before it are leaked).
        let mut made: Vec<(ComponentId, TempValue)> = missing
            .iter()
            .map(|&(rid, make)| {
                let layout = self.infos[rid.index()].layout;
                let mut temp = TempValue::new(layout);
                // SAFETY: `temp` has room for a value of `rid`, which `make` makes.
                unsafe { make.write(temp.ptr(), layout.size()) };
                (rid, temp)
            })
            .collect();
        let edge = self.archetypes[src]
            .add_edges
            .get(&id)
            .and_then(|&e| self.slots.leads(e));
        let dst = match edge {
            // Only `id` arrives: the cached edge, as a plain insert takes.
            Some(d) if missing.is_empty() => d,
            _ => {
                ids.extend(missing.iter().map(|m| m.0));
                ids.sort_unstable();
                let d = self.archetype_for(&ids);
                if missing.is_empty() {
                    self.add_edge(src, id, d);
                }
                d
            }
        };
        let row = self.move_row(e, loc, dst);
        let arch = &self.archetypes[dst];
        let stamp = self.changes.stamp();
        let put = |cid: ComponentId, from: *const u8| {
            let col = arch
                .column_index(cid)
                .expect("invariant: the archetype was made with every arriving component");
            let c = &arch.columns[col];
            // SAFETY: `row` is `e`'s new row, whose column `col` the move
            // left uninitialized; `from` is a valid value of `cid`, moved in.
            unsafe { std::ptr::copy_nonoverlapping(from, c.ptr(row), c.size()) };
            if arch.tracked() {
                arch.stamp(col, row, stamp, true);
            }
        };
        put(id, value);
        for (rid, temp) in &mut made {
            put(*rid, temp.ptr());
        }
        // The temporaries only free their room: the values are in the world now.
        drop(made);
        if missing.is_empty() {
            // Only `id` arrived (no allocation on this path).
            if self.hooks.adds_component(id) {
                self.run_hooks(id, e, crate::hooks::Kind::Add);
                self.flush_hooks();
            }
        } else if self.hooks.adds_component(id) || missing.iter().any(|m| self.hooks.adds_component(m.0)) {
            let mut arrived: Vec<ComponentId> = missing.iter().map(|m| m.0).collect();
            arrived.push(id);
            arrived.sort_unstable();
            for cid in arrived {
                self.run_hooks(cid, e, crate::hooks::Kind::Add);
            }
            self.flush_hooks();
        }
        true
    }

    /// An insert that replaces `e`'s component `id`, which has
    /// `on_replace` hooks: they run with the new value at `value`, then the
    /// old value is dropped and the new one written, then their commands.
    ///
    /// # Safety
    /// As [`insert_dynamic_inner`](Self::insert_dynamic_inner); `e` is alive
    /// and has `id`.
    #[cold]
    #[inline(never)]
    unsafe fn replace_hooked(&mut self, e: Entity, id: ComponentId, value: *const u8) -> bool {
        // SAFETY: the caller's value is a valid one of component `id`,
        // outside the world.
        unsafe { self.run_replace_hooks(id, e, value) };
        // Hooks can't change the world's structure; looked up again all the same.
        let Some(loc) = self.entities.get(e) else { return false };
        let arch = &self.archetypes[loc.archetype as usize];
        let Some(col) = arch.column_index(id) else { return false };
        let c = &arch.columns[col];
        let dst = c.ptr(loc.row as usize);
        // SAFETY: as in `insert_dynamic_inner`: the old value is dropped,
        // then overwritten with the caller's.
        unsafe {
            if let Some(d) = c.drop_fn() {
                d(dst);
            }
            std::ptr::copy_nonoverlapping(value, dst, c.size());
        }
        if arch.tracked() {
            arch.stamp(col, loc.row as usize, self.changes.stamp(), false);
        }
        self.flush_hooks();
        true
    }

    /// Implementation of [`RawWorld::remove_dynamic`](crate::RawWorld::remove_dynamic).
    pub(crate) fn remove_dynamic_inner(&mut self, e: Entity, id: ComponentId) -> bool {
        self.recorder.note(|l| l.structural = true);
        if self.hooked && self.hooks.removes_component(id) && self.locate(e, id).is_some() {
            // The hooks run first, while `e` still has it; their commands
            // after it went.
            self.run_hooks(id, e, crate::hooks::Kind::Remove);
            let removed = self.remove_dynamic_now(e, id);
            self.flush_hooks();
            return removed;
        }
        self.remove_dynamic_now(e, id)
    }

    /// [`remove_dynamic_inner`](Self::remove_dynamic_inner) after the hooks.
    fn remove_dynamic_now(&mut self, e: Entity, id: ComponentId) -> bool {
        let Some(loc) = self.entities.get(e) else { return false };
        let arch = &self.archetypes[loc.archetype as usize];
        let Some(col) = arch.column_index(id) else { return false };
        let c = &arch.columns[col];
        let Some(drop) = c.drop_fn() else {
            self.move_without(e, loc, id);
            return true;
        };
        // Move the value out first, then drop it: if drop panics, the world is consistent.
        let ptr = c.ptr(loc.row as usize);
        let mut tmp = TempValue::new(self.infos[id.index()].layout);
        unsafe { std::ptr::copy_nonoverlapping(ptr, tmp.ptr(), c.size()) };
        self.move_without(e, loc, id);
        unsafe { drop(tmp.ptr()) };
        true
    }

    /// Implementation of [`RawWorld::for_each_chunk`](crate::RawWorld::for_each_chunk).
    pub(crate) fn for_each_chunk_inner(&mut self, ids: &[ComponentId], mut f: impl FnMut(Chunk<'_>)) {
        const MAX: usize = 32;
        assert!(ids.len() <= MAX, "for_each_chunk: at most {MAX} components");
        self.recorder.note(|l| ids.iter().for_each(|&id| l.component(id, true)));
        let mut ptrs = [std::ptr::null_mut(); MAX];
        let mut archetypes = self.query_matches.chunk_archetypes(
            &self.archetypes,
            ids,
            None,
            crate::QueryCache::Auto,
            self.slots.scan(),
        );
        'arch: while let Some((arch, columns)) = archetypes.next_with_columns() {
            if arch.is_empty() {
                continue;
            }
            for (i, &id) in ids.iter().enumerate() {
                match crate::query::column_with_hint(arch, columns, id).and_then(|column| arch.column_base_at(column)) {
                    Some(p) => ptrs[i] = p,
                    None => continue 'arch,
                }
            }
            if arch.tracked() {
                // The column pointers allow writing: every row counts as changed.
                crate::change::mark_columns(arch, ids, self.changes.stamp());
            }
            f(Chunk {
                entities: &arch.entities,
                columns: &ptrs[..ids.len()],
            });
        }
    }

    // ---- internals -------------------------------------------------------------

    /// The archetype of exactly `ids` (sorted and unique), created if there
    /// is none: in the slot of a reclaimed archetype if there is one, and in
    /// the storage of one as wide if one is kept.
    fn archetype_for(&mut self, ids: &[ComponentId]) -> usize {
        if let Some(&a) = self.archetype_by_ids.get(ids) {
            return a;
        }
        let (arch, key) = match self.slots.shell(ids.len()) {
            Some(shell) => Archetype::reuse(shell, ids, &self.infos),
            None => (Archetype::new(ids.into(), &self.infos), ids.into()),
        };
        let a = match self.slots.free_slot() {
            Some(a) => {
                self.archetypes[a] = arch;
                // Indexed query signatures learn what the slot holds now.
                self.query_matches.reused(a, &self.archetypes);
                a
            }
            None => {
                self.archetypes.push(arch);
                self.archetypes.len() - 1
            }
        };
        self.slots.created(a);
        if self.hooked {
            self.hooks.archetype_created(a, &self.archetypes[a].ids);
        }
        self.archetype_by_ids.insert(key, a);
        self.new_layout();
        a
    }

    /// Forgets where each bundle type spawns: the archetype slots were laid
    /// out again. Each is placed again at its next spawn.
    pub(crate) fn forget_bundles(&mut self) {
        self.bundles.clear();
    }

    /// The archetype slots were laid out anew: see [`World::layout`].
    #[inline]
    fn new_layout(&mut self) {
        self.layouts += 1;
        self.layout = self.layouts;
    }

    /// Records that adding component `id` to archetype `src` leads to `dst`,
    /// and removing it from `dst` back to `src`, replacing stale edges.
    ///
    /// An edge map holds at most one edge per component: an add map one for
    /// each registered component its archetype lacks, a remove map one for
    /// each it has. A full map whose bound is small grows to it at once,
    /// rather than doubling: in a world that reclaims archetypes, edges keep
    /// going (with the archetype they lead to) and coming back, and a map
    /// that grew a step at a time would allocate again whenever its
    /// archetype reached a new most edges.
    fn add_edge(&mut self, src: usize, id: ComponentId, dst: usize) {
        const AT_ONCE: usize = 32;
        let registered = self.infos.len();
        let (to_dst, to_src) = (self.slots.edge(dst), self.slots.edge(src));
        let s = &mut self.archetypes[src];
        let most = registered - s.ids.len();
        if s.add_edges.len() == s.add_edges.capacity() && most <= AT_ONCE {
            s.add_edges.reserve(most - s.add_edges.len());
        }
        s.add_edges.insert(id, to_dst);
        let d = &mut self.archetypes[dst];
        let most = d.ids.len();
        if d.remove_edges.len() == d.remove_edges.capacity() && most <= AT_ONCE {
            d.remove_edges.reserve(most - d.remove_edges.len());
        }
        d.remove_edges.insert(id, to_src);
    }

    /// Reclaims the archetypes that have been empty the longest while the
    /// world keeps more empty ones than its policy allows (see `slots.rs`).
    /// Called right after an archetype lost its last entity, at the end of
    /// a structural change: nothing holds an archetype index across it.
    #[inline]
    fn reclaim_if_over(&mut self) {
        if self.slots.over() {
            self.reclaim_empty();
        }
    }

    #[cold]
    #[inline(never)]
    fn reclaim_empty(&mut self) {
        while self.slots.over() {
            let Some(a) = self.slots.next_empty() else { break };
            self.reclaim(a);
        }
    }

    /// Turns empty archetype `a` into a dead slot: forgets its component
    /// set and keeps its storage for the next new archetype. Its neighbours'
    /// edges to it go stale with its generation (`Slots::leads`), so they
    /// aren't touched here.
    fn reclaim(&mut self, a: usize) {
        let key = self.archetype_by_ids.remove_entry(&self.archetypes[a].ids[..]);
        debug_assert_eq!(key.as_ref().map(|k| k.1), Some(a));
        let mut shell = self.archetypes[a].retire(&mut self.slots.buffers);
        shell.key = key.map(|k| k.0);
        self.slots.reclaimed(a, shell);
        self.new_layout();
    }

    fn bundle_archetype<B: Bundle>(&mut self) -> usize {
        if let Some(b) = self.bundles.get(&TypeId::of::<B>()) {
            return b.archetype;
        }
        let mut ids = Vec::new();
        B::component_ids(self, &mut ids);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            ids.len(),
            "bundle `{}` has the same component twice; remove the duplicate component",
            std::any::type_name::<B>()
        );
        self.place_bundle(TypeId::of::<B>(), &ids)
    }

    /// Caches where bundle `key`, of components `ids` (in its order, no
    /// duplicates), spawns: the archetype of those and the components they
    /// require, tagged ([`HOOKED_BUNDLE`]) if that has `on_add` hooks or
    /// the bundle lacks a required component. Returns the tagged index.
    fn place_bundle(&mut self, key: TypeId, ids: &[ComponentId]) -> usize {
        let mut all = ids.to_vec();
        let mut missing = Vec::new();
        if self.hooked {
            self.hooks.missing(ids, &all, &mut missing);
            all.extend(missing.iter().map(|m| m.0));
        }
        all.sort_unstable();
        let a = self.archetype_for(&all);
        // The bundle remembers its archetype: never reclaim it.
        self.slots.pin(a);
        let arch = &self.archetypes[a];
        let column = |id: ComponentId| {
            arch.column_index(id)
                .expect("invariant: the archetype was made with every component of the bundle")
        };
        let columns = ids.iter().map(|&id| column(id)).collect();
        let fills: Box<[_]> = missing.iter().map(|&(id, make)| (column(id), make)).collect();
        let adds = self.hooks.adds(a);
        let tagged = !fills.is_empty() || adds;
        let archetype = if tagged { a | HOOKED_BUNDLE } else { a };
        self.bundles.insert(
            key,
            BundleInfo {
                archetype,
                columns,
                fills,
                adds,
            },
        );
        archetype
    }

    /// Places every cached bundle again ([`place_bundle`](Self::place_bundle)),
    /// after a hook or a requirement was registered: requirements change
    /// the archetype, hooks the tag. In the order of their archetypes, so
    /// new archetypes get the same indices in every run.
    pub(crate) fn rebuild_bundles(&mut self) {
        let mut cached: Vec<(usize, TypeId, Vec<ComponentId>)> = self
            .bundles
            .iter()
            .map(|(&key, info)| {
                let arch = &self.archetypes[info.archetype & !HOOKED_BUNDLE];
                let ids = info.columns.iter().map(|&c| arch.ids[c]).collect();
                (info.archetype & !HOOKED_BUNDLE, key, ids)
            })
            .collect();
        cached.sort_unstable_by_key(|c| c.0);
        for (_, key, ids) in cached {
            self.place_bundle(key, &ids);
        }
    }

    /// Moves `e` to the archetype without component `id`. The value of `id`
    /// is forgotten: the caller must have read or copied it out.
    fn move_without(&mut self, e: Entity, loc: Location, id: ComponentId) {
        let src = loc.archetype as usize;
        if self.archetypes[src].tracked() && self.infos[id.index()].tracked {
            self.changes.log_removal(id, e);
        }
        let dst = self.archetype_removing(src, id);
        self.move_row(e, loc, dst);
    }

    /// The archetype `src` leads to when component `id`, which it lacks,
    /// arrives: the cached edge, or the archetype found or made (and the
    /// edge recorded). Creating an archetype can reclaim none: indices stay.
    fn archetype_adding(&mut self, src: usize, id: ComponentId) -> usize {
        if let Some(d) = self.archetypes[src]
            .add_edges
            .get(&id)
            .and_then(|&e| self.slots.leads(e))
        {
            return d;
        }
        let mut ids = std::mem::take(&mut self.scratch_ids);
        ids.clear();
        ids.extend_from_slice(&self.archetypes[src].ids);
        ids.push(id);
        ids.sort_unstable();
        let d = self.archetype_for(&ids);
        self.scratch_ids = ids;
        self.add_edge(src, id, d);
        d
    }

    /// The archetype `src` leads to when component `id`, which it has,
    /// goes: as [`archetype_adding`](Self::archetype_adding), the other way.
    fn archetype_removing(&mut self, src: usize, id: ComponentId) -> usize {
        if let Some(d) = self.archetypes[src]
            .remove_edges
            .get(&id)
            .and_then(|&e| self.slots.leads(e))
        {
            return d;
        }
        let mut ids = std::mem::take(&mut self.scratch_ids);
        ids.clear();
        ids.extend(self.archetypes[src].ids.iter().copied().filter(|&c| c != id));
        let d = self.archetype_for(&ids);
        self.scratch_ids = ids;
        self.add_edge(d, id, src);
        d
    }

    /// Moves `e`'s row to archetype `dst`, copying shared columns and
    /// forgetting the rest. Returns the new row; columns of `dst` that the
    /// source lacks are left uninitialized for the caller to fill.
    fn move_row(&mut self, e: Entity, loc: Location, dst: usize) -> usize {
        let src = loc.archetype as usize;
        let row = loc.row as usize;
        let (s, d) = two_mut(&mut self.archetypes, src, dst);
        d.reserve(1, &mut self.slots.buffers);
        let new_row = d.len();
        let last = s.len() - 1;
        // Both id lists are sorted, so one merge pass finds each source column
        // in the destination (`j` only moves forward): O(n), not a scan per column.
        let mut j = 0;
        for (i, col) in s.columns.iter_mut().enumerate() {
            let id = s.ids[i];
            while j < d.ids.len() && d.ids[j] < id {
                j += 1;
            }
            if j < d.ids.len() && d.ids[j] == id {
                unsafe { std::ptr::copy_nonoverlapping(col.ptr(row), d.columns[j].ptr(new_row), col.size()) };
            }
            unsafe { col.swap_remove_forget(row, last) };
        }
        if s.tracked() {
            // The components the entity keeps take their stamps along.
            let now = self.changes.stamp();
            s.copy_stamps_to(row, d, new_row, now);
            s.swap_remove_stamps(row, last, now);
        }
        s.entities.swap_remove(row);
        if row != last {
            let moved = s.entities[row];
            self.entities.set_row(moved, row);
        }
        d.entities.push(e);
        self.entities.set(
            e,
            Location {
                archetype: dst as u32,
                row: new_row as u32,
            },
        );
        if new_row == 0 {
            self.slots.filled(dst);
        }
        if last == 0 {
            self.slots.emptied(src);
            self.reclaim_if_over();
        }
        new_row
    }
}

fn sort_keys(keyed: &mut [(u64, u32)]) {
    #[cfg(feature = "parallel")]
    if keyed.len() > 50_000 {
        use rayon::slice::ParallelSliceMut;
        crate::par::run(|| keyed.par_sort_unstable_by_key(|k| k.0));
        return;
    }
    keyed.sort_unstable_by_key(|k| k.0);
}

#[cold]
#[track_caller]
fn missing_resource<R>() -> ! {
    panic!(
        "no resource of type `{0}`; add one first with `world.insert_resource(...)`",
        std::any::type_name::<R>()
    )
}

#[cold]
#[track_caller]
fn missing_resource_named(name: &str) -> ! {
    panic!("no resource of type `{name}`; add one first with `world.insert_resource(...)`")
}

#[cold]
#[track_caller]
fn resource_named_twice(caller: &str, name: &str) -> ! {
    panic!("{caller}: resource `{name}` is named twice; each resource can be borrowed once")
}

fn two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b);
    if a < b {
        let (l, r) = v.split_at_mut(b);
        (&mut l[a], &mut r[0])
    } else {
        let (l, r) = v.split_at_mut(a);
        (&mut r[0], &mut l[b])
    }
}

/// Frees entity `e`, allocated and not yet placed, if dropped: while the
/// required values of a spawn are made, so a constructor that panics
/// leaves no id behind that is neither alive nor free.
struct Unplaced<'a> {
    entities: &'a mut Entities,
    e: Entity,
}

impl Drop for Unplaced<'_> {
    fn drop(&mut self) {
        self.entities.free(self.e);
    }
}

/// Room for one value of a runtime layout, freed without dropping the value.
/// On the stack when it fits [`TempInline`] (most components), so that
/// removing a component with a destructor doesn't allocate; on the heap
/// otherwise.
struct TempValue {
    inline: std::mem::MaybeUninit<TempInline>,
    /// Null unless the value needed the heap.
    heap: *mut u8,
    layout: Layout,
}

#[repr(C, align(16))]
struct TempInline([u8; 64]);

impl TempValue {
    fn new(layout: Layout) -> Self {
        let inline = layout.size() <= size_of::<TempInline>() && layout.align() <= align_of::<TempInline>();
        let heap = if layout.size() == 0 || inline {
            std::ptr::null_mut()
        } else {
            // SAFETY: the layout has a non-zero size.
            let p = unsafe { std::alloc::alloc(layout) };
            if p.is_null() {
                std::alloc::handle_alloc_error(layout);
            }
            p
        };
        TempValue {
            inline: std::mem::MaybeUninit::uninit(),
            heap,
            layout,
        }
    }

    /// Where the value goes. Computed on each call (not stored), so the
    /// inline buffer may move with `self`.
    fn ptr(&mut self) -> *mut u8 {
        if self.layout.size() == 0 {
            std::ptr::without_provenance_mut(self.layout.align())
        } else if self.heap.is_null() {
            self.inline.as_mut_ptr().cast()
        } else {
            self.heap
        }
    }
}

impl Drop for TempValue {
    fn drop(&mut self) {
        if !self.heap.is_null() {
            // SAFETY: allocated in `new` with this layout.
            unsafe { std::alloc::dealloc(self.heap, self.layout) }
        }
    }
}
