//! Component hooks: functions a world calls when a component arrives on an
//! entity, is replaced, or leaves it. The user-facing overview is in the
//! crate docs ("Component hooks", `lib.rs`); the decision and its
//! measurements in `ARCHITECTURE.md` and `crates/ponko_bench/HOOKS.md`.
//!
//! Where they hook in: a world keeps its hooks in a box, made with the
//! world, and one flag, `World::hooked`, next to its other fields. Spawns
//! check nothing: a bundle whose archetype has `on_add` hooks has its
//! cached archetype index tagged, which sends it down a cold path.
//! Despawns, inserts and removes check `hooked` and only then look
//! further, at one byte per archetype or one entry per component. `on_add` runs after the entity is complete; `on_remove` and
//! `on_replace` before the value goes, while it is still in place. A
//! [`HookWorld`] can't make structural changes, so nothing a hook does
//! moves the entity the world is working on; its commands are applied by
//! `World::flush_hooks` when the outermost call is done.

use std::ptr::NonNull;

use crate::bundle::Component;
use crate::commands::Commands;
use crate::entity::Entity;
use crate::query::{Query, QueryData};
use crate::storage::{ComponentId, TypeMap};
use crate::world::World;

/// How to make a required component's value: `write(data, dst, size)`
/// writes one at `dst`. For Rust, `data` is the constructor (a `fn() -> R`);
/// for a byte template, the template's bytes, `size` of them.
#[derive(Clone, Copy)]
pub(crate) struct Make {
    write: unsafe fn(*const (), *mut u8, usize),
    data: *const (),
}

// SAFETY: `data` is a function pointer or points to bytes the world owns
// and never changes while they are in use; both can be shared.
unsafe impl Send for Make {}
unsafe impl Sync for Make {}

impl Make {
    /// Makes values with `make`.
    pub(crate) fn rust<R: Component>(make: fn() -> R) -> Self {
        Make {
            write: write_made::<R>,
            data: make as *const (),
        }
    }

    /// Copies the bytes at `bytes`, which the caller keeps alive and
    /// unchanged as long as this is used (the registry owns them).
    pub(crate) fn bytes(bytes: *const u8) -> Self {
        Make {
            write: write_copy,
            data: bytes.cast(),
        }
    }

    /// Writes a new value to `dst`, `size` bytes (the component's size).
    ///
    /// # Safety
    /// `dst` is valid for writes of the component this makes, `size` is
    /// its size, and the template or constructor is still registered.
    #[inline]
    pub(crate) unsafe fn write(self, dst: *mut u8, size: usize) {
        // SAFETY: the caller's contract.
        unsafe { (self.write)(self.data, dst, size) }
    }
}

/// [`Make::rust`]'s writer: `data` is a `fn() -> R`.
///
/// # Safety
/// As [`Make::write`]; `data` was made from a `fn() -> R`.
unsafe fn write_made<R: Component>(data: *const (), dst: *mut u8, _: usize) {
    // SAFETY: `Make::rust::<R>` stored a `fn() -> R` here; a function
    // pointer survives the round trip.
    let make = unsafe { std::mem::transmute::<*const (), fn() -> R>(data) };
    // SAFETY: `dst` is valid for a write of an `R` (the caller's contract).
    unsafe { dst.cast::<R>().write(make()) };
}

/// [`Make::bytes`]'s writer.
///
/// # Safety
/// As [`Make::write`]; `data` points to `size` readable bytes.
unsafe fn write_copy(data: *const (), dst: *mut u8, size: usize) {
    // SAFETY: the caller's contract; the template isn't in the world.
    unsafe { std::ptr::copy_nonoverlapping(data.cast::<u8>(), dst, size) };
}

/// The bytes a requirement given as bytes copies ([`Make::bytes`]).
///
/// Owned through a raw pointer rather than a `Box`: moving a `Box`
/// asserts unique access to its bytes, which would invalidate the pointer
/// the `Make` keeps (Miri's Stacked Borrows reports it). Moving this
/// asserts nothing; the bytes are freed when it drops.
pub(crate) struct Template(NonNull<[u8]>);

impl Template {
    /// Takes ownership of `bytes`.
    pub(crate) fn new(bytes: Box<[u8]>) -> Self {
        Template(NonNull::from(Box::leak(bytes)))
    }

    /// The first byte, for [`Make::bytes`]; valid while this lives.
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr().cast::<u8>().cast_const()
    }
}

impl Drop for Template {
    fn drop(&mut self) {
        // SAFETY: made by `Box::leak` in `new`, freed only here.
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

// SAFETY: plain bytes, owned, never written after `new`.
unsafe impl Send for Template {}
// SAFETY: as above; shared access only reads.
unsafe impl Sync for Template {}

/// A component that another one requires.
struct Required {
    id: ComponentId,
    make: Make,
    /// The template `make` copies, for requirements given as bytes.
    _bytes: Option<Template>,
}

/// An `on_add` or `on_remove` hook: called with the world and the entity.
/// See [`World::on_add`] and [`World::on_remove`].
pub type Hook = for<'w> fn(&mut HookWorld<'w>, Entity);

/// An `on_replace` hook: called with the world (where the entity still has
/// its old value), the entity, and the new value. See [`World::on_replace`].
pub type ReplaceHook<T> = for<'w, 'v> fn(&mut HookWorld<'w>, Entity, &'v T);

/// Archetype flag: a component here has an `on_add` hook.
const ADD: u8 = 1;
/// Archetype flag: a component here has an `on_remove` hook.
const REMOVE: u8 = 2;
/// Archetype flag: a component here requires others
/// ([`Component::requires`](crate::Component::requires)).
const REQUIRES: u8 = 4;

/// An `on_replace` hook without its type: the hook as a pointer, and the
/// function that turns it back into a `ReplaceHook<T>` and calls it.
#[derive(Clone, Copy)]
struct Replace {
    call: unsafe fn(*const (), &mut HookWorld<'_>, Entity, *const u8),
    hook: *const (),
}

// SAFETY: `hook` is a function pointer (no data behind it), which is Send
// and Sync.
unsafe impl Send for Replace {}
unsafe impl Sync for Replace {}

/// Calls `hook`, a `ReplaceHook<T>`, with the value at `new`.
///
/// # Safety
/// `hook` was made from a `ReplaceHook<T>` (by `World::on_replace::<T>`)
/// and `new` points to a valid `T` that outlives the call.
unsafe fn call_replace<T: Component>(hook: *const (), world: &mut HookWorld<'_>, e: Entity, new: *const u8) {
    // SAFETY: the caller's contract: `hook` is a `ReplaceHook<T>` cast to a
    // pointer, and a function pointer survives the round trip.
    let hook = unsafe { std::mem::transmute::<*const (), ReplaceHook<T>>(hook) };
    // SAFETY: the caller's contract; the value isn't in the world, so the
    // hook's access to the world can't reach it.
    hook(world, e, unsafe { &*new.cast::<T>() });
}

/// The hooks of one component type, and the components it requires.
#[derive(Default)]
struct ComponentHooks {
    add: Vec<Hook>,
    remove: Vec<Hook>,
    replace: Vec<Replace>,
    required: Vec<Required>,
}

/// A world's hooks, and the commands they recorded. Boxed in the world, so
/// a world without hooks carries a pointer, not this.
#[derive(Default)]
pub(crate) struct Hooks {
    /// By component index; components past the end have none.
    of: Vec<ComponentHooks>,
    /// By archetype slot: [`ADD`] and [`REMOVE`] for its components. Kept
    /// only once the world is `hooked` (rebuilt at every registration).
    archetypes: Vec<u8>,
    /// What hooks record; applied by [`World::flush_hooks`].
    queue: Commands,
    /// The other buffer, swapped in while `queue` is applied.
    spare: Commands,
    /// Calls in progress that apply the queue when they end (see
    /// [`World::hold_hooks`]); `flush_hooks` does nothing while it is not 0.
    hold: u32,
    /// By bundle type: the columns of its archetype that the bundle doesn't
    /// fill, and how to make their required values. Only bundles that lack
    /// a component they require are here (and tagged `HOOKED_BUNDLE`).
    pub(crate) fills: TypeMap<Box<[(usize, Make)]>>,
}

impl Hooks {
    /// Whether archetype `a` has a component with an `on_add` hook.
    #[inline]
    pub(crate) fn adds(&self, a: usize) -> bool {
        self.archetypes.get(a).is_some_and(|f| f & ADD != 0)
    }

    /// Whether archetype `a` has a component with an `on_remove` hook.
    #[inline]
    pub(crate) fn removes(&self, a: usize) -> bool {
        self.archetypes.get(a).is_some_and(|f| f & REMOVE != 0)
    }

    /// Whether component `id` has an `on_add` hook.
    #[inline]
    pub(crate) fn adds_component(&self, id: ComponentId) -> bool {
        self.of.get(id.index()).is_some_and(|h| !h.add.is_empty())
    }

    /// Whether component `id` has an `on_remove` hook.
    #[inline]
    pub(crate) fn removes_component(&self, id: ComponentId) -> bool {
        self.of.get(id.index()).is_some_and(|h| !h.remove.is_empty())
    }

    /// Whether component `id` has an `on_replace` hook.
    #[inline]
    pub(crate) fn replaces_component(&self, id: ComponentId) -> bool {
        self.of.get(id.index()).is_some_and(|h| !h.replace.is_empty())
    }

    /// Whether component `id` requires others.
    #[inline]
    pub(crate) fn requires_component(&self, id: ComponentId) -> bool {
        self.of.get(id.index()).is_some_and(|h| !h.required.is_empty())
    }

    /// Whether archetype `a` has a component that requires others.
    #[inline]
    pub(crate) fn requires(&self, a: usize) -> bool {
        self.archetypes.get(a).is_some_and(|f| f & REQUIRES != 0)
    }

    /// The flags of an archetype of components `ids`.
    fn flags(&self, ids: &[ComponentId]) -> u8 {
        ids.iter().fold(0, |f, &id| {
            let Some(h) = self.of.get(id.index()) else { return f };
            f | if h.add.is_empty() { 0 } else { ADD }
                | if h.remove.is_empty() { 0 } else { REMOVE }
                | if h.required.is_empty() { 0 } else { REQUIRES }
        })
    }

    /// Adds to `out` the components that components `order` require
    /// (directly, or through what they require) and that are neither in
    /// `present` nor in `out` already, each with how to make its value.
    /// Breadth first, in `order`'s order: the nearest requirement's value
    /// wins, and of two at the same depth the one of the earlier component.
    pub(crate) fn missing(&self, order: &[ComponentId], present: &[ComponentId], out: &mut Vec<(ComponentId, Make)>) {
        let start = out.len();
        let mut i = 0;
        loop {
            let next = if i < order.len() {
                order[i]
            } else if let Some(&(id, _)) = out.get(start + i - order.len()) {
                id
            } else {
                break;
            };
            i += 1;
            let Some(h) = self.of.get(next.index()) else { continue };
            for r in &h.required {
                if !present.contains(&r.id) && !out[start..].iter().any(|m| m.0 == r.id) {
                    out.push((r.id, r.make));
                }
            }
        }
    }

    /// Records that component `of` requires `required`, made by `make`
    /// (and owning `bytes`, which `make` copies, if any); replaces an
    /// earlier requirement of the same pair.
    pub(crate) fn add_requirement(
        &mut self,
        of: ComponentId,
        required: ComponentId,
        make: Make,
        bytes: Option<Template>,
    ) {
        let list = &mut self.of_mut(of).required;
        let r = Required {
            id: required,
            make,
            _bytes: bytes,
        };
        match list.iter_mut().find(|r| r.id == required) {
            Some(old) => *old = r,
            None => list.push(r),
        }
    }

    /// Whether component `of` requires `required`.
    pub(crate) fn has_requirement(&self, of: ComponentId, required: ComponentId) -> bool {
        self.of
            .get(of.index())
            .is_some_and(|h| h.required.iter().any(|r| r.id == required))
    }

    /// Notes the components of archetype slot `a`, just created or reused,
    /// in a world that is `hooked`.
    pub(crate) fn archetype_created(&mut self, a: usize, ids: &[ComponentId]) {
        if self.archetypes.len() <= a {
            self.archetypes.resize(a + 1, 0);
        }
        self.archetypes[a] = self.flags(ids);
    }

    /// The hooks of component `id`, growing the table to it.
    fn of_mut(&mut self, id: ComponentId) -> &mut ComponentHooks {
        if self.of.len() <= id.index() {
            self.of.resize_with(id.index() + 1, ComponentHooks::default);
        }
        &mut self.of[id.index()]
    }
}

/// The world as a hook sees it: components, resources and queries, and
/// [`commands`](Self::commands) for structural changes, which wait until
/// the operation that ran the hook is done. See [`World::on_add`].
///
/// ```
/// use ponko_ecs::{Component, Entity, HookWorld, World};
///
/// #[derive(Component)]
/// struct Hp(i32);
/// #[derive(Component)]
/// struct Shield;
///
/// // Every entity that gets Hp gets a Shield too.
/// fn shield(world: &mut HookWorld, e: Entity) {
///     world.commands().insert(e, Shield);
/// }
///
/// let mut world = World::new();
/// world.on_add::<Hp>(shield);
/// let e = world.spawn((Hp(10),));
/// assert!(world.has::<Shield>(e)); // applied before `spawn` returned
/// ```
pub struct HookWorld<'w> {
    world: &'w mut World,
}

impl HookWorld<'_> {
    /// The whole world, to read: what [`World`]'s `&self` methods offer
    /// (`components_of`, `removed`, `change_tick`, ...).
    #[must_use]
    pub fn world(&self) -> &World {
        self.world
    }

    /// Component `T` of `e`: [`World::get`].
    #[must_use]
    pub fn get<T: Component>(&self, e: Entity) -> Option<&T> {
        self.world.get(e)
    }

    /// Component `T` of `e` to change: [`World::get_mut`]. A change for
    /// change detection, as there.
    pub fn get_mut<T: Component>(&mut self, e: Entity) -> Option<&mut T> {
        self.world.get_mut(e)
    }

    /// Whether `e` has `T`: [`World::has`].
    #[must_use]
    pub fn has<T: Component>(&self, e: Entity) -> bool {
        self.world.has::<T>(e)
    }

    /// Whether `e` is alive: [`World::is_alive`]. The entity a hook is
    /// called for always is.
    #[must_use]
    pub fn is_alive(&self, e: Entity) -> bool {
        self.world.is_alive(e)
    }

    /// A query: [`World::query`].
    pub fn query<Q: QueryData>(&mut self) -> Query<'_, Q> {
        self.world.query()
    }

    /// Resource `R`: [`World::resource`].
    ///
    /// # Panics
    /// If there is none.
    #[must_use]
    #[track_caller]
    pub fn resource<R: Send + Sync + 'static>(&self) -> &R {
        self.world.resource()
    }

    /// Resource `R` to change: [`World::resource_mut`].
    ///
    /// # Panics
    /// If there is none.
    #[track_caller]
    pub fn resource_mut<R: Send + Sync + 'static>(&mut self) -> &mut R {
        self.world.resource_mut()
    }

    /// Resource `R`, if there is one: [`World::get_resource`].
    #[must_use]
    pub fn get_resource<R: Send + Sync + 'static>(&self) -> Option<&R> {
        self.world.get_resource()
    }

    /// Resource `R` to change, if there is one: [`World::get_resource_mut`].
    pub fn get_resource_mut<R: Send + Sync + 'static>(&mut self) -> Option<&mut R> {
        self.world.get_resource_mut()
    }

    /// Adds or replaces resource `R`: [`World::insert_resource`].
    pub fn insert_resource<R: Send + Sync + 'static>(&mut self, value: R) {
        self.world.insert_resource(value);
    }

    /// Resource `R` and the rest of the world at once: [`World::resource_scope`].
    /// For a hook that reads a component into a resource, such as an index:
    ///
    /// ```
    /// use std::collections::HashMap;
    /// use ponko_ecs::{Component, Entity, HookWorld, World};
    ///
    /// #[derive(Component)]
    /// struct Name(String);
    /// #[derive(Default)]
    /// struct Names(HashMap<String, Entity>);
    ///
    /// fn forget(world: &mut HookWorld, e: Entity) {
    ///     world.resource_scope::<Names, _>(|world, names| {
    ///         if let Some(name) = world.get::<Name>(e) {
    ///             names.0.remove(&name.0);
    ///         }
    ///     });
    /// }
    ///
    /// let mut world = World::new();
    /// world.insert_resource(Names::default());
    /// world.on_remove::<Name>(forget);
    /// let e = world.spawn((Name("crab".into()),));
    /// world.resource_mut::<Names>().0.insert("crab".into(), e);
    /// world.despawn(e);
    /// assert!(world.resource::<Names>().0.is_empty());
    /// ```
    ///
    /// # Panics
    /// If there is no `R`.
    #[track_caller]
    pub fn resource_scope<R: Send + Sync + 'static, T>(
        &mut self,
        f: impl FnOnce(&mut HookWorld<'_>, &mut R) -> T,
    ) -> T {
        self.world
            .resource_scope::<R, T>(|world, r| f(&mut HookWorld { world }, r))
    }

    /// Commands for structural changes (spawn, despawn, insert, remove),
    /// applied when the operation that ran this hook is done, before the
    /// call that made it returns. Spawns get their entity id at once.
    pub fn commands(&mut self) -> &mut Commands {
        let world = &mut *self.world;
        if !world.hooks.queue.is_bound_to(world) {
            let mut queue = std::mem::take(&mut world.hooks.queue);
            queue.bind(world);
            world.hooks.queue = queue;
        }
        &mut world.hooks.queue
    }
}

/// Ends a [`World::hold_hooks`] (also when a hook or command panics), then
/// applies the commands hooks recorded if nothing else holds them.
pub(crate) struct Held<'a> {
    pub(crate) world: &'a mut World,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        self.world.hooks.hold -= 1;
        if !std::thread::panicking() {
            self.world.flush_hooks();
        }
    }
}

/// Ends a flush's hold and resumes access recording, even if a command
/// panics, keeping its buffer.
struct Flushing<'a> {
    world: &'a mut World,
    buffer: Commands,
    was: bool,
}

impl Drop for Flushing<'_> {
    fn drop(&mut self) {
        self.world.hooks.hold -= 1;
        self.world.recorder.resume(self.was);
        // Empty now (applying empties it, also on a panic): the next spare.
        self.world.hooks.spare = std::mem::take(&mut self.buffer);
    }
}

/// Resumes access recording after hooks ran, also when one panics.
struct Unrecorded<'a> {
    world: &'a mut World,
    was: bool,
}

impl Drop for Unrecorded<'_> {
    fn drop(&mut self) {
        self.world.recorder.resume(self.was);
    }
}

impl World {
    /// Calls `hook` whenever component `T` arrives on an entity that didn't
    /// have one: every kind of spawn, and [`insert`](Self::insert) on an
    /// entity without `T` (also through [`Commands`] and the raw and C
    /// paths). It runs once the entity is complete, so `world.get::<T>(e)`
    /// is the new value. An insert that replaces a `T` runs
    /// [`on_replace`](Self::on_replace) instead.
    ///
    /// The hook gets a [`HookWorld`]: it reads and writes components and
    /// resources at once and records structural changes as commands,
    /// applied before the call that ran it returns. See the
    /// [component hooks](crate#component-hooks) overview for the order hooks run in
    /// and what they cost.
    ///
    /// A component can have several hooks of each kind; they run in the
    /// order they were registered. Hooks stay registered for the life of
    /// the world, through [`clear`](Self::clear) and [`reset`](Self::reset).
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, HookWorld, World};
    ///
    /// #[derive(Component)]
    /// struct Enemy;
    /// #[derive(Default)]
    /// struct Alive(u32);
    ///
    /// fn count(world: &mut HookWorld, _: Entity) {
    ///     world.resource_mut::<Alive>().0 += 1;
    /// }
    /// fn uncount(world: &mut HookWorld, _: Entity) {
    ///     world.resource_mut::<Alive>().0 -= 1;
    /// }
    ///
    /// let mut world = World::new();
    /// world.insert_resource(Alive::default());
    /// world.on_add::<Enemy>(count);
    /// world.on_remove::<Enemy>(uncount);
    /// let a = world.spawn((Enemy,));
    /// world.spawn((Enemy,));
    /// world.despawn(a);
    /// assert_eq!(world.resource::<Alive>().0, 1);
    /// ```
    pub fn on_add<T: Component>(&mut self, hook: Hook) {
        let id = self.register::<T>();
        self.hooks.of_mut(id).add.push(hook);
        self.hooks_changed();
    }

    /// Calls `hook` whenever component `T` is about to leave an entity:
    /// [`remove`](Self::remove), [`despawn`](Self::despawn),
    /// [`clear`](Self::clear) and [`reset`](Self::reset) (also through
    /// [`Commands`] and the raw and C paths). It runs before the value is
    /// dropped or returned, while the entity still has everything, so
    /// `world.get::<T>(e)` is the value that goes. A despawn runs the
    /// hooks of all the entity's components, then removes it.
    ///
    /// See [`on_add`](Self::on_add) for what the hook can do.
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, HookWorld, World};
    ///
    /// #[derive(Component)]
    /// struct Crate(u32);
    /// #[derive(Component)]
    /// struct Loot(u32);
    ///
    /// // A crate leaves its loot behind, as a new entity.
    /// fn drop_loot(world: &mut HookWorld, e: Entity) {
    ///     let coins = world.get::<Crate>(e).unwrap().0;
    ///     world.commands().spawn((Loot(coins),));
    /// }
    ///
    /// let mut world = World::new();
    /// world.on_remove::<Crate>(drop_loot);
    /// let c = world.spawn((Crate(30),));
    /// world.despawn(c);
    /// assert_eq!(world.query::<&Loot>().iter().map(|l| l.0).sum::<u32>(), 30);
    /// ```
    pub fn on_remove<T: Component>(&mut self, hook: Hook) {
        let id = self.register::<T>();
        self.hooks.of_mut(id).remove.push(hook);
        self.hooks_changed();
    }

    /// Calls `hook` whenever [`insert`](Self::insert) is about to overwrite
    /// an entity's component `T` (also through [`Commands`] and the raw
    /// and C paths). It runs before the old value is dropped:
    /// `world.get::<T>(e)` is the old value, and the hook's third argument
    /// the new one. For an index by value, that is both sides of the move.
    /// Writes through `get_mut` and queries are not replacements and run
    /// no hook (change detection sees them).
    ///
    /// See [`on_add`](Self::on_add) for what the hook can do.
    ///
    /// ```
    /// use ponko_ecs::{Component, Entity, HookWorld, World};
    ///
    /// #[derive(Component)]
    /// struct Level(u32);
    /// #[derive(Default)]
    /// struct LevelUps(u32);
    ///
    /// fn level_up(world: &mut HookWorld, e: Entity, new: &Level) {
    ///     let old = world.get::<Level>(e).unwrap().0;
    ///     world.resource_mut::<LevelUps>().0 += new.0.saturating_sub(old);
    /// }
    ///
    /// let mut world = World::new();
    /// world.insert_resource(LevelUps::default());
    /// world.on_replace::<Level>(level_up);
    /// let hero = world.spawn((Level(1),));
    /// world.insert(hero, Level(3));
    /// assert_eq!(world.resource::<LevelUps>().0, 2);
    /// ```
    pub fn on_replace<T: Component>(&mut self, hook: ReplaceHook<T>) {
        let id = self.register::<T>();
        self.hooks.of_mut(id).replace.push(Replace {
            call: call_replace::<T>,
            hook: hook as *const (),
        });
        self.hooks_changed();
    }

    /// After a hook or a requirement was registered: the world now checks
    /// for hooks, with the flags of every archetype, places its bundles
    /// again, and has its command buffers.
    pub(crate) fn hooks_changed(&mut self) {
        self.hooked = true;
        let flags: Vec<u8> = self.archetypes.iter().map(|a| self.hooks.flags(&a.ids)).collect();
        self.hooks.archetypes = flags;
        self.rebuild_bundles();
        let mut queue = std::mem::take(&mut self.hooks.queue);
        queue.bind(self);
        self.hooks.queue = queue;
        let mut spare = std::mem::take(&mut self.hooks.spare);
        spare.bind(self);
        self.hooks.spare = spare;
    }

    /// Holds back the commands hooks record until the returned guard is
    /// dropped: for calls made of several operations (`spawn_batch`,
    /// `clear`, `apply`), so their hooks' commands run after all of it.
    pub(crate) fn hold_hooks(&mut self) -> Held<'_> {
        self.hooks.hold += 1;
        Held { world: self }
    }

    /// Applies the commands hooks recorded, and those the hooks they run
    /// record, in the order recorded, until there are none. Does nothing
    /// while a call holds them ([`hold_hooks`](Self::hold_hooks)) or one is
    /// applying them already.
    pub(crate) fn flush_hooks(&mut self) {
        while self.hooks.hold == 0 && !self.hooks.queue.is_empty() {
            let buffer = std::mem::take(&mut self.hooks.queue);
            self.hooks.queue = std::mem::take(&mut self.hooks.spare);
            self.hooks.hold += 1;
            let was = self.recorder.pause();
            let mut flushing = Flushing {
                world: self,
                buffer,
                was,
            };
            flushing.buffer.apply(flushing.world);
            drop(flushing);
        }
    }

    /// Runs the `on_add` hooks of every component of archetype `a` (which
    /// has some: [`Hooks::adds`]) for `e`, just placed there, then applies
    /// their commands.
    #[cold]
    #[inline(never)]
    pub(crate) fn run_spawn_hooks(&mut self, a: usize, e: Entity) {
        let mut i = 0;
        while let Some(&id) = self.archetypes[a].ids.get(i) {
            i += 1;
            self.run_hooks(id, e, Kind::Add);
        }
        self.flush_hooks();
    }

    /// Runs the `on_remove` hooks of every component of `e`, in archetype
    /// `a`, before it is despawned.
    #[cold]
    #[inline(never)]
    pub(crate) fn run_despawn_hooks(&mut self, a: usize, e: Entity) {
        let mut i = 0;
        while let Some(&id) = self.archetypes[a].ids.get(i) {
            i += 1;
            self.run_hooks(id, e, Kind::Remove);
        }
    }

    /// Runs the `on_add` or `on_remove` hooks of component `id` for `e`,
    /// with access recording paused (the hooks' work isn't the caller's).
    pub(crate) fn run_hooks(&mut self, id: ComponentId, e: Entity, kind: Kind) {
        let was = self.recorder.pause();
        let guard = Unrecorded { world: self, was };
        let mut j = 0;
        // Looked up again for each hook, which borrows the whole world.
        while let Some(&hook) = guard.world.hooks.of.get(id.index()).and_then(|h| match kind {
            Kind::Add => h.add.get(j),
            Kind::Remove => h.remove.get(j),
        }) {
            j += 1;
            hook(&mut HookWorld { world: guard.world }, e);
        }
    }

    /// Runs the `on_replace` hooks of component `id` for `e`, about to get
    /// the value at `new`.
    ///
    /// # Safety
    /// `id` is a Rust component `T` and `new` points to a valid `T` that is
    /// not in the world and outlives the call.
    pub(crate) unsafe fn run_replace_hooks(&mut self, id: ComponentId, e: Entity, new: *const u8) {
        let was = self.recorder.pause();
        let guard = Unrecorded { world: self, was };
        let mut j = 0;
        while let Some(&Replace { call, hook }) = guard.world.hooks.of.get(id.index()).and_then(|h| h.replace.get(j)) {
            j += 1;
            // SAFETY: `on_replace::<T>` paired `call` with a hook of the
            // component's own type, and the caller vouches for `new`.
            unsafe { call(hook, &mut HookWorld { world: guard.world }, e, new) };
        }
    }

    /// Drops the commands hooks recorded and not yet applied (releasing
    /// the ids of their spawns): for [`reset`](Self::reset), which starts
    /// the world over.
    pub(crate) fn drop_hook_commands(&mut self) {
        self.hooks.queue.clear();
    }
}

/// Which hooks [`World::run_hooks`] runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Add,
    Remove,
}
