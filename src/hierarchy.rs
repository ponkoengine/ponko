//! The entity hierarchy: parents, ordered children, and transforms that
//! follow their parent, in 2D and 3D.
//!
//! **Maturity: proven**, from the guide chapter `hierarchy.md`. Game: Coin Dash and Ember Hop,
//! packaged and played on Linux, macOS and Windows (Ember Hop's prefabs with children). Tests:
//! `tests/hierarchy.rs`. Platforms: Linux, macOS and Windows, headless (the workspace tests in the
//! list of checks). Graphics: not involved.
//!
//! The link from a child to its parent is its attachment:
//! [`Attached`] in 2D, `Attached3D` in 3D (with the
//! `3d` feature). The attachment's offset is the child's transform in its
//! parent's space, and the child's own `Transform` holds where that puts it
//! in the world. A parent keeps its children, in order, in a [`Children`]
//! component that the engine maintains.
//!
//! [`WorldHierarchy`] (in the prelude) reads and edits the tree:
//!
//! | Method | Does |
//! |---|---|
//! | [`set_parent`](WorldHierarchy::set_parent) | links a child to a parent without moving it; last among its siblings |
//! | [`remove_parent`](WorldHierarchy::remove_parent) | makes it a root where it is |
//! | [`set_child_index`](WorldHierarchy::set_child_index) | moves it among its siblings |
//! | [`parent`](WorldHierarchy::parent), [`children`](WorldHierarchy::children) | the links, children in order |
//! | [`descendants`](WorldHierarchy::descendants), [`ancestors`](WorldHierarchy::ancestors) | the subtree depth first, the chain up |
//! | [`despawn_recursive`](WorldHierarchy::despawn_recursive) | an entity and everything under it |
//! | [`update_hierarchy`](WorldHierarchy::update_hierarchy) | brings children lists and transforms up to date |
//!
//! Systems that hold the world through a query, view systems and the
//! threads of a parallel query edit it through [`Commands`] instead:
//! [`CommandsHierarchy`] (in the prelude too) records
//! [`despawn_recursive`](CommandsHierarchy::despawn_recursive) there, which
//! despawns the subtree as it is when the commands are applied.
//!
//! [`systems::follow_attachments`](crate::systems::follow_attachments) runs
//! `update_hierarchy` once per tick. It looks only at what changed since
//! its last run: a parent whose transform moved, an attachment added or
//! edited. A subtree whose root stayed put and whose links didn't change
//! gets no traversal and no transform writes: what remains is the scan
//! for changed attachments, which skips 64 entities at a time where
//! nothing changed, and a look at each root. Removed links leave their old parent's list directly, without a walk
//! over the tree. Attachments inserted directly (`world.insert`, a scene,
//! a prefab, C) join their parent's children list at the next update,
//! after the ones already there, in entity order.
//!
//! An entity whose parent is despawned is despawned too, with everything
//! under it, at the next update; so is a chain of links that loops.
//!
//! ```
//! use ponko::prelude::*;
//!
//! let mut world = World::new();
//! let body = world.spawn((d2::Transform::at(vec2(10.0, 0.0)),));
//! let arm = world.spawn((d2::Transform::at(vec2(12.0, 0.0)),));
//! let hand = world.spawn((d2::Transform::at(vec2(14.0, 0.0)),));
//! world.set_parent(arm, body).unwrap();
//! world.set_parent(hand, arm).unwrap();
//!
//! world.get_mut::<d2::Transform>(body).unwrap().position.y = 5.0;
//! world.update_hierarchy(); // what `systems::follow_attachments` does each tick
//! assert_eq!(world.get::<d2::Transform>(hand).unwrap().position, vec2(14.0, 5.0));
//! assert_eq!(world.descendants(body).collect::<Vec<_>>(), [arm, hand]);
//! assert_eq!(world.ancestors(hand).collect::<Vec<_>>(), [arm, body]);
//! assert_eq!(world.despawn_recursive(body), 3);
//! ```

#![deny(missing_docs)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

use glam::Vec2;
use ponko_ecs::{Changed, Commands, Component, ComponentId, Entity, HookWorld, Tick, Without, World};
use smallvec::SmallVec;

use crate::attach::Attached;
#[cfg(feature = "3d")]
use crate::attach::Attached3D;
use crate::d2::Transform as T2;
#[cfg(feature = "3d")]
use crate::d3::Transform as T3;
use crate::error::{Error, ErrorKind};

/// A name for an entity, for people: the editor's hierarchy shows it and
/// renames it, and scenes and prefabs save it (it is reflected as
/// `EntityName`, a string). The engine never looks entities up by it, and
/// names needn't be unique.
///
/// ```
/// use ponko::hierarchy::EntityName;
/// use ponko::prelude::*;
///
/// let mut world = World::new();
/// let door = world.spawn((d2::Transform::default(), EntityName::new("Front door")));
/// assert_eq!(world.get::<EntityName>(door).unwrap().as_str(), "Front door");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Component, serde::Serialize, serde::Deserialize)]
pub struct EntityName(pub String);

impl EntityName {
    /// The name `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The prefab file an entity was placed from: the editor puts it on the
/// root of every prefab it places (with
/// [`PrefabParts`](crate::scene::PrefabParts) listing the rest), and scenes
/// save it (reflected as `PrefabInstance`), so the editor can show where the
/// instance differs from its prefab, put those differences back, or write
/// them into the prefab for every instance. The game never needs it;
/// [`Ctx::spawn_prefab`](crate::ctx::Ctx::spawn_prefab) doesn't add it.
///
/// ```
/// use ponko::hierarchy::PrefabInstance;
/// use ponko::prelude::*;
///
/// let mut world = World::new();
/// let cart = world.spawn((d2::Transform::default(), PrefabInstance::new("prefabs/cart.json")));
/// assert_eq!(world.get::<PrefabInstance>(cart).unwrap().path, "prefabs/cart.json");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Component, serde::Serialize, serde::Deserialize)]
pub struct PrefabInstance {
    /// The prefab file, as `spawn_prefab` names it (relative to `assets/`).
    pub path: String,
}

impl PrefabInstance {
    /// An instance of prefab file `path`.
    #[must_use]
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// A parent's children, in order: kept by the engine (see the
/// [module](self)). Read it through [`WorldHierarchy::children`]; change it
/// through [`set_parent`](WorldHierarchy::set_parent),
/// [`remove_parent`](WorldHierarchy::remove_parent) and
/// [`set_child_index`](WorldHierarchy::set_child_index).
///
/// The first four entries are kept in the component itself: linking a
/// child to a parent with few children allocates nothing.
///
/// Reflected as a list of entities, so saves, scenes and prefabs keep the
/// order. A list may briefly name an entity that no longer hangs off this
/// one (it was despawned, or linked elsewhere with `world.insert`):
/// `children` skips those, and the next update drops them.
#[derive(Debug, Clone, Default, Component)]
pub struct Children {
    list: List,
    /// The root's transforms when its subtree was last placed (roots only).
    seen: Seen,
}

impl Children {
    /// How many entities the list names, stale ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// Whether the list is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
}

impl PartialEq for Children {
    fn eq(&self, other: &Self) -> bool {
        self.list == other.list
    }
}

impl serde::Serialize for Children {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.list.as_slice().serialize(s)
    }
}

impl<'de> serde::Deserialize<'de> for Children {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self {
            list: List::from_vec(Vec::deserialize(d)?),
            seen: Seen::default(),
        })
    }
}

/// A children list: the first few in the component itself, so linking a
/// child to a parent that had none allocates nothing.
type List = SmallVec<[Entity; 4]>;

/// A root's transforms as last placed: a change in either means its
/// subtree has to move.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Seen {
    d2: Option<T2>,
    #[cfg(feature = "3d")]
    d3: Option<T3>,
}

/// An entity's link to its parent.
#[derive(Clone, Copy)]
enum Link {
    D2(Attached),
    #[cfg(feature = "3d")]
    D3(Attached3D),
}

impl Link {
    fn to(self) -> Entity {
        match self {
            Link::D2(a) => a.to,
            #[cfg(feature = "3d")]
            Link::D3(a) => a.to,
        }
    }
}

/// `e`'s link: its 2D attachment, else its 3D one.
fn link(world: &World, e: Entity) -> Option<Link> {
    if let Some(&a) = world.get::<Attached>(e) {
        return Some(Link::D2(a));
    }
    #[cfg(feature = "3d")]
    if let Some(&a) = world.get::<Attached3D>(e) {
        return Some(Link::D3(a));
    }
    None
}

fn link_to(world: &World, e: Entity) -> Option<Entity> {
    link(world, e).map(Link::to)
}

/// `child` is listed under `parent` and still hangs off it.
fn is_child(world: &World, parent: Entity, child: Entity) -> bool {
    child != parent && link_to(world, child) == Some(parent) && world.is_alive(child)
}

/// The children of an entity, in order: from [`WorldHierarchy::children`].
pub struct ChildIter<'w> {
    world: &'w World,
    parent: Entity,
    list: std::slice::Iter<'w, Entity>,
}

impl Iterator for ChildIter<'_> {
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        self.list
            .by_ref()
            .copied()
            .find(|&c| is_child(self.world, self.parent, c))
    }
}

/// Everything under an entity, depth first, each parent before its
/// children and children in order: from [`WorldHierarchy::descendants`].
pub struct Descendants<'w> {
    world: &'w World,
    /// Entities still to visit, the next one last.
    stack: Vec<Entity>,
    /// Stops a loop of links (possible only between updates) from going on forever.
    left: usize,
}

impl Iterator for Descendants<'_> {
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        if self.left == 0 {
            return None;
        }
        let e = self.stack.pop()?;
        self.left -= 1;
        let at = self.stack.len();
        self.stack.extend(self.world.children(e));
        self.stack[at..].reverse();
        Some(e)
    }
}

/// The chain above an entity, its parent first: from
/// [`WorldHierarchy::ancestors`].
pub struct Ancestors<'w> {
    world: &'w World,
    at: Entity,
    left: usize,
}

impl Iterator for Ancestors<'_> {
    type Item = Entity;

    fn next(&mut self) -> Option<Entity> {
        if self.left == 0 {
            return None;
        }
        self.left -= 1;
        let up = link_to(self.world, self.at).filter(|&p| self.world.is_alive(p))?;
        self.at = up;
        Some(up)
    }
}

/// The hierarchy's methods on [`World`]; see the [module](self).
///
/// Comes in with `use ponko::prelude::*;`.
pub trait WorldHierarchy {
    /// The entity `e` hangs off, if it has a link and that entity is alive.
    fn parent(&self, e: Entity) -> Option<Entity>;

    /// `e`'s children, in order. Entities linked to it with `world.insert`
    /// since the last [`update_hierarchy`](Self::update_hierarchy) appear
    /// after it.
    fn children(&self, e: Entity) -> ChildIter<'_>;

    /// Everything under `e` (not `e` itself), depth first: each entity
    /// before its children, siblings in order.
    fn descendants(&self, e: Entity) -> Descendants<'_>;

    /// The entities above `e`, nearest first, up to its root.
    fn ancestors(&self, e: Entity) -> Ancestors<'_>;

    /// Makes `child` hang off `parent`, last among its children, without
    /// moving it: its new attachment's offset is where it is now in
    /// `parent`'s space. It leaves its old parent's children. 3D entities
    /// get an `Attached3D` (with the `3d` feature), 2D ones an
    /// [`Attached`]; to give an offset of your own, insert one of those
    /// instead.
    ///
    /// # Errors
    ///
    /// [`NotFound`](ErrorKind::NotFound) if either entity is dead;
    /// [`Invalid`](ErrorKind::Invalid) if `child` is `parent` or above it
    /// (a loop), or the two don't both have a 2D or both a 3D transform.
    fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<(), Error>;

    /// Makes `child` a root where it is now: removes its attachment and
    /// takes it out of its parent's children. `false` if it had no parent.
    fn remove_parent(&mut self, child: Entity) -> bool;

    /// Moves `child` to position `index` among its parent's children (the
    /// last if `index` is past the end). `false` if it has no parent.
    fn set_child_index(&mut self, child: Entity, index: usize) -> bool;

    /// Despawns `e` and everything under it; returns how many entities
    /// that was (0 if `e` was dead).
    fn despawn_recursive(&mut self, e: Entity) -> usize;

    /// Brings the hierarchy up to date: adds newly attached entities to
    /// their parents' children, despawns entities whose parent is gone (and
    /// what hangs off them), and places every subtree whose root moved or
    /// whose links changed since the last update. The rest is left alone.
    /// [`systems::follow_attachments`](crate::systems::follow_attachments)
    /// calls it once per tick.
    ///
    /// Turns on change tracking ([`World::track_changes`]) for the
    /// attachment components and [`Children`] the first time.
    fn update_hierarchy(&mut self);
}

impl WorldHierarchy for World {
    fn parent(&self, e: Entity) -> Option<Entity> {
        link_to(self, e).filter(|&p| p != e && self.is_alive(p))
    }

    fn children(&self, e: Entity) -> ChildIter<'_> {
        let list = self.get::<Children>(e).map_or(&[][..], |c| &c.list[..]);
        ChildIter {
            world: self,
            parent: e,
            list: list.iter(),
        }
    }

    fn descendants(&self, e: Entity) -> Descendants<'_> {
        let mut stack: Vec<Entity> = self.children(e).collect();
        stack.reverse();
        Descendants {
            world: self,
            stack,
            left: self.len(),
        }
    }

    fn ancestors(&self, e: Entity) -> Ancestors<'_> {
        Ancestors {
            world: self,
            at: e,
            left: self.len(),
        }
    }

    fn set_parent(&mut self, child: Entity, parent: Entity) -> Result<(), Error> {
        for e in [child, parent] {
            if !self.is_alive(e) {
                return Err(Error::new(ErrorKind::NotFound, format!("no entity {}", e.to_bits())));
            }
        }
        if child == parent || self.ancestors(parent).any(|a| a == child) {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "entity {} is above entity {} already: that link would make a loop",
                    child.to_bits(),
                    parent.to_bits()
                ),
            ));
        }
        let Some(new) = link_where_it_is(self, child, parent) else {
            return Err(Error::new(
                ErrorKind::Invalid,
                format!(
                    "entities {} and {} need the same kind of transform (both 2D or both 3D) to be linked",
                    child.to_bits(),
                    parent.to_bits()
                ),
            ));
        };
        unlist(self, child);
        self.remove::<Attached>(child);
        #[cfg(feature = "3d")]
        self.remove::<Attached3D>(child);
        match new {
            Link::D2(a) => self.insert(child, a),
            #[cfg(feature = "3d")]
            Link::D3(a) => self.insert(child, a),
        };
        list(self, parent, child);
        Ok(())
    }

    fn remove_parent(&mut self, child: Entity) -> bool {
        if link(self, child).is_none() {
            return false;
        }
        unlist(self, child);
        self.remove::<Attached>(child);
        #[cfg(feature = "3d")]
        self.remove::<Attached3D>(child);
        true
    }

    fn set_child_index(&mut self, child: Entity, index: usize) -> bool {
        let Some(parent) = self.parent(child) else {
            return false;
        };
        list(self, parent, child);
        if let Some(c) = self.get_mut::<Children>(parent)
            && let Some(at) = c.list.iter().position(|&e| e == child)
        {
            c.list.remove(at);
            let index = index.min(c.list.len());
            c.list.insert(index, child);
        }
        true
    }

    fn despawn_recursive(&mut self, e: Entity) -> usize {
        if !self.is_alive(e) {
            return 0;
        }
        let doomed: Vec<Entity> = std::iter::once(e).chain(self.descendants(e)).collect();
        unlist(self, e);
        doomed.into_iter().filter(|&d| self.despawn(d)).count()
    }

    fn update_hierarchy(&mut self) {
        update(self, Mode::Tick);
    }
}

/// The hierarchy's edits on [`Commands`], for systems that can't change the
/// world directly: a system iterating a query, a view system
/// (`sys.commands`), the threads of a parallel query (one buffer behind a
/// `Mutex`, as for spawns). See the [module](self).
///
/// Comes in with `use ponko::prelude::*;`.
pub trait CommandsHierarchy {
    /// Despawns `e` and everything under it when the commands are applied:
    /// the subtree as it is then, not as it was when this was recorded.
    /// Children linked before (by earlier commands too) go with it;
    /// entities taken out of it before (another parent, `remove_parent`)
    /// stay. Does nothing if `e` is dead by then.
    ///
    /// Like [`WorldHierarchy::despawn_recursive`], it follows the children
    /// lists: an entity attached with `insert` since the last
    /// [`update_hierarchy`](WorldHierarchy::update_hierarchy) goes at the
    /// next update, as anything whose parent is gone.
    ///
    /// ```
    /// use ponko::prelude::*;
    ///
    /// #[derive(Component)]
    /// struct Hp(i32);
    ///
    /// let mut world = World::new();
    /// let body = world.spawn((d2::Transform::default(), Hp(0)));
    /// let arm = world.spawn((d2::Transform::default(),));
    /// world.set_parent(arm, body).unwrap();
    ///
    /// let mut commands = world.commands();
    /// world.query::<(Entity, &Hp)>().for_each(|(e, hp)| {
    ///     if hp.0 <= 0 {
    ///         commands.despawn_recursive(e);
    ///     }
    /// });
    /// world.apply(&mut commands);
    /// assert!(!world.is_alive(body) && !world.is_alive(arm));
    /// ```
    fn despawn_recursive(&mut self, e: Entity);
}

impl CommandsHierarchy for Commands {
    fn despawn_recursive(&mut self, e: Entity) {
        self.add(move |world| {
            WorldHierarchy::despawn_recursive(world, e);
        });
    }
}

/// `child`'s link to `parent` that keeps it where it is now.
fn link_where_it_is(world: &World, child: Entity, parent: Entity) -> Option<Link> {
    #[cfg(feature = "3d")]
    if let (Some(c), Some(p)) = (world.get::<T3>(child), world.get::<T3>(parent)) {
        let offset = crate::attach::relative3d(p, c);
        return Some(Link::D3(Attached3D::to(parent).offset(offset)));
    }
    let (c, p) = (world.get::<T2>(child)?, world.get::<T2>(parent)?);
    let divide = |a: f32, b: f32| if b == 0.0 { a } else { a / b };
    let local = Vec2::from_angle(-p.rotation).rotate(c.position - p.position);
    let offset = Vec2::new(divide(local.x, p.scale.x), divide(local.y, p.scale.y));
    Some(Link::D2(
        Attached::to(parent).offset(offset).rotated(c.rotation - p.rotation),
    ))
}

/// Adds `child` at the end of `parent`'s children, unless it is there.
fn list(world: &mut World, parent: Entity, child: Entity) {
    match world.get_mut::<Children>(parent) {
        Some(c) => {
            if !c.list.contains(&child) {
                c.list.push(child);
            }
        }
        None => {
            world.insert(
                parent,
                Children {
                    list: smallvec::smallvec![child],
                    seen: Seen::default(),
                },
            );
        }
    }
}

/// Takes `child` out of its current parent's children.
fn unlist(world: &mut World, child: Entity) {
    if let Some(parent) = link_to(world, child)
        && let Some(c) = world.get_mut::<Children>(parent)
    {
        c.list.retain(|e| *e != child);
    }
}

/// What [`update`] may do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// The tick's update: despawns broken links and moves on.
    Tick,
    /// The editor's: places everything but despawns nothing and leaves the
    /// changes for the next tick's update to see too.
    Settle,
}

/// How an entity stands in this update's walk up the tree.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// Nothing at or above it moves.
    Still,
    /// It or something above it is placed this update.
    Moves,
    /// Being walked: meeting it again means the links loop.
    Visiting,
    /// In or under a loop, or under a dead parent.
    Broken,
}

/// What the hierarchy keeps between updates; the vectors are scratch, kept
/// so that a steady state allocates nothing.
#[derive(Default)]
struct State {
    tracking: bool,
    /// The change tick of the last update.
    seen: Tick,
    /// Entity index of a linked child to `(child, parent)`, as of the last
    /// update: where to take a removed or re-pointed link out of, without
    /// looking over the whole tree.
    up: Vec<Option<(Entity, Entity)>>,
    /// `(old parent, child)` entries to take out of children lists.
    unlink: Vec<(Entity, Entity)>,
    /// `(parent, child)` links to list again: their parent lost its list.
    relist: Vec<(Entity, Entity)>,
    /// Scratch set for big children lists.
    set: HashSet<Entity>,
    changed: Vec<(Entity, Entity)>,
    changed_children: Vec<Entity>,
    marks: HashMap<Entity, Mark>,
    path: Vec<Entity>,
    /// Changed links that start a placement: placed from their parents first.
    starts: Vec<Entity>,
    /// Placed entities whose children are placed next. The first level
    /// has one node per root with children, empty for those that stayed
    /// put.
    frontier: Vec<Node>,
    /// How many of `frontier`'s nodes are in `table`.
    indexed: usize,
    /// How many children the lists of `frontier`'s nodes name.
    kids: usize,
    next: Vec<Node>,
    /// The children of the frontier: (index of the parent in `frontier`, child).
    pairs: Vec<(u32, Entity)>,
    placed: Vec<Option<Placed>>,
    /// What [`place_linear`] did with each attached entity, in query order.
    found2: Vec<Found<T2>>,
    #[cfg(feature = "3d")]
    found3: Vec<Found<T3>>,
    /// Entities [`place_linear`] places after its pass.
    later: Vec<Entity>,
    /// Entity index of a parent to its place in `frontier` (see
    /// [`slot`]). An entry left from earlier levels or updates is harmless:
    /// it counts only if that place holds that entity.
    table: Vec<AtomicU32>,
    broken: Vec<Entity>,
    stale: Vec<Entity>,
}

/// The update behind [`WorldHierarchy::update_hierarchy`].
pub(crate) fn update(world: &mut World, mode: Mode) {
    if world.get_resource::<State>().is_none() {
        world.insert_resource(State::default());
    }
    world.resource_scope::<State, _>(|world, s| update_with(world, s, mode));
}

fn update_with(world: &mut World, s: &mut State, mode: Mode) {
    if !s.tracking {
        world.track_changes::<Attached>();
        #[cfg(feature = "3d")]
        world.track_changes::<Attached3D>();
        world.track_changes::<Children>();
        world.insert_resource(LostLinks::default());
        world.on_remove::<Children>(children_leaving);
        s.tracking = true;
    }
    let since = s.seen;
    s.broken.clear();
    s.stale.clear();
    s.unlink.clear();

    // Links removed and parents despawned: only the entities involved.
    unlink_removed(world, s, since);
    find_lost_parents(world, s);

    // New and edited links join their parents' children.
    s.changed.clear();
    let changed = &mut s.changed;
    world
        .query::<(Entity, &Attached)>()
        .filter::<Changed<Attached>>()
        .since(since)
        .for_each(|(e, a)| changed.push((a.to, e)));
    #[cfg(feature = "3d")]
    world
        .query::<(Entity, &Attached3D)>()
        .filter::<(Changed<Attached3D>, Without<Attached>)>()
        .since(since)
        .for_each(|(e, a)| changed.push((a.to, e)));
    s.changed.extend_from_slice(&s.relist);
    s.changed.sort_unstable();
    s.changed.dedup();
    repoint(s);
    apply_unlinks(world, s);
    join_parents(world, s);

    // Roots that moved since they were last placed, compared on all cores
    // when there are enough of them to be worth it (`par_collect_into`
    // times its first rows and stays on this thread under about 50 µs):
    // each root with children is a node of the first level, empty if it
    // stayed put.
    s.starts.clear();
    s.marks.clear();
    s.pairs.clear();
    {
        #[cfg(feature = "3d")]
        let mut q = world
            .query::<(Entity, &mut Children, Option<&T2>, Option<&T3>)>()
            .filter::<(Without<Attached>, Without<Attached3D>)>();
        #[cfg(not(feature = "3d"))]
        let mut q = world
            .query::<(Entity, &mut Children, Option<&T2>)>()
            .filter::<Without<Attached>>();
        q.par_collect_into(&mut s.frontier, |item| {
            #[cfg(feature = "3d")]
            let (e, c, t2, t3) = item;
            #[cfg(not(feature = "3d"))]
            let (e, c, t2) = item;
            let now = Seen {
                d2: t2.copied(),
                #[cfg(feature = "3d")]
                d3: t3.copied(),
            };
            if now == c.seen {
                return Node::EMPTY;
            }
            c.seen = now;
            Node {
                e: Some(e),
                kids: c.list.len() as u32,
                at: At {
                    d2: now.d2,
                    #[cfg(feature = "3d")]
                    d3: now.d3,
                },
            }
        });
    }
    s.indexed = 0;
    let slots = world.entity_slots();
    s.kids = index_frontier(s, slots);

    // Edited links start a placement too, unless something above them is placed anyway.
    find_starts(world, s);

    // Place the subtrees, from each start down, a level at a time.
    place_all(world, s);
    s.stale.sort_unstable();
    s.stale.dedup();

    for &parent in &s.stale {
        prune(world, parent);
    }
    if mode == Mode::Tick {
        s.broken.sort_unstable();
        s.broken.dedup();
        for &broken in &s.broken {
            world.despawn_recursive(broken);
        }
        s.seen = world.advance_change_tick();
    }
}

/// The slot of `child` in `s.up`, grown to fit.
fn up_slot(up: &mut Vec<Option<(Entity, Entity)>>, child: Entity) -> &mut Option<(Entity, Entity)> {
    let i = child.index() as usize;
    if up.len() <= i {
        up.resize(i + 1, None);
    }
    &mut up[i]
}

/// Queues removed links (a despawned child, a removed attachment) to be
/// taken out of their old parent's children.
fn unlink_removed(world: &World, s: &mut State, since: Tick) {
    let removed = world.removed_since::<Attached>(since).iter();
    #[cfg(feature = "3d")]
    let removed = removed.chain(world.removed_since::<Attached3D>(since));
    for &r in removed {
        let Some(slot) = s.up.get_mut(r.index() as usize) else {
            continue;
        };
        let Some((child, old)) = *slot else { continue };
        if child != r || (world.is_alive(r) && link_to(world, r) == Some(old)) {
            // Another entity's slot now, or linked to the same parent again.
            continue;
        }
        *slot = None;
        s.unlink.push((old, r));
    }
}

/// The children lists of parents despawned (or that lost their
/// `Children`) since the last update, as `(parent, child)` pairs: filled by
/// [`children_leaving`], an `on_remove` hook of [`Children`], and emptied
/// by [`find_lost_parents`]. A resource of its own (not in [`State`]), so
/// the hook finds it while an update holds the state.
#[derive(Default)]
struct LostLinks(Vec<(Entity, Entity)>);

/// `on_remove` hook of [`Children`]: notes the list that goes, so the next
/// update finds the children of a despawned parent without looking at
/// every link.
fn children_leaving(world: &mut HookWorld, parent: Entity) {
    if world.get_resource::<LostLinks>().is_none() {
        return;
    }
    world.resource_scope::<LostLinks, _>(|world, lost| {
        if let Some(c) = world.get::<Children>(parent) {
            lost.0.extend(c.list.iter().map(|&child| (parent, child)));
        }
    });
}

/// Finds the children of parents despawned (or that lost their
/// `Children`) since the last update: they are broken if the parent is
/// dead, and listed again if it is alive. Looks only at the lists those
/// parents had ([`LostLinks`]), not at every link. Not inlined: it runs
/// once an update, and inlined it changed how the placement loops of
/// `update_with` compiled (a deep chain placed measurably slower).
#[inline(never)]
fn find_lost_parents(world: &mut World, s: &mut State) {
    s.relist.clear();
    let Some(lost) = world.get_resource_mut::<LostLinks>() else {
        return;
    };
    if lost.0.is_empty() {
        return;
    }
    let mut links = std::mem::take(&mut lost.0);
    links.sort_unstable();
    links.dedup();
    for &(parent, child) in &links {
        if child == parent || world.get::<Children>(parent).is_some() {
            // Not a link, or the parent has a list again (nothing was lost).
            continue;
        }
        if !world.is_alive(child) || link_to(world, child) != Some(parent) {
            if let Some(slot) = s.up.get_mut(child.index() as usize)
                && *slot == Some((child, parent))
            {
                *slot = None;
            }
        } else if world.is_alive(parent) {
            s.relist.push((parent, child));
        } else {
            s.broken.push(child);
        }
    }
    links.clear();
    // Back with its capacity, after anything hooks noted meanwhile (none:
    // nothing above despawns), so a steady state allocates nothing.
    if let Some(lost) = world.get_resource_mut::<LostLinks>() {
        links.append(&mut lost.0);
        lost.0 = links;
    }
}

/// Records the links in `s.changed` in `s.up`, queueing a re-pointed
/// child to leave its old parent's children.
fn repoint(s: &mut State) {
    for &(parent, child) in &s.changed {
        if child == parent {
            continue;
        }
        let slot = up_slot(&mut s.up, child);
        if let Some((c, old)) = *slot
            && c == child
            && old != parent
        {
            s.unlink.push((old, child));
        }
        *slot = Some((child, parent));
    }
}

/// Takes the `(parent, child)` entries in `s.unlink` out of the parents'
/// children lists.
fn apply_unlinks(world: &mut World, s: &mut State) {
    if s.unlink.is_empty() {
        return;
    }
    s.unlink.sort_unstable();
    s.unlink.dedup();
    let mut i = 0;
    while i < s.unlink.len() {
        let parent = s.unlink[i].0;
        let mut end = i;
        while end < s.unlink.len() && s.unlink[end].0 == parent {
            end += 1;
        }
        let group = &s.unlink[i..end];
        i = end;
        let Some(c) = world.get_mut::<Children>(parent) else {
            continue;
        };
        if group.len() <= 8 {
            for &(_, child) in group {
                if let Some(at) = c.list.iter().position(|&e| e == child) {
                    c.list.remove(at);
                }
            }
        } else {
            s.set.clear();
            s.set.extend(group.iter().map(|&(_, child)| child));
            let set = &s.set;
            c.list.retain(|e| !set.contains(e));
        }
    }
}

/// Drops the entries of `parent`'s children that no longer hang off it.
fn prune(world: &mut World, parent: Entity) {
    let Some(c) = world.get::<Children>(parent) else { return };
    if c.list.iter().all(|&k| is_child(world, parent, k)) {
        return;
    }
    let keep: List = c.list.iter().copied().filter(|&k| is_child(world, parent, k)).collect();
    if let Some(c) = world.get_mut::<Children>(parent) {
        c.list = keep;
    }
}

/// Adds the children in `s.changed` (sorted by parent, then child) to their
/// parents' lists, after the entries already there.
fn join_parents(world: &mut World, s: &mut State) {
    let mut i = 0;
    let have = &mut s.set;
    while i < s.changed.len() {
        let parent = s.changed[i].0;
        let mut end = i;
        while end < s.changed.len() && s.changed[end].0 == parent {
            end += 1;
        }
        let group = &s.changed[i..end];
        i = end;
        if !world.is_alive(parent) {
            s.broken.extend(group.iter().map(|&(_, c)| c));
            continue;
        }
        if world.get::<Children>(parent).is_none() {
            world.insert(parent, Children::default());
        }
        let Some(c) = world.get_mut::<Children>(parent) else {
            continue;
        };
        // A big group checks membership in a set; a small one just looks.
        let big = group.len() > 8 && c.list.len() > 8;
        if big {
            have.clear();
            have.extend(c.list.iter().copied());
        }
        for &(_, child) in group {
            if child == parent {
                continue;
            }
            let listed = if big {
                !have.insert(child)
            } else {
                c.list.contains(&child)
            };
            if !listed {
                c.list.push(child);
            }
        }
    }
}

/// Decides which changed links start a placement: those with nothing above
/// them that moves this update. Marks links that loop or hang off a dead
/// entity as broken.
fn find_starts(world: &World, s: &mut State) {
    if s.changed.is_empty() {
        return;
    }
    // A sorted copy to look things up in: the changed children.
    s.changed_children.clear();
    s.changed_children.extend(s.changed.iter().map(|&(_, c)| c));
    s.changed_children.sort_unstable();
    for i in 0..s.changed.len() {
        let child = s.changed[i].1;
        if s.marks.contains_key(&child) {
            continue;
        }
        let up = link_to(world, child);
        // A child of a root that moves (every root, on the first update) is
        // placed with it: no more lookups.
        if let Some(p) = up
            && p != child
            && slot(&s.table, &s.frontier, p).is_some()
        {
            continue;
        }
        // The common case, a child of a still root: no walk to remember.
        if let Some(p) = up
            && p != child
            && world.is_alive(p)
            && link(world, p).is_none()
        {
            s.starts.push(child);
            continue;
        }
        // Walk up until something already known (or a root).
        s.path.clear();
        let mut at = child;
        let above = loop {
            match s.marks.get(&at) {
                Some(Mark::Still) => break Mark::Still,
                Some(Mark::Moves) => break Mark::Moves,
                Some(Mark::Visiting | Mark::Broken) => break Mark::Broken,
                None => {}
            }
            match link_to(world, at) {
                None => {
                    let mark = if slot(&s.table, &s.frontier, at).is_some() {
                        Mark::Moves
                    } else {
                        Mark::Still
                    };
                    s.marks.insert(at, mark);
                    break mark;
                }
                Some(p) if p == at || !world.is_alive(p) => {
                    s.marks.insert(at, Mark::Visiting);
                    s.path.push(at);
                    break Mark::Broken;
                }
                Some(p) => {
                    s.marks.insert(at, Mark::Visiting);
                    s.path.push(at);
                    at = p;
                }
            }
        };
        // Back down, nearest the root first.
        let mut above = above;
        for &n in s.path.iter().rev() {
            let mark = match above {
                Mark::Broken | Mark::Visiting => {
                    s.broken.push(n);
                    Mark::Broken
                }
                Mark::Moves => Mark::Moves,
                Mark::Still if s.changed_children.binary_search(&n).is_ok() => {
                    s.starts.push(n);
                    Mark::Moves
                }
                Mark::Still => Mark::Still,
            };
            s.marks.insert(n, mark);
            above = mark;
        }
    }
}

/// Levels with at least this many children may be placed in one pass over
/// every attached entity ([`place_linear`]) instead of a lookup each.
const LINEAR: usize = 2048;

/// The children of a level placed by lookups ([`place_level`]) that run on
/// the calling thread first, timed: they decide whether the rest is shared
/// out between cores.
const PROBE: usize = 256;

/// A pass projected to take less than this on the calling thread stays
/// there: waking other threads costs more than it saves, and on a busy
/// machine a woken thread may not run for milliseconds. The same budget as
/// the ECS's parallel loops, which the other passes use.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub(crate) const SERIAL_BUDGET_NS: u128 = 50_000;

/// In a web build with threads, handing a level to the frame pool's
/// workers and waiting for them costs about 0.1 ms, so a level must project
/// to 0.4 ms on the page's thread (about 14,000 attached children placed by
/// lookups). In headless Chromium with 13 threads, 4,096 children took 0.12
/// ms on the page's thread and 0.14 ms shared out, 16,384 took 0.47 and
/// 0.26 ms (`web/smoke/threads.mjs --case thresholds`; the guide's "Threads
/// on the web").
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub(crate) const SERIAL_BUDGET_NS: u128 = 400_000;

/// The components placement reads and writes, looked up once per update.
#[derive(Clone, Copy)]
struct Ids {
    children: Option<ComponentId>,
    attached: Option<ComponentId>,
    #[cfg(feature = "3d")]
    attached3d: Option<ComponentId>,
    t2: Option<ComponentId>,
    /// Someone tracks changes to it: writes go through `get_mut`, which stamps them.
    t2_tracked: bool,
    #[cfg(feature = "3d")]
    t3: Option<ComponentId>,
    #[cfg(feature = "3d")]
    t3_tracked: bool,
}

impl Ids {
    fn of(world: &World) -> Self {
        let tracked = |id: Option<ComponentId>| id.is_some_and(|id| world.component_info(id).tracks_changes());
        let t2 = world.component_id::<T2>();
        #[cfg(feature = "3d")]
        let t3 = world.component_id::<T3>();
        Ids {
            children: world.component_id::<Children>(),
            attached: world.component_id::<Attached>(),
            #[cfg(feature = "3d")]
            attached3d: world.component_id::<Attached3D>(),
            t2,
            t2_tracked: tracked(t2),
            #[cfg(feature = "3d")]
            t3,
            #[cfg(feature = "3d")]
            t3_tracked: tracked(t3),
        }
    }
}

/// Component `T` (whose id is `id`) of `e`, by id: no type lookup.
fn read<T: Component>(world: &World, id: Option<ComponentId>, e: Entity) -> Option<&T> {
    let p = world.raw_ref().get_dynamic(e, id?)?;
    // SAFETY: `id` is `T`'s id in this world (from `component_id::<T>`), so
    // the pointer is to a live, aligned `T`, valid while `world` is borrowed.
    Some(unsafe { &*p.cast::<T>() })
}

/// Writes `value` as `e`'s component `T` (whose id is `id`), if it has one.
fn write<T: Component + Copy>(world: &mut World, id: Option<ComponentId>, tracked: bool, e: Entity, value: T) {
    if tracked {
        if let Some(t) = world.get_mut::<T>(e) {
            *t = value;
        }
        return;
    }
    if let Some(p) = id.and_then(|id| world.raw().get_dynamic_mut(e, id)) {
        // SAFETY: `id` is `T`'s id in this world, so the pointer is to a
        // live, aligned `T`; nothing else uses the world meanwhile. `T` is
        // `Copy`, so overwriting drops nothing.
        unsafe { p.cast::<T>().write(value) };
    }
}

/// An entity's transforms where already known (`None`: read the world).
#[derive(Clone, Copy, Default)]
struct At {
    d2: Option<T2>,
    #[cfg(feature = "3d")]
    d3: Option<T3>,
}

/// A placed entity whose children are placed next.
#[derive(Clone, Copy)]
struct Node {
    /// `None`: an empty place (a root that stayed put).
    e: Option<Entity>,
    /// How many children its list names; [`SOME`] where not counted.
    kids: u32,
    at: At,
}

/// [`Node::kids`] of a node placed by a lookup: it has a children list,
/// not counted (the next level counts it as it lists the children).
const SOME: u32 = u32::MAX;

impl Node {
    const EMPTY: Node = Node {
        e: None,
        kids: 0,
        at: At {
            d2: None,
            #[cfg(feature = "3d")]
            d3: None,
        },
    };
}

/// Where `p` is in `frontier`, if it is there.
fn slot(table: &[AtomicU32], frontier: &[Node], p: Entity) -> Option<usize> {
    let j = table.get(p.index() as usize)?.load(Relaxed) as usize;
    (frontier.get(j)?.e == Some(p)).then_some(j)
}

/// Puts the nodes of `s.frontier` not yet in `s.table` there, growing it
/// to `slots`. Returns how many children their lists name.
///
/// On the calling thread: one store per node is too little work to share.
/// On all cores it took as long as here (0.15-0.23 ms for 100,000 nodes),
/// and on a busy machine a parallel pass waits for its slowest thread,
/// which the system may not run for milliseconds.
fn index_frontier(s: &mut State, slots: usize) -> usize {
    if s.table.len() < slots {
        s.table.resize_with(slots, || AtomicU32::new(u32::MAX));
    }
    let (table, from) = (&s.table, s.indexed);
    let put = |(j, n): (usize, &Node)| {
        if let Some(e) = n.e {
            table[e.index() as usize].store((from + j) as u32, Relaxed);
        }
        n.kids as usize
    };
    let kids = s.frontier[from..].iter().enumerate().map(put).sum();
    s.indexed = s.frontier.len();
    kids
}

/// Lists the children of `s.frontier` in `s.pairs`, looked up.
fn list_pairs(world: &World, s: &mut State, ids: &Ids) {
    s.pairs.clear();
    for (i, n) in s.frontier.iter().enumerate() {
        if let Some(e) = n.e
            && n.kids > 0
            && let Some(c) = read::<Children>(world, ids.children, e)
        {
            s.pairs.extend(c.list.iter().map(|&k| (i as u32, k)));
        }
    }
}

/// A child's new transform, and whether it has children of its own.
#[derive(Clone, Copy)]
struct Placed {
    child: Entity,
    at: Placement,
    parent: bool,
}

#[derive(Clone, Copy)]
enum Placement {
    D2(T2),
    #[cfg(feature = "3d")]
    D3(T3),
}

impl Placement {
    fn at(self) -> At {
        match self {
            Placement::D2(t) => At {
                d2: Some(t),
                #[cfg(feature = "3d")]
                d3: None,
            },
            #[cfg(feature = "3d")]
            Placement::D3(t) => At { d2: None, d3: Some(t) },
        }
    }
}

/// Where `child` goes, from `parent`'s transforms (`at`, or the world's),
/// if it hangs off `parent` and both have the transforms its link needs.
/// Reads only, so levels are worked out in parallel.
fn compute(world: &World, ids: &Ids, parent: Entity, at: &At, child: Entity) -> Option<Placed> {
    if child == parent {
        return None;
    }
    let placement = if let Some(a) = read::<Attached>(world, ids.attached, child) {
        if a.to != parent {
            return None;
        }
        let p = match at.d2 {
            Some(p) => p,
            None => *read::<T2>(world, ids.t2, parent)?,
        };
        let own = read::<T2>(world, ids.t2, child)?;
        Placement::D2(T2 {
            position: p.position + Vec2::from_angle(p.rotation).rotate(a.offset * p.scale),
            rotation: p.rotation + a.rotation,
            scale: own.scale,
        })
    } else {
        compute3d(world, ids, parent, at, child)?
    };
    Some(Placed {
        child,
        at: placement,
        parent: ids
            .children
            .is_some_and(|id| world.raw_ref().get_dynamic(child, id).is_some()),
    })
}

/// [`compute`]'s 3D half: where `child`'s `Attached3D` puts it.
#[cfg(feature = "3d")]
fn compute3d(world: &World, ids: &Ids, parent: Entity, at: &At, child: Entity) -> Option<Placement> {
    let a = read::<Attached3D>(world, ids.attached3d, child)?;
    if a.to != parent {
        return None;
    }
    let p = match at.d3 {
        Some(p) => p,
        None => *read::<T3>(world, ids.t3, parent)?,
    };
    read::<T3>(world, ids.t3, child)?;
    Some(Placement::D3(crate::attach::compose3d(&p, &a.offset)))
}

/// Without the `3d` feature there are no 3D links.
#[cfg(not(feature = "3d"))]
fn compute3d(_: &World, _: &Ids, _: Entity, _: &At, _: Entity) -> Option<Placement> {
    None
}

fn apply(world: &mut World, ids: &Ids, placed: &Placed) {
    match placed.at {
        Placement::D2(t) => write(world, ids.t2, ids.t2_tracked, placed.child, t),
        #[cfg(feature = "3d")]
        Placement::D3(t) => write(world, ids.t3, ids.t3_tracked, placed.child, t),
    }
}

/// How many entities have an attachment.
fn count_attached(world: &mut World) -> usize {
    let n = world.query::<&Attached>().count();
    #[cfg(feature = "3d")]
    let n = n + world.query::<&Attached3D>().count();
    n
}

/// What [`place_linear`] did with one attached entity.
#[derive(Clone, Copy)]
enum Found<T> {
    /// Its parent isn't placed in this level.
    No,
    /// Placed; it has no children.
    Leaf,
    /// Placed at this transform; its list names this many children.
    Parent(Entity, u32, T),
    /// Its parent is placed in the other dimension (a 2D child of a 3D
    /// parent, say): placed after the pass, with a lookup.
    Later(Entity),
}

/// Counts what `found` placed and the children its parents' lists name,
/// and adds those parents to `next` and the entities to place later to
/// `later`, in one pass on the calling thread: like [`index_frontier`],
/// too little work per entity to share between cores.
fn gather<T: Copy>(
    found: &[Found<T>],
    next: &mut Vec<Node>,
    later: &mut Vec<Entity>,
    at: impl Fn(T) -> At,
) -> (usize, usize) {
    let (mut placed, mut kids) = (0, 0);
    for f in found {
        match *f {
            Found::No => {}
            Found::Leaf => placed += 1,
            Found::Parent(e, n, t) => {
                placed += 1;
                kids += n as usize;
                next.push(Node {
                    e: Some(e),
                    kids: n,
                    at: at(t),
                });
            }
            Found::Later(e) => later.push(e),
        }
    }
    (placed, kids)
}

/// Places a big level in one pass over the attached entities, in storage
/// order, on all cores when the pass is long enough to be worth it
/// (`par_collect_into` measures its first rows): each finds its parent's node in `s.frontier` through
/// `s.table` and writes its own transform, with no lookups of its own.
/// `kids` is how many children the level's lists name. Fills `s.next`;
/// returns how many children its nodes' lists name, if it counted them.
/// Only when nothing tracks transform changes (this marks no stamps), and
/// the level is a good part of everything attached.
fn place_linear(world: &mut World, s: &mut State, ids: &Ids, kids: usize) -> Option<usize> {
    index_frontier(s, world.entity_slots());
    let (table, frontier) = (&s.table, &s.frontier);
    let parent = |e: Entity, p: Entity| Some(&frontier[slot(table, frontier, p).filter(|_| p != e)?].at);
    s.later.clear();
    world
        .query::<(Entity, &Attached, &mut T2, Option<&Children>)>()
        .par_collect_into(&mut s.found2, |(e, a, t, list)| {
            let Some(at) = parent(e, a.to) else { return Found::No };
            let Some(p) = at.d2 else { return Found::Later(e) };
            *t = T2 {
                position: p.position + Vec2::from_angle(p.rotation).rotate(a.offset * p.scale),
                rotation: p.rotation + a.rotation,
                scale: t.scale,
            };
            match list {
                Some(c) => Found::Parent(e, c.list.len() as u32, *t),
                None => Found::Leaf,
            }
        });
    let (mut placed, kids2) = gather(&s.found2, &mut s.next, &mut s.later, |t| Placement::D2(t).at());
    let mut next = Some(kids2);
    #[cfg(feature = "3d")]
    {
        world
            .query::<(Entity, &Attached3D, &mut T3, Option<&Children>)>()
            .filter::<Without<Attached>>()
            .par_collect_into(&mut s.found3, |(e, a, t, list)| {
                let Some(at) = parent(e, a.to) else { return Found::No };
                let Some(p) = at.d3 else { return Found::Later(e) };
                *t = crate::attach::compose3d(&p, &a.offset);
                match list {
                    Some(c) => Found::Parent(e, c.list.len() as u32, *t),
                    None => Found::Leaf,
                }
            });
        let (more, kids) = gather(&s.found3, &mut s.next, &mut s.later, |t| Placement::D3(t).at());
        placed += more;
        next = next.map(|n| n + kids);
    }
    for i in 0..s.later.len() {
        let kid = s.later[i];
        let Some(j) = link_to(world, kid).and_then(|p| slot(&s.table, &s.frontier, p)) else {
            continue;
        };
        let Some(p) = s.frontier[j].e else { continue };
        if let Some(p) = compute(world, ids, p, &s.frontier[j].at, kid) {
            apply(world, ids, &p);
            placed += 1;
            if p.parent {
                next = None;
                s.next.push(Node {
                    e: Some(kid),
                    kids: SOME,
                    at: p.at.at(),
                });
            }
        }
    }
    // Every listed child that still hangs off its parent was placed, unless
    // a list names one that doesn't (or one without a transform): find those.
    if placed != kids {
        list_pairs(world, s, ids);
        for &(i, kid) in &s.pairs {
            if let Some(parent) = s.frontier[i as usize].e
                && !is_child(world, parent, kid)
            {
                s.stale.push(parent);
            }
        }
    }
    next
}

/// Places every subtree: the moved roots are already in `s.frontier`;
/// the changed children in `s.starts` are placed from their parents
/// first. Then level by level down; a level whose work, measured, would
/// take long enough is worked out on all cores.
fn place_all(world: &mut World, s: &mut State) {
    let ids = Ids::of(world);
    let mut counted = true;
    for i in 0..s.starts.len() {
        let start = s.starts[i];
        let Some(parent) = link_to(world, start) else { continue };
        if let Some(p) = compute(world, &ids, parent, &At::default(), start) {
            apply(world, &ids, &p);
            if p.parent {
                counted = false;
                s.frontier.push(Node {
                    e: Some(start),
                    kids: SOME,
                    at: p.at.at(),
                });
            }
        }
    }
    let mut attached = None;
    // How many children the level's lists name; `None` when it has nodes
    // placed by lookups, whose lists are counted as they are listed.
    let mut level = counted.then_some(s.kids);
    loop {
        let kids = match level {
            Some(n) => n,
            None => {
                list_pairs(world, s, &ids);
                s.pairs.len()
            }
        };
        if kids == 0 {
            break;
        }
        s.next.clear();
        let linear = kids >= LINEAR && !ids.t2_tracked && {
            #[cfg(feature = "3d")]
            let untracked = !ids.t3_tracked;
            #[cfg(not(feature = "3d"))]
            let untracked = true;
            untracked && kids * 4 >= *attached.get_or_insert_with(|| count_attached(world))
        };
        let listed = level.is_none();
        level = if linear {
            place_linear(world, s, &ids, kids)
        } else {
            if !listed {
                list_pairs(world, s, &ids);
            }
            place_level(world, s, &ids)
        };
        std::mem::swap(&mut s.frontier, &mut s.next);
        s.indexed = 0;
    }
    s.frontier.clear();
}

/// Places the children in `s.pairs`, each looked up on its own, and fills
/// `s.next` with those that have children; returns `Some(0)` if none has.
/// A level is worked out on all cores when its first [`PROBE`] children,
/// timed, project it over [`SERIAL_BUDGET_NS`]: by the work measured, not
/// the number of children.
fn place_level(world: &mut World, s: &mut State, ids: &Ids) -> Option<usize> {
    {
        let (w, frontier) = (&*world, &s.frontier);
        let one = |&(i, kid): &(u32, Entity)| {
            let n = &frontier[i as usize];
            compute(w, ids, n.e?, &n.at, kid)
        };
        s.placed.clear();
        let (first, rest) = s.pairs.split_at(PROBE.min(s.pairs.len()));
        if rest.is_empty() {
            s.placed.extend(first.iter().map(one));
        } else {
            let start = web_time::Instant::now();
            s.placed.extend(first.iter().map(one));
            if crate::frame_pool::share(|| worth_sharing(start.elapsed().as_nanos(), first.len(), s.pairs.len())) {
                use rayon::prelude::*;
                let placed = &mut s.placed;
                crate::frame_pool::run(|| {
                    // Once per shared level, on the thread that shares it
                    // out (on the web, a worker).
                    crate::profile_scope!("hierarchy/shared_level");
                    placed.par_extend(rest.par_iter().map(one));
                });
            } else {
                s.placed.extend(rest.iter().map(one));
            }
        }
    }
    for (&(i, kid), placed) in s.pairs.iter().zip(&s.placed) {
        match placed {
            Some(p) => {
                apply(world, ids, p);
                if p.parent {
                    s.next.push(Node {
                        e: Some(kid),
                        kids: SOME,
                        at: p.at.at(),
                    });
                }
            }
            None => {
                if let Some(parent) = s.frontier[i as usize].e
                    && !is_child(world, parent, kid)
                {
                    s.stale.push(parent);
                }
            }
        }
    }
    s.next.is_empty().then_some(0)
}

/// Whether a pass of `total` items, whose first `done` took `spent_ns` on
/// the calling thread, is worth sharing out between cores: whether the
/// whole pass projects to at least [`SERIAL_BUDGET_NS`].
fn worth_sharing(spent_ns: u128, done: usize, total: usize) -> bool {
    spent_ns * total as u128 / done.max(1) as u128 >= SERIAL_BUDGET_NS
}

/// [`update`] for the editor: places everything now, despawns nothing.
pub(crate) fn settle(world: &mut World) {
    update(world, Mode::Settle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_level_is_shared_out_only_when_its_timed_children_project_over_the_budget() {
        // 256 children at 30 ns each (lookups): 2,000 project to 60 µs.
        assert!(worth_sharing(256 * 30, 256, 2_000));
        // 1,000 of them project to 30 µs: the calling thread places them.
        assert!(!worth_sharing(256 * 30, 256, 1_000));
        // At 0.4 ns each, 100,000 project to 39 µs (here) and 200,000 to
        // 78 µs (shared): the work decides, not the count.
        assert!(!worth_sharing(100, 256, 100_000));
        assert!(worth_sharing(100, 256, 200_000));
    }
}
