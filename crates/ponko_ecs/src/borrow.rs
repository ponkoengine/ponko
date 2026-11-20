//! Borrowing several things from a [`World`] at once: a few resources,
//! resources next to a query, and other entities while a query runs.
//!
//! A query borrows the whole world, and so does `resource_mut`. The types
//! here declare up front what else is needed, so the check for overlap runs
//! once, when the borrow is made, and never per entity.

use std::any::TypeId;
use std::marker::PhantomData;

use crate::entity::{Entities, Entity};
use crate::query::{Query, QueryData, check_aliasing};
use crate::seal::Seal;
use crate::storage::{Archetype, ComponentId};
use crate::world::World;

// ---- several resources at once ------------------------------------------------

/// The resource types [`World::resources_mut`] borrows together: a tuple of
/// 1 to 8 different resource types. Implemented by this crate only: the
/// trait is sealed.
pub trait ResourceTuple {
    #[doc(hidden)]
    const SEAL: Seal;
    /// One `&mut` per resource, in the tuple's order.
    type Refs<'w>;
    #[doc(hidden)]
    fn fetch(world: &mut World) -> Self::Refs<'_>;
}

macro_rules! resource_tuple {
    ($($t:ident $v:ident),*) => {
        impl<$($t: Send + Sync + 'static),*> ResourceTuple for ($($t,)*) {
            const SEAL: Seal = Seal;
            type Refs<'w> = ($(&'w mut $t,)*);
            #[track_caller]
            fn fetch(world: &mut World) -> Self::Refs<'_> {
                let [$($v),*] = world.resource_boxes(
                    "resources_mut",
                    [$(TypeId::of::<$t>()),*],
                    [$(std::any::type_name::<$t>()),*],
                );
                ($($v.downcast_mut::<$t>().unwrap(),)*)
            }
        }
    };
}
resource_tuple!(A a);
resource_tuple!(A a, B b);
resource_tuple!(A a, B b, C c);
resource_tuple!(A a, B b, C c, D d);
resource_tuple!(A a, B b, C c, D d, E e);
resource_tuple!(A a, B b, C c, D d, E e, F f);
resource_tuple!(A a, B b, C c, D d, E e, F f, G g);
resource_tuple!(A a, B b, C c, D d, E e, F f, G g, H h);

// ---- resources next to a query ---------------------------------------------------

/// The resources a query carries with [`Query::with_resources`]: `&R` to
/// read one, `&mut R` to change it, or a tuple of those (up to 8), each a
/// different resource. Implemented by this crate only: the trait is sealed.
pub trait ResourceSet {
    #[doc(hidden)]
    const SEAL: Seal;
    /// What the loop body receives next to the query's item.
    type Item<'a>;
    #[doc(hidden)]
    type Taken;
    /// Reports `(type, name, written)` for each resource.
    #[doc(hidden)]
    fn declare(f: &mut impl FnMut(TypeId, &'static str, bool));
    /// Moves the resources out of the world. They are all present.
    #[doc(hidden)]
    fn take(world: &mut World) -> Self::Taken;
    #[doc(hidden)]
    fn restore(world: &mut World, taken: Self::Taken);
    #[doc(hidden)]
    fn item(taken: &mut Self::Taken) -> Self::Item<'_>;
}

impl<R: Send + Sync + 'static> ResourceSet for &R {
    const SEAL: Seal = Seal;
    type Item<'a> = &'a R;
    type Taken = Box<R>;
    fn declare(f: &mut impl FnMut(TypeId, &'static str, bool)) {
        f(TypeId::of::<R>(), std::any::type_name::<R>(), false)
    }
    fn take(world: &mut World) -> Box<R> {
        world.take_resource_box()
    }
    fn restore(world: &mut World, taken: Box<R>) {
        world.put_resource_box(taken)
    }
    #[inline(always)]
    fn item(taken: &mut Box<R>) -> &R {
        taken
    }
}

impl<R: Send + Sync + 'static> ResourceSet for &mut R {
    const SEAL: Seal = Seal;
    type Item<'a> = &'a mut R;
    type Taken = Box<R>;
    fn declare(f: &mut impl FnMut(TypeId, &'static str, bool)) {
        f(TypeId::of::<R>(), std::any::type_name::<R>(), true)
    }
    fn take(world: &mut World) -> Box<R> {
        world.take_resource_box()
    }
    fn restore(world: &mut World, taken: Box<R>) {
        world.put_resource_box(taken)
    }
    #[inline(always)]
    fn item(taken: &mut Box<R>) -> &mut R {
        taken
    }
}

macro_rules! resource_set {
    ($($t:ident $i:tt),*) => {
        impl<$($t: ResourceSet),*> ResourceSet for ($($t,)*) {
            const SEAL: Seal = Seal;
            type Item<'a> = ($($t::Item<'a>,)*);
            type Taken = ($($t::Taken,)*);
            fn declare(f: &mut impl FnMut(TypeId, &'static str, bool)) {
                $($t::declare(f);)*
            }
            fn take(world: &mut World) -> Self::Taken {
                ($($t::take(world),)*)
            }
            fn restore(world: &mut World, taken: Self::Taken) {
                $($t::restore(world, taken.$i);)*
            }
            #[inline(always)]
            fn item(taken: &mut Self::Taken) -> Self::Item<'_> {
                ($($t::item(&mut taken.$i),)*)
            }
        }
    };
}
resource_set!(A 0);
resource_set!(A 0, B 1);
resource_set!(A 0, B 1, C 2);
resource_set!(A 0, B 1, C 2, D 3);
resource_set!(A 0, B 1, C 2, D 3, E 4);
resource_set!(A 0, B 1, C 2, D 3, E 4, F 5);
resource_set!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
resource_set!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);

/// A query that carries resources, from [`Query::with_resources`].
///
/// The resources are out of the world while this lives and go back when it
/// is dropped, after the loop or on a panic. Leaking it (`std::mem::forget`)
/// leaks them too: they never return, and the world has no such resources
/// afterwards. Let it drop.
#[must_use = "a query does nothing until iterated"]
pub struct QueryResources<'w, Q: QueryData, R: ResourceSet> {
    query: Query<'w, Q>,
    /// The resources, out of the world until this is dropped. Nothing can
    /// look them up meanwhile: the query borrows the world.
    taken: Option<R::Taken>,
}

impl<Q: QueryData, R: ResourceSet> QueryResources<'_, Q, R> {
    /// Runs `f` for every matching entity, with the query's item and the
    /// resources. As fast as [`Query::for_each`].
    #[inline]
    pub fn for_each(&mut self, mut f: impl FnMut(Q::Item<'_>, R::Item<'_>)) {
        let taken = self.taken.as_mut().expect("resources are held until drop");
        self.query.for_each(|item| f(item, R::item(taken)));
    }

    /// The resources, for work before or after the loop.
    pub fn resources(&mut self) -> R::Item<'_> {
        R::item(self.taken.as_mut().expect("resources are held until drop"))
    }
}

impl<Q: QueryData, R: ResourceSet> Drop for QueryResources<'_, Q, R> {
    fn drop(&mut self) {
        if let Some(taken) = self.taken.take() {
            R::restore(self.query.world, taken);
        }
    }
}

// ---- other entities while a query runs ----------------------------------------------

/// Query data that only reads: `&T`, `Option<&T>`, [`Entity`], or a tuple of
/// those. [`Lookup::get`] needs it to hand out several items at once, and
/// [`World::query_ref`] to query a shared world.
/// Implemented by this crate only: the trait is sealed.
///
/// # Safety
/// `fetch` must create no mutable reference. `ids_ref` reports an id no
/// archetype has for a component the world never registered.
pub unsafe trait ReadOnlyQueryData: QueryData {
    #[doc(hidden)]
    const READ_ONLY_SEAL: Seal;
    /// The ids without registering anything: a component the world doesn't
    /// know yet gets an id no archetype has.
    #[doc(hidden)]
    fn ids_ref(world: &World) -> Self::Ids;
}

// SAFETY: these fetch `&T`, `Option<&T>` and a copy of the entity id.
unsafe impl<T: crate::Component> ReadOnlyQueryData for &T {
    const READ_ONLY_SEAL: Seal = Seal;
    fn ids_ref(world: &World) -> ComponentId {
        world.component_id::<T>().unwrap_or(ComponentId::UNKNOWN)
    }
}
unsafe impl<T: crate::Component> ReadOnlyQueryData for Option<&T> {
    const READ_ONLY_SEAL: Seal = Seal;
    fn ids_ref(world: &World) -> ComponentId {
        world.component_id::<T>().unwrap_or(ComponentId::UNKNOWN)
    }
}
unsafe impl ReadOnlyQueryData for Entity {
    const READ_ONLY_SEAL: Seal = Seal;
    fn ids_ref(_: &World) {}
}

macro_rules! read_only_tuple {
    ($($t:ident $i:tt),*) => {
        // SAFETY: a tuple fetches what its elements fetch.
        unsafe impl<$($t: ReadOnlyQueryData),*> ReadOnlyQueryData for ($($t,)*) {
            const READ_ONLY_SEAL: Seal = Seal;
            fn ids_ref(world: &World) -> Self::Ids {
                ($($t::ids_ref(world),)*)
            }
        }
    };
}
read_only_tuple!(A 0);
read_only_tuple!(A 0, B 1);
read_only_tuple!(A 0, B 1, C 2);
read_only_tuple!(A 0, B 1, C 2, D 3);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
read_only_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);

/// Panics if `Q` and the lookup `L` would alias: both touch a component and
/// at least one of them mutably.
fn check_disjoint<Q: QueryData, L: QueryData>(world: &World, q: &Q::Ids, l: &L::Ids) {
    const MAX: usize = 32;
    let mut main = [(ComponentId(0), false); MAX];
    let mut n = 0;
    // `check_aliasing` already capped `Q` at MAX components.
    Q::access(q, &mut |id, mutable, _| {
        main[n] = (id, mutable);
        n += 1;
    });
    L::access(l, &mut |id, mutable, _| {
        for &(other, other_mut) in &main[..n] {
            if other == id && (mutable || other_mut) {
                panic!(
                    "lookup `{}` and query `{}` both access `{}`, at least once mutably; that would alias",
                    std::any::type_name::<L>(),
                    std::any::type_name::<Q>(),
                    world.component_info(id).name()
                );
            }
        }
    });
}

/// A query that can also look at other entities while it loops, from
/// [`Query::with_lookup`].
#[must_use = "a query does nothing until iterated"]
pub struct QueryLookup<'w, Q: QueryData, L: QueryData> {
    query: Query<'w, Q>,
    ids: L::Ids,
}

impl<Q: QueryData, L: QueryData> QueryLookup<'_, Q, L> {
    /// Runs `f` for every matching entity, with the query's item and a
    /// [`Lookup`] for reaching other entities (or other components of the
    /// same one).
    #[inline]
    pub fn for_each(&mut self, mut f: impl FnMut(Q::Item<'_>, &mut Lookup<'_, L>)) {
        let world: &World = self.query.world;
        let mut lookup = Lookup {
            entities: &raw const world.entities,
            archetypes: &raw const world.archetypes[..],
            ids: self.ids,
            stamp: world.tracks_changes().then(|| world.changes.stamp()),
            _world: PhantomData,
            _data: PhantomData,
        };
        self.query.for_each(|item| f(item, &mut lookup));
    }
}

/// Reaches entities by handle while a query loops, without ending the loop
/// first. Passed to the body of [`QueryLookup::for_each`].
///
/// `L` says what it fetches, like a query: `&T`, `&mut T`, `Option<..>`,
/// [`Entity`] or a tuple.
pub struct Lookup<'a, L: QueryData> {
    // Raw, because the running query holds the world's `&mut`. It only reads
    // these two while it loops (nothing can spawn, despawn or move a row),
    // and so does the lookup.
    entities: *const Entities,
    archetypes: *const [Archetype],
    ids: L::Ids,
    /// The world's change stamp, if it tracks any component: what
    /// [`get_mut`](Self::get_mut) writes for the tracked components it hands
    /// out, as [`World::get_mut`] does.
    stamp: Option<u32>,
    _world: PhantomData<&'a World>,
    _data: PhantomData<fn() -> L>,
}

impl<L: QueryData> Lookup<'_, L> {
    #[inline]
    fn locate(&self, e: Entity) -> Option<(&Archetype, L::State, usize)> {
        // SAFETY: both point into the world the running query borrows for
        // longer than this lookup lives, and nothing writes to the entity
        // table or the archetype list while a query loops.
        let (entities, archetypes) = unsafe { (&*self.entities, &*self.archetypes) };
        let loc = entities.get(e)?;
        let arch = &archetypes[loc.archetype as usize];
        let state = L::state(arch, &self.ids)?;
        Some((arch, state, loc.row as usize))
    }

    /// `L`'s item for entity `e`, or `None` if it is dead or lacks one of
    /// the components. Only for lookups that read; several results can be
    /// held at once.
    #[inline]
    pub fn get(&self, e: Entity) -> Option<L::Item<'_>>
    where
        L: ReadOnlyQueryData,
    {
        let (_, state, row) = self.locate(e)?;
        // SAFETY: `e` is alive, so `row` is a row of its archetype. `L` only
        // reads, and the query writes none of `L`'s components (checked in
        // `with_lookup`).
        Some(unsafe { L::fetch(state, row) })
    }

    /// `L`'s item for entity `e`, with its `&mut` components writable, or
    /// `None` if it is dead or lacks one of the components. One result at a
    /// time.
    ///
    /// Like [`World::get_mut`], it counts as a change of every tracked
    /// component `L` writes, so a later `Changed<T>` query sees it.
    #[inline]
    pub fn get_mut(&mut self, e: Entity) -> Option<L::Item<'_>> {
        let (arch, state, row) = self.locate(e)?;
        if let Some(stamp) = self.stamp
            && arch.tracked()
        {
            // The query writes none of these stamps and reads none of them
            // through a filter (both checked in `with_lookup`).
            crate::change::mark_row::<L>(arch, &self.ids, stamp, row);
        }
        // SAFETY: `e` is alive, so `row` is a row of its archetype. The query
        // touches none of the components `L` writes and writes none that `L`
        // reads (checked in `with_lookup`), `L` doesn't alias itself, and
        // `&mut self` keeps this the only item out of the lookup.
        Some(unsafe { L::fetch(state, row) })
    }

    /// `true` if `e` is alive and has every component `L` requires.
    #[inline]
    #[must_use]
    pub fn contains(&self, e: Entity) -> bool {
        self.locate(e).is_some()
    }
}

impl<'w, Q: QueryData> Query<'w, Q> {
    /// Carries resources into the loop, so its body can read and change them
    /// without copying values out before the query or putting them back
    /// after. `R` is `&T` to read a resource, `&mut T` to change it, or a
    /// tuple of those (up to 8).
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Pos(f32);
    /// struct Speed(f32);
    /// struct Moved(u32);
    ///
    /// let mut world = World::new();
    /// world.insert_resource(Speed(2.0));
    /// world.insert_resource(Moved(0));
    /// world.spawn((Pos(1.0),));
    ///
    /// world
    ///     .query::<&mut Pos>()
    ///     .with_resources::<(&Speed, &mut Moved)>()
    ///     .for_each(|pos, (speed, moved)| {
    ///         pos.0 += speed.0;
    ///         moved.0 += 1;
    ///     });
    /// assert_eq!(world.resource::<Moved>().0, 1);
    /// ```
    ///
    /// Call it last, after the filters. The resources are checked here, once,
    /// not per entity, and carrying them doesn't allocate.
    ///
    /// # Panics
    /// If a resource is missing, or named twice.
    #[track_caller]
    pub fn with_resources<R: ResourceSet>(self) -> QueryResources<'w, Q, R> {
        self.world.check_resources::<R>("with_resources");
        let taken = R::take(self.world);
        QueryResources {
            query: self,
            taken: Some(taken),
        }
    }

    /// Lets the loop body look at other entities by handle: read their
    /// components with [`Lookup::get`], or change them with
    /// [`Lookup::get_mut`]. `L` is what to fetch from them, written like a
    /// query: `&T`, `&mut T`, `Option<&T>`, [`Entity`] or a tuple.
    ///
    /// ```
    /// # use ponko_ecs::*;
    /// #[derive(Component)]
    /// struct Health(i32);
    /// #[derive(Component)]
    /// struct Bullet {
    ///     target: Entity,
    ///     damage: i32,
    /// }
    ///
    /// let mut world = World::new();
    /// let crab = world.spawn((Health(10),));
    /// world.spawn((Bullet { target: crab, damage: 3 },));
    ///
    /// world.query::<&Bullet>().with_lookup::<&mut Health>().for_each(|bullet, others| {
    ///     if let Some(health) = others.get_mut(bullet.target) {
    ///         health.0 -= bullet.damage;
    ///     }
    /// });
    /// assert_eq!(world.get::<Health>(crab).unwrap().0, 7);
    /// ```
    ///
    /// The lookup may read components the query reads. It can't touch a
    /// component the query writes, or write one the query reads: that would
    /// be two references to one value. For that case, read with the query
    /// and apply the changes after the loop, or use
    /// [`World::get_many_mut`].
    ///
    /// The query's filters don't apply to the lookup: it reaches any entity
    /// that has `L`'s components. Call it last, after the filters.
    ///
    /// Writes through [`Lookup::get_mut`] count as changes, as with
    /// [`World::get_mut`]. So the lookup can't write a component the query
    /// filters on with [`Added`](crate::Added) or [`Changed`](crate::Changed):
    /// that filter reads the very stamps the lookup would change while the
    /// query decides which rows to visit.
    ///
    /// # Panics
    /// If the query and the lookup access the same component and at least
    /// one of them mutably, the lookup asks for a component twice, once
    /// mutably, or the lookup writes a component the query's `Added` or
    /// `Changed` filter reads.
    #[track_caller]
    pub fn with_lookup<L: QueryData>(self) -> QueryLookup<'w, Q, L> {
        let ids = L::ids(self.world);
        check_aliasing::<L>(self.world, &ids);
        check_disjoint::<Q, L>(self.world, &self.ids, &ids);
        let filters = self.filters();
        L::access(&ids, &mut |id, mutable, _| {
            if mutable && filters.reads_stamps_of(id) {
                panic!(
                    "lookup `{}` writes `{}`, which the query's `Added`/`Changed` filter reads; \
                     filter on another component, or change it after the loop",
                    std::any::type_name::<L>(),
                    self.world.component_info(id).name()
                );
            }
        });
        self.world
            .recorder
            .note(|l| L::access(&ids, &mut |id, write, _| l.component(id, write)));
        QueryLookup { query: self, ids }
    }
}
