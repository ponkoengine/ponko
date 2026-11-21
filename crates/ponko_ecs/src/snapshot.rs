//! Typed in-memory world snapshots: [`World::save_snapshot`] and
//! [`World::restore_snapshot`], for rollback netcode and anything else that
//! must go back to an earlier state of the same world and go on from there
//! exactly as it did the first time.
//!
//! A snapshot copies, per archetype, its entities and every column (a plain
//! byte copy for types without a destructor, `Clone` for the others), the
//! entity allocator (free list, generations, the ids command buffers will
//! get) and the layout of the archetype slots. Restoring puts all of it
//! back, so the same entities have the same ids, the next spawns get the
//! ids they got after the save, and queries visit entities in the order they
//! did then. Nothing is serialized and, once a snapshot has held a world of
//! this size, saving into it again allocates nothing for plain components.
//!
//! **Why the slot layout.** Queries visit archetypes in slot order. A world
//! that went on past the save may have made or reclaimed archetypes; a
//! restore that only put the rows back would leave those, and the next new
//! archetype could land in another slot than it did the first time, so a
//! simulation run again from the restore could visit entities in another
//! order than a peer that never restored. When the layout changed since the
//! save, a restore lays the slots out again as they were (and drops the
//! query index and the transition edges, which only cost speed).

use std::alloc::Layout;
use std::any::{Any, TypeId};
use std::marker::PhantomData;
use std::ptr::NonNull;

use crate::bundle::Component;
use crate::entity::{EntitiesState, Entity};
use crate::slots::SlotsState;
use crate::storage::{Archetype, ComponentId, DropFn};
use crate::world::World;

/// Clones `count` values from `src` into the uninitialized `dst`.
#[doc(hidden)]
pub type CloneFn = unsafe fn(src: *const u8, dst: *mut u8, count: usize);

/// Clones `count` `T`s from `src` into uninitialized memory at `dst`. If a
/// clone panics, the ones made before it are leaked, never dropped twice.
///
/// # Safety
/// `src` holds `count` initialized `T`s, `dst` has room for `count` `T`s,
/// both aligned for `T`, and they don't overlap.
unsafe fn clone_values<T: Clone>(src: *const u8, dst: *mut u8, count: usize) {
    let (src, dst) = (src.cast::<T>(), dst.cast::<T>());
    for i in 0..count {
        // SAFETY: the caller's contract; `i < count`.
        unsafe { dst.add(i).write((*src.add(i)).clone()) };
    }
}

/// What `#[derive(Component)]` asks to learn whether a type is `Clone`
/// ("autoref specialization": `(&&Probe::<T>::NEW).clone_fn()` picks
/// [`ViaClone`] when `T: Clone`, else [`NotClone`]). Not part of the API.
#[doc(hidden)]
pub struct Probe<T>(PhantomData<fn() -> T>);

impl<T> Probe<T> {
    #[doc(hidden)]
    pub const NEW: Self = Probe(PhantomData);
}

#[doc(hidden)]
pub trait ViaClone {
    #[doc(hidden)]
    fn clone_fn(&self) -> Option<CloneFn>;
}

impl<T: Clone + Send + Sync + 'static> ViaClone for &Probe<T> {
    fn clone_fn(&self) -> Option<CloneFn> {
        Some(clone_values::<T>)
    }
}

#[doc(hidden)]
pub trait NotClone {
    #[doc(hidden)]
    fn clone_fn(&self) -> Option<CloneFn>;
}

impl<T> NotClone for Probe<T> {
    fn clone_fn(&self) -> Option<CloneFn> {
        None
    }
}

/// Why a world couldn't be saved into, or restored from, a
/// [`WorldSnapshot`].
///
/// ```
/// use ponko_ecs::{Component, SnapshotError, World, WorldSnapshot};
///
/// #[derive(Component)]
/// struct Socket(u32); // not Clone
///
/// let mut world = World::new();
/// world.spawn((Socket(3),));
/// let mut snapshot = WorldSnapshot::new();
/// let err = world.save_snapshot(&mut snapshot).unwrap_err();
/// assert!(matches!(err, SnapshotError::NotCloneable { .. }));
/// assert!(err.to_string().contains("Socket"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SnapshotError {
    /// A component the world holds can't be copied: a Rust type that isn't
    /// `Clone` (derive `Clone`, or call
    /// [`World::register_clone`] for a hand-written `impl Component`), or a
    /// C or C++ component with a destructor.
    NotCloneable {
        /// The component's name ([`ComponentInfo::name`](crate::ComponentInfo::name)).
        component: String,
    },
    /// The snapshot was saved from another world (or never saved): a
    /// snapshot only restores the world it was saved from.
    OtherWorld,
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::NotCloneable { component } => write!(
                f,
                "component `{component}` can't be copied into a snapshot; derive `Clone` for it \
                 (or call `World::register_clone` for a hand-written `impl Component`)"
            ),
            SnapshotError::OtherWorld => f.write_str(
                "the snapshot was saved from another world (or never saved); restore a world only from its own snapshots",
            ),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// One column of a saved archetype: `cap` values' worth of memory, of which
/// the table's first `len` rows are initialized.
struct SavedColumn {
    id: ComponentId,
    data: NonNull<u8>,
    item: Layout,
    cap: usize,
    drop: Option<DropFn>,
    clone: Option<CloneFn>,
}

impl SavedColumn {
    fn new(id: ComponentId, item: Layout, drop: Option<DropFn>, clone: Option<CloneFn>) -> Self {
        SavedColumn {
            id,
            // Dangling but aligned: valid for zero-sized items and no rows.
            data: NonNull::new(std::ptr::without_provenance_mut(item.align()))
                .expect("invariant: an alignment is never zero"),
            item,
            cap: 0,
            drop,
            clone,
        }
    }

    fn array(&self, rows: usize) -> Layout {
        Layout::from_size_align(self.item.size() * rows, self.item.align())
            .expect("too much component data for one snapshot column")
    }

    /// Drops the first `len` values.
    ///
    /// # Safety
    /// The first `len` values are initialized, and are not used again.
    unsafe fn drop_values(&mut self, len: usize) {
        if let Some(drop) = self.drop {
            for row in 0..len {
                // SAFETY: an initialized value of this column's type.
                unsafe { drop(self.data.as_ptr().add(row * self.item.size())) };
            }
        }
    }

    /// Room for `rows` values; the old ones must have been dropped.
    fn reserve(&mut self, rows: usize) {
        if rows <= self.cap || self.item.size() == 0 {
            self.cap = self.cap.max(rows);
            return;
        }
        let new_cap = rows.max(self.cap * 2).max(8);
        let layout = self.array(new_cap);
        // SAFETY: the size is non-zero (non-zero items, at least 8 rows).
        let p = unsafe { std::alloc::alloc(layout) };
        let p = NonNull::new(p).unwrap_or_else(|| std::alloc::handle_alloc_error(layout));
        self.free();
        self.data = p;
        self.cap = new_cap;
    }

    fn free(&mut self) {
        if self.cap > 0 && self.item.size() > 0 {
            // SAFETY: allocated by `reserve` with this layout.
            unsafe { std::alloc::dealloc(self.data.as_ptr(), self.array(self.cap)) };
        }
        self.cap = 0;
        self.data = NonNull::new(std::ptr::without_provenance_mut(self.item.align()))
            .expect("invariant: an alignment is never zero");
    }

    /// Copies `rows` values from `src` into this column, whose old values
    /// were dropped: a clone each, or the bytes.
    ///
    /// # Safety
    /// `src` holds `rows` initialized values of this column's type, and
    /// `reserve(rows)` was called.
    unsafe fn copy_from(&mut self, src: *const u8, rows: usize) {
        // SAFETY: the caller's contract; the snapshot's memory and the
        // world's never overlap.
        unsafe { copy_values(self.clone, src, self.data.as_ptr(), rows, self.item.size()) };
    }
}

/// Copies `rows` values of `size` bytes from `src` to uninitialized `dst`:
/// with `clone` if there is one, else as bytes.
///
/// # Safety
/// `src` holds `rows` initialized values, `dst` has room for them, they
/// don't overlap, and values without `clone` are plain bytes (checked by the
/// caller with [`ComponentInfo::can_snapshot`](crate::ComponentInfo::can_snapshot)).
unsafe fn copy_values(clone: Option<CloneFn>, src: *const u8, dst: *mut u8, rows: usize, size: usize) {
    match clone {
        // SAFETY: the caller's contract.
        Some(clone) => unsafe { clone(src, dst, rows) },
        // SAFETY: the caller's contract.
        None => unsafe { std::ptr::copy_nonoverlapping(src, dst, rows * size) },
    }
}

/// One archetype slot as saved: its component set (`None` for a dead
/// slot) and its rows.
#[derive(Default)]
struct SavedTable {
    ids: Option<Box<[ComponentId]>>,
    len: usize,
    entities: Vec<Entity>,
    columns: Vec<SavedColumn>,
}

impl SavedTable {
    /// Drops the saved values and forgets the rows.
    fn clear(&mut self) {
        let len = std::mem::take(&mut self.len);
        for c in &mut self.columns {
            // SAFETY: the first `len` values are initialized; `len` is 0 now.
            unsafe { c.drop_values(len) };
        }
        self.entities.clear();
    }

    /// Makes the columns match `arch`'s components, keeping the memory of
    /// those it already has. The table holds no rows.
    fn shape_like(&mut self, arch: &Archetype, world: &World) {
        debug_assert_eq!(self.len, 0);
        let same =
            self.columns.len() == arch.ids.len() && self.columns.iter().zip(arch.ids.iter()).all(|(c, &id)| c.id == id);
        if same {
            return;
        }
        for c in &mut self.columns {
            c.free();
        }
        self.columns.clear();
        for &id in arch.ids.iter() {
            let info = world.component_info(id);
            self.columns
                .push(SavedColumn::new(id, info.layout, info.drop, info.clone));
        }
    }
}

impl Drop for SavedTable {
    fn drop(&mut self) {
        self.clear();
        for c in &mut self.columns {
            c.free();
        }
    }
}

/// A saved resource ([`World::snapshot_resource`]): `None` if the world
/// didn't have it.
struct SavedResource {
    id: TypeId,
    value: Option<Box<dyn Any + Send + Sync>>,
}

/// A resource type a world includes in its snapshots, and how.
#[derive(Clone, Copy)]
pub(crate) struct SnapshotResource {
    id: TypeId,
    /// Copies the world's value into the saved slot (reusing it).
    save: fn(&World, &mut Option<Box<dyn Any + Send + Sync>>),
    /// Makes the world's value the saved one (removes it if `None`).
    load: fn(&mut World, &Option<Box<dyn Any + Send + Sync>>),
}

fn save_resource<R: Clone + Send + Sync + 'static>(world: &World, into: &mut Option<Box<dyn Any + Send + Sync>>) {
    match (
        world.get_resource::<R>(),
        into.as_mut().and_then(|b| b.downcast_mut::<R>()),
    ) {
        (Some(value), Some(saved)) => saved.clone_from(value),
        (Some(value), None) => *into = Some(Box::new(value.clone())),
        (None, _) => *into = None,
    }
}

fn load_resource<R: Clone + Send + Sync + 'static>(world: &mut World, from: &Option<Box<dyn Any + Send + Sync>>) {
    match from.as_ref().and_then(|b| b.downcast_ref::<R>()) {
        Some(saved) => match world.get_resource_mut::<R>() {
            Some(value) => value.clone_from(saved),
            None => world.insert_resource(saved.clone()),
        },
        None => {
            world.remove_resource::<R>();
        }
    }
}

/// A copy of a [`World`] in memory: every entity with its components, the
/// entity allocator, and the resources the world names with
/// [`World::snapshot_resource`]. Made by [`World::save_snapshot`], put back
/// by [`World::restore_snapshot`].
///
/// Keep one and save into it again and again: it reuses its memory, so
/// saving a world of plain components allocates nothing once the snapshot
/// has held a world that big. See the module docs for what is saved and why.
///
/// ```
/// use ponko_ecs::{Component, World, WorldSnapshot};
///
/// #[derive(Component, Clone, Debug, PartialEq)]
/// struct Pos(i32);
///
/// let mut world = World::new();
/// let a = world.spawn((Pos(1),));
/// let mut snapshot = WorldSnapshot::new();
/// world.save_snapshot(&mut snapshot).unwrap();
///
/// let b = world.spawn((Pos(2),));
/// world.get_mut::<Pos>(a).unwrap().0 = 5;
/// world.despawn(a);
///
/// world.restore_snapshot(&snapshot).unwrap();
/// assert_eq!(world.get::<Pos>(a), Some(&Pos(1))); // the same id again
/// assert!(!world.is_alive(b));
/// // ...and the next spawn gets the id it got after the save.
/// assert_eq!(world.spawn((Pos(2),)), b);
/// ```
#[derive(Default)]
pub struct WorldSnapshot {
    /// The world saved here ([`World::id`]), 0 if none.
    world: u64,
    /// The world's archetype layout when saved (see `World::layout`).
    layout: u64,
    slots: SlotsState,
    /// One per archetype slot.
    tables: Vec<SavedTable>,
    entities: EntitiesState,
    resources: Vec<SavedResource>,
}

// SAFETY: the saved values are components, which are `Send + Sync` (Rust
// types by their trait bounds, C and C++ ones by the promise their
// registration makes); the rest is plain data.
unsafe impl Send for WorldSnapshot {}
// SAFETY: as above; a shared snapshot is only read.
unsafe impl Sync for WorldSnapshot {}

impl std::fmt::Debug for WorldSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorldSnapshot")
            .field("entities", &self.len())
            .field("bytes", &self.heap_bytes())
            .finish_non_exhaustive()
    }
}

impl WorldSnapshot {
    /// An empty snapshot, to save worlds into.
    ///
    /// ```
    /// let snapshot = ponko_ecs::WorldSnapshot::new();
    /// assert_eq!(snapshot.len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Entities in the snapshot.
    ///
    /// ```
    /// use ponko_ecs::{Component, World, WorldSnapshot};
    /// #[derive(Component, Clone)]
    /// struct Tag;
    /// let mut world = World::new();
    /// world.spawn_batch((0..3).map(|_| (Tag,)));
    /// let mut snapshot = WorldSnapshot::new();
    /// world.save_snapshot(&mut snapshot).unwrap();
    /// assert_eq!(snapshot.len(), 3);
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.tables.iter().map(|t| t.len).sum()
    }

    /// Whether the snapshot holds no entities.
    ///
    /// ```
    /// assert!(ponko_ecs::WorldSnapshot::new().is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the snapshot holds on the heap (its component columns, entity
    /// lists and allocator copy; not what saved values own themselves).
    ///
    /// ```
    /// assert_eq!(ponko_ecs::WorldSnapshot::new().heap_bytes(), 0);
    /// ```
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        let tables: usize = self
            .tables
            .iter()
            .map(|t| {
                t.entities.capacity() * std::mem::size_of::<Entity>()
                    + t.columns.iter().map(|c| c.item.size() * c.cap).sum::<usize>()
            })
            .sum();
        tables + self.entities.heap_bytes() + self.slots.heap_bytes()
    }
}

impl World {
    /// Copies this world into `into`: every entity and component, the
    /// entity allocator, and the resources named with
    /// [`snapshot_resource`](Self::snapshot_resource). Whatever `into` held
    /// before is replaced; its memory is reused.
    ///
    /// Every component the world holds must be copyable
    /// ([`ComponentInfo::can_snapshot`](crate::ComponentInfo::can_snapshot)):
    /// `#[derive(Component)]` on a `Clone` type is (plain data is copied as
    /// bytes, other types with `Clone`), and so is a C or C++ component
    /// without a destructor. Otherwise nothing is saved and the error names
    /// the component.
    ///
    /// ```
    /// use ponko_ecs::{Component, World, WorldSnapshot};
    ///
    /// #[derive(Component, Clone, Debug, PartialEq)]
    /// struct Name(String);
    ///
    /// let mut world = World::new();
    /// let e = world.spawn((Name("crab".into()),));
    /// let mut snapshot = WorldSnapshot::new();
    /// world.save_snapshot(&mut snapshot).unwrap();
    /// world.get_mut::<Name>(e).unwrap().0.push_str("bot");
    /// world.restore_snapshot(&snapshot).unwrap();
    /// assert_eq!(world.get::<Name>(e), Some(&Name("crab".into())));
    /// ```
    ///
    /// # Errors
    ///
    /// [`SnapshotError::NotCloneable`] if an entity has a component that
    /// can't be copied.
    pub fn save_snapshot(&self, into: &mut WorldSnapshot) -> Result<(), SnapshotError> {
        // Check everything first, so a failed save leaves `into` as it was.
        for arch in self.archetypes.iter().filter(|a| !a.is_empty()) {
            for &id in arch.ids.iter() {
                let info = self.component_info(id);
                if !info.can_snapshot() {
                    return Err(SnapshotError::NotCloneable {
                        component: info.name().to_string(),
                    });
                }
            }
        }
        let slots = self.archetypes.len();
        // A world can only gain slots, but the snapshot may come from a
        // bigger world (another one, or before a restore of a smaller one).
        into.tables.truncate(slots);
        into.tables.resize_with(slots, SavedTable::default);
        self.slots.save_state(&mut into.slots);
        for (a, (arch, table)) in self.archetypes.iter().zip(&mut into.tables).enumerate() {
            table.clear();
            if into.layout != self.layout || into.world != self.uid {
                table.ids = (!self.slots.is_dead(a)).then(|| arch.ids.clone());
            }
            if arch.is_empty() {
                continue;
            }
            table.shape_like(arch, self);
            let rows = arch.len();
            for (c, column) in table.columns.iter_mut().zip(arch.columns.iter()) {
                c.reserve(rows);
                // SAFETY: the archetype's first `rows` values are initialized
                // and copyable (checked above); the column has room.
                unsafe { c.copy_from(column.base(), rows) };
            }
            table.entities.extend_from_slice(&arch.entities);
            table.len = rows;
        }
        self.entities.save_state(&mut into.entities);
        into.resources.truncate(self.snapshot_resources.len());
        for (i, r) in self.snapshot_resources.iter().enumerate() {
            if into.resources.get(i).is_none_or(|s| s.id != r.id) {
                into.resources.truncate(i);
                into.resources.push(SavedResource { id: r.id, value: None });
            }
            (r.save)(self, &mut into.resources[i].value);
        }
        into.world = self.uid;
        into.layout = self.layout;
        Ok(())
    }

    /// Makes this world what it was when `from` was saved: the same
    /// entities with the same ids and components, in the same order, the
    /// same ids for the next spawns, and the resources named with
    /// [`snapshot_resource`](Self::snapshot_resource). A simulation run
    /// from here does what it did after the save.
    ///
    /// - No hooks run ([`on_add`](Self::on_add) and the others): the world
    ///   jumps to the saved state, nothing is spawned or despawned.
    /// - For change detection, every restored value of a tracked component
    ///   ([`track_changes`](Self::track_changes)) counts as added and
    ///   changed now; no removals are logged.
    /// - Command buffers recorded before the restore can't spawn after it
    ///   (as after a [`reset`](Self::reset)): apply them first.
    /// - Resources not named with `snapshot_resource` are left alone.
    ///
    /// If a component's `Clone` panics, the world is left empty (its
    /// entity ids start over) and the panic goes on.
    ///
    /// ```
    /// use ponko_ecs::{Component, World, WorldSnapshot};
    ///
    /// #[derive(Component, Clone, Debug, PartialEq)]
    /// struct Hp(u32);
    ///
    /// let mut world = World::new();
    /// let hero = world.spawn((Hp(10),));
    /// let mut before_fight = WorldSnapshot::new();
    /// world.save_snapshot(&mut before_fight).unwrap();
    ///
    /// world.despawn(hero);
    /// world.restore_snapshot(&before_fight).unwrap();
    /// assert_eq!(world.get::<Hp>(hero), Some(&Hp(10)));
    /// ```
    ///
    /// # Errors
    ///
    /// [`SnapshotError::OtherWorld`] if `from` wasn't saved from this world.
    pub fn restore_snapshot(&mut self, from: &WorldSnapshot) -> Result<(), SnapshotError> {
        if from.world != self.uid {
            return Err(SnapshotError::OtherWorld);
        }
        self.recorder.note(|l| l.structural = true);
        if self.layout != from.layout {
            self.lay_out_like(from);
        }
        let moved = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.restore_rows(from)));
        if let Err(panic) = moved {
            self.forget_everything(from);
            std::panic::resume_unwind(panic);
        }
        self.slots.restore_state(&from.slots);
        self.entities.restore_state(&from.entities);
        for (i, saved) in from.resources.iter().enumerate() {
            let Some(&r) = self.snapshot_resources.get(i) else {
                break;
            };
            if r.id == saved.id {
                (r.load)(self, &saved.value);
            }
        }
        Ok(())
    }

    /// Includes resource `R` in this world's snapshots from now on: saved
    /// with `Clone` by [`save_snapshot`](Self::save_snapshot), put back (or
    /// removed, if the world didn't have it then) by
    /// [`restore_snapshot`](Self::restore_snapshot). Other resources are
    /// left alone by both. Naming one twice changes nothing.
    ///
    /// ```
    /// use ponko_ecs::{World, WorldSnapshot};
    ///
    /// #[derive(Clone)]
    /// struct Score(u32);
    ///
    /// let mut world = World::new();
    /// world.snapshot_resource::<Score>();
    /// world.insert_resource(Score(1));
    /// let mut snapshot = WorldSnapshot::new();
    /// world.save_snapshot(&mut snapshot).unwrap();
    /// world.resource_mut::<Score>().0 = 9;
    /// world.restore_snapshot(&snapshot).unwrap();
    /// assert_eq!(world.resource::<Score>().0, 1);
    /// ```
    pub fn snapshot_resource<R: Clone + Send + Sync + 'static>(&mut self) {
        let id = TypeId::of::<R>();
        if self.snapshot_resources.iter().all(|r| r.id != id) {
            self.snapshot_resources.push(SnapshotResource {
                id,
                save: save_resource::<R>,
                load: load_resource::<R>,
            });
        }
    }

    /// Lets snapshots copy component `T` with its `Clone`: what
    /// `#[derive(Component)]` does by itself for a `Clone` type, for a type
    /// with a hand-written `impl Component`. Registers `T` if it wasn't.
    ///
    /// ```
    /// use ponko_ecs::{Component, World, WorldSnapshot};
    ///
    /// #[derive(Clone)]
    /// struct Hand(u8);
    /// impl Component for Hand {}
    ///
    /// let mut world = World::new();
    /// world.spawn((Hand(5),));
    /// assert!(world.save_snapshot(&mut WorldSnapshot::new()).is_err());
    /// world.register_clone::<Hand>();
    /// assert!(world.save_snapshot(&mut WorldSnapshot::new()).is_ok());
    /// ```
    pub fn register_clone<T: Component + Clone>(&mut self) {
        let id = self.register::<T>();
        self.infos[id.index()].clone = Some(clone_values::<T>);
    }

    /// Puts the saved rows back, slot by slot. The layout is the saved one.
    fn restore_rows(&mut self, from: &WorldSnapshot) {
        let stamp = self.changes.stamp();
        for (arch, table) in self.archetypes.iter_mut().zip(&from.tables) {
            // Drop what is there: also when the saved rows are the same
            // entities, a clone must replace the value.
            let len = arch.len();
            arch.entities.clear();
            for c in arch.columns.iter() {
                if let Some(drop) = c.drop_fn() {
                    for row in 0..len {
                        // SAFETY: rows below the old length were initialized;
                        // the entity list no longer covers them.
                        unsafe { drop(c.ptr(row)) };
                    }
                }
            }
            if table.len == 0 {
                continue;
            }
            arch.reserve(table.len, &mut self.slots.buffers);
            for (c, saved) in arch.columns.iter().zip(&table.columns) {
                debug_assert_eq!(c.size(), saved.item.size());
                // SAFETY: the saved column holds `table.len` initialized
                // values of this column's component (same world, same
                // layout); the archetype has room for them.
                unsafe { copy_values(saved.clone, saved.data.as_ptr(), c.base(), table.len, c.size()) };
            }
            arch.entities.extend_from_slice(&table.entities);
            if arch.tracked() {
                for row in 0..table.len {
                    arch.stamp_row(row, stamp);
                }
            }
        }
    }

    /// After a panic part way through a restore: forgets every row (the
    /// values are leaked, never dropped twice) and every entity id, leaving
    /// a consistent, empty world.
    #[cold]
    fn forget_everything(&mut self, from: &WorldSnapshot) {
        // The layout is the saved one: so is the bookkeeping, then every
        // slot that had rows in it is emptied.
        self.slots.restore_state(&from.slots);
        for (a, (arch, table)) in self.archetypes.iter_mut().zip(&from.tables).enumerate() {
            arch.entities.clear();
            if table.len > 0 {
                self.slots.emptied(a);
            }
        }
        self.entities.reset();
    }

    /// Lays the archetype slots out as they were when `from` was saved:
    /// the same component set in every slot, the same slots dead, no more
    /// slots. Rows aren't put back here.
    #[cold]
    #[inline(never)]
    fn lay_out_like(&mut self, from: &WorldSnapshot) {
        let n = from.tables.len();
        // Every row goes: the saved ones are put back after.
        for arch in &mut self.archetypes {
            let len = arch.len();
            arch.entities.clear();
            for c in arch.columns.iter() {
                if let Some(drop) = c.drop_fn() {
                    for row in 0..len {
                        // SAFETY: rows below the old length were initialized;
                        // the entity list no longer covers them.
                        unsafe { drop(c.ptr(row)) };
                    }
                }
            }
            // Edges may lead to slots that change below.
            arch.add_edges.clear();
            arch.remove_edges.clear();
        }
        self.archetypes.truncate(n);
        for (a, table) in from.tables.iter().enumerate() {
            let want: &[ComponentId] = table.ids.as_deref().unwrap_or(&[]);
            let keep = a < self.archetypes.len()
                && !self.slots.is_dead(a)
                && table.ids.is_some()
                && *self.archetypes[a].ids == *want;
            if keep {
                continue;
            }
            let arch = Archetype::new(want.into(), &self.infos);
            if a < self.archetypes.len() {
                self.archetypes[a] = arch;
            } else {
                self.archetypes.push(arch);
            }
        }
        self.archetype_by_ids.clear();
        for (a, table) in from.tables.iter().enumerate() {
            if let Some(ids) = &table.ids {
                self.archetype_by_ids.insert(ids.clone(), a);
            }
            if self.hooked {
                self.hooks.archetype_created(a, &self.archetypes[a].ids);
            }
        }
        self.query_matches.forget_layout();
        // Placed again at their next spawn, in the archetypes they had.
        self.forget_bundles();
        self.layout = from.layout;
    }
}
