//! Commands: changes recorded now, applied to the world later.

use std::mem::{MaybeUninit, size_of};
use std::ptr;
use std::sync::Arc;

use crate::bundle::{Bundle, Component};
use crate::entity::{Applying, Entity, Reserve};
use crate::world::World;

/// One recorded command: where its value sits in `Commands::bytes`, and the
/// function that reads it back out and runs it (`Some(world)`) or drops it
/// (`None`).
struct Op {
    call: unsafe fn(*mut MaybeUninit<u8>, Option<&mut World>),
    offset: usize,
}

/// Reads the `F` stored at `p` and runs it on `world`, or drops it.
///
/// # Safety
///
/// `p` must point at an initialized `F` written by `Commands::write`, and that
/// `F` must not be read again afterwards (it has been moved out).
unsafe fn call<F: FnOnce(&mut World)>(p: *mut MaybeUninit<u8>, world: Option<&mut World>) {
    // The byte buffer has no alignment, so the value is copied out unaligned.
    let f = unsafe { ptr::read_unaligned(p.cast::<F>()) };
    match world {
        Some(world) => f(world),
        None => drop(f),
    }
}

/// Reads the bundle stored at `p` and spawns it at the next id of the
/// buffer being applied, or drops it (the buffer gives its id back).
///
/// # Safety
///
/// As [`call`], for a `B`.
unsafe fn call_spawn<B: Bundle>(p: *mut MaybeUninit<u8>, world: Option<&mut World>) {
    let bundle = unsafe { ptr::read_unaligned(p.cast::<B>()) };
    match world {
        Some(world) => {
            let e = world.entities.take_applied();
            // Panics only before the entity is placed; the buffer then
            // gives `e` back.
            world.spawn_reserved(e, bundle);
            world.entities.placed();
        }
        None => drop(bundle),
    }
}

/// Reads the callback stored at `p` and runs it with the next id of the
/// buffer being applied, or drops it (the buffer gives its id back).
///
/// # Safety
///
/// As [`call`], for an `S`.
unsafe fn call_spawn_with<S: FnOnce(&mut World, Entity) -> bool>(p: *mut MaybeUninit<u8>, world: Option<&mut World>) {
    let spawn = unsafe { ptr::read_unaligned(p.cast::<S>()) };
    match world {
        Some(world) => {
            let e = world.entities.take_applied();
            // `spawn` places the entity or reports that it didn't.
            if spawn(world, e) {
                world.entities.placed();
            } else {
                world.entities.not_placed();
            }
        }
        None => drop(spawn),
    }
}

/// A list of world changes to apply later. Use it to spawn or despawn while a
/// query is running, since the query holds the world.
///
/// ```
/// # use ponko_ecs::*;
/// #[derive(Component)]
/// struct Health(i32);
///
/// let mut world = World::new();
/// world.spawn((Health(0),));
/// world.spawn((Health(5),));
///
/// let mut commands = Commands::new();
/// world.query::<(Entity, &Health)>().for_each(|(e, h)| {
///     if h.0 <= 0 {
///         commands.despawn(e);
///     }
/// });
/// world.apply(&mut commands);
/// assert_eq!(world.len(), 1);
/// ```
///
/// Commands run in the order they were recorded. Keep one `Commands` around
/// and reuse it: applying empties it without freeing its memory.
///
/// Recording doesn't allocate per command: closures are packed into one byte
/// buffer that is reused across applies.
///
/// # Spawning
///
/// [`spawn`](Self::spawn) returns the new entity's id right away, so later
/// commands can use it before the entity exists. For that the buffer is
/// tied to a world: `ctx.commands` and a view system's commands already
/// are, and [`World::commands`] makes one. The id is reserved in that world
/// when it is handed out: nothing else gets it, the entity becomes alive
/// when the spawn is applied, and if the spawn never happens (the buffer
/// is cleared or dropped, or an earlier command panics) the id is released
/// and stays dead.
///
/// ```
/// # use ponko_ecs::*;
/// #[derive(Component)]
/// struct Gun;
/// #[derive(Component)]
/// struct Bullet { from: Entity }
/// #[derive(Component)]
/// struct LastShot(Entity);
///
/// let mut world = World::new();
/// let gun = world.spawn((Gun,));
/// let mut commands = world.commands();
/// let bullet = commands.spawn((Bullet { from: gun },));
/// commands.insert(gun, LastShot(bullet));
/// assert!(!world.is_alive(bullet), "not spawned before it is applied");
///
/// world.apply(&mut commands);
/// assert_eq!(world.get::<Bullet>(bullet).unwrap().from, gun);
/// assert_eq!(world.get::<LastShot>(gun).unwrap().0, bullet);
/// ```
#[derive(Default)]
pub struct Commands {
    ops: Vec<Op>,
    /// The recorded values, packed back to back, unaligned.
    bytes: Vec<MaybeUninit<u8>>,
    /// The allocator of the world this buffer is tied to.
    reserve: Option<Arc<Reserve>>,
    /// Ids reserved for this buffer since its last apply, in the order it
    /// hands them out: `ids[..handed]` went to the recorded spawns, the
    /// rest are spares, so most spawns don't take the allocator's lock.
    /// Spares go back at apply, clear and drop.
    ids: Vec<Entity>,
    handed: usize,
    /// The allocator epoch of `ids[stale..]`; the ones before were
    /// reserved before a reset.
    epoch: u32,
    stale: usize,
    /// How many it handed out before the last apply that had spawns: how
    /// many it takes from the world's set-aside slots at its first spawn
    /// after an apply.
    last_handed: u32,
    /// The last chunk of `ids` is never-used indices (else set-aside
    /// slots).
    fresh: bool,
}

/// How many never-used ids a buffer reserves at once, at most. Set-aside
/// slots it takes as many as it spawned before its last apply; when they
/// run out, chunks double from one, so a buffer that spawns a few doesn't
/// hold many.
const MAX_CHUNK: u32 = 256;

impl Commands {
    /// An empty command list that isn't tied to a world: it can do
    /// everything but [`spawn`](Self::spawn) until it is
    /// ([`bind`](Self::bind)). [`World::commands`] makes a tied one.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Mark;
    /// # #[derive(Component)]
    /// # struct Thing;
    /// let mut world = World::new();
    /// let e = world.spawn((Thing,));
    /// let mut commands = Commands::new();
    /// assert!(!commands.is_bound_to(&world));
    /// commands.insert(e, Mark); // everything but `spawn` works untied
    /// world.apply(&mut commands);
    /// assert!(world.get::<Mark>(e).is_some());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ties this buffer to `world`, so it can [`spawn`](Self::spawn) there.
    /// Does nothing if it already is. Use it for a buffer kept in a struct
    /// before the world exists; otherwise [`World::commands`] is simpler.
    ///
    /// # Panics
    ///
    /// If the buffer holds spawns reserved in another world: apply or
    /// [`clear`](Self::clear) them first.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Marker;
    /// let mut world = World::new();
    /// let mut commands = Commands::new(); // kept in a struct, say
    /// commands.bind(&world);
    /// let e = commands.spawn((Marker,));
    /// world.apply(&mut commands);
    /// assert!(world.get::<Marker>(e).is_some());
    /// ```
    pub fn bind(&mut self, world: &World) {
        let theirs = world.entities.reserve();
        match &self.reserve {
            Some(ours) if Arc::ptr_eq(ours, theirs) => {}
            ours => {
                assert!(
                    ours.is_none() || self.handed == 0,
                    "this command buffer holds spawns for another world; apply or clear them before binding it to this one"
                );
                self.reserve = Some(Arc::clone(theirs));
            }
        }
    }

    /// `true` if this buffer is tied to `world` ([`bind`](Self::bind)).
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// let (world, other) = (World::new(), World::new());
    /// let commands = world.commands();
    /// assert!(commands.is_bound_to(&world));
    /// assert!(!commands.is_bound_to(&other));
    /// ```
    #[must_use]
    pub fn is_bound_to(&self, world: &World) -> bool {
        self.reserve
            .as_ref()
            .is_some_and(|r| Arc::ptr_eq(r, world.entities.reserve()))
    }

    /// Spawns an entity with these components when applied, and returns
    /// its id now. The id is valid in later commands of this buffer (and
    /// anywhere else: it is reserved in the buffer's world), and the entity
    /// is alive once the spawn has been applied. If the spawn never
    /// happens, the id stays dead.
    ///
    /// Commands run in recorded order, so a command recorded after the
    /// spawn sees the entity; one applied earlier (from another buffer)
    /// finds it not alive yet.
    ///
    /// Reserving takes a lock shared with the world's other buffers, so
    /// buffers on several threads (one per task of a parallel loop) can
    /// spawn at once and never get the same id. The ids are deterministic
    /// when the spawns are recorded in a deterministic order; from the
    /// threads of a parallel query they aren't (which item gets which id
    /// depends on timing): collect with `par_collect_into` and spawn in its
    /// order to keep them so.
    ///
    /// # Panics
    ///
    /// If the buffer isn't tied to a world: make it with
    /// [`World::commands`] or call [`bind`](Self::bind). When applied, if
    /// it is applied to another world than its own, or after that world was
    /// [`reset`](World::reset).
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Enemy;
    /// # #[derive(Component)]
    /// # struct Target(Entity);
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// let enemy = commands.spawn((Enemy,));
    /// // The id works at once, in later commands of the buffer.
    /// let marker = commands.spawn((Target(enemy),));
    /// assert!(!world.is_alive(enemy), "alive once the spawn is applied");
    /// world.apply(&mut commands);
    /// assert_eq!(world.get::<Target>(marker).unwrap().0, enemy);
    /// ```
    #[inline]
    #[track_caller]
    pub fn spawn<B: Bundle>(&mut self, bundle: B) -> Entity {
        let e = self.reserve_id();
        // Spawn commands take the buffer's ids in order when applied; the
        // ids of those that don't happen are given back.
        self.write(bundle, call_spawn::<B>);
        e
    }

    /// Reserves an entity id now and calls `spawn(world, id)` when applied
    /// to place the entity, for spawns that aren't a [`Bundle`] (the C
    /// API's raw component values). `spawn` returns `true` if it spawned
    /// the entity at exactly `id` (with [`World::spawn_reserved`]) and
    /// `false` if it didn't, which releases the id; so does a panic before
    /// the entity is placed. Prefer [`spawn`](Self::spawn).
    ///
    /// # Panics
    ///
    /// As [`spawn`](Self::spawn).
    #[doc(hidden)]
    #[inline]
    #[track_caller]
    pub fn spawn_with<S: FnOnce(&mut World, Entity) -> bool + Send + 'static>(&mut self, spawn: S) -> Entity {
        let e = self.reserve_id();
        // As in `spawn`.
        self.write(spawn, call_spawn_with::<S>);
        e
    }

    /// Hands out the next reserved id.
    #[inline]
    #[track_caller]
    fn reserve_id(&mut self) -> Entity {
        // Spares only exist in a buffer tied to a world.
        let e = match self.ids.get(self.handed) {
            Some(&e) => e,
            None => self.reserve_chunk(),
        };
        self.handed += 1;
        e
    }

    /// Takes more ids from the world's allocator (under its lock) when the
    /// spares run out, and returns the first.
    #[cold]
    #[track_caller]
    fn reserve_chunk(&mut self) -> Entity {
        let Some(reserve) = &self.reserve else {
            unbound();
        };
        // The first time after an apply, as many set-aside slots as the
        // buffer spawned before the last apply with spawns; later, double
        // what it spawned since.
        let pooled = if self.handed == 0 {
            self.last_handed.max(1)
        } else {
            u32::try_from(self.handed).unwrap_or(u32::MAX)
        };
        let had = self.ids.len();
        let (epoch, fresh) = reserve.reserve_many(pooled, pooled.min(MAX_CHUNK), &mut self.ids);
        self.fresh = fresh;
        if epoch != self.epoch {
            // The world was reset since the last chunk.
            self.epoch = epoch;
            self.stale = had;
        }
        self.ids[had]
    }

    /// Despawns `e` when applied (no-op if it's already gone by then).
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Thing;
    /// let mut world = World::new();
    /// let e = world.spawn((Thing,));
    /// let mut commands = world.commands();
    /// commands.despawn(e);
    /// assert!(world.is_alive(e), "nothing happens until the buffer is applied");
    /// world.apply(&mut commands);
    /// assert!(!world.is_alive(e));
    /// ```
    pub fn despawn(&mut self, e: Entity) {
        self.push(move |w| {
            w.despawn(e);
        });
    }

    /// Adds or replaces component `value` on `e` when applied.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Health(u32);
    /// let mut world = World::new();
    /// let e = world.spawn((Health(1),));
    /// let mut commands = world.commands();
    /// commands.insert(e, Health(10));
    /// world.apply(&mut commands);
    /// assert_eq!(world.get::<Health>(e).unwrap().0, 10);
    /// ```
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) {
        self.push(move |w| {
            w.insert(e, value);
        });
    }

    /// Removes component `T` from `e` when applied.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Burning;
    /// let mut world = World::new();
    /// let e = world.spawn((Burning,));
    /// let mut commands = world.commands();
    /// commands.remove::<Burning>(e);
    /// world.apply(&mut commands);
    /// assert!(world.get::<Burning>(e).is_none());
    /// ```
    pub fn remove<T: Component>(&mut self, e: Entity) {
        self.push(move |w| {
            w.remove::<T>(e);
        });
    }

    /// Runs any code with full world access when applied.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// commands.add(|world| {
    ///     world.insert_resource(7u32);
    /// });
    /// world.apply(&mut commands);
    /// assert_eq!(*world.resource::<u32>(), 7);
    /// ```
    pub fn add(&mut self, f: impl FnOnce(&mut World) + Send + 'static) {
        self.push(f);
    }

    /// Moves `f` into the byte buffer and records how to run it.
    fn push<F: FnOnce(&mut World) + Send + 'static>(&mut self, f: F) {
        self.write(f, call::<F>);
    }

    /// Moves `value` into the byte buffer and records `run` to read it back.
    /// `run` must read a `T` and nothing else.
    fn write<T: Send + 'static>(&mut self, value: T, run: unsafe fn(*mut MaybeUninit<u8>, Option<&mut World>)) {
        // Reserve both first, so nothing can fail between writing `value`
        // and recording it.
        self.ops.reserve(1);
        let offset = self.bytes.len();
        self.bytes.reserve(size_of::<T>());
        // SAFETY: the reserve above makes `offset..offset + size_of::<T>()`
        // in bounds of the allocation; the write is unaligned because the
        // buffer is bytes. Those bytes are now initialized, so the new length
        // covers only written memory. `run` reads `T` back exactly once.
        unsafe {
            ptr::write_unaligned(self.bytes.as_mut_ptr().add(offset).cast::<T>(), value);
            self.bytes.set_len(offset + size_of::<T>());
        }
        self.ops.push(Op { call: run, offset });
    }

    /// Number of recorded commands.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Thing;
    /// let mut world = World::new();
    /// let e = world.spawn((Thing,));
    /// let mut commands = world.commands();
    /// commands.despawn(e);
    /// assert_eq!(commands.len(), 1);
    /// world.apply(&mut commands);
    /// assert_eq!(commands.len(), 0, "applying empties the buffer");
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// `true` if nothing is recorded.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Thing;
    /// let mut world = World::new();
    /// let e = world.spawn((Thing,));
    /// let mut commands = world.commands();
    /// assert!(commands.is_empty());
    /// commands.despawn(e);
    /// assert!(!commands.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Forgets everything recorded without applying it (the values the
    /// commands hold are dropped), keeping the buffers for reuse.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// # #[derive(Component)]
    /// # struct Level(u32);
    /// let mut world = World::new();
    /// let mut commands = world.commands();
    /// commands.spawn((Level(1),));
    /// commands.clear();
    /// assert!(commands.is_empty());
    /// world.apply(&mut commands);
    /// assert_eq!(world.len(), 0);
    /// ```
    pub fn clear(&mut self) {
        self.drop_recorded();
        self.ops.clear();
        self.bytes.clear();
        // None of the spawns happened.
        let applying = self.take_handed();
        self.put_back(applying.finish());
    }

    /// Drops the values of every recorded command and empties `ops`, so none
    /// is read again: also when a value's `Drop` panics part way through (the
    /// values after it are then leaked, never dropped twice).
    fn drop_recorded(&mut self) {
        let base = self.bytes.as_mut_ptr();
        for op in self.ops.drain(..) {
            // SAFETY: recorded commands that were never applied; each read once.
            unsafe { (op.call)(base.add(op.offset), None) };
        }
    }

    /// Takes the handed-out ids, for applying or dropping the recorded
    /// spawns, and gives the spares back to the world.
    fn take_handed(&mut self) -> Applying {
        let mut list = std::mem::take(&mut self.ids);
        let handed = std::mem::take(&mut self.handed);
        if let Some(reserve) = &self.reserve {
            // Spares all come from the last chunk.
            reserve.give_back(&list[handed..], self.epoch, self.fresh);
        }
        list.truncate(handed);
        Applying::new(list, std::mem::take(&mut self.stale), self.epoch, self.reserve.clone())
    }

    /// Keeps `list`'s memory for the next ids.
    fn put_back(&mut self, mut list: Vec<Entity>) {
        list.clear();
        if self.ids.capacity() < list.capacity() {
            self.ids = list;
        }
    }

    pub(crate) fn apply(&mut self, world: &mut World) {
        let handed = self.handed;
        let ours = self.is_bound_to(world);
        let applying = self.take_handed();
        // Set free slots aside for as many spawns as this buffer reserved,
        // before its own despawns free more: the next ones reuse slots
        // instead of growing the table. Only for its own world; a buffer of
        // another one panics at its first spawn below.
        if handed > 0 {
            let n = u32::try_from(handed).unwrap_or(u32::MAX);
            self.last_handed = n;
            if ours {
                world.entities.refill(n);
            }
        }
        let outer = world.entities.begin_apply(applying);

        /// Ends the apply and empties the buffers, also when a command
        /// panics part way through: drops the commands not run yet, and
        /// gives back the ids of the spawns that didn't happen.
        struct Rest<'a> {
            commands: &'a mut Commands,
            world: &'a mut World,
            next: usize,
            outer: Option<Applying>,
        }
        impl Drop for Rest<'_> {
            fn drop(&mut self) {
                let c = &mut *self.commands;
                let base = c.bytes.as_mut_ptr();
                let applying = self.world.entities.end_apply(self.outer.take().unwrap_or_default());
                // Taken out first, so a panicking drop can't leave an op to
                // be read again (the rest are leaked then, never dropped
                // twice).
                let ops = std::mem::take(&mut c.ops);
                for op in &ops[self.next.min(ops.len())..] {
                    // SAFETY: ops from `next` on were never read.
                    unsafe { (op.call)(base.add(op.offset), None) };
                }
                c.bytes.clear();
                c.ops = ops;
                c.ops.clear();
                c.put_back(applying.finish());
            }
        }

        let mut rest = Rest {
            commands: self,
            world,
            next: 0,
            outer: Some(outer),
        };
        while let Some(&Op { call, offset }) = rest.commands.ops.get(rest.next) {
            // Count it as taken before it runs: if it panics, `Rest` must not
            // drop it again.
            rest.next += 1;
            // SAFETY: each op is read once, in order; the buffer isn't touched
            // while a command runs (it only gets the world).
            unsafe { call(rest.commands.bytes.as_mut_ptr().add(offset), Some(&mut *rest.world)) };
        }
        drop(rest);
        // Empty now: tie it to the world it was applied to, so a buffer
        // made before that world (or kept across a replaced world) spawns
        // there next.
        if !self.is_bound_to(world) {
            self.reserve = Some(Arc::clone(world.entities.reserve()));
        }
    }
}

#[cold]
#[track_caller]
fn unbound() -> ! {
    panic!(
        "Commands::spawn needs a buffer tied to a world, to reserve the new entity's id there: \
         make it with `world.commands()` or call `commands.bind(&world)` (`ctx.commands` already is)"
    )
}

impl Drop for Commands {
    fn drop(&mut self) {
        self.clear();
    }
}
