//! What a physics world knows of each simulated entity, packed so its memory
//! follows the live bodies rather than the highest entity index ever used.
//!
//! The links sit in one dense `Vec` (a removal moves the last one into the
//! gap), found by entity index through a small hash map. Code that must visit
//! links in a fixed order (stepping, removals) asks for their slots sorted by
//! entity index, which is the order the old index-addressed table had, so
//! runs stay bit-identical.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use ponko_ecs::Entity;

/// Hashes an entity index (a `u32`) with one multiply: indices are small,
/// dense integers, and the map only needs them spread over its buckets.
#[derive(Default)]
pub(crate) struct IndexHasher(u64);

impl Hasher for IndexHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(8) ^ u64::from(b)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = u64::from(i).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

/// Below this many slots of spare capacity the table never shrinks, so small
/// worlds don't reallocate back and forth.
const KEEP: usize = 64;

/// Links by entity, packed. `T` is the physics world's per-entity record.
pub(crate) struct LinkTable<T> {
    items: Vec<T>,
    /// The entity of `items[k]`.
    entities: Vec<Entity>,
    /// Entity index to its slot in `items`.
    slots: HashMap<u32, u32, BuildHasherDefault<IndexHasher>>,
}

impl<T> Default for LinkTable<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            entities: Vec::new(),
            slots: HashMap::default(),
        }
    }
}

impl<T> LinkTable<T> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    #[cfg_attr(not(feature = "physics3d"), allow(dead_code))] // only 3D physics uses it
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The slot of `e`'s link (not of another entity that had its index).
    #[inline]
    pub(crate) fn slot(&self, e: Entity) -> Option<usize> {
        let k = *self.slots.get(&e.index())? as usize;
        (self.entities[k] == e).then_some(k)
    }

    #[inline]
    pub(crate) fn get(&self, e: Entity) -> Option<&T> {
        self.slot(e).map(|k| &self.items[k])
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        self.slot(e).map(|k| &mut self.items[k])
    }

    /// The link at `slot`.
    #[cfg_attr(not(feature = "physics3d"), allow(dead_code))] // only 3D physics uses it
    #[inline]
    pub(crate) fn at(&self, slot: usize) -> &T {
        &self.items[slot]
    }

    #[cfg_attr(not(feature = "physics3d"), allow(dead_code))] // only 3D physics uses it
    #[inline]
    pub(crate) fn at_mut(&mut self, slot: usize) -> &mut T {
        &mut self.items[slot]
    }

    /// Every link, in no particular order.
    #[cfg_attr(not(feature = "physics3d"), allow(dead_code))] // only 3D physics uses it
    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    /// Puts `e`'s link in. Whatever link had `e`'s index is replaced and
    /// returned (a recycled index whose old entity is gone).
    pub(crate) fn insert(&mut self, e: Entity, link: T) -> Option<T> {
        if let Some(&k) = self.slots.get(&e.index()) {
            self.entities[k as usize] = e;
            return Some(std::mem::replace(&mut self.items[k as usize], link));
        }
        self.slots.insert(e.index(), self.items.len() as u32);
        self.items.push(link);
        self.entities.push(e);
        None
    }

    /// Takes out the link of whatever entity has `index`. The last link
    /// moves into its slot, so slots taken before are stale after this.
    pub(crate) fn remove_index(&mut self, index: u32) -> Option<T> {
        let k = self.slots.remove(&index)? as usize;
        let link = self.items.swap_remove(k);
        self.entities.swap_remove(k);
        if let Some(moved) = self.entities.get(k) {
            self.slots.insert(moved.index(), k as u32);
        }
        self.shrink();
        Some(link)
    }

    /// Gives memory back once most of it is unused (a wave of entities
    /// left), keeping room to double so steady churn doesn't reallocate.
    fn shrink(&mut self) {
        let len = self.items.len();
        let keep = (len * 2).max(KEEP);
        if self.items.capacity() > len * 4 && self.items.capacity() > keep {
            self.items.shrink_to(keep);
            self.entities.shrink_to(keep);
        }
        if self.slots.capacity() > len * 4 && self.slots.capacity() > keep {
            self.slots.shrink_to(keep);
        }
    }

    /// Sets `out` to the slots of the links `keep` accepts, in entity-index
    /// order (the order the links had when they were stored by index).
    #[cfg_attr(not(feature = "physics3d"), allow(dead_code))] // only 3D physics uses it
    pub(crate) fn sorted_slots(&self, out: &mut Vec<u32>, mut keep: impl FnMut(&T) -> bool) {
        out.clear();
        out.extend((0..self.items.len() as u32).filter(|&k| keep(&self.items[k as usize])));
        out.sort_unstable_by_key(|&k| self.entities[k as usize].index());
    }

    /// Sets `out` to the entity indices of the links `pick` accepts, sorted.
    pub(crate) fn sorted_indices(&self, out: &mut Vec<u32>, mut pick: impl FnMut(&T) -> bool) {
        out.clear();
        out.extend(
            self.items
                .iter()
                .zip(&self.entities)
                .filter(|(link, _)| pick(link))
                .map(|(_, e)| e.index()),
        );
        out.sort_unstable();
    }

    /// Bytes the table holds on the heap (an estimate for the map).
    #[cfg(test)]
    pub(crate) fn heap_bytes(&self) -> usize {
        self.items.capacity() * size_of::<T>()
            + self.entities.capacity() * size_of::<Entity>()
            + self.slots.capacity() * (size_of::<(u32, u32)>() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ponko_ecs::World;

    #[test]
    fn removals_keep_lookups_right_and_order_by_index() {
        let mut world = World::new();
        let es: Vec<Entity> = (0..10)
            .map(|_| world.spawn((crate::d2::Transform::default(),)))
            .collect();
        let mut t = LinkTable::default();
        for &e in es.iter().rev() {
            t.insert(e, e.index());
        }
        for &e in &es[2..5] {
            assert_eq!(t.remove_index(e.index()), Some(e.index()));
        }
        for &e in &es {
            let alive = !(2..5).contains(&(e.index() as usize - es[0].index() as usize));
            assert_eq!(t.get(e).copied(), alive.then_some(e.index()));
        }
        let mut slots = Vec::new();
        t.sorted_slots(&mut slots, |_| true);
        let order: Vec<u32> = slots.iter().map(|&k| *t.at(k as usize)).collect();
        assert!(order.is_sorted());
        assert_eq!(order.len(), 7);
    }

    #[test]
    fn a_recycled_index_does_not_find_the_old_link() {
        let mut world = World::new();
        let old = world.spawn((crate::d2::Transform::default(),));
        let mut t = LinkTable::default();
        t.insert(old, 1);
        world.despawn(old);
        let new = world.spawn((crate::d2::Transform::default(),));
        assert_eq!(new.index(), old.index());
        assert_eq!(t.get(new), None);
        assert_eq!(t.insert(new, 2), Some(1));
        assert_eq!(t.get(old), None);
        assert_eq!(t.get(new), Some(&2));
    }

    #[test]
    fn capacity_follows_the_live_links() {
        let mut world = World::new();
        let mut t = LinkTable::default();
        let es: Vec<Entity> = (0..10_000)
            .map(|_| world.spawn((crate::d2::Transform::default(),)))
            .collect();
        for &e in &es {
            t.insert(e, [0u8; 256]);
        }
        for &e in &es[10..] {
            t.remove_index(e.index());
        }
        assert_eq!(t.len(), 10);
        assert!(t.heap_bytes() < 64 * 1024, "{} bytes for 10 links", t.heap_bytes());
    }
}
