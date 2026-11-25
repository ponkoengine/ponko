//! The restricted view a declared system gets instead of `&mut Ctx`:
//! [`SystemView`], for systems that take it ([`Game::add`](crate::game::Game::add)).
//!
//! **Maturity: tested**, from the guide chapter `system-access.md`. Game: none. Tests:
//! `tests/system_access.rs`, `tests/system_views.rs`. Platforms: Linux, macOS and Windows, headless
//! (the workspace tests in the list of checks). Graphics: not involved. Missing: a reference game
//! that uses it.
//!
//! A view system can reach only what its [`SystemAccess`] declares. The
//! world part is a [`WorldView`] (declared components and resources, no
//! structural changes). Events, commands and services are fields with the
//! names `Ctx` has (`sys.events`, `sys.commands`, `sys.input`, `sys.time`,
//! ...), each behind a handle that checks the declaration on every use, in
//! every build. There is no path from a `SystemView` to the rest of `Ctx`,
//! so a declaration can't be wrong in a way that goes unnoticed: what isn't
//! declared panics when it is used, naming the system.
//!
//! A view system runs in one of two ways, with the same results. Alone, it
//! borrows what it declared from `Ctx`. In a parallel batch
//! ([`Game::parallel_systems`](crate::game::Game::parallel_systems)) it shares the
//! world with the batch's other systems and gets its own command buffer and
//! an outbox for its events, both delivered when the batch ends.

use std::any::TypeId;

use glam::Vec2;
use ponko_ecs::{Commands, ViewState, WorldView};
use web_time::Instant;

use crate::access::Service;
use crate::access::SystemAccess;
use crate::events::{EventSlot, Events, Outbox};
use crate::{
    audio::{Playing, Sound},
    ctx::Ctx,
    input::Key,
    random::Random,
};

/// What a view system (a system that takes `&mut SystemView`, added with
/// [`Game::add`](crate::game::Game::add)) gets instead of `&mut Ctx`: exactly
/// what its [`SystemAccess`] declares.
///
/// - [`world`](Self::world): the declared components and resources (see
///   [`WorldView`]). Spawning, despawning, inserting and removing
///   components go through [`commands`](Self::commands).
/// - [`events`](Self::events): declared event types.
/// - Services, each a field named as on `Ctx` and behind its declaration
///   ([`ViewService`]): [`input`](Self::input) and [`time`](Self::time)
///   (read), [`audio`](Self::audio), the cameras, the
///   [`spatial`](Self::spatial) grid (read), the physics worlds and
///   lockstep networking ([`net.lockstep`](ViewNet::lockstep)). Drawing, UI, assets and profiling stay on `Ctx`
///   systems.
/// - The shortcuts `Ctx` has for those services: [`dt`](Self::dt),
///   [`key_down`](Self::key_down), [`key_pressed`](Self::key_pressed),
///   [`key_released`](Self::key_released), [`mouse_pos`](Self::mouse_pos),
///   [`play_sound`](Self::play_sound) and [`random`](Self::random).
///
/// So a system reads the same with either parameter:
/// `ctx.input.action_down("jump")` is `sys.input.action_down("jump")`.
/// Every use checks the declaration and panics, naming the system and what
/// is missing, if it isn't declared.
///
/// ```
/// use ponko::prelude::*;
/// #[derive(Component)]
/// struct Health(i32);
/// struct Died(Entity);
///
/// fn hurt(sys: &mut ponko::system_view::SystemView) {
///     let mut died = Vec::new();
///     sys.world.query::<(Entity, &mut Health)>().for_each(|(e, h)| {
///         h.0 -= 1;
///         if h.0 <= 0 {
///             died.push(e);
///         }
///     });
///     for e in died {
///         sys.events.send(Died(e));
///         sys.commands.despawn(e);
///     }
/// }
///
/// let world = Game::new("view", 1, 1)
///     .add(
///         Phase::AfterPhysics,
///         hurt.access(ponko::access::SystemAccess::new().write::<Health>().send_events::<Died>().commands()),
///     )
///     .run_headless(3, |ctx| {
///         ctx.world.spawn((Health(2),));
///     });
/// assert!(world.is_empty());
/// ```
pub struct SystemView<'a> {
    /// The declared components and resources.
    pub world: WorldView<'a>,
    /// Commands to apply after the system (declared with
    /// [`SystemAccess::commands`]): spawning, despawning, inserting and
    /// removing components. As `ctx.commands`.
    pub commands: ViewCommands<'a>,
    /// The declared event types. As `ctx.events`.
    pub events: ViewEvents<'a>,
    /// Keys, mouse, gamepads and named actions, as `ctx.input`. Read only
    /// (`Service::Input`).
    pub input: ViewService<'a, crate::input::Input>,
    /// The clocks, as `ctx.time`. Read only (`Service::Time`).
    pub time: ViewService<'a, crate::time::Time>,
    /// Sounds, buses and the listener, as `ctx.audio` (`Service::Audio`).
    pub audio: ViewService<'a, crate::audio::Audio>,
    /// The 2D camera, as `ctx.camera` (`Service::Camera`).
    pub camera: ViewService<'a, crate::d2::Camera>,
    /// The 3D camera, as `ctx.scene3d.camera` (feature `3d`, `Service::Camera`).
    #[cfg(feature = "3d")]
    pub camera3d: ViewService<'a, crate::d3::Camera>,
    /// The spatial grid, as `ctx.spatial`. Read only (`Service::Spatial`).
    pub spatial: ViewService<'a, crate::spatial::SpatialGrid>,
    /// The 2D physics world, as `ctx.physics` (feature `physics`,
    /// `Service::Physics`): ray casts and overlap queries read it,
    /// impulses and gravity write it.
    #[cfg(feature = "physics")]
    pub physics: ViewService<'a, crate::d2::Physics>,
    /// The 3D physics world, as `ctx.physics3d` (feature `physics3d`,
    /// `Service::Physics3D`).
    #[cfg(feature = "physics3d")]
    pub physics3d: ViewService<'a, crate::d3::Physics>,
    /// Networking, as `ctx.net`: `sys.net.lockstep` is
    /// `ctx.net.lockstep`.
    pub net: ViewNet<'a>,
    /// Debug drawing, as `ctx.debug`, with no declaration needed. Alone it
    /// is `ctx.debug`; in a parallel batch, a buffer of the system's own,
    /// added to `ctx.debug` when the batch ends.
    pub debug: &'a mut crate::debug::DebugDraw,
    name: &'static str,
    access: &'a SystemAccess,
    random: Random,
}

/// The networking of a [`SystemView`], named as on `ctx.net`
/// ([`Networking`](crate::net::Networking)): `sys.net.lockstep` is
/// `ctx.net.lockstep`. Only lockstep, the part a simulation system steps
/// with, is here; messages, snapshots and interest stay on `Ctx` systems.
#[non_exhaustive]
pub struct ViewNet<'a> {
    /// Lockstep networking, as `ctx.net.lockstep` (`Service::Net`).
    pub lockstep: ViewService<'a, crate::net::Net>,
}

/// One engine service of a [`SystemView`], behind the system's declaration.
///
/// It is used like the service itself, with the names `Ctx` has:
/// `sys.input.action_down("jump")`, `sys.time.sim_tick()`,
/// `sys.camera.zoom = 2.0`. Reading it needs the service declared
/// ([`SystemAccess::read_service`] or `write_service`); changing it needs
/// [`SystemAccess::write_service`]. Anything else panics on use, in every
/// build, naming the system and the missing declaration.
pub struct ViewService<'a, T> {
    system: &'static str,
    service: Service,
    /// `None` if the system doesn't declare the service, else whether it
    /// may write it.
    declared: Option<bool>,
    held: Held<'a, T>,
}

/// The service a [`ViewService`] stands for.
enum Held<'a, T> {
    /// One that views only read: shared by a parallel batch.
    Shared(&'a T),
    /// One that only one system at a time may hold: lent to a view system
    /// that runs alone.
    Lent(&'a mut T),
    /// In a parallel batch, a service that isn't shared. A system that
    /// declares one never joins a batch, so it is never reached.
    Absent,
}

impl<'a, T> ViewService<'a, T> {
    /// The handle of `service` for the system `of` names (its name and
    /// declaration).
    fn new((system, access, service): (&'static str, &SystemAccess, Service), held: Held<'a, T>) -> Self {
        ViewService {
            system,
            service,
            declared: access.service(service),
            held,
        }
    }

    #[track_caller]
    fn need(&self, write: bool) {
        need(self.system, self.service, self.declared, write);
    }
}

/// Panics unless a system that declared `service` as `declared` may read
/// it (or write it, for `write`).
#[track_caller]
fn need(system: &str, service: Service, declared: Option<bool>, write: bool) {
    if !declared.is_some_and(|w| w || !write) {
        let verb = if write { "write" } else { "read" };
        panic!(
            "system `{system}` doesn't declare that it may {verb} service `{service}`; add `{verb}_service(Service::{service:?})` to its SystemAccess"
        );
    }
}

impl<T> std::ops::Deref for ViewService<'_, T> {
    type Target = T;

    /// # Panics
    /// If the system doesn't declare the service.
    #[track_caller]
    fn deref(&self) -> &T {
        self.need(false);
        match &self.held {
            Held::Shared(service) => service,
            Held::Lent(service) => service,
            Held::Absent => unreachable!("a system that declares an exclusive service runs alone"),
        }
    }
}

impl<T> std::ops::DerefMut for ViewService<'_, T> {
    /// # Panics
    /// If the system doesn't declare that it writes the service.
    #[track_caller]
    fn deref_mut(&mut self) -> &mut T {
        self.need(true);
        match &mut self.held {
            Held::Lent(service) => service,
            // Refused at registration: `SystemAccess::view_problem`.
            Held::Shared(_) => unreachable!("a view system can't declare writing a service that views only read"),
            Held::Absent => unreachable!("a system that declares an exclusive service runs alone"),
        }
    }
}

/// The command buffer of a view system: [`SystemView::commands`]. Used like
/// [`Commands`]; any use panics unless the system declares
/// [`SystemAccess::commands`].
pub struct ViewCommands<'a> {
    system: &'static str,
    declared: bool,
    commands: &'a mut Commands,
}

impl ViewCommands<'_> {
    #[track_caller]
    fn need(&self) {
        if !self.declared {
            panic!(
                "system `{}` doesn't declare that it records commands; add `.commands()` to its SystemAccess",
                self.system
            );
        }
    }
}

impl std::ops::Deref for ViewCommands<'_> {
    type Target = Commands;

    /// # Panics
    /// If the system doesn't declare that it records commands.
    #[track_caller]
    fn deref(&self) -> &Commands {
        self.need();
        self.commands
    }
}

impl std::ops::DerefMut for ViewCommands<'_> {
    /// # Panics
    /// If the system doesn't declare that it records commands.
    #[track_caller]
    fn deref_mut(&mut self) -> &mut Commands {
        self.need();
        self.commands
    }
}

/// Where a view system's events come from and go to.
enum Port<'a> {
    /// Alone: the game's event store, directly.
    Direct(&'a mut Events),
    /// In a batch: the kept events of its declared types, shared and
    /// read-only; the types it has read in this run; its outbox.
    Batch {
        slots: &'a [EventSlot<'a>],
        read: &'a mut Vec<TypeId>,
        outbox: &'a mut Outbox,
    },
}

impl SystemView<'_> {
    /// The system's name (its function path).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The system's declaration.
    #[must_use]
    pub fn access(&self) -> &SystemAccess {
        self.access
    }

    // The shortcuts `Ctx` has, for the services a view offers.

    /// As [`Ctx::dt`]: the fixed step (times the interval) in simulation
    /// phases, the frame time in frame phases. Needs `Service::Time`.
    #[track_caller]
    #[must_use]
    pub fn dt(&self) -> f32 {
        self.time.dt()
    }

    /// As [`Ctx::random`]: this system's stream for this tick. Needs
    /// `Service::Random`.
    #[track_caller]
    #[must_use]
    pub fn random(&self) -> Random {
        need(self.name, Service::Random, self.access.service(Service::Random), false);
        self.random
    }

    /// As [`Ctx::key_down`]. Needs `Service::Input`.
    #[track_caller]
    #[must_use]
    pub fn key_down(&self, key: Key) -> bool {
        self.input.key_down(key)
    }

    /// As [`Ctx::key_pressed`]. Needs `Service::Input`.
    #[track_caller]
    #[must_use]
    pub fn key_pressed(&self, key: Key) -> bool {
        self.input.key_pressed(key)
    }

    /// As [`Ctx::key_released`]. Needs `Service::Input`.
    #[track_caller]
    #[must_use]
    pub fn key_released(&self, key: Key) -> bool {
        self.input.key_released(key)
    }

    /// As [`Ctx::mouse_pos`]. Needs `Service::Input`.
    #[track_caller]
    #[must_use]
    pub fn mouse_pos(&self) -> Vec2 {
        self.input.mouse_pos()
    }

    /// As [`Ctx::play_sound`]. Needs `Service::Audio` written.
    #[track_caller]
    pub fn play_sound(&mut self, sound: Sound) -> Playing {
        self.audio.play(sound)
    }
}

/// The declared event types of a view system: [`SystemView::events`].
///
/// Same visibility rules as [`Events`]: see the guide's "Systems and the
/// frame" chapter. In a parallel batch, sent events reach the store when
/// the batch ends, in registration order; no system of the batch could have
/// seen them earlier ([`SystemAccess::conflicts_before`]).
///
/// Event types are `Send + Sync`: a batch reads them from several threads.
pub struct ViewEvents<'a> {
    name: &'static str,
    access: &'a SystemAccess,
    port: Port<'a>,
}

impl ViewEvents<'_> {
    #[track_caller]
    fn need<E: 'static>(&self, sent: bool) {
        let id = TypeId::of::<E>();
        let ok = if sent {
            self.access.sends_events(id)
        } else {
            self.access.reads_events(id)
        };
        if !ok {
            let (verb, method) = if sent {
                ("send", "send_events")
            } else {
                ("read", "read_events")
            };
            panic!(
                "system `{}` doesn't declare that it may {verb} events `{}`; add `.{method}::<...>()` to its SystemAccess",
                self.name,
                std::any::type_name::<E>()
            );
        }
    }

    /// As [`Events::send`]. Needs [`SystemAccess::send_events`].
    #[track_caller]
    pub fn send<E: Send + Sync + 'static>(&mut self, event: E) {
        self.need::<E>(true);
        match &mut self.port {
            Port::Direct(events) => events.send(event),
            Port::Batch { outbox, .. } => outbox.send(event),
        }
    }

    /// As [`Events::read`]. Needs [`SystemAccess::read_events`].
    #[track_caller]
    pub fn read<E: Send + Sync + 'static>(&mut self) -> &[E] {
        self.need::<E>(false);
        match &mut self.port {
            Port::Direct(events) => events.read(),
            Port::Batch { slots, read, .. } => {
                let id = TypeId::of::<E>();
                // A second read in one run finds nothing new, as alone.
                if read.contains(&id) {
                    return &[];
                }
                read.push(id);
                match slots.iter().find(|s| s.id == id) {
                    Some(slot) => &items::<E>(slot)[slot.unread.clone()],
                    None => &[],
                }
            }
        }
    }

    /// As [`Events::peek`]. Needs [`SystemAccess::read_events`].
    #[track_caller]
    #[must_use]
    pub fn peek<E: Send + Sync + 'static>(&self) -> &[E] {
        self.need::<E>(false);
        match &self.port {
            Port::Direct(events) => events.peek(),
            Port::Batch { slots, .. } => match slots.iter().find(|s| s.id == TypeId::of::<E>()) {
                Some(slot) => items::<E>(slot),
                None => &[],
            },
        }
    }
}

/// The kept `E` events behind a batch's slot.
fn items<'a, E: 'static>(slot: &EventSlot<'a>) -> &'a [E] {
    slot.items.downcast_ref::<Vec<E>>().expect("event slot type")
}

/// A boxed view system.
pub(crate) type ViewFn = Box<dyn FnMut(&mut SystemView<'_>) + Send>;

/// A view system as the game keeps it: its view state, and everything else
/// it needs to run ([`ViewTask`]). Two parts, because in a batch the state
/// lends the world view to the task (`ViewGroup::run`).
pub(crate) struct ViewRunner {
    pub(crate) state: ViewState,
    pub(crate) task: ViewTask,
}

/// The part of a view system that runs: its function and declaration, and
/// what a batch gives it of its own.
pub(crate) struct ViewTask {
    name: &'static str,
    access: SystemAccess,
    f: ViewFn,
    /// In a batch: its own commands, applied when the batch ends.
    pub(crate) commands: Commands,
    /// In a batch: what it drew with `sys.debug`, added to `ctx.debug`
    /// when the batch ends.
    pub(crate) debug: crate::debug::DebugDraw,
    /// In a batch: the events it sent, delivered when the batch ends.
    pub(crate) outbox: Outbox,
    /// In a batch: the event types it read in this run.
    pub(crate) read: Vec<TypeId>,
    /// In a batch: its part of the batch's event slots.
    pub(crate) slots: std::ops::Range<usize>,
    /// In a batch: its random stream, taken before it starts.
    pub(crate) random: Random,
    /// In a batch: how long its last run took, and the allocations made
    /// meanwhile (only counted when the batch runs in order).
    pub(crate) last_ms: f32,
    pub(crate) allocs: u64,
    /// Its last run in a batch returned (it didn't panic).
    pub(crate) finished: bool,
    /// In a batch: a run condition doesn't hold, so this run does nothing.
    pub(crate) skip: bool,
    /// Its usual run time in microseconds (a moving average), which decides
    /// whether a batch is worth sending to other threads. 0 before it ran.
    pub(crate) usual_us: f32,
}

/// What every system of a batch shares, read-only.
pub(crate) struct Shared<'a> {
    pub(crate) slots: &'a [EventSlot<'a>],
    pub(crate) input: &'a crate::input::Input,
    pub(crate) time: &'a crate::time::Time,
    pub(crate) spatial: &'a crate::spatial::SpatialGrid,
    /// Count each system's allocations: only exact when they run in order.
    pub(crate) count_allocs: bool,
}

impl ViewRunner {
    pub(crate) fn new(name: &'static str, access: SystemAccess, f: ViewFn) -> Self {
        // Made once per registered system, for panic messages.
        let label: &'static str = Box::leak(format!("system `{name}`").into_boxed_str());
        ViewRunner {
            state: access.view_state().labeled(label),
            task: ViewTask {
                name,
                access,
                f,
                commands: Commands::new(),
                debug: crate::debug::DebugDraw::new(false),
                outbox: Outbox::default(),
                read: Vec::new(),
                slots: 0..0,
                random: Random::new(0, 0, 0),
                last_ms: 0.0,
                allocs: 0,
                finished: false,
                skip: false,
                usual_us: 0.0,
            },
        }
    }

    /// The declaration.
    pub(crate) fn access(&self) -> &SystemAccess {
        &self.task.access
    }

    /// Runs the system alone: lends the declared written resources out of
    /// the world, builds the view from separate `Ctx` fields, runs it and
    /// puts the resources back (also if it panics). Its commands go to
    /// `ctx.commands` and its events straight to `ctx.events`, like any
    /// system's.
    pub(crate) fn run_alone(&mut self, ctx: &mut Ctx) {
        let random = ctx.random();
        let ViewTask { name, access, f, .. } = &mut self.task;
        let (name, access): (&'static str, &SystemAccess) = (name, access);
        let rest = Rest {
            commands: &mut ctx.commands,
            events: Port::Direct(&mut ctx.events),
            input: &ctx.input,
            time: &ctx.time,
            spatial: &ctx.spatial,
            random,
            audio: Held::Lent(&mut ctx.audio),
            camera: Held::Lent(&mut ctx.camera),
            #[cfg(feature = "3d")]
            camera3d: Held::Lent(&mut ctx.scene3d.camera),
            #[cfg(feature = "physics")]
            physics: Held::Lent(&mut ctx.physics),
            #[cfg(feature = "physics3d")]
            physics3d: Held::Lent(&mut ctx.physics3d),
            net: Held::Lent(&mut ctx.net.lockstep),
            debug: &mut ctx.debug,
        };
        // The world's own change window, which the game set for this system.
        self.state.override_changes_since(None);
        self.state.override_change_tick(None);
        self.state.run(&mut ctx.world, |world| {
            f(&mut rest.view(world, name, access));
        });
    }
}

/// Everything of a [`SystemView`] but its world, name and declaration.
struct Rest<'a> {
    commands: &'a mut Commands,
    events: Port<'a>,
    input: &'a crate::input::Input,
    time: &'a crate::time::Time,
    spatial: &'a crate::spatial::SpatialGrid,
    random: Random,
    audio: Held<'a, crate::audio::Audio>,
    camera: Held<'a, crate::d2::Camera>,
    #[cfg(feature = "3d")]
    camera3d: Held<'a, crate::d3::Camera>,
    #[cfg(feature = "physics")]
    physics: Held<'a, crate::d2::Physics>,
    #[cfg(feature = "physics3d")]
    physics3d: Held<'a, crate::d3::Physics>,
    net: Held<'a, crate::net::Net>,
    debug: &'a mut crate::debug::DebugDraw,
}

impl<'a> Rest<'a> {
    /// What a parallel batch gives a system: its own commands, events and
    /// random stream, the shared read-only services, and no others.
    fn shared(
        commands: &'a mut Commands,
        events: Port<'a>,
        random: Random,
        shared: &Shared<'a>,
        debug: &'a mut crate::debug::DebugDraw,
    ) -> Self {
        Rest {
            commands,
            events,
            input: shared.input,
            time: shared.time,
            spatial: shared.spatial,
            random,
            audio: Held::Absent,
            camera: Held::Absent,
            #[cfg(feature = "3d")]
            camera3d: Held::Absent,
            #[cfg(feature = "physics")]
            physics: Held::Absent,
            #[cfg(feature = "physics3d")]
            physics3d: Held::Absent,
            net: Held::Absent,
            debug,
        }
    }

    /// The view of system `name`, which declares `access`.
    fn view(self, world: WorldView<'a>, name: &'static str, access: &'a SystemAccess) -> SystemView<'a> {
        let of = |service| (name, access, service);
        SystemView {
            world,
            commands: ViewCommands {
                system: name,
                declared: access.records_commands(),
                commands: self.commands,
            },
            events: ViewEvents {
                name,
                access,
                port: self.events,
            },
            input: ViewService::new(of(Service::Input), Held::Shared(self.input)),
            time: ViewService::new(of(Service::Time), Held::Shared(self.time)),
            audio: ViewService::new(of(Service::Audio), self.audio),
            camera: ViewService::new(of(Service::Camera), self.camera),
            #[cfg(feature = "3d")]
            camera3d: ViewService::new(of(Service::Camera), self.camera3d),
            spatial: ViewService::new(of(Service::Spatial), Held::Shared(self.spatial)),
            #[cfg(feature = "physics")]
            physics: ViewService::new(of(Service::Physics), self.physics),
            #[cfg(feature = "physics3d")]
            physics3d: ViewService::new(of(Service::Physics3D), self.physics3d),
            net: ViewNet {
                lockstep: ViewService::new(of(Service::Net), self.net),
            },
            debug: self.debug,
            name,
            access,
            random: self.random,
        }
    }
}

impl ViewTask {
    /// Runs the system in a batch, on any thread: `world` is its view of
    /// the shared world. Everything else is its own or read-only.
    pub(crate) fn run_shared(&mut self, world: WorldView<'_>, shared: &Shared<'_>) {
        #[cfg(feature = "profiling")]
        puffin::profile_scope!("system", self.name);
        self.read.clear();
        if self.skip {
            self.finished = true;
            return;
        }
        // The thread it runs on, as for a system run alone.
        let allocs_before = shared.count_allocs.then(crate::perf::thread_alloc_counts);
        let start = Instant::now();
        self.finished = false;
        let events = Port::Batch {
            slots: &shared.slots[self.slots.clone()],
            read: &mut self.read,
            outbox: &mut self.outbox,
        };
        let mut view = Rest::shared(&mut self.commands, events, self.random, shared, &mut self.debug).view(
            world,
            self.name,
            &self.access,
        );
        (self.f)(&mut view);
        self.finished = true;
        self.last_ms = start.elapsed().as_secs_f32() * 1000.0;
        self.allocs = allocs_before.map_or(0, |before| {
            crate::perf::thread_alloc_counts().count.saturating_sub(before.count)
        });
    }
}
