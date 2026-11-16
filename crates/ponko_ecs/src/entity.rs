//! Entity ids and the allocator that hands them out.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// A handle to a thing in the [`World`](crate::World). Cheap to copy and store.
///
/// Handles are generational: after an entity is despawned, its old handle stays
/// dead even if the slot is reused, so `world.get(old)` returns `None` instead
/// of someone else's data.
///
/// ```
/// use ponko_ecs::{Component, World};
/// #[derive(Component)]
/// struct Tag;
/// let mut world = World::new();
/// let a = world.spawn((Tag,));
/// let copy = a; // handles are `Copy`
/// world.despawn(a);
/// let b = world.spawn((Tag,)); // may reuse the slot ...
/// assert!(world.is_alive(b));
/// assert!(!world.is_alive(copy)); // ... but the old handle stays dead
/// assert_ne!(a, b);
/// ```
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(C)] // C/C++ read entity arrays directly: `{ uint32_t index, generation; }`
pub struct Entity {
    pub(crate) index: u32,
    pub(crate) generation: u32,
}

impl Entity {
    /// Packs the handle into 64 bits (for saving, or passing to C/C++).
    #[must_use]
    pub fn to_bits(self) -> u64 {
        (self.generation as u64) << 32 | self.index as u64
    }

    /// Inverse of [`to_bits`](Self::to_bits).
    #[must_use]
    pub fn from_bits(bits: u64) -> Self {
        Entity {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }

    /// Slot number. Slots are reused after despawn; use the whole `Entity` as an id.
    ///
    /// ```
    /// use ponko_ecs::{Component, World};
    /// #[derive(Component)]
    /// struct Tag;
    /// let mut world = World::new();
    /// let e = world.spawn((Tag,));
    /// assert_eq!(e.index(), 0); // the first slot of a new world
    /// ```
    #[must_use]
    pub fn index(self) -> u32 {
        self.index
    }
}

/// Saved as its 64-bit id ([`Entity::to_bits`]), so components that refer
/// to entities can be saved and loaded (with the `serde` feature).
///
/// ```
/// use ponko_ecs::Entity;
///
/// let e = Entity::from_bits(5 | 2 << 32);
/// let json = serde_json::to_string(&e).unwrap();
/// assert_eq!(json, (5u64 | 2 << 32).to_string());
/// assert_eq!(serde_json::from_str::<Entity>(&json).unwrap(), e);
/// ```
#[cfg(feature = "serde")]
impl serde::Serialize for Entity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(self.to_bits())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Entity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        u64::deserialize(d).map(|bits| Entity::from_bits(bits).remapped())
    }
}

thread_local! {
    /// The maps of the [`Entity::with_remap`] calls running on this thread,
    /// innermost last.
    static REMAPS: std::cell::RefCell<Vec<std::collections::HashMap<Entity, Entity>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Pops the innermost remap, also when `f` panics.
struct PopRemap;
impl Drop for PopRemap {
    fn drop(&mut self) {
        REMAPS.with(|r| r.borrow_mut().pop());
    }
}

impl Entity {
    /// Runs `f` with the saved ids in `map` replaced by their values as
    /// entities are read: every `Entity` deserialized on this thread while
    /// `f` runs (with the `serde` feature), and every [`remapped`](Self::remapped)
    /// call, reads a key of `map` as its value. Others read as written.
    ///
    /// This is how copies of saved entities keep pointing at each other: a
    /// prefab's parts get new ids when spawned, and a component of one that
    /// names another (an attachment, a joint) must name the new one. Only
    /// values typed `Entity` change, so a number that happens to equal a saved
    /// id is left alone. Calls nest: inside, only the innermost map applies.
    ///
    /// ```
    /// use ponko_ecs::Entity;
    /// use std::collections::HashMap;
    ///
    /// let (saved, spawned) = (Entity::from_bits(3), Entity::from_bits(17));
    /// let map = HashMap::from([(saved, spawned)]);
    /// let read = Entity::with_remap(&map, || saved.remapped());
    /// assert_eq!(read, spawned);
    /// assert_eq!(saved.remapped(), saved, "outside, ids read as written");
    /// ```
    pub fn with_remap<R>(map: &std::collections::HashMap<Entity, Entity>, f: impl FnOnce() -> R) -> R {
        REMAPS.with(|r| r.borrow_mut().push(map.clone()));
        let _pop = PopRemap;
        f()
    }

    /// This id as the innermost running [`with_remap`](Self::with_remap)
    /// maps it; itself when none runs or its map doesn't name it.
    #[must_use]
    pub fn remapped(self) -> Entity {
        REMAPS.with(|r| r.borrow().last().and_then(|m| m.get(&self).copied()).unwrap_or(self))
    }
}

impl fmt::Debug for Entity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Entity({}v{})", self.index, self.generation)
    }
}

#[derive(Copy, Clone)]
pub(crate) struct Location {
    pub(crate) archetype: u32,
    pub(crate) row: u32,
}

#[derive(Copy, Clone)]
struct Slot {
    /// Unused while the slot is reserved: the reserved id carries it.
    generation: u32,
    /// `archetype == DEAD` means the slot is free, `RESERVED` that it is
    /// set aside for command buffers or held by one.
    loc: Location,
}

const DEAD: u32 = u32::MAX;
const RESERVED: u32 = u32::MAX - 1;

const FREE_SLOT: Slot = Slot {
    generation: 0,
    loc: Location {
        archetype: DEAD,
        row: 0,
    },
};
const RESERVED_SLOT: Slot = Slot {
    generation: 0,
    loc: Location {
        archetype: RESERVED,
        row: 0,
    },
};

/// The part of a world's allocator that command buffers share
/// ([`World::commands`](crate::World::commands)): they take entity ids
/// from it while the world is borrowed elsewhere, also on other threads.
///
/// Ids come from two places. `pool` holds free slots the world set aside
/// for reservations (and ids whose spawn never happened, one generation
/// on); when it is empty, a reservation takes a never-used index from
/// `next`, which the world's own fresh allocations count up too. The world
/// tops `pool` up from its free list when a buffer that reserved is
/// applied, so a game that spawns and despawns through commands reuses its
/// slots instead of growing the slot table. Ids nobody takes for a while
/// go back to the world's free list ([`Entities::return_idle`]).
pub(crate) struct Reserve {
    /// The next index nobody has taken, shared with the world's fresh
    /// allocations.
    next: AtomicU32,
    pool: Mutex<Pool>,
}

struct Pool {
    /// Counts [`World::reset`](crate::World::reset)s: a reservation made
    /// before one belongs to the old numbering.
    epoch: u32,
    /// Taken from the end, so the start holds the ids that waited longest.
    ids: Vec<Entity>,
    /// Ids buffers took since the last [`Entities::return_idle`], less
    /// what they gave back unused.
    taken: usize,
    /// How many ids the pool keeps between frames: the most buffers took
    /// in one frame lately.
    want: usize,
    /// The most buffers took in one frame in the current window.
    peak: usize,
    /// `return_idle` calls in the current window, and how many make one.
    calls: u32,
    horizon: u32,
    /// A window lowered `want`, and no buffer took ids since.
    lowered: bool,
    /// The first buffer to take ids after the pool shrank ran out: it took
    /// never-used indices instead, so the window was too short.
    too_soon: bool,
}

/// How many [`World::return_idle_ids`](crate::World::return_idle_ids)
/// calls make one window: when buffers took fewer ids in every frame of a
/// window, the pool keeps fewer. It doubles each time a buffer then runs
/// out.
const FIRST_HORIZON: u32 = 2;

impl Pool {
    fn new(epoch: u32) -> Self {
        Pool {
            epoch,
            ids: Vec::new(),
            taken: 0,
            want: 0,
            peak: 0,
            calls: 0,
            horizon: FIRST_HORIZON,
            lowered: false,
            too_soon: false,
        }
    }
}

impl Reserve {
    fn new() -> Self {
        let reserve = Reserve {
            next: AtomicU32::new(0),
            pool: Mutex::new(Pool::new(0)),
        };
        // Some platforms allocate a mutex at its first lock: do it now, so
        // the per-frame `return_idle` never allocates.
        drop(reserve.pool());
        reserve
    }

    fn pool(&self) -> std::sync::MutexGuard<'_, Pool> {
        // The pool's state is valid after any panic (every change is one
        // push or pop), so a poisoned lock is still usable.
        self.pool.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Appends ids nobody else has or will get to `out`, under one lock,
    /// and returns the epoch they belong to and whether they are new
    /// indices: up to `pooled` from the pool, or `fresh` never-used indices
    /// if the pool is empty. They aren't alive until the world spawns them
    /// ([`Entities::claim`]); until then `get` doesn't find them. Ids not
    /// handed out go back with [`give_back`](Self::give_back).
    ///
    /// Taking several at once and giving the rest back hands out the same
    /// ids, in `out`'s order, as taking them one at a time: so which entity
    /// gets which id doesn't depend on how many buffers share the spawns
    /// (the scheduler gives each view system its own, and one at a time
    /// they share the game's).
    pub(crate) fn reserve_many(&self, pooled: u32, fresh: u32, out: &mut Vec<Entity>) -> (u32, bool) {
        let mut pool = self.pool();
        let from_pool = pool.ids.len().min(pooled as usize);
        // The first take after the pool shrank tells whether it shrank too soon.
        if std::mem::take(&mut pool.lowered) && from_pool == 0 {
            pool.too_soon = true;
        }
        if from_pool > 0 && from_pool == pool.ids.len() && out.is_empty() {
            // All of them: hand the whole list over (the pool keeps `out`'s
            // empty one, and its memory).
            std::mem::swap(&mut pool.ids, out);
            // In the order popping them one by one gives.
            out.reverse();
        } else if from_pool > 0 {
            let at = pool.ids.len() - from_pool;
            out.extend(pool.ids.drain(at..).rev());
        }
        let fresh = if from_pool == 0 { fresh } else { 0 };
        if fresh > 0 {
            // Under the lock, so a reset can't come between.
            let first = take_fresh(&self.next, fresh);
            out.extend((first..first + fresh).map(|index| Entity { index, generation: 0 }));
        }
        pool.taken = pool.taken.saturating_add(from_pool + fresh as usize);
        (pool.epoch, fresh > 0)
    }

    /// Returns ids reserved in `epoch` that were never handed out (the end
    /// of the last chunk; `fresh` if that was never-used indices), as if
    /// they had never been taken: never-used indices go back to the counter
    /// when nothing took any since; otherwise, and set-aside slots always,
    /// they go back to the pool, so the next one popped is the first of
    /// them. Ids from before a reset are forgotten.
    pub(crate) fn give_back(&self, ids: &[Entity], epoch: u32, fresh: bool) {
        let (Some(first), Some(last)) = (ids.first(), ids.last()) else {
            return;
        };
        let mut pool = self.pool();
        if pool.epoch != epoch {
            return;
        }
        pool.taken = pool.taken.saturating_sub(ids.len());
        if fresh
            && self
                .next
                .compare_exchange(last.index + 1, first.index, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            return;
        }
        pool.ids.extend(ids.iter().rev());
    }

    /// Takes `index` out of the pool if it waits there (no buffer holds
    /// it), with the generation its next entity gets. Slow (a scan), for
    /// restoring saved entities only.
    fn take_back(&self, index: u32) -> Option<u32> {
        let mut pool = self.pool();
        let at = pool.ids.iter().position(|e| e.index == index)?;
        Some(pool.ids.swap_remove(at).generation)
    }

    /// Gives back reserved ids, of `epoch`, whose spawns never happened.
    /// Their handles must stay dead, so each slot comes back one generation
    /// on (or is retired if that would wrap). Ids from before a reset are
    /// forgotten.
    pub(crate) fn release(&self, ids: &[Entity], epoch: u32) {
        if ids.is_empty() {
            return;
        }
        let mut pool = self.pool();
        if pool.epoch != epoch {
            return;
        }
        pool.ids.extend(ids.iter().filter_map(|e| {
            Some(Entity {
                index: e.index,
                generation: e.generation.checked_add(1)?,
            })
        }));
    }
}

/// Takes `n` never-used indices from `next`; returns the first.
fn take_fresh(next: &AtomicU32, n: u32) -> u32 {
    let first = next.fetch_add(n, Ordering::Relaxed);
    // `DEAD` and `RESERVED` are never indices. The counter is far from
    // wrapping: that would take four billion more allocations after this
    // panic.
    assert!(
        first.checked_add(n).is_some_and(|end| end < RESERVED),
        "too many entities; despawn unused entities or spawn fewer"
    );
    first
}

/// Moves up to `n` slots from the free list into `ids`, marked reserved.
fn fill(slots: &mut [Slot], free: &mut Vec<u32>, ids: &mut Vec<Entity>, mut n: usize) {
    ids.reserve(n.min(free.len()));
    while n > 0
        && let Some(index) = free.pop()
    {
        let slot = &mut slots[index as usize];
        // The free list can hold slots claimed since; skip those.
        if slot.loc.archetype == DEAD {
            ids.push(Entity {
                index,
                generation: slot.generation,
            });
            *slot = RESERVED_SLOT;
            n -= 1;
        }
    }
}

/// Room for one more in `v`, out of the way of the common path.
#[cold]
#[inline(never)]
fn grow<T>(v: &mut Vec<T>) {
    v.reserve(1);
}

/// Grows the table to `len` slots. The new indices are reserved (they
/// belong to command buffers) except the world's own, below `own_end`:
/// those become free slots.
#[cold]
fn grow_table(slots: &mut Vec<Slot>, free: &mut Vec<u32>, own_end: u32, len: usize) {
    let first = slots.len();
    if first >= len {
        return;
    }
    let own = (own_end as usize).clamp(first, len);
    slots.resize(own, FREE_SLOT);
    free.extend(first as u32..own as u32);
    slots.resize(len, RESERVED_SLOT);
}

/// How many never-used indices the world takes from the shared counter at
/// once for its own spawns, so a direct spawn touches nothing it shares
/// with command buffers.
const OWN_CHUNK: u32 = 256;

/// The ids of the command buffer being applied, for its spawn commands:
/// each takes the next one. See [`Entities::begin_apply`].
#[derive(Default)]
pub(crate) struct Applying {
    /// The ids the buffer handed out, in the order it recorded its spawns.
    ids: Vec<Entity>,
    /// How many spawn commands took theirs.
    next: usize,
    /// The ids before this index can't be spawned here: reserved before a
    /// reset, or (all of them) in another world.
    valid_from: usize,
    /// The ids before this index were reserved before a reset.
    stale: usize,
    /// The allocator epoch of the others.
    epoch: u32,
    /// The buffer belongs to another world.
    foreign: bool,
    /// The last id taken isn't placed yet: if its command fails, it must be
    /// given back.
    placing: bool,
    /// The buffer's allocator, to give back the ids whose spawns don't
    /// happen.
    reserve: Option<Arc<Reserve>>,
}

impl Applying {
    #[cold]
    #[track_caller]
    fn invalid(&self) -> ! {
        if self.foreign {
            panic!("commands tied to one world were applied to another; apply them to the world that made them");
        }
        panic!(
            "a spawn recorded before `World::reset` (or a load) can't be applied after it: its id means another entity now"
        );
    }

    pub(crate) fn new(ids: Vec<Entity>, stale: usize, epoch: u32, reserve: Option<Arc<Reserve>>) -> Self {
        Applying {
            ids,
            next: 0,
            valid_from: usize::MAX,
            stale,
            epoch,
            foreign: true,
            placing: false,
            reserve,
        }
    }

    /// Gives back the ids whose spawns didn't happen (those not taken, and
    /// the last one taken if it wasn't placed) and returns the list.
    pub(crate) fn finish(mut self) -> Vec<Entity> {
        let from = self.next - usize::from(self.placing);
        let from = from.max(self.stale);
        if let Some(reserve) = &self.reserve
            && from < self.ids.len()
        {
            reserve.release(&self.ids[from..], self.epoch);
        }
        std::mem::take(&mut self.ids)
    }
}

pub(crate) struct Entities {
    slots: Vec<Slot>,
    free: Vec<u32>,
    alive: usize,
    /// The world's own never-used indices, for its spawns, are
    /// `slots.len()..own_end`: a direct spawn only compares the table's
    /// length with this, and touches nothing shared with command buffers.
    own_end: u32,
    /// Shared with command buffers; see [`Reserve`].
    reserve: Arc<Reserve>,
    /// The world's copy of the pool's epoch, read without the lock.
    epoch: u32,
    /// The command buffer being applied.
    applying: Applying,
}

impl Default for Entities {
    fn default() -> Self {
        Entities {
            slots: Vec::new(),
            free: Vec::new(),
            alive: 0,
            own_end: 0,
            reserve: Arc::new(Reserve::new()),
            epoch: 0,
            applying: Applying::default(),
        }
    }
}

impl Entities {
    pub(crate) fn alloc(&mut self) -> Entity {
        // The free list can hold slots that `alloc_at` claimed since; skip those.
        while let Some(index) = self.free.pop() {
            let slot = &self.slots[index as usize];
            if slot.loc.archetype == DEAD {
                self.alive += 1;
                return Entity {
                    index,
                    generation: slot.generation,
                };
            }
        }
        self.alloc_fresh()
    }

    /// The next never-used index, from the world's own range.
    #[inline]
    fn alloc_fresh(&mut self) -> Entity {
        if self.slots.len() >= self.own_end as usize {
            self.take_own_chunk();
        }
        let index = self.slots.len() as u32;
        if self.slots.len() == self.slots.capacity() {
            grow(&mut self.slots);
        }
        self.slots.push(FREE_SLOT);
        self.alive += 1;
        Entity { index, generation: 0 }
    }

    /// Takes the next never-used indices for the world. Command buffers
    /// may have taken those between the end of the table and them: they
    /// stay reserved.
    #[cold]
    #[inline(never)]
    fn take_own_chunk(&mut self) {
        let first = take_fresh(&self.reserve.next, OWN_CHUNK);
        self.grow_reserved(first as usize);
        self.own_end = first + OWN_CHUNK;
    }

    /// Grows the table to `len` slots; see [`grow_table`].
    fn grow_reserved(&mut self, len: usize) {
        grow_table(&mut self.slots, &mut self.free, self.own_end, len);
    }

    /// Claims exactly `e` (for restoring saved worlds). Fails if its slot is
    /// in use or held by a command buffer, or if `e` is older than the
    /// slot's generation (a handle that was despawned here must stay dead).
    /// A free slot the world set aside for command buffers is taken back.
    /// The caller must then place the entity with [`set`](Self::set) before
    /// anything reads it.
    pub(crate) fn alloc_at(&mut self, e: Entity) -> bool {
        let index = e.index as usize;
        // The highest index `take_fresh` hands out is `RESERVED - 2`.
        if e.index >= RESERVED - 1 {
            return false;
        }
        if (self.slots.len()..self.own_end as usize).contains(&index) {
            // The world's own never-used indices: those before `e` become
            // free slots.
            let first = self.slots.len() as u32;
            self.slots.resize(index + 1, FREE_SLOT);
            self.free.extend(first..e.index);
        } else if let next = self.reserve.next.fetch_max(e.index + 1, Ordering::Relaxed)
            && next <= e.index
        {
            // Take every never-used index up to `e` for the world.
            // Indices in between that buffers took stay reserved; the
            // world's new ones are free, `e` is claimed below.
            self.grow_reserved(next as usize);
            let first_new = self.slots.len() as u32;
            self.slots.resize(index + 1, FREE_SLOT);
            self.free.extend(first_new..e.index);
        } else if self.slots.get(index).is_none_or(|s| s.loc.archetype == RESERVED) {
            // Set aside for command buffers: free if it still waits in the
            // pool, taken if a buffer holds it.
            let Some(generation) = self.reserve.take_back(e.index) else {
                return false;
            };
            self.grow_reserved(index + 1);
            self.slots[index] = Slot {
                generation,
                loc: Location {
                    archetype: DEAD,
                    row: 0,
                },
            };
            // A free slot of the world's again, also if `e` is too old for
            // it below (`alloc` skips it if it is claimed here).
            self.free.push(e.index);
        }
        let slot = &mut self.slots[index];
        if slot.loc.archetype != DEAD || e.generation < slot.generation {
            return false;
        }
        // Mark it taken right away; `set` fills in the real location.
        slot.generation = e.generation;
        slot.loc = Location { archetype: 0, row: 0 };
        self.alive += 1;
        true
    }

    /// The allocator command buffers share.
    pub(crate) fn reserve(&self) -> &Arc<Reserve> {
        &self.reserve
    }

    /// Starts applying a command buffer: its spawn commands take their ids
    /// from `applying`, in order. Returns what an apply in progress had
    /// (a command may apply another buffer), for [`end_apply`](Self::end_apply).
    pub(crate) fn begin_apply(&mut self, mut applying: Applying) -> Applying {
        let ours = applying.reserve.as_ref().is_some_and(|r| Arc::ptr_eq(r, &self.reserve));
        applying.foreign = !ours;
        applying.valid_from = if ours && applying.epoch == self.epoch {
            applying.stale
        } else {
            usize::MAX
        };
        std::mem::replace(&mut self.applying, applying)
    }

    /// Ends the apply [`begin_apply`](Self::begin_apply) started, going
    /// back to `outer`.
    pub(crate) fn end_apply(&mut self, outer: Applying) -> Applying {
        std::mem::replace(&mut self.applying, outer)
    }

    /// The id of the next spawn command of the buffer being applied. The
    /// caller places it with [`claim`](Self::claim), then calls
    /// [`placed`](Self::placed), or gives it back with
    /// [`not_placed`](Self::not_placed).
    ///
    /// # Panics
    ///
    /// If the buffer belongs to another world, or reserved it before a
    /// reset.
    #[inline]
    pub(crate) fn take_applied(&mut self) -> Entity {
        let a = &mut self.applying;
        let i = a.next;
        a.next = i + 1;
        a.placing = true;
        if i < a.valid_from {
            self.applying.invalid();
        }
        self.applying.ids[i]
    }

    /// The id [`take_applied`](Self::take_applied) gave is placed.
    #[inline]
    pub(crate) fn placed(&mut self) {
        self.applying.placing = false;
    }

    /// The id [`take_applied`](Self::take_applied) gave won't be placed:
    /// give it back.
    pub(crate) fn not_placed(&mut self) {
        let a = &mut self.applying;
        a.placing = false;
        if let Some(reserve) = &a.reserve {
            reserve.release(&a.ids[a.next - 1..a.next], a.epoch);
        }
    }

    /// Makes reserved `e` the next entity to place, at its generation; the
    /// caller places it with [`set`](Self::set) right after.
    ///
    /// # Panics
    ///
    /// If `e` isn't reserved (nothing is changed then).
    #[inline]
    pub(crate) fn claim(&mut self, e: Entity) {
        let index = e.index as usize;
        if index >= self.slots.len() {
            // A fresh index a buffer took past the end of the table.
            assert!(
                e.index < self.reserve.next.load(Ordering::Relaxed),
                "entity {e:?} was never reserved in this world; reserve it in this world before spawning"
            );
            self.grow_reserved(index + 1);
        }
        let slot = &mut self.slots[index];
        assert!(
            slot.loc.archetype == RESERVED,
            "entity {e:?} isn't a reserved id of this world, or it was spawned already; reserve a fresh id"
        );
        slot.generation = e.generation;
        slot.loc = Location { archetype: 0, row: 0 };
        self.alive += 1;
    }

    /// Sets up to `n` free slots aside for command buffers, so their next
    /// `n` spawns reuse slots instead of growing the table. A buffer calls
    /// it at apply with what it handed out, so the pool keeps the size the
    /// buffers take from it; [`return_idle`](Self::return_idle) evens it
    /// out once per frame.
    pub(crate) fn refill(&mut self, n: u32) {
        let mut pool = self.reserve.pool();
        fill(&mut self.slots, &mut self.free, &mut pool.ids, n as usize);
    }

    /// Sizes the pool for the next frame: as many ids as buffers took in
    /// one frame lately, topped up from the free list or with the extra
    /// (those that waited longest) given back to it. "Lately" is a window
    /// of `horizon` calls; it doubles when a buffer runs out right after
    /// the pool shrank. Called once per frame; see
    /// [`World::return_idle_ids`](crate::World::return_idle_ids).
    pub(crate) fn return_idle(&mut self) {
        let mut pool = self.reserve.pool();
        let pool = &mut *pool;
        if pool.too_soon {
            pool.too_soon = false;
            pool.horizon = pool.horizon.saturating_mul(2);
        }
        let taken = std::mem::take(&mut pool.taken);
        pool.peak = pool.peak.max(taken);
        pool.want = pool.want.max(taken);
        pool.calls += 1;
        if pool.calls >= pool.horizon {
            if pool.peak < pool.want {
                pool.lowered = true;
                pool.want = pool.peak;
            }
            pool.peak = 0;
            pool.calls = 0;
        }
        if let Some(extra) = pool.ids.len().checked_sub(pool.want) {
            for e in pool.ids.drain(..extra) {
                // Pooled ids may lie past the end of the table (fresh
                // indices given back), and carry the slot's next generation.
                grow_table(&mut self.slots, &mut self.free, self.own_end, e.index as usize + 1);
                self.slots[e.index as usize] = Slot {
                    generation: e.generation,
                    loc: Location {
                        archetype: DEAD,
                        row: 0,
                    },
                };
                self.free.push(e.index);
            }
        } else {
            let missing = pool.want - pool.ids.len();
            fill(&mut self.slots, &mut self.free, &mut pool.ids, missing);
        }
    }

    pub(crate) fn free(&mut self, e: Entity) {
        let slot = &mut self.slots[e.index as usize];
        slot.loc.archetype = DEAD;
        self.alive -= 1;
        // A slot whose generation would wrap is retired, so old handles can never come back to life.
        if let Some(g) = slot.generation.checked_add(1) {
            slot.generation = g;
            if self.free.len() == self.free.capacity() {
                grow(&mut self.free);
            }
            self.free.push(e.index);
        }
    }

    /// Forgets every id, as a new allocator, but keeps sharing the same
    /// [`Reserve`] with existing command buffers: what they reserve from
    /// now on is valid here, what they reserved before is not (the epoch
    /// tells).
    pub(crate) fn reset(&mut self) {
        self.own_end = 0;
        let mut pool = self.reserve.pool();
        *pool = Pool::new(pool.epoch.wrapping_add(1));
        // Under the lock: no reservation sees the old count afterwards.
        self.reserve.next.store(0, Ordering::Relaxed);
        self.epoch = pool.epoch;
        drop(pool);
        self.slots.clear();
        self.free.clear();
        self.alive = 0;
    }

    #[inline]
    pub(crate) fn get(&self, e: Entity) -> Option<Location> {
        let s = self.slots.get(e.index as usize)?;
        // One compare rules out free and reserved slots alike: `RESERVED`
        // is `DEAD - 1`.
        (s.generation == e.generation && s.loc.archetype < RESERVED).then_some(s.loc)
    }

    #[inline]
    pub(crate) fn set(&mut self, e: Entity, loc: Location) {
        self.slots[e.index as usize].loc = loc;
    }

    #[inline]
    pub(crate) fn set_row(&mut self, e: Entity, row: usize) {
        self.slots[e.index as usize].loc.row = row as u32;
    }

    pub(crate) fn len(&self) -> usize {
        self.alive
    }

    /// Size of the slot table: one past the highest index ever handed out.
    pub(crate) fn slots(&self) -> usize {
        self.slots.len()
    }
}
