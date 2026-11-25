//! Declared system access: what a system reads and writes, and which
//! declared systems could run together.
//!
//! **Maturity: tested**, from the guide chapter `ui-accessibility.md`. Game: Coin Dash names its
//! controls for screen readers. Tests: `tests/ui_access*.rs`: AccessKit data on every OS, the macOS
//! adapter, UI Automation on Windows, AT-SPI on Linux. Platforms: Linux, macOS and Windows,
//! headless (the workspace tests in the list of checks). Graphics: not involved. Missing: the
//! records with a real screen reader were made on macOS only; none yet for Windows or Linux.
//!
//! Systems run one after another, in phase and registration order, unless
//! [`Game::parallel_systems`](crate::game::Game::parallel_systems) is on: then
//! view systems whose declarations don't conflict run at the same time
//! (see [`schedule`](crate::schedule)). The guide's "Declared system
//! access" and "Parallel systems" chapters have the full design.
//!
//! # The model
//!
//! A [`SystemAccess`] lists:
//!
//! - **components** read or written (`&T` / `&mut T` in queries, `get`, `get_mut`),
//! - **resources** read or written,
//! - **events** read or sent ([`Events`](crate::events::Events)),
//! - whether the system records **commands** ([`Commands`](ponko_ecs::Commands)),
//! - the engine **services** it uses ([`Service`]: input, time, random,
//!   audio, cameras, physics, ...), read or written.
//!
//! Or it is [`exclusive`](SystemAccess::exclusive): it may use anything, and
//! conflicts with every other system. A system registered without a
//! declaration (every `.sim_system(...)`, `.system(...)`, built-in and
//! hot-patched system, and C/C++ systems added without one) is exclusive,
//! so existing games behave exactly as before.
//!
//! # When two declarations conflict
//!
//! Two systems conflict, and so must keep their order, when either is
//! exclusive, or both touch the same component, resource or service and at
//! least one writes it, or events or commands make their order visible.
//! [`SystemAccess::conflicts_with`] is the symmetric, conservative rule;
//! [`SystemAccess::conflicts_before`] the finer one for a known order, under
//! the batch barrier contract it documents. Only systems in the same
//! [`Phase`] are compared: phases are ordered barriers.
//!
//! # Soundness strategy
//!
//! A declaration is a promise; the engine must not trust it blindly.
//!
//! 1. **View systems enforce it.** A system that takes a
//!    [`SystemView`](crate::system_view::SystemView) instead of `&mut Ctx`
//!    (see [`Game::add`](crate::game::Game::add)) gets exactly that: a restricted
//!    [`WorldView`](crate::ecs_core::WorldView) of its declared components and
//!    resources, and checked access to its events, commands and services,
//!    in every build. Nothing else is reachable, so such a system can't go
//!    beyond its declaration. Only view systems may ever share a parallel
//!    batch ([`AccessReport::parallel_pairs`]).
//! 2. **`Ctx` systems are checked in debug builds.** A `&mut Ctx` system
//!    with a declaration ([`IntoSystem::access`](crate::game::IntoSystem::access),
//!    or a C or C++ system with one) keeps `&mut Ctx`. In builds with
//!    debug assertions it runs with access recording on
//!    ([`World::record_access`](ponko_ecs::World::record_access) and the same
//!    for events); afterwards any component, resource or event it used
//!    without declaring it, any structural change made directly on the world
//!    (spawn, despawn, insert, remove: those need exclusivity or commands),
//!    and any undeclared command panics, naming the system and what it
//!    used. Services aren't recorded. These systems always run sequentially.
//! 3. **No shared `Ctx`, ever.** The scheduler never hands `&mut Ctx` (or
//!    `&mut World`) to two systems at once; views share `&World` through a
//!    checked [`ViewGroup`](crate::ecs_core::ViewGroup).
//!
//! # Scope and limits
//!
//! - **Drawing, UI, assets and profiling** ([`Service::Gfx`], `Ui`,
//!   `Assets`, `Perf`) can be declared but aren't offered to view systems;
//!   systems that use them keep `&mut Ctx`.
//! - **Physics and navigation** steps declare what they touch
#![cfg_attr(
    feature = "physics",
    doc = "  ([`step_physics_access`](crate::systems::step_physics_access) and its"
)]
#![cfg_attr(not(feature = "physics"), doc = "  (`step_physics_access` and its")]
//!   siblings in [`systems`](crate::systems)) and keep `&mut Ctx`: checked
//!   in debug builds, sequential. Other built-in systems stay exclusive.
//! - **Foreign (C/C++) systems** declare components, resources and events
//!   by registered name and stay sequential.
//! - **Hot patching** swaps a system's code, not its declaration. A patched
//!   view system still can't reach anything undeclared; a patched `Ctx`
//!   system is caught by the debug check on its next run.
//! - Release builds skip the `Ctx` recorder entirely; views check each
//!   accessor call (never per entity) in every build.
//!
//! ```
//! use ponko::prelude::*;
//!
//! #[derive(Component)]
//! struct Pos(f32);
//! #[derive(Component)]
//! struct Vel(f32);
//! #[derive(Component)]
//! struct Health(i32);
//!
//! use ponko::access::SystemAccess;
//!
//! fn movement(ctx: &mut Ctx) {
//!     ctx.world.query::<(&mut Pos, &Vel)>().for_each(|(p, v)| p.0 += v.0);
//! }
//! fn decay(ctx: &mut Ctx) {
//!     ctx.world.query::<&mut Health>().for_each(|h| h.0 -= 1);
//! }
//!
//! let game = Game::new("access", 320, 240)
//!     .add(Phase::AfterPhysics, movement.access(SystemAccess::new().write::<Pos>().read::<Vel>()))
//!     .add(Phase::AfterPhysics, decay.access(SystemAccess::new().write::<Health>()));
//! let report = game.access_report();
//! assert_eq!(report.compatible_pairs().count(), 1);
//! ```

use std::any::TypeId;
use std::fmt;

use ponko_ecs::{AccessLog, Component, ViewState, World};

#[cfg(debug_assertions)]
use crate::ctx::Ctx;
use crate::game::Phase;

/// An engine service on [`Ctx`] that a system can declare,
/// beside its components, resources, events and commands.
///
/// Names follow the service fields of `Ctx`. Two declarations conflict on
/// a service when both use it and at least one writes it. View systems can
/// use the services marked "view" below, through
/// [`SystemView`](crate::system_view::SystemView); the others hold state
/// that belongs to the window, renderer or asset tables, so a system that
/// needs them keeps `&mut Ctx` (declared, or exclusive).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Service {
    /// Keys, mouse, gamepads and named actions (`ctx.input`). View: read.
    Input,
    /// Frame and simulation clocks: `dt`, ticks, pause and speed
    /// (`ctx.time`). View: read.
    Time,
    /// The system's random stream (`ctx.random()`). Only read: each
    /// system's stream depends on the seed, tick and system alone. View: read.
    Random,
    /// Drawing, textures, meshes, materials and render settings (`ctx.gfx`).
    Gfx,
    /// Sounds, buses and the listener (`ctx.audio`). View: write (play sounds).
    Audio,
    /// Hot reload, data files, prefabs and scenes (`ctx.assets`).
    Assets,
    /// The retained UI tree, style and input capture (`ctx.ui`).
    Ui,
    /// Profiling, statistics, the HUD and traces (`ctx.perf`).
    Perf,
    /// The 2D and 3D cameras (`ctx.camera`, `ctx.scene3d.camera`). View: read and write.
    Camera,
    /// The spatial grid (`ctx.spatial`). View: read.
    Spatial,
    /// The 2D physics world (`ctx.physics`). View: read and write.
    Physics,
    /// The 3D physics world (`ctx.physics3d`). View: read and write.
    Physics3D,
    /// Lockstep networking (`ctx.net.lockstep`). View: read and write.
    Net,
}

impl Service {
    /// Every service, in the order of their C numbers (`PONKO_SERVICE_*`).
    pub const ALL: [Service; 13] = [
        Service::Input,
        Service::Time,
        Service::Random,
        Service::Gfx,
        Service::Audio,
        Service::Assets,
        Service::Ui,
        Service::Perf,
        Service::Camera,
        Service::Spatial,
        Service::Physics,
        Service::Physics3D,
        Service::Net,
    ];

    /// The service's field name on `Ctx`, e.g. `"input"`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Service::Input => "input",
            Service::Time => "time",
            Service::Random => "random",
            Service::Gfx => "gfx",
            Service::Audio => "audio",
            Service::Assets => "assets",
            Service::Ui => "ui",
            Service::Perf => "perf",
            Service::Camera => "camera",
            Service::Spatial => "spatial",
            Service::Physics => "physics",
            Service::Physics3D => "physics3d",
            Service::Net => "net",
        }
    }

    /// What a [`SystemView`](crate::system_view::SystemView) offers: `None` if nothing,
    /// else whether it can also write.
    pub(crate) fn in_view(self) -> Option<bool> {
        match self {
            Service::Input | Service::Time | Service::Random | Service::Spatial => Some(false),
            Service::Audio | Service::Camera | Service::Physics | Service::Physics3D | Service::Net => Some(true),
            Service::Gfx | Service::Assets | Service::Ui | Service::Perf => None,
        }
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How a declared item is identified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    /// A Rust type.
    Type(TypeId),
    /// A component, resource or event type registered by name (C and
    /// C++), matched by the item's name.
    Named,
}

/// One declared component, resource or event type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Item {
    key: Key,
    name: &'static str,
    /// Written (components, resources) or sent (events).
    write: bool,
}

impl Item {
    fn same(&self, other: &Item) -> bool {
        match (self.key, other.key) {
            (Key::Named, Key::Named) => self.name == other.name,
            (a, b) => a == b,
        }
    }
}

fn item<T: 'static>(write: bool) -> Item {
    Item {
        key: Key::Type(TypeId::of::<T>()),
        name: std::any::type_name::<T>(),
        write,
    }
}

/// A name that lives as long as the program: declarations of C and C++
/// systems are made once, at registration.
fn named(name: &str, write: bool) -> Item {
    Item {
        key: Key::Named,
        name: Box::leak(name.to_owned().into_boxed_str()),
        write,
    }
}

/// Adds `it`, a write replacing a read of the same type.
fn add(items: &mut Vec<Item>, it: Item) {
    match items.iter_mut().find(|i| i.same(&it)) {
        Some(i) => i.write |= it.write,
        None => items.push(it),
    }
}

/// Adds event right `it`: sending and reading are separate rights, kept
/// as separate entries.
fn add_event(items: &mut Vec<Item>, it: Item) {
    if !items.iter().any(|i| i.same(&it) && i.write == it.write) {
        items.push(it);
    }
}

/// What a system declares it reads and writes.
///
/// Two declared systems in the same [`Phase`] conflict (and
/// keep their order) when either is exclusive, both touch the same
/// component, resource or [`Service`] and one writes it, one sends an event
/// type the other reads or both send it, or either records commands (any
/// system can see the world's structure the commands change). [`conflicts_before`](Self::conflicts_before)
/// has the finer rules for a known order. In builds with debug assertions,
/// each declared `Ctx` system runs with access recording on and panics on
/// any component, resource, event or command it used without declaring
/// it; a view system (one that takes `&mut SystemView`) can't reach
/// anything undeclared at all.
///
/// The one declaration type: attach it to a system of either kind with
/// [`IntoSystem::access`](crate::game::IntoSystem::access).
///
/// [`SystemAccess::new`] declares nothing; chain the builder methods to add
/// to it. Declaring a write also covers reading the same type.
///
/// ```
/// use ponko::prelude::*;
/// #[derive(Component)]
/// struct Pos(f32);
/// #[derive(Component)]
/// struct Vel(f32);
/// struct Gravity(f32);
///
/// let fall = ponko::access::SystemAccess::new().write::<Vel>().read_resource::<Gravity>();
/// let movement = ponko::access::SystemAccess::new().write::<Pos>().read::<Vel>();
/// let names: Vec<String> = fall.conflicts_with(&movement).iter().map(|c| c.to_string()).collect();
/// assert_eq!(names.len(), 1);
/// assert!(names[0].contains("Vel"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemAccess {
    exclusive: bool,
    components: Vec<Item>,
    resources: Vec<Item>,
    events: Vec<Item>,
    commands: bool,
    /// Declared services and whether each is written.
    services: Vec<(Service, bool)>,
    /// The typed components and resources, as the ECS view takes them.
    world: WorldDecls,
    /// For each typed event type read: how to share its kept events.
    event_items: Vec<EventItems>,
}

/// How to declare each typed component and resource on an ECS
/// [`ViewState`], in declaration order. It repeats what `components` and
/// `resources` list, so it takes no part in comparing declarations.
#[derive(Clone, Default)]
struct WorldDecls(Vec<fn(ViewState) -> ViewState>);

impl PartialEq for WorldDecls {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for WorldDecls {}

impl fmt::Debug for WorldDecls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} declared", self.0.len())
    }
}

/// How to get the kept events of one type as a value threads can share.
#[derive(Clone, Copy)]
struct EventItems {
    id: TypeId,
    items: SharedItems,
}

/// From [`Events::shared_items`](crate::events::Events::shared_items).
pub(crate) type SharedItems = for<'a> fn(&'a crate::events::Events) -> Option<&'a (dyn std::any::Any + Send + Sync)>;

impl PartialEq for EventItems {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for EventItems {}

impl fmt::Debug for EventItems {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.id.fmt(f)
    }
}

/// Why a view system can't share a parallel batch whatever its neighbours
/// declare: it runs alone, like a `Ctx` system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unbatchable {
    /// It declares a service that only one system at a time may hold.
    Service(Service),
    /// It sends and reads this event type: alone, it sees its own events
    /// at once; in a batch they would only arrive when the batch ends.
    SendsAndReads(&'static str),
}

impl SystemAccess {
    /// A declaration of nothing: such a system touches no components,
    /// resources, events or services and records no commands.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// May use anything; conflicts with every other system. What systems
    /// registered without a declaration get.
    #[must_use]
    pub fn exclusive() -> Self {
        SystemAccess {
            exclusive: true,
            ..Self::default()
        }
    }

    /// Reads component `T` (`&T` in a query, `world.get::<T>`).
    #[must_use]
    pub fn read<T: Component>(mut self) -> Self {
        add(&mut self.components, item::<T>(false));
        self.world.0.push(ViewState::read::<T>);
        self
    }

    /// Reads and writes component `T` (`&mut T` in a query, `world.get_mut::<T>`).
    #[must_use]
    pub fn write<T: Component>(mut self) -> Self {
        add(&mut self.components, item::<T>(true));
        self.world.0.push(ViewState::write::<T>);
        self
    }

    /// Reads resource `R`.
    #[must_use]
    pub fn read_resource<R: Send + Sync + 'static>(mut self) -> Self {
        add(&mut self.resources, item::<R>(false));
        self.world.0.push(ViewState::read_resource::<R>);
        self
    }

    /// Reads and writes resource `R`, including inserting or removing it.
    #[must_use]
    pub fn write_resource<R: Send + Sync + 'static>(mut self) -> Self {
        add(&mut self.resources, item::<R>(true));
        self.world.0.push(ViewState::write_resource::<R>);
        self
    }

    /// Reads `E` events ([`Events::read`](crate::events::Events::read) or `peek`).
    ///
    /// Declared event types are `Send + Sync`, like components and
    /// resources: a system in a parallel batch reads them from another
    /// thread. A system whose events aren't stays undeclared (exclusive).
    #[must_use]
    pub fn read_events<E: Send + Sync + 'static>(mut self) -> Self {
        add_event(&mut self.events, item::<E>(false));
        let id = TypeId::of::<E>();
        if !self.event_items.iter().any(|e| e.id == id) {
            self.event_items.push(EventItems {
                id,
                items: crate::events::Events::shared_items::<E>,
            });
        }
        self
    }

    /// Sends `E` events. Doesn't cover reading them; add
    /// [`read_events`](Self::read_events) for that.
    #[must_use]
    pub fn send_events<E: Send + Sync + 'static>(mut self) -> Self {
        add_event(&mut self.events, item::<E>(true));
        self
    }

    /// Records commands ([`Ctx::commands`](crate::ctx::Ctx::commands)), applied
    /// after the system.
    #[must_use]
    pub fn commands(mut self) -> Self {
        self.commands = true;
        self
    }

    /// Reads `service` (asks it questions without changing it).
    #[must_use]
    pub fn read_service(mut self, service: Service) -> Self {
        self.add_service(service, false);
        self
    }

    /// Reads and changes `service`.
    #[must_use]
    pub fn write_service(mut self, service: Service) -> Self {
        self.add_service(service, true);
        self
    }

    fn add_service(&mut self, service: Service, write: bool) {
        match self.services.iter_mut().find(|(s, _)| *s == service) {
            Some((_, w)) => *w |= write,
            None => self.services.push((service, write)),
        }
    }

    /// Reads the component registered under `name` (C and C++ components,
    /// or any component looked up by name). The raw C path can't tell reads
    /// from writes, so the debug check accepts any use of a component
    /// declared by name; declare [`write_named`](Self::write_named) if the
    /// system changes it, so conflicts are reported.
    ///
    /// Every `*_named` call keeps a copy of `name` for the rest of the
    /// program: declare at registration, not per frame.
    #[must_use]
    pub fn read_named(mut self, name: &str) -> Self {
        add(&mut self.components, named(name, false));
        self
    }

    /// Reads and writes the component registered under `name`.
    #[must_use]
    pub fn write_named(mut self, name: &str) -> Self {
        add(&mut self.components, named(name, true));
        self
    }

    /// Reads the resource stored under `name` without a Rust type (the C
    /// and C++ path). Such resources are reached through raw pointers, so
    /// the debug check can't tell reads from writes: declare
    /// [`write_resource_named`](Self::write_resource_named) if the system
    /// changes it.
    #[must_use]
    pub fn read_resource_named(mut self, name: &str) -> Self {
        add(&mut self.resources, named(name, false));
        self
    }

    /// Reads and writes the resource stored under `name` without a Rust type.
    #[must_use]
    pub fn write_resource_named(mut self, name: &str) -> Self {
        add(&mut self.resources, named(name, true));
        self
    }

    /// Reads events whose type is the component registered under `name`
    /// (C and C++ send events by component id).
    #[must_use]
    pub fn read_events_named(mut self, name: &str) -> Self {
        add_event(&mut self.events, named(name, false));
        self
    }

    /// Sends events whose type is the component registered under `name`.
    #[must_use]
    pub fn send_events_named(mut self, name: &str) -> Self {
        add_event(&mut self.events, named(name, true));
        self
    }

    /// `true` for [`exclusive`](Self::exclusive) access.
    #[must_use]
    pub fn is_exclusive(&self) -> bool {
        self.exclusive
    }

    /// `Some(written)` if `service` is declared, `None` if not.
    #[must_use]
    pub fn service(&self, service: Service) -> Option<bool> {
        self.services.iter().find(|(s, _)| *s == service).map(|&(_, w)| w)
    }

    /// `true` if the system records commands.
    #[must_use]
    pub fn records_commands(&self) -> bool {
        self.commands
    }

    /// The declaration as names, for [`EcsDiagnostics`](crate::diagnostics::EcsDiagnostics).
    pub(crate) fn describe(&self) -> crate::diagnostics::AccessDiagnostics {
        let names = |items: &[Item], write: bool| -> Vec<String> {
            (items.iter().filter(|i| i.write == write))
                .map(|i| i.name.to_string())
                .collect()
        };
        let services = |write: bool| -> Vec<String> {
            (self.services.iter().filter(|(_, w)| *w == write))
                .map(|(s, _)| s.name().to_string())
                .collect()
        };
        crate::diagnostics::AccessDiagnostics {
            exclusive: self.exclusive,
            components_read: names(&self.components, false),
            components_written: names(&self.components, true),
            resources_read: names(&self.resources, false),
            resources_written: names(&self.resources, true),
            events_read: names(&self.events, false),
            events_sent: names(&self.events, true),
            services_read: services(false),
            services_written: services(true),
            commands: self.commands,
        }
    }

    /// `true` if `E` events may be read.
    pub(crate) fn reads_events(&self, id: TypeId) -> bool {
        self.events.iter().any(|i| i.key == Key::Type(id) && !i.write)
    }

    /// `true` if `E` events may be sent.
    pub(crate) fn sends_events(&self, id: TypeId) -> bool {
        self.events.iter().any(|i| i.key == Key::Type(id) && i.write)
    }

    /// The typed event types this system reads, with how to share each.
    pub(crate) fn event_reads(&self) -> impl Iterator<Item = (TypeId, SharedItems)> + '_ {
        self.event_items.iter().map(|e| (e.id, e.items))
    }

    /// Why a view system with this declaration must run alone, if it must.
    ///
    /// In a batch a system gets only what can be shared or is its own: the
    /// world view, its command buffer, an outbox for its events, and the
    /// services a view only reads (input, time, random, the spatial grid).
    pub(crate) fn batch_problem(&self) -> Option<Unbatchable> {
        if let Some(&(s, _)) = self.services.iter().find(|(s, _)| s.in_view() != Some(false)) {
            return Some(Unbatchable::Service(s));
        }
        (self.events.iter())
            .find(|a| a.write && self.events.iter().any(|b| !b.write && a.same(b)))
            .map(|a| Unbatchable::SendsAndReads(a.name))
    }

    /// An ECS view state that declares the typed components and resources.
    pub(crate) fn view_state(&self) -> ViewState {
        self.world
            .0
            .iter()
            .fold(ViewState::new(), |state, declare| declare(state))
    }

    /// Why a view system can't have this declaration, if it can't.
    pub(crate) fn view_problem(&self) -> Option<String> {
        if self.exclusive {
            return Some("is exclusive; a system that may use anything takes `&mut Ctx`".into());
        }
        let untyped = |items: &[Item]| items.iter().find(|i| !matches!(i.key, Key::Type(_))).map(|i| i.name);
        if let Some(name) = untyped(&self.components)
            .or_else(|| untyped(&self.resources))
            .or_else(|| untyped(&self.events))
        {
            return Some(format!("declares `{name}`, which has no Rust type"));
        }
        for &(s, write) in &self.services {
            match s.in_view() {
                None => {
                    return Some(format!(
                        "declares service `{s}`, which a view doesn't offer; take `&mut Ctx` instead"
                    ));
                }
                Some(false) if write => {
                    return Some(format!(
                        "declares writing service `{s}`, which a view only reads; take `&mut Ctx` instead"
                    ));
                }
                _ => {}
            }
        }
        None
    }

    /// Why this system and `other` can't run together in either order;
    /// empty if they could. The conservative, symmetric rules: see
    /// [`conflicts_before`](Self::conflicts_before) for a known order.
    #[must_use]
    pub fn conflicts_with(&self, other: &SystemAccess) -> Vec<AccessConflict> {
        self.conflicts(other, false)
    }

    /// Why this system, registered (and so sequentially run) **before**
    /// `later` in the same phase, can't share a parallel batch with it;
    /// empty if it can.
    ///
    /// The finer barrier rules assume the batch contract a parallel
    /// executor must keep: commands recorded in a batch apply when the batch
    /// ends, in registration order, and events sent in it become visible
    /// when it ends, in registration order. Then, unlike
    /// [`conflicts_with`](Self::conflicts_with):
    ///
    /// - two systems that send the same event type don't conflict (their
    ///   events are published in the sequential order);
    /// - a reader before a sender doesn't conflict (sequentially it only
    ///   sees those events on its next run anyway); a sender before a
    ///   reader does;
    /// - a system that records commands conflicts with every **later**
    ///   system, which would see the applied commands sequentially (any
    ///   system can see the world's structure, declared or not); a system
    ///   before a recorder doesn't conflict with it.
    ///
    /// Components, resources and services conflict as in `conflicts_with`.
    ///
    /// ```
    /// use ponko::prelude::*;
    /// struct Hit;
    /// let send = ponko::access::SystemAccess::new().send_events::<Hit>();
    /// let read = ponko::access::SystemAccess::new().read_events::<Hit>();
    /// assert!(!read.conflicts_before(&send).is_empty() == false); // reader first: fine
    /// assert_eq!(send.conflicts_before(&read).len(), 1); // sender first: the reader would see it
    /// assert!(send.conflicts_before(&send.clone()).is_empty()); // published in order
    /// ```
    #[must_use]
    pub fn conflicts_before(&self, later: &SystemAccess) -> Vec<AccessConflict> {
        self.conflicts(later, true)
    }

    fn conflicts(&self, other: &SystemAccess, ordered: bool) -> Vec<AccessConflict> {
        if self.exclusive || other.exclusive {
            return vec![AccessConflict::Exclusive];
        }
        let mut out = Vec::new();
        let data = |items: &[Item], others: &[Item], out: &mut Vec<AccessConflict>, make: fn(&'static str) -> _| {
            for a in items {
                if others.iter().any(|b| a.same(b) && (a.write || b.write)) {
                    out.push(make(a.name));
                }
            }
        };
        data(&self.components, &other.components, &mut out, AccessConflict::Component);
        data(&self.resources, &other.resources, &mut out, AccessConflict::Resource);
        for a in &self.events {
            let clash = other.events.iter().any(|b| {
                a.same(b)
                    && if ordered {
                        // Only an earlier send seen by a later read.
                        a.write && !b.write
                    } else {
                        // Send/read or send/send of one type; read/read is fine.
                        a.write || b.write
                    }
            });
            let name = AccessConflict::Event(a.name);
            if clash && !out.contains(&name) {
                out.push(name);
            }
        }
        // Every system can see the world's structure (`len`, `is_alive`,
        // `has`, query filters) without declaring anything, so applied
        // commands are visible to any system that runs after them.
        let commands = if ordered {
            self.commands
        } else {
            self.commands || other.commands
        };
        if commands {
            out.push(AccessConflict::Commands);
        }
        for &(s, w) in &self.services {
            if other.services.iter().any(|&(t, v)| s == t && (w || v)) {
                out.push(AccessConflict::Service(s));
            }
        }
        out
    }

    /// What `used` shows this system did beyond its declaration, as
    /// readable lines; empty if it stayed within it.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub(crate) fn undeclared(&self, world: &World, used: &AccessLog, events: &EventUse, commands: bool) -> Vec<String> {
        if self.exclusive {
            return Vec::new();
        }
        let mut out = Vec::new();
        let covers = |items: &[Item], id: Option<TypeId>, name: &str, write: bool| {
            items.iter().any(|i| {
                match i.key {
                    Key::Type(t) => id == Some(t) && (i.write || !write),
                    // By name is the C and C++ path, whose raw pointers
                    // can't tell reads from writes: any declaration covers use.
                    Key::Named => i.name == name,
                }
            })
        };
        let verb = |write| if write { "writes" } else { "reads" };
        for &(c, write) in &used.components {
            let info = world.component_info(c);
            if !covers(&self.components, info.type_id(), info.name(), write) {
                out.push(format!("{} component `{}`", verb(write), info.name()));
            }
        }
        for &(id, name, write) in &used.resources {
            if !covers(&self.resources, Some(id), name, write) {
                out.push(format!("{} resource `{name}`", verb(write)));
            }
        }
        for name in &used.dynamic_resources {
            if !covers(&self.resources, None, name, false) {
                out.push(format!("uses resource `{name}`, which has no Rust type"));
            }
        }
        if used.structural {
            out.push("changes the world's structure directly (spawn, despawn, insert or remove); use commands".into());
        }
        for &(id, name, sent) in &events.used {
            if !self.events.iter().any(|i| i.key == Key::Type(id) && i.write == sent) {
                let what = if sent { "sends" } else { "reads" };
                out.push(format!("{what} events `{name}`"));
            }
        }
        for &(id, sent) in &events.raw {
            let name = world.component_at(id as usize).map(|c| world.component_info(c).name());
            let declared = name.is_some_and(|n| {
                self.events
                    .iter()
                    .any(|i| i.key == Key::Named && i.name == n && i.write == sent)
            });
            if !declared {
                let what = if sent { "sends" } else { "reads" };
                out.push(format!("{what} events `{}`", name.unwrap_or("of an unknown type")));
            }
        }
        if commands && !self.commands {
            out.push("records commands".into());
        }
        out
    }
}

/// Why two systems can't run together: from
/// [`SystemAccess::conflicts_with`] and [`AccessReport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccessConflict {
    /// At least one of them is exclusive (or undeclared).
    Exclusive,
    /// Both touch this component and at least one writes it.
    Component(&'static str),
    /// Both touch this resource and at least one writes it.
    Resource(&'static str),
    /// One sends this event type and the other reads or sends it.
    Event(&'static str),
    /// One records commands (with the ordered rules: the earlier one).
    Commands,
    /// Both use this service and at least one writes it.
    Service(Service),
    /// This system, registered between the two in the same phase, must run
    /// between them: it is undeclared (exclusive) or conflicts with one of
    /// them. Only in an [`AccessReport`].
    Between(&'static str),
}

impl fmt::Display for AccessConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccessConflict::Exclusive => f.write_str("exclusive access"),
            AccessConflict::Component(n) => write!(f, "component `{n}`"),
            AccessConflict::Resource(n) => write!(f, "resource `{n}`"),
            AccessConflict::Event(n) => write!(f, "events `{n}`"),
            AccessConflict::Commands => f.write_str("commands"),
            AccessConflict::Service(s) => write!(f, "service `{s}`"),
            AccessConflict::Between(n) => write!(f, "`{n}`, which runs between them"),
        }
    }
}

/// Event types a system used while its declaration was checked.
#[derive(Default)]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) struct EventUse {
    /// Type, name, sent (else read). One entry per (type, sent).
    pub(crate) used: Vec<(TypeId, &'static str, bool)>,
    /// Events without a Rust type (the C/C++ path): id, sent.
    pub(crate) raw: Vec<(u32, bool)>,
}

#[cfg_attr(not(debug_assertions), allow(dead_code))]
impl EventUse {
    pub(crate) fn note(&mut self, id: TypeId, name: &'static str, sent: bool) {
        if !self.used.iter().any(|&(i, _, s)| i == id && s == sent) {
            self.used.push((id, name, sent));
        }
    }

    pub(crate) fn note_raw(&mut self, id: u32, sent: bool) {
        if !self.raw.contains(&(id, sent)) {
            self.raw.push((id, sent));
        }
    }
}

/// Debug builds: starts recording what the system about to run uses, into
/// the buffers of the last check.
#[cfg(debug_assertions)]
pub(crate) fn begin_check(ctx: &mut Ctx) {
    let (log, mut events) = std::mem::take(&mut ctx.events.spare_logs);
    events.used.clear();
    events.raw.clear();
    ctx.world.record_access_into(log);
    *ctx.events.used.get_mut() = Some(events);
}

/// Debug builds: stops recording and panics if the system that just ran
/// (before its commands are applied) went beyond `access`.
#[cfg(debug_assertions)]
pub(crate) fn end_check(ctx: &mut Ctx, system: &str, access: &SystemAccess) {
    let used = ctx.world.take_access_log().unwrap_or_default();
    let events = ctx.events.used.get_mut().take().unwrap_or_default();
    let problems = access.undeclared(&ctx.world, &used, &events, !ctx.commands.is_empty());
    if !problems.is_empty() {
        panic!(
            "system `{system}` went beyond its declared access: it {}. Declare it in its SystemAccess, \
             or register the system without one (exclusive).",
            problems.join(", it ")
        );
    }
    // Keep the buffers for the next check.
    ctx.events.spare_logs = (used, events);
}

/// One registered system, as the report sees it.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReportedSystem {
    /// The registration's id: what [`Game::add_system`](crate::game::Game::add_system)
    /// returned for it, for [`Game::remove`](crate::game::Game::remove) and
    /// [`Game::order`](crate::game::Game::order).
    pub id: crate::game::SystemId,
    /// The system's name: its function path, or the name it was given
    /// ([`IntoSystem::named`](crate::game::IntoSystem::named)).
    pub name: &'static str,
    /// The phase it runs in.
    pub phase: Phase,
    /// Its declaration; `None` means undeclared, which counts as exclusive.
    pub access: Option<SystemAccess>,
    /// `true` for a view system (one that takes `&mut SystemView`): it can
    /// reach nothing beyond its declaration, so the scheduler may put it in
    /// a batch. Declared `&mut Ctx` systems are only checked in debug builds
    /// and must stay sequential.
    pub enforced: bool,
}

/// Two systems in the same phase and whether they conflict.
#[derive(Clone, Debug)]
pub struct ReportedPair {
    /// Index into [`AccessReport::systems`] of the earlier system.
    pub first: usize,
    /// Index of the later system (same phase).
    pub second: usize,
    /// Why they can't share a parallel batch, by the ordered rules of
    /// [`SystemAccess::conflicts_before`], plus [`AccessConflict::Between`]
    /// for a system between them that must stay there; empty if they could.
    pub conflicts: Vec<AccessConflict>,
}

/// Which declared systems could run together: from
/// [`Game::access_report`](crate::game::Game::access_report).
///
/// Lists every system and, for each pair in the same phase where both have
/// a declaration, its conflicts. Undeclared systems are exclusive and are
/// left out of the pairs, but one registered between two declared systems
/// keeps them apart ([`AccessConflict::Between`]). Purely informational:
/// [`Game::schedule_plan`](crate::game::Game::schedule_plan) shows what really
/// runs together.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct AccessReport {
    /// Every registered system, in run order.
    pub systems: Vec<ReportedSystem>,
    /// Pairs of declared systems in the same phase.
    pub pairs: Vec<ReportedPair>,
}

impl AccessReport {
    /// Builds the report for systems in run order.
    pub(crate) fn new(systems: Vec<ReportedSystem>) -> Self {
        let mut pairs = Vec::new();
        for (i, a) in systems.iter().enumerate() {
            let Some(aa) = &a.access else { continue };
            for (j, b) in systems.iter().enumerate().skip(i + 1) {
                if let Some(ba) = &b.access
                    && a.phase == b.phase
                {
                    let mut conflicts = aa.conflicts_before(ba);
                    // A system between them that can't move out of the way
                    // keeps them apart: batching would move one across it.
                    let between = systems[i + 1..j].iter().find(|k| {
                        k.phase == a.phase
                            && k.access.as_ref().is_none_or(|ka| {
                                !aa.conflicts_before(ka).is_empty() || !ka.conflicts_before(ba).is_empty()
                            })
                    });
                    if let Some(k) = between {
                        conflicts.push(AccessConflict::Between(k.name));
                    }
                    pairs.push(ReportedPair {
                        first: i,
                        second: j,
                        conflicts,
                    });
                }
            }
        }
        AccessReport { systems, pairs }
    }

    /// Names of the pairs whose declarations don't conflict.
    pub fn compatible_pairs(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.pairs
            .iter()
            .filter(|p| p.conflicts.is_empty())
            .map(|p| (self.systems[p.first].name, self.systems[p.second].name))
    }

    /// Names of the compatible pairs where both are view systems and every
    /// system registered between them in the phase is a view system too:
    /// the only ones a parallel executor may run together. (A declared
    /// `Ctx` system between them is a barrier: its declaration is only
    /// checked in debug builds, so it isn't trusted to move.)
    pub fn parallel_pairs(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        self.pairs
            .iter()
            .filter(|p| self.parallel(p))
            .map(|p| (self.systems[p.first].name, self.systems[p.second].name))
    }

    fn parallel(&self, p: &ReportedPair) -> bool {
        let phase = self.systems[p.first].phase;
        p.conflicts.is_empty()
            && self.systems[p.first..=p.second]
                .iter()
                .all(|s| s.enforced || s.phase != phase)
    }
}

impl fmt::Display for AccessReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for p in &self.pairs {
            let (a, b) = (self.systems[p.first].name, self.systems[p.second].name);
            if self.parallel(p) {
                writeln!(f, "{a} + {b}: could run together")?;
            } else if p.conflicts.is_empty() {
                writeln!(f, "{a} + {b}: no conflict, but a `Ctx` system stays sequential")?;
            } else {
                let why: Vec<String> = p.conflicts.iter().map(ToString::to_string).collect();
                writeln!(f, "{a} + {b}: conflict on {}", why.join(", "))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component)]
    struct Pos;
    struct Hit;

    #[test]
    fn a_write_replaces_a_read_of_the_same_type() {
        let a = SystemAccess::new().read::<Pos>().write::<Pos>().read::<Pos>();
        assert_eq!(a.components.len(), 1);
        assert!(a.components[0].write);
        let n = SystemAccess::new().read_named("pos").write_named("pos");
        assert_eq!(n.components.len(), 1);
        assert!(n.components[0].write);
    }

    #[test]
    fn sending_and_reading_one_event_type_are_two_rights() {
        let a = SystemAccess::new()
            .send_events::<Hit>()
            .read_events::<Hit>()
            .send_events::<Hit>();
        assert_eq!(a.events.len(), 2);
        let b = SystemAccess::new()
            .read_events::<Hit>()
            .send_events::<Hit>()
            .read_events::<Hit>();
        assert_eq!(b.events.len(), 2);
    }

    #[test]
    fn exclusive_declares_nothing_else_and_covers_everything() {
        let mut world = World::new();
        let id = world.register::<Pos>();
        let mut used = AccessLog::default();
        used.components.push((id, true));
        used.structural = true;
        let events = EventUse {
            raw: vec![(7, true)],
            ..EventUse::default()
        };
        assert!(
            SystemAccess::exclusive()
                .undeclared(&world, &used, &events, true)
                .is_empty()
        );
        assert!(!SystemAccess::new().undeclared(&world, &used, &events, true).is_empty());
    }
}
