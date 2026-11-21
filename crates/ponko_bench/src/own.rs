//! Minimal archetype ECS for the M3 spike.
//!
//! Every component, Rust-typed or not, is a runtime-registered column
//! (name, size, align, drop fn). The typed API is sugar on top: it maps a
//! `TypeId` to a `ComponentId` once, and queries resolve one base pointer per
//! archetype, then run a plain typed loop. That is the D12 question: does
//! runtime registration cost the Rust path anything?

use std::alloc::{self, Layout};
use std::any::TypeId;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Entity {
    index: u32,
    generation: u32,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct ComponentId(u32);

type DropFn = unsafe fn(*mut u8);

unsafe fn drop_ptr<T>(p: *mut u8) {
    unsafe { p.cast::<T>().drop_in_place() }
}

struct ComponentInfo {
    #[allow(dead_code)]
    name: String,
    layout: Layout,
    drop: Option<DropFn>,
}

/// One type-erased column. Capacity is owned by the archetype (all columns share it).
struct Column {
    data: NonNull<u8>,
    item: Layout,
    drop: Option<DropFn>,
}

impl Column {
    fn new(item: Layout, drop: Option<DropFn>) -> Self {
        // Dangling but aligned: valid for zero-sized items and empty columns.
        let data = NonNull::new(std::ptr::without_provenance_mut(item.align())).unwrap();
        Column { data, item, drop }
    }

    #[inline(always)]
    fn ptr(&self, row: usize) -> *mut u8 {
        unsafe { self.data.as_ptr().add(row * self.item.size()) }
    }

    fn array(&self, cap: usize) -> Layout {
        Layout::from_size_align(self.item.size() * cap, self.item.align()).unwrap()
    }

    unsafe fn grow(&mut self, old_cap: usize, new_cap: usize) {
        if self.item.size() == 0 {
            return;
        }
        let new = self.array(new_cap);
        let p = unsafe {
            if old_cap == 0 {
                alloc::alloc(new)
            } else {
                alloc::realloc(self.data.as_ptr(), self.array(old_cap), new.size())
            }
        };
        self.data = NonNull::new(p).unwrap_or_else(|| alloc::handle_alloc_error(new));
    }

    /// Moves the last row into `row` without dropping `row` (caller already moved or dropped it).
    unsafe fn swap_remove_forget(&mut self, row: usize, last: usize) {
        if row != last {
            unsafe { std::ptr::copy_nonoverlapping(self.ptr(last), self.ptr(row), self.item.size()) }
        }
    }

    unsafe fn dealloc(&mut self, cap: usize) {
        if self.item.size() != 0 && cap != 0 {
            unsafe { alloc::dealloc(self.data.as_ptr(), self.array(cap)) }
        }
    }
}

pub struct Archetype {
    ids: Box<[ComponentId]>,
    columns: Box<[Column]>,
    entities: Vec<Entity>,
    cap: usize,
    add_edges: HashMap<ComponentId, usize>,
    remove_edges: HashMap<ComponentId, usize>,
}

impl Archetype {
    fn new(ids: Box<[ComponentId]>, infos: &[ComponentInfo]) -> Self {
        let columns = ids
            .iter()
            .map(|id| {
                let info = &infos[id.0 as usize];
                Column::new(info.layout, info.drop)
            })
            .collect();
        Archetype {
            ids,
            columns,
            entities: Vec::new(),
            cap: 0,
            add_edges: HashMap::new(),
            remove_edges: HashMap::new(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    #[inline]
    fn column_index(&self, id: ComponentId) -> Option<usize> {
        // Archetypes have few components; a linear scan beats binary search here.
        self.ids.iter().position(|&c| c == id)
    }

    /// Base pointer of a component's column, for chunk-style (C/C++) systems.
    #[inline]
    pub fn column_ptr(&self, id: ComponentId) -> Option<*mut u8> {
        self.column_index(id).map(|i| self.columns[i].data.as_ptr())
    }

    fn reserve(&mut self, additional: usize) {
        let needed = self.len() + additional;
        if needed <= self.cap {
            return;
        }
        let new_cap = needed.max(self.cap * 2).max(8);
        for c in self.columns.iter_mut() {
            unsafe { c.grow(self.cap, new_cap) }
        }
        self.cap = new_cap;
        self.entities.reserve(new_cap - self.entities.len());
    }

    /// Removes `row` by swapping the last row in; drops the removed values.
    /// Returns the entity that moved into `row`, if any.
    unsafe fn swap_remove_drop(&mut self, row: usize) -> Option<Entity> {
        let last = self.len() - 1;
        for c in self.columns.iter_mut() {
            if let Some(d) = c.drop {
                unsafe { d(c.ptr(row)) }
            }
            unsafe { c.swap_remove_forget(row, last) }
        }
        self.entities.swap_remove(row);
        (row != last).then(|| self.entities[row])
    }
}

impl Drop for Archetype {
    fn drop(&mut self) {
        let len = self.len();
        for c in self.columns.iter_mut() {
            if let Some(d) = c.drop {
                for row in 0..len {
                    unsafe { d(c.ptr(row)) }
                }
            }
            unsafe { c.dealloc(self.cap) }
        }
    }
}

#[derive(Copy, Clone)]
struct Meta {
    generation: u32,
    archetype: u32,
    row: u32,
}

const DEAD: u32 = u32::MAX;

/// `TypeId` is already a good hash; skip SipHash for type lookups.
#[derive(Default)]
pub struct TypeIdHasher(u64);

impl std::hash::Hasher for TypeIdHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 ^= n;
    }
}

type TypeMap<V> = HashMap<TypeId, V, std::hash::BuildHasherDefault<TypeIdHasher>>;

pub trait Component: 'static {}
impl<T: 'static> Component for T {}

pub struct World {
    infos: Vec<ComponentInfo>,
    by_type: TypeMap<ComponentId>,
    archetypes: Vec<Archetype>,
    archetype_by_ids: HashMap<Box<[ComponentId]>, usize>,
    metas: Vec<Meta>,
    free: Vec<u32>,
    /// Bundle type → (archetype, column index for each tuple element).
    bundles: TypeMap<(usize, Box<[usize]>)>,
    scratch: Vec<ComponentId>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        let mut w = World {
            infos: Vec::new(),
            by_type: TypeMap::default(),
            archetypes: Vec::new(),
            archetype_by_ids: HashMap::new(),
            metas: Vec::new(),
            free: Vec::new(),
            bundles: TypeMap::default(),
            scratch: Vec::new(),
        };
        w.archetype_for(Box::new([])); // archetype 0: no components
        w
    }

    // ---- components -------------------------------------------------------

    pub fn register<T: Component>(&mut self) -> ComponentId {
        if let Some(&id) = self.by_type.get(&TypeId::of::<T>()) {
            return id;
        }
        let drop = std::mem::needs_drop::<T>().then_some(drop_ptr::<T> as DropFn);
        let id = self.push_info(std::any::type_name::<T>().into(), Layout::new::<T>(), drop);
        self.by_type.insert(TypeId::of::<T>(), id);
        id
    }

    /// Registers a component known only by name, size and alignment (the C API path).
    pub fn register_dynamic(&mut self, name: &str, size: usize, align: usize, drop: Option<DropFn>) -> ComponentId {
        let layout = Layout::from_size_align(size, align).expect("bad component layout");
        self.push_info(name.into(), layout, drop)
    }

    fn push_info(&mut self, name: String, layout: Layout, drop: Option<DropFn>) -> ComponentId {
        let id = ComponentId(self.infos.len() as u32);
        self.infos.push(ComponentInfo { name, layout, drop });
        id
    }

    #[inline]
    pub fn component_id<T: Component>(&self) -> Option<ComponentId> {
        self.by_type.get(&TypeId::of::<T>()).copied()
    }

    // ---- archetypes ------------------------------------------------------

    /// `ids` must be sorted and unique.
    fn archetype_for(&mut self, ids: Box<[ComponentId]>) -> usize {
        if let Some(&a) = self.archetype_by_ids.get(&ids) {
            return a;
        }
        let a = self.archetypes.len();
        self.archetypes.push(Archetype::new(ids.clone(), &self.infos));
        self.archetype_by_ids.insert(ids, a);
        a
    }

    pub fn archetypes(&self) -> &[Archetype] {
        &self.archetypes
    }

    // ---- entities ----------------------------------------------------------

    fn alloc_entity(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            Entity {
                index,
                generation: self.metas[index as usize].generation,
            }
        } else {
            let index = self.metas.len() as u32;
            self.metas.push(Meta {
                generation: 0,
                archetype: DEAD,
                row: 0,
            });
            Entity { index, generation: 0 }
        }
    }

    #[inline]
    fn meta(&self, e: Entity) -> Option<Meta> {
        let m = *self.metas.get(e.index as usize)?;
        (m.generation == e.generation && m.archetype != DEAD).then_some(m)
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.meta(e).is_some()
    }

    pub fn len(&self) -> usize {
        self.metas.len() - self.free.len()
    }

    /// Archetype for bundle `B`; caches the column mapping in `self.bundles`.
    fn bundle_archetype<B: Bundle>(&mut self) -> usize {
        if let Some(b) = self.bundles.get(&TypeId::of::<B>()) {
            return b.0;
        }
        let mut ids = Vec::new();
        B::component_ids(self, &mut ids);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "bundle has the same component twice");
        let a = self.archetype_for(sorted.into());
        let cols: Box<[usize]> = ids
            .iter()
            .map(|&id| self.archetypes[a].column_index(id).unwrap())
            .collect();
        self.bundles.insert(TypeId::of::<B>(), (a, cols));
        a
    }

    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let a = self.bundle_archetype::<B>();
        let e = self.alloc_entity();
        let cols = &self.bundles[&TypeId::of::<B>()].1;
        let arch = &mut self.archetypes[a];
        arch.reserve(1);
        let row = arch.len();
        unsafe {
            bundle.write(&mut |i, src| {
                let c = &arch.columns[cols[i]];
                std::ptr::copy_nonoverlapping(src, c.ptr(row), c.item.size());
            });
        }
        arch.entities.push(e);
        self.metas[e.index as usize] = Meta {
            generation: e.generation,
            archetype: a as u32,
            row: row as u32,
        };
        e
    }

    /// Spawns many entities of one bundle type; reserves once.
    pub fn spawn_batch<B: Bundle>(&mut self, iter: impl IntoIterator<Item = B>) {
        let iter = iter.into_iter();
        let a = self.bundle_archetype::<B>();
        self.archetypes[a].reserve(iter.size_hint().0);
        for b in iter {
            self.spawn(b);
        }
    }

    /// Spawns from raw component bytes (the C API path). Each pointer must
    /// point at a valid value of that component; ownership moves into the world.
    pub unsafe fn spawn_dynamic(&mut self, components: &[(ComponentId, *const u8)]) -> Entity {
        self.scratch.clear();
        self.scratch.extend(components.iter().map(|c| c.0));
        self.scratch.sort_unstable();
        let ids: Box<[ComponentId]> = self.scratch.as_slice().into();
        let a = self.archetype_for(ids);
        let e = self.alloc_entity();
        let arch = &mut self.archetypes[a];
        arch.reserve(1);
        let row = arch.len();
        for &(id, src) in components {
            let c = &arch.columns[arch.column_index(id).unwrap()];
            unsafe { std::ptr::copy_nonoverlapping(src, c.ptr(row), c.item.size()) };
        }
        arch.entities.push(e);
        self.metas[e.index as usize] = Meta {
            generation: e.generation,
            archetype: a as u32,
            row: row as u32,
        };
        e
    }

    pub fn despawn(&mut self, e: Entity) -> bool {
        let Some(m) = self.meta(e) else { return false };
        let moved = unsafe { self.archetypes[m.archetype as usize].swap_remove_drop(m.row as usize) };
        if let Some(moved) = moved {
            self.metas[moved.index as usize].row = m.row;
        }
        let meta = &mut self.metas[e.index as usize];
        meta.generation = meta.generation.wrapping_add(1);
        meta.archetype = DEAD;
        self.free.push(e.index);
        true
    }

    pub fn get<T: Component>(&self, e: Entity) -> Option<&T> {
        let m = self.meta(e)?;
        let arch = &self.archetypes[m.archetype as usize];
        let c = arch.column_index(self.component_id::<T>()?)?;
        Some(unsafe { &*arch.columns[c].ptr(m.row as usize).cast::<T>() })
    }

    pub fn get_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        let m = self.meta(e)?;
        let id = self.component_id::<T>()?;
        let arch = &mut self.archetypes[m.archetype as usize];
        let c = arch.column_index(id)?;
        Some(unsafe { &mut *arch.columns[c].ptr(m.row as usize).cast::<T>() })
    }

    /// Adds (or replaces) a component.
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> bool {
        let Some(m) = self.meta(e) else { return false };
        let id = self.register::<T>();
        let src = m.archetype as usize;
        if let Some(c) = self.archetypes[src].column_index(id) {
            unsafe { *self.archetypes[src].columns[c].ptr(m.row as usize).cast::<T>() = value };
            return true;
        }
        let dst = match self.archetypes[src].add_edges.get(&id) {
            Some(&d) => d,
            None => {
                let mut ids = self.archetypes[src].ids.to_vec();
                ids.push(id);
                ids.sort_unstable();
                let d = self.archetype_for(ids.into());
                self.archetypes[src].add_edges.insert(id, d);
                self.archetypes[d].remove_edges.insert(id, src);
                d
            }
        };
        let new_row = self.move_row(e, m, dst);
        let arch = &mut self.archetypes[dst];
        let c = arch.column_index(id).unwrap();
        unsafe { arch.columns[c].ptr(new_row).cast::<T>().write(value) };
        true
    }

    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        let m = self.meta(e)?;
        let id = self.component_id::<T>()?;
        let src = m.archetype as usize;
        let c = self.archetypes[src].column_index(id)?;
        let value = unsafe { self.archetypes[src].columns[c].ptr(m.row as usize).cast::<T>().read() };
        let dst = match self.archetypes[src].remove_edges.get(&id) {
            Some(&d) => d,
            None => {
                let ids: Vec<_> = self.archetypes[src].ids.iter().copied().filter(|&c| c != id).collect();
                let d = self.archetype_for(ids.into());
                self.archetypes[src].remove_edges.insert(id, d);
                self.archetypes[d].add_edges.insert(id, src);
                d
            }
        };
        self.move_row(e, m, dst);
        Some(value)
    }

    /// Moves `e`'s row from its archetype to `dst`, copying shared columns.
    /// Columns missing in `dst` are forgotten (caller has read or will drop them).
    /// Returns the new row; the caller must initialize `dst` columns not in the source.
    fn move_row(&mut self, e: Entity, m: Meta, dst: usize) -> usize {
        let src = m.archetype as usize;
        let row = m.row as usize;
        let (s, d) = two_mut(&mut self.archetypes, src, dst);
        d.reserve(1);
        let new_row = d.len();
        let last = s.len() - 1;
        for (i, col) in s.columns.iter_mut().enumerate() {
            if let Some(j) = d.column_index(s.ids[i]) {
                unsafe { std::ptr::copy_nonoverlapping(col.ptr(row), d.columns[j].ptr(new_row), col.item.size()) };
            }
            unsafe { col.swap_remove_forget(row, last) };
        }
        s.entities.swap_remove(row);
        if row != last {
            let moved = s.entities[row];
            self.metas[moved.index as usize].row = row as u32;
        }
        d.entities.push(e);
        self.metas[e.index as usize] = Meta {
            generation: e.generation,
            archetype: dst as u32,
            row: new_row as u32,
        };
        new_row
    }

    // ---- queries -------------------------------------------------------------

    /// Runs `f` for every entity that has all components in `Q`.
    #[inline]
    pub fn for_each<Q: Query>(&mut self, mut f: impl FnMut(Q::Item<'_>)) {
        let Some(ids) = Q::ids(self) else { return };
        for arch in self.archetypes.iter_mut() {
            let len = arch.len();
            if len == 0 {
                continue;
            }
            let Some(state) = Q::state(arch, &ids) else { continue };
            for row in 0..len {
                f(unsafe { Q::fetch(state, row) });
            }
        }
    }

    /// Chunk iteration for systems that see raw columns (the C/C++ path):
    /// `f(len, column_ptrs)` once per matching archetype, pointers in `ids` order.
    pub fn for_each_chunk(&mut self, ids: &[ComponentId], mut f: impl FnMut(usize, &[*mut u8])) {
        let mut ptrs = Vec::with_capacity(ids.len());
        'arch: for arch in self.archetypes.iter_mut() {
            if arch.len() == 0 {
                continue;
            }
            ptrs.clear();
            for &id in ids {
                match arch.column_ptr(id) {
                    Some(p) => ptrs.push(p),
                    None => continue 'arch,
                }
            }
            f(arch.len(), &ptrs);
        }
    }
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

// ---- bundles -----------------------------------------------------------------

/// A tuple of components that can be spawned together.
///
/// # Safety
/// `write` must call `f` exactly once per element, in the order of `component_ids`,
/// with a pointer to a value that the callee takes ownership of.
pub unsafe trait Bundle: 'static {
    fn component_ids(world: &mut World, out: &mut Vec<ComponentId>);
    unsafe fn write(self, f: &mut impl FnMut(usize, *const u8));
}

macro_rules! bundle_tuple {
    ($($t:ident $i:tt),*) => {
        unsafe impl<$($t: Component),*> Bundle for ($($t,)*) {
            fn component_ids(world: &mut World, out: &mut Vec<ComponentId>) {
                $(out.push(world.register::<$t>());)*
            }
            #[allow(non_snake_case)]
            unsafe fn write(self, f: &mut impl FnMut(usize, *const u8)) {
                let me = ManuallyDrop::new(self);
                $(f($i, (&me.$i as *const $t).cast());)*
            }
        }
    };
}
bundle_tuple!(A 0);
bundle_tuple!(A 0, B 1);
bundle_tuple!(A 0, B 1, C 2);
bundle_tuple!(A 0, B 1, C 2, D 3);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4);
bundle_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);

// ---- queries -----------------------------------------------------------------

/// Something that can be fetched per entity: `&T`, `&mut T`, or a tuple of those.
///
/// # Safety
/// `state`/`fetch` must only hand out references to the columns named in `ids`,
/// and `ids` must reject aliasing `&mut`.
pub unsafe trait Query {
    type Item<'a>;
    /// Resolved component ids (once per query run).
    type Ids;
    /// Per-archetype base pointers.
    type State: Copy;
    fn ids(world: &World) -> Option<Self::Ids>;
    fn state(arch: &Archetype, ids: &Self::Ids) -> Option<Self::State>;
    unsafe fn fetch<'a>(state: Self::State, row: usize) -> Self::Item<'a>;
}

unsafe impl<T: Component> Query for &T {
    type Item<'a> = &'a T;
    type Ids = ComponentId;
    type State = *const T;
    #[inline]
    fn ids(world: &World) -> Option<ComponentId> {
        world.component_id::<T>()
    }
    #[inline]
    fn state(arch: &Archetype, id: &ComponentId) -> Option<*const T> {
        arch.column_ptr(*id).map(|p| p as *const T)
    }
    #[inline(always)]
    unsafe fn fetch<'a>(state: *const T, row: usize) -> &'a T {
        unsafe { &*state.add(row) }
    }
}

unsafe impl<T: Component> Query for &mut T {
    type Item<'a> = &'a mut T;
    type Ids = ComponentId;
    type State = *mut T;
    #[inline]
    fn ids(world: &World) -> Option<ComponentId> {
        world.component_id::<T>()
    }
    #[inline]
    fn state(arch: &Archetype, id: &ComponentId) -> Option<*mut T> {
        arch.column_ptr(*id).map(|p| p as *mut T)
    }
    #[inline(always)]
    unsafe fn fetch<'a>(state: *mut T, row: usize) -> &'a mut T {
        unsafe { &mut *state.add(row) }
    }
}

macro_rules! query_tuple {
    ($($t:ident $i:tt),*) => {
        unsafe impl<$($t: Query),*> Query for ($($t,)*) {
            type Item<'a> = ($($t::Item<'a>,)*);
            type Ids = ($($t::Ids,)*);
            type State = ($($t::State,)*);
            #[inline]
            fn ids(world: &World) -> Option<Self::Ids> {
                // Spike shortcut: aliasing check (same T twice with &mut) is left to M3 proper.
                Some(($($t::ids(world)?,)*))
            }
            #[inline]
            fn state(arch: &Archetype, ids: &Self::Ids) -> Option<Self::State> {
                Some(($($t::state(arch, &ids.$i)?,)*))
            }
            #[inline(always)]
            unsafe fn fetch<'a>(state: Self::State, row: usize) -> Self::Item<'a> {
                unsafe { ($($t::fetch(state.$i, row),)*) }
            }
        }
    };
}
query_tuple!(A 0);
query_tuple!(A 0, B 1);
query_tuple!(A 0, B 1, C 2);
query_tuple!(A 0, B 1, C 2, D 3);

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[derive(Debug, PartialEq, Clone, Copy)]
    struct Pos(f32, f32);
    #[derive(Debug, PartialEq, Clone, Copy)]
    struct Vel(f32, f32);
    struct Tag;

    #[test]
    fn spawn_get_despawn() {
        let mut w = World::new();
        let a = w.spawn((Pos(1.0, 2.0), Vel(3.0, 4.0)));
        let b = w.spawn((Pos(5.0, 6.0),));
        assert_eq!(w.get::<Pos>(a), Some(&Pos(1.0, 2.0)));
        assert_eq!(w.get::<Vel>(b), None);
        assert!(w.despawn(a));
        assert!(!w.contains(a));
        assert_eq!(w.get::<Pos>(a), None);
        let c = w.spawn((Pos(7.0, 8.0), Vel(0.0, 0.0)));
        assert_eq!(c.index, a.index);
        assert_ne!(c.generation, a.generation);
        assert_eq!(w.get::<Pos>(a), None, "stale handle must not see the new entity");
        assert_eq!(w.get::<Pos>(c), Some(&Pos(7.0, 8.0)));
    }

    #[test]
    fn swap_remove_keeps_rows_valid() {
        let mut w = World::new();
        let es: Vec<_> = (0..10).map(|i| w.spawn((Pos(i as f32, 0.0),))).collect();
        w.despawn(es[2]);
        w.despawn(es[0]);
        for (i, &e) in es.iter().enumerate() {
            if i == 0 || i == 2 {
                assert!(!w.contains(e));
            } else {
                assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, 0.0)));
            }
        }
    }

    #[test]
    fn insert_remove_moves_archetype() {
        let mut w = World::new();
        let es: Vec<_> = (0..5).map(|i| w.spawn((Pos(i as f32, 0.0),))).collect();
        w.insert(es[1], Vel(1.0, 1.0));
        w.insert(es[3], Tag);
        assert_eq!(w.get::<Vel>(es[1]), Some(&Vel(1.0, 1.0)));
        for (i, &e) in es.iter().enumerate() {
            assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, 0.0)));
        }
        assert_eq!(w.remove::<Vel>(es[1]), Some(Vel(1.0, 1.0)));
        assert_eq!(w.remove::<Vel>(es[1]), None);
        assert_eq!(w.get::<Pos>(es[1]), Some(&Pos(1.0, 0.0)));
        w.insert(es[0], Vel(9.0, 9.0));
        w.insert(es[0], Vel(2.0, 2.0)); // replace
        assert_eq!(w.get::<Vel>(es[0]), Some(&Vel(2.0, 2.0)));
    }

    #[test]
    fn query_matches_across_archetypes() {
        let mut w = World::new();
        w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0)));
        w.spawn((Pos(0.0, 0.0), Vel(1.0, 0.0), Tag));
        w.spawn((Pos(0.0, 0.0),));
        w.for_each::<(&mut Pos, &Vel)>(|(p, v)| p.0 += v.0);
        let mut sum = 0.0;
        let mut n = 0;
        w.for_each::<(&Pos,)>(|(p,)| {
            sum += p.0;
            n += 1;
        });
        assert_eq!((sum, n), (2.0, 3));
    }

    #[test]
    fn drops_run_exactly_once() {
        let rc = Rc::new(());
        {
            let mut w = World::new();
            let a = w.spawn((rc.clone(), Pos(0.0, 0.0)));
            let b = w.spawn((rc.clone(), Pos(0.0, 0.0)));
            w.spawn((rc.clone(),));
            assert_eq!(Rc::strong_count(&rc), 4);
            w.despawn(a);
            assert_eq!(Rc::strong_count(&rc), 3);
            w.insert(b, Vel(0.0, 0.0)); // moves the Rc to another archetype
            assert_eq!(Rc::strong_count(&rc), 3);
            let taken = w.remove::<Rc<()>>(b).unwrap();
            assert_eq!(Rc::strong_count(&rc), 3);
            drop(taken);
            assert_eq!(Rc::strong_count(&rc), 2);
        }
        assert_eq!(Rc::strong_count(&rc), 1);
    }

    #[test]
    fn dynamic_components_and_chunks() {
        let mut w = World::new();
        let p = w.register_dynamic("Pos", 8, 4, None);
        let v = w.register_dynamic("Vel", 8, 4, None);
        for i in 0..100 {
            let pos = [i as f32, 0.0f32];
            let vel = [1.0f32, 2.0f32];
            unsafe { w.spawn_dynamic(&[(p, pos.as_ptr().cast()), (v, vel.as_ptr().cast())]) };
        }
        w.for_each_chunk(&[p, v], |len, cols| {
            let pos = unsafe { std::slice::from_raw_parts_mut(cols[0].cast::<[f32; 2]>(), len) };
            let vel = unsafe { std::slice::from_raw_parts(cols[1].cast::<[f32; 2]>(), len) };
            for (p, v) in pos.iter_mut().zip(vel) {
                p[0] += v[0];
                p[1] += v[1];
            }
        });
        let mut sum = 0.0;
        w.for_each_chunk(&[p], |len, cols| {
            let pos = unsafe { std::slice::from_raw_parts(cols[0].cast::<[f32; 2]>(), len) };
            sum += pos.iter().map(|p| p[0] + p[1]).sum::<f32>();
        });
        assert_eq!(sum, (0..100).sum::<i32>() as f32 + 300.0);
    }

    #[test]
    fn get_mut_writes_the_entitys_own_row() {
        let mut w = World::new();
        let es: Vec<_> = (0..4).map(|i| w.spawn((Pos(i as f32, 0.0), Vel(0.0, 0.0)))).collect();
        w.get_mut::<Pos>(es[2]).unwrap().1 = 9.0;
        assert!(w.get_mut::<Tag>(es[2]).is_none());
        for (i, &e) in es.iter().enumerate() {
            let y = if i == 2 { 9.0 } else { 0.0 };
            assert_eq!(w.get::<Pos>(e), Some(&Pos(i as f32, y)));
        }
        w.despawn(es[2]);
        assert!(w.get_mut::<Pos>(es[2]).is_none(), "a dead entity has nothing");
    }

    #[test]
    fn zero_sized_components() {
        let mut w = World::new();
        let es: Vec<_> = (0..20).map(|_| w.spawn((Tag, Pos(1.0, 1.0)))).collect();
        w.despawn(es[3]);
        let mut n = 0;
        w.for_each::<(&Tag, &Pos)>(|_| n += 1);
        assert_eq!(n, 19);
    }
}
